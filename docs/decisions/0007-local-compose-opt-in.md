# ADR-0007: Secure-by-default local Compose configuration

**Status:** Accepted
**Deciders:** Renzu maintainer
**Date:** 2026-11-27

## Context

The initial local Compose file published PostgreSQL on `127.0.0.1:5432` and
supplied default Grafana credentials (`admin` / `admin`) through environment
fallbacks. These were documented as local-only and bound to loopback, which
satisfied the MVP security boundary, but they made insecure convenience the
default whenever `docker compose up -d` was run.

COMPOSE-03 requires both PostgreSQL host publication and Grafana credentials to
be opt-in. The CLI, verification scripts, and CI E2E job also drive Compose, so
the chosen mechanism must keep those workflows reproducible.

## Decision

- Keep `compose.yaml` secure by default:
  - do not publish a PostgreSQL host port;
  - require `GF_SECURITY_ADMIN_USER` and `GF_SECURITY_ADMIN_PASSWORD` from the
    environment, with no fallback values.
- Add `.env.example` with local-only Grafana example credentials. Copying it to
  `.env` is the explicit credential opt-in.
- Add `compose.local.yaml` as a small overlay that publishes PostgreSQL on
  `127.0.0.1:5432`. Using the overlay is the explicit host-port opt-in.
- Use both files in `scripts/ci-local.sh`, `scripts/verify-infrastructure.sh`,
  the CI E2E job, and the `renzu up/status/down` CLI default path. An explicit
  `--compose-file` or `RENZU_COMPOSE_FILE` remains a single-file override.
- Ignore `.env` and `.env.*` except `.env.example` in Git.

## Reasoning

- A secure default prevents accidental reuse of lab credentials or host-port
  publication outside the local loopback lab.
- Keeping the opt-in files in the repository preserves the documented
  quickstart; users take two explicit actions instead of editing Compose.
- Scripts and CLI need the same file set as the docs, otherwise the proof
  scripts would either fail or silently test a different topology.

## Rejected alternatives

- **Keep current defaults and rely on documentation:** rejected because it does
  not make the insecure settings opt-in.
- **Require environment variables in every command:** rejected because it is
  verbose and error-prone for the documented local workflow.
- **Publish PostgreSQL via a separate proxy container:** rejected because it
  adds a service and failure mode without solving the credential default.

## Consequences

### Positive

- No default Grafana password in the base Compose file.
- No PostgreSQL host port unless the local overlay is selected.
- Local and CI workflows use one documented Compose file set.

### Negative / trade-offs

- Quickstart requires `cp .env.example .env` and two `-f` flags.
- Existing host-port conflict overrides must be appended after
  `compose.local.yaml`.
- `renzu up` now adds the overlay only when no explicit compose file is given.

## Related work

- COMPOSE-03: Make Postgres publication and Grafana credentials opt-in.
- PRD NFR-04: bind published ports to local interfaces and document any default
  backend credentials as local-only.
