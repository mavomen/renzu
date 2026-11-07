# PRD: Renzu Observability

> **Status:** Draft v0.0.0
> **Project type:** Open-source, local-first observability engineering platform
> **Primary language:** Rust
> **Initial deployment:** Local development environment using Docker Compose
> **Primary platform:** Linux (other platforms are untested and unsupported for now)
> **Target audience:** Software engineers, platform engineers, SREs, and developers learning distributed observability

## 1. Executive Summary

Renzu Observability is a Rust-based distributed observability laboratory designed to help developers understand, reproduce, investigate, and diagnose failures in distributed applications.

The project combines an instrumented demonstration system, a reproducible fault-injection environment, a telemetry pipeline, and a developer-oriented command-line interface. It composes established tools (OpenTelemetry Collector, Jaeger, Prometheus, Grafana). The Rust code is the demo services, load generator, fault-injection controls, report generator, and CLI.

The project uses the publicly available code repository accompanying *Mastering Distributed Observability in Rust* as a learning reference. It independently establishes product requirements, architectural decisions, testing standards, and development workflows.

The project is intended to demonstrate practical engineering competency in:

- Rust application and systems programming.
- Distributed tracing, metrics, and structured logging.
- Asynchronous execution and service-to-service communication.
- Performance measurement and profiling.
- Infrastructure automation and developer tooling.
- Reliability engineering and reproducible incident investigation.

### Product vision

Make distributed systems failures reproducible, observable, and understandable through a single local development environment.

### Core value proposition

A developer should be able to start the environment, reproduce a known failure, inspect the resulting telemetry, and understand the diagnosis without manually assembling an observability stack.

---

## 2. Problem Statement

Distributed applications introduce failure modes that are difficult to investigate using application logs alone.

Examples include:

- Latency accumulating across multiple services.
- Database queries degrading overall request performance.
- Timeouts propagating through service dependencies.
- Errors occurring only under concurrent load.
- Missing or incorrectly propagated trace context.
- Telemetry pipelines dropping or delaying diagnostic data.
- Resource saturation affecting otherwise healthy services.

Existing observability platforms provide powerful components, but learning to operate them effectively requires understanding how instrumentation, telemetry collection, storage, visualization, and application behavior interact.

Renzu Observability addresses this learning and engineering problem through a reproducible, end-to-end environment.

### Current limitations to address

1. No unified workflow for launching a demonstration system and its observability infrastructure.
2. No standardized mechanism for reproducing common distributed-system failures.
3. No consistent correlation between service requests, logs, metrics, and traces.
4. No repeatable process for comparing system behavior before and after a change.
5. No single developer-facing interface for managing the laboratory and investigating scenarios.

These are initial product hypotheses to validate during implementation, not claims that every existing observability platform lacks these capabilities.

---

## 3. Goals and Non-Goals

### 3.1 Goals

- Provide a reproducible local observability environment.
- Demonstrate distributed tracing across multiple Rust services.
- Collect and correlate traces, metrics, and structured logs.
- Provide controlled fault-injection scenarios.
- Support repeatable load tests and performance comparisons.
- Provide a cohesive Rust CLI for common development workflows.
- Establish automated tests, CI, documentation, and reproducible setup.
- Serve as a practical learning platform for distributed observability and Rust infrastructure engineering.

### 3.2 Non-goals

The initial release will not:

- Replace Prometheus, Grafana, Jaeger, or the OpenTelemetry Collector.
- Implement a custom time-series database or tracing backend.
- Provide a production-ready hosted SaaS platform.
- Manage production infrastructure or credentials.
- Execute autonomous production remediation.
- Require an LLM or paid AI API.
- Support every possible telemetry backend.
- Implement a complete e-commerce business.
- Claim production readiness based solely on local demonstrations.
- Reproduce copyrighted book chapters or redistribute material without appropriate rights.

---

## 4. Target Users

### Primary: Software developer

Needs to understand how application behavior translates into traces, metrics, and logs.

Success means the developer can reproduce and diagnose a failure using the project documentation and tooling.

### Secondary: Platform engineer or SRE

Needs a repeatable environment for testing telemetry pipelines, service dependencies, and operational workflows.

Success means scenarios run consistently and produce measurable, inspectable outcomes.

### Secondary: Rust learner

Wants practical experience with asynchronous Rust, instrumentation, integration testing, and system design.

