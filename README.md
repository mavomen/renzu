# Renzu Observability

Renzu is a local-first observability laboratory for reproducing and investigating
failures in a small distributed system. Two Rust/Axum services emit OpenTelemetry
traces, metrics, and logs through a Collector into Tempo, Prometheus, and Loki.
A `renzu` CLI drives scenarios, load tests, reports, and the local environment.

This is an MVP at `v0.0.0`. It is a development lab, not a production monitoring
platform. There are no releases or published images.

## What you can do with it

- Start eight pinned containers with one Docker Compose command.
- Exercise a gateway and orders service with request tracing, fault injection,
  and health/readiness endpoints that reflect telemetry export health.
- Query traces in Tempo through Grafana Explore, metrics in Prometheus, and
  logs in Loki.
- Prove log/trace correlation: a traced request produces a Loki log with a
  native `traceid`, and the same ID resolves to a Tempo trace.
- Run a scenario or load test and save a JSON report with trace deep links.

## Requirements

- Linux, Docker Engine with Compose V2 (`docker compose`).
- Rust toolchain pinned in `rust-toolchain.toml` (currently 1.97.0).
- About 3.75 GiB RAM and 3.75 CPU cores for the container limits.
- Loopback ports: `3000`, `3100`, `3200`, `4317`, `4318`, `4319`, `8080`,
  `8081`, `9090`, `9464`, `13133`, and `5432`.

## Quickstart

```bash
# 1. Build and test the Rust workspace.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# 2. Start the stack.
docker compose up -d

# 3. Generate traffic.
curl -fsS http://127.0.0.1:8080/health
curl -fsS http://127.0.0.1:8080/order/demo-1

# 4. Inspect telemetry in Grafana at http://127.0.0.1:3000.
#    Default local-only login: admin / admin.
#    Explore -> Tempo for traces, Loki for logs, Prometheus for metrics.

# 5. Tear down. Add -v to remove named volumes.
docker compose down
```

If host port `5432` is already in use, create an untracked Compose override that
maps a different host port (for example `15432:5432`) and pass it to both `up`
and `down` with `-f compose.yaml -f <override-file>`. Inter-service DNS is
unaffected.

### Run the proof scripts

With the stack running:

```bash
./scripts/verify-infrastructure.sh        # health, startup/shutdown behavior
./scripts/e2e-trace-visualization.sh      # synthetic trace is queryable in Tempo
./scripts/e2e-log-trace-correlation.sh    # real request: Loki log -> Tempo trace
./scripts/e2e-metrics-report.sh           # Prometheus counter + saved JSON report
./scripts/e2e-incident-journey.sh         # baseline vs db-latency, compare, Tempo
```

The correlation script sends `GET /order/<unique-id>` to the gateway, finds the
matching application log in Loki, reads its `traceid`, and asserts that Tempo
returns the same trace with a span for that order. The metrics script generates
gateway traffic, asserts that `renzu_http_requests_total` is queryable in
Prometheus, runs a short baseline load test, and checks the saved report. The
incident-journey script reproduces the full latency investigation below using
an isolated reports directory. All scripts leave Compose running.

### Investigate a latency incident

Run the baseline and faulted workloads against `/ready`, the endpoint where the
database-latency fault is injected. The default load-test target (`/health`)
does not inject latency, so the `--url` flag is required:

```bash
cargo run -p renzu -- load-test --scenario baseline \
  --url http://127.0.0.1:8080/ready --duration 10 --rate 20
# -> run id: baseline-<timestamp>

cargo run -p renzu -- load-test --scenario db-latency \
  --url http://127.0.0.1:8080/ready --duration 10 --rate 20
# -> run id: db-latency-<timestamp>

cargo run -p renzu -- compare <baseline-run-id> <db-latency-run-id>
cargo run -p renzu -- diagnose <db-latency-run-id>
```

