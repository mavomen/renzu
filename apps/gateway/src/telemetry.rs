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
//! Logs are emitted as OTLP LogRecords via `opentelemetry-appender-tracing`.
//! The `OpenTelemetryTracingBridge` layer converts `tracing` events into OTLP
//! log records, automatically attaching `trace_id`, `span_id`, and `trace_flags`
//! from the current span context. Logs are exported through a
//! [`SimpleLogProcessor`] to the same OTLP endpoint (logs path).
//!
//! Sensitive values can be redacted at the call site with [`Redacted`]:
//! `tracing::info!(auth = %Redacted(&token), "authenticating")` renders
//! `[REDACTED]`. Redaction is opt-in; values logged without `Redacted` are
//! emitted as-is.

use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::runtime::Tokio;
use opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::trace::{SpanData, SpanExporter};
use opentelemetry_sdk::Resource;
use std::fmt::{self};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
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

/// Wraps an OTLP span exporter, counting export failures so readiness can
/// reflect exporter health.
#[derive(Debug)]
struct CountingSpanExporter {
    inner: opentelemetry_otlp::SpanExporter,
    failures: Arc<AtomicU64>,
}

impl CountingSpanExporter {
    fn new(exporter: opentelemetry_otlp::SpanExporter) -> Self {
        Self {
            inner: exporter,
            failures: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl SpanExporter for CountingSpanExporter {
    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        let failures = self.failures.clone();
        async move {
            match self.inner.export(batch).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    failures.fetch_add(1, Ordering::Relaxed);
                    Err(e)
                }
            }
        }
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }
}

/// Wraps an OTLP log exporter, counting export failures so readiness can
/// reflect exporter health.
#[derive(Debug)]
struct CountingLogExporter {
    inner: opentelemetry_otlp::LogExporter,
    failures: Arc<AtomicU64>,
}

impl CountingLogExporter {
    fn new(exporter: opentelemetry_otlp::LogExporter) -> Self {
        Self {
            inner: exporter,
            failures: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl opentelemetry_sdk::logs::LogExporter for CountingLogExporter {
    fn export(
        &self,
        batch: opentelemetry_sdk::logs::LogBatch<'_>,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        let failures = self.failures.clone();
        async move {
            match self.inner.export(batch).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    failures.fetch_add(1, Ordering::Relaxed);
                    Err(e)
                }
            }
        }
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }
}

/// Runtime environment, read from `RENZU_ENVIRONMENT` (default `local`).
///
/// Set once during [`Telemetry::init`] and read by the JSON event formatter for
/// every log event.
static ENVIRONMENT: OnceLock<String> = OnceLock::new();

/// Handle to an initialized OpenTelemetry tracer and logger provider.
///
/// Shutting down the handle flushes in-flight spans and log records to the OTLP
/// endpoint before the process exits.
pub struct Telemetry {
    inner: Arc<TelemetryInner>,
}

struct TelemetryInner {
    provider: SdkTracerProvider,
    logger_provider: opentelemetry_sdk::logs::SdkLoggerProvider,
    export_failures: Arc<AtomicU64>,
    log_export_failures: Arc<AtomicU64>,
    /// Guard for the thread-level tracing dispatcher default, used when the
    /// process-wide global default is already installed (e.g. in tests where
    /// multiple tests each call Telemetry::init).
    #[allow(dead_code)]
    _dispatch_guard: Option<tracing::dispatcher::DefaultGuard>,
}

impl Clone for Telemetry {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Telemetry {
    /// Returns true if no span export has failed since the provider was created.
    ///
    /// A cumulative counter: once a failure occurs, readiness stays degraded
    /// until the process restarts.
    pub fn export_healthy(&self) -> bool {
        self.inner.export_failures.load(Ordering::Relaxed) == 0
    }

