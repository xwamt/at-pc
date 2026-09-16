#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "==> Verifying benchmarks compilation without running..."
cargo bench -p at-pc-benchmarks --no-run

echo "==> Validating performance threshold configurations..."
python3 "${SCRIPT_DIR}/check_perf_thresholds.py" --check-config
python3 "${SCRIPT_DIR}/test_check_perf_thresholds.py"

echo "==> CI performance threshold gate passed successfully."
