//! Telemetry initialization for the gateway HTTP service.
//!
//! Builds an OpenTelemetry [`SdkTracerProvider`] with an OTLP span exporter and
//! installs a `tracing-opentelemetry` layer into the global tracing subscriber.
//! Export failures are logged to stderr and never fatal.
//!
//! Spans are exported through a [`BatchSpanProcessor`] (via
//! `with_batch_exporter`), which runs exports on a background thread. This keeps
//! the synchronous OTLP export off the async runtime -- the OTLP HTTP exporter
//! performs blocking I/O and must not be invoked from within an asynchronous
//! context.
//!
//! Logs are emitted as newline-delimited JSON to stdout. Every log event carries
//! `service.name`, `service.version` and `environment` (from `RENZU_ENVIRONMENT`,
//! default `local`) so they can be queried and correlated without a log backend.
//! `RENZU_LOG_LEVEL` controls verbosity via [`EnvFilter`].
//!
//! Sensitive values can be redacted at the call site with [`Redacted`]:
//! `tracing::info!(auth = %Redacted(&token), "authenticating")` renders
//! `[REDACTED]`. Redaction is opt-in; values logged without `Redacted` are
//! emitted as-is.
//!
//! Best-effort `trace_id`/`span_id` correlation is noted as a gap: tracing
//! event fields are immutable at creation time, and the OpenTelemetry trace
//! context is not exposed as tracing span fields at M1, so it cannot be
//! injected into log events here. It is recorded on spans (F-006) and the
//! fields can be added to log output once spans declare them.

use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use std::fmt::{self};
use std::sync::OnceLock;
use std::time::Duration;
use tracing::Subscriber;
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, JsonFields, Writer};
use tracing_subscriber::fmt::time::{FormatTime, SystemTime as SystemTimeTimer};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

const SERVICE_NAME: &str = "gateway";
const DEFAULT_ENVIRONMENT: &str = "local";
const DEFAULT_OTLP_ENDPOINT: &str = "http://127.0.0.1:4317/v1/traces";
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Runtime environment, read from `RENZU_ENVIRONMENT` (default `local`).
///
/// Set once during [`Telemetry::init`] and read by the JSON event formatter for
/// every log event.
static ENVIRONMENT: OnceLock<String> = OnceLock::new();

/// Handle to an initialized OpenTelemetry tracer provider.
///
/// Shutting down the handle flushes in-flight spans to the OTLP endpoint
/// before the process exits.
pub struct Telemetry {
    provider: SdkTracerProvider,
}

/// A value wrapper that redacts sensitive information from log output.
///
/// Wrap a value in `Redacted` at a log call site so it is rendered as
/// `[REDACTED]` instead of the raw value:
///
/// ```text
/// tracing::info!(auth = %Redacted(&token), "authenticating");
/// ```
///
/// Redaction is call-site opt-in: values logged without `Redacted` are emitted
/// normally. Use it for `Authorization`, `Cookie`, `password`, `token`,
/// `secret`, `api_key` and similar fields.
pub struct Redacted<T>(pub T);

impl<T> fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// JSON event formatter that injects `service.name`, `service.version` and
/// `environment` into every log event.
pub struct ServiceFields;

impl<S, N> FormatEvent<S, N> for ServiceFields
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> fmt::Result {
        let mut writer = writer;
        let meta = event.metadata();

        let mut timestamp = String::new();
        SystemTimeTimer.format_time(&mut Writer::new(&mut timestamp))?;

        write!(writer, "{{")?;
        let mut first = true;
        write_str_field(&mut writer, &mut first, "timestamp", &timestamp)?;
        write_str_field(&mut writer, &mut first, "level", meta.level().as_str())?;
        write_str_field(&mut writer, &mut first, "service.name", SERVICE_NAME)?;
        write_str_field(
            &mut writer,
            &mut first,
            "service.version",
            env!("CARGO_PKG_VERSION"),
        )?;
        write_str_field(&mut writer, &mut first, "environment", environment())?;
        write_str_field(&mut writer, &mut first, "target", meta.target())?;

        // Event fields, already JSON-encoded as an object by `JsonFields`.
        let mut fields_buf = String::new();
        FormatFields::format_fields(&JsonFields::new(), Writer::new(&mut fields_buf), event)?;
        if !first {
            write!(writer, ",")?;
        }
        write!(writer, "\"fields\":")?;
        write!(writer, "{}", fields_buf)?;
        write!(writer, "}}")?;
        writeln!(writer)
    }
}

