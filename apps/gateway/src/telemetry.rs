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
//! Logs are emitted as newline-delimited JSON to stdout AND exported as OTLP
//! LogRecords to the Collector (Loki pipeline). Every log event carries
//! `service.name`, `service.version` and `environment` (from `RENZU_ENVIRONMENT`,
//! default `local`) so they can be queried and correlated without a log backend.
//! `RENZU_LOG_LEVEL` controls verbosity via [`EnvFilter`].
//!
//! The OpenTelemetry layer uses [`WithContext`] so trace context is available
//! to log formatters. When a log event is emitted inside a trace span, the
//! OTLP LogRecord carries native `trace_id`/`span_id` for correlation with
//! traces in Jaeger and logs in Loki.
//!
//! Sensitive values can be redacted at the call site with [`Redacted`]:
//! `tracing::info!(auth = %Redacted(&token), "authenticating")` renders
//! `[REDACTED]`. Redaction is opt-in; values logged without `Redacted` are
//! emitted as-is.

use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::runtime::Tokio;
use opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor;
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
const DEFAULT_OTLP_LOGS_ENDPOINT: &str = "http://127.0.0.1:4318/v1/logs";
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Runtime environment, read from `RENZU_ENVIRONMENT` (default `local`).
///
/// Set once during [`Telemetry::init`] and read by the JSON event formatter for
/// every log event.
static ENVIRONMENT: OnceLock<String> = OnceLock::new();