Success means the codebase demonstrates clear module boundaries, meaningful tests, and justified architectural decisions.

### Project maintainer

Needs to develop the project incrementally while retaining architectural control and verifying every change.

Success means changes remain reviewable, tests are trustworthy, and project documentation reflects the actual implementation.

---

## 5. Product Principles

### 5.1 Local-first

The entire MVP must run on a developer's machine without paid services, cloud accounts, or external AI APIs.

Internet access may be required to obtain source code, dependencies, and container images.

### 5.2 Observable by design

Every important operation should expose enough diagnostic information to understand its behavior.

Instrumentation must have explicit boundaries, naming conventions, and documented limitations.

### 5.3 Reproducible experiments

Every fault scenario and load-test run must have a defined configuration and a recorded result.

Comparisons must disclose changes in workload, environment, and measurement conditions.

### 5.4 Existing standards first

Prefer OpenTelemetry and established observability tools over proprietary instrumentation formats and custom infrastructure.

### 5.5 Incremental architecture

Build the smallest complete vertical slice before introducing additional services, abstractions, or crates.

### 5.6 Evidence over claims

A feature is complete only when its behavior can be demonstrated through tests, measurements, or a reproducible manual procedure.

### 5.7 Proportionate process

Documentation and verification effort should match the size of the change. The Definition of Done applies in full to features that change behavior; documentation-only or configuration-only changes apply only the items that are relevant. P1 and P2 items are a loose backlog until the MVP is accepted and should not generate planning overhead before then.

---

## 6. Functional Requirements

Requirements are assigned priorities:

- **P0:** Required for the MVP.
- **P1:** Required for the first meaningful extension.
- **P2:** Future enhancement.

### FR-01: Environment lifecycle — P0

The project must provide commands to start, inspect, and stop the local environment.

Requirements:

- Start required application and observability services.
- Report the state of managed components.
- Stop the environment without leaving project-owned processes running.
- Document prerequisites and resource requirements.
- Provide useful errors when dependencies or ports are unavailable.

Acceptance criteria:

- A clean checkout can be started by following the README.
- The CLI distinguishes successful startup from partially healthy startup.
- Shutdown does not silently claim success when required cleanup fails.

### FR-02: Instrumented Rust services — P0

The demonstration system must contain at least two independently running Rust services.

Requirements:

- Use an asynchronous HTTP framework.
- Instrument incoming requests and outgoing service calls.
- Emit structured logs.
- Record request duration, outcome, and relevant service attributes.
- Propagate distributed trace context between services.
- Include a PostgreSQL dependency in the first distributed-system milestone.

Acceptance criteria:

- A request traversing multiple services produces a connected distributed trace.
- Failed requests are distinguishable from successful requests.
- Database operations appear in the appropriate trace when instrumentation supports them.
- Instrumentation does not expose credentials or sensitive payloads.

### FR-03: Telemetry collection — P0

The environment must collect three primary telemetry signals:

1. Distributed traces.
2. Metrics.
3. Structured logs.

Requirements:

- Use OpenTelemetry-compatible instrumentation and export where supported.
- Use an OpenTelemetry Collector for telemetry pipeline management.
- Provide Prometheus for metrics and Grafana for visualization.
- Provide Tempo (via Grafana) for trace inspection.
- Select the log storage and query backend during M0 and record the decision in an ADR (`docs/decisions/`). Loki is the leading candidate because of its Grafana integration and support for linking log lines to traces, but it is not decided until the ADR exists. Evaluate candidates on local resource footprint, OTLP ingestion path, Grafana integration, and trace-ID linking.
- Include health checks and troubleshooting guidance.

Acceptance criteria:

- Each telemetry signal can be inspected through its intended interface.
- A developer can correlate an application request with its trace and relevant logs when the configured backend supports the required correlation.
- Telemetry export failures are observable.
- The system documents any gaps in cross-signal correlation.
- The log backend ADR exists before any log-pipeline or log-correlation work begins.

### FR-04: Developer CLI — P0

Provide a Rust command-line application named `renzu`.

Initial command interface:

```text
renzu up
renzu status
renzu down
renzu scenario list
renzu scenario run <name>
renzu load-test --scenario <name>
renzu report <run-id>
```

Requirements:

- Provide actionable error messages.
- Support command help and argument validation.
- Use stable exit codes for success and failure.
- Avoid shell-specific assumptions where practical.
- Keep command behavior documented and testable.

