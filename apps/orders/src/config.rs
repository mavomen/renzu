use serde::Deserialize;
use std::net::SocketAddr;

/// Runtime configuration for the orders HTTP service.
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
}

impl Config {
    /// Load configuration from environment variables, applying defaults.
    ///
    /// Environment variables:
    /// - `RENZU_HOST` (default `127.0.0.1`)
    /// - `RENZU_PORT` (default `8081`)
    /// - `RENZU_LOG_LEVEL` (default `info`)
    pub fn from_env() -> Result<Config, ConfigError> {
        let host = std::env::var("RENZU_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port: u16 = std::env::var("RENZU_PORT")
            .unwrap_or_else(|_| "8081".to_string())
            .parse()
            .map_err(ConfigError::InvalidPort)?;
        let log_level = std::env::var("RENZU_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());

        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(ConfigError::InvalidAddress)?;

        let _ = addr; // validated above; host/port stored separately for logging

        Ok(Config {
            host,
            port,
            log_level,
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

    #[test]
    fn from_env_uses_defaults_when_vars_absent() {
        std::env::remove_var("RENZU_HOST");
        std::env::remove_var("RENZU_PORT");
        std::env::remove_var("RENZU_LOG_LEVEL");

        let config = Config::from_env().expect("defaults should load");
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8081);
        assert_eq!(config.log_level, "info");
        assert_eq!(config.bind_addr(), "127.0.0.1:8081");
    }

    #[test]
    fn from_env_reads_overrides() {
        std::env::set_var("RENZU_HOST", "0.0.0.0");
        std::env::set_var("RENZU_PORT", "9091");
        std::env::set_var("RENZU_LOG_LEVEL", "debug");

        let config = Config::from_env().expect("overrides should load");
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 9091);
        assert_eq!(config.log_level, "debug");

        std::env::remove_var("RENZU_HOST");
        std::env::remove_var("RENZU_PORT");
        std::env::remove_var("RENZU_LOG_LEVEL");
    }

    #[test]
    fn from_env_rejects_invalid_port() {
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
