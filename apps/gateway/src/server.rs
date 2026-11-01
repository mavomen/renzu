use axum::{
    extract::Extension,
    http::header::{HeaderMap, CONTENT_TYPE},
    response::{Json, Result},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, Span};

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

/// Shared fault-injection state. All faults are disabled by default.
#[derive(Clone, Default)]
pub struct FaultState {
    db_latency: Arc<Mutex<bool>>,
    downstream_timeout: Arc<Mutex<bool>>,
    controlled_error: Arc<Mutex<bool>>,
}

impl FaultState {
    fn is_db_latency_enabled(&self) -> bool {
        *self.db_latency.lock().unwrap()
    }

    fn set_db_latency(&self, enabled: bool) {
        *self.db_latency.lock().unwrap() = enabled;
    }

    fn is_downstream_timeout_enabled(&self) -> bool {
        *self.downstream_timeout.lock().unwrap()
    }

    fn set_downstream_timeout(&self, enabled: bool) {
        *self.downstream_timeout.lock().unwrap() = enabled;
    }

    fn is_controlled_error_enabled(&self) -> bool {
        *self.controlled_error.lock().unwrap()
    }

    fn set_controlled_error(&self, enabled: bool) {
        *self.controlled_error.lock().unwrap() = enabled;
    }
}

pub fn app(
    pool: Arc<db::DbPool>,
    orders_client: reqwest::Client,
    fault_state: Arc<FaultState>,
) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/", get(root))
        .route("/db/health", get(db_health))
        .route("/order/:id", get(order_forward))
        .route(
            "/faults/database-latency",
            post(enable_db_latency).delete(disable_db_latency),
        )
        .route(
            "/faults/downstream-timeout",
            post(enable_downstream_timeout).delete(disable_downstream_timeout),
        )
        .route(
            "/faults/controlled-error",
            post(enable_controlled_error).delete(disable_controlled_error),
        )
        .layer(Extension(pool))
        .layer(Extension(orders_client))
        .layer(Extension(fault_state))
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
    Extension(fault_state): Extension<Arc<FaultState>>,
) -> Result<Json<ReadyResponse>, AppError> {
    let span = Span::current();

    // Inject database latency if the fault is enabled.
    if fault_state.is_db_latency_enabled() {
        sleep(Duration::from_millis(200)).await;
    }

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
        "endpoints": ["/", "/health", "/ready", "/db/health", "/order/:id", "/faults/database-latency", "/faults/downstream-timeout", "/faults/controlled-error"],
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
    Extension(fault_state): Extension<Arc<FaultState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    headers: HeaderMap,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    let span = Span::current();
    span.record("order.id", &id);

    // Inject controlled error if enabled.
    if fault_state.is_controlled_error_enabled() {
        return Err(AppError::Internal(
            "controlled error: simulated gateway error".to_string(),
        ));
    }

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

    // Inject downstream timeout if enabled.
    if fault_state.is_downstream_timeout_enabled() {
        sleep(Duration::from_secs(5)).await;
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

/// Enable database latency fault.
async fn enable_db_latency(
    Extension(fault_state): Extension<Arc<FaultState>>,
) -> Json<serde_json::Value> {
    fault_state.set_db_latency(true);
    Json(serde_json::json!({ "fault": "database-latency", "enabled": true }))
}

/// Disable database latency fault.
async fn disable_db_latency(
    Extension(fault_state): Extension<Arc<FaultState>>,
) -> Json<serde_json::Value> {
    fault_state.set_db_latency(false);
    Json(serde_json::json!({ "fault": "database-latency", "enabled": false }))
}

/// Enable downstream timeout fault.
async fn enable_downstream_timeout(
    Extension(fault_state): Extension<Arc<FaultState>>,
) -> Json<serde_json::Value> {
    fault_state.set_downstream_timeout(true);
    Json(serde_json::json!({ "fault": "downstream-timeout", "enabled": true }))
}

/// Disable downstream timeout fault.
async fn disable_downstream_timeout(
    Extension(fault_state): Extension<Arc<FaultState>>,
) -> Json<serde_json::Value> {
    fault_state.set_downstream_timeout(false);
    Json(serde_json::json!({ "fault": "downstream-timeout", "enabled": false }))
}

/// Request body for enabling controlled error fault.
#[derive(Deserialize)]
struct ControlledErrorRequest {
    message: Option<String>,
}

/// Enable controlled error fault.
async fn enable_controlled_error(
    Extension(fault_state): Extension<Arc<FaultState>>,
    body: Option<axum::extract::Json<ControlledErrorRequest>>,
) -> Json<serde_json::Value> {
    fault_state.set_controlled_error(true);
    let message = body
        .as_ref()
        .and_then(|b| b.message.as_deref())
        .unwrap_or("controlled error");
    Json(serde_json::json!({
        "fault": "controlled-error",
        "enabled": true,
        "message": message
    }))
}

/// Disable controlled error fault.
async fn disable_controlled_error(
    Extension(fault_state): Extension<Arc<FaultState>>,
) -> Json<serde_json::Value> {
    fault_state.set_controlled_error(false);
    Json(serde_json::json!({ "fault": "controlled-error", "enabled": false }))
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

    let fault_state = Arc::new(FaultState::default());
    let app = app(pool.clone(), orders_client, fault_state).layer(MetricsLayer::new(metrics));
    info!(address = %addr, "gateway listening");

    let result = axum::serve(listener, app).await;
    let _ = metrics_handle.shutdown();
    pool.close().await;
    result.map_err(|e| AppError::Internal(format!("server error: {e}")))?;
    Ok(())
}
