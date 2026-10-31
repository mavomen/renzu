use axum::{
    http::{Request, StatusCode},
    Router,
};
use gateway::server::app;
use gateway::telemetry::Telemetry;
use http_body_util::{BodyExt, Empty};
use hyper_util::rt::TokioIo;
use opentelemetry_proto::tonic::{
    collector::trace::v1::ExportTraceServiceRequest, common::v1::any_value::Value,
    common::v1::AnyValue,
};
use prost::Message;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

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

    let router: Router = app();
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
    let router: Router = app();
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
    let router: Router = app();
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
    let router: Router = app();
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
    let router: Router = app();
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

/// Collect an axum response body into bytes.
async fn collect_body(response: axum::response::Response) -> Vec<u8> {
    let collected = response
        .into_body()
        .collect()
        .await
        .expect("body should read");
    collected.to_bytes().to_vec()
}