The exact syntax may evolve during implementation, provided equivalent functionality remains available.

Acceptance criteria:

- All implemented commands have automated tests for relevant success and failure paths.
- Commands do not report successful completion before the underlying operation has completed.
- Scenario execution and reporting use explicit run identifiers.

### FR-05: Fault injection — P0

Provide controlled scenarios that demonstrate common distributed-system failures.

Initial scenarios:

- Baseline operation.
- Artificial database latency.
- Elevated application error rate.
- Downstream service timeout.
- Cascading failure.

Requirements:

- Scenarios must be deterministic enough to reproduce under documented conditions.
- Faults must be explicitly enabled and disabled.
- Scenario parameters must be validated.
- Fault injection must be isolated from unrelated host services.
- Fault control interfaces must be disabled by default and bound to local interfaces only.
- Each scenario must include expected symptoms and investigation guidance.

Acceptance criteria:

- Each scenario can be run independently.
- The induced fault is distinguishable from an instrumentation or collection failure.
- Scenario cleanup restores the environment to its expected state.
- A report records the scenario configuration and observed results.

### FR-06: Load testing — P0

Provide a reproducible mechanism for generating application traffic.

Requirements:

- Configure request rate or concurrency.
- Configure test duration.
- Record successful and failed requests.
- Measure throughput and latency distributions.
- Record relevant workload parameters.
- Support a basic comparison between a baseline run and a faulted run (general run-to-run comparison is FR-08).

Acceptance criteria:

- Results include total requests, error count, throughput, and latency percentiles where the implementation supports reliable measurement.
- Measurement units and calculation methods are documented.
- Results do not present measurements from different workloads as directly comparable without qualification.

### FR-07: Diagnostic reports — P0

Produce a report for each completed experiment.

The report must include:

- Unique run identifier.
- Scenario name and configuration.
- Start time, duration, and completion status.
- Workload parameters.
- Request success and failure statistics.
- Latency statistics.
- Relevant telemetry references.
- Known limitations and measurement warnings.

Acceptance criteria:

- Reports can be saved locally and inspected without a running CLI process.
- Incomplete experiments are marked as incomplete.
- Missing telemetry is reported explicitly rather than represented as a healthy result.

### FR-08: Profiling and optimization — P1

Connect performance investigations to suitable profiling data.

Potential capabilities:

- CPU profiling.
- Memory analysis.
- Async-task performance investigation.
- Slow SQL investigation.
- Comparison of any two runs, with environment and workload differences flagged.

Acceptance criteria:

- Profiling instructions are reproducible.
- Benchmarks document their environment and workload.
- Claimed improvements are supported by measurements.
- Profiling overhead and measurement limitations are documented.

### FR-09: SLO workbench — P1

Support basic reliability measurements.

Potential capabilities:

- Service-level indicators.
- Configurable latency and availability objectives.
- Error-budget calculations.
- Reliability comparisons across experiments.

Acceptance criteria:

- Every SLI and SLO has an explicit definition.
- Calculations have automated tests.
- Reports distinguish observed measurements from configured objectives.

### FR-10: Trace-to-profile correlation — P2

Explore correlating slow distributed traces with profiling data.

The feature must not imply that trace spans can always be mapped precisely to individual CPU samples.

Acceptance criteria:

- Correlation methods and their limitations are documented.
- The feature works with a reproducible demonstration workload.
- Performance conclusions can be independently verified.

### FR-11: AI-assisted incident investigation — P2

Explore an optional local-model-based diagnostic assistant.

Requirements:

- No paid AI service is required.
- Telemetry is supplied as bounded, relevant evidence.
- Diagnostic hypotheses reference their supporting evidence.
- The system distinguishes observations from inferred explanations.
- No autonomous remediation is performed.

This feature must remain separate from the core telemetry pipeline.

---

## 7. Non-Functional Requirements

### NFR-01: Reproducibility

The project must pin or constrain relevant toolchain, dependency, and container image versions and document how to reproduce experiments.

### NFR-02: Reliability

- Services expose health information where appropriate.
- Startup and shutdown handle partial failures.
- Tests cover expected error conditions.
- Telemetry failures must not be silently misreported as application success.

### NFR-03: Performance

