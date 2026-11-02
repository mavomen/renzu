#!/usr/bin/env bash
set -euo pipefail

# E2E verification that traces produced by the gateway flow through the
# OpenTelemetry Collector into Jaeger and are inspectable via the Jaeger API.
#
# Prerequisites: docker compose is up and healthy (jaeger + otel-collector).
#   docker compose up -d
#
# Usage:
#   ./scripts/e2e-trace-visualization.sh
#
# The script builds ./target/debug/gateway, runs it against the collector's
# OTLP HTTP receiver (127.0.0.1:4318), hits GET /health, /ready and /, then
# polls the Jaeger API until traces for service `gateway` appear. It asserts
# the traces carry service.name=gateway as a resource attribute and span
# durations greater than zero. The gateway process is always killed on exit.
# Compose is left running.

GATEWAY_BIN="./target/debug/gateway"
GATEWAY_HOST="127.0.0.1"
GATEWAY_PORT=8080
OTLP_ENDPOINT="http://127.0.0.1:4318/v1/traces"
JAEGER_HOST="127.0.0.1"
JAEGER_PORT=16686
JAEGER_API="http://${JAEGER_HOST}:${JAEGER_PORT}"
SERVICE_NAME="gateway"
JAEGER_TIMEOUT=30   # seconds to poll Jaeger for traces
JAEGER_INTERVAL=3   # seconds between Jaeger polls
COMPOSE_WAIT=5      # seconds to wait for compose ports before failing fast

GATEWAY_PID=""
GATEWAY_LOG="/tmp/renzu-e2e-gateway.log"

fail() {
    echo "ERROR: $*" >&2
    exit 1
}

# Kill the gateway process if it is still running. Safe to call multiple times.
cleanup() {
    if [[ -n "${GATEWAY_PID:-}" ]] && kill -0 "${GATEWAY_PID}" 2>/dev/null; then
        echo "==> stopping gateway (pid ${GATEWAY_PID})"
        kill "${GATEWAY_PID}" 2>/dev/null || true
        wait "${GATEWAY_PID}" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Wait until a TCP port accepts connections. Used for compose ports (short
# timeout: compose is assumed up) and the gateway port (longer timeout).
wait_for_port() {
    local host="$1" port="$2" label="$3" timeout="$4"
    local deadline=$(( $(date +%s) + timeout ))
    while [[ $(date +%s) -lt $deadline ]]; do
        if (echo > "/dev/tcp/${host}/${port}") >/dev/null 2>&1; then
            echo "==> ${label} accepting connections on ${host}:${port}"
            return 0
        fi
        sleep 1
    done
    fail "${label} did not accept connections on ${host}:${port} within ${timeout}s"
}

echo "==> checking compose dependencies are reachable"
wait_for_port "${JAEGER_HOST}" "${JAEGER_PORT}" "Jaeger API" "${COMPOSE_WAIT}"
wait_for_port "127.0.0.1" 4318 "OTel Collector OTLP HTTP receiver" "${COMPOSE_WAIT}"

echo "==> building gateway binary"
cargo build --bin gateway

echo "==> starting gateway on ${GATEWAY_HOST}:${GATEWAY_PORT} (OTLP -> ${OTLP_ENDPOINT})"
RENZU_OTLP_ENDPOINT="${OTLP_ENDPOINT}" "${GATEWAY_BIN}" > "${GATEWAY_LOG}" 2>&1 &
GATEWAY_PID=$!
echo "==> gateway pid ${GATEWAY_PID} (logs: ${GATEWAY_LOG})"

wait_for_port "${GATEWAY_HOST}" "${GATEWAY_PORT}" "gateway" 30

echo "==> hitting gateway endpoints"
for path in /health /ready /; do
    code=$(curl -s -o /dev/null -w "%{http_code}" --max-time 10 \
        "http://${GATEWAY_HOST}:${GATEWAY_PORT}${path}") || fail "curl ${path} failed"
    echo "   GET ${path} -> HTTP ${code}"
    if [[ "${code}" != "200" ]]; then
        fail "GET ${path} returned HTTP ${code} (expected 200)"
    fi
done

echo "==> querying Jaeger for service '${SERVICE_NAME}' traces (timeout ${JAEGER_TIMEOUT}s)"
deadline=$(( $(date +%s) + JAEGER_TIMEOUT ))
traces_json=""
while [[ $(date +%s) -lt $deadline ]]; do
    traces_json=$(curl -s --max-time 10 "${JAEGER_API}/api/traces?service=${SERVICE_NAME}" 2>/dev/null) || traces_json=""
    if echo "${traces_json}" | python3 -c "import sys,json; sys.exit(0 if json.load(sys.stdin).get('data') else 1)" 2>/dev/null; then
        break
    fi
    echo "   no traces yet, retrying in ${JAEGER_INTERVAL}s..."
    sleep "${JAEGER_INTERVAL}"
done

if [[ -z "${traces_json}" ]]; then
    fail "no traces found for service '${SERVICE_NAME}' in Jaeger within ${JAEGER_TIMEOUT}s"
fi

echo "==> asserting trace data: service.name=${SERVICE_NAME} resource attribute and span duration > 0"
python3 - "$traces_json" <<'PY'
import json
import sys

raw = sys.argv[1]
try:
    data = json.loads(raw)
except json.JSONDecodeError as exc:
    print(f"FAIL: Jaeger response is not valid JSON: {exc}", file=sys.stderr)
    print(raw[:2000], file=sys.stderr)
    sys.exit(1)

traces = data.get("data") or []
if not traces:
    print("FAIL: no traces found for service 'gateway' in Jaeger", file=sys.stderr)
    sys.exit(1)

ok = False
for trace in traces:
    for span in trace.get("spans") or []:
        process = span.get("process") or {}
        service_name = process.get("serviceName")
        tags = {t.get("key"): t.get("value") for t in (process.get("tags") or [])}
        has_service_attr = service_name == "gateway" or tags.get("service.name") == "gateway"
        duration = span.get("duration") or 0
        if has_service_attr and duration > 0:
            ok = True
            break
    if ok:
        break

if not ok:
    print(f"FAIL: no span with service.name=gateway resource attribute and duration > 0 "
          f"across {len(traces)} trace(s)", file=sys.stderr)
    sys.exit(1)

print(f"PASS: {len(traces)} trace(s) found with service.name=gateway resource "
      f"attribute and span duration > 0")
PY

echo "==> e2e trace visualization verification complete"