    /// Returns true if no log export has failed since the provider was created.
    pub fn log_export_healthy(&self) -> bool {
        self.inner.log_export_failures.load(Ordering::Relaxed) == 0
    }
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
    /// Initialize OpenTelemetry tracing and logging, and install the global
    /// subscriber.
    ///
    /// The OTLP endpoint is taken from `RENZU_OTLP_ENDPOINT` (default
    /// [`DEFAULT_OTLP_ENDPOINT`]). The environment is taken from
    /// `RENZU_ENVIRONMENT` (default [`DEFAULT_ENVIRONMENT`]). Resource
    /// attributes `service.name` and `service.version` are recorded on every
    /// span and log record.
    ///
    /// A JSON formatting layer, the OpenTelemetry trace layer, and the
    /// OpenTelemetry log bridge layer are installed as the global tracing
    /// subscriber using `log_level`. Export failures are logged, never fatal.
    pub fn init(log_level: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = std::env::var("RENZU_OTLP_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_OTLP_ENDPOINT.to_string());

        let environment =
            std::env::var("RENZU_ENVIRONMENT").unwrap_or_else(|_| DEFAULT_ENVIRONMENT.to_string());
        let _ = ENVIRONMENT.set(environment.clone());

        // Build the log endpoint by replacing /traces with /logs
        let log_endpoint = endpoint.replace("/v1/traces", "/v1/logs");

        // Span exporter
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(&endpoint)
            .build()
            .map_err(|e| format!("failed to build OTLP span exporter: {e}"))?;

        let counting_exporter = CountingSpanExporter::new(exporter);
        let export_failures = counting_exporter.failures.clone();

        let provider = SdkTracerProvider::builder()
            .with_span_processor(BatchSpanProcessor::builder(counting_exporter, Tokio).build())
            .with_resource(
                Resource::builder()
                    .with_attributes(vec![
                        KeyValue::new("service.name", SERVICE_NAME),
                        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                    ])
                    .build(),
            )
            .build();

        // Log exporter
        let log_exporter = opentelemetry_otlp::LogExporter::builder()
            .with_http()
            .with_endpoint(&log_endpoint)
            .build()
            .map_err(|e| format!("failed to build OTLP log exporter: {e}"))?;

        let counting_log_exporter = CountingLogExporter::new(log_exporter);
        let log_export_failures = counting_log_exporter.failures.clone();

        let logger_provider = SdkLoggerProvider::builder()
            .with_simple_exporter(counting_log_exporter)
            .with_resource(
                Resource::builder()
                    .with_attributes(vec![
                        KeyValue::new("service.name", SERVICE_NAME),
                        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                    ])
                    .build(),
            )
            .build();

        // OpenTelemetry trace layer
        let tracer = provider.tracer(SERVICE_NAME);
        let otel_layer = tracing_opentelemetry::OpenTelemetryLayer::new(tracer);

        // OpenTelemetry log bridge layer
        let log_bridge = OpenTelemetryTracingBridge::new(&logger_provider);

        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));
        let fmt_layer = tracing_subscriber::fmt::layer()
            .json()
            .event_format(ServiceFields)
            .with_filter(env_filter.clone());

        let subscriber = tracing_subscriber::registry()
            .with(fmt_layer)
            .with(otel_layer)
            .with(log_bridge);

        let _dispatch_guard = match subscriber.try_init() {
            Ok(()) => None,
            Err(_) => {
                // Global default already installed (e.g. from another test).
                // Fall back to a thread-local default so spans created on this
                // thread still use this subscriber. Rebuild the layers since
                // they were moved into the first subscriber.
                let otel_layer =
                    tracing_opentelemetry::OpenTelemetryLayer::new(provider.tracer(SERVICE_NAME));
                let log_bridge = OpenTelemetryTracingBridge::new(&logger_provider);
                let fmt_layer = tracing_subscriber::fmt::layer()
                    .json()
                    .event_format(ServiceFields)
                    .with_filter(env_filter.clone());
                let subscriber = tracing_subscriber::registry()
                    .with(fmt_layer)
                    .with(otel_layer)
                    .with(log_bridge);
                Some(tracing::dispatcher::set_default(&tracing::Dispatch::from(
                    subscriber,
                )))
            }
        };

        tracing::info!(endpoint = %endpoint, log_endpoint = %log_endpoint, "telemetry initialized");

        Ok(Self {
            inner: Arc::new(TelemetryInner {
                provider,
                logger_provider,
                export_failures,
                log_export_failures,
                _dispatch_guard,
            }),
        })
    }

    /// Force a flush of all queued spans and log records to the OTLP endpoint.
    ///
    /// Waits for the batch processor to drain its queue. Errors are logged,
    /// never fatal.
    pub fn force_flush(&self) {
        if let Err(e) = self.inner.provider.force_flush() {
            tracing::warn!(error = %e, "trace force flush failed");
        }
        if let Err(e) = self.inner.logger_provider.force_flush() {
            tracing::warn!(error = %e, "log force flush failed");
        }
    }

    /// Shut down the tracer and logger providers, flushing in-flight data.
    ///
    /// Waits up to [`SHUTDOWN_TIMEOUT`] for export to complete. Timeout or
    /// export errors are logged, never fatal.
    pub fn shutdown(&self) {
        match self.inner.provider.shutdown_with_timeout(SHUTDOWN_TIMEOUT) {
            Ok(()) => tracing::debug!("trace shutdown complete"),
            Err(e) => tracing::warn!(error = %e, "trace shutdown failed"),
        }
        match self
            .inner
            .logger_provider
            .shutdown_with_timeout(SHUTDOWN_TIMEOUT)
        {
            Ok(()) => tracing::debug!("log shutdown complete"),
            Err(e) => tracing::warn!(error = %e, "log shutdown failed"),
        }
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        self.shutdown();
    }
}
