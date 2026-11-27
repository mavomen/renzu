#!/usr/bin/env bash
set -euo pipefail

# Local mirror of .github/workflows/ci.yml.
# Runs the same three checks so CI is reproducible without GitHub.

# Local-only opt-in credentials for the Compose lab. CI sets the same values;
# these defaults keep the script standalone-friendly on loopback.
export GF_SECURITY_ADMIN_USER="${GF_SECURITY_ADMIN_USER:-admin}"
export GF_SECURITY_ADMIN_PASSWORD="${GF_SECURITY_ADMIN_PASSWORD:-admin}"
COMPOSE_FILES=(-f compose.yaml -f compose.local.yaml)
if [[ -n "${RENZU_COMPOSE_OVERRIDE:-}" ]]; then
    COMPOSE_FILES+=(-f "$RENZU_COMPOSE_OVERRIDE")
fi

echo "==> cargo fmt --all -- --check"
cargo fmt --all -- --check

echo "==> cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> cargo test --workspace"
cargo test --workspace

echo "==> starting infrastructure for e2e tests"
docker compose "${COMPOSE_FILES[@]}" up -d

echo "==> waiting for services to be healthy"
for i in $(seq 1 60); do
    if docker compose "${COMPOSE_FILES[@]}" ps --format json | jq -s -e 'length > 0 and all(.[]; .Health == "healthy")' >/dev/null; then
        echo "services healthy"
        break
    fi
    sleep 2
done

echo "==> running infrastructure verification"
./scripts/verify-infrastructure.sh

echo "==> running trace visualization E2E"
./scripts/e2e-trace-visualization.sh

echo "==> running log/trace correlation E2E"
./scripts/e2e-log-trace-correlation.sh

echo "==> running metrics and report E2E"
./scripts/e2e-metrics-report.sh

echo "==> running incident investigation journey E2E"
./scripts/e2e-incident-journey.sh

echo "==> stopping infrastructure"
docker compose "${COMPOSE_FILES[@]}" down -v

echo "==> all checks passed"