//! Telemetry initialization for the orders HTTP service.
//!
//! Builds an OpenTelemetry SdkTracerProvider with an OTLP span exporter and
//! installs a tracing-opentelemetry layer into the global tracing subscriber.
//! Export failures are logged to stderr and never fatal.
//!
//! Logs are emitted as newline-delimited JSON to stdout.
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use std::time::Duration;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
const SERVICE_NAME: &str = "orders";
const DEFAULT_ENVIRONMENT: &str = "local";
const DEFAULT_OTLP_ENDPOINT: &str = "http://127.0.0.1:4317/v1/traces";
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Telemetry {
    provider: SdkTracerProvider,
}

impl Telemetry {
    pub fn init(log_level: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = std::env::var("RENZU_OTLP_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_OTLP_ENDPOINT.to_string());
        let environment =
            std::env::var("RENZU_ENVIRONMENT").unwrap_or_else(|_| DEFAULT_ENVIRONMENT.to_string());
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
                        KeyValue::new("environment", environment),
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
            .with_filter(env_filter);
        tracing_subscriber::registry()
            .with(fmt_layer)
            .with(otel_layer)
            .try_init()
            .map_err(|e| format!("failed to install tracing subscriber: {e}"))?;
        tracing::info!(endpoint = %endpoint, "telemetry initialized");
        Ok(Self { provider })
    }
    pub fn force_flush(&self) {
        if let Err(e) = self.provider.force_flush() {
            tracing::warn!(error = %e, "telemetry force flush failed");
        }
    }
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
