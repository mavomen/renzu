# ADR-0002: Log Backend Selection

**Status:** Accepted
**Deciders:** Renzu maintainer
**Date:** 2026-10-28

## Context

PRD §6 (FR-03) and §8 require selecting a log storage/query backend in M0,
before any log-pipeline or log-correlation work (F-013). FEATURES.md F-001
scope requires recording the log-backend choice with reasoning and rejected
alternatives. Loki is the leading candidate named in PRD §8 and FEATURES.md,
but the decision was not final until this ADR.

This ADR blocks F-013 (Log Pipeline and Trace Correlation). No log-pipeline
or log-correlation work may begin until this ADR is accepted.

## Candidates evaluated

| Candidate | Local footprint | OTLP ingestion | Grafana integration | trace-ID linking | Notes |
|---|---|---|---|---|---|
| **Loki** | Small (single Go binary, embedded storage) | Native OTLP `/loki/api/v1/push` | Native (Grafana data source) | Via `traceID` label + LogQL | Leading candidate; purpose-built for logs + Grafana |
| Elasticsearch | Heavy (JVM, high RAM/disk) | Via agents/Filebeat/OTEL | Available but secondary | Possible but complex | Overkill for local dev lab |
| Jaeger | Traces only | Traces (OTLP) | Limited | N/A | Not a general log store; already selected for traces (F-008) |
| Defer (stdout only) | None | N/A | N/A | N/A | Already M1 behavior (structured stdout logs); defers the decision, does not satisfy F-013 |

## Decision

**Select Loki** as the log storage and query backend, provisioned in M2
alongside the other telemetry backends.

### Reasoning

- **OTLP ingestion path:** Loki exposes an OTLP logs endpoint, so the Renzu
  OpenTelemetry Collector (F-007) can route logs to Loki through the same OTLP
  pipeline used for traces and metrics — no separate log shipper required.
- **Grafana integration:** Loki is a first-class Grafana data source, matching
  the existing Grafana decision (F-010) and keeping the observability UI
  surface to one tool.
- **trace-ID linking:** Loki supports the `traceID` label on log streams, and
  Grafana can link log lines to traces by trace ID. This directly supports
  FR-03 correlation ("correlate an application request with its trace and
  relevant logs") and ARCHITECTURE.md §5 (traces-to-logs correlation).
- **Local footprint:** Single binary, modest CPU/RAM, embeds storage (BoltDB
  or filesystem chunks), suitable for a developer laptop.
- **Maturity:** Widely adopted, stable OTLP and LogQL APIs, good local
  documentation.

### Rejected alternatives

- **Elasticsearch:** Rejected on resource footprint (JVM, high RAM/disk) and
  operational complexity for a local-first lab. Reconsider only if full-text
  search requirements emerge (P1+).
- **Jaeger:** Rejected because it is a trace store, not a general log backend;
  already selected for traces (F-008). Do not repurpose it for logs.
- **Defer / stdout only:** Rejected for M2+ because F-013 requires logs to
  reach a queryable backend. Structured stdout logging remains the M1 behavior
  (F-005) and continues as the application-side emission target.

## Consequences

### Positive
- Logs, traces, and metrics all flow through the OpenTelemetry Collector to
  Grafana-backed backends (Loki for logs, Jaeger for traces, Prometheus for
  metrics).
- Single UI (Grafana) for metrics, dashboards, and log querying.
- Log-to-trace correlation possible via `traceID` label (to be demonstrated in
  F-013).

### Negative / trade-offs
- Loki query language (LogQL) is distinct from PromQL and Jaeger query; the
  developer must learn LogQL for log inspection (documented in runbooks).
- trace-ID linking depends on the application including trace/span IDs in
  structured log fields (F-005/F-013); correlation is best-effort and gaps
  must be documented (ARCHITECTURE.md §5).

### Risks
- Loki trace-ID linking via label has cardinality implications if misused
  (high-cardinality labels). Mitigation: include `traceID` only, not arbitrary
  request IDs, as labels (AGENTS.md §7 high-cardinality guidance).
- Loki is an additional process in the Compose stack; resource planning
  (NFR-07) must account for it.

## Related decisions

- ADR-0001: reference repository reconnaissance (MIT license; no code copied).
- F-003: Compose will provision Loki on `127.0.0.1` with a pinned image tag.
- F-007: Collector routes logs to Loki via OTLP.
- F-010: Grafana provisions Loki as a data source.
- F-013: Log pipeline and trace correlation demonstrates Loki trace-ID linking.
