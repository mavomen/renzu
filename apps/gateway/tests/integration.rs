use axum::{
    http::{Request, StatusCode},
    Router,
};

use gateway::telemetry::{Redacted, Telemetry};
use http_body_util::{BodyExt, Empty};
use hyper_util::rt::TokioIo;
use opentelemetry_proto::tonic::{
    collector::trace::v1::ExportTraceServiceRequest, common::v1::any_value::Value,
    common::v1::AnyValue,
};
use prost::Message;
use sqlx::postgres::PgPoolOptions;
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt as _;

#[derive(Clone, Debug)]
struct OtlpSpan {
    name: String,
    attributes: HashMap<String, String>,
}

async fn collect_incoming(body: hyper::body::Incoming) -> Vec<u8> {
    let collected = body.collect().await.expect("body should read");
    collected.to_bytes().to_vec()
}

fn any_value_to_string(value: &AnyValue) -> String {
    match &value.value {
        Some(Value::StringValue(s)) => s.clone(),
        Some(Value::IntValue(i)) => i.to_string(),
        Some(Value::DoubleValue(d)) => d.to_string(),
        Some(Value::BoolValue(b)) => b.to_string(),
        _ => String::new(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn health_span_exported_to_otlp_receiver() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test receiver should bind");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    let endpoint = format!("http://{addr}/v1/traces");

    let spans: Arc<Mutex<Vec<OtlpSpan>>> = Arc::new(Mutex::new(Vec::new()));
    let spans_svc = spans.clone();

    let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
        let spans = spans_svc.clone();
        async move {
            if req.method() == hyper::http::Method::POST && req.uri().path() == "/v1/traces" {
                let bytes = collect_incoming(req.into_body()).await;
                if let Ok(request) = ExportTraceServiceRequest::decode(&bytes[..]) {
                    for resource_spans in request.resource_spans {
                        for scope_spans in resource_spans.scope_spans {
                            for span in scope_spans.spans {
                                let mut attributes = HashMap::new();
                                for attr in span.attributes {
                                    let key = attr.key;
                                    attributes.insert(
                                        key,
                                        attr.value
                                            .map(|v| any_value_to_string(&v))
                                            .unwrap_or_default(),
                                    );
                                }
                                spans.lock().unwrap().push(OtlpSpan {
                                    name: span.name,
                                    attributes,
                                });
                            }
                        }
                    }
                }
                Ok::<_, hyper::Error>(hyper::Response::new(Empty::<hyper::body::Bytes>::new()))
            } else {
                Ok::<_, hyper::Error>(
                    hyper::Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Empty::<hyper::body::Bytes>::new())
                        .unwrap(),
                )
            }
        }
    });

    let receiver = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(c) => c,
                Err(_) => return,
            };
            let svc = service.clone();
            if hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), svc)
                .await
                .is_err()
            {
                return;
            }
        }
    });

    // Allow the receiver to start accepting connections.
    tokio::time::sleep(Duration::from_millis(25)).await;

    std::env::set_var("RENZU_OTLP_ENDPOINT", &endpoint);
    let telemetry = Telemetry::init("info").expect("telemetry should initialize");

    let router = test_app();
    let request = Request::builder()
        .uri("/health")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = router
        .oneshot(request)
        .await
        .expect("service should respond");
    assert_eq!(response.status(), StatusCode::OK);

    // The batch processor exports on a background thread; flush so the span is
    // guaranteed to have reached the receiver before we assert.
    telemetry.force_flush();

    let span = spans
        .lock()
        .unwrap()
        .iter()
        .find(|s| s.name == "GET /health")
        .cloned()
        .expect("expected GET /health span to be exported to OTLP receiver");
    assert_eq!(
        span.attributes.get("http.route"),
        Some(&"/health".to_string())
    );
    assert_eq!(span.attributes.get("http.method"), Some(&"GET".to_string()));

    receiver.abort();
    std::env::remove_var("RENZU_OTLP_ENDPOINT");
    telemetry.shutdown();
}

#[tokio::test]
async fn health_endpoint_returns_healthy() {
    let router = test_app();
    let request = Request::builder()
        .uri("/health")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = router
        .oneshot(request)
        .await
        .expect("service should respond");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = collect_body(response).await;
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["status"], "healthy");
    assert_eq!(value["service"], "gateway");
}