- Establish a baseline before claiming optimization.
- Measure the overhead of instrumentation when practical.
- Keep load generation configurable to avoid exhausting the developer's machine unintentionally.
- Avoid arbitrary performance targets until baseline measurements exist.

### NFR-04: Security

- Never commit secrets or credentials.
- Redact sensitive information from logs and reports.
- Bind services and published container ports to local interfaces by default unless external access is explicitly required.
- Document any default backend credentials as local-only.
- Use least-privilege container configurations where practical.
- Validate inputs to fault-injection and load-testing commands.
- Never execute untrusted scenario definitions as arbitrary shell commands.

### NFR-05: Maintainability

- Use Cargo workspaces where they provide meaningful boundaries.
- Keep abstractions proportional to current requirements.
- Enforce formatting, linting, and automated tests in CI.
- Document significant architectural decisions.
- Avoid duplicate implementations of established observability components.

### NFR-06: Usability

- A clean checkout must have a documented setup procedure.
- Common workflows must be accessible through the CLI.
- Failures must include enough context to identify the next diagnostic step.
- Documentation must distinguish implemented features from planned features.

### NFR-07: Resource awareness

The local environment must document approximate CPU, memory, disk, and container requirements, and the hardware they were measured on. Resource-intensive scenarios must have configurable limits.

---

## 8. Proposed Technical Stack

These are initial technology choices subject to verification during repository reconnaissance.

| Concern | Proposed technology |
|---|---|
| Language | Rust |
| Async runtime | Tokio |
| HTTP framework | Axum |
| Instrumentation | `tracing` |
| Telemetry standard | OpenTelemetry |
| Telemetry pipeline | OpenTelemetry Collector |
| Metrics | Prometheus |
| Dashboards | Grafana |
| Trace visualization | Tempo (via Grafana) |
| Log backend | **To be decided in M0 via ADR** (candidate: Loki) |
| Database | PostgreSQL |
| CLI | `clap` |
| Local orchestration | Docker Compose |
| CI | GitHub Actions |
| Integration testing | Rust tests and end-to-end scenarios |

The log backend choice should minimize operational complexity and remain compatible with the project's correlation and query requirements. It must be decided before any log-pipeline or log-correlation work begins.

Dependency versions must be checked for compatibility before implementation. Do not assume that every crate or backend API in older book examples remains current. The OpenTelemetry and `tracing-opentelemetry` crates in particular have changed APIs frequently; pin versions early and record them.

---

## 9. Proposed Repository Structure

```text
renzu-observability/
├── ARCHITECTURE.md
├── PRD.md
├── README.md
├── LICENSE
├── Cargo.toml
├── rust-toolchain.toml
├── compose.yaml
├── .github/
│   └── workflows/
├── apps/
│   ├── gateway/
│   ├── orders/
│   └── payments/
├── crates/
│   ├── telemetry/
│   ├── config/
│   ├── loadgen/
│   └── diagnostics/
├── deploy/
│   ├── otel-collector/
│   ├── prometheus/
│   └── grafana/
├── scenarios/
│   ├── baseline/
│   ├── slow-database/
│   └── cascading-failure/
├── tests/
│   └── integration/
└── docs/
    ├── architecture/
    ├── decisions/        # ADRs, e.g. 0001-reference-recon.md, 0002-log-backend.md
    ├── experiments/
    └── runbooks/
```

This structure is provisional. Directories and crates should be introduced only when a real implementation need exists.

`ARCHITECTURE.md` is a short top-level overview of system boundaries and data flow. Detailed diagrams and per-component notes live in `docs/architecture/`. Decisions live in `docs/decisions/`.

The initial milestone may consist of one service, one telemetry configuration, and a minimal CLI.

---

## 10. MVP User Journeys

### Journey A: Start the observability environment

1. Clone the repository.
2. Install documented prerequisites.
3. Start the environment using `renzu up`.
4. Inspect service health using `renzu status`.
5. Open the documented telemetry interfaces.
6. Generate a request and inspect its trace.

**Expected outcome:** The developer can verify that the application and telemetry pipeline work together.

### Journey B: Investigate slow requests

1. Start the baseline environment.
2. Run a baseline load test.
3. Execute the slow-database scenario.
4. Run the same workload under documented conditions.
5. Inspect the resulting metrics and traces.
6. Generate a diagnostic report.
7. View the baseline and faulted runs side by side.

**Expected outcome:** The developer can identify the injected latency and demonstrate its effect on request performance.

