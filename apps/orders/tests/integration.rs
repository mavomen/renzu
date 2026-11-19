use axum::{
    http::{Request, StatusCode},
    Router,
};

use http_body_util::{BodyExt, Empty};
use hyper_util::rt::TokioIo;
use opentelemetry_proto::tonic::{
    collector::logs::v1::ExportLogsServiceRequest, collector::trace::v1::ExportTraceServiceRequest,
    common::v1::any_value::Value, common::v1::AnyValue,
};
use orders::telemetry::{Redacted, Telemetry};
use prost::Message;
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
    trace_id: String,
    attributes: HashMap<String, String>,
}

#[derive(Clone, Debug)]
struct OtlpLogRecord {
    trace_id: String,
    span_id: String,
    #[allow(dead_code)]
    severity_text: String,
    body: Option<String>,
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

/// Build a test router with telemetry.
///
/// Uses a lazy database pool so no real database connection is established.
fn test_app() -> Router {
    let telemetry = orders::telemetry::Telemetry::init("info").expect("telemetry init");
    orders::server::app(telemetry)
}

#[tokio::test(flavor = "multi_thread")]
async fn log_and_trace_share_trace_id() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test receiver should bind");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    let endpoint = format!("http://{addr}/v1/traces");

    let spans: Arc<Mutex<Vec<OtlpSpan>>> = Arc::new(Mutex::new(Vec::new()));
    let spans_svc = spans.clone();

    let logs: Arc<Mutex<Vec<OtlpLogRecord>>> = Arc::new(Mutex::new(Vec::new()));
    let logs_svc = logs.clone();

    let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
        let spans = spans_svc.clone();
        let logs = logs_svc.clone();
        async move {
            let path = req.uri().path().to_string();
            let method = req.method().clone();
            let bytes = collect_incoming(req.into_body()).await;

            if method == hyper::http::Method::POST && path == "/v1/traces" {
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
                                let trace_id = hex::encode(&span.trace_id);
                                spans
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .push(OtlpSpan {
                                        name: span.name,
                                        trace_id,
                                        attributes,
                                    });
                            }
                        }
                    }
                }
                Ok::<_, hyper::Error>(hyper::Response::new(Empty::<hyper::body::Bytes>::new()))
            } else if method == hyper::http::Method::POST && path == "/v1/logs" {
                if let Ok(request) = ExportLogsServiceRequest::decode(&bytes[..]) {
                    for resource_logs in request.resource_logs {
                        for scope_logs in resource_logs.scope_logs {
                            for log_record in scope_logs.log_records {
                                let trace_id = hex::encode(log_record.trace_id);
                                let span_id = hex::encode(log_record.span_id);
                                let severity_text = log_record.severity_text;
                                let body = log_record.body.map(|v| any_value_to_string(&v));
                                logs.lock().unwrap_or_else(|e| e.into_inner()).push(
                                    OtlpLogRecord {
                                        trace_id,
                                        span_id,
                                        severity_text,
                                        body,
                                    },
                                );
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

    // The span is held by the response body. Drop the response so the span
    // closes and is handed to the batch processor, then flush so the span
    // and log records are guaranteed to have reached the receiver.
    drop(response);
    telemetry.force_flush();

    let received_spans: Vec<_> = spans
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect();
    let span = received_spans
        .iter()
        .find(|s| {
            s.name == "http.request" && s.attributes.get("route") == Some(&"/health".to_string())
        })
        .cloned()
        .expect("expected GET /health span to be exported to OTLP receiver");
    let span_trace_id = span.trace_id.clone();

    let received_logs: Vec<_> = logs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect();
    assert!(
        !received_logs.is_empty(),
        "expected at least one log record to be exported to OTLP receiver"
    );
    let log = received_logs
        .iter()
        .find(|l| {
            l.body
                .as_ref()
                .is_some_and(|b| b.contains("health check requested"))
        })
        .expect("expected log record for health check request");
    assert_eq!(
        log.trace_id, span_trace_id,
        "log trace_id should match span trace_id"
    );
    assert!(!log.span_id.is_empty(), "log record should have span_id");

    drop(receiver);
    std::env::remove_var("RENZU_OTLP_ENDPOINT");
    telemetry.shutdown();
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
        .event_format(orders::telemetry::ServiceFields)
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
            !fields.values().any(|v| v.as_str() == Some("s3cr3t-token"))
                && !fields.values().any(|v| v.as_str() == Some("hunter2")),
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
        tracing::info!("starting orders service");
    });

    let output = String::from_utf8_lossy(&captured_clone.lock().unwrap()).to_string();
    std::env::remove_var("RENZU_ENVIRONMENT");

    let lines: Vec<&str> = output.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "expected one log line, got:\n{output}");

    let value: serde_json::Value =
        serde_json::from_str(lines[0]).expect("log line should be valid JSON");
    assert_eq!(value["service.name"], "orders");
    assert_eq!(value["service.version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["environment"], "local");
}
