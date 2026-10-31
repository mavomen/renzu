use axum::{
    response::{Json, Result},
    routing::get,
    Router,
};
use serde_json::json;
use std::net::SocketAddr;
use tracing::{info, instrument, Span};

use crate::config::Config;
use crate::error::AppError;

/// Health check response.
#[derive(serde::Serialize)]
struct HealthResponse {
    status: String,
    service: String,
}

/// Readiness check response.
#[derive(serde::Serialize)]
struct ReadyResponse {
    status: String,
    service: String,
}

/// Build the Axum application router.
///
/// Routes:
/// - `GET /health` — liveness probe, always returns healthy.
/// - `GET /ready` — readiness probe, returns healthy when dependencies are up.
/// - `GET /` — root endpoint returning service metadata.
pub fn app() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/", get(root))
        .layer(tower_http::trace::TraceLayer::new_for_http())
}

/// Liveness probe. Always responds healthy.
#[instrument(name = "GET /health", skip_all, fields(http.method = "GET", http.route = "/health"))]
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        service: "gateway".to_string(),
    })
}

/// Readiness probe. Responds healthy when the service is ready to serve traffic.
#[instrument(name = "GET /ready", skip_all, fields(http.method = "GET", http.route = "/ready"))]
async fn ready() -> Result<Json<ReadyResponse>, AppError> {
    // No external dependencies in F-004, so the service is always ready.
    let span = Span::current();
    let response = Ok(Json(ReadyResponse {
        status: "ready".to_string(),
        service: "gateway".to_string(),
    }));
    if let Err(ref err) = response {
        span.record("error", tracing::field::display(err));
    }
    response
}

/// Root endpoint returning service metadata.
#[instrument(name = "GET /", skip_all, fields(http.method = "GET", http.route = "/"))]
async fn root() -> Json<serde_json::Value> {
    Json(json!({
        "service": "gateway",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": ["/", "/health", "/ready"],
    }))
}

/// Start the HTTP server bound to the address derived from `config`.
pub async fn run(config: Config) -> Result<(), AppError> {
    let addr: SocketAddr = config
        .bind_addr()
        .parse()
        .map_err(|e| AppError::Internal(format!("invalid bind address: {e}")))?;
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        AppError::Internal(format!("failed to bind to {}: {e}", config.bind_addr()))
    })?;
    info!(address = %addr, "gateway listening");
    axum::serve(listener, app())
        .await
        .map_err(|e| AppError::Internal(format!("server error: {e}")))?;
    Ok(())
}
