//! Load generation and measurement for `renzu load-test`.
//!
//! Generates controlled HTTP traffic against a target URL while a scenario's
//! fault is enabled on the gateway, then measures request counts, errors,
//! throughput, and latency percentiles.
//!
//! The scenario's fault is enabled before load is generated and always
//! disabled afterward, even if load generation fails. If the process is
//! interrupted (Ctrl+C) mid-test, fault cleanup is not guaranteed — this is a
//! known limitation.

use crate::scenarios::default_registry;
use crate::scenarios::runner;
use hdrhistogram::Histogram;
use reqwest::{blocking::Client as BlockingClient, Client};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::runtime::Runtime;

/// Base URL of the gateway fault-control and health API.
const GATEWAY_BASE_URL: &str = "http://127.0.0.1:8080";

/// Validation limits.
const MIN_DURATION_SECS: u64 = 1;
const MAX_DURATION_SECS: u64 = 3600;
const MIN_RATE: u64 = 1;
const MAX_RATE: u64 = 1000;
const MIN_CONCURRENCY: u64 = 1;
const MAX_CONCURRENCY: u64 = 100;

/// Per-request timeout for load-test requests.
const REQUEST_TIMEOUT_SECS: u64 = 10;

/// Highest latency the histogram tracks (nanoseconds). Must exceed the request
/// timeout so recorded latencies and coordinated-omission corrections fit.
const MAX_LATENCY_NANOS: u64 = u64::MAX;

/// Jaeger base URL from environment, defaulting to local Jaeger.
fn jaeger_base_url() -> String {
    std::env::var("RENZU_JAEGER_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:16686".to_string())
}

/// Build telemetry reference URLs (Jaeger + Grafana Explore) for a trace ID.
fn build_telemetry_refs(trace_id: &str, jaeger_base: &str) -> Vec<String> {
    let mut refs = Vec::with_capacity(2);
    refs.push(format!(
        "{}/trace/{}",
        jaeger_base.trim_end_matches('/'),
        trace_id
    ));
    // Grafana Explore URL — reasonable default using Jaeger datasource name "Jaeger".
    // Query format: /explore?orgId=1&left=["now-1h","now","Jaeger",{"query":"traceID","traceID":"<id>"}]
    let grafana_url = format!(
        "{}/explore?orgId=1&left=[\"now-1h\",\"now\",\"Jaeger\",{{\"query\":\"traceID\",\"traceID\":\"{}\"}}]",
        jaeger_base.trim_end_matches('/').replace(":16686", ":3000"),
        trace_id
    );
    refs.push(grafana_url);
    refs
}

/// Generate a W3C traceparent header value: `00-<32-hex-trace-id>-<16-hex-span-id>-01`.
fn generate_traceparent() -> (String, String) {
    let trace_id: String = (0..16)
        .map(|_| format!("{:02x}", rand::random::<u8>()))
        .collect();
    let span_id: String = (0..8)
        .map(|_| format!("{:02x}", rand::random::<u8>()))
        .collect();
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);
    (trace_id, traceparent)
}

/// Per-request trace metadata collected during load generation.
#[derive(Debug, Clone)]
struct TraceRecord {
    trace_id: String,
    latency_nanos: u64,
    success: bool,
}

/// Select trace IDs for telemetry references: all errors + slowest N (default 5),
/// deduplicated. Returns Jaeger + Grafana Explore URLs for each selected trace ID.
fn select_telemetry_references(records: &[TraceRecord]) -> Vec<String> {
    if records.is_empty() {
        return Vec::new();
    }

    const SLOWEST_N: usize = 5;

    let error_ids: Vec<String> = records
        .iter()
        .filter(|r| !r.success)
        .map(|r| r.trace_id.clone())
        .collect();

    let mut sorted_by_latency = records.to_vec();
    sorted_by_latency.sort_by_key(|b| std::cmp::Reverse(b.latency_nanos));
    let slowest_ids: Vec<String> = sorted_by_latency
        .iter()
        .take(SLOWEST_N)
        .map(|r| r.trace_id.clone())
        .collect();

    // Deduplicate: combine error + slowest, unique
    let mut all_ids = error_ids;
    all_ids.extend(slowest_ids);
    all_ids.sort();
    all_ids.dedup();

    let jaeger_base = jaeger_base_url();
    let mut refs = Vec::new();
    for trace_id in all_ids {
        refs.extend(build_telemetry_refs(&trace_id, &jaeger_base));
    }
    refs
}

