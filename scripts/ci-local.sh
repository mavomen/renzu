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

echo "==> all checks passed"