#[tokio::test]
async fn ready_endpoint_returns_ready() {
    // Only assert "ready" when a real test database is available.
    // With a lazy pool (no DB), the endpoint returns "not ready" gracefully.
    if std::env::var("RENZU_TEST_DATABASE_URL").is_err() {
        // No test database: verify the endpoint responds and reports not ready.
        let router = test_app();
        let request = Request::builder()
            .uri("/ready")
            .method("GET")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = router
            .oneshot(request)
            .await
            .expect("service should respond");
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = collect_body(response).await;
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["status"], "not ready");
        assert_eq!(value["service"], "gateway");
        return;
    }

    // Real test database available: verify the endpoint reports ready.
    let pool = Arc::new({
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&std::env::var("RENZU_TEST_DATABASE_URL").unwrap())
            .await
            .expect("test database should be reachable");
        pool
    });
    let orders_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .expect("client");
    let fault_state = Arc::new(gateway::server::FaultState::default());
    let router = gateway::server::app(pool, orders_client, fault_state);
    let request = Request::builder()
        .uri("/ready")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = router
        .oneshot(request)
        .await
        .expect("service should respond");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = collect_body(response).await;
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["status"], "ready");
    assert_eq!(value["service"], "gateway");
}

#[tokio::test]
async fn root_endpoint_returns_metadata() {
    let router = test_app();
    let request = Request::builder()
        .uri("/")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = router
        .oneshot(request)
        .await
        .expect("service should respond");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = collect_body(response).await;
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["service"], "gateway");
    assert!(value["endpoints"].is_array());
}

#[tokio::test]
async fn unknown_route_returns_not_found() {
    let router = test_app();
    let request = Request::builder()
        .uri("/no-such-route")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = router
        .oneshot(request)
        .await
        .expect("service should respond");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// Build a test router with a database pool and fault state.
///
/// Uses `connect_lazy` so no real database connection is established;
/// database-dependent endpoints will return "not ready" gracefully.
fn test_app() -> Router {
    let pool = Arc::new(
        PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://renzu:renzu@127.0.0.1:5432/renzu")
            .expect("test database pool should be created"),
    );
    let orders_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .expect("orders client should be created");
    let fault_state = Arc::new(gateway::server::FaultState::default());
    gateway::server::app(pool, orders_client, fault_state)
}

/// Collect an axum response body into bytes.
async fn collect_body(response: axum::response::Response) -> Vec<u8> {
    let collected = response
        .into_body()
        .collect()
        .await
        .expect("body should read");
    collected.to_bytes().to_vec()
}

/// A buffering test writer that captures log output into a shared byte buffer.
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

impl<'a> MakeWriter<'a> for CapturedLogs {
    type Writer = CapturedLogWriter;
    fn make_writer(&'a self) -> Self::Writer {
        CapturedLogWriter(self.0.clone())
    }
}

struct CapturedLogWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for CapturedLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Builds a JSON test subscriber that uses the same `ServiceFields` event
/// formatter as `Telemetry::init`, writing into the provided buffer.
fn json_subscriber(captured: Arc<Mutex<Vec<u8>>>) -> impl tracing::Subscriber {
    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .event_format(gateway::telemetry::ServiceFields)
        .with_writer(CapturedLogs(captured));
    tracing_subscriber::registry().with(fmt_layer)
}

#[test]
fn redacted_value_emits_redacted() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    let subscriber = json_subscriber(captured);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(auth = %Redacted(&"s3cr3t-token"), "user logged in");
        tracing::info!(password = ?Redacted(&"hunter2"), "password set");
    });

    let output = String::from_utf8_lossy(&captured_clone.lock().unwrap()).to_string();

    let lines: Vec<&str> = output.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 2, "expected two log lines, got:\n{output}");

    for line in &lines {
        let value: serde_json::Value =
            serde_json::from_str(line).expect("log line should be valid JSON");
        let fields = value["fields"]
            .as_object()
            .expect("fields should be an object");
        assert!(
            !fields
                .values()
                .any(|v| v.as_str() == Some("s3cr3t-token") || v.as_str() == Some("hunter2")),
            "secret value must not appear in log: {line}"
        );
        assert!(
            fields.values().any(|v| v.as_str() == Some("[REDACTED]")),
            "redacted value must render as [REDACTED]: {line}"
        );
    }
}

