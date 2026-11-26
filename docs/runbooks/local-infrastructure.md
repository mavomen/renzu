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

### Grafana admin credentials

Grafana admin credentials are configurable via environment variables:

| Variable | Default | Notes |
|----------|---------|-------|
| `GF_SECURITY_ADMIN_USER` | `admin` | Local-only default (PRD NFR-04). **Must be overridden in shared environments.** |
| `GF_SECURITY_ADMIN_PASSWORD` | `admin` | Local-only default (PRD NFR-04). **Must be overridden in shared environments.** |

Override at startup:

```bash
GF_SECURITY_ADMIN_USER=myadmin GF_SECURITY_ADMIN_PASSWORD=secret docker compose up -d
```

### PostgreSQL host port publication

PostgreSQL port 5432 is published to `127.0.0.1:5432` by default for local development convenience. This is bound to loopback only (PRD NFR-04).

For shared or production-like environments, disable host port publication by removing the `ports` block from the `postgres` service in `compose.yaml` (or via a Compose override file). Inter-service communication uses Docker network DNS (`postgres:5432`) and is unaffected.

## Prerequisites

- Docker Engine and Docker Compose (Compose V2, `docker compose`).
- Approximately 4.75 CPU cores and 4.5 GB RAM for all containers combined (see Resource Estimates below).
- Approximately 5–10 GB disk for volumes plus image layers.
- No cloud account, paid service, or external AI API required (local-first).

## Startup

```bash
docker compose up -d
```

Startup order is enforced via `depends_on` with `condition: service_healthy`:
- Collector waits for Tempo and Loki.
- Prometheus waits for Collector.
- Grafana waits for Prometheus and Loki.
- Orders waits for Collector.
- Gateway waits for Postgres and Collector.

```bash
docker compose ps
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
| PostgreSQL | 127.0.0.1:5432 | 5432 | PostgreSQL (local dev only; disable for shared envs) |
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

Startup failures produce actionable diagnostics via `docker compose logs <service>`.

## Shutdown

```bash
docker compose down
```

Removes the containers and the network. Named volumes (`postgres_data`, `loki_data`, `tempo_data`)
are preserved; run `docker compose down -v` to remove them.

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
| Memory | 4.25 GiB |
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
# Requires docker compose up -d (all services) to be running.
./scripts/e2e-trace-visualization.sh
```

The script sends a synthetic OTLP trace directly to Tempo's HTTP receiver and
asserts the trace is queryable by ID with a named span and a positive duration.
It exits 0 on success; compose is left running.

### Log/trace correlation E2E (F-013)

```bash
# Requires docker compose up -d (all services) to be running.
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
- Redaction only applies where call sites opt in; unredacted sensitive data
  logged directly is not caught.

## Error handling and export failure observability

- Collector export failures are logged to the container's stdout/stderr and are
  never silently dropped. Inspect them with `docker compose logs otel-collector`.
- Gateway/Orders-side export failures are logged by the application and are
  never fatal to the application.
- If any service reports `unhealthy` in `docker compose ps`, telemetry export is
  degraded or unavailable; check `docker compose logs <service>`.
- The metrics pipeline currently exports to Prometheus (via Collector) and is
  scraped by Prometheus. Metrics are queryable in Grafana.

## Known limitations

- Tempo, Loki, and PostgreSQL use named volumes for persistence; data survives
  `docker compose down` but not `docker compose down -v`.
- Runtime validation (`docker compose up`) requires a Docker daemon and the
  ability to pull pinned images. If either is unavailable, `docker compose
  config` still validates the YAML.
- Resource limits are estimates. If containers are OOM-killed or CPU-throttled
  under load, raise the corresponding limits in `compose.yaml` and re-test.
- Health-check probes assume the service images ship the required probe tools
  (wget, curl, pg_isready). The pinned images used here include them.