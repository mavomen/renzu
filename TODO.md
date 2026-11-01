F-014: Rust Developer CLI Foundation — DONE (merged into develop as 582ae2f via feat/f-015-environment-lifecycle merge).
F-015: Environment Lifecycle Commands — DONE (up/status/down driving compose.yaml via docker compose; merged into develop as 582ae2f).
F-006: Distributed Trace Instrumentation — DONE (implemented on branch feat/f-006-trace-instrumentation; gateway OTel provider, spans, and OTLP receiver integration test added; awaiting verification review).
F-005: Structured Application Logging — IN_PROGRESS (branch feat/f-005-structured-application-logging; JSON stdout logging, service.name/service.version/environment fields, RENZU_ENVIRONMENT, Redacted redaction helper).
F-007: OpenTelemetry Collector Pipeline — IN_PROGRESS (branch feat/f-007-otel-collector-pipeline; collector config + compose healthcheck + runbook + FEATURES.md + ADR-0005).
F-008: Distributed Trace Visualization — IN_PROGRESS (branch feat/f-008-distributed-trace-visualization; E2E script + CI steps + runbook docs).
F-009: Metrics Instrumentation — IN_PROGRESS (branch feat/f-009-metrics-instrumentation; OpenTelemetry metrics provider, request rate/error/duration instruments with low-cardinality http.method/http.status_code labels, tower MetricsLayer, unit tests with in-memory exporter).
F-010: Metrics Dashboards — IN_PROGRESS (branch feat/f-010-metrics-dashboards; stacked on F-009; Prometheus + Grafana in compose, collector metrics pipeline to Prometheus, Grafana datasource + initial dashboard).
