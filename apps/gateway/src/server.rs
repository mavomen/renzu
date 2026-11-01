use axum::{
    extract::Extension,
    http::header::{HeaderMap, CONTENT_TYPE},
    response::{Json, Result},
    routing::get,
    Router,
};
use serde::Serialize;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

use crate::config::Config;
use crate::db;
use crate::error::AppError;
use crate::metrics::MetricsLayer;

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

pub fn app(pool: Arc<db::DbPool>, orders_client: reqwest::Client) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/", get(root))
        .route("/db/health", get(db_health))
        .route("/order/:id", get(order_forward))
        .layer(Extension(pool))
        .layer(Extension(orders_client))
        .layer(tower_http::trace::TraceLayer::new_for_http())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        service: "gateway".to_string(),
    })
}

async fn ready(
    Extension(pool): Extension<Arc<db::DbPool>>,
) -> Result<Json<ReadyResponse>, AppError> {
    let span = tracing::Span::current();
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

async fn root() -> Json<serde_json::Value> {
    Json(json!({
        "service": "gateway",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": ["/", "/health", "/ready", "/db/health", "/order/:id"],
    }))
}

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

async fn order_forward(
    Extension(orders_client): Extension<reqwest::Client>,
    axum::extract::Path(id): axum::extract::Path<String>,
    headers: HeaderMap,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    let span = tracing::Span::current();
    span.record("order.id", &id);

    let orders_url =
        std::env::var("RENZU_ORDERS_URL").unwrap_or_else(|_| "http://orders:8081".to_string());

    let mut request = orders_client
        .get(format!("{orders_url}/order/{id}"))
        .header(CONTENT_TYPE.as_str(), "application/json");

    if let Some(traceparent) = headers.get("traceparent") {
        if let Ok(tp_str) = traceparent.to_str() {
            request = request.header("traceparent", tp_str);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("orders request failed: {e}")))?;

    let status = response.status().as_u16();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("orders response parse failed: {e}")))?;

    span.record("orders.status", status.to_string().as_str());

    Ok(axum::Json(serde_json::json!({
        "order_id": id,
        "gateway_status": "forwarded",
        "orders": body,
    })))
}

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

    let orders_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| AppError::Internal(format!("orders client build failed: {e}")))?;

    let app = app(pool.clone(), orders_client).layer(MetricsLayer::new(metrics));
    info!(address = %addr, "gateway listening");

    let result = axum::serve(listener, app).await;
    let _ = metrics_handle.shutdown();
    pool.close().await;
    result.map_err(|e| AppError::Internal(format!("server error: {e}")))?;
    Ok(())
}
