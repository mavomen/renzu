F-014: Rust Developer CLI Foundation — DONE (merged into develop as 582ae2f via feat/f-015-environment-lifecycle merge).
F-015: Environment Lifecycle Commands — DONE (merged into develop as 582ae2f).
F-013: Log Pipeline and Trace Correlation — DONE (FEATURES.md marks DONE; Loki in compose.yaml, collector logs pipeline, Grafana datasource provisioned).
F-012: Second Rust Service — DONE (merged into develop; orders crate, trace context propagation, connected trace integration test).
F-011: PostgreSQL Integration — DONE (merged into develop as 89a014b; PostgreSQL in compose.yaml, sqlx connection pool, DB-backed route, traced DB operations).
F-010: Metrics Dashboards — DONE (branch feat/f-010-metrics-dashboards; collector metrics pipeline debug→prometheus; Prometheus scrapes otel-collector:9464; Grafana datasource + gateway-metrics dashboard; end-to-end verified).
F-009: Metrics Instrumentation — DONE (implemented on develop as 2ddc3c3; metrics.rs with OTLP exporter, MetricsLayer tower layer, unit tests with in-memory exporter; wired into server.rs).
F-008: Distributed Trace Visualization — DONE (merged into develop as f28fe87; Jaeger in compose, runbook, e2e script; error span recording; error_span_exported_to_otlp_receiver test).
F-007: OpenTelemetry Collector Pipeline — DONE (merged into develop as 6d41371; collector config per ADR-0005; compose healthcheck; end-to-end trace flow verified).
F-006: Distributed Trace Instrumentation — DONE (merged into develop as 2aaa517; gateway OTel provider, spans, OTLP receiver integration test).
F-005: Structured Application Logging — DONE (merged into develop as 6e67efe; JSON stdout logging, service.name/service.version/environment fields, RENZU_ENVIRONMENT, Redacted redaction helper, unit tests).
F-004: Rust HTTP Service — DONE (implemented on develop; HTTP service, config, health/readiness endpoints, error handling, tests).
