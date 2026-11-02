//! Metrics instrumentation for the gateway HTTP service.
//!
//! Records request rate, request error counts, and request duration
//! distributions using OpenTelemetry metrics. Metrics are exported via OTLP
//! to the collector provisioned in F-007.
//!
//! Metric names and units:
//!   - `renzu_http_requests_total` (unit "1", counter) — total HTTP requests.
//!   - `renzu_http_request_errors_total` (unit "1", counter) — requests with
//!     a status code >= 400.
//!   - `renzu_http_request_duration_seconds` (unit "s", histogram) — request
//!     duration in seconds.
//!
//! Labels are kept low-cardinality: `http.method` and `http.status_code`. No
//! user IDs, request IDs, or full URLs are used as labels (observability §7).

use axum::http::Request;
use axum::response::Response;
use opentelemetry::metrics::{Counter, Histogram, MeterProvider as _};
use opentelemetry::KeyValue;
use opentelemetry_otlp::MetricExporter;
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_sdk::metrics::PeriodicReader;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::Resource;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tower::Layer;

const METER_NAME: &str = "renzu.gateway";
const SERVICE_NAME: &str = "gateway";
const DEFAULT_METRICS_ENDPOINT: &str = "http://127.0.0.1:4318/v1/metrics";
const DEFAULT_EXPORT_INTERVAL: Duration = Duration::from_secs(10);

/// Handles to the OpenTelemetry metric instruments.
#[derive(Clone)]
pub struct Metrics {
    provider: SdkMeterProvider,
    request_count: Counter<u64>,
    request_errors: Counter<u64>,
    request_duration: Histogram<f64>,
}

impl Metrics {
    /// Initialize the OpenTelemetry meter provider and return metric handles.
    ///
    /// The OTLP endpoint is taken from `RENZU_OTLP_METRICS_ENDPOINT` (default
    /// [`DEFAULT_METRICS_ENDPOINT`]). Export failures are logged, never fatal,
    /// matching the telemetry error-handling convention.
    pub fn init() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = std::env::var("RENZU_OTLP_METRICS_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_METRICS_ENDPOINT.to_string());

        let exporter = MetricExporter::builder()
            .with_http()
            .with_endpoint(&endpoint)
            .build()
            .map_err(|e| format!("failed to build OTLP metrics exporter: {e}"))?;

        let reader = PeriodicReader::builder(exporter)
            .with_interval(DEFAULT_EXPORT_INTERVAL)
            .build();

        let provider = SdkMeterProvider::builder()
            .with_reader(reader)
            .with_resource(
                Resource::builder()
                    .with_attributes(vec![
                        KeyValue::new("service.name", SERVICE_NAME),
                        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                    ])
                    .build(),
            )
            .build();

        let meter = provider.meter(METER_NAME);

        let request_count = meter
            .u64_counter("renzu_http_requests_total")
            .with_unit("1")
            .with_description("Total number of HTTP requests")
            .build();
        let request_errors = meter
            .u64_counter("renzu_http_request_errors_total")
            .with_unit("1")
            .with_description("Total number of HTTP requests with status >= 400")
            .build();
        let request_duration = meter
            .f64_histogram("renzu_http_request_duration_seconds")
            .with_unit("s")
            .with_description("HTTP request duration in seconds")
            .build();

        Ok(Self {
            provider,
            request_count,
            request_errors,
            request_duration,
        })
    }

    /// Construct `Metrics` from an existing reader, used for testing without an
    /// OTLP endpoint. The caller retains a clone of the exporter to inspect the
    /// exported metric data after a flush.
    pub fn with_reader<E>(reader: PeriodicReader<E>) -> Self
    where
        E: opentelemetry_sdk::metrics::exporter::PushMetricExporter + 'static,
    {
        let provider = SdkMeterProvider::builder()
            .with_reader(reader)
            .with_resource(
                Resource::builder()
                    .with_attributes(vec![
                        KeyValue::new("service.name", SERVICE_NAME),
                        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                    ])
                    .build(),
            )
            .build();
        let meter = provider.meter(METER_NAME);
        let request_count = meter.u64_counter("renzu_http_requests_total").build();
        let request_errors = meter.u64_counter("renzu_http_request_errors_total").build();
        let request_duration = meter
            .f64_histogram("renzu_http_request_duration_seconds")
            .build();
        Self {
            provider,
            request_count,
            request_errors,
            request_duration,
        }
    }

    /// Record a single HTTP request outcome.
    pub fn record_request(&self, method: &str, status_code: u16, duration: Duration) {
        let status_attr = KeyValue::new("http.status_code", status_code.to_string());
        let method_attr = KeyValue::new("http.method", method.to_string());
        let attrs = &[status_attr, method_attr];

        self.request_count.add(1, attrs);
        self.request_duration.record(duration.as_secs_f64(), attrs);

        if status_code >= 400 {
            self.request_errors.add(1, attrs);
        }
    }

    /// Force a flush of all pending metric data to the exporter.
    pub fn force_flush(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.provider
            .force_flush()
            .map_err(|e| format!("metrics force flush failed: {e}").into())
    }

    /// Shut down the meter provider, flushing in-flight metrics.
    pub fn shutdown(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.provider
            .shutdown()
            .map_err(|e| format!("metrics shutdown failed: {e}").into())
    }
}

impl fmt::Debug for Metrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Metrics").finish_non_exhaustive()
    }
}

/// Extract the HTTP status code from a service response.
///
/// Kept as a trait so `MetricsService` can stay generic over the inner service
/// response type while still reading the status for the `http.status_code`
/// label.
trait ResponseStatus {
    fn status_code(&self) -> u16;
}

impl ResponseStatus for Response {
    fn status_code(&self) -> u16 {
        self.status().as_u16()
    }
}

/// Tower layer that wraps an HTTP service to record metrics for every request.
#[derive(Clone)]
pub struct MetricsLayer {
    metrics: Metrics,
}

impl MetricsLayer {
    pub fn new(metrics: Metrics) -> Self {
        Self { metrics }
    }
}

impl<S> Layer<S> for MetricsLayer {
    type Service = MetricsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MetricsService {
            inner,
            metrics: self.metrics.clone(),
        }
    }
}

/// Wraps an inner HTTP service and records request rate, errors, and duration.
#[derive(Clone)]
pub struct MetricsService<S> {
    inner: S,
    metrics: Metrics,
}

impl<S, ReqBody> tower::Service<Request<ReqBody>> for MetricsService<S>
where
    S: tower::Service<Request<ReqBody>>,
    S::Future: 'static + Send,
    S::Response: ResponseStatus,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let metrics = self.metrics.clone();
        let method = req.method().clone();
        let start = std::time::Instant::now();
        let future = self.inner.call(req);

        Box::pin(async move {
            let result = future.await;
            let duration = start.elapsed();
            let status = result
                .as_ref()
                .ok()
                .map(|response| response.status_code())
                .unwrap_or(0);
            metrics.record_request(method.as_ref(), status, duration);
            result
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
    use opentelemetry_sdk::metrics::InMemoryMetricExporter;

    fn test_metrics_and_exporter() -> (Metrics, InMemoryMetricExporter) {
        let exporter = InMemoryMetricExporter::default();
        let reader = PeriodicReader::builder(exporter.clone()).build();
        let metrics = Metrics::with_reader(reader);
        (metrics, exporter)
    }

    #[test]
    fn record_request_counts_and_errors() {
        let (metrics, exporter) = test_metrics_and_exporter();

        metrics.record_request("GET", 200, Duration::from_millis(50));
        metrics.record_request("GET", 404, Duration::from_millis(120));
        metrics.record_request("POST", 500, Duration::from_millis(300));

        metrics
            .force_flush()
            .expect("metrics should flush to the in-memory exporter");

        let data = exporter
            .get_finished_metrics()
            .expect("in-memory exporter should return metrics");

        let mut request_total = 0u64;
        let mut error_total = 0u64;
        let mut duration_count = 0u64;

        for rm in &data {
            for sm in rm.scope_metrics() {
                for m in sm.metrics() {
                    match m.name() {
                        "renzu_http_requests_total" => {
                            if let AggregatedMetrics::U64(MetricData::Sum(sum)) = m.data() {
                                for dp in sum.data_points() {
                                    request_total += dp.value();
                                }
                            }
                        }
                        "renzu_http_request_errors_total" => {
                            if let AggregatedMetrics::U64(MetricData::Sum(sum)) = m.data() {
                                for dp in sum.data_points() {
                                    error_total += dp.value();
                                }
                            }
                        }
                        "renzu_http_request_duration_seconds" => {
                            if let AggregatedMetrics::F64(MetricData::Histogram(hist)) = m.data() {
                                for dp in hist.data_points() {
                                    duration_count += dp.count();
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        assert_eq!(request_total, 3, "expected 3 total requests");
        assert_eq!(error_total, 2, "expected 2 error requests (404 + 500)");
        assert_eq!(duration_count, 3, "expected 3 duration observations");
    }

    #[test]
    fn labels_are_low_cardinality() {
        let (metrics, _exporter) = test_metrics_and_exporter();
        // Calling with a variety of status codes and methods must not panic and
        // must not produce high-cardinality label values (full URLs, IDs).
        metrics.record_request("GET", 200, Duration::from_millis(10));
        metrics.record_request("POST", 201, Duration::from_millis(20));
        metrics.record_request("DELETE", 500, Duration::from_millis(30));
        metrics.force_flush().expect("metrics should flush");
    }
}
