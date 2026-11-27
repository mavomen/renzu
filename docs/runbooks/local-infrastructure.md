# Local Infrastructure Runbook (F-003)

## Overview

The local observability environment is orchestrated with Docker Compose and
contains eight services:

- **Tempo** — trace storage and query (replaces Jaeger per ADR-06).
- **OpenTelemetry Collector** — single OTLP ingest point (traces, metrics, logs).
- **Prometheus** — metrics storage and query.
- **Grafana** — visualization UI for traces, metrics, and logs.
- **Loki** — log aggregation and query.
- **PostgreSQL** — application database for orders and gateway.
- **Orders** — application service (Rust crate).
- **Gateway** — application service (Rust crate), proxies to Orders and Postgres.

All published ports are bound to `127.0.0.1` only (PRD NFR-04).
Image tags are pinned explicitly (no `:latest`) (PRD NFR-01).

## Credentials and opt-in configuration

The base `compose.yaml` is secure by default: Grafana credentials must come
from the environment and PostgreSQL has no published host port. Local
convenience is an explicit opt-in.

### Grafana admin credentials

Grafana admin credentials are required by `compose.yaml`:

| Variable | Required value | Notes |
|----------|----------------|-------|
| `GF_SECURITY_ADMIN_USER` | caller-supplied | No default in the base Compose file |
| `GF_SECURITY_ADMIN_PASSWORD` | caller-supplied | No default in the base Compose file |

For the loopback-only lab, opt in with the example values:

```bash
cp .env.example .env
docker compose -f compose.yaml -f compose.local.yaml up -d
```

The example values are `admin` / `admin`, documented as local-only. `.env` is
ignored by Git. Outside the local lab, provide different values through the
environment or a secret manager.

### PostgreSQL host port publication

The base Compose file does not publish PostgreSQL to the host. Inter-service
communication uses Docker network DNS (`postgres:5432`) and is unaffected.

For local host-side inspection, opt in with `compose.local.yaml`:

```bash
docker compose -f compose.yaml -f compose.local.yaml up -d
```

That overlay publishes `127.0.0.1:5432:5432` (PRD NFR-04). If another service
occupies that host port, add an untracked override after the local overlay to
map a different host port, for example `15432:5432`. The proof scripts accept
that file through `RENZU_COMPOSE_OVERRIDE`.

## Prerequisites

- Docker Engine and Docker Compose (Compose V2, `docker compose`).
- Approximately 3.75 CPU cores and 3.75 GiB RAM for all containers combined (see Resource Estimates below).
- Approximately 5–10 GB disk for volumes plus image layers.
- No cloud account, paid service, or external AI API required (local-first).

## Startup

```bash
docker compose -f compose.yaml -f compose.local.yaml up -d
```

Startup order is enforced via `depends_on` with `condition: service_healthy`:
- Collector waits for Tempo and Loki.
- Prometheus waits for Collector.
- Grafana waits for Prometheus and Loki.
- Orders waits for Collector.
- Gateway waits for Postgres and Collector.

```bash
docker compose -f compose.yaml -f compose.local.yaml ps
```

Expected output: all eight services report `healthy`.

## Ports table

All published ports are bound to `127.0.0.1` only (PRD NFR-04).

| Service | Port (host) | Container port | Purpose |
|---------|-------------|----------------|---------|
| Tempo | 127.0.0.1:3200 | 3200 | Tempo HTTP API (query/ingest) |
| OTel Collector | 127.0.0.1:4317 | 4317 | OTLP gRPC receiver |
| OTel Collector | 127.0.0.1:4318 | 4318 | OTLP HTTP receiver |
| OTel Collector | 127.0.0.1:9464 | 9464 | Prometheus metrics endpoint |
| OTel Collector | 127.0.0.1:13133 | 13133 | Health check endpoint |
| Prometheus | 127.0.0.1:9090 | 9090 | Prometheus query UI + API |
| Loki | 127.0.0.1:3100 | 3100 | Loki HTTP API |
| Grafana | 127.0.0.1:3000 | 3000 | Grafana UI |
| PostgreSQL | 127.0.0.1:5432 (opt-in overlay) | 5432 | PostgreSQL host-side inspection; not published by base Compose |
| Orders | 127.0.0.1:8081 | 8081 | Orders HTTP API |
| Gateway | 127.0.0.1:8080 | 8080 | Gateway HTTP API |

