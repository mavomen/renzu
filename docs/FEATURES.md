# Feature List: Renzu Observability

> **Status:** Draft v0.0.0
> **Source of truth:** `PRD.md`
> **Location:** `docs/FEATURES.md`
> **Purpose:** Track product capabilities, implementation scope, dependencies, and acceptance criteria.
> **Priority levels:** P0 = MVP, P1 = first extension, P2 = future enhancement.
> **Status values:** `PLANNED`, `IN_PROGRESS`, `BLOCKED`, `DONE`, `DEFERRED`.

---

## 1. Feature Management Rules

1. Every feature must have a unique, stable identifier.
2. `PRD.md` defines product requirements; this file tracks their implementation.
3. Features must be implemented in dependency order.
4. Each feature must have an observable acceptance criterion.
5. A feature is `DONE` only after implementation, relevant verification, and documentation are complete.
6. Do not mark features complete based solely on a successful build.
7. Do not implement deferred features without an explicit scope decision.
8. Split features that require multiple independently verifiable deliverables.
9. Update this file when implementation changes the scope or status of a feature.
10. Do not create empty modules, placeholder crates, or speculative abstractions merely to match the proposed directory structure.
11. P1 and P2 features are a loose backlog. Do not detail, split, or schedule them until the MVP (all P0 features) is accepted.
12. Every feature must be verifiable using only its own dependencies. If an acceptance criterion needs a component from a later feature, move the criterion to that feature.

## 2. Status Summary

| Priority | Total planned | Scope |
|---|---:|---|
| P0 | 18 | Minimum viable product |
| P1 | 5 | Profiling and reliability extensions |
| P2 | 3 | Advanced diagnostics and AI |
| **Total** | **26** | |

All features begin as `PLANNED`. Update counts and statuses as implementation progresses.

### Milestone-to-feature mapping

| PRD milestone | Features |
|---|---|
| M0: Reconnaissance | F-001 |
| M1: First observable service | F-002, F-003, F-004, F-005, F-006, F-007, F-008 |
| M2: Distributed tracing | F-009, F-010, F-011, F-012, F-013 |
| M3: Reproducible incidents | F-016 (baseline and slow-database first), F-017, F-018 |
| M4: CLI and developer experience | F-014 (started early), F-015, CLI wiring from F-016 to F-018, end-to-end CI from F-018 |
| M5: Performance and reliability extensions | F-019 to F-023 |
| Outside milestone plan | F-024 to F-026 |

The stages in Section 6 are an implementation ordering, not a restatement of milestones. For example, Stage B includes F-009 and F-010, which belong to milestone M2.

### CLI command-to-feature mapping

| Command | Built by |
|---|---|
| CLI skeleton, `--help`, exit codes | F-014 |
| `renzu up`, `renzu status`, `renzu down` | F-015 |
| `renzu scenario list`, `renzu scenario run <name>` | F-016 |
| `renzu load-test --scenario <name>` | F-017 |
| `renzu report <run-id>` | F-018 |

---

## 3. P0: MVP Features

### F-001: Reference Repository Reconnaissance

**Status:** `DONE`

**Objective:** Understand the Packt reference implementation before adopting its code or architecture, and settle the decisions that block later features.

Scope:
- Inspect repository structure, license, notices, and documentation.
- Identify runnable examples and required services.
- Record Rust toolchain and dependency versions.
- Run at least one reference example when practical.
- Document compatibility issues and useful implementation patterns.
- Select the log storage and query backend (candidate: Loki) and record the decision. Compare candidates on local resource footprint, OTLP ingestion path, Grafana integration, and trace-ID linking.
- Record findings in `decisions/0001-reference-recon.md` and the log backend decision in `decisions/0002-log-backend.md`.

