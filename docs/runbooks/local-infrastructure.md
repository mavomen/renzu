# Local Infrastructure Runbook (F-003)

## Overview

The local observability environment is orchestrated with Docker Compose and
contains two infrastructure services only (M1 slice):

- **OpenTelemetry Collector** — single OTLP ingest point (traces and metrics).
- **Jaeger** — trace storage and query UI.

No application services run here yet; the first Rust crate lands in F-004.

## Prerequisites

- Docker Engine and Docker Compose (Compose V2, `docker compose`).
- Approximately 1–2 GB RAM and 2–4 GB disk for the two containers plus image layers.
- No cloud account, paid service, or external AI API required (local-first).

## Startup

```bash
docker compose up -d
```

Startup order is enforced: the Collector starts only after Jaeger reports
healthy (`depends_on` with `condition: service_healthy`).

```bash
docker compose ps
```

Expected output: both services report `healthy`.

## Ports table

All published ports are bound to `127.0.0.1` only (PRD NFR-04).

| Service | Port (host) | Container port | Purpose |
|---------|-------------|----------------|---------|
| Jaeger | 127.0.0.1:16686 | 16686 | Jaeger query UI |
| OTel Collector | 127.0.0.1:4317 | 4317 | OTLP gRPC receiver |
| OTel Collector | 127.0.0.1:4318 | 4318 | OTLP HTTP receiver |

Jaeger's OTLP ports (4317/4318) and the Jaeger collector port (14250) are
reached by the Collector over the Docker network (`jaeger:14250`); they are not
published to the host.

## Health checks

| Service | Probe | Meaning |
|---------|-------|---------|
| Jaeger | `curl http://localhost:16686/` (HTTP 200) | Jaeger query UI is up and serving. |
| OTel Collector | `otelcol validate --config=/etc/otelcol/config.yaml` (config validation) | Collector config is valid; a valid config is what binds the OTLP ports (4317/4318). |

The otel/opentelemetry-collector image is distroless: it ships no shell and no probe
tools (nc/curl/wget), so an in-container TCP connect to port 4317 is not possible.
The healthcheck therefore validates the collector config instead. `docker compose ps`
reports `healthy` once validation passes, `starting` during the `start_period`, and
`unhealthy` after `retries` consecutive failures. An invalid config prevents the
collector from serving telemetry at all, so config validation is a meaningful proxy
for port binding.

Startup failures produce actionable diagnostics via `docker compose logs
<service>`.

## Shutdown

```bash
docker compose down
```

Removes the containers and the network. Named volumes are not used in F-003, so
no persistent data is removed by `down`. Jaeger in-memory trace data is
discarded on shutdown (expected for the M1 slice).

Verify no leftover project-owned containers:

```bash
docker ps -a --filter name=renzu-
```

## Telemetry smoke test (manual, requires Docker)

Send a test OTLP span to the Collector and confirm it appears in Jaeger:

```bash
# Export via OTLP HTTP to the Collector
curl -X POST http://127.0.0.1:4318/v1/traces \
  -H "Content-Type: application/json" \
  -d '{"resourceSpans":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"smoke-test"}}]},"spans":[{"traceId":"0123456789abcdef0123456789abcdef","spanId":"0123456789abcdef","name":"smoke-span","kind":1,"startTimeUnixNano":"1000","endTimeUnixNano":"2000","status":{"code":1}}]}]}'
```

Then open the Jaeger UI at http://127.0.0.1:16686 and query service
`smoke-test`.

## Trace Visualization (Jaeger UI)

Once the gateway has been exercised against the Collector (for example via
`scripts/e2e-trace-visualization.sh`), traces are queryable in Jaeger:

1. Open http://127.0.0.1:16686 in a browser.
2. From the "Service" dropdown, select `gateway`.
3. Click "Find Traces".
4. Inspect each trace:
   - Span `operationName` shows the instrumented route (for example `GET /health`).
   - Span `duration` shows the operation timing (microseconds; should be > 0).
   - Span tags show `http.method` and `http.route`.
   - Process tags show `service.name=gateway` (the resource attribute set by the
     gateway and passed through the Collector unchanged).