/// Inject traceparent header into a request builder.
fn with_traceparent(
    builder: reqwest::RequestBuilder,
    traceparent: &str,
) -> reqwest::RequestBuilder {
    builder.header("traceparent", traceparent)
}

/// Workload shape: either a fixed request rate or a concurrency level.
#[derive(Debug, Clone)]
pub enum Workload {
    /// Send `rps` requests per second, paced evenly.
    Rate { rps: u64 },
    /// Run `n` threads sending requests as fast as possible.
    Concurrency { n: u64 },
}

/// Configuration for a single load test.
#[derive(Debug, Clone)]
pub struct LoadTestConfig {
    /// Name of the scenario whose fault is enabled during the test.
    pub scenario: String,
    /// How long to generate load.
    pub duration: Duration,
    /// Workload shape (rate or concurrency).
    pub workload: Workload,
    /// Target URL for each request.
    pub url: String,
}

/// Statistics describing observed request latencies.
#[derive(Debug, Clone)]
pub struct LatencyStats {
    pub count: usize,
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

/// Outcome of a load test run.
#[derive(Debug, Clone)]
pub struct LoadTestResult {
    /// Unique identifier assigned to this run (set by the CLI before report
    /// writing; empty while the result is still in loadgen's ownership).
    pub run_id: String,
    /// Whether the workload finished and fault cleanup was verified.
    pub completion_status: CompletionStatus,
    pub config: LoadTestConfig,
    pub total: u64,
    pub successful: u64,
    pub failed: u64,
    pub throughput_rps: f64,
    pub latency: LatencyStats,
    /// Deep links to traces for slowest/error requests: Jaeger and Grafana Explore URLs.
    pub telemetry_references: Vec<String>,
}

/// Completion status of an experiment run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompletionStatus {
    /// The workload finished and fault cleanup was verified.
    Complete,
    /// The run was interrupted or cleanup failed.
    Incomplete,
}

impl LoadTestResult {
    /// Render the result as a human-readable string for stdout.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Load test results\n");
        out.push_str("=================\n");
        out.push_str(&format!("Scenario: {}\n", self.config.scenario));
        out.push_str(&format!("URL: {}\n", self.config.url));
        out.push_str(&format!("Duration: {}s\n", self.config.duration.as_secs()));
        match &self.config.workload {
            Workload::Rate { rps } => {
                out.push_str(&format!("Workload: {rps} req/s\n"));
            }
            Workload::Concurrency { n } => {
                out.push_str(&format!("Workload: concurrency {n}\n"));
            }
        }
        out.push('\n');
        out.push_str("Requests:\n");
        out.push_str(&format!("  total: {}\n", self.total));
        out.push_str(&format!("  successful: {}\n", self.successful));
        out.push_str(&format!("  failed: {}\n", self.failed));
        out.push('\n');
        out.push_str(&format!("Throughput: {:.2} req/s\n", self.throughput_rps));
        out.push('\n');
        out.push_str("Latency (ms):\n");
        out.push_str(&format!("  samples: {}\n", self.latency.count));
        out.push_str(&format!("  min: {:.2}\n", self.latency.min_ms));
        out.push_str(&format!("  max: {:.2}\n", self.latency.max_ms));
        out.push_str(&format!("  mean: {:.2}\n", self.latency.mean_ms));
        out.push_str(&format!("  p50: {:.2}\n", self.latency.p50_ms));
        out.push_str(&format!("  p95: {:.2}\n", self.latency.p95_ms));
        out.push_str(&format!("  p99: {:.2}\n", self.latency.p99_ms));
        out.push('\n');
        out.push_str("Note: percentiles are approximate with few samples.\n");
        out
    }
}

/// Errors produced by the load generator.
#[derive(Debug, Error)]
pub enum LoadTestError {
    #[error("unknown scenario: {0}")]
    UnknownScenario(String),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("gateway health check failed: {0}")]
    GatewayUnreachable(String),
    #[error("failed to enable fault: {0}")]
    EnableFault(String),
    #[error("HTTP error: {0}")]
    HttpClient(String),
}