Acceptance criteria:
- The reference repository has been inspected.
- Licensing and reuse constraints are documented.
- At least one example has been evaluated.
- Initial architecture assumptions are recorded.
- Unverified claims about the reference implementation are explicitly marked.
- The log backend is chosen, with the reasoning and rejected alternatives recorded in an ADR.

Dependencies: None.

### F-002: Development Toolchain and CI

**Status:** `DONE`

**Objective:** Establish a reproducible development workflow.

Scope:
- Configure the Rust toolchain.
- Establish workspace conventions (workspace version `0.0.0`, `publish = false`).
- Configure formatting and linting.
- Add GitHub Actions for relevant checks.
- Document local verification commands.
- Choose and add the project `LICENSE`; record the choice in an ADR.
- Review `ARCHITECTURE.md` against the decisions from F-001 and update it where they differ.

Acceptance criteria:
- The project builds from a clean checkout.
- Formatting and lint checks run in CI.
- Automated tests run in CI.
- Toolchain requirements are documented.
- A license file exists and the choice is recorded.

Dependencies: F-001.

### F-003: Local Infrastructure Orchestration

**Status:** `DONE`

**Objective:** Provide a repeatable local environment.

Scope:
- Configure Docker Compose.
- Add required infrastructure services incrementally, each with a pinned image tag.
- Configure persistent storage only where required.
- Publish ports on `127.0.0.1` only.
- Add health checks and documented ports.
- Define startup dependencies and shutdown behavior.
- Document approximate resource requirements.

Acceptance criteria:
- The infrastructure starts using documented commands.
- Health status distinguishes ready services from unhealthy services.
- Startup failures produce actionable diagnostics.
- Shutdown behavior is documented and tested.

Dependencies: F-002.

### F-004: Rust HTTP Service

**Status:** `IN_PROGRESS`

**Objective:** Establish the first independently runnable application service.

Scope:
- Implement a minimal Rust HTTP service.
- Add configuration handling.
- Add health and readiness endpoints where appropriate.
- Handle invalid requests and application errors.
- Add unit and integration tests.

Acceptance criteria:
- The service starts using documented configuration.
- Health endpoints report appropriate states.
- Invalid input produces appropriate HTTP errors.
- Tests cover successful and failed requests.

Dependencies: F-002.

### F-005: Structured Application Logging

**Status:** `IN_PROGRESS`

**Objective:** Produce useful, structured diagnostic logs.

Scope:
- Configure structured (JSON) logging using the Rust tracing ecosystem, written to stdout.
- Define service and environment fields.
- Include request correlation information where available.
- Configure log levels.
- Prevent sensitive information from entering logs.
- Use a field layout that can later be shipped to the backend chosen in F-001.

Acceptance criteria:
- Logs are structured and can be queried with documented local tooling (for example `jq` on the JSON output).
- Relevant errors contain sufficient diagnostic context.
- Log levels can be configured.
- Automated tests or inspection procedures verify sensitive-data redaction.

Dependencies: F-001 (log field conventions), F-004.

### F-006: Distributed Trace Instrumentation

**Status:** `IN_PROGRESS`

**Objective:** Trace operations within an application service.

Scope:
- Add OpenTelemetry-compatible instrumentation.
- Configure trace providers and exporters.
- Instrument HTTP request handling.
- Record errors and relevant operation attributes.
- Configure resource attributes consistently.
- Pin and record OpenTelemetry and `tracing-opentelemetry` crate versions.

Acceptance criteria:
- A test request produces an exported trace, verified against a test OTLP receiver in an integration test. (End-to-end delivery through the Collector is verified in F-007 and F-008.)
- Trace IDs and span relationships are inspectable.
- Error spans contain appropriate diagnostic information.
- Export failures are not silently discarded.

Dependencies: F-003, F-004.

### F-007: OpenTelemetry Collector Pipeline

**Status:** `IN_PROGRESS`

**Objective:** Centralize telemetry collection and export.