/// Write a single `"key":<json>` field, prefixing a comma when it is not the
/// first field in the object. `value` is JSON-escaped via `serde_json`.
fn write_str_field(
    writer: &mut Writer<'_>,
    first: &mut bool,
    key: &str,
    value: &str,
) -> fmt::Result {
    if !*first {
        write!(writer, ",")?;
    }
    *first = false;
    write!(
        writer,
        "\"{}\":{}",
        key,
        // `serde_json::to_string` on a `&str` cannot fail; it only produces a
        // quoted, escaped JSON string.
        serde_json::to_string(value).expect("serializing a &str to JSON cannot fail")
    )
}

/// Return the `RENZU_ENVIRONMENT` value, defaulting to [`DEFAULT_ENVIRONMENT`].
fn environment() -> &'static str {
    ENVIRONMENT.get_or_init(|| {
        std::env::var("RENZU_ENVIRONMENT").unwrap_or_else(|_| DEFAULT_ENVIRONMENT.to_string())
    })
}

impl Telemetry {
    /// Initialize OpenTelemetry tracing and install the global subscriber.
    ///
    /// The OTLP endpoint is taken from `RENZU_OTLP_ENDPOINT` (default
    /// [`DEFAULT_OTLP_ENDPOINT`]). The environment is taken from
    /// `RENZU_ENVIRONMENT` (default [`DEFAULT_ENVIRONMENT`]). Resource
    /// attributes `service.name` and `service.version` are recorded on every
    /// span.
    ///
    /// A JSON formatting layer and the OpenTelemetry layer are installed as the
    /// global tracing subscriber using `log_level`. Export failures are logged,
    /// never fatal.
    pub fn init(log_level: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = std::env::var("RENZU_OTLP_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_OTLP_ENDPOINT.to_string());

        let environment =
            std::env::var("RENZU_ENVIRONMENT").unwrap_or_else(|_| DEFAULT_ENVIRONMENT.to_string());
        let _ = ENVIRONMENT.set(environment.clone());

        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(&endpoint)
            .build()
            .map_err(|e| format!("failed to build OTLP exporter: {e}"))?;

        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(
                Resource::builder()
                    .with_attributes(vec![
                        KeyValue::new("service.name", SERVICE_NAME),
                        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                    ])
                    .build(),
            )
            .build();

        let tracer = provider.tracer(SERVICE_NAME);
        let otel_layer = tracing_opentelemetry::OpenTelemetryLayer::new(tracer);

        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));
        let fmt_layer = tracing_subscriber::fmt::layer()
            .json()
            .event_format(ServiceFields)
            .with_filter(env_filter);

        tracing_subscriber::registry()
            .with(fmt_layer)
            .with(otel_layer)
            .try_init()
            .map_err(|e| format!("failed to install tracing subscriber: {e}"))?;

        tracing::info!(endpoint = %endpoint, "telemetry initialized");

        Ok(Self { provider })
    }

    /// Force a flush of all queued spans to the OTLP endpoint.
    ///
    /// Waits for the batch processor to drain its queue. Errors are logged,
    /// never fatal.
    pub fn force_flush(&self) {
        if let Err(e) = self.provider.force_flush() {
            tracing::warn!(error = %e, "telemetry force flush failed");
        }
    }

    /// Shut down the tracer provider, flushing in-flight spans.
    ///
    /// Waits up to [`SHUTDOWN_TIMEOUT`] for export to complete. Timeout or
    /// export errors are logged, never fatal.
    pub fn shutdown(self) {
        match self.provider.shutdown_with_timeout(SHUTDOWN_TIMEOUT) {
            Ok(()) => tracing::debug!("telemetry shutdown complete"),
            Err(e) => tracing::warn!(error = %e, "telemetry shutdown failed"),
        }
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        let _ = self.provider.shutdown_with_timeout(SHUTDOWN_TIMEOUT);
    }
}
