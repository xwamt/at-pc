#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# at-pc-server Linux Health & Functionality Verification Script
# ==============================================================================

SERVER_HOST="${1:-127.0.0.1}"
MCP_PORT="${2:-9800}"
WS_PORT="${3:-9801}"

echo "=== Verifying AT-PC Server on ${SERVER_HOST} ==="

PASS_COUNT=0
FAIL_COUNT=0

check_step() {
    local desc="$1"
    shift
    echo -n "  [..] ${desc} ... "
    if "$@"; then
        echo -e "\r  [\033[32mPASS\033[0m] ${desc}"
        PASS_COUNT=$((PASS_COUNT + 1))
    else
        echo -e "\r  [\033[31mFAIL\033[0m] ${desc}"
        FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
}

# 1. Verify HTTP Health Endpoint
check_step "HTTP Health Check (/health on port ${MCP_PORT})" \
    curl -sf "http://${SERVER_HOST}:${MCP_PORT}/health" >/dev/null

# 2. Verify Health Response JSON Payload
check_health_json() {
    local res
    res=$(curl -sf "http://${SERVER_HOST}:${MCP_PORT}/health")
    echo "${res}" | grep -q '"status"[[:space:]]*:[[:space:]]*"ok"'
}
check_step "Verify Health JSON status == 'ok'" check_health_json

# 3. Verify MCP SSE Endpoint
# 若定义了 AT_PC_TOKEN，则对 /sse 检查携带 Bearer 凭据（门禁开启时必需）。
# 未定义 AT_PC_TOKEN 时行为保持现状（本地开发态仍全过）。
check_sse() {
    local auth_args=()
    if [ -n "${AT_PC_TOKEN:-}" ]; then
        auth_args=(-H "Authorization: Bearer ${AT_PC_TOKEN}")
    fi
    curl -sf -I "${auth_args[@]}" "http://${SERVER_HOST}:${MCP_PORT}/sse" | grep -qi "text/event-stream"
}
check_step "MCP SSE Stream Endpoint (/sse on port ${MCP_PORT})" check_sse

# 4. Verify Web Dashboard / Management UI
check_step "Web Dashboard UI (/dashboard on port ${MCP_PORT})" \
    curl -sf "http://${SERVER_HOST}:${MCP_PORT}/dashboard" >/dev/null

# 5. Verify WebSocket Port Connectivity
check_ws_port() {
    if command -v nc >/dev/null 2>&1; then
        nc -z -w 3 "${SERVER_HOST}" "${WS_PORT}"
    else
        timeout 3 bash -c "</dev/tcp/${SERVER_HOST}/${WS_PORT}" 2>/dev/null
    fi
}
check_step "WebSocket Agent Gateway connectivity (port ${WS_PORT})" check_ws_port

echo "=============================================="
echo "Results: ${PASS_COUNT} passed, ${FAIL_COUNT} failed."
echo "=============================================="

if [ "${FAIL_COUNT}" -eq 0 ]; then
    echo -e "\033[32m✔ Server is healthy and operating normally.\033[0m"
    echo "  - Web Dashboard: http://${SERVER_HOST}:${MCP_PORT}/dashboard"
    echo "  - MCP Endpoint:  http://${SERVER_HOST}:${MCP_PORT}/sse"
    echo "  - Agent Gateway: ws://${SERVER_HOST}:${WS_PORT}/ws"
    exit 0
else
    echo -e "\033[31m✘ Verification encountered failures. Please check server logs.\033[0m"
    exit 1
fi