Scope:
- Configure the OpenTelemetry Collector.
- Receive OTLP from the application.
- Export traces to the selected trace backend.
- Define the metrics pipeline. Delivery to the metrics backend is verified in F-010, once that backend exists.
- Document batching, resource attributes, and error handling.
- Log routing is out of scope here; it is handled in F-013 after the log backend is provisioned.

Acceptance criteria:
- The Collector receives telemetry from the application.
- Traces reach the configured trace backend.
- Collector health and export failures are observable.
- Unsupported or unavailable pipeline capabilities are documented.

Dependencies: F-003, F-006.

### F-008: Distributed Trace Visualization

**Status:** `IN_PROGRESS`

**Objective:** Inspect trace spans and operation timing.

Scope:
- Configure Jaeger or another explicitly approved trace backend.
- Expose the trace UI locally.
- Document trace lookup and investigation.
- Verify resource and service attributes.

Acceptance criteria:
- A developer can find and inspect an application trace.
- Span relationships and durations are visible.
- Failed operations can be identified.
- UI access instructions are documented.

Dependencies: F-007.


### F-009: Metrics Instrumentation

**Status:** `IN_PROGRESS`

**Objective:** Expose application health and performance metrics.

Scope:
- Record request rate.
- Record request error counts.
- Record request duration distributions.
- Define service-level metric names and labels.
- Avoid high-cardinality labels.

Acceptance criteria:
- Metrics are emitted by the service and can be inspected locally (for example at an endpoint or a test receiver).
- Request rate, errors, and latency can be evaluated.
- Metric units and label semantics are documented.
- Automated checks verify that expected metrics are emitted.

Dependencies: F-004, F-007.

### F-010: Metrics Dashboards

**Status:** `IN_PROGRESS`

**Objective:** Make service health and performance easy to inspect.

Scope:
- Configure Prometheus and verify the metrics path chosen in the ADR (Collector export or scraping).
- Configure Grafana.
- Provision an initial dashboard.
- Visualize request rate, error rate, and latency.
- Document how to inspect and troubleshoot the dashboard.

Acceptance criteria:
- Prometheus scrapes or receives the expected metrics.
- Metrics sent through the Collector reach the metrics backend.
- Grafana displays real application measurements.
- Dashboard configuration is reproducible.
- Missing telemetry is not presented as a healthy zero-value measurement without qualification.

Dependencies: F-009.

### F-011: PostgreSQL Integration

**Status:** `DONE`

**Objective:** Introduce a database dependency into the demonstration system.

Scope:
- Provision PostgreSQL locally.
- Implement a minimal database-backed operation.
- Configure connection management.
- Handle database failures and timeouts.
- Instrument database operations where supported.

Acceptance criteria:
- Database-backed requests work in the normal scenario.
- Database errors produce appropriate application behavior.
- Database operations appear in traces where instrumentation supports them.
- Connection and migration procedures are documented.

Dependencies: F-003, F-004, F-006.

### F-012: Second Rust Service

**Status:** `DONE`

**Objective:** Establish a genuinely distributed request path.

Scope:
- Add a second independently running Rust service.
- Implement an HTTP request between services.
- Propagate distributed trace context.
- Instrument outbound and inbound requests.
- Handle downstream failures.

Acceptance criteria:
- Both services run independently.
- A request traversing both services produces a connected trace.
- Downstream failures are represented in telemetry.
- Service boundaries and configuration are documented.

Dependencies: F-006, F-007, F-008. (F-006 and F-007 provide the instrumentation and export path; F-008 is required to verify the connected trace.)

### F-013: Log Pipeline and Trace Correlation

**Status:** `DONE`

**Objective:** Ship application logs to the chosen backend and connect them with distributed traces.

Scope:
- Provision the log backend chosen in F-001.
- Route logs through the Collector to that backend.
- Include trace and span identifiers in relevant logs.
- Configure backend correlation as supported by the chosen backend.
- Define consistent service attributes.
- Document correlation limitations.

Acceptance criteria:
- Logs from a traced request reach the log backend and are queryable there.
- Those logs contain the appropriate trace identifiers.
- A developer can use those identifiers to locate the corresponding trace.
- Correlation behavior is tested or demonstrated.
- No unsupported cross-backend correlation is claimed.

Dependencies: F-001 (log backend decision), F-005, F-007, F-008, F-012.

### F-014: Rust Developer CLI Foundation

**Status:** `DONE`

**Objective:** Create the `renzu` command-line application.

Scope:
- Configure CLI argument parsing.
- Establish command and error-handling conventions.
- Add top-level help.
- Define exit-code behavior.
- Add command-level tests.
- Implement only the skeleton; subcommands are added by F-015 to F-018.

Acceptance criteria:
- The CLI builds and displays useful help.
- Invalid arguments produce clear errors.
- Exit codes distinguish success from failure.
- Command behavior is documented.

Dependencies: F-002. Scheduled after F-004 so the first real subcommand arrives soon after the skeleton.

### F-015: Environment Lifecycle Commands

**Status:** `DONE`

**Objective:** Manage the local environment through the CLI.

Scope:
- Implement `renzu up`.
- Implement `renzu status`.
- Implement `renzu down`.
- Integrate with the selected orchestration mechanism.
- Handle partial startup and shutdown failures.

Acceptance criteria:
- Commands operate on the documented project environment.
- Status reflects actual component health.
- Partial failures are reported.
- Shutdown does not claim success when required cleanup fails.

Dependencies: F-003, F-014.

### F-016: Baseline and Fault-Injection Scenarios

**Status:** `IN_PROGRESS`

**Objective:** Provide reproducible system behaviors for investigation.

Scope:
- Define a baseline scenario.
- Introduce controlled database latency.
- Introduce a downstream timeout.
- Define a controlled error scenario.
- Document scenario parameters and cleanup.
- Add cascading-failure behavior after the individual failure modes work.
- Implement `renzu scenario list` and `renzu scenario run <name>`, with explicit run identifiers.

Acceptance criteria:
- Scenarios have explicit configuration and expected outcomes.
- Faults can be enabled and disabled safely.
- Fault control endpoints are disabled by default and bound to `127.0.0.1`.
- Each scenario produces inspectable telemetry.
- Cleanup restores the expected baseline state.
- Scenarios do not execute arbitrary untrusted shell commands.
- `scenario list` and `scenario run` have tests for success and failure paths.

Dependencies: F-008, F-010, F-011, F-012, F-014.

### F-017: Load Generation and Measurement

**Status:** `IN_PROGRESS`

**Objective:** Generate controlled traffic and measure system behavior.

Scope:
- Configure test duration.
- Configure concurrency or request rate.
- Record successful and failed requests.
- Calculate throughput and latency percentiles.
- Record workload configuration.
- Bound resource consumption.
- Implement `renzu load-test --scenario <name>`.

Acceptance criteria:
- Results include request counts, errors, throughput, and supported latency statistics.
- Invalid or unsafe configurations are rejected.
- Workload settings are included in the result.
- Measurements can be repeated under documented conditions.
- `load-test` has tests for success and failure paths.

Dependencies: F-012, F-014.

### F-018: Experiment Reports and End-to-End Verification

**Status:** `PLANNED`

**Objective:** Turn scenario executions into reproducible investigations.

Scope:
- Generate unique run identifiers.
- Record scenario and workload configuration.
- Record timestamps and completion state.
- Include relevant measurements and telemetry references.
- Show a minimal side-by-side of a baseline run and a faulted run. General comparison of arbitrary runs is F-020.
- Implement `renzu report <run-id>`.
- Add end-to-end tests for the primary user journeys.

