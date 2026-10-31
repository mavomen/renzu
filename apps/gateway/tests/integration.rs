use axum::{
    http::{Request, StatusCode},
    Router,
};
use gateway::server::app;
use http_body_util::BodyExt;
use tower::ServiceExt;

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