impl LoadTestError {
    /// Convert into a [`CliError`](crate::CliError) with the appropriate exit code.
    pub fn to_cli_error(&self) -> crate::CliError {
        match self {
            LoadTestError::InvalidConfig(_) | LoadTestError::UnknownScenario(_) => {
                crate::CliError::Usage(self.to_string())
            }
            LoadTestError::GatewayUnreachable(_) | LoadTestError::EnableFault(_) => {
                crate::CliError::Orchestration(self.to_string())
            }
            LoadTestError::HttpClient(_) => crate::CliError::Internal(self.to_string()),
        }
    }
}

/// Validate a load-test configuration, rejecting unsafe values.
///
/// # Errors
/// Returns [`LoadTestError::InvalidConfig`] for out-of-range duration, rate,
/// or concurrency, and [`LoadTestError::UnknownScenario`] when the scenario
/// is not present in the registry.
pub fn validate_config(config: &LoadTestConfig) -> Result<(), LoadTestError> {
    let duration_secs = config.duration.as_secs();
    if !(MIN_DURATION_SECS..=MAX_DURATION_SECS).contains(&duration_secs) {
        return Err(LoadTestError::InvalidConfig(format!(
            "duration must be between {MIN_DURATION_SECS} and {MAX_DURATION_SECS} seconds, got {duration_secs}"
        )));
    }

    let registry = default_registry();
    if registry.find(&config.scenario).is_none() {
        return Err(LoadTestError::UnknownScenario(config.scenario.clone()));
    }

    match &config.workload {
        Workload::Rate { rps } => {
            if *rps < MIN_RATE || *rps > MAX_RATE {
                return Err(LoadTestError::InvalidConfig(format!(
                    "rate must be between {MIN_RATE} and {MAX_RATE} req/s, got {rps}"
                )));
            }
        }
        Workload::Concurrency { n } => {
            if *n < MIN_CONCURRENCY || *n > MAX_CONCURRENCY {
                return Err(LoadTestError::InvalidConfig(format!(
                    "concurrency must be between {MIN_CONCURRENCY} and {MAX_CONCURRENCY}, got {n}"
                )));
            }
        }
    }

    Ok(())
}

/// Enable the scenario's fault, generate load, then always disable the fault.
///
/// # Errors
/// Returns a [`LoadTestError`] if the gateway is unreachable, the fault cannot
/// be enabled or disabled, or load generation fails. The fault is always
/// disabled on the way out, even when load generation fails.
pub fn run_load_test(config: &LoadTestConfig) -> Result<LoadTestResult, LoadTestError> {
    validate_config(config)?;

    let registry = default_registry();
    let scenario = registry
        .find(&config.scenario)
        .ok_or_else(|| LoadTestError::UnknownScenario(config.scenario.clone()))?;

    // Blocking client for gateway health checks and fault enable/disable.
    // The scenarios runner keeps using this blocking client unchanged.
    let client = BlockingClient::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| LoadTestError::HttpClient(e.to_string()))?;

    // Verify the gateway is up before enabling any fault.
    let health_url = format!("{GATEWAY_BASE_URL}/health");
    let health_response = client
        .get(&health_url)
        .send()
        .map_err(|e| LoadTestError::GatewayUnreachable(e.to_string()))?;
    if !health_response.status().is_success() {
        return Err(LoadTestError::GatewayUnreachable(format!(
            "health check returned {}",
            health_response.status()
        )));
    }

    // Enable the scenario's fault (if any), then always clean up.
    runner::enable_fault(&client, scenario).map_err(LoadTestError::EnableFault)?;

    // Async client for load generation, driven by a Tokio runtime created
    // here so `main.rs` can stay synchronous.
    let runtime = Runtime::new().map_err(|e| LoadTestError::HttpClient(e.to_string()))?;
    let async_client = Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| LoadTestError::HttpClient(e.to_string()))?;
    let config = config.clone();
    let generate_result =
        runtime.block_on(async move { generate_load(&async_client, &config).await });

    let disable_result = runner::disable_fault(&client, scenario);

    let mut result = match generate_result {
        Ok(result) => result,
        Err(e) => {
            // Best-effort cleanup on failure; the generation error takes priority.
            let _ = disable_result;
            return Err(e);
        }
    };

    // Completion status: complete only if the workload finished and cleanup
    // was verified (ARCHITECTURE §6). A failed cleanup is reported as
    // incomplete, never silenced, but it is not a hard error — the experiment
    // did run and produced a result.
    result.completion_status = if disable_result.is_ok() {
        CompletionStatus::Complete
    } else {
        CompletionStatus::Incomplete
    };

    Ok(result)
}

