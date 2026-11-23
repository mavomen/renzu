#!/usr/bin/env bash
set -euo pipefail

# E2E verification that traces produced by the gateway flow through the
# OpenTelemetry Collector into Tempo and are inspectable via the Tempo API.
#
# Prerequisites: docker compose is up and healthy (tempo + otel-collector).
#   docker compose up -d
#
# Usage:
#   ./scripts/e2e-trace-visualization.sh
#
# The script builds ./target/debug/gateway, runs it against the collector's
# OTLP HTTP receiver (127.0.0.1:4318), hits GET /health, /ready and /, then
# polls the Tempo API until traces for service `gateway` appear. It asserts
# the traces carry service.name=gateway as a resource attribute and span
# durations greater than zero. The gateway process is always killed on exit.
# Compose is left running.

GATEWAY_BIN="./target/debug/gateway"
GATEWAY_HOST="127.0.0.1"
GATEWAY_PORT=8082
OTLP_ENDPOINT="http://127.0.0.1:4318/v1/traces"
TEMPO_HOST="127.0.0.1"
TEMPO_PORT=3200
TEMPO_API="http://${TEMPO_HOST}:${TEMPO_PORT}"
SERVICE_NAME="gateway"
TEMPO_TIMEOUT=60   # seconds to poll Tempo for traces
TEMPO_INTERVAL=3   # seconds between Tempo polls
COMPOSE_WAIT=5     # seconds to wait for compose ports before failing fast

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
wait_for_port "${TEMPO_HOST}" "${TEMPO_PORT}" "Tempo HTTP API" "${COMPOSE_WAIT}"
wait_for_port "127.0.0.1" 4318 "OTel Collector OTLP HTTP receiver" "${COMPOSE_WAIT}"

echo "==> building gateway binary"
cargo build --bin gateway

echo "==> starting gateway on ${GATEWAY_HOST}:${GATEWAY_PORT} (OTLP -> ${OTLP_ENDPOINT})"
RENZU_OTLP_ENDPOINT="${OTLP_ENDPOINT}" \
RENZU_PORT="${GATEWAY_PORT}" \
RENZU_DATABASE_URL="postgres://renzu:renzu@127.0.0.1:5432/renzu" \
RENZU_ORDERS_URL="http://127.0.0.1:8081" \
"${GATEWAY_BIN}" > "${GATEWAY_LOG}" 2>&1 &
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

echo "==> querying Tempo for service '${SERVICE_NAME}' traces (timeout ${TEMPO_TIMEOUT}s)"
deadline=$(( $(date +%s) + TEMPO_TIMEOUT ))
trace_ids=()

while [[ $(date +%s) -lt $deadline ]]; do
    # Use Tempo's native search API with TraceQL to find traces for the service
    # TraceQL query: {service.name = "gateway"}
    search_json=$(curl -s --max-time 10 -G "${TEMPO_API}/api/search" --data-urlencode 'q={service.name = "'"${SERVICE_NAME}"'"}' --data-urlencode 'limit=20' 2>/dev/null) || search_json=""
    
    if [[ -n "${search_json}" ]]; then
        # Extract trace IDs from search results
        trace_ids=($(echo "${search_json}" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    traces = data.get('traces', [])
    for t in traces:
        trace_id = t.get('traceID') or t.get('traceId')
        if trace_id:
            print(trace_id)
except:
    pass
" 2>/dev/null))
        
        if [[ ${#trace_ids[@]} -gt 0 ]]; then
            break
        fi
    fi
    echo "   no traces yet, retrying in ${TEMPO_INTERVAL}s..."
    sleep "${TEMPO_INTERVAL}"
done

if [[ ${#trace_ids[@]} -eq 0 ]]; then
    fail "no traces found for service '${SERVICE_NAME}' in Tempo within ${TEMPO_TIMEOUT}s"
fi

echo "==> found ${#trace_ids[@]} trace(s), fetching details for first trace: ${trace_ids[0]}"

# Fetch the first trace by ID using Tempo's native trace-by-id API
trace_json=$(curl -s --max-time 10 "${TEMPO_API}/api/traces/${trace_ids[0]}" 2>/dev/null) || trace_json=""

if [[ -z "${trace_json}" ]]; then
    fail "failed to fetch trace ${trace_ids[0]} from Tempo"
fi

echo "==> asserting trace data: service.name=${SERVICE_NAME} resource attribute and span duration > 0"
python3 - "$trace_json" <<'PY'
import json
import sys

raw = sys.argv[1]
try:
    data = json.loads(raw)
except json.JSONDecodeError as exc:
    print(f"FAIL: Tempo response is not valid JSON: {exc}", file=sys.stderr)
    print(raw[:2000], file=sys.stderr)
    sys.exit(1)

# Tempo native trace-by-id API returns a single trace object with spans
traces = data.get("traces") or [data] if isinstance(data, dict) else []
if not traces:
    # Try alternate format: data might be a list of traces
    traces = data if isinstance(data, list) else []

if not traces:
    print("FAIL: no traces found in Tempo response", file=sys.stderr)
    sys.exit(1)

ok = False
for trace in traces:
    # Trace format: {"spans": [...], "traceID": "..."}
    spans = trace.get("spans") or trace.get("data", {}).get("spans") or []
    for span in spans:
        # Span attributes are in span.attributes or span.tags
        attrs = span.get("attributes") or {}
        tags = span.get("tags") or {}
        service_name = attrs.get("service.name") or tags.get("service.name")
        duration = span.get("duration") or 0
        # Duration in Tempo is in nanoseconds as string
        if isinstance(duration, str):
            try:
                duration = int(duration)
            except:
                duration = 0
        if service_name == "gateway" and duration > 0:
            ok = True
            break
    if ok:
        break

if not ok:
    print(f"FAIL: no span with service.name=gateway attribute and duration > 0", file=sys.stderr)
    sys.exit(1)

print(f"PASS: trace found with service.name=gateway attribute and span duration > 0")
PY

echo "==> e2e trace visualization verification complete"