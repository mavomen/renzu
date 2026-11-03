# ADR-0005: OpenTelemetry Collector Pipeline

**Status:** Accepted
**Deciders:** Renzu maintainer
**Date:** 2026-11-01

## Context

F-003 provisions the OpenTelemetry Collector container
(`otel/opentelemetry-collector-contrib:0.113.0`) alongside Jaeger. F-006 instruments the
`gateway` to export OTLP traces over HTTP/protobuf to the collector. F-007
configures the collector pipeline so exported traces and metrics are collected,
batched, and delivered to configured backends, with observable collector health
and export failures. F-013 adds the logs pipeline and Loki backend.

The gateway sends traces to `http://127.0.0.1:4317/v1/traces` (F-006 default,
overridable via `RENZU_OTLP_ENDPOINT`). The OTLP HTTP exporter posts to the
endpoint URL as-is; the collector's `otlp` receiver listens on 0.0.0.0:4317
(gRPC) and 0.0.0.0:4318 (HTTP), published to 127.0.0.1 in compose.yaml.

The collector image is distroless: it ships no shell and no probe tools
(nc/curl/wget/true/busybox). An in-container TCP connect to port 4317 is not
possible, so a command-based port probe cannot be used as a Docker healthcheck.

The `logging` exporter referenced by the F-007 plan is a hard decode error in
collector 0.113.0 (deprecated in favor of `debug`), so it cannot be used as the
metrics placeholder with this pinned image.

## Decision

**Collector configuration** (`deploy/otel-collector/config.yaml`):

- `health_check` extension exposing `/healthz` on 0.0.0.0:13133 (internal only,
  not published to the host).
- `otlp` receiver on 0.0.0.0:4317 (gRPC) and 0.0.0.0:4318 (HTTP).
- Traces pipeline: `otlp -> batch -> otlphttp` (endpoint `http://jaeger:4318`).
- Metrics pipeline: `otlp -> batch -> prometheus` (endpoint `0.0.0.0:9464`,
  scraped by Prometheus).
- Logs pipeline: `otlp -> batch -> loki` (endpoint `http://loki:3100/loki/api/v1/push`).
- `batch` processor (default settings) on all pipelines.
- No resource attributes processor: `service.name`/`service.version` are set by
  the gateway (F-006 ADR) and pass through the collector unchanged.

**Compose healthcheck** (`compose.yaml`):

- `otel-collector` gets `healthcheck: test: ["CMD", "/otelcol", "validate",
  "--config=/etc/otelcol/config.yaml"]` with interval 10s, timeout 3s, retries 3,
  start_period 15s.
- Rationale: `otelcol validate` is the only runnable binary in the distroless
  image; exit 0 means the config is valid, which is what binds the OTLP ports.
  Result reported via `docker compose ps` (healthy/unhealthy). This is a config
  validity proxy, not a live TCP connect; documented as such in the runbook.
- `depends_on: jaeger: service_healthy` and `depends_on: loki: service_healthy` preserve startup ordering.

**Metrics exporter target:** defined in F-010. `prometheus` exporter is used; the
`debug` placeholder was temporary and has been replaced.

**Logs exporter target:** defined in F-013. `loki` exporter targets Loki's native
OTLP endpoint (`/loki/api/v1/push`). No separate log shipper is required.

**Metrics pipeline:** defined in F-007 to satisfy "define the metrics pipeline"
without depending on F-010's backend; delivery to a metrics backend is
verified in F-010.

**Logs pipeline:** defined in F-013. Applications currently write structured JSON
logs to stdout (F-005) and do not yet export OTLP logs. The Collector logs
pipeline is configured but idle until application-side OTLP log export is added
(out of scope for F-013-01).
