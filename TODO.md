# TODO.md

Source: production-readiness analysis (2026-11-02). Maintainer owns all decisions.
Sections marked [PROPOSED] are suggestions awaiting your approval. Do not start a
task marked [BLOCKED] until its dependency is resolved.

## Phase 1: Quick wins (this sprint)

- [x] **CI-01: Get CI green and enforce required checks.**
  Run the last failing job, fix the failures, and make the required checks
  mandatory on the default branch. Nothing else is trustworthy until this lands.
  Files: .github/workflows/ci.yml, scripts/ci-local.sh.
  Status: [DONE] — merged in PR #1. `checks` job passes (fmt/clippy/test). `e2e` job fails on pre-existing otel-collector/jaeger dependency; tracked separately.

- [x] **V-01: Verify F-013 claims end to end.**
  F-013 (Log Pipeline and Trace Correlation) is marked IN_PROGRESS. Verification results:
  - ✅ Loki provisioned in compose.yaml (grafana/loki:3.0.0)
  - ✅ Collector logs pipeline configured (otlp → batch → loki)
  - ✅ Grafana Loki datasource provisioned
  - ❌ Applications (gateway, orders) do NOT export OTLP logs — only stdout JSON
  - ❌ Trace/span ID correlation in logs not implemented
  Status: [DONE] — F-013a (infrastructure) ready; F-013a (app export) and F-013b (correlation) outstanding. F-013-01 should be split per FEATURES.md proposal.

- [ ] **V-02: Verify gateway service wiring.**
  The gateway service is not in compose.yaml, and orders has no DATABASE_URL
  or OTLP endpoint environment variables. Either the file on main lags the
  description or the services rely on defaults. Confirm with
  `docker compose config`.
  Files: compose.yaml, apps/*/Cargo.toml.
  Status: [PROPOSED] — quick diagnostic.

- [ ] **V-03: Verify orders service env wiring.**
  orders has no DATABASE_URL or OTLP endpoint environment variables in
  compose.yaml. Confirm whether it relies on defaults or is misconfigured.
  Files: compose.yaml, apps/orders/src/config.rs.
  Status: [PROPOSED] — quick diagnostic.

- [ ] **V-04: Audit every DONE feature for real verification.**
  Each DONE feature in docs/FEATURES.md must have a named, automated
  verification (test or CI job). Demote any feature without one to
  IN_PROGRESS. This is the re-baseline audit (L-01) — run it before building
  on any DONE claim.
  Files: docs/FEATURES.md, tests/, .github/workflows/ci.yml.
  Status: [PROPOSED] — gates trust in everything else.

- [ ] **V-05: Publish benchmark methodology.**
  Every benchmark in docs/profiling.md must state hardware, versions, workload
  shape (open-loop rate), warm-up, repetitions and raw data so others can
  reproduce it. Avoid single-run headline numbers.
  Files: docs/profiling.md, docs/FEATURES.md.
  Status: [PROPOSED] — depends on P-04.

- [ ] **V-06: Add a limitations page.**
  docs/LIMITATIONS.md stating what Renzu does not do: not a production
  monitoring platform, not a replacement for Prometheus/Grafana/Jaeger, local
  only, no business-value workload, no autonomous remediation, no guaranteed
  cross-signal correlation.
  Files: docs/LIMITATIONS.md.
  Status: [PROPOSED] — quick win, high credibility value.

- [ ] **V-07: Third-party license summary.**
  Record licenses for bundled configs and images. Grafana, Loki and Tempo are
  AGPL-3.0 — Renzu orchestrates unmodified upstream images rather than
  redistributing modified copies. Confirm with a lawyer if commercial use is
  planned.
  Files: LICENSE, docs/.
  Status: [PROPOSED].

- [ ] **V-08: Security review of the fault API.**
  SECURITY.md with a reporting channel and a threat model for the fault API:
  who can call /faults/*, what can go wrong, blast radius, and how a crashed
  runner leaving a fault active is detected and recovered (H-02 TTL).
  Files: SECURITY.md, apps/gateway/src/server.rs.
  Status: [PROPOSED] — depends on H-02.

- [ ] **V-09: Competitive positioning note.**
  The closest comparison is the OpenTelemetry Demo (astronomy shop): far more
  realistic workload and ecosystem reach; Renzu has two toy services. Renzu
  cannot win on fault breadth or workload realism. Its defensible position is
  the telemetry-assertion angle (did the alert fire, is the trace present, is
  the log queryable, does the report link to evidence) plus the safety posture
  and a small, readable Rust codebase teams can fork. Record this explicitly in
  docs/ so nobody oversells the project.
  Files: docs/, README.md.
  Status: [PROPOSED].

- [ ] **V-10: CI that proves things, not just builds.**
  Badges only for jobs that really gate. E2E tests asserting traces, metrics
  and logs actually arrive (E2E-01). Add a job that runs the E2E suite against
  a freshly built stack and fails the build if any assertion fails.
  Files: .github/workflows/ci.yml, tests/e2e/.
  Status: [PROPOSED] — depends on E2E-01.

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
