use reqwest::Client;
use serde_json::Value;
use std::collections::HashSet;
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

const GATEWAY_URL: &str = "http://127.0.0.1:8080";
const LOKI_URL: &str = "http://127.0.0.1:3100";
const JAEGER_URL: &str = "http://127.0.0.1:16686";
const TEST_ORDER_ID: &str = "test-correlation-123";

fn docker_compose_available() -> bool {
    let docker_version = Command::new("docker")
        .arg("version")
        .arg("--format")
        .arg("{{.Server.Version}}")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !docker_version {
        return false;
    }

    let compose_config = Command::new("docker")
        .arg("compose")
        .arg("config")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !compose_config {
        return false;
    }

    let output = match Command::new("docker")
        .arg("compose")
        .arg("config")
        .arg("--format")
        .arg("json")
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };

    if !output.status.success() {
        return false;
    }

    let config: Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let services = match config.get("services").and_then(|s| s.as_object()) {
        Some(s) => s,
        None => return false,
    };

    let images_output = match Command::new("docker")
        .arg("images")
        .arg("--format")
        .arg("{{.Repository}}:{{.Tag}}")
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };

    let available_images: HashSet<String> = String::from_utf8_lossy(&images_output.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect();

    for (_, service) in services {
        if let Some(build) = service.get("build") {
            let image_name = if let Some(ctx) = build.get("context").and_then(|c| c.as_str()) {
                let dockerfile = build
                    .get("dockerfile")
                    .and_then(|d| d.as_str())
                    .unwrap_or("Dockerfile");
                let path = std::path::Path::new(ctx).join(dockerfile);
                if path.exists() {
                    let name = std::path::Path::new(ctx)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");
                    format!("renzu-{name}:latest")
                } else {
                    continue;
                }
            } else {
                continue;
            };
            if !available_images.contains(&image_name) {
                return false;
            }
        } else if let Some(image) = service.get("image").and_then(|i| i.as_str()) {
            if !available_images.contains(image) {
                return false;
            }
        }
    }

    true
}

#[allow(dead_code)]
fn workspace_root() -> std::path::PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR is set for integration tests");
    std::path::PathBuf::from(manifest_dir).join("../..")
}

#[tokio::test]
async fn log_trace_correlation_in_loki_and_jaeger() {
    if !docker_compose_available() {
        eprintln!("skipping log/trace correlation test: docker compose is not available");
        return;
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("http client");

    let request_url = format!("{GATEWAY_URL}/order/{TEST_ORDER_ID}");

    let response = client.get(&request_url).send().await;

    match response {
        Ok(resp) => {
            let _ = resp.status();
        }
        Err(e) => {
            eprintln!("request to gateway failed (orders service may be down): {e}");
        }
    };

    sleep(Duration::from_secs(5)).await;

    let loki_trace_id = query_loki_for_trace_id(&client).await;
    let jaeger_trace_id = query_jaeger_for_trace_id(&client).await;

    match (loki_trace_id, jaeger_trace_id) {
        (Some(loki_id), Some(jaeger_id)) => {
            assert_eq!(
                loki_id, jaeger_id,
                "Loki log trace_id ({loki_id}) does not match Jaeger trace trace_id ({jaeger_id})"
            );
            println!("✓ Log/trace correlation verified: trace_id = {loki_id}");
        }
        (None, Some(jaeger_id)) => {
            panic!("Loki query returned no trace_id, but Jaeger has trace_id: {jaeger_id}");
        }
        (Some(loki_id), None) => {
            panic!("Jaeger query returned no trace_id, but Loki has trace_id: {loki_id}");
        }
        (None, None) => {
            panic!("Neither Loki nor Jaeger returned a trace_id for the test request");
        }
    }
}

async fn query_loki_for_trace_id(client: &Client) -> Option<String> {
    let query = format!("{{service_name=\"gateway\"}} |~ \"{TEST_ORDER_ID}\"");
    let url = format!(
        "{LOKI_URL}/loki/api/v1/query_range?query={}&limit=10",
        urlencoding::encode(&query)
    );

    for _ in 1..=10 {
        let response = client.get(&url).send().await;
        match response {
            Ok(resp) if resp.status().is_success() => {
                let body: Value = match resp.json().await {
                    Ok(v) => v,
                    Err(_) => {
                        sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };

                if let Some(data) = body
                    .get("data")
                    .and_then(|d| d.get("result"))
                    .and_then(|r| r.as_array())
                {
                    for stream in data {
                        if let Some(values) = stream.get("values").and_then(|v| v.as_array()) {
                            for entry in values {
                                if let Some(line) = entry.get(1).and_then(|l| l.as_str()) {
                                    if let Some(trace_id) = extract_trace_id_from_log_line(line) {
                                        return Some(trace_id);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(_) => {}
        }
        sleep(Duration::from_secs(1)).await;
    }

    None
}

async fn query_jaeger_for_trace_id(client: &Client) -> Option<String> {
    let url = format!("{JAEGER_URL}/api/traces?service=gateway&limit=5");

    for _ in 1..=10 {
        let response = client.get(&url).send().await;
        match response {
            Ok(resp) if resp.status().is_success() => {
                let body: Value = match resp.json().await {
                    Ok(v) => v,
                    Err(_) => {
                        sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };

                if let Some(traces) = body.get("data").and_then(|d| d.as_array()) {
                    for trace in traces {
                        if let Some(spans) = trace.get("spans").and_then(|s| s.as_array()) {
                            for span in spans {
                                if let Some(operation) =
                                    span.get("operationName").and_then(|o| o.as_str())
                                {
                                    if operation.contains("order")
                                        || operation.contains("http.request")
                                    {
                                        if let Some(trace_id) =
                                            span.get("traceID").and_then(|t| t.as_str())
                                        {
                                            return Some(trace_id.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(_) => {}
        }
        sleep(Duration::from_secs(1)).await;
    }

    None
}

fn extract_trace_id_from_log_line(line: &str) -> Option<String> {
    let value: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return None,
    };

    if let Some(fields) = value.get("fields").and_then(|f| f.as_object()) {
        if let Some(trace_id) = fields.get("trace_id").and_then(|t| t.as_str()) {
            return Some(trace_id.to_string());
        }
    }

    if let Some(resource_logs) = value.get("resourceLogs").and_then(|r| r.as_array()) {
        for rl in resource_logs {
            if let Some(scope_logs) = rl.get("scopeLogs").and_then(|s| s.as_array()) {
                for sl in scope_logs {
                    if let Some(log_records) = sl.get("logRecords").and_then(|l| l.as_array()) {
                        for lr in log_records {
                            if let Some(trace_id) = lr.get("traceId").and_then(|t| t.as_str()) {
                                return Some(trace_id.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    None
}
