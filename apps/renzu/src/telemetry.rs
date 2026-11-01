//! Structured logging initialization for the `renzu` CLI.
//!
//! Builds a tracing subscriber that emits newline-delimited JSON logs to
//! stdout. Every log event carries `service.name`, `service.version` and
//! `environment` (from `RENZU_ENVIRONMENT`, default `local`) so they can be
//! queried and correlated without a log backend. `RENZU_LOG_LEVEL` controls
//! verbosity via [`EnvFilter`].
//!
//! Sensitive values can be redacted at the call site with [`Redacted`]:
//! `tracing::info!(auth = %Redacted(&token), "authenticating")` renders
//! `[REDACTED]`. Redaction is opt-in; values logged without `Redacted` are
//! emitted as-is.

use std::fmt::{self};
use std::sync::OnceLock;
use tracing::Subscriber;
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, JsonFields, Writer};
use tracing_subscriber::fmt::time::{FormatTime, SystemTime as SystemTimeTimer};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

const SERVICE_NAME: &str = "renzu";
const DEFAULT_ENVIRONMENT: &str = "local";

/// Runtime environment, read from `RENZU_ENVIRONMENT` (default `local`).
///
/// Set once during [`Telemetry::init`] and read by the JSON event formatter for
/// every log event.
static ENVIRONMENT: OnceLock<String> = OnceLock::new();

/// Handle to an initialized logging subscriber.
///
/// The subscriber is installed globally for the lifetime of the process.
pub struct Telemetry(());

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
    /// Initialize structured JSON logging and install the global subscriber.
    ///
    /// The environment is taken from `RENZU_ENVIRONMENT` (default
    /// [`DEFAULT_ENVIRONMENT`]). `log_level` is used as the default verbosity
    /// unless `RENZU_LOG_LEVEL` is set in the environment.
    pub fn init(log_level: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let env_value =
            std::env::var("RENZU_ENVIRONMENT").unwrap_or_else(|_| DEFAULT_ENVIRONMENT.to_string());
        let _ = ENVIRONMENT.set(env_value.clone());

        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));
        let fmt_layer = tracing_subscriber::fmt::layer()
            .json()
            .event_format(ServiceFields)
            .with_filter(env_filter);

        tracing_subscriber::registry()
            .with(fmt_layer)
            .try_init()
            .map_err(|e| format!("failed to install tracing subscriber: {e}"))?;

        tracing::info!(
            service.name = SERVICE_NAME,
            environment = %environment(),
            "telemetry initialized"
        );

        Ok(Self(()))
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        // No async exporter to flush for the logging-only subscriber.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for CapturedLogs {
        type Writer = CapturedLogWriter;
        fn make_writer(&'a self) -> Self::Writer {
            CapturedLogWriter(self.0.clone())
        }
    }

    struct CapturedLogWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for CapturedLogWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn json_subscriber(captured: Arc<Mutex<Vec<u8>>>) -> impl tracing::Subscriber {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .json()
            .event_format(ServiceFields)
            .with_writer(CapturedLogs(captured));
        tracing_subscriber::registry().with(fmt_layer)
    }

    #[test]
    fn redacted_value_emits_redacted() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        let subscriber = json_subscriber(captured);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(auth = %Redacted(&"s3cr3t-token"), "user logged in");
            tracing::info!(password = ?Redacted(&"hunter2"), "password set");
        });

        let output = String::from_utf8_lossy(&captured_clone.lock().unwrap()).to_string();
        let lines: Vec<&str> = output.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 2, "expected two log lines, got:\n{output}");

        for line in &lines {
            let value: serde_json::Value =
                serde_json::from_str(line).expect("log line should be valid JSON");
            let fields = value["fields"]
                .as_object()
                .expect("fields should be an object");
            assert!(
                !fields.values().any(|v| v.as_str() == Some("s3cr3t-token"))
                    && !fields.values().any(|v| v.as_str() == Some("hunter2")),
                "secret value must not appear in log: {line}"
            );
            assert!(
                fields.values().any(|v| v.as_str() == Some("[REDACTED]")),
                "redacted value must render as [REDACTED]: {line}"
            );
        }
    }

    #[test]
    fn log_events_carries_service_and_environment_fields() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        let subscriber = json_subscriber(captured);

        tracing::subscriber::with_default(subscriber, || {
            std::env::set_var("RENZU_ENVIRONMENT", "local");
            tracing::info!("starting renzu service");
        });

        let output = String::from_utf8_lossy(&captured_clone.lock().unwrap()).to_string();
        std::env::remove_var("RENZU_ENVIRONMENT");

        let lines: Vec<&str> = output.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 1, "expected one log line, got:\n{output}");

        let value: serde_json::Value =
            serde_json::from_str(lines[0]).expect("log line should be valid JSON");
        assert_eq!(value["service.name"], "renzu");
        assert_eq!(value["service.version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["environment"], "local");
    }
}
