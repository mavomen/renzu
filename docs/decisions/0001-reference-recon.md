# ADR-0001: Reference Repository Reconnaissance

**Status:** Accepted
**Deciders:** Renzu maintainer
**Date:** 2026-10-28

## Context

PRD §17 and FEATURES.md §2 / §9 designate the Packt repository
`https://github.com/PacktPublishing/Mastering-Distributed-Observability-in-Rust`
as the learning reference for *Mastering Distributed Observability in Rust*.
M0 requires inspecting it before adopting code, architecture, or dependency
versions, and recording licensing/reuse constraints.

## Repository inspected

| Field | Value |
|---|---|
| URL | https://github.com/PacktPublishing/Mastering-Distributed-Observability-in-Rust |
| Book | Mastering Distributed Observability in Rust, First Edition |
| Authors | Manjunath Gangappa, Rajkumar Rangaraj |
| ISBN | 9781806671793 |
| Publisher | Packt Publishing |
| Default branch | `main` |
| Commit count | 4 commits |
| Top-level folders | `code/`, `scripts/` |
| Root files | `LICENSE`, `README.md`, `.gitignore`, `.gitattributes` |

## License and reuse constraints

- Repository LICENSE: **MIT License**, Copyright (c) 2025 Packt.
- MIT permits use, copy, modification, merge, publish, distribute, sublicense,
  and sale, provided the copyright notice and permission notice are included in
  all copies or substantial portions of the software.
- Workspace `Cargo.toml` declares `license = "MIT OR Apache-2.0"` — a minor
  inconsistency with the root MIT LICENSE; treat the root LICENSE as
  authoritative.

**Reuse position:** Renzu will **not copy** reference code into its tree. The
reference is used only as a learning reference for patterns (service layout,
OTel instrumentation stack, Compose orchestration). Any permitted reuse will be
explicitly attributed. This aligns with AGENTS.md §6 (reference repository) and
PRD §14 (licensing uncertainty mitigation) and §17 (reference resources).

## Runnable examples evaluated

The `code/` folder contains one Cargo workspace per book chapter:

- `code/base` — shared workspace: `otelmart`, `products`, `inventory`, `orders`,
  `schemas`
- `code/chapter_4`, `chapter_5`, `chapter_6`, `chapter_7`, `chapter_8`,
  `chapter_9`, `chapter_10`, `chapter_13`, `chapter_14` — per-chapter copies of
  the same service layout.

Each chapter workspace is a standalone Cargo project with its own
`Cargo.toml`, `Cargo.lock`, `docker-compose.yml`, and `Dockerfile` per service.
`scripts/` contains only `drive_checkout.ps1` (PowerShell, Windows checkout
helper; not part of the Rust build).

**Evaluated:** `code/base` and `code/chapter_4` (structure, `Cargo.toml`,
`docker-compose.yml`, `Architecture.md`). Not built or run (no network/build
verification in this task; recorded as unverified).

## Dependency versions recorded

From `code/chapter_4/Cargo.toml` `[workspace.dependencies]`:

| Crate | Version | Notes |
|---|---|---|
| axum | 0.8.7 | HTTP framework |
| tokio | 1.48.0 (features full) | Async runtime |
| tower | 0.5.2 | Service abstractions |
| tower-http | 0.6.6 (cors) | HTTP middleware |
| sqlx | 0.8.6 (postgres, …) | Database driver + migrations |
| redis | 0.32.7 | Cache client |
| serde | 1.0.228 | Serialization |
| anyhow | 1.0.100 | Error handling |
| tracing | 0.1 | Structured logging instrumentation |
| tracing-subscriber | 0.3 (env-filter, json) | JSON structured logs to stdout |
| opentelemetry | 0.31 | OTel API |
| opentelemetry_sdk | 0.31 | OTel SDK |
| opentelemetry-otlp | 0.31 (grpc-tonic) | OTLP exporter |
| tracing-opentelemetry | 0.32 | tracing → OTel bridge |
| axum-tracing-opentelemetry | 0.33.0 | Axum HTTP request instrumentation |
| reqwest-middleware | 0.5 (json) | Outbound HTTP client middleware |
| reqwest-tracing | 0.7.0 (opentelemetry_0_31) | Outbound HTTP trace propagation |
| reqwest | 0.13.1 (json) | HTTP client |

Edition 2021, resolver "2", workspace package version 0.1.0.

## Compatibility issues and observations (unverified unless noted)

1. **README states "Rust 1.8 or later"** — factually implausible for these
   crate versions (Rust 1.8 predates them by years). Treat as an unverified /
   incorrect placeholder. Recorded here; do not rely on it for MSRV.
   Renzu will pin its own MSRV in F-002.
2. **No telemetry pipeline in `docker-compose.yml`.** The reference compose
   runs only PostgreSQL plus the four Rust services. There is no OpenTelemetry
   Collector, Jaeger, Prometheus, Grafana, or Loki in the reference stack.
   Renzu must supply its own pipeline (F-003/F-007).
3. **Ports published on 0.0.0.0** (e.g. `5433:5432`, `3001:3001`, `4200:4200`),
   not `127.0.0.1`. Conflicts with Renzu security principle
   (ARCHITECTURE.md §7: publish ports on 127.0.0.1 only). Renzu will bind to
   loopback.
4. **Hardcoded database credentials** in compose
   (`opentel_user`/`opentel_pass`/`opentel_db`). Renzu must not commit
   credentials (AGENTS.md §8).
5. **Each service runs SQLx migrations on startup.** Useful pattern to adopt
   for F-011 (PostgreSQL integration).
6. **Service-per-schema in a shared PostgreSQL** (`users`, `products`,
   `inventory`, `orders` schemas). Pattern noted for F-011.
7. **Low commit activity (4 commits).** Do not treat the reference as actively
   maintained; treat all APIs as candidates for re-verification against
   current crate documentation (PRD §14 dependency-incompatibility risk).

## Decision

Adopt the reference **only as a pattern source** (service layout, OTel
stack versions as a starting point, Compose service ordering with health
checks). Do not copy code, do not adopt its MSRV claim, do not adopt its
publish-all-ports or hardcoded-credential practices.

Record the log-backend decision separately in ADR-0002.
