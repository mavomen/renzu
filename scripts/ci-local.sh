#!/usr/bin/env bash
set -euo pipefail

# Local mirror of .github/workflows/ci.yml.
# Runs the same three checks so CI is reproducible without GitHub.

echo "==> cargo fmt --all -- --check"
cargo fmt --all -- --check

echo "==> cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> cargo test --workspace"
cargo test --workspace

echo "==> starting infrastructure for e2e tests"
docker compose up -d

echo "==> waiting for services to be healthy"
for i in $(seq 1 60); do
    if docker compose ps --format json | jq -s -e 'length > 0 and all(.[]; .Health == "healthy")' >/dev/null; then
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

echo "==> stopping infrastructure"
docker compose down -v

echo "==> all checks passed"