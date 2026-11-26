use orders::config::Config;
use orders::server::run;
use orders::telemetry::Telemetry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().map_err(|e| format!("configuration error: {e}"))?;

    #[cfg(feature = "profiling")]
    if std::env::var("RENZU_PROFILING").is_ok() {
        console_subscriber::init();
    }

    let telemetry =
        Telemetry::init(&config.log_level).map_err(|e| format!("telemetry error: {e}"))?;
    tracing::info!(config = ?config, "starting orders service");
    let result = run(config, telemetry.clone()).await;
    telemetry.shutdown();
    result.map_err(|e| format!("server error: {e}"))?;
    Ok(())
}