Inter-service communication (e.g., Collector → Tempo, Gateway → Postgres) uses
Docker network DNS names; those ports are not published to the host.

## Health checks

| Service | Probe | Meaning |
|---------|-------|---------|
| Tempo | `wget -qO- http://localhost:3200/ready` | Tempo ready endpoint returns 200. |
| OTel Collector | `curl -f http://localhost:13133/healthz` | Collector health extension reports healthy. |
| Prometheus | `wget -qO- http://localhost:9090/-/healthy` | Prometheus healthy endpoint returns 200. |
| Loki | `wget -qO- http://localhost:3100/ready` | Loki ready endpoint returns 200. |
| Grafana | `wget -qO- http://localhost:3000/api/health` | Grafana health API returns 200. |
| PostgreSQL | `pg_isready -U $RENZU_DB_USER -d $RENZU_DB_NAME` | Postgres accepts connections. |
| Orders | `curl -fsS --max-time 2 http://127.0.0.1:8081/health` | Orders health endpoint returns 200. |
| Gateway | `curl -fsS --max-time 2 http://127.0.0.1:8080/health` | Gateway health endpoint returns 200. |

All healthchecks use `interval: 10s`, `timeout: 3s`, `retries: 3`, `start_period: 15s`.

### Application readiness

`GET /ready` on the gateway (`:8080`) and orders (`:8081`) reports `not ready`
when telemetry export has failed. It checks both span and log export failures;
the gateway also checks database connectivity and injected faults. The response
includes cumulative counters:

```json
{
  "status": "ready",
  "service": "orders",
  "span_export_failures": 0,
  "log_export_failures": 0
}
```

A non-zero counter remains until the process restarts, so `/ready` stays
degraded after an export outage. Inspect
`docker compose -f compose.yaml -f compose.local.yaml logs <service>` for
`Export.Failed.Exhausted`, `BatchLogProcessor`, or `BatchSpanProcessor` warnings.

Startup failures produce actionable diagnostics via `docker compose logs <service>`.

## Shutdown

```bash
docker compose -f compose.yaml -f compose.local.yaml down
```

Removes the containers and the network. Named volumes (`postgres_data`, `loki_data`, `tempo_data`)
are preserved; add `-v` to remove them.

Verify no leftover project-owned containers:

```bash
docker ps -a --filter name=renzu-
```

## Resource estimates

Each service declares `deploy.resources.limits` in `compose.yaml`. The values below
are **estimates**, not measured capacities. They serve as a starting point for
local development; the maintainer should measure actual usage under load and tune
accordingly. Too-low limits may cause OOM kills or CPU throttling.

### Per-service limits

| Service | CPU (cores) | Memory | Disk (volume) | Notes |
|---------|-------------|--------|---------------|-------|
| Tempo | 0.5 | 512 MiB | `tempo_data` (persistent) | Trace storage grows with retention. |
| OTel Collector | 0.5 | 512 MiB | — (no volume) | Build-based image; buffers in memory. |
| Prometheus | 1.0 | 1 GiB | — (config-only) | TSDB in memory; add volume for persistence if needed. |
| Loki | 0.5 | 512 MiB | `loki_data` (persistent) | Log chunks + index. |
| Grafana | 0.25 | 256 MiB | — (provisioning read-only) | Dashboards/provisioning via bind mounts. |
| PostgreSQL | 0.5 | 512 MiB | `postgres_data` (persistent) | Database files; grows with data. |
| Orders | 0.25 | 256 MiB | — (no volume) | Application crate; stateless. |
| Gateway | 0.25 | 256 MiB | — (no volume) | Application crate; stateless. |

### Total environment footprint (limits)

| Resource | Sum of limits |
|----------|---------------|
| CPU | 3.75 cores |
| Memory | 3.75 GiB |
| Disk (volumes) | Variable — depends on trace/log/DB retention |

