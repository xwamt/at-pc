#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# at-pc API Key Gate M0 Verification Suite
#
# Automates the G0 verification checks specified in §3.2 of:
# docs/plans/2026-09-20-api-key-gate-implementation-plan.md
#
# Checks covered:
#   G0-1 : cargo test regression, P0 boot/health tests, and F1/F2/F3 no-diff
#   G0-2 : Non-loopback (--host 0.0.0.0) without credentials refuses to start
#   G0-3 : Loopback (--host 127.0.0.1) without credentials starts normally
#   G0-4 : First boot generates bootstrap_key.txt (0600), format match, idempotency
#   G0-5 : /health loopback exemption vs non-loopback credential requirement
#   G0-7 : Static dashboard shell is reachable; sensitive data endpoints require auth
#   G0-8 : /sse endpoint event data is /messages without ?token= leak
#   G0-10: Gate authenticity: AUTH_FAILED audit record has redacted token_prefix
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# --- Colors ---
if [ -t 1 ]; then
    GREEN='\033[0;32m'
    RED='\033[0;31m'
    YELLOW='\033[1;33m'
    BLUE='\033[0;34m'
    CYAN='\033[0;36m'
    BOLD='\033[1m'
    NC='\033[0m'
else
    GREEN=''
    RED=''
    YELLOW=''
    BLUE=''
    CYAN=''
    BOLD=''
    NC=''
fi

# --- CLI Options & Defaults ---
FAST_MODE=false
SKIP_CARGO_TEST=false
CUSTOM_MCP_PORT="${AT_PC_TEST_MCP_PORT:-}"
CUSTOM_WS_PORT="${AT_PC_TEST_WS_PORT:-}"
CUSTOM_BIN="${AT_PC_BIN:-}"

print_usage() {
    cat <<EOF
Usage: deploy/verify_gate.sh [OPTIONS]

Runs the M0 API Key Gate verification test suite (G0-1 through G0-10).

Options:
    --fast              Skip rebuilding release binary if it already exists
    --skip-cargo-test   Skip G0-1 cargo test regression suite (for fast live-gate testing)
    --mcp-port <PORT>   Set MCP HTTP/SSE test port (default: 9800 if free, else dynamic)
    --ws-port <PORT>    Set WebSocket test port (default: 9801 if free, else dynamic)
    --bin <PATH>        Custom path to at-pc-server binary
    -h, --help          Show this help message

Environment Variables:
    AT_PC_TEST_MCP_PORT Override MCP test port
    AT_PC_TEST_WS_PORT  Override WS test port
    AT_PC_BIN           Override binary path
    FAST=1              Equivalent to --fast
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --fast)
            FAST_MODE=true
            shift
            ;;
        --skip-cargo-test)
            SKIP_CARGO_TEST=true
            shift
            ;;
        --mcp-port)
            CUSTOM_MCP_PORT="$2"
            shift 2
            ;;
        --ws-port)
            CUSTOM_WS_PORT="$2"
            shift 2
            ;;
        --bin)
            CUSTOM_BIN="$2"
            shift 2
            ;;
        -h|--help)
            print_usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            print_usage
            exit 1
            ;;
    esac
done

if [ "${FAST:-0}" = "1" ]; then
    FAST_MODE=true
fi

# --- Global Tracking ---
TOTAL_CHECKS=0
PASS_COUNT=0
FAIL_COUNT=0
FAILED_STEPS=()
PIDS_TO_KILL=()

WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/atpc-gate-verify-XXXXXX")