The fault adds 200 ms to each `/ready` request and is cleaned up automatically
when the load test ends. Compare prints a compatibility warning because the
scenario names differ; the p95 delta still shows the injected latency. Reports
include best-effort Tempo API links for the slowest requests; the E2E script
verifies traces independently through Tempo's search API because those links
use client-generated IDs that are not guaranteed to resolve.

## CLI workflow

```bash
cargo run -p renzu -- --help
cargo run -p renzu -- scenario list
cargo run -p renzu -- load-test --scenario baseline --duration 10 --rate 20
cargo run -p renzu -- report <run-id>
cargo run -p renzu -- compare <baseline-run-id> <comparison-run-id>
```

A load test writes `./reports/<run-id>.json` (override with
`RENZU_REPORTS_DIR`). Reports include Tempo API and Grafana Explore links for
error and slowest requests. The links use client-generated `traceparent` IDs and
are best-effort: no global W3C propagator is installed in the services, so a
link may not resolve to a service span. Set `RENZU_TEMPO_URL` and
`RENZU_GRAFANA_URL` if those services are not on their default loopback ports.

`up`, `status`, and `down` use `compose.yaml` from the current directory unless
`RENZU_COMPOSE_FILE` or `--compose-file` is set.

## Configuration

| Variable | Used by | Default | Purpose |
|---|---|---|---|
| `RENZU_OTLP_ENDPOINT` | gateway, orders | `http://127.0.0.1:4318/v1/traces` | Collector OTLP/HTTP base; `/v1/logs` is derived from it |
| `RENZU_OTLP_METRICS_ENDPOINT` | gateway | `http://127.0.0.1:4318/v1/metrics` | Collector OTLP/HTTP metrics receiver |
| `RENZU_LOG_LEVEL` | gateway, orders | `info` | `tracing` filter |
| `RENZU_ENVIRONMENT` | gateway, orders | `local` | Resource attribute |
| `GF_SECURITY_ADMIN_USER` | Grafana | `admin` | Local-only default; override off loopback |
| `GF_SECURITY_ADMIN_PASSWORD` | Grafana | `admin` | Local-only default; override off loopback |
| `RENZU_DB_USER` / `RENZU_DB_PASSWORD` / `RENZU_DB_NAME` | Postgres | `renzu` / `renzu` / `renzu` | Local-only defaults |
| `RENZU_TEMPO_URL` | `renzu` | `http://127.0.0.1:3200` | Report trace links |
| `RENZU_GRAFANA_URL` | `renzu` | `http://127.0.0.1:3000` | Report Explore links |
| `RENZU_REPORTS_DIR` | `renzu` | `./reports` | Report output directory |

Port `4317` is the Collector's gRPC receiver. The applications use OTLP/HTTP
exporters and must target `4318`.

## Known limitations

- Local lab only: loopback ports, default credentials, and no production
  storage or access control. The fault endpoints on the gateway are
  unauthenticated.
- Loki labels application OTLP log streams `service_name=unknown_service`;
  `traceid` and `spanid` are in the log line, not indexed labels. Grafana has a
  log-to-trace derived field, but trace-to-log UI linking is not configured.
- Trace propagation to the downstream orders service is forwarded as a header,
  but no global W3C propagator is installed, so the two services can appear as
  separate traces.
- Workspace tests assume no application is already bound to the gateway and
  orders loopback ports. Stop `gateway` and `orders` before running
  `cargo test --workspace` against a live Compose stack.
- No profiler or autonomous remediation. P1/P2 backlog items are not part of
  this MVP.

## Documentation

- `docs/PRD.md` — product requirements.
- `docs/FEATURES.md` — feature backlog, status, and verification notes.
- `docs/ARCHITECTURE.md` and `docs/decisions/` — architecture and ADRs.
- `docs/runbooks/local-infrastructure.md` — ports, health checks, and smoke tests.
- `TODO.md` — maintainer-owned next actions (local file, not committed).
