use gateway::config::Config;
use gateway::server::run;
use gateway::telemetry::Telemetry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().map_err(|e| format!("configuration error: {e}"))?;
    let telemetry =
        Telemetry::init(&config.log_level).map_err(|e| format!("telemetry error: {e}"))?;
    tracing::info!(config = ?config, "starting gateway service");
    let result = run(config).await;
    telemetry.shutdown();
    result.map_err(|e| format!("server error: {e}"))?;
    Ok(())
}
