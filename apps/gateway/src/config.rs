use serde::Deserialize;
use std::net::SocketAddr;

/// Runtime configuration for the gateway HTTP service.
///
/// Values are read from environment variables with sensible defaults so the
/// service can be started without a config file.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Host the HTTP server binds to.
    pub host: String,
    /// Port the HTTP server listens on.
    pub port: u16,
    /// tracing log level (off, error, warn, info, debug, trace).
    pub log_level: String,
    /// PostgreSQL connection URL.
    pub database_url: String,
    /// Orders service URL (e.g. http://orders:8081).
    pub orders_url: String,
}

impl Config {
    /// Load configuration from environment variables, applying defaults.
    ///
    /// Environment variables:
    /// - `RENZU_HOST` (default `127.0.0.1`)
    /// - `RENZU_PORT` (default `8080`)
    /// - `RENZU_LOG_LEVEL` (default `info`)
    /// - `RENZU_DATABASE_URL` (default `postgres://renzu:renzu@127.0.0.1:5432/renzu`, local-only)
    /// - `RENZU_ORDERS_URL` (default `http://orders:8081`, local-only)
    pub fn from_env() -> Result<Config, ConfigError> {
        let host = std::env::var("RENZU_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port: u16 = std::env::var("RENZU_PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse()
            .map_err(ConfigError::InvalidPort)?;
        let log_level = std::env::var("RENZU_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
        let database_url = std::env::var("RENZU_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://renzu:renzu@127.0.0.1:5432/renzu".to_string());
        let orders_url =
            std::env::var("RENZU_ORDERS_URL").unwrap_or_else(|_| "http://orders:8081".to_string());

        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(ConfigError::InvalidAddress)?;

        let _ = addr; // validated above; host/port stored separately for logging

        Ok(Config {
            host,
            port,
            log_level,
            database_url,
            orders_url,
        })
    }

    /// Return the bind address as `host:port`.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn from_env_uses_defaults_when_vars_absent() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::remove_var("RENZU_HOST");
        std::env::remove_var("RENZU_PORT");
        std::env::remove_var("RENZU_LOG_LEVEL");
        std::env::remove_var("RENZU_DATABASE_URL");
        std::env::remove_var("RENZU_ORDERS_URL");

        let config = Config::from_env().expect("defaults should load");
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8080);
        assert_eq!(config.log_level, "info");
        assert_eq!(
            config.database_url,
            "postgres://renzu:renzu@127.0.0.1:5432/renzu"
        );
        assert_eq!(config.orders_url, "http://orders:8081");
        assert_eq!(config.bind_addr(), "127.0.0.1:8080");
    }

    #[test]
    fn from_env_reads_overrides() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("RENZU_HOST", "0.0.0.0");
        std::env::set_var("RENZU_PORT", "9090");
        std::env::set_var("RENZU_LOG_LEVEL", "debug");
        std::env::set_var(
            "RENZU_DATABASE_URL",
            "postgres://user:pass@localhost:5432/db",
        );
        std::env::set_var("RENZU_ORDERS_URL", "http://orders:9091");

        let config = Config::from_env().expect("overrides should load");
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 9090);
        assert_eq!(config.log_level, "debug");
        assert_eq!(
            config.database_url,
            "postgres://user:pass@localhost:5432/db"
        );
        assert_eq!(config.orders_url, "http://orders:9091");

        std::env::remove_var("RENZU_HOST");
        std::env::remove_var("RENZU_PORT");
        std::env::remove_var("RENZU_LOG_LEVEL");
        std::env::remove_var("RENZU_DATABASE_URL");
        std::env::remove_var("RENZU_ORDERS_URL");
    }

    #[test]
    fn from_env_rejects_invalid_port() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("RENZU_PORT", "not-a-port");
        let result = Config::from_env();
        assert!(result.is_err(), "invalid port should error");
        std::env::remove_var("RENZU_PORT");
    }
}

/// Configuration loading errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid port value: {0}")]
    InvalidPort(#[source] std::num::ParseIntError),
    #[error("invalid listen address: {0}")]
    InvalidAddress(#[source] std::net::AddrParseError),
}
