#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# at-pc-server Linux Deployment Script
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "=== AT-PC Server Deployment ==="

# Ensure target directories exist
mkdir -p /var/lib/at-pc /var/log/at-pc /etc/at-pc 2>/dev/null || true

# Check mode
MODE="${1:-auto}"

deploy_docker() {
    echo ">> Deploying with Docker Compose..."
    if ! command -v docker >/dev/null 2>&1; then
        echo "Error: docker is not installed or not in PATH." >&2
        exit 1
    fi

    cd "${ROOT_DIR}"
    if docker compose version >/dev/null 2>&1; then
        docker compose up --build -d
    elif command -v docker-compose >/dev/null 2>&1; then
        docker-compose up --build -d
    else
        echo "Error: neither 'docker compose' nor 'docker-compose' found." >&2
        exit 1
    fi

    echo ">> Docker container launched successfully."
    echo ">> Checking status..."
    sleep 2
    "${SCRIPT_DIR}/verify.sh" || true
}

deploy_systemd() {
    echo ">> Installing and deploying via Systemd..."
    if [ ! -f "${SCRIPT_DIR}/systemd/at-pc-server.service" ]; then
        echo "Error: systemd service template not found at ${SCRIPT_DIR}/systemd/at-pc-server.service" >&2
        exit 1
    fi

    # Find or build binary
    BIN_SRC=""
    if [ -f "${ROOT_DIR}/at-pc-server" ]; then
        BIN_SRC="${ROOT_DIR}/at-pc-server"
    elif [ -f "${ROOT_DIR}/target/release/at-pc-server" ]; then
        BIN_SRC="${ROOT_DIR}/target/release/at-pc-server"
    elif command -v cargo >/dev/null 2>&1; then
        echo ">> Building headless at-pc-server binary with Cargo..."
        (cd "${ROOT_DIR}" && cargo build --release -p at-pc-server --no-default-features)
        BIN_SRC="${ROOT_DIR}/target/release/at-pc-server"
    else
        echo "Error: binary at-pc-server not found and cargo not installed." >&2
        exit 1
    fi

    cp -f "${BIN_SRC}" /usr/local/bin/at-pc-server
    chmod 755 /usr/local/bin/at-pc-server

    if [ -f "${ROOT_DIR}/server_config.example.toml" ] && [ ! -f /etc/at-pc/server_config.toml ]; then
        cp "${ROOT_DIR}/server_config.example.toml" /etc/at-pc/server_config.toml
    fi

    cp -f "${SCRIPT_DIR}/systemd/at-pc-server.service" /etc/systemd/system/at-pc-server.service
    systemctl daemon-reload
    systemctl enable at-pc-server
    systemctl restart at-pc-server

    echo ">> Systemd service started. Service status:"
    systemctl status at-pc-server --no-pager || true
    sleep 2
    "${SCRIPT_DIR}/verify.sh" || true
}

deploy_standalone() {
    echo ">> Running standalone in background..."
    BIN_SRC=""
    if [ -f "${ROOT_DIR}/at-pc-server" ]; then
        BIN_SRC="${ROOT_DIR}/at-pc-server"
    elif [ -f "${ROOT_DIR}/target/release/at-pc-server" ]; then
        BIN_SRC="${ROOT_DIR}/target/release/at-pc-server"
    elif command -v cargo >/dev/null 2>&1; then
        echo ">> Building headless at-pc-server binary with Cargo..."
        (cd "${ROOT_DIR}" && cargo build --release -p at-pc-server --no-default-features)
        BIN_SRC="${ROOT_DIR}/target/release/at-pc-server"
    else
        echo "Error: binary at-pc-server not found and cargo not installed." >&2
        exit 1
    fi

    pkill -f "at-pc-server" || true
    sleep 1

    nohup "${BIN_SRC}" \
        --host 0.0.0.0 \
        --ws-port 9801 \
        --mcp-port 9800 \
        --log-file /var/log/at-pc/at-pc-server.log \
        > /dev/null 2>&1 &

    echo ">> Server started in background (PID: $!)."
    sleep 2
    "${SCRIPT_DIR}/verify.sh" || true
}

case "${MODE}" in
    docker)
        deploy_docker
        ;;
    systemd)
        deploy_systemd
        ;;
    standalone)
        deploy_standalone
        ;;
    auto)
        if command -v docker >/dev/null 2>&1 && [ -f "${ROOT_DIR}/docker-compose.yml" ]; then
            deploy_docker
        elif command -v systemctl >/dev/null 2>&1 && [ "$EUID" -eq 0 ]; then
            deploy_systemd
        else
            deploy_standalone
        fi
        ;;
    *)
        echo "Usage: $0 [auto|docker|systemd|standalone]"
        exit 1
        ;;
esac