/// Generate load against the target URL for the configured duration.
async fn generate_load(
    client: &Client,
    config: &LoadTestConfig,
) -> Result<LoadTestResult, LoadTestError> {
    let start = Instant::now();
    let deadline = start + config.duration;

    let mut histogram = Histogram::<u64>::new_with_max(MAX_LATENCY_NANOS, 3)
        .map_err(|e| LoadTestError::HttpClient(format!("failed to create histogram: {e}")))?;
    let mut total: u64 = 0;
    let mut successful: u64 = 0;
    let mut trace_records: Vec<TraceRecord> = Vec::new();

    match &config.workload {
        Workload::Rate { rps } => {
            run_rate_limited(
                client,
                config,
                *rps,
                &deadline,
                &mut histogram,
                &mut total,
                &mut successful,
                &mut trace_records,
            )
            .await?
        }
        Workload::Concurrency { n } => {
            run_concurrent(
                client,
                config,
                *n,
                &deadline,
                &mut histogram,
                &mut total,
                &mut successful,
                &mut trace_records,
            )
            .await?
        }
    }

    let elapsed = start.elapsed();
    let failed = total.saturating_sub(successful);
    let throughput_rps = if elapsed.as_secs_f64() > 0.0 {
        total as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };
    let latency = compute_latency_stats_from_histogram(&histogram);
    let telemetry_references = select_telemetry_references(&trace_records);

    Ok(LoadTestResult {
        run_id: String::new(),
        completion_status: CompletionStatus::Incomplete,
        config: config.clone(),
        total,
        successful,
        failed,
        throughput_rps,
        latency,
        telemetry_references,
    })
}

