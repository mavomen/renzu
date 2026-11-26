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
    span_export_failures: u64,
    log_export_failures: u64,
}

pub fn app(telemetry: Telemetry) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/", get(root))
        .route("/order/:id", get(order))
        .layer(Extension(telemetry))
        .layer(
            TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<_>| {
                tracing::span!(
                    tracing::Level::INFO,
                    "http.request",
                    method = %req.method(),
                    route = %req.uri().path(),
                )
            }),
        )
}

async fn health() -> Json<HealthResponse> {
    tracing::info!(route = "/health", "health check requested");
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
        span_export_failures: telemetry.export_failures(),
        log_export_failures: telemetry.log_export_failures(),
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

pub async fn run(config: Config, telemetry: Telemetry) -> Result<(), AppError> {
    let addr: SocketAddr = config
        .bind_addr()
        .parse()
        .map_err(|e| AppError::Internal(format!("invalid bind address: {e}")))?;
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        AppError::Internal(format!("failed to bind to {}: {e}", config.bind_addr()))
    })?;
    let app = app(telemetry);
    info!(address = %addr, "orders service listening");
    let result = axum::serve(listener, app).await;
    result.map_err(|e| AppError::Internal(format!("server error: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ready_reports_log_export_failure() {
        let Json(response) = ready(Extension(Telemetry::with_counters(0, 2))).await;

        assert_eq!(response.status, "not ready");
        assert_eq!(response.service, "orders");
        assert_eq!(response.span_export_failures, 0);
        assert_eq!(response.log_export_failures, 2);

        let value = serde_json::to_value(&response).expect("response should serialize");
        assert_eq!(value["status"], "not ready");
        assert_eq!(value["span_export_failures"], 0);
        assert_eq!(value["log_export_failures"], 2);
    }
}