> **Note:** Docker Compose `deploy.resources.limits` are enforced by the container
> runtime (cgroups). The host must have at least the summed CPU and memory
> available. If the host has fewer resources, containers will be throttled or
> OOM-killed. These estimates assume a development machine with ≥ 4 CPU cores
> and ≥ 8 GiB RAM.

## Telemetry smoke test (manual, requires Docker)

Send a test OTLP span to the Collector and confirm it appears in Tempo (via Grafana):

```bash
# Export via OTLP HTTP to the Collector
curl -X POST http://127.0.0.1:4318/v1/traces \
  -H "Content-Type: application/json" \
  -d '{"resourceSpans":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"smoke-test"}}]},"spans":[{"traceId":"0123456789abcdef0123456789abcdef","spanId":"0123456789abcdef","name":"smoke-span","kind":1,"startTimeUnixNano":"1000","endTimeUnixNano":"2000","status":{"code":1}}]}]}'
```

Then open Grafana at http://127.0.0.1:3000 (login with `GF_SECURITY_ADMIN_USER`/`GF_SECURITY_ADMIN_PASSWORD`, default `admin/admin`), navigate to Explore, select the provisioned Tempo data source, and query service `smoke-test`.

## Trace Visualization (Tempo + Grafana)

Once the gateway has been exercised against the Collector (for example via
`scripts/e2e-trace-visualization.sh`), traces are queryable in Grafana:

1. Open http://127.0.0.1:3000 in a browser (login with `GF_SECURITY_ADMIN_USER`/`GF_SECURITY_ADMIN_PASSWORD`, default `admin/admin`).
2. From the "Explore" view, select the Tempo data source.
3. From the "Service" dropdown, select `gateway`.
4. Click "Run query".
5. Inspect each trace:
   - Span `operationName` shows the instrumented route (for example `GET /health`).
   - Span `duration` shows the operation timing (should be > 0).
   - Span tags show `http.method` and `http.route`.
   - Resource attributes show `service.name=gateway`.

Automated verification:

```bash
# Requires the local opt-in Compose stack to be running.
./scripts/e2e-trace-visualization.sh
```

The script sends a synthetic OTLP trace directly to Tempo's HTTP receiver and
asserts the trace is queryable by ID with a named span and a positive duration.
It exits 0 on success; compose is left running.

### Log/trace correlation E2E (F-013)

```bash
# Requires the local opt-in Compose stack to be running.
./scripts/e2e-log-trace-correlation.sh
```

This exercises the real application and Collector path: it requests
`GET /order/<unique-id>` on the gateway, finds the resulting application log in
Loki by that unique ID, reads the log's native `traceid`, and asserts that the
same trace ID is retrievable from Tempo with a span for the same order ID. It
exits 0 on success; compose is left running.

In Grafana, the Loki data source provisions a derived field named `TraceID` for
log lines containing `"traceid":"<id>"`. Selecting that link opens the trace in
the provisioned Tempo data source.

### Metrics and report E2E

```bash
# Requires the local opt-in Compose stack to be running.
./scripts/e2e-metrics-report.sh
```

This generates gateway traffic, polls the Prometheus API until
`renzu_http_requests_total` is queryable with a positive value, runs a short
baseline load test, and asserts the saved JSON report exists with schema v1 and
a matching run ID. The gateway exports metrics to the Collector through
`RENZU_OTLP_METRICS_ENDPOINT`, which `compose.yaml` sets to the Collector's
OTLP/HTTP receiver. The script exits 0 on success; compose is left running.

### Incident investigation journey E2E

```bash
# Requires the local opt-in Compose stack to be running.
./scripts/e2e-incident-journey.sh
```

This reproduces PRD Journey B. It runs a baseline load test and a `db-latency`
load test against the gateway's `/ready` endpoint, where the 200 ms fault is
injected. The script uses an isolated temporary reports directory and asserts:

- both runs produce schema-v1 reports with the expected scenario names;
- the faulted p95 latency exceeds the baseline by at least 150 ms;
- `renzu compare` reports a positive p95 delta;
- `/ready` latency returns to normal after fault cleanup;
- a trace generated during the faulted workload is queryable in Tempo.

