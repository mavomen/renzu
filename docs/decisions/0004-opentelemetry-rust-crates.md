# ADR-0004: OpenTelemetry Rust Crate Selection

**Status:** Accepted
**Deciders:** Renzu maintainer
**Date:** 2026-10-31

## Context

F-006 (Distributed Trace Instrumentation) adds OpenTelemetry-compatible trace
instrumentation to the `gateway` HTTP service. A test request must produce an
exported trace verifiable against a test OTLP receiver. The crate set must:

- Provide an OpenTelemetry SDK tracer provider (`opentelemetry_sdk`).
- Export spans over OTLP to a collector (`opentelemetry_otlp`).
- Bridge `tracing` spans to OpenTelemetry (`tracing_opentelemetry`).
- Pin versions compatible with the workspace toolchain (MSRV 1.75.0 per
  `rust-toolchain.toml`) and with `tracing >= 0.1.40` (required by
  `tracing_opentelemetry` 0.34.0).

## Decision

Adopt the OpenTelemetry Rust crates, pinned in `Cargo.toml` workspace
dependencies:

| Crate | Version | Purpose |
|---|---|---|
| `opentelemetry` | `0.33.1` | Core API (KeyValue, TracerProvider trait) |
| `opentelemetry_sdk` | `0.33.1` | TracerProvider/SDK, simple span processor |
| `opentelemetry_otlp` | `0.33.1` | OTLP span exporter (HTTP/protobuf transport) |
| `tracing_opentelemetry` | `0.34.0` | Tracing layer bridging spans to OpenTelemetry |

Version rationale:

- All four are current as of the decision date and share the `0.33.x`/0.34.0
  release line, which keeps proto definitions and the OTLP wire format aligned.
- `tracing_opentelemetry` 0.34.0 requires `tracing >= 0.1.40`; the workspace
  `tracing = "0.1"` resolves to a compatible `0.1.x` (verified via `cargo
  check`; `cargo update -p tracing` if a strict pin is ever needed).
- MSRV 1.75.0 satisfies the workspace toolchain (`rust-toolchain.toml` pins
  1.97.0, well above the minimum).

## Transport and endpoint

The `gateway` exporter uses OTLP over HTTP/protobuf (`opentelemetry_otlp`
default `http-proto` feature), endpoint `http://127.0.0.1:4317/v1/traces` by
default, overridable via `RENZU_OTLP_ENDPOINT`. HTTP/protobuf was chosen over
gRPC/tonic because:

- It keeps the dependency surface minimal (no `tonic` in the gateway binary or
  integration tests).
- The integration test receiver is a plain HTTP server, making the F-006
  acceptance criterion ("test request produces an exported trace, verified
  against a test OTLP receiver") straightforward to verify without a gRPC
  test server.

Operators matching the Compose collector (F-003/F-007) should set
`RENZU_OTLP_ENDPOINT=http://127.0.0.1:4318/v1/traces` to use the collector's
OTLP HTTP receiver. The HTTP exporter posts to the endpoint URL as-is, so the
configured endpoint must include the `/v1/traces` path.

## Resource attributes

Every span carries:

- `service.name = "gateway"`
- `service.version = CARGO_PKG_VERSION`

These are consistent with the resource-attribute convention planned for F-008
and F-010, so trace backends (Jaeger) and dashboards (Grafana) can group by
service without drift.

## Span naming

Gateway handler spans are named `GET /path` (e.g. `GET /health`, `GET /ready`,
`GET /`), matching the literal Axum routes. Request attributes `http.method`
and `http.route` are recorded on every span; error spans record `error` so the
`tracing_opentelemetry` layer maps them to OpenTelemetry exceptions.

## Shutdown and failure behavior

`Telemetry::shutdown()` calls `SdkTracerProvider::shutdown_with_timeout` with a
5s timeout to flush in-flight spans before exit. Export failures and shutdown
timeouts are logged to stderr and are never fatal — the request is still served
and the exit code is unaffected (observability conventions §7: telemetry export
failures must be observable, never silently discarded, and never fatal).

## Consequences

### Positive

- Four well-maintained crates from the OpenTelemetry Rust project; no bespoke
  trace wire format.
- HTTP/protobuf exporter is simple to test and operates over the same OTLP
  endpoint shape the collector (F-007) and Jaeger (F-008) already use.
- Shutdown flush with timeout prevents losing in-flight spans on process exit.

### Negative / trade-offs

- OTLP trace export remains beta in opentelemetry 0.33.x; acceptable for F-006
  per documented stability, revisit for F-007/F-008 hardening.
- Default endpoint `http://127.0.0.1:4317` assumes an OTLP receiver on that
  port; operators must set `RENZU_OTLP_ENDPOINT` to match their collector's
  configured receiver (HTTP 4318 or gRPC 4317).
- gRPC/tonic export (lower latency, binary framing) is intentionally deferred;
  it can be added later by switching the exporter transport without changing
  span instrumentation.

## Related decisions

- F-006: Distributed Trace Instrumentation (this work).
- F-007: OpenTelemetry Collector Pipeline (consumes exported traces).
- F-008: Distributed Trace Visualization (Jaeger backend).
- AGENTS.md §7: observability conventions (export failures observable, never
  fatal; no high-cardinality labels).