Acceptance criteria:
- Every completed experiment produces a local report.
- Interrupted experiments are explicitly marked incomplete.
- Missing telemetry is reported as a diagnostic limitation.
- A clean checkout can reproduce a documented latency investigation.
- The README documents the complete workflow.
- `report` has tests for success and failure paths (including unknown run IDs).

Dependencies: F-013, F-015, F-016, F-017.

---

## 4. P1: Reliability and Performance Extensions

> Loose backlog. Do not refine or schedule until all P0 features are accepted.

### F-019: Profiling Workflow

**Priority:** P1
**Status:** `PLANNED`

Scope:
- Establish reproducible CPU profiling.
- Document memory investigation.
- Investigate async-task performance.
- Provide a repeatable profiling workload.

Acceptance criteria:
- Profiling instructions work on the documented environment.
- Profiling results can be inspected.
- Tooling overhead and limitations are documented.

Dependencies: F-018.

### F-020: Performance Comparison Reports

**Priority:** P1
**Status:** `PLANNED`

Scope:
- Compare any two experiment runs (generalizing the baseline-vs-faulted view from F-018).
- Identify changes in throughput, errors, and latency.
- Record environment and workload differences.
- Avoid misleading comparisons when configurations differ.

Acceptance criteria:
- Reports show metric deltas and the underlying measurements.
- Incompatible runs are flagged.
- Benchmark claims remain traceable to the recorded experiments.

Dependencies: F-018.

### F-021: Service-Level Indicators

**Priority:** P1
**Status:** `PLANNED`

Scope:
- Define availability and latency SLIs.
- Make measurement windows configurable.
- Calculate compliance with explicitly defined thresholds.
- Test calculation logic.

Acceptance criteria:
- SLI definitions are documented.
- Calculations are covered by tests.
- Insufficient data is distinguished from successful compliance.

Dependencies: F-010, F-018.

### F-022: SLO and Error-Budget Reporting

**Priority:** P1
**Status:** `PLANNED`

Scope:
- Define configurable SLO targets.
- Calculate error budgets.
- Display observed performance against targets.
- Include SLO results in experiment reports where appropriate.

Acceptance criteria:
- Calculations use documented definitions and time windows.
- Error-budget results are tested.
- Reports distinguish configured objectives from observed measurements.

Dependencies: F-021.

### F-023: Expanded Failure Scenarios

**Priority:** P1
**Status:** `PLANNED`

Scope:
- Extend cascading-failure demonstrations beyond the initial scenario in F-016.
- Test telemetry Collector unavailability.
- Test database connection failure.
- Test recovery after faults.
- Document the symptoms and investigation workflow for each scenario.

Acceptance criteria:
- Each scenario is reproducible.
- Recovery behavior is verified.
- Application failures and telemetry failures are distinguishable.
- Scenario cleanup is tested.

Dependencies: F-016, F-018.

---

## 5. P2: Advanced Capabilities

> Loose backlog. Do not refine or schedule until the MVP and relevant P1 work are accepted.

### F-024: Trace-to-Profile Correlation

**Priority:** P2
**Status:** `PLANNED`

Scope:
- Investigate correlation between slow traces and profiling samples.
- Build a reproducible demonstration workload.
- Show relevant profiling evidence.
- Document correlation limitations.

Acceptance criteria:
- The correlation mechanism is technically justified.
- Results are reproducible.
- The feature does not claim unsupported per-span CPU attribution.

Dependencies: F-019, F-020.

### F-025: Explainable Incident Assistant

**Priority:** P2
**Status:** `PLANNED`

Scope:
- Accept bounded telemetry evidence.
- Generate diagnostic hypotheses using an optional local model.
- Reference the evidence supporting each hypothesis.
- Separate observations from inferred explanations.
- Keep model execution optional.

Acceptance criteria:
- The core platform works without an AI model.
- Every generated hypothesis identifies its supporting evidence where available.
- Missing evidence and uncertainty are explicit.
- The assistant cannot autonomously modify the environment or remediate incidents.