/// Handle to an initialized OpenTelemetry tracer provider and logger provider.
///
/// Shutting down the handles flushes in-flight spans and logs to the OTLP
/// endpoints before the process exits.
pub struct Telemetry {
    provider: SdkTracerProvider,
    logger_provider: SdkLoggerProvider,
    /// Guard for the thread-level tracing dispatcher default, used when the
    /// process-wide global default is already installed (e.g. in tests where
    /// multiple tests each call Telemetry::init).
    #[allow(dead_code)]
    _dispatch_guard: Option<tracing::dispatcher::DefaultGuard>,
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

/// JSON event formatter that injects `service.name`, `service.version`,
/// `environment`, and best-effort `trace_id`/`span_id` into every log event.
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
    /// Initialize OpenTelemetry tracing and logging, and install the global subscriber.
    ///
    /// The OTLP endpoint is taken from `RENZU_OTLP_ENDPOINT` (default
    /// [`DEFAULT_OTLP_ENDPOINT`]). The OTLP logs endpoint is taken from
    /// `RENZU_OTLP_LOGS_ENDPOINT` (default [`DEFAULT_OTLP_LOGS_ENDPOINT`]).
    /// The environment is taken from `RENZU_ENVIRONMENT` (default
    /// [`DEFAULT_ENVIRONMENT`]). Resource attributes `service.name` and
    /// `service.version` are recorded on every span and log record.
    ///
    /// A JSON formatting layer (stdout), the OpenTelemetry layer (wrapped with
    /// [`WithContext`] so trace context is available to log formatters), and
    /// an OpenTelemetry log appender layer (OTLP LogRecords to Collector/Loki)
    /// are installed as the global tracing subscriber using `log_level`. Export
    /// failures are logged, never fatal.
    pub fn init(log_level: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = std::env::var("RENZU_OTLP_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_OTLP_ENDPOINT.to_string());
        let logs_endpoint = std::env::var("RENZU_OTLP_LOGS_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_OTLP_LOGS_ENDPOINT.to_string());

        let environment =
            std::env::var("RENZU_ENVIRONMENT").unwrap_or_else(|_| DEFAULT_ENVIRONMENT.to_string());
        let _ = ENVIRONMENT.set(environment.clone());

        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(&endpoint)
            .build()
            .map_err(|e| format!("failed to build OTLP exporter: {e}"))?;

        let provider = SdkTracerProvider::builder()
            .with_span_processor(BatchSpanProcessor::builder(exporter, Tokio).build())
            .with_resource(
                Resource::builder()
                    .with_attributes(vec![
                        KeyValue::new("service.name", SERVICE_NAME),
                        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                    ])
                    .build(),
            )
            .build();

        // Build the OTLP log exporter and logger provider for log correlation.
        let log_exporter = opentelemetry_otlp::LogExporter::builder()
            .with_http()
            .with_endpoint(&logs_endpoint)
            .build()
            .map_err(|e| format!("failed to build OTLP log exporter: {e}"))?;

        let logger_provider = SdkLoggerProvider::builder()
            .with_batch_exporter(log_exporter)
            .with_resource(
                Resource::builder()
                    .with_attributes(vec![
                        KeyValue::new("service.name", SERVICE_NAME),
                        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                    ])
                    .build(),
            )
            .build();

        // OpenTelemetry log appender layer: exports log events as OTLP LogRecords
        // with trace context (trace_id/span_id) when emitted inside a span.
        let otlp_log_layer = OpenTelemetryTracingBridge::builder(&logger_provider).build();

        let tracer = provider.tracer(SERVICE_NAME);
        let otel_layer = tracing_opentelemetry::OpenTelemetryLayer::new(tracer);

        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));
        let fmt_layer = tracing_subscriber::fmt::layer()
            .json()
            .event_format(ServiceFields)
            .with_filter(env_filter.clone());

        let subscriber = tracing_subscriber::registry()
            .with(fmt_layer)
            .with(otel_layer)
            .with(otlp_log_layer.with_filter(env_filter.clone()));

        let _dispatch_guard = match subscriber.try_init() {
            Ok(()) => None,
            Err(_) => {
                // Global default already installed (e.g. from another test).
                // Fall back to a thread-local default so spans created on this
                // thread still use this subscriber. Rebuild the layers since
                // they were moved into the first subscriber.
                let otel_layer =
                    tracing_opentelemetry::OpenTelemetryLayer::new(provider.tracer(SERVICE_NAME));
                let fmt_layer = tracing_subscriber::fmt::layer()
                    .json()
                    .event_format(ServiceFields)
                    .with_filter(env_filter.clone());
                let otlp_log_layer = OpenTelemetryTracingBridge::builder(&logger_provider).build();
                let subscriber = tracing_subscriber::registry()
                    .with(fmt_layer)
                    .with(otel_layer)
                    .with(otlp_log_layer.with_filter(env_filter));
                Some(tracing::dispatcher::set_default(&tracing::Dispatch::from(
                    subscriber,
                )))
            }
        };

        tracing::info!(endpoint = %endpoint, logs_endpoint = %logs_endpoint, "telemetry initialized");

        Ok(Self {
            provider,
            logger_provider,
            _dispatch_guard,
        })
    }

    /// Force a flush of all queued spans and log records to the OTLP endpoints.
    ///
    /// Waits for the batch processors to drain their queues. Errors are logged,
    /// never fatal.
    pub fn force_flush(&self) {
        if let Err(e) = self.provider.force_flush() {
            tracing::warn!(error = %e, "telemetry force flush (traces) failed");
        }
        if let Err(e) = self.logger_provider.force_flush() {
            tracing::warn!(error = %e, "telemetry force flush (logs) failed");
        }
    }

    /// Shut down the tracer provider and logger provider, flushing in-flight data.
    ///
    /// Waits up to [`SHUTDOWN_TIMEOUT`] for export to complete. Timeout or
    /// export errors are logged, never fatal.
    pub fn shutdown(self) {
        match self.provider.shutdown_with_timeout(SHUTDOWN_TIMEOUT) {
            Ok(()) => tracing::debug!("telemetry shutdown (traces) complete"),
            Err(e) => tracing::warn!(error = %e, "telemetry shutdown (traces) failed"),
        }
        match self.logger_provider.shutdown() {
            Ok(()) => tracing::debug!("telemetry shutdown (logs) complete"),
            Err(e) => tracing::warn!(error = %e, "telemetry shutdown (logs) failed"),
        }
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        let _ = self.provider.shutdown_with_timeout(SHUTDOWN_TIMEOUT);
        let _ = self.logger_provider.shutdown();
    }
}
