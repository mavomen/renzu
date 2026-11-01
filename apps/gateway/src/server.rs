use axum::extract::Extension;
use axum::{
    response::{Json, Result},
    routing::get,
    Router,
};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, instrument, Span};

use crate::config::Config;
use crate::db;
use crate::error::AppError;
use crate::metrics::MetricsLayer;

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
/// - `GET /db/health` — database connectivity probe.
pub fn app(pool: Arc<db::DbPool>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/", get(root))
        .route("/db/health", get(db_health))
        .layer(Extension(pool))
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

/// Readiness probe. Responds healthy when the service and its dependencies
/// (PostgreSQL) are ready to serve traffic.
#[instrument(name = "GET /ready", skip_all, fields(http.method = "GET", http.route = "/ready"))]
async fn ready(
    Extension(pool): Extension<Arc<db::DbPool>>,
) -> Result<Json<ReadyResponse>, AppError> {
    let span = Span::current();
    let status = match db::health_check(&pool).await {
        Ok(()) => "ready",
        Err(_) => "not ready",
    };
    let response = Ok(Json(ReadyResponse {
        status: status.to_string(),
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

/// Database health check. Runs `SELECT 1` against PostgreSQL and reports
/// whether the database is reachable.
#[instrument(name = "GET /db/health", skip_all, fields(http.method = "GET", http.route = "/db/health"))]
async fn db_health(
    Extension(pool): Extension<Arc<db::DbPool>>,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    match db::health_check(&pool).await {
        Ok(()) => Ok(axum::Json(serde_json::json!({
            "status": "healthy",
            "service": "gateway",
            "database": "connected",
        }))),
        Err(e) => Err(AppError::Internal(format!(
            "database health check failed: {e}"
        ))),
    }
}

/// Start the HTTP server bound to the address derived from `config`.
///
/// The router is wrapped with [`MetricsLayer`] so every request (including
/// 404s) is counted in `renzu_http_requests_total`, error counts, and request
/// duration. The meter provider is flushed on shutdown.
pub async fn run(config: Config) -> Result<(), AppError> {
    let addr: SocketAddr = config
        .bind_addr()
        .parse()
        .map_err(|e| AppError::Internal(format!("invalid bind address: {e}")))?;
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        AppError::Internal(format!("failed to bind to {}: {e}", config.bind_addr()))
    })?;
    let metrics = crate::metrics::Metrics::init()
        .map_err(|e| AppError::Internal(format!("metrics init error: {e}")))?;
    let metrics_handle = metrics.clone();
    let pool = Arc::new(
        db::init_db(&config.database_url)
            .await
            .map_err(|e| AppError::Internal(format!("database init error: {e}")))?,
    );
    let app = app(pool.clone()).layer(MetricsLayer::new(metrics));
    info!(address = %addr, "gateway listening");

    let result = axum::serve(listener, app).await;
    let _ = metrics_handle.shutdown();
    pool.close().await;
    result.map_err(|e| AppError::Internal(format!("server error: {e}")))?;
    Ok(())
}
