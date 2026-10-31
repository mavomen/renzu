use gateway::config::Config;
use gateway::server::run;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().map_err(|e| format!("configuration error: {e}"))?;
    gateway::init_logging(&config.log_level);
    tracing::info!(config = ?config, "starting gateway service");
    run(config)
        .await
        .map_err(|e| format!("server error: {e}"))?;
    Ok(())
}