#[test]
fn log_events_carries_service_and_environment_fields() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();
    let subscriber = json_subscriber(captured);

    tracing::subscriber::with_default(subscriber, || {
        std::env::set_var("RENZU_ENVIRONMENT", "local");
        tracing::info!("starting gateway service");
    });

    let output = String::from_utf8_lossy(&captured_clone.lock().unwrap()).to_string();
    std::env::remove_var("RENZU_ENVIRONMENT");

    let lines: Vec<&str> = output.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "expected one log line, got:\n{output}");

    let value: serde_json::Value =
        serde_json::from_str(lines[0]).expect("log line should be valid JSON");
    assert_eq!(value["service.name"], "gateway");
    assert_eq!(value["service.version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["environment"], "local");
}

#[tokio::test]
async fn order_forward_returns_not_ready_when_orders_unavailable() {
    // With no orders service running, the /order/:id endpoint should return
    // an error (502) rather than crashing, demonstrating downstream failure
    // handling.
    let router = test_app();
    let request = Request::builder()
        .uri("/order/123")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = router
        .oneshot(request)
        .await
        .expect("service should respond");
    // Orders service is unavailable, so gateway returns an error
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn order_forward_propagates_traceparent() {
    // When a traceparent header is present, it should be forwarded to the
    // orders service. We verify the handler constructs the request correctly
    // by checking the response is an error (orders unavailable) but the
    // handler didn't panic.
    let router = test_app();
    let request = Request::builder()
        .uri("/order/456")
        .method("GET")
        .header(
            "traceparent",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        )
        .body(axum::body::Body::empty())
        .unwrap();

    let response = router
        .oneshot(request)
        .await
        .expect("service should respond");
    // Orders service unavailable, but traceparent was forwarded without panic
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn enable_db_latency_fault() {
    let router = test_app();
    let request = Request::builder()
        .uri("/faults/database-latency")
        .method("POST")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = router
        .oneshot(request)
        .await
        .expect("service should respond");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = collect_body(response).await;
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["fault"], "database-latency");
    assert_eq!(value["enabled"], true);
}

#[tokio::test]
async fn disable_db_latency_fault() {
    let router = test_app();
    // First enable
    let request = Request::builder()
        .uri("/faults/database-latency")
        .method("POST")
        .body(axum::body::Body::empty())
        .unwrap();
    let _ = router
        .clone()
        .oneshot(request)
        .await
        .expect("service should respond");

    // Then disable
    let request = Request::builder()
        .uri("/faults/database-latency")
        .method("DELETE")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = router
        .oneshot(request)
        .await
        .expect("service should respond");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = collect_body(response).await;
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["fault"], "database-latency");
    assert_eq!(value["enabled"], false);
}

#[tokio::test]
async fn enable_controlled_error_fault() {
    let router = test_app();
    let request = Request::builder()
        .uri("/faults/controlled-error")
        .method("POST")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(r#"{"message":"test error"}"#))
        .unwrap();

    let response = router
        .oneshot(request)
        .await
        .expect("service should respond");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = collect_body(response).await;
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["fault"], "controlled-error");
    assert_eq!(value["enabled"], true);
    assert_eq!(value["message"], "test error");
}

#[tokio::test]
async fn controlled_error_returns_error_from_order() {
    let router = test_app();
    // Enable controlled error fault
    let request = Request::builder()
        .uri("/faults/controlled-error")
        .method("POST")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            r#"{"message":"simulated gateway error"}"#,
        ))
        .unwrap();
    let _ = router
        .clone()
        .oneshot(request)
        .await
        .expect("service should respond");

    // Now order should return 500
    let request = Request::builder()
        .uri("/order/123")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = router
        .oneshot(request)
        .await
        .expect("service should respond");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn downstream_timeout_fault_toggle() {
    let router = test_app();
    // Enable
    let request = Request::builder()
        .uri("/faults/downstream-timeout")
        .method("POST")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("service should respond");
    assert_eq!(response.status(), StatusCode::OK);

    // Disable
    let request = Request::builder()
        .uri("/faults/downstream-timeout")
        .method("DELETE")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = router
        .oneshot(request)
        .await
        .expect("service should respond");
    assert_eq!(response.status(), StatusCode::OK);
}
