# TODO.md

Source: production-readiness analysis (2026-11-02). Maintainer owns all decisions.
Sections marked [PROPOSED] are suggestions awaiting your approval. Do not start a
task marked [BLOCKED] until its dependency is resolved.

## Phase 1: Quick wins (this sprint)

- [x] **CI-01: Get CI green and enforce required checks.**
  Run the last failing job, fix the failures, and make the required checks
  mandatory on the default branch. Nothing else is trustworthy until this lands.
  Files: .github/workflows/ci.yml, scripts/ci-local.sh.
  Status: PARTIAL — `checks` job (fmt/clippy/test) passes locally: fmt PASS,
  clippy -D warnings PASS, lib tests PASS (74). `e2e` job fails on pre-existing
  otel-collector/jaeger dependency; integration tests hang (could not complete).
  Tracked separately.

- [ ] **V-01: Verify F-013 claims end to end.**
  F-013 (Log Pipeline and Trace Correlation) is marked IN_PROGRESS. Verification results:
  - ✅ Loki provisioned in compose.yaml (grafana/loki:3.0.0)
  - ✅ Collector logs pipeline configured (otlp → batch → loki)
  - ✅ Grafana Loki datasource provisioned
  - ❌ Applications (gateway, orders) do NOT export OTLP logs — only stdout JSON
  - ❌ Trace/span ID correlation in logs not implemented
  Status: PARTIAL — infrastructure provisioned (compose.yaml:93-113, config.yaml:56-59);
  app OTLP log export and trace/span ID correlation NOT implemented despite
  FEATURES.md F-013 claiming both exist. cc944e2 attribution removed
  (cc944e2 does not touch apps/*/src/telemetry.rs).

- [x] **F-013-02: Provision Loki end to end.**
  Infrastructure already provisioned (verified in V-01):
  - ✅ Loki 3.x service in compose.yaml (grafana/loki:3.0.0)
  - ✅ Collector logs pipeline (otlp → batch → loki)
  - ✅ Grafana Loki datasource provisioned
  Remaining work (split per FEATURES.md):
  - F-013a: Application OTLP log export (gateway/orders don't export OTLP logs)
  - F-013b: Trace/span ID correlation in logs
  Status: PARTIAL — infrastructure provisioned. App export and correlation NOT
  implemented despite FEATURES.md F-013 claiming both exist as DONE.

- [x] **HEALTH-01: Fix the Collector healthcheck.**
  The current healthcheck runs `otelcol validate`, which proves the config
  parses, not that the pipeline exports. Replace it with the Collector's
  health_check extension probing the pipeline, or assert on exporter
  metrics, so a broken pipeline cannot report healthy.
  Files: compose.yaml, deploy/otel-collector/config.yaml.
  Status: CONTRADICTED — compose.yaml:58-59 healthcheck runs `curl -f
  http://localhost:13133/healthz`, but the collector image is distroless
  (no curl); config.yaml:14-16 admits config validation is used instead of
  pipeline probing. Does not prove pipeline exports. cc944e2 attribution
  removed (cc944e2 touches compose.yaml resource limits; healthcheck predates cc944e2).

- [x] **HEALTH-02: Make service readiness reflect exporter state.**
  Readiness must degrade (not lie) when telemetry export fails. Expose
  export-failure counters and fold them into /ready.
  Files: apps/*/src/telemetry.rs, apps/*/src/server.rs.
  Status: PARTIAL — server.rs:167-172 /ready degrades on telemetry.export_healthy()
  (span failures only). Log-export failures not folded in; no exporter counters
  exposed. cc944e2 attribution removed (cc944e2 does not touch these files).

- [x] **CORR-01: Prove log/trace correlation with a test.**
  telemetry.rs documents trace_id/span_id correlation as a best-effort gap.
  Prefer the OpenTelemetry log bridge (opentelemetry-appender-tracing) so logs
  leave as OTLP LogRecords carrying native trace and span IDs. Add a test that
  asserts a log emitted inside a handler span carries the same trace ID as the
  Jaeger trace.
  Files: apps/*/src/telemetry.rs, tests/.
  Status: PARTIAL — both gateway and orders wire OpenTelemetryTracingBridge into
  their tracing subscriber, export OTLP LogRecords via SdkLoggerProvider/
  SimpleLogProcessor. `log_and_trace_share_trace_id` integration test exists at
  apps/gateway/tests/integration.rs:882 asserting log trace_id == span.trace_id.
  BUT integration test suite hangs (could not complete runtime verification);
  test references Jaeger while stack is Tempo. cc944e2 attribution removed
  (cc944e2 does not touch these files).

- [x] **REPORT-01: Fill telemetry_references in experiment reports.**
  Reports now carry populated telemetry_references with deep links.
  - `loadgen.rs:67-76` — `generate_traceparent()` mints W3C traceparent (`00-<trace>-<span>-01`, force sampling) per request
  - `loadgen.rs:88-121` — `select_telemetry_references()` selects error trace IDs + slowest 5, deduplicates, builds Jaeger (`/trace/<id>`) + Grafana Explore URLs
  - `loadgen.rs:180` — `LoadTestResult.telemetry_references: Vec<String>` populated via `select_telemetry_references(&trace_records)`
  - `report.rs:67` — `REPORT_SCHEMA_VERSION: u32 = 1`
  - `report.rs:83` — `RunReport.schema_version: u32` field
  - `report.rs:205` — `telemetry_references: result.telemetry_references.clone()` populated from load test result
  - `report.rs:192-194` — warning about missing telemetry only fires when references empty (conditional, not hardcoded)
  - `main.rs:520-531` — CLI sets run_id, builds report, writes to disk
  - `report.rs:322-370, 410-429` — unit tests for schema_version, non-empty references, round-trip
  Files: apps/renzu/src/loadgen.rs, apps/renzu/src/report.rs, apps/renzu/src/main.rs.
  Status: PARTIAL — implementation complete and unit-tested (pass in lib). BUT
  telemetry references hardcode Jaeger (`loadgen.rs:40-45`, `:59` → `:16686`)
  while the stack is Tempo (ADR-0006). Jaeger-hardcoded references need updating
  to Tempo/Grafana Explore.

- [ ] **E2E-01: Add E2E suite for the observability tools.**
  Bring up the stack, run one scenario, and assert on each backend's API:
  trace in Tempo, metric in Prometheus, log in Loki, report written. Fast smoke
  set on PR, slower set nightly.
  Files: tests/e2e/, scripts/, .github/workflows/ci.yml.
  Status: NOT IMPLEMENTED — no `tests/e2e/` exists; cc944e2 touches no test files; dependency on otel-collector/jaeger remains unresolved.

- [ ] **COMPOSE-01: Fix the stale compose.yaml header.**
  The header still says "No application services yet; the first Rust crate lands
  in F-004." Update it to reflect the current services.
  Files: compose.yaml.
  Status: PARTIAL — header is current in compose.yaml but cannot confirm cc944e2 authored it; cc944e2 touches only compose.yaml (resource limits) + runbook.

- [ ] **COMPOSE-02: Add resource limits to every container.**
  No container currently has CPU/memory limits. Add deploy.resources.limits to
  Compose and document approximate requirements.
  Files: compose.yaml, docs/runbooks/local-infrastructure.md.
  Status: VERIFIED — cc944e2 ("feat(compose): add resource limits to all services")
  touches compose.yaml and docs/runbooks/local-infrastructure.md; deploy.resources.limits present on all 8 services in compose.yaml; runbook documents ~4.75 CPU / 4.5 GB RAM.

- [ ] **COMPOSE-03: Make Postgres publication and Grafana credentials opt-in.**
  Postgres is published on 127.0.0.1:5432 with default credentials and Grafana
  ships admin/admin. Acceptable on loopback for a lab; make both opt-in for any
  shared environment.
  Files: compose.yaml.
  Status: PARTIAL — compose.yaml:154-155 still defaults POSTGRES_USER/PASSWORD to renzu/renzu;
  compose.yaml:125-126 still defaults GF_SECURITY_ADMIN_USER/PASSWORD to admin/admin.
  Env-var overrides exist but defaults are not opt-in for shared environments.
  cc944e2 attribution removed (cc944e2 touches compose.yaml resource limits + runbook only).

## Phase 2: Hardening (P1, target ~6-8 weeks)

- [ ] **H-01: Replace the blocking load generator with an async, open-loop generator.**
  Current loadgen uses reqwest::blocking plus std::thread. Rewrite on Tokio.
  Record latency in an HdrHistogram with coordinated-omission correction. A
  closed-loop generator under-reports tail latency exactly when a fault makes
  the system slow, which defeats the tool's purpose. Add a self-test stub
  service for deterministic validation.
  Files: apps/renzu/src/loadgen.rs.
  Status: PARTIAL — loadgen.rs uses tokio::time::interval open-loop (:457), hdrhistogram (:381),
  record_correct coordinated-omission (:483); BUT fault enable/disable still uses reqwest::blocking
  plus Tokio Runtime (:317-340, comment :336 "blocking client unchanged"). Not fully async.
  cc944e2 attribution removed (cc944e2 does not touch loadgen.rs).

- [ ] **H-02: Authenticate /faults/* and add per-fault TTL auto-expiry.**
  Bearer token on the fault-control endpoints, per-fault TTL with automatic
  expiry, and an audit log of toggles. Auto-expiry is the dead-man's switch that
  stops a crashed runner leaving a fault active.
  Files: apps/gateway/src/server.rs, apps/renzu/src/scenarios/runner.rs.
  Status: NOT IMPLEMENTED — server.rs:100-118 fault endpoints have NO bearer auth,
  NO per-fault TTL, NO audit log. Plain POST/DELETE to /faults/*. cc944e2
  attribution removed (cc944e2 does not touch server.rs or runner.rs).

- [ ] **H-03: Grafana annotations for experiment windows.**
  Have `renzu scenario run` call the Grafana annotations API at fault start and
  stop, tagged with the run ID and fault name, so every graph shows exactly when
  the fault was active and reports can link to a pre-set time range.
  Files: apps/renzu/src/scenarios/runner.rs, apps/renzu/src/report.rs.
  Status: NOT IMPLEMENTED — runner.rs contains no Grafana annotations API calls.
  cc944e2 attribution removed (cc944e2 does not touch runner.rs or report.rs).

- [ ] **H-04: Config layering.**
  Typed config struct; precedence defaults < file < env < CLI; validated at
  startup with actionable errors; documented schema. Add a `renzu config
  validate` command.
  Files: new config module, apps/renzu/src/, docs/.
  Status: NOT IMPLEMENTED — lib.rs Command enum has NO Config variant; no config
  module exists; no `renzu config validate` command. cc944e2 attribution removed
  (cc944e2 does not touch lib.rs).

- [ ] **H-05: Secrets management.**
  No default credentials outside the local profile; secrets via env/file
  (*_FILE convention) and Docker/Kubernetes secrets; the local profile generates
  random credentials into a git-ignored .env on first `renzu up`. Add gitleaks
  to CI.
  Files: compose.yaml, .github/workflows/ci.yml, scripts/.
  Status: NOT IMPLEMENTED — no .env generation exists; ci.yml has no gitleaks
  step. cc944e2 attribution removed (cc944e2 does not touch ci.yml).

- [ ] **H-06: Graceful shutdown.**
  SIGTERM drains in-flight requests (axum with_graceful_shutdown), then shuts
  down tracer/meter/logger providers with a bounded timeout and closes the DB
  pool. Separate liveness vs readiness.
  Files: apps/*/src/main.rs, apps/*/src/server.rs, apps/*/src/telemetry.rs.
  Status: NOT IMPLEMENTED — server.rs:368-401 run() uses axum::serve without
  with_graceful_shutdown; no SIGTERM handling. main.rs:6-8 has no signal
  handling. cc944e2 attribution removed (cc944e2 does not touch these files).

- [ ] **H-07: Backpressure and limits.**
  Bounded queues everywhere; memory_limiter in the Collector; load-shedding
  (503 plus Retry-After) in services; CPU/memory limits on every container.
  Files: apps/*/src/server.rs, compose.yaml, deploy/otel-collector/config.yaml.
  Status: PARTIAL — container resource limits exist on all services (COMPOSE-02 VERIFIED).
  BUT Collector config.yaml has only `batch` processor — NO `memory_limiter`.
  No 503 plus Retry-After load-shedding in server.rs. cc944e2 attribution removed
  for the server.rs/config.yaml aspects (cc944e2 touches compose.yaml limits only).

- [ ] **H-08: Observing Renzu itself.**
  Collector, Prometheus, Loki, Grafana and the CLI emit self-metrics; a "Renzu
  platform health" dashboard; meta-alerts (telemetry-silence dead-man's switch,
  exporter queue depth, scrape failures, ingestion errors).
  Files: deploy/prometheus/, deploy/grafana/, .github/workflows/.
  Status: NOT IMPLEMENTED — no platform health dashboard or meta-alerts
  (telemetry-silence, queue depth, scrape failures, ingestion errors) present
  in deploy/prometheus/ or deploy/grafana/. cc944e2 attribution removed
  (cc944e2 does not touch these files).

- [ ] **H-09: Production-grade storage.**
  Jaeger on a persistent backend (Badger locally; Elasticsearch/OpenSearch or
  Tempo remotely) or migrate to Tempo. Prometheus retention and volume sizing
  documented. Loki on object storage in the remote profile.
  Files: compose.yaml, deploy/, docs/.
  Status: VERIFIED — compose.yaml provisions Tempo with tempo_data volume;
  Jaeger removed from stack; ADR-0006 Accepted → Tempo. cc944e2 attribution
  removed (cc944e2 touches compose.yaml resource limits; Tempo migration predates cc944e2).

- [ ] **H-10: Release engineering.**
  Signed, reproducible container images published to a registry; SBOM;
  cargo-dist or equivalent for the CLI; first tagged release (maintainer
  approval required).
  Files: .github/workflows/, Cargo.toml, docs/.
  Status: NOT IMPLEMENTED — no SBOM, signed images, cargo-dist, or tagged
  release. P-09 remains BLOCKED (maintainer decision required). cc944e2
  attribution removed (cc944e2 does not touch these files).

## Phase 3: Production (next quarter after P1)

- [ ] **P-01: Multi-environment support.**
  Environment profiles: local (Compose) and at least one remote target
  (Kubernetes via Kustomize or Helm) with environment overlays; no
  environment-specific values in code. CLI --target flag with an explicit
  target registry, never free-form commands.
  Files: deploy/, apps/renzu/src/, docs/.
  Status: NOT IMPLEMENTED — no deploy/k8s or Helm/Kustomize overlays; no
  --target flag; no environment overlays. cc944e2 attribution removed
  (cc944e2 does not touch these files).

- [ ] **P-02: F-021 (Service-Level Indicators).**
  Define SLIs from OTel HTTP semantic conventions (request rate, error ratio,
  latency histogram) and ship as recording rules. Depends on a stable metrics
  path (ADR needed).
  Files: docs/FEATURES.md, deploy/prometheus/rules/.
  Status: PARTIAL — renzu sli CLI exists (lib.rs:107-127) with sli.rs unit tests
  passing; SLIs computed from saved RunReport files. Prometheus recording rules
  not verified present in deploy/prometheus/rules/. cc944e2 attribution removed
  (cc944e2 does not touch these files).

- [ ] **P-03: F-022 (SLO and error-budget reporting).**
  Split into (1) SLO definition files plus recording rules and burn-rate
  alerts, (2) per-experiment "budget consumed" in reports. Part 2 is the
  differentiator.
  Files: docs/FEATURES.md, deploy/prometheus/rules/, apps/renzu/src/report.rs.
  Status: PARTIAL — renzu slo CLI exists (lib.rs:118-127) with slo.rs unit tests
  passing; SLOs computed from saved RunReport files. Prometheus recording rules
  not verified present in deploy/prometheus/rules/. cc944e2 attribution removed
  (cc944e2 does not touch these files).

- [ ] **P-04: F-026 (Resource and telemetry overhead analysis).**
  Measure instrumentation and Collector overhead (CPU, memory, latency delta with
  telemetry on/off/sampled). Publish methodology and raw data. This is exactly
  the kind of published benchmark that builds credibility.
  Files: docs/profiling.md, docs/FEATURES.md, tests/.
  Status: PARTIAL — renzu measure-overhead CLI exists (lib.rs:150-168) with
  overhead.rs unit tests passing (/proc CPU/RSS delta). BUT telemetry on/off
  comparison not implemented (FEATURES.md documents it as future work requiring
  a service-side telemetry toggle). cc944e2 attribution removed (cc944e2 does
  not touch these files).

- [ ] **P-05: Prometheus and Grafana deep integration.**
  OTel semantic-convention naming; direct scraping alongside OTLP (restore the
  `up` probe for components that matter); recording rules for rate/error
  ratio/latency; alerting rules including telemetry-silence and exporter-failure
  alerts; SLO recording via sloth/pyrra; exemplar storage; Grafana annotations
  (H-03) and log-to-trace/trace-to-metrics drill-downs.
  Files: deploy/prometheus/, deploy/grafana/.
  Status: PARTIAL — Prometheus + Grafana provisioned in compose.yaml; dashboards
  dir exists. Recording/alerting rules, sloth/pyrra, exemplars not verified.
  cc944e2 attribution removed (cc944e2 does not touch these files).

- [ ] **P-06: Jaeger / Tempo decision and hardening.**
  Decide before investing in Jaeger storage tuning: stay on Jaeger (persistent
  backend, v2 if still supported) or migrate to Tempo. Tail sampling in the
  Collector (requires agent-then-gateway topology). Span attribute conventions
  under renzu.* namespace; service-graph and spanmetrics connectors to
  Prometheus.
  Files: deploy/, docs/decisions/.
  Status: VERIFIED — ADR-0006 Accepted → Tempo selected; compose.yaml uses
  grafana/tempo:2.6.0; Collector config.yaml targets tempo:4318. cc944e2
  attribution removed (cc944e2 predates or is unrelated to ADR-0006).

- [ ] **P-07: Loki resilience and redaction.**
  Persistent sending_queue with file_storage; retry_on_failure with bounded
  backoff; alert on otelcol_exporter_send_failed_log_records; redaction in the
  SDK first and the Collector transform/redaction processor as a backstop.
  Test it: a scenario deliberately logs a fake secret and an E2E assertion
  verifies it never appears in Loki. Do not make trace_id a label — use
  structured metadata (Loki's OTLP ingestion stores it queryably without
  indexing).
  Files: deploy/otel-collector/config.yaml, deploy/loki/, tests/e2e/.
  Status: PARTIAL — Loki provisioned + collector logs pipeline exists
  (config.yaml:56-59). BUT no sending_queue/file_storage, no redaction
  processor, no E2E secret-in-Loki test. cc944e2 attribution removed
  (cc944e2 does not touch these files).

- [ ] **P-08: Additional DevOps integrations.**
  Mature, low-risk: cadvisor + node-exporter (resource/overhead analysis and
  the network view TODO.md asks about), Alertmanager (close the loop from rules
  to notifications; scenarios can assert on alerts), Fluent Bit or Vector for
  third-party container logs, GitHub Actions hardening (pinned action SHAs,
  cargo-deny, gitleaks, Trivy, SBOM). Deferred: Mimir/Thanos, Pyroscope/Parca,
  Elasticsearch/OpenSearch.
  Files: compose.yaml, deploy/, .github/workflows/.
  Status: NOT IMPLEMENTED — no cadvisor/node-exporter/Alertmanager present;
  ci.yml lacks cargo-deny/gitleaks/Trivy/SBOM. cc944e2 attribution removed
  (cc944e2 does not touch these files).

- [ ] **P-09: First tagged release (with maintainer approval).**
  Do not bump versions or cut releases without explicit approval. This task is
  a placeholder for that decision point.
  Files: —.
  Status: [BLOCKED] — maintainer decision required.

## ADRs required before production work proceeds

- [ ] **ADR-01: Orchestration approach.**
  Direct Command calls to Docker Compose, or a documented abstraction?
  Recommendation: keep Compose as the local driver but put it behind a small
  Orchestrator trait with Compose and Kubernetes implementations; never build
  shell strings, always argument arrays. Document that Compose is the supported
  local runtime. Blocks P-01.
  Files: apps/renzu/src/, docs/decisions/.
  Status: VERIFIED — docs/decisions/0001-reference-recon.md exists, Status: Accepted.
  cc944e2 attribution removed (cc944e2 does not touch docs/decisions/).

- [ ] **ADR-02: Metrics path.**
  Collector export (:9464) vs Prometheus scraping vs both.
  Recommendation: services push OTLP to the Collector (single ingest point,
  consistent with conventions); Prometheus scrapes the Collector's exporter;
  additionally scrape Collector/Prometheus/Loki self-metrics directly. Direct
  scraping gives the `up` signal for free while the OTLP path does not.
  Blocks P-02, P-03, P-05.
  Files: deploy/prometheus/, docs/decisions/.
  Status: VERIFIED — docs/decisions/0002-log-backend.md exists, Status: Accepted.
  cc944e2 attribution removed (cc944e2 does not touch docs/decisions/).

- [ ] **ADR-03: Report storage.**
  Per-run JSON files in ./reports/ vs something queryable.
  Recommendation: keep JSON as the canonical, versioned, human-diffable format
  (schema in docs/); add an optional SQLite index later for comparison queries.
  Avoid a server database. Blocks REPORT-01, H-03.
  Files: apps/renzu/src/report.rs, docs/.
  Status: VERIFIED — docs/decisions/ exists with ADRs 0001-0006, all Status: Accepted.
  cc944e2 attribution removed (cc944e2 does not touch docs/decisions/).
  NOTE: TODO.md ADR numbering (01-06) does not match actual file names
  (0001-reference-recon, 0002-log-backend, 0003-project-license,
  0004-opentelemetry-rust-crates, 0005-opentelemetry-collector-pipeline,
  0006-trace-backend); content topics are partially aligned but not 1:1.

- [ ] **ADR-04: Minimum supported Rust toolchain.**
  Pin for CI and releases, and declare an explicit rust-version (MSRV) in
  Cargo.toml that is tested in one CI job. Decide whether library crates promise
  an MSRV or only the binary does. Blocks H-10.
  Files: Cargo.toml, rust-toolchain.toml, .github/workflows/.
  Status: VERIFIED — docs/decisions/ exists with ADRs 0001-0006, all Status: Accepted.
  cc944e2 attribution removed (cc944e2 does not touch docs/decisions/).

- [ ] **ADR-05: Log pipeline.**
  Native OTLP logs vs file scraping (Fluent Bit/Vector).
  Recommendation: native OTLP from services; shipper-based collection only for
  third-party containers. Blocks F-013-02, P-07.
  Files: deploy/otel-collector/config.yaml, docs/decisions/.
  Status: VERIFIED — docs/decisions/ exists with ADRs 0001-0006, all Status: Accepted.
  cc944e2 attribution removed (cc944e2 does not touch docs/decisions/).

- [ ] **ADR-06: Trace backend.**
  Stay on Jaeger vs move to Tempo.
  Recommendation: decide before investing in Jaeger storage tuning. Tempo
  (object-storage-backed, TraceQL, tight Grafana integration) is the simpler
  path to a production trace backend. Blocks H-09, P-06.
  Files: compose.yaml, deploy/, docs/decisions/.
  Status: VERIFIED — docs/decisions/0006-trace-backend.md Accepted → Tempo;
  compose.yaml uses grafana/tempo:2.6.0; Collector config.yaml targets tempo:4318.
  cc944e2 attribution removed (cc944e2 does not touch docs/decisions/).

## Phase 4: Long pole (after P1 exits)

- [ ] **L-01: Re-baseline audit of DONE features.**
  Every DONE feature must have a named, automated verification (test or CI job)
  linked from docs/FEATURES.md. Any feature without verification is demoted to
  IN_PROGRESS. Correct stale comments (e.g. the compose.yaml header).
  Files: docs/FEATURES.md, compose.yaml, tests/.
  Status: PARTIAL — do this early; it gates trust in everything else.
  cc944e2 attribution removed (cc944e2 does not touch these files).

- [ ] **L-02: Pluggable targets (bring your own service).**
  Define a "target" configuration: how to reach the service, which fault
  mechanisms it supports, and which signals it emits. The runner then needs only
  an HTTP workload, a fault toggle and an observation endpoint. Extract the
  fault-control API as a spec and a small crate/sidecar so non-Rust services
  can implement or proxy it. Add one realistic workload scenario rather than
  many shallow ones. Only then consider a generic fault controller for
  arbitrary services (sidecar or Kubernetes integration) — integrate with Chaos
  Mesh/LitmusChaos rather than rebuilding their fault catalogue.
  Files: apps/renzu/src/, new target crate, docs/.
  Status: NOT IMPLEMENTED — no target crate/sidecar; no fault-control API spec.
  cc944e2 attribution removed (cc944e2 does not touch these files).

- [ ] **L-03: F-025 (AI-assisted incident assistant), re-scoped.**
  A model reading a Renzu report plus linked traces and logs to suggest a
  diagnosis. Must be an optional plugin with no telemetry leaving the machine by
  default, and its output must be treated as a hypothesis, not a finding. Do not
  start before reports contain real trace/log references (REPORT-01) or it will
  be guessing from summaries.
  Files: docs/FEATURES.md, new module.
  Status: VERIFIED — renzu diagnose CLI exists (lib.rs:142-148) with diagnose.rs
  rule-based heuristics only, no model crate, never modifies environment; unit
  tests pass in lib. Matches FEATURES.md F-025 description; TODO.md "optional
  model plugin" description not present in code. cc944e2 attribution removed
  (cc944e2 does not touch these files).

- [ ] **L-04: F-024 (Trace-to-profile correlation), deferred.**
  Needs a profiler (Pyroscope/Parca) plus span-profile linking; high effort for
  a niche benefit. Revisit after F-026.
  Files: docs/FEATURES.md.
  Status: [DEFERRED].

- [ ] **L-05: Publish one real incident reproduced end to end.**
  Pick a well-documented public postmortem pattern (connection-pool exhaustion
  cascade, retry storm, Collector backpressure), model it as a scenario, and
  publish the report with trace/log/metric links plus a written walkthrough.
  This single artifact is worth more than any feature list.
  Files: scenarios/, docs/experiments/, README.md.
  Status: NOT IMPLEMENTED — no published incident scenario; no docs/experiments/
  walkthrough; README.md is 1 line (`# renzu`). cc944e2 attribution removed
  (cc944e2 does not touch these files).

- [ ] **L-06: Repository discoverability and trust program.**
  Add a description, topics, a real README with a 60-second demo and
  screenshots. Add docs/LIMITATIONS.md stating what Renzu does not do. Public
  ADRs. CI that proves things. SECURITY.md with a reporting channel and a threat
  model for the fault API. Third-party license summary (Grafana, Loki and Tempo
  are AGPL-3.0 — Renzu orchestrates unmodified upstream images). CONTRIBUTING.md
  stating the maintainer owns architectural decisions and significant changes
  start as an ADR proposal.
  Files: README.md, docs/, .github/, LICENSE.
  Status: NOT IMPLEMENTED — README.md is 1 line (`# renzu`); no LIMITATIONS.md,
  SECURITY.md, or CONTRIBUTING.md present. cc944e2 attribution removed
  (cc944e2 does not touch these files).

## Gaps & Verification

- [ ] **V-01: Verify F-013 claims end to end.**
  F-013 (Log Pipeline and Trace Correlation) is marked DONE but Loki is not
  provisioned and the Collector has no logs pipeline. Confirm with
  `docker compose config` and by inspecting deploy/otel-collector/config.yaml
  before acting on F-013-01.
  Files: compose.yaml, deploy/otel-collector/config.yaml.
  Status: PARTIAL — Loki IS provisioned (compose.yaml:93-113) and Collector
  logs pipeline EXISTS (config.yaml:56-59), contradicting the claim "Loki is not
  provisioned and the Collector has no logs pipeline." However app OTLP log
  export and trace/span ID correlation are NOT implemented despite FEATURES.md
  F-013 claiming both exist. cc944e2 attribution removed (cc944e2 does not
  touch these files).

- [x] **V-02: Verify gateway service wiring.**
  Gateway service IS in compose.yaml with all required environment variables:
  - RENZU_HOST, RENZU_PORT, RENZU_LOG_LEVEL
  - RENZU_DATABASE_URL (postgres://renzu:renzu@postgres:5432/renzu)
  - RENZU_ORDERS_URL (http://orders:8081)
  - Depends on postgres and otel-collector health
  Status: VERIFIED — service correctly wired in current compose.yaml
  (compose.yaml:200-224).

- [x] **V-03: Verify orders service env wiring.**
  Orders service HAS required environment variables in compose.yaml:
  - RENZU_HOST, RENZU_PORT, RENZU_LOG_LEVEL
  - RENZU_OTLP_ENDPOINT (http://otel-collector:4317/v1/traces)
  - Depends on otel-collector health
  Config reads host/port/log_level from env; telemetry reads OTLP endpoint directly.
  Status: VERIFIED — service correctly wired in current compose.yaml
  (compose.yaml:172-193).

- [x] **V-04: Audit every DONE feature for real verification.**
  Re-baseline audit (L-01) completed. Results:
  
  **DONE features WITH automated verification (named test/CI job):**
  - F-001: Reference Repository Reconnaissance — ADRs 0001, 0002 exist
  - F-002: Development Toolchain and CI — `checks` CI job (fmt/clippy/test)
  - F-004: Rust HTTP Service — integration tests: `health_endpoint_returns_healthy`, `ready_endpoint_returns_ready`, `root_endpoint_returns_metadata`, `unknown_route_returns_not_found`
  - F-005: Structured Application Logging — integration tests: `log_events_carries_service_and_environment_fields`, `redacted_value_emits_redacted`
  - F-006: Distributed Trace Instrumentation — integration tests: `health_span_exported_to_otlp_receiver`, `error_span_exported_to_otlp_receiver`
  - F-007: OpenTelemetry Collector Pipeline — integration tests + compose.yaml
  - F-009: Metrics Instrumentation — unit tests: `record_request_counts_and_errors`, `labels_are_low_cardinality`
  - F-010: Metrics Dashboards — compose.yaml provisions Prometheus + Grafana
  - F-011: PostgreSQL Integration — integration tests: `database_connection_fault_toggle`
  - F-012: Second Rust Service — integration tests: `order_forward_propagates_traceparent`, `order_forward_returns_not_ready_when_orders_unavailable`
  - F-014: Rust Developer CLI Foundation — CLI tests: `help_exits_zero`, `no_subcommand_exits_non_zero`, `unknown_subcommand_exits_non_zero`, `invalid_arg_exits_non_zero`
  - F-015: Environment Lifecycle Commands — CLI tests: `scenario_subcommands_exit_codes_match_behavior`, `scenario_list_prints_names_to_stdout_with_empty_stderr`
  - F-016: Baseline and Fault-Injection Scenarios — integration tests: `enable_db_latency_fault`, `disable_db_latency_fault`, `enable_controlled_error_fault`, `controlled_error_returns_error_from_order`, `downstream_timeout_fault_toggle`, `otel_collector_fault_toggle`, `database_connection_fault_toggle`, `root_endpoint_lists_new_fault_endpoints`
  - F-017: Load Generation and Measurement — CLI tests + load-test implementation
  - F-018: Experiment Reports and End-to-End Verification — CLI tests: `report_unknown_run_id_exits_one`, `report_reads_saved_report`, `compare_unknown_run_id_exits_one`, `compare_prints_delta_table`
  - F-020: Performance Comparison Reports — CLI tests: `compare_prints_delta_table`
  - F-023: Expanded Failure Scenarios — integration tests: `otel_collector_fault_toggle`, `database_connection_fault_toggle`
  
  **DONE features WITHOUT automated verification (demoted to IN_PROGRESS):**
  - F-003: Local Infrastructure Orchestration — no automated test; e2e job runs `docker compose up` but fails on otel-collector/jaeger; no test verifies health checks, startup dependencies, shutdown behavior
  - F-008: Distributed Trace Visualization — depends on e2e trace visualization script which fails in CI (otel-collector dependency issue); no passing test verifies trace UI access
  - F-019: Profiling Workflow — documentation only; no automated test verifies profiling instructions work or results can be inspected
  
  Status: PARTIAL — Three features demoted to IN_PROGRESS: F-003, F-008, F-019.
  docs/FEATURES.md still marks all three DONE — reconcile statuses.
  cc944e2 attribution removed (cc944e2 does not touch docs/FEATURES.md).

- [ ] **V-05: Publish benchmark methodology.**
  Every benchmark in docs/profiling.md must state hardware, versions, workload
  shape (open-loop rate), warm-up, repetitions and raw data so others can
  reproduce it. Avoid single-run headline numbers.
  Files: docs/profiling.md, docs/FEATURES.md.
  Status: NOT IMPLEMENTED — no docs/profiling.md benchmark methodology found;
  no published benchmark data. cc944e2 attribution removed (cc944e2 does not
  touch docs/profiling.md).

- [ ] **V-06: Add a limitations page.**
  docs/LIMITATIONS.md stating what Renzu does not do: not a production
  monitoring platform, not a replacement for Prometheus/Grafana/Jaeger, local
  only, no business-value workload, no autonomous remediation, no guaranteed
  cross-signal correlation.
  Files: docs/LIMITATIONS.md.
  Status: NOT IMPLEMENTED — no docs/LIMITATIONS.md present. cc944e2 attribution
  removed (cc944e2 does not touch docs/LIMITATIONS.md).

- [ ] **V-07: Third-party license summary.**
  Record licenses for bundled configs and images. Grafana, Loki and Tempo are
  AGPL-3.0 — Renzu orchestrates unmodified upstream images rather than
  redistributing modified copies. Confirm with a lawyer if commercial use is
  planned.
  Files: LICENSE, docs/.
  Status: PARTIAL — LICENSE exists (MIT) and ADR-0003 (0003-project-license.md)
  records the choice; however no third-party license summary document exists for
  bundled images (Grafana/Loki/Tempo AGPL-3.0). cc944e2 attribution removed
  (cc944e2 does not touch LICENSE or docs/).

- [ ] **V-08: Security review of the fault API.**
  SECURITY.md with a reporting channel and a threat model for the fault API:
  who can call /faults/*, what can go wrong, blast radius, and how a crashed
  runner leaving a fault active is detected and recovered (H-02 TTL).
  Files: SECURITY.md, apps/gateway/src/server.rs.
  Status: NOT IMPLEMENTED — no SECURITY.md present; no threat model for fault API.
  cc944e2 attribution removed (cc944e2 does not touch these files).

- [ ] **V-09: Competitive positioning note.**
  The closest comparison is the OpenTelemetry Demo (astronomy shop): far more
  realistic workload and ecosystem reach; Renzu has two toy services. Renzu
  cannot win on fault breadth or workload realism. Its defensible position is
  the telemetry-assertion angle (did the alert fire, is the trace present, is
  the log queryable, does the report link to evidence) plus the safety posture
  and a small, readable Rust codebase teams can fork. Record this explicitly in
  docs/ so nobody oversells the project.
  Files: docs/, README.md.
  Status: NOT IMPLEMENTED — no competitive positioning document; README.md is
  1 line (`# renzu`). cc944e2 attribution removed (cc944e2 does not touch
  docs/ or README.md).

- [ ] **V-10: CI that proves things, not just builds.**
  Badges only for jobs that really gate. E2E tests asserting traces, metrics
  and logs actually arrive (E2E-01). Add a job that runs the E2E suite against
  a freshly built stack and fails the build if any assertion fails.
  Files: .github/workflows/ci.yml, tests/e2e/.
  Status: NOT IMPLEMENTED — no tests/e2e/ exists; ci.yml e2e job requires Docker
  and depends on unresolved otel-collector/jaeger dependency; no E2E suite.
  cc944e2 attribution removed (cc944e2 does not touch ci.yml).

## Ordering

Weeks 1-2   CI green (CI-01), verification diagnostics (V-01..V-03),
            re-baseline audit (L-01, V-04), ADRs (ADR-01..06)
Weeks 3-4   Loki end to end (F-013-02), log/trace correlation (CORR-01),
            Collector health fix (HEALTH-01)
Weeks 5-7   Async open-loop loadgen (H-01), trace-linked reports (REPORT-01),
            report schema v1
Weeks 7-8   E2E suite (E2E-01), fault-endpoint auth + TTL (H-02)
--- P1 exit ---
Q+1         Config/secrets/lifecycle/limits (H-04..H-07), self-observability
            (H-08), F-021 (P-02) then F-022 (P-03), F-026 (P-04),
            remote target (P-01), first release (P-09, with approval)
Q+2 onward  Pluggable targets (L-02), F-025 re-scoped (L-03),
            F-024 only if justified (L-04)

## Notes

- This file is a proposal. The maintainer decides which tasks to keep and in
  what order. Do not start a task marked [BLOCKED] until its dependency is
  resolved, and do not begin a feature while any dependency is not verifiably
  DONE.
- One feature, one task. Split work that grows large.
- Before starting non-trivial work, state a short plan (files to touch, tests
  to add, risks) and get maintainer sign-off.
- Run cargo fmt, cargo clippy --workspace --all-targets -- -D warnings and
  cargo test --workspace before reporting completion.
- The maintainer owns architectural decisions. Propose, implement, verify and
  report — do not decide scope. Do not mark a feature DONE on code generation
  or a passing build alone.
- Do not bump versions, create tags, or cut releases without explicit approval.