cleanup() {
    local exit_code=$?
    # Stop any background processes started by this script
    if [ ${#PIDS_TO_KILL[@]} -gt 0 ]; then
        for pid in "${PIDS_TO_KILL[@]}"; do
            if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
                kill "$pid" 2>/dev/null || true
                wait "$pid" 2>/dev/null || true
            fi
        done
    fi
    # Remove temporary work directory
    if [ -d "$WORK_DIR" ]; then
        rm -rf "$WORK_DIR"
    fi
    exit "$exit_code"
}
trap cleanup EXIT INT TERM

# --- Port Management ---
is_port_in_use() {
    local port="$1"
    if command -v lsof >/dev/null 2>&1; then
        lsof -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1
    elif command -v nc >/dev/null 2>&1; then
        nc -z 127.0.0.1 "$port" >/dev/null 2>&1
    else
        bash -c "</dev/tcp/127.0.0.1/$port" >/dev/null 2>&1
    fi
}

find_free_port() {
    local port="$1"
    while is_port_in_use "$port"; do
        port=$((port + 1))
    done
    echo "$port"
}

# MCP Port Selection
if [ -n "$CUSTOM_MCP_PORT" ]; then
    MCP_PORT="$CUSTOM_MCP_PORT"
else
    if is_port_in_use 9800; then
        MCP_PORT=$(find_free_port 19800)
    else
        MCP_PORT=9800
    fi
fi

# WS Port Selection
if [ -n "$CUSTOM_WS_PORT" ]; then
    WS_PORT="$CUSTOM_WS_PORT"
else
    if is_port_in_use 9801 || [ "$MCP_PORT" -eq 9801 ]; then
        WS_PORT=$(find_free_port 19801)
        if [ "$WS_PORT" -eq "$MCP_PORT" ]; then
            WS_PORT=$(find_free_port $((WS_PORT + 1)))
        fi
    else
        WS_PORT=9801
    fi
fi

# LAN IP Detection for Non-loopback Checks
detect_lan_ip() {
    local lan=""
    if [[ "$OSTYPE" == "darwin"* ]]; then
        lan=$(ipconfig getifaddr en0 2>/dev/null || ipconfig getifaddr en1 2>/dev/null || true)
    fi
    if [ -z "$lan" ] && command -v hostname >/dev/null 2>&1; then
        lan=$(hostname -I 2>/dev/null | awk '{print $1}' || true)
    fi
    if [ -z "$lan" ]; then
        lan=$(ifconfig 2>/dev/null | grep "inet " | grep -v "127.0.0.1" | awk '{print $2}' | head -n 1 || true)
    fi
    echo "$lan"
}
LAN_IP=$(detect_lan_ip)

# --- Process Helpers ---
stop_server() {
    local pid="$1"
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
        for _ in {1..30}; do
            if ! kill -0 "$pid" 2>/dev/null; then
                break
            fi
            sleep 0.1
        done
        if kill -0 "$pid" 2>/dev/null; then
            kill -9 "$pid" 2>/dev/null || true
        fi
        wait "$pid" 2>/dev/null || true
    fi
}

wait_for_http() {
    local url="$1"
    local expected_code="${2:-200}"
    local auth_header="${3:-}"
    local timeout_sec="${4:-10}"

    local auth_args=()
    if [ -n "$auth_header" ]; then
        auth_args=(-H "$auth_header")
    fi

    local start_time
    start_time=$(date +%s)
    while true; do
        local code
        if [ ${#auth_args[@]} -gt 0 ]; then
            code=$(curl -s -o /dev/null -w "%{http_code}" "${auth_args[@]}" "$url" 2>/dev/null || echo "000")
        else
            code=$(curl -s -o /dev/null -w "%{http_code}" "$url" 2>/dev/null || echo "000")
        fi
        if [ "$code" = "$expected_code" ]; then
            return 0
        fi
        local now
        now=$(date +%s)
        if [ $((now - start_time)) -ge "$timeout_sec" ]; then
            return 1
        fi
        sleep 0.2
    done
}

run_check() {
    local check_id="$1"
    local desc="$2"
    shift 2

    TOTAL_CHECKS=$((TOTAL_CHECKS + 1))
    printf "  [..] %-8s: %s ..." "$check_id" "$desc"

    set +e
    "$@"
    local rc=$?
    set -e

    if [ "$rc" -eq 0 ]; then
        printf "\r  [${GREEN}PASS${NC}] %-8s: %s\n" "$check_id" "$desc"
        PASS_COUNT=$((PASS_COUNT + 1))
        return 0
    else
        printf "\r  [${RED}FAIL${NC}] %-8s: %s\n" "$check_id" "$desc"
        FAIL_COUNT=$((FAIL_COUNT + 1))
        FAILED_STEPS+=("$check_id: $desc")
        return 1
    fi
}

# ==============================================================================
# Banner & Environment Setup
# ==============================================================================

echo "======================================================================"
echo -e "${BOLD}at-pc API Key Gate M0 Verification Suite (G0 Checks)${NC}"
echo "======================================================================"
echo "  Repo root : $REPO_ROOT"
echo "  Work dir  : $WORK_DIR"
echo "  MCP port  : $MCP_PORT"
echo "  WS port   : $WS_PORT"
if [ -n "$LAN_IP" ]; then
    echo "  LAN IP    : $LAN_IP"
else
    echo "  LAN IP    : (none detected, loopback only)"
fi
echo "======================================================================"
echo ""

# Determine binary path and build if needed
BIN="${CUSTOM_BIN:-$REPO_ROOT/target/release/at-pc-server}"

if [ "$FAST_MODE" = true ] && [ -x "$BIN" ]; then
    echo -e "${CYAN}→ [FAST MODE] Skipping server build (${BIN} found)${NC}"
else
    echo -e "${CYAN}→ Building headless release server (cargo build --release -p at-pc-server --no-default-features)...${NC}"
    cargo build --release -p at-pc-server --no-default-features
fi

if [ ! -x "$BIN" ]; then
    echo -e "${RED}ERROR: Binary not found or not executable: $BIN${NC}" >&2
    exit 1
fi
echo ""

# Shared server state across G0-4, G0-5, G0-7, G0-8, G0-10
SHARED_DIR="$WORK_DIR/g0_shared"
SHARED_PID=""
BOOTSTRAP_KEY=""

# ==============================================================================
# Test Implementations
# ==============================================================================

# --- Check G0-1 ---
check_g0_1() {
    # 1. Verify F1/F2/F3 tests have no diff from HEAD
    if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        local f1="crates/server/tests/p1_auth_rbac_call_id_test.rs"
        local f2="crates/server/tests/static_asset_security_test.rs"
        local f3="crates/server/tests/ws_gateway_test.rs"
        if ! git diff --quiet HEAD -- "$f1" "$f2" "$f3"; then
            echo "" >&2
            echo "       FAIL: F1/F2/F3 test files have uncommitted git diff against HEAD" >&2
            git diff --stat HEAD -- "$f1" "$f2" "$f3" >&2
            return 1
        fi
    fi

    # 2. Run P0 boot and health loopback tests
    if ! cargo test -p at-pc-server --test p0_boot_gate_test --test p0_health_loopback_test >/dev/null 2>&1; then
        echo "" >&2
        echo "       FAIL: p0_boot_gate_test or p0_health_loopback_test failed" >&2
        cargo test -p at-pc-server --test p0_boot_gate_test --test p0_health_loopback_test -- --nocapture || true
        return 1
    fi

    # 3. Run F1/F2/F3 locked baseline tests
    if ! cargo test -p at-pc-server \
        --test p1_auth_rbac_call_id_test \
        --test static_asset_security_test \
        --test ws_gateway_test >/dev/null 2>&1; then
        echo "" >&2
        echo "       FAIL: F1/F2/F3 baseline tests failed" >&2
        return 1
    fi

    # 4. Full workspace regression test
    local test_output
    test_output=$(cargo test --workspace --no-fail-fast 2>&1) || {
        echo "" >&2
        echo "       FAIL: cargo test --workspace failed" >&2
        echo "$test_output" | tail -n 25 >&2
        return 1
    }

    local total_failed
    total_failed=$(echo "$test_output" | grep -E "^test result:" | awk -F'[ ;]' '{f+=$6} END {print f+0}')
    if [ "$total_failed" -ne 0 ]; then
        echo "" >&2
        echo "       FAIL: workspace test suite reported $total_failed failed test(s)" >&2
        return 1
    fi

    return 0
}

# --- Check G0-2 ---
check_g0_2() {
    local dir="$WORK_DIR/g0_2"
    mkdir -p "$dir"

    AT_PC_DATA_DIR="$dir" "$BIN" \
        --host 0.0.0.0 \
        --no-bootstrap \
        --mcp-port "$MCP_PORT" \
        --ws-port "$WS_PORT" \
        >"$dir/stdout.log" 2>&1 &
    local pid=$!

    # Wait up to 5s for process to exit
    local start_time
    start_time=$(date +%s)
    while kill -0 "$pid" 2>/dev/null; do
        sleep 0.1
        local now
        now=$(date +%s)
        if [ $((now - start_time)) -ge 5 ]; then
            kill -9 "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
            echo "" >&2
            echo "       FAIL: Server did not exit on non-loopback without credentials" >&2
            return 1
        fi
    done

    wait "$pid" 2>/dev/null
    local exit_code=$?

    if [ "$exit_code" -ne 1 ]; then
        echo "" >&2
        echo "       FAIL: Expected exit code 1, got $exit_code" >&2
        cat "$dir/stdout.log" >&2
        return 1
    fi

    if ! grep -qiE "credential|--insecure-no-auth" "$dir/stdout.log"; then
        echo "" >&2
        echo "       FAIL: stdout/stderr missing credential or --insecure-no-auth instructions" >&2
        cat "$dir/stdout.log" >&2
        return 1
    fi

    if is_port_in_use "$MCP_PORT"; then
        echo "" >&2
        echo "       FAIL: Port $MCP_PORT is listening but expected port to remain unbound" >&2
        return 1
    fi

    return 0
}

# --- Check G0-3 ---
check_g0_3() {
    local dir="$WORK_DIR/g0_3"
    mkdir -p "$dir"

    AT_PC_DATA_DIR="$dir" "$BIN" \
        --host 127.0.0.1 \
        --no-bootstrap \
        --mcp-port "$MCP_PORT" \
        --ws-port "$WS_PORT" \
        >"$dir/stdout.log" 2>&1 &
    local pid=$!
    PIDS_TO_KILL+=("$pid")

    if ! wait_for_http "http://127.0.0.1:$MCP_PORT/health" "200" "" 10; then
        echo "" >&2
        echo "       FAIL: Server did not respond with 200 on /health within 10s" >&2
        cat "$dir/stdout.log" >&2
        stop_server "$pid"
        return 1
    fi

    local term_code
    term_code=$(curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$MCP_PORT/api/terminals" 2>/dev/null || echo "000")
    if [ "$term_code" != "200" ]; then
        echo "" >&2
        echo "       FAIL: Expected /api/terminals to return 200, got $term_code" >&2
        stop_server "$pid"
        return 1
    fi

    stop_server "$pid"
    return 0
}

# --- Check G0-4 ---
check_g0_4() {
    mkdir -p "$SHARED_DIR"

    AT_PC_DATA_DIR="$SHARED_DIR" "$BIN" \
        --host 0.0.0.0 \
        --mcp-port "$MCP_PORT" \
        --ws-port "$WS_PORT" \
        --audit-file "$SHARED_DIR/audit.jsonl" \
        --allow-plaintext \
        >"$SHARED_DIR/stdout.log" 2>&1 &
    local pid=$!
    PIDS_TO_KILL+=("$pid")

    if ! wait_for_http "http://127.0.0.1:$MCP_PORT/health" "200" "" 10; then
        echo "" >&2
        echo "       FAIL: Server did not start cleanly on first boot" >&2
        cat "$SHARED_DIR/stdout.log" >&2
        stop_server "$pid"
        return 1
    fi

    local key_file="$SHARED_DIR/bootstrap_key.txt"
    local found=0
    for _ in {1..50}; do
        if [ -f "$key_file" ]; then
            found=1
            break
        fi
        if ! kill -0 "$pid" 2>/dev/null; then
            break
        fi
        sleep 0.1
    done

    if [ "$found" -ne 1 ]; then
        echo "" >&2
        echo "       FAIL: bootstrap_key.txt was not generated" >&2
        cat "$SHARED_DIR/stdout.log" >&2
        stop_server "$pid"
        return 1
    fi

    # Check Unix permissions (0600)
    local perms=""
    if [[ "$OSTYPE" == "darwin"* ]]; then
        perms=$(stat -f "%Lp" "$key_file" 2>/dev/null || true)
    else
        perms=$(stat -c "%a" "$key_file" 2>/dev/null || true)
    fi
    if [ "$perms" != "600" ]; then
        echo "" >&2
        echo "       FAIL: Expected bootstrap_key.txt mode 0600, got $perms" >&2
        stop_server "$pid"
        return 1
    fi

    local key_in_file
    key_in_file=$(grep -oE 'atpc_sk_[0-9a-z]{8}_[A-Za-z0-9_-]{43}' "$key_file" | head -n 1 || true)
    local key_in_stdout
    key_in_stdout=$(grep -oE 'atpc_sk_[0-9a-z]{8}_[A-Za-z0-9_-]{43}' "$SHARED_DIR/stdout.log" | head -n 1 || true)

    if [ -z "$key_in_file" ]; then
        echo "" >&2
        echo "       FAIL: No valid key pattern found in $key_file" >&2
        stop_server "$pid"
        return 1
    fi

    if [ "$key_in_file" != "$key_in_stdout" ]; then
        echo "" >&2
        echo "       FAIL: Key in file ($key_in_file) does not match stdout ($key_in_stdout)" >&2
        stop_server "$pid"
        return 1
    fi

    if ! echo "$key_in_file" | grep -qE '^atpc_sk_[0-9a-z]{8}_[A-Za-z0-9_-]{43}$'; then
        echo "" >&2
        echo "       FAIL: Key '$key_in_file' failed format regex ^atpc_sk_[0-9a-z]{8}_[A-Za-z0-9_-]{43}$" >&2
        stop_server "$pid"
        return 1
    fi

    BOOTSTRAP_KEY="$key_in_file"

    # Stop server to test restart idempotency
    stop_server "$pid"

    # Restart in the SAME data directory
    AT_PC_DATA_DIR="$SHARED_DIR" "$BIN" \
        --host 0.0.0.0 \
        --mcp-port "$MCP_PORT" \
        --ws-port "$WS_PORT" \
        --audit-file "$SHARED_DIR/audit.jsonl" \
        --allow-plaintext \
        >"$SHARED_DIR/stdout_restart.log" 2>&1 &
    local restart_pid=$!
    PIDS_TO_KILL+=("$restart_pid")
    SHARED_PID="$restart_pid"

    if ! wait_for_http "http://127.0.0.1:$MCP_PORT/health" "200" "" 10; then
        echo "" >&2
        echo "       FAIL: Server did not restart cleanly on second boot" >&2
        cat "$SHARED_DIR/stdout_restart.log" >&2
        return 1
    fi

    local key_after_restart
    key_after_restart=$(grep -oE 'atpc_sk_[0-9a-z]{8}_[A-Za-z0-9_-]{43}' "$key_file" | head -n 1 || true)
    if [ "$key_after_restart" != "$BOOTSTRAP_KEY" ]; then
        echo "" >&2
        echo "       FAIL: Key changed after restart (was $BOOTSTRAP_KEY, now $key_after_restart)" >&2
        return 1
    fi

    local count_in_restart_stdout
    count_in_restart_stdout=$(grep -cE 'atpc_sk_[0-9a-z]{8}_[A-Za-z0-9_-]{43}' "$SHARED_DIR/stdout_restart.log" || true)
    if [ "$count_in_restart_stdout" -ne 0 ]; then
        echo "" >&2
        echo "       FAIL: Second boot stdout printed bootstrap key (expected 0, got $count_in_restart_stdout)" >&2
        return 1
    fi

    return 0
}

# --- Check G0-5 ---
check_g0_5() {
    if [ -z "$SHARED_PID" ] || ! kill -0 "$SHARED_PID" 2>/dev/null; then
        echo "" >&2
        echo "       FAIL: Shared server from G0-4 is not running" >&2
        return 1
    fi

    # 1. Loopback without key: returns 200 with minimal payload {"status":"ok"}
    local lb_no_key
    lb_no_key=$(curl -s "http://127.0.0.1:$MCP_PORT/health" 2>/dev/null || true)
    if ! echo "$lb_no_key" | grep -q '"status"[[:space:]]*:[[:space:]]*"ok"'; then
        echo "" >&2
        echo "       FAIL: Loopback /health without key did not contain '\"status\":\"ok\"'" >&2
        echo "       Response: $lb_no_key" >&2
        return 1
    fi

    # Check no sensitive fields leaked
    if echo "$lb_no_key" | grep -qE "version|mcp_port|ws_port|connected_terminals"; then
        echo "" >&2
        echo "       FAIL: Loopback /health without key leaked sensitive fields" >&2
        echo "       Response: $lb_no_key" >&2
        return 1
    fi

    # 2. Loopback with key: returns 200 with full fields (e.g. connected_terminals)
    local lb_with_key
    lb_with_key=$(curl -s -H "Authorization: Bearer $BOOTSTRAP_KEY" "http://127.0.0.1:$MCP_PORT/health" 2>/dev/null || true)
    if ! echo "$lb_with_key" | grep -q "connected_terminals"; then
        echo "" >&2
        echo "       FAIL: Loopback /health with key missing 'connected_terminals'" >&2
        echo "       Response: $lb_with_key" >&2
        return 1
    fi

    # 3. Non-loopback via LAN IP: 401 without key, 200 with key
    if [ -n "$LAN_IP" ]; then
        local lan_no_key_code
        lan_no_key_code=$(curl -s -o /dev/null -w "%{http_code}" "http://$LAN_IP:$MCP_PORT/health" 2>/dev/null || echo "000")
        if [ "$lan_no_key_code" != "401" ]; then
            echo "" >&2
            echo "       FAIL: Non-loopback /health without key expected 401, got $lan_no_key_code" >&2
            return 1
        fi

        local lan_with_key_code
        lan_with_key_code=$(curl -s -o /dev/null -w "%{http_code}" -H "Authorization: Bearer $BOOTSTRAP_KEY" "http://$LAN_IP:$MCP_PORT/health" 2>/dev/null || echo "000")
        if [ "$lan_with_key_code" != "200" ]; then
            echo "" >&2
            echo "       FAIL: Non-loopback /health with key expected 200, got $lan_with_key_code" >&2
            return 1
        fi
    fi

    return 0
}

# --- Check G0-7 ---
check_g0_7() {
    if [ -z "$SHARED_PID" ] || ! kill -0 "$SHARED_PID" 2>/dev/null; then
        echo "" >&2
        echo "       FAIL: Shared server from G0-4 is not running" >&2
        return 1
    fi

    local target_host="${LAN_IP:-127.0.0.1}"

    # 1. Static root / returns 200
    local root_code
    root_code=$(curl -s -o /dev/null -w "%{http_code}" "http://$target_host:$MCP_PORT/" 2>/dev/null || echo "000")
    if [ "$root_code" != "200" ]; then
        echo "" >&2
        echo "       FAIL: GET / expected 200, got $root_code" >&2
        return 1
    fi

    # 2. Static /dashboard returns 200
    local dash_code
    dash_code=$(curl -s -o /dev/null -w "%{http_code}" "http://$target_host:$MCP_PORT/dashboard" 2>/dev/null || echo "000")
    if [ "$dash_code" != "200" ]; then
        echo "" >&2
        echo "       FAIL: GET /dashboard expected 200, got $dash_code" >&2
        return 1
    fi

    # 3. /api/terminals without key returns 401
    local term_code
    term_code=$(curl -s -o /dev/null -w "%{http_code}" "http://$target_host:$MCP_PORT/api/terminals" 2>/dev/null || echo "000")
    if [ "$term_code" != "401" ]; then
        echo "" >&2
        echo "       FAIL: GET /api/terminals without key expected 401, got $term_code" >&2
        return 1
    fi

    # 4. /api/terminals does not leak terminal_id
    local term_body
    term_body=$(curl -s "http://$target_host:$MCP_PORT/api/terminals" 2>/dev/null || true)
    if echo "$term_body" | grep -q "terminal_id"; then
        echo "" >&2
        echo "       FAIL: GET /api/terminals without key leaked 'terminal_id'" >&2
        return 1
    fi

    # 5. Static shell / does not leak sensitive credentials or active sessions
    local root_body
    root_body=$(curl -s "http://$target_host:$MCP_PORT/" 2>/dev/null || true)
    if echo "$root_body" | grep -qE "atpc_sk_"; then
        echo "" >&2
        echo "       FAIL: Static root shell leaked root API key (atpc_sk_)" >&2
        return 1
    fi

    return 0
}

# --- Check G0-8 ---
check_g0_8() {
    if [ -z "$SHARED_PID" ] || ! kill -0 "$SHARED_PID" 2>/dev/null; then
        echo "" >&2
        echo "       FAIL: Shared server from G0-4 is not running" >&2
        return 1
    fi

    local sse_data
    sse_data=$(curl -sN -H "Authorization: Bearer $BOOTSTRAP_KEY" --max-time 3 "http://127.0.0.1:$MCP_PORT/sse" 2>/dev/null | head -n 10 || true)

    if ! echo "$sse_data" | grep -q "/messages"; then
        echo "" >&2
        echo "       FAIL: /sse response did not contain '/messages' endpoint event data" >&2
        echo "       Output: $sse_data" >&2
        return 1
    fi

    if echo "$sse_data" | grep -q "token="; then
        echo "" >&2
        echo "       FAIL: /sse endpoint event data leaked token parameter" >&2
        echo "       Output: $sse_data" >&2
        return 1
    fi

    return 0
}

# --- Check G0-10 ---
check_g0_10() {
    if [ -z "$SHARED_PID" ] || ! kill -0 "$SHARED_PID" 2>/dev/null; then
        echo "" >&2
        echo "       FAIL: Shared server from G0-4 is not running" >&2
        return 1
    fi

    local bad_key="atpc_sk_aaaaaaaa_ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"
    local target_host="${LAN_IP:-127.0.0.1}"

    # Request with invalid key -> 401
    local code
    code=$(curl -s -o /dev/null -w "%{http_code}" -H "Authorization: Bearer $bad_key" "http://$target_host:$MCP_PORT/api/terminals" 2>/dev/null || echo "000")
    if [ "$code" != "401" ]; then
        echo "" >&2
        echo "       FAIL: Request with invalid key expected 401, got $code" >&2
        return 1
    fi

    # Wait for audit record to flush
    local audit_file="$SHARED_DIR/audit.jsonl"
    local recorded=0
    for _ in {1..30}; do
        if [ -f "$audit_file" ] && grep -q "atp\*\*\*" "$audit_file" 2>/dev/null; then
            recorded=1
            break
        fi
        sleep 0.1
    done

    if [ "$recorded" -ne 1 ]; then
        echo "" >&2
        echo "       FAIL: AUTH_FAILED audit record not found in $audit_file" >&2
        return 1
    fi

    local prefix=""
    if command -v jq >/dev/null 2>&1; then
        prefix=$(jq -r 'select(.action=="AUTH_FAILED") | .token_prefix' "$audit_file" 2>/dev/null | tail -n 1 || true)
    else
        prefix=$(grep '"action":"AUTH_FAILED"' "$audit_file" | grep -oE '"token_prefix":"[^"]+"' | tail -n 1 | cut -d'"' -f4 || true)
    fi

    if [ "$prefix" != "atp***" ]; then
        echo "" >&2
        echo "       FAIL: Expected redacted token_prefix 'atp***', got '$prefix'" >&2
        return 1
    fi

    if grep -q "atpc_sk_aaaaaaaa" "$audit_file"; then
        echo "" >&2
        echo "       FAIL: Raw secret key leaked into audit log $audit_file" >&2
        return 1
    fi

    return 0
}

# ==============================================================================
# Execution Sequence
# ==============================================================================

if [ "$SKIP_CARGO_TEST" = true ]; then
    echo -e "  [${YELLOW}SKIP${NC}] G0-1    : Skipped (--skip-cargo-test specified)"
else
    run_check "G0-1" "Workspace tests, P0 boot/health tests, and F1/F2/F3 zero diff" check_g0_1
fi

run_check "G0-2" "Non-loopback without credentials fails to start (exit 1, ports unbound)" check_g0_2
run_check "G0-3" "Loopback without credentials starts and allows dev access" check_g0_3
run_check "G0-4" "First boot generates bootstrap_key.txt (0600), format match, idempotency" check_g0_4
run_check "G0-5" "/health loopback minimal exemption vs non-loopback authentication" check_g0_5
run_check "G0-7" "Static dashboard shell is reachable; data endpoints require authentication" check_g0_7
run_check "G0-8" "/sse endpoint event data is /messages without token leak" check_g0_8
run_check "G0-10" "Gate authenticity: AUTH_FAILED audit record has redacted token_prefix" check_g0_10

# Cleanly terminate shared server
if [ -n "$SHARED_PID" ]; then
    stop_server "$SHARED_PID"
    SHARED_PID=""
fi

# ==============================================================================
# Final Summary
# ==============================================================================

echo ""
echo "======================================================================"
echo -e "${BOLD}M0 API Key Gate Verification Summary${NC}"
echo "======================================================================"
echo " Total checks : $TOTAL_CHECKS"
echo -e " Passed       : ${GREEN}${PASS_COUNT}${NC}"
if [ "$FAIL_COUNT" -gt 0 ]; then
    echo -e " Failed       : ${RED}${FAIL_COUNT}${NC}"
    echo ""
    echo -e "${RED}Failed Checks:${NC}"
    for step in "${FAILED_STEPS[@]}"; do
        echo -e "  - ${RED}${step}${NC}"
    done
    echo "======================================================================"
    exit 1
else
    echo -e " Failed       : 0"
    echo "======================================================================"
    echo -e "${GREEN}✔ All M0 API Key Gate verification checks PASSED.${NC}"
    exit 0
fi