#[allow(clippy::too_many_arguments)]
/// Send requests at a fixed rate, paced evenly across the duration.
///
/// Uses `tokio::time::interval`, which fires at fixed absolute times regardless
/// of how long each request takes (open-loop). Slow responses therefore leave
/// gaps that `record_correct` fills in, preserving tail-latency visibility.
async fn run_rate_limited(
    client: &Client,
    config: &LoadTestConfig,
    rps: u64,
    deadline: &Instant,
    histogram: &mut Histogram<u64>,
    total: &mut u64,
    successful: &mut u64,
    trace_records: &mut Vec<TraceRecord>,
) -> Result<(), LoadTestError> {
    let pace = Duration::from_secs_f64(1.0 / rps as f64);
    let interval_nanos = pace.as_nanos() as u64;
    let mut ticker = tokio::time::interval(pace);

    while Instant::now() < *deadline {
        ticker.tick().await;
        if Instant::now() >= *deadline {
            break;
        }
        let (trace_id, traceparent) = generate_traceparent();
        let start = Instant::now();
        let result = with_traceparent(client.get(&config.url), &traceparent)
            .send()
            .await;
        let latency_nanos = start.elapsed().as_nanos() as u64;
        *total += 1;
        let success = result
            .as_ref()
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if success {
            *successful += 1;
        }
        trace_records.push(TraceRecord {
            trace_id,
            latency_nanos,
            success,
        });
        let _ = histogram.record_correct(latency_nanos, interval_nanos).ok();
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
/// Run `n` tasks, each sending requests back-to-back until the deadline.
///
/// Each task records latencies into a local vector, then merges them into the
/// shared histogram once all tasks finish.
async fn run_concurrent(
    client: &Client,
    config: &LoadTestConfig,
    n: u64,
    deadline: &Instant,
    histogram: &mut Histogram<u64>,
    total: &mut u64,
    successful: &mut u64,
    trace_records: &mut Vec<TraceRecord>,
) -> Result<(), LoadTestError> {
    let url = config.url.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(Vec<TraceRecord>, u64, u64)>(n as usize);
    let mut handles = Vec::with_capacity(n as usize);

    for _ in 0..n {
        let tx = tx.clone();
        let client = client.clone();
        let url = url.clone();
        let deadline = *deadline;
        let handle = tokio::spawn(async move {
            let mut local_records = Vec::new();
            let mut local_total: u64 = 0;
            let mut local_successful: u64 = 0;
            while Instant::now() < deadline {
                let (trace_id, traceparent) = generate_traceparent();
                let start = Instant::now();
                let result = with_traceparent(client.get(&url), &traceparent)
                    .send()
                    .await;
                let latency_nanos = start.elapsed().as_nanos() as u64;
                local_total += 1;
                let success = result
                    .as_ref()
                    .map(|r| r.status().is_success())
                    .unwrap_or(false);
                if success {
                    local_successful += 1;
                }
                local_records.push(TraceRecord {
                    trace_id,
                    latency_nanos,
                    success,
                });
            }
            let _ = tx
                .send((local_records, local_total, local_successful))
                .await;
        });
        handles.push(handle);
    }

    drop(tx);
    while let Some((mut local, local_total, local_successful)) = rx.recv().await {
        for record in local.drain(..) {
            histogram.saturating_record(record.latency_nanos);
            trace_records.push(record);
        }
        *total += local_total;
        *successful += local_successful;
    }

    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}

/// Compute latency statistics from a histogram of nanosecond values, converting
/// to milliseconds for the [`LatencyStats`] output.
fn compute_latency_stats_from_histogram(histogram: &Histogram<u64>) -> LatencyStats {
    if histogram.is_empty() {
        return LatencyStats {
            count: 0,
            min_ms: 0.0,
            max_ms: 0.0,
            mean_ms: 0.0,
            p50_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
        };
    }

    let count = histogram.len() as usize;
    let nanos_to_ms = |nanos: u64| nanos as f64 / 1_000_000.0;

    LatencyStats {
        count,
        min_ms: nanos_to_ms(histogram.min()),
        max_ms: nanos_to_ms(histogram.max()),
        mean_ms: histogram.mean() / 1_000_000.0,
        p50_ms: nanos_to_ms(histogram.value_at_percentile(50.0)),
        p95_ms: nanos_to_ms(histogram.value_at_percentile(95.0)),
        p99_ms: nanos_to_ms(histogram.value_at_percentile(99.0)),
    }
}

/// Compute latency statistics from observed latencies in milliseconds.
///
/// Retained as public API (and unit-tested); production load generation now
/// feeds [`LatencyStats`] directly from the HdrHistogram.
#[allow(dead_code)]
pub fn compute_latency_stats(latencies: &[f64]) -> LatencyStats {
    if latencies.is_empty() {
        return LatencyStats {
            count: 0,
            min_ms: 0.0,
            max_ms: 0.0,
            mean_ms: 0.0,
            p50_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
        };
    }

    let mut sorted = latencies.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let count = sorted.len();
    let min = sorted[0];
    let max = sorted[count - 1];
    let mean = sorted.iter().sum::<f64>() / count as f64;

    LatencyStats {
        count,
        min_ms: min,
        max_ms: max,
        mean_ms: mean,
        p50_ms: percentile(&sorted, 50.0),
        p95_ms: percentile(&sorted, 95.0),
        p99_ms: percentile(&sorted, 99.0),
    }
}

/// Nearest-rank percentile of a sorted slice.
///
/// Returns `0.0` for an empty slice.
///
/// Retained as public API helper (unit-tested); production load generation now
/// uses the HdrHistogram directly.
#[allow(dead_code)]
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> LoadTestConfig {
        LoadTestConfig {
            scenario: "baseline".to_string(),
            duration: Duration::from_secs(5),
            workload: Workload::Rate { rps: 10 },
            url: "http://127.0.0.1:8080/health".to_string(),
        }
    }

    #[test]
    fn validate_config_accepts_valid_config() {
        assert!(validate_config(&valid_config()).is_ok());
    }

    #[test]
    fn validate_config_rejects_duration_zero() {
        let mut config = valid_config();
        config.duration = Duration::from_secs(0);
        let err = validate_config(&config).unwrap_err();
        assert!(matches!(err, LoadTestError::InvalidConfig(_)));
    }

    #[test]
    fn validate_config_rejects_duration_too_large() {
        let mut config = valid_config();
        config.duration = Duration::from_secs(3601);
        let err = validate_config(&config).unwrap_err();
        assert!(matches!(err, LoadTestError::InvalidConfig(_)));
    }

    #[test]
    fn validate_config_rejects_rate_zero() {
        let mut config = valid_config();
        config.workload = Workload::Rate { rps: 0 };
        let err = validate_config(&config).unwrap_err();
        assert!(matches!(err, LoadTestError::InvalidConfig(_)));
    }

    #[test]
    fn validate_config_rejects_rate_too_large() {
        let mut config = valid_config();
        config.workload = Workload::Rate { rps: 1001 };
        let err = validate_config(&config).unwrap_err();
        assert!(matches!(err, LoadTestError::InvalidConfig(_)));
    }

    #[test]
    fn validate_config_rejects_concurrency_zero() {
        let mut config = valid_config();
        config.workload = Workload::Concurrency { n: 0 };
        let err = validate_config(&config).unwrap_err();
        assert!(matches!(err, LoadTestError::InvalidConfig(_)));
    }

    #[test]
    fn validate_config_rejects_concurrency_too_large() {
        let mut config = valid_config();
        config.workload = Workload::Concurrency { n: 101 };
        let err = validate_config(&config).unwrap_err();
        assert!(matches!(err, LoadTestError::InvalidConfig(_)));
    }

    #[test]
    fn validate_config_rejects_unknown_scenario() {
        let mut config = valid_config();
        config.scenario = "no-such-scenario".to_string();
        let err = validate_config(&config).unwrap_err();
        assert!(matches!(err, LoadTestError::UnknownScenario(_)));
    }

    #[test]
    fn compute_latency_stats_empty() {
        let stats = compute_latency_stats(&[]);
        assert_eq!(stats.count, 0);
        assert_eq!(stats.min_ms, 0.0);
        assert_eq!(stats.max_ms, 0.0);
        assert_eq!(stats.mean_ms, 0.0);
        assert_eq!(stats.p50_ms, 0.0);
        assert_eq!(stats.p95_ms, 0.0);
        assert_eq!(stats.p99_ms, 0.0);
    }

    #[test]
    fn compute_latency_stats_known_values() {
        let latencies: Vec<f64> = (1..=10).map(|v| v as f64).collect();
        let stats = compute_latency_stats(&latencies);
        assert_eq!(stats.count, 10);
        assert_eq!(stats.min_ms, 1.0);
        assert_eq!(stats.max_ms, 10.0);
        assert!((stats.mean_ms - 5.5).abs() < f64::EPSILON);
        assert_eq!(stats.p50_ms, 5.0);
        assert_eq!(stats.p95_ms, 10.0);
        assert_eq!(stats.p99_ms, 10.0);
    }

    #[test]
    fn percentile_edge_cases() {
        assert_eq!(percentile(&[], 50.0), 0.0);
        let single = vec![42.0];
        assert_eq!(percentile(&single, 50.0), 42.0);
        assert_eq!(percentile(&single, 100.0), 42.0);
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&values, 50.0), 3.0);
        assert_eq!(percentile(&values, 100.0), 5.0);
    }

    #[test]
    fn to_cli_error_maps_invalid_config_to_usage() {
        let err = LoadTestError::InvalidConfig("bad".to_string());
        match err.to_cli_error() {
            crate::CliError::Usage(_) => {}
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    #[test]
    fn to_cli_error_maps_unknown_scenario_to_usage() {
        let err = LoadTestError::UnknownScenario("nope".to_string());
        match err.to_cli_error() {
            crate::CliError::Usage(_) => {}
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    #[test]
    fn to_cli_error_maps_gateway_to_orchestration() {
        let err = LoadTestError::GatewayUnreachable("down".to_string());
        match err.to_cli_error() {
            crate::CliError::Orchestration(_) => {}
            other => panic!("expected Orchestration, got {other:?}"),
        }
    }
}