The default load-test target (`/health`) does not inject latency, so the script
passes `--url http://127.0.0.1:8080/ready` explicitly. `renzu compare` prints a
compatibility warning because the scenario names differ; the delta is still
meaningful. Report deep links are best-effort, so Tempo is queried by the
faulted run's time window rather than by a client-generated trace ID. The script
exits 0 on success; compose is left running.

## Batching

Both pipelines use the `batch` processor with default settings (timeout and size).
Batching reduces the number of export requests and smooths traffic to the backends.
No custom tuning is applied yet; batching behavior is revisited if throughput or
memory pressure requires it (see docs/decisions/0005-opentelemetry-collector-pipeline.md).

## Resource attributes

Resource attributes (`service.name`, `service.version`) are set by the gateway
and orders applications and flow through the collector unchanged. The collector
does not add a resource processor; it passes through attributes supplied by the
application. This avoids speculative abstraction and keeps attribute ownership in
the application.

## Application logging

The gateway and orders emit newline-delimited JSON logs to stdout for local
reading. The same `tracing` events are converted to OTLP LogRecords by
`OpenTelemetryTracingBridge`; the Collector forwards those records to Loki
(F-013).

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
- `RENZU_OTLP_ENDPOINT` (default `http://127.0.0.1:4318/v1/traces`) is the
  Collector's OTLP **HTTP** endpoint. Compose sets it to
  `http://otel-collector:4318/v1/traces` for both services. The log endpoint is
  derived by replacing `/v1/traces` with `/v1/logs`. Port `4317` is the
  Collector's gRPC receiver and must not be used with the HTTP exporter.

OTLP records stored in Loki carry `body`, `severity`, `traceid`, `spanid`,
`attributes`, and `instrumentation_scope` in the log line. The `renzu` CLI uses
`RENZU_TEMPO_URL` (default `http://127.0.0.1:3200`) and `RENZU_GRAFANA_URL`
(default `http://127.0.0.1:3000`) to build trace deep links in experiment
reports.

### Sensitive-data redaction

Redaction is call-site opt-in through the [`Redacted`] wrapper:

```rust
tracing::info!(auth = %Redacted(&token), "authenticating");
```

Values wrapped in `Redacted` render as `[REDACTED]`; values logged without it are
emitted as-is. Documented sensitive keys: `Authorization`, `Cookie`,
`password`, `token`, `secret`, `api_key`.

### Known limitations

- OTLP log records carry native `traceid` and `spanid` fields from the current
  span. Loki stores them inside the log line rather than as indexed labels.
  Correlation from a log line to a trace is available through the Grafana
  derived field; trace-to-log UI links are not configured because the Collector
  currently labels application log streams `service_name=unknown_service`.
- The gateway forwards an inbound `traceparent` header to orders, but no global
  W3C propagator is installed. Report trace links built from client-generated
  `traceparent` IDs are therefore best-effort and may not resolve to a service
  span.
- Redaction only applies where call sites opt in; unredacted sensitive data
  logged directly is not caught.

## Error handling and export failure observability

- Collector export failures are logged to the container's stdout/stderr and are
  never silently dropped. Inspect them with
  `docker compose -f compose.yaml -f compose.local.yaml logs otel-collector`.
- Gateway/Orders-side export failures are logged by the application and are
  never fatal to the application.
- If any service reports `unhealthy` in `docker compose -f compose.yaml -f
  compose.local.yaml ps`, telemetry export is degraded or unavailable; check
  `docker compose -f compose.yaml -f compose.local.yaml logs <service>`.
- The metrics pipeline currently exports to Prometheus (via Collector) and is
  scraped by Prometheus. Metrics are queryable in Grafana.

## Known limitations

- Tempo, Loki, and PostgreSQL use named volumes for persistence; data survives
  `docker compose -f compose.yaml -f compose.local.yaml down` but not with `-v`.
- Runtime validation (`docker compose up`) requires a Docker daemon and the
  ability to pull pinned images. If either is unavailable, `docker compose -f
  compose.yaml -f compose.local.yaml config` still validates the YAML after
  opting into local credentials.
- Resource limits are estimates. If containers are OOM-killed or CPU-throttled
  under load, raise the corresponding limits in `compose.yaml` and re-test.
- Health-check probes assume the service images ship the required probe tools
  (wget, curl, pg_isready). The pinned images used here include them.