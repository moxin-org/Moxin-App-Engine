#!/usr/bin/env bash
set -euo pipefail

# Local/CI observability stack smoke checks.
# Usage:
#   bash ops/observability/smoke_check.sh

PROM_URL="${PROM_URL:-http://127.0.0.1:9090}"
GRAFANA_URL="${GRAFANA_URL:-http://127.0.0.1:3000}"
LOKI_URL="${LOKI_URL:-http://127.0.0.1:3100}"
JAEGER_URL="${JAEGER_URL:-http://127.0.0.1:16686}"
ZIPKIN_URL="${ZIPKIN_URL:-http://127.0.0.1:9411}"
APP_METRICS_URL="${APP_METRICS_URL:-http://127.0.0.1:8080/metrics}"
SMOKE_SERVICE_NAME="${SMOKE_SERVICE_NAME:-mofa-smoke}"

wait_http_ok() {
  local label="$1"
  local url="$2"
  local timeout_s="${3:-90}"
  local start_ts now elapsed
  start_ts="$(date +%s)"
  echo "  - ${label}: waiting (timeout=${timeout_s}s)"
  while true; do
    if curl -fsS "$url" >/dev/null 2>&1; then
      now="$(date +%s)"
      elapsed="$(( now - start_ts ))"
      echo "    ${label}: ready in ${elapsed}s"
      return 0
    fi
    now="$(date +%s)"
    elapsed="$(( now - start_ts ))"
    if (( elapsed >= timeout_s )); then
      echo "    ${label}: timed out after ${elapsed}s (${url})" >&2
      return 1
    fi
    echo "    ${label}: still waiting (${elapsed}s elapsed)"
    sleep 2
  done
}

run_check() {
  local label="$1"
  shift
  echo "  - ${label}"
  "$@"
  echo "    ${label}: ok"
}

inject_zipkin_trace() {
  local now_us trace_id span_id
  now_us="$(( $(date +%s%N) / 1000 ))"
  trace_id="$(hexdump -n 16 -e '16/1 "%02x"' /dev/urandom)"
  span_id="$(hexdump -n 8 -e '8/1 "%02x"' /dev/urandom)"

  if ! curl -fsS -X POST "${ZIPKIN_URL}/api/v2/spans" \
    -H 'Content-Type: application/json' \
    -d "[{\"traceId\":\"${trace_id}\",\"id\":\"${span_id}\",\"name\":\"observability-smoke\",\"timestamp\":${now_us},\"duration\":1000,\"localEndpoint\":{\"serviceName\":\"${SMOKE_SERVICE_NAME}\"},\"tags\":{\"smoke\":\"true\"}}]" >/dev/null; then
    echo "Zipkin ingest failed at ${ZIPKIN_URL}/api/v2/spans." >&2
    echo "Ensure Jaeger enables Zipkin collector (COLLECTOR_ZIPKIN_HOST_PORT=:9411) and restart the stack." >&2
    return 1
  fi
}

echo "[1/8] Waiting for service health"
wait_http_ok "Prometheus health" "${PROM_URL}/-/healthy" 90
wait_http_ok "Grafana health" "${GRAFANA_URL}/api/health" 90
wait_http_ok "Loki readiness" "${LOKI_URL}/ready" 90
wait_http_ok "Jaeger UI" "${JAEGER_URL}/" 90

echo "[2/8] Metrics endpoint responds"
wait_http_ok "App metrics endpoint" "${APP_METRICS_URL}" 120


echo "[3/8] /metrics contains expected MoFA metric families"
run_check "mofa_llm_requests_total HELP line exists" bash -c "curl -fsS '${APP_METRICS_URL}' | grep -q '^# HELP mofa_llm_requests_total'"
run_check "mofa_llm_requests_total TYPE line exists" bash -c "curl -fsS '${APP_METRICS_URL}' | grep -q '^# TYPE mofa_llm_requests_total'"

echo "[4/8] Prometheus can query scrape health"
run_check "Prometheus query: up{job=\"mofa-app\"}" bash -c "curl -fsSG '${PROM_URL}/api/v1/query' --data-urlencode 'query=up{job=\"mofa-app\"}' | jq -e '.status == \"success\" and (.data.result | length >= 1)' >/dev/null"

echo "[5/8] Prometheus can query mofa_llm_requests_total"
run_check "Prometheus query: mofa_llm_requests_total" bash -c "curl -fsSG '${PROM_URL}/api/v1/query' --data-urlencode 'query=mofa_llm_requests_total' | jq -e '.status == \"success\"' >/dev/null"

echo "[6/8] Loki has ingested logs"
run_check "Loki query returns at least one stream" bash -c "curl -fsSG '${LOKI_URL}/loki/api/v1/query' --data-urlencode 'query={compose_service=~\".+\"}' | jq -e '.status == \"success\" and (.data.result | length > 0)' >/dev/null"

echo "[7/8] Injecting one trace via Zipkin and validating Jaeger visibility"
inject_zipkin_trace
sleep 3
run_check "Jaeger service list includes ${SMOKE_SERVICE_NAME}" bash -c "curl -fsS '${JAEGER_URL}/api/services' | jq -e --arg svc '${SMOKE_SERVICE_NAME}' '.data | index(\$svc) != null' >/dev/null"
run_check "Jaeger trace query returns data" bash -c "curl -fsSG '${JAEGER_URL}/api/traces' --data-urlencode 'service=${SMOKE_SERVICE_NAME}' --data-urlencode 'limit=1' | jq -e '.data | length > 0' >/dev/null"

echo "[8/8] Smoke check passed"
