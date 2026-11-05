use axum::extract::{Extension, Path};
use axum::{response::Json, routing::get, Router};
use serde::Serialize;
use serde_json::json;
use std::net::SocketAddr;
use tracing::{info, instrument, Span};

use crate::config::Config;
use crate::error::AppError;
use crate::telemetry::Telemetry;
use tower_http::trace::TraceLayer;

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    service: String,
}

#[derive(Serialize)]
struct ReadyResponse {
    status: String,
    service: String,
}

pub fn app(telemetry: Telemetry) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/", get(root))
        .route("/order/:id", get(order))
        .layer(Extension(telemetry))
        .layer(TraceLayer::new_for_http())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        service: "orders".to_string(),
    })
}

async fn ready(Extension(telemetry): Extension<Telemetry>) -> Json<ReadyResponse> {
    // Telemetry export failure: report not ready so load balancers stop sending
    // traffic to a service that cannot export its own telemetry.
    let status = if telemetry.export_healthy() {
        "ready"
    } else {
        "not ready"
    };
    Json(ReadyResponse {
        status: status.to_string(),
        service: "orders".to_string(),
    })
}

async fn root() -> Json<serde_json::Value> {
    Json(json!({
        "service": "orders",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": ["/", "/health", "/ready", "/order/:id"],
    }))
}

#[instrument(name = "GET /order/:id", skip_all, fields(http.method = "GET", http.route = "/order/:id"))]
async fn order(Path(id): Path<String>) -> Json<serde_json::Value> {
    let span = Span::current();
    span.record("order.id", &id);
    Json(json!({
        "order_id": id,
        "status": "fulfilled",
        "service": "orders",
    }))
}

pub async fn run(config: Config) -> Result<(), AppError> {
    let addr: SocketAddr = config
        .bind_addr()
        .parse()
        .map_err(|e| AppError::Internal(format!("invalid bind address: {e}")))?;
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        AppError::Internal(format!("failed to bind to {}: {e}", config.bind_addr()))
    })?;
    let telemetry = crate::telemetry::Telemetry::init(&config.log_level)
        .map_err(|e| AppError::Internal(format!("telemetry init error: {e}")))?;
    let app = app(telemetry.clone());
    info!(address = %addr, "orders service listening");
    let result = axum::serve(listener, app).await;
    telemetry.shutdown();
    result.map_err(|e| AppError::Internal(format!("server error: {e}")))?;
    Ok(())
}