Automated verification:

```bash
# Requires docker compose up -d (jaeger + otel-collector) to be running.
./scripts/e2e-trace-visualization.sh
```

The script builds and runs the gateway, hits `GET /health`, `/ready` and `/`,
polls the Jaeger API (`http://127.0.0.1:16686/api/traces?service=gateway`),
and asserts that traces appear with `service.name=gateway` as a resource
attribute and span durations greater than zero. It exits 0 on success and
always stops the gateway process it started; compose is left running.

## Batching

Both pipelines use the `batch` processor with default settings (timeout and size).
Batching reduces the number of export requests and smooths traffic to the backends.
No custom tuning is applied yet; batching behavior is revisited if throughput or
memory pressure requires it (see docs/decisions/0005-opentelemetry-collector-pipeline.md).

## Resource attributes

Resource attributes (`service.name`, `service.version`) are set by the gateway
application (F-006 ADR) and flow through the collector unchanged. The collector
does not add a resource processor; it passes through attributes supplied by the
application. This avoids speculative abstraction and keeps attribute ownership in
the application.

## Application logging (F-005)

The gateway emits newline-delimited JSON logs to stdout (M1 behavior; they join
the Loki pipeline in F-013/M2). No log backend is provisioned here.

Each log event is a JSON object with at least these fields:

| Field | Source | Example |
|-------|--------|---------|
| `timestamp` | event time | `2026-11-01T02:00:00Z` |
| `level` | tracing level | `INFO` |
| `service.name` | constant | `gateway` |
| `service.version` | `CARGO_PKG_VERSION` | `0.0.0` |
| `environment` | `RENZU_ENVIRONMENT` (default `local`) | `local` |
| `target` | call-site module | `renzu_gateway::main` |
| `fields` | event fields | `{"message":"..."}` |

Configuration:

- `RENZU_LOG_LEVEL` (default `info`) controls verbosity via `EnvFilter`, the same
  values as tracing (`off`, `error`, `warn`, `info`, `debug`, `trace`).
- `RENZU_ENVIRONMENT` (default `local`) distinguishes deployment environments
  without provisioning a backend.

### Sensitive-data redaction

Redaction is call-site opt-in through the [`Redacted`] wrapper:

```rust
tracing::info!(auth = %Redacted(&token), "authenticating");
```

Values wrapped in `Redacted` render as `[REDACTED]`; values logged without it are
emitted as-is. Documented sensitive keys: `Authorization`, `Cookie`,
`password`, `token`, `secret`, `api_key`.

### Known limitations

- `trace_id`/`span_id` are not injected into log events at M1: tracing event
  fields are immutable at creation time, and the OpenTelemetry trace context is
  not exposed as tracing span fields. Correlation is recorded on spans (F-006)
  and the log fields can be populated once spans declare them (F-013).
- Redaction only applies where call sites opt in; unredacted sensitive data
  logged directly is not caught.


## Error handling and export failure observability

- Collector export failures are logged to the container's stdout/stderr and are
  never silently dropped. Inspect them with `docker compose logs otel-collector`.
- Gateway-side export failures are logged by the gateway (F-006 behavior) and are
  never fatal to the application.
- If the collector reports `unhealthy` in `docker compose ps`, telemetry export is
  degraded or unavailable; check `docker compose logs otel-collector` and validate
  the config with `otelcol validate --config=/etc/otelcol/config.yaml`.
- The metrics pipeline currently exports to `debug` (stdout) as a local placeholder;
  it is swapped to Prometheus in F-010. Until then, metrics are observable in the
  collector logs, not in a metrics backend.

## Known limitations

- Jaeger stores traces in memory only; data is lost on restart (M1 slice).
- Runtime validation (`docker compose up`) requires a Docker daemon and the
  ability to pull pinned images. If either is unavailable, `docker compose
  config` still validates the YAML.
- Health-check probes assume the collector image ships probe tools; the pinned
  `otel/opentelemetry-collector:0.113.0` image is distroless and ships none, so the
  collector healthcheck validates its config instead of running an in-container TCP
  connect. If the image changes to one with a shell/probe tools, the probe can be
  revisited.