Dependencies: F-018.

### F-026: Resource and Telemetry Overhead Analysis

**Priority:** P2
**Status:** `PLANNED`

Scope:
- Measure application behavior with instrumentation enabled and disabled where practical.
- Record CPU and memory usage.
- Evaluate sampling and batching configurations.
- Compare telemetry volume and overhead.

Acceptance criteria:
- Measurements document the workload and environment.
- Results distinguish observed differences from causal conclusions.
- Configuration changes and their effects are recorded.

Dependencies: F-019, F-020.

---

## 6. Implementation Order

The recommended implementation order follows dependencies, not feature IDs alone.

### Stage A: Establish the foundation

1. F-001: Reference repository reconnaissance (includes log backend ADR).
2. F-002: Development toolchain and CI.
3. F-003: Local infrastructure orchestration.
4. F-004: First Rust HTTP service.
5. F-014: Rust CLI foundation.

### Stage B: Make one service observable

6. F-005: Structured application logging.
7. F-006: Distributed trace instrumentation.
8. F-007: OpenTelemetry Collector pipeline.
9. F-008: Distributed trace visualization.
10. F-009: Metrics instrumentation.
11. F-010: Metrics dashboards.

### Stage C: Introduce distributed behavior

12. F-011: PostgreSQL integration.
13. F-012: Second Rust service.
14. F-013: Log pipeline and trace correlation.

### Stage D: Reproduce and investigate failures

15. F-015: Environment lifecycle commands.
16. F-016: Baseline and fault-injection scenarios (+ `scenario` commands).
17. F-017: Load generation and measurement (+ `load-test` command).
18. F-018: Experiment reports and end-to-end verification (+ `report` command).

### Stage E: Extend after MVP acceptance

19. F-019: Profiling workflow.
20. F-020: Performance comparison reports.
21. F-021: Service-level indicators.
22. F-022: SLO and error-budget reporting.
23. F-023: Expanded failure scenarios.
24. F-024: Trace-to-profile correlation.
25. F-025: Explainable incident assistant.
26. F-026: Resource and telemetry overhead analysis.

Stages may be adjusted after reconnaissance, but dependency changes must be documented.

---

## 7. Definition of Done

A feature may be marked `DONE` only when the following apply. Items that clearly do not apply to a documentation-only or configuration-only feature may be marked N/A with a one-line reason.

- [ ] Its scope matches the requirements.
- [ ] The implementation is complete.
- [ ] Relevant automated tests pass.
- [ ] Relevant formatting and lint checks pass.
- [ ] Failure behavior has been tested or documented.
- [ ] Security implications have been reviewed.
- [ ] User-facing documentation is updated.
- [ ] Acceptance criteria are demonstrably satisfied.
- [ ] Known limitations are documented.
- [ ] The maintainer has reviewed the changes.

Do not mark a feature complete when required verification could not be run. Record the limitation and use `BLOCKED` or leave the feature `IN_PROGRESS`.

---

## 8. Change Management

When a feature changes:

1. Update this file.
2. Update `PRD.md` if product scope or requirements change.
3. Update `ARCHITECTURE.md` (and `architecture/` where relevant) if system boundaries, dependencies, or design decisions change.
4. Add or update an architectural decision record in `decisions/` for significant choices.
5. Update tests and operational documentation.
6. Verify the affected functionality.

Do not duplicate implementation details in this backlog. Keep feature descriptions focused on outcomes, scope, dependencies, and acceptance criteria.

---

## 9. Immediate Next Action

Start with **F-001: Reference Repository Reconnaissance**.

Do not implement the entire feature list in a single task.

First establish what the reference repository actually contains, which examples run, what can legally be reused, and which dependencies remain compatible. Record the findings in `decisions/0001-reference-recon.md`, choose the log backend in `decisions/0002-log-backend.md`, and then use those findings to validate the proposed architecture before proceeding to F-002 and F-003.