### Journey C: Reproduce a cascading failure

1. Start the environment.
2. Execute the downstream-timeout scenario.
3. Observe failures across service boundaries.
4. Inspect trace propagation and error rates.
5. Read the scenario runbook.
6. Restore the baseline state.

**Expected outcome:** The developer understands how a downstream failure affects dependent services and can verify recovery.

---

## 11. Milestones and Release Criteria

### M0: Reconnaissance

Deliverables:

- Review the Packt repository and its license.
- Identify runnable examples and their prerequisites.
- Record toolchain and dependency compatibility.
- Select the log backend and record the decision as an ADR.
- Document the proposed architecture and major decisions.
- Establish the local development and verification workflow.

Exit criteria:

- At least one reference example has been evaluated.
- The project scope is documented.
- The log backend decision is recorded.
- No unverified assumptions about the reference repository are treated as facts.

### M1: First observable service

Deliverables:

- One Rust HTTP service.
- Structured logs and request instrumentation.
- A working trace export path through the Collector to Tempo (via Grafana).
- A minimal local observability environment.
- The project license.

Exit criteria:

- A request produces an inspectable trace.
- Startup and shutdown are documented.
- Automated tests pass.

### M2: Distributed tracing

Deliverables:

- Two independently running services.
- PostgreSQL integration.
- Cross-service trace propagation.
- Initial metrics and dashboards.
- Log pipeline and log-trace correlation.

Exit criteria:

- A request crossing service boundaries produces a connected trace.
- Failure paths are observable.
- The complete workflow can be reproduced from the README.

### M3: Reproducible incidents

Deliverables:

- Baseline and slow-database scenarios.
- Configurable load generation.
- Experiment identifiers and diagnostic reports.

Exit criteria:

- Baseline and faulted runs can be compared under documented conditions.
- Reports capture configuration and measurements.
- Fault injection is cleaned up safely.

### M4: CLI and developer experience

Deliverables:

- Unified lifecycle and scenario commands.
- Actionable errors.
- Command documentation.
- End-to-end CI verification.

Exit criteria:

- The principal MVP journeys work without undocumented manual steps.
- CLI behavior is tested.
- A clean checkout can reproduce the demonstration.

### M5: Performance and reliability extensions

Deliverables:

- Profiling workflows.
- Performance comparison reports.
- Initial SLO calculations.
- Additional incident scenarios.

Exit criteria:

- Measurements are reproducible.
- Benchmark limitations are documented.
- New functionality does not compromise the established MVP workflows.

---

## 12. Testing Strategy

### Unit tests

Test isolated behavior, including:

- CLI argument validation.
- Scenario configuration.
- Report generation.
- Statistical calculations.
- Error handling and state transitions.

### Integration tests

Verify:

- Service startup and health.
- HTTP request behavior.
- Database interactions.
- Telemetry configuration.
- Trace propagation.
- Graceful handling of unavailable dependencies.

### End-to-end tests

Exercise the primary user journeys from environment startup through scenario execution and report generation.

### Failure testing

Deliberately test:

- Unavailable downstream services.
- Database connection failures.
- Invalid configuration.
- Telemetry Collector unavailability.
- Partial startup.
- Interrupted experiments.
- Fault-injection cleanup failures.

### Performance tests

Use repeatable workloads and record:

- Throughput.
- Error rate.
- Latency distributions.
- Relevant resource consumption.
- Test configuration and environmental limitations.

Performance results must not be treated as universal guarantees.

---

## 13. Success Metrics

The project will be evaluated on reproducibility, technical correctness, and practical usefulness rather than repository size or commit count.

| Metric | Initial success criterion |
|---|---|
| Clean setup | A new user can start the documented environment |
| Trace propagation | Connected traces are visible across two services |
| Telemetry coverage | Logs, metrics, and traces can all be inspected |
| Incident reproducibility | Baseline and injected-failure scenarios run repeatedly |
| Diagnostic output | Every completed experiment produces a report |
| Verification | Relevant automated checks pass in CI |
| Documentation | Primary workflows are documented and reproducible |
| Security | No committed secrets; unsafe scenario execution is prevented |
| Performance evidence | Comparisons record workload and measurement conditions |

The MVP is successful when a developer can independently reproduce and investigate at least one distributed latency incident from a clean checkout.

---

## 14. Risks and Mitigations

