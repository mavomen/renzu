# ARCHITECTURE.md

> **Status:** Draft v0.0.0 (proposed architecture, nothing implemented yet)
> **Source:** `PRD.md`
> **Detail:** Diagrams and per-component notes live in `docs/architecture/`. Decisions live in `docs/decisions/`.

This document describes the intended system boundaries and data flow. Every element is marked with its implementation state. Nothing here is a claim of working behavior until the component is implemented and verified.

| Marker | Meaning |
|---|---|
| **Planned** | Designed, not implemented |
| **Decision pending** | Blocked on an ADR (see Section 9) |
| **Implemented** | Feature is done and verified |

Update the markers as components land. Keep this file short; move detail into `docs/architecture/`.

---

## 1. Purpose and Scope

Renzu Observability is a local-first laboratory for reproducing, observing, and diagnosing distributed-system failures. It runs on one developer machine under Docker Compose and is controlled through a Rust CLI, `renzu`.

It composes existing observability tools. It does not replace Prometheus, Grafana, Jaeger, or the OpenTelemetry Collector, and it does not implement its own storage or tracing backend. The Rust code in this project is the demo services, the load generator, the fault-injection controls, the report generator, and the CLI.

---

## 2. System Context

```mermaid
flowchart LR
    Dev([Developer]) --> CLI[renzu CLI]
    CLI -->|lifecycle| Compose[Docker Compose]
    CLI -->|scenario / load-test| Apps
    CLI -->|reads and writes| Reports[(Local run reports)]

    subgraph Apps[Demo application]
        GW[gateway] --> ORD[orders]
        ORD --> PAY[payments]
        ORD --> PG[(PostgreSQL)]
    end

    Apps -->|OTLP: traces, metrics, logs| COL[OpenTelemetry Collector]
    COL --> JAE[Jaeger]
    COL --> PROM[Prometheus]
    COL --> LOGS[(Log backend: decision pending)]
    PROM --> GRAF[Grafana]
    LOGS --> GRAF
    Dev --> JAE
    Dev --> GRAF
```

The diagram shows the target shape. The first milestone is a subset: one service, the Collector, and a trace backend (see Section 8).

---

## 3. Components

### 3.1 Demo application (**Planned**)

A small set of independently running Rust services. Their purpose is to produce realistic, controllable distributed behavior, not business value.

| Service | Role | Milestone |
|---|---|---|
| Initial service | First instrumented HTTP service with health endpoints | M1 |
| Second service | Downstream dependency; creates a real cross-service request path | M2 |
| PostgreSQL | Database dependency; target of latency and failure injection | M2 |

The directory layout proposes `gateway`, `orders`, and `payments`. Only the services needed by the current feature should exist. The names and the number of services are not fixed until M2; the PRD requires a minimum of two.

Every service:

- Uses Tokio and Axum.
- Exposes health and readiness endpoints.
- Emits structured logs, traces, and metrics using `tracing` and OpenTelemetry-compatible export.
- Propagates trace context on every outbound call.
- Never logs credentials or sensitive payloads.

### 3.2 Telemetry pipeline (**Planned**)

| Component | Responsibility | Milestone |
|---|---|---|
| OpenTelemetry Collector | Single ingest point (OTLP); batching, resource attributes, routing to backends | M1 (traces), M2 (metrics, logs) |
| Jaeger | Trace storage and UI | M1 |
| Prometheus | Metrics storage and query | M2 |
| Grafana | Dashboards for metrics (and logs, if the backend supports it) | M2 |
| Log backend | Log storage and query. **Decision pending (ADR-0002); Loki is the leading candidate** | Decision in M0, provisioned in M2 |

Services export to the Collector rather than directly to backends. This keeps backend choices out of application code and makes the pipeline itself observable. In M1, logs go to structured stdout only; they join the pipeline once the log backend is provisioned.

### 3.3 `renzu` CLI (**Planned**)

A Rust binary built with `clap`. It orchestrates the environment and experiments. It does not sit on the request path. Each command lands together with the feature that needs it.

| Command | Responsibility | Milestone |
|---|---|---|
| Skeleton, help, exit codes | Conventions for errors and exit codes | Started early (after the first service) |
| `up`, `status`, `down` | Environment lifecycle; reports partial health honestly | M4 |
| `scenario list`, `scenario run <name>` | Apply and clean up a named fault scenario | M3 |
| `load-test --scenario <name>` | Generate traffic and record measurements | M3 |
| `report <run-id>` | Read and display a saved experiment report | M3 |

Open question: whether the CLI drives Docker Compose directly or calls a documented orchestration layer (PRD Open Question 5). Record the answer as an ADR.

### 3.4 Load generator (**Planned**)

Generates bounded, configurable traffic (duration plus rate or concurrency) and records request counts, errors, throughput, and latency percentiles. It is a library used by the CLI, introduced in M3. It rejects unsafe configurations to avoid exhausting the host.

### 3.5 Scenario and fault-injection mechanism (**Planned**)

Scenarios are declarative, validated configuration. They are never executed as shell commands.

Proposed approach (to be confirmed in M3):

- Faults (database latency, error rate, downstream timeout) are enabled by application-level configuration, such as a control endpoint or configuration reload on the target service, rather than by manipulating the host.
- Control endpoints are disabled by default, enabled only through explicit configuration, and bound to `127.0.0.1`. Their parameters are bounded and validated.
- Each fault has explicit enable and disable operations.
- Cleanup is verified, not assumed. A failed cleanup is reported as a failure.
- Faults affect only project-owned containers.

### 3.6 Experiment reports (**Planned**)

Each experiment produces one local report keyed by a unique run ID, written to disk so it can be read without a running CLI process.

A report records:

- Run ID, scenario name and configuration.
- Start time, duration, and completion status (`complete` or `incomplete`).
- Workload parameters.
- Success and failure counts and latency statistics.
- References to telemetry (for example trace IDs).
- Warnings, including any missing telemetry.

The storage format and location are decided in M3. Telemetry availability and experiment completion are tracked as separate fields.

---

## 4. Request and Telemetry Flow

1. The load generator (or a manual request) sends an HTTP request to the entry service.
2. The entry service starts a root span, then calls the downstream service and PostgreSQL. Trace context travels in standard propagation headers on each outbound call.
3. Each service emits logs, spans, and metrics using OTLP to the Collector.
4. The Collector batches telemetry and exports traces to Jaeger, metrics to Prometheus, and logs to the chosen log backend.
5. The developer inspects traces in Jaeger, metrics and dashboards in Grafana, and logs through the log backend's interface.

Metrics may reach Prometheus through Collector export or direct scraping of service endpoints. This choice is made in M2 and recorded in an ADR.

---

## 5. Cross-Signal Correlation

- **Traces to logs:** Logs carry the trace ID and span ID of the active span. Whether a developer can jump from a trace to its logs, or the reverse, depends on the log backend and its Grafana integration. This is decision pending; no correlation is claimed until log-trace correlation is demonstrated in M2.
- **Metrics to traces:** Not planned for the MVP. Do not claim exemplar or other metric-to-trace links unless tested.
- **Run reports to telemetry:** Reports store references (such as trace IDs and time windows) so a reader can find the relevant telemetry. Reports do not copy telemetry.

Known correlation gaps must be documented next to the feature that has them.

---

## 6. Experiment Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Pending
    Pending --> Running: scenario enabled / load started
    Running --> Cleaning: workload finished
    Running --> Interrupted: error, signal, or timeout
    Interrupted --> Cleaning: attempt cleanup
    Cleaning --> Completed: workload finished and cleanup verified
    Cleaning --> Incomplete: interrupted, or cleanup failed
    Completed --> [*]: report written (complete)
    Incomplete --> [*]: report written (incomplete)
```

Rules:

- Every run has a unique run ID from creation.
- A run is `complete` only if the workload finished and cleanup was verified.
- Interrupted runs and failed cleanups produce a report marked `incomplete`, never silence.
- Missing or delayed telemetry is recorded as a warning, never interpreted as healthy behavior.

---

## 7. Cross-Cutting Concerns

**Failure semantics.** Startup and shutdown handle partial failure and report it. `renzu up` distinguishes fully healthy from partially healthy. `renzu down` does not claim success if required cleanup failed.

**Distinguishing faults from telemetry failures.** A fault injected by a scenario must be distinguishable from a collection or instrumentation failure. Collector and exporter health are observable in their own right.

**Security.** Services and published ports bind to `127.0.0.1` by default. No secrets are committed. Sensitive data is redacted from logs and reports. Scenario and load-test inputs are validated. Containers use least privilege where practical. Container images are pinned to explicit tags.

**Resource awareness.** The full stack is heavy for a laptop (Collector, three telemetry backends, a log backend, PostgreSQL, multiple services). Build the stack incrementally, document approximate CPU, memory, and disk needs, and bound load-test intensity.

**Reproducibility.** Toolchain and dependency versions are pinned. Experiment reports record environment and workload so comparisons can be qualified.

---

## 8. Incremental Build Plan

The architecture is built in vertical slices. Do not create components ahead of the feature that needs them.

| Slice | Contents |
|---|---|
| M1: First observable service | One service, Collector, Jaeger, structured stdout logs, minimal Compose |
| M2: Distributed | Metrics and dashboards, PostgreSQL, second service, log backend, log-trace correlation |
| M3 and M4: Incidents and CLI | Scenarios, load generation, reports, lifecycle commands, end-to-end tests |
| M5: Extensions | Profiling, comparison, SLOs, more scenarios |

---

## 9. Decisions

Significant decisions are recorded as ADRs in `docs/decisions/`.

| ADR | Topic | State |
|---|---|---|
| 0001 | Reference repository reconnaissance (license, runnable examples, versions) | Accepted (M0) |
| 0002 | Log backend selection (candidate: Loki) | Accepted (M0) |
| 0003 | Project license | Accepted (M0) |
| TBD | Minimum supported Rust toolchain | Open |
| TBD | CLI orchestration: direct Compose control vs documented layer | Open |
| TBD | Metrics path: Collector export vs Prometheus scraping | Open (M2) |
| TBD | Fault-injection control mechanism | Open (M3) |
| TBD | Report storage format and location | Open (M3) |
| TBD | Default load-test resource limits | Open (M3) |
| TBD | Profiling approach | Open (M5) |

When an ADR is accepted, update this table and any affected markers above.

---

## 10. Known Limitations and Assumptions

- This document reflects planning only. No claim here has been verified against running code.
- The Packt reference repository has not yet been inspected. Do not rely on assumptions about its structure, license, or compatibility until the M0 reconnaissance records them.
- Service names, service count, and exact telemetry paths may change after reconnaissance and early implementation.
- Linux is the primary development platform. Other platforms are untested.
- Local-machine measurements are not general performance guarantees.
