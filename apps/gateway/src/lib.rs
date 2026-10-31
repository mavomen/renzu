use tracing_subscriber::EnvFilter;

pub mod config;
pub mod error;
pub mod server;
pub mod telemetry;

/// Initialize structured JSON logging with tracing.
///
/// The log level is taken from the `RENZU_LOG_LEVEL` environment variable
/// (default `info`). Logs are emitted as JSON to stdout so they can be
/// consumed by downstream log tooling.
pub fn init_logging(log_level: &str) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(env_filter)
        .init();
}
