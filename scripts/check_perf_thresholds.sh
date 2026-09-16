#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "==> Verifying benchmarks compilation without running..."
cargo bench -p at-pc-benchmarks --no-run

echo "==> Running unit tests for threshold validator..."
python3 "${SCRIPT_DIR}/test_check_perf_thresholds.py"

echo "==> Running release performance probe and asserting threshold limits..."
PROBE_OUTPUT="$(mktemp /tmp/perf_probe_XXXXXX.txt)"
trap 'rm -f "${PROBE_OUTPUT}"' EXIT

cargo run --release -p at-pc-benchmarks --example perf_probe > "${PROBE_OUTPUT}"
python3 "${SCRIPT_DIR}/check_perf_thresholds.py" --probe-output "${PROBE_OUTPUT}"

echo "==> Running release profile performance test assertions..."
cargo test --release -p at-pc-desktop-core --test block_hash_test
cargo test --release -p at-pc-agent --test som_perf_test
cargo test --release -p at-pc-agent --test process_cache_test
if [[ "$(uname)" == "Darwin" ]]; then
  cargo test --release -p at-pc-agent --test macos_window_review_test
fi
cargo test --release -p at-pc-server --test registry_scaling_test

echo "==> All performance threshold gates passed successfully."
