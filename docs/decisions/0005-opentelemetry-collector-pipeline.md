# ADR-0005: OpenTelemetry Collector Pipeline

**Status:** Accepted
**Deciders:** Renzu maintainer
**Date:** 2026-11-01

## Context

F-003 provisions the OpenTelemetry Collector container
(`otel/opentelemetry-collector:0.113.0`) alongside Jaeger. F-006 instruments the
`gateway` to export OTLP traces over HTTP/protobuf to the collector. F-007
configures the collector pipeline so exported traces and metrics are collected,
batched, and delivered to configured backends, with observable collector health
and export failures.

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
- Metrics pipeline: `otlp -> batch -> debug` (stdout placeholder, local only).
- `batch` processor (default settings) on both pipelines.
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
- `depends_on: jaeger: service_healthy` preserves startup ordering.

**Metrics exporter target:** deferred to F-010. `debug` (stdout) is the local
placeholder; F-010 swaps it for Prometheus once that backend exists. Documented
as temporary in the runbook and ADR.

**Metrics pipeline:** defined now (F-007) to satisfy "define the metrics
pipeline" without depending on F-010's backend; delivery to a metrics backend is
verified in F-010.

## Consequences

### Positive

- Traces flow end-to-end: gateway OTLP HTTP -> collector `otlp` receiver ->
  `batch` -> `otlphttp` -> Jaeger OTLP HTTP receiver (jaeger:4318).
- `batch` reduces export request count and smooths traffic to Jaeger.
- Collector health observable via `docker compose ps`; startup ordering enforced
  by `depends_on` on healthy Jaeger.
- Export failures are logged (collector stdout/stderr; gateway stderr) and never
  silently discarded or fatal (AGENTS.md §7).

### Negative / trade-offs

- `otelcol validate` healthcheck validates config statically; it does not confirm
  the running process is actively listening on 4317. An invalid config prevents
  the container from serving, so validity is a meaningful proxy for port binding.
- `debug` exporter writes metrics to stdout only; metrics are not stored or
  queryable until F-010 (Prometheus).
- `logging` exporter unavailable in 0.113.0; `debug` used instead. If a future
  collector version re-adds or re-deprecates `debug`, the placeholder exporter
  must be revisited.
- No resource attributes processor in the collector; attribute ownership stays in
  the gateway. Drift between F-006/F-008/F-010 conventions must be managed by
  the application.

## Related decisions

- F-003: Local Infrastructure Orchestration (collector container).
- F-006: Distributed Trace Instrumentation (gateway OTLP export).
- F-007: OpenTelemetry Collector Pipeline (this work).
- F-008: Distributed Trace Visualization (Jaeger backend).
- F-010: Prometheus metrics backend (swaps `debug` for Prometheus).
- ADR-0004: OpenTelemetry Rust Crate Selection (resource attributes, span naming,
  shutdown behavior).
- AGENTS.md §7: observability conventions (export failures observable, never
  fatal; no high-cardinality labels).