### Dependency incompatibility

**Risk:** Book examples may use outdated crate APIs or incompatible dependency versions.

**Mitigation:** Verify versions and consult official documentation before implementation. Pin versions early. Record migration decisions when necessary.

### Scope expansion

**Risk:** The project grows into a general-purpose monitoring platform before the core workflows work.

**Mitigation:** Enforce the MVP boundary and defer profiling correlation, SLO tooling, and AI assistance. Treat P1 and P2 as a loose backlog until MVP acceptance.

### Process overhead

**Risk:** Maintaining documentation and tracking takes more effort than building the project.

**Mitigation:** Apply the Definition of Done proportionately (Principle 5.7). Keep feature descriptions focused on outcomes. Do not pre-plan P1 and P2 work in detail.

### Undecided log backend

**Risk:** Logging, Collector, and correlation work proceeds on an assumed backend and must be redone.

**Mitigation:** Decide the log backend in M0 and record it as an ADR before any log-pipeline or log-correlation work begins.

### Telemetry overhead

**Risk:** Instrumentation or exporters distort benchmark results.

**Mitigation:** Document instrumentation settings and measure overhead where relevant.

### Misleading diagnostic reports

**Risk:** Missing telemetry is interpreted as healthy application behavior.

**Mitigation:** Track experiment completion and telemetry availability separately. Surface missing data explicitly.

### Excessive local resource usage

**Risk:** Multiple containers and load tests overwhelm a modest development machine.

**Mitigation:** Start with a minimal environment, document resource needs, and provide configurable load limits.

### Licensing uncertainty

**Risk:** Reference code or documentation is reused beyond its permitted terms, or the project itself ships without a clear license.

**Mitigation:** Inspect the reference repository license and applicable file-level notices. Keep independently written documentation and clearly attribute permitted reuse. Choose the project license early (by M1).

---

## 15. Open Questions

Resolve these during M0 rather than allowing them to silently shape the implementation.

1. Which reference examples provide the best foundation for the first observable service?
2. **Which log backend offers the simplest reliable local setup?** Must be answered with an ADR in M0; it blocks all log-pipeline and log-correlation work.
3. What is the minimum supported Rust toolchain?
4. Which profiling approach is practical on the initial Linux development environment?
5. Should the CLI manage Docker Compose directly or invoke a documented orchestration layer?
6. What resource limits should the default load-test profile enforce?
7. Which parts of the reference implementation can be reused under their existing licenses?
8. Which license should the project use?

These questions must be resolved through repository inspection, compatibility checks, and small technical experiments.

---

## 16. Definition of Done

A feature is complete only when all applicable conditions are satisfied. Items that clearly do not apply to a documentation-only or configuration-only change may be marked not applicable, with a one-line reason.

- Its requirements and behavior are documented.
- Its implementation matches the agreed scope.
- Relevant unit and integration tests exist.
- Required formatting, linting, and test commands have been run.
- Failure behavior has been considered.
- Security implications have been reviewed.
- Configuration and operational documentation are updated.
- The feature can be demonstrated or reproduced.
- Any known limitations are recorded.
- The maintainer has reviewed and accepted the change.

A successful build alone is not sufficient evidence of completion.

---

## 17. Reference Resources

- Packt Publishing, *Mastering Distributed Observability in Rust*: https://github.com/PacktPublishing/Mastering-Distributed-Observability-in-Rust
- The Rust Programming Language: https://doc.rust-lang.org/book/
- Tokio: https://tokio.rs/tokio/tutorial
- OpenTelemetry: https://opentelemetry.io/docs/
- Rust `tracing`: https://docs.rs/tracing
- Prometheus: https://prometheus.io/docs/
- Jaeger: https://www.jaegertracing.io/docs/
- Grafana: https://grafana.com/docs/

The reference repository is a learning resource, not a substitute for verifying current APIs, licensing, architecture, and implementation behavior.

---

## 18. Final Product Direction

Renzu Observability will begin as a local Rust observability laboratory with a small distributed application and a reproducible incident-investigation workflow.

The first release will prioritize correctness, operational clarity, and a complete end-to-end demonstration over feature breadth.

After the MVP is stable, the project can evolve toward performance profiling, reliability engineering, and trace-to-profile correlation.

The guiding principle is simple:

**Build a system that makes failures understandable before building a system that claims to explain them automatically.**
