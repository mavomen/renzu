# ADR-0006: Trace Backend Selection

**Status:** Accepted
**Deciders:** Renzu maintainer
**Date:** 2026-11-07

## Context

PRD FR-03 and §8 (initial technology choices subject to verification) require a trace storage and query backend for M1. FEATURES.md F-008 scope: "Configure Jaeger or another explicitly approved trace backend." The current compose stack uses Jaeger all-in-one:1.58.0, and the Collector exports traces via `otlphttp` to `http://jaeger:4318`. This ADR records the formal decision with reasoning and rejected alternatives.

This ADR blocks H-09 (production-grade storage) and P-06 (Jaeger/Tempo decision).

## Candidates evaluated

| Criterion | Jaeger (current) | Tempo |
|---|---|---|
| Image | jaegertracing/all-in-one:1.58.0 (pinned) | grafana/tempo:2.6.0 (pinned) |
| OTLP receiver | 4317 gRPC, 4318 HTTP | 4318 HTTP |
| Collector exporter | otlphttp → jaeger:4318 | otlphttp → tempo:4318 |
| Grafana integration | Native data source | Native + TraceQL |
| Trace UI | Jaeger UI (port 16686) | Grafana Explore + Tempo UI |
| Local footprint | Moderate (single binary, embedded storage) | Lightweight (local filesystem blocks, no embedded DB) |
| Maturity | Well established, stable OTLP | Mature, Grafana-aligned, TraceQL GA in 2.0+ |

## Decision

**Select Tempo** as the trace storage and query backend, replacing Jaeger in the local Compose stack.

### Reasoning

- **Grafana integration (single-UI principle):** ADR-0002 selected Loki for logs explicitly to keep the observability UI surface to one tool (Grafana). Tempo is Grafana's native trace backend and integrates with Grafana Explore, TraceQL, and dashboard links natively. This avoids a separate Jaeger UI at port 16686 and aligns trace inspection with the existing metrics/logs workflow in Grafana.
- **OTLP ingestion path:** Tempo accepts OTLP HTTP on port 4318. The Collector's existing `otlphttp` exporter targets `http://tempo:4318` with no pipeline changes — only the endpoint hostname changes.
- **Local footprint:** Tempo's local block storage (filesystem) is lighter than Jaeger's all-in-one with embedded BadgerDB. No separate query/collector/agent processes; a single Tempo binary serves ingestion, compaction, and query.
- **Query language:** TraceQL (GA since Tempo 2.0) provides a dedicated trace query language accessible from Grafana Explore, more expressive than Jaeger's basic filter UI for complex investigations.
- **Maturity:** Tempo 2.x is production-ready, used in Grafana Cloud, and has stable OTLP ingestion. The 2.6.x line is current and well-supported.

### Rejected alternatives

- **Jaeger:** Rejected because it introduces a second UI (port 16686), diverging from the single-UI principle established for logs (ADR-0002) and metrics (Grafana dashboards). Jaeger's embedded storage and multi-process all-in-one image are heavier for local dev. Keeping Jaeger would leave trace inspection fragmented from the Grafana workflow used for metrics and logs.
- **Defer / stdout only:** Rejected because F-008 requires a queryable trace backend for trace visualization; structured stdout traces (M1 behavior) do not satisfy the acceptance criteria (find spans, durations, failures via UI).

## Consequences

### Positive

- Traces, metrics, and logs all queryable in Grafana (Explore for traces/metrics/logs, dashboards for metrics).
- Single port (3000) for the observability UI surface; no separate Jaeger UI port.
- Collector pipeline unchanged except endpoint hostname (`jaeger` → `tempo`).
- TraceQL enables advanced trace queries (root cause latency, error filtering, service-graph style queries) from Grafana.
- Lighter local resource profile (single Tempo process, filesystem blocks).

### Negative / trade-offs

- Developers familiar with Jaeger UI must learn Grafana Explore + TraceQL for trace inspection (documented in runbooks).
- Tempo does not expose a standalone UI; trace inspection requires Grafana running (already required for metrics/logs).
- Trace-to-logs correlation via traceID label (Loki) now works entirely within Grafana, but the linkage must be verified end-to-end (F-013).

### Risks

- Tempo's local block storage requires a `wal` and `blocks` directory; Compose volume mounts must be configured correctly (addressed in compose.yaml change).
- OTLP gRPC (4317) is not supported by Tempo; the Collector uses HTTP (4318) which is already the configured path. No regression.
- If Grafana is down, trace inspection is unavailable (mitigation: Grafana is already required for metrics/logs; single point of failure is accepted for local lab).

## Related decisions

- ADR-0002: Log backend selection (Loki) — established single-UI principle with Grafana.
- ADR-0005: OpenTelemetry Collector pipeline — traces pipeline uses `otlphttp` exporter; only endpoint changes.
- F-008: Distributed Trace Visualization — now implemented with Tempo + Grafana.
- F-010: Metrics Dashboards — Grafana already provisioned; Tempo adds as a data source.
- F-013: Log Pipeline and Trace Correlation — traceID linking in Loki + Grafana works with Tempo traces.
- H-09: Production-grade trace storage (blocked on this ADR).
- P-06: Jaeger/Tempo decision (resolved by this ADR).