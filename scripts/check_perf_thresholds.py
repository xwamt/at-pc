#!/usr/bin/env python3
"""Validates performance thresholds configuration and optionally asserts runtime probe limits.

Usage:
  python3 scripts/check_perf_thresholds.py [--check-config] [--probe-output <path_or_stdin>]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
THRESHOLDS_BENCH_FILE = ROOT / "crates" / "benchmarks" / "thresholds.toml"
THRESHOLDS_ROOT_FILE = ROOT / "thresholds.toml"


def load_toml(path: Path) -> dict:
    if not path.is_file():
        raise FileNotFoundError(f"Thresholds file not found: {path}")
    raw = path.read_text(encoding="utf-8")
    try:
        import tomllib

        return tomllib.loads(raw)
    except ImportError:
        pass
    try:
        import tomli  # type: ignore[import-not-found]

        return tomli.loads(raw)
    except ImportError:
        pass
    # Fallback minimal parser for simple key-value TOML
    data: dict = {}
    current_section = data
    for line in raw.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[") and line.endswith("]"):
            sec_name = line[1:-1].strip()
            parts = sec_name.split(".")
            target = data
            for p in parts:
                target = target.setdefault(p, {})
            current_section = target
            continue
        if "=" in line:
            key, val = line.split("=", 1)
            key = key.strip()
            val = val.strip().split("#")[0].strip()
            if val.lower() == "true":
                current_section[key] = True
            elif val.lower() == "false":
                current_section[key] = False
            else:
                try:
                    if "." in val:
                        current_section[key] = float(val)
                    else:
                        current_section[key] = int(val)
                except ValueError:
                    current_section[key] = val.strip('"\'')
    return data


def validate_thresholds_config(config_path: Path) -> dict:
    data = load_toml(config_path)
    if "schema_version" not in data:
        raise ValueError(f"{config_path}: missing 'schema_version'")

    thresholds = data.get("thresholds", {})
    desktop_core = data.get("benchmarks", {}).get("desktop_core", {})

    p1080_max = desktop_core.get("block_hash_1080p_ms_max") or thresholds.get("block_hash_1080p_ms_max")
    p2560_max = desktop_core.get("block_hash_2560_ms_max") or thresholds.get("block_hash_2560_ms_max")

    if p1080_max is None or float(p1080_max) <= 0:
        raise ValueError(f"{config_path}: invalid or missing block_hash_1080p_ms_max")
    if p2560_max is None or float(p2560_max) <= 0:
        raise ValueError(f"{config_path}: invalid or missing block_hash_2560_ms_max")

    return {
        "block_hash_1080p_ms_max": float(p1080_max),
        "block_hash_2560_ms_max": float(p2560_max),
        "relative_regression_percent": float(thresholds.get("relative_regression_percent", 10.0)),
    }


def parse_probe_output(text: str) -> dict[str, float]:
    """Extract metrics from perf_probe output."""
    metrics: dict[str, float] = {}
    # dirty-check block hash    0.494 ms      2.5%
    m = re.search(r"dirty-check (?:block )?hash\s+([0-9.]+)\s*ms", text)
    if m:
        metrics["block_hash_1080p_ms"] = float(m.group(1))

    # RGBA -> RGB             0.573 ms      2.9%
    m = re.search(r"RGBA\s*->\s*RGB\s+([0-9.]+)\s*ms", text)
    if m:
        metrics["rgba_to_rgb_ms"] = float(m.group(1))

    return metrics


def check_probe_against_thresholds(metrics: dict[str, float], thresholds: dict[str, float]) -> list[str]:
    violations = []
    if "block_hash_1080p_ms" in metrics:
        actual = metrics["block_hash_1080p_ms"]
        limit = thresholds["block_hash_1080p_ms_max"]
        if actual > limit:
            violations.append(f"block_hash_1080p_ms exceeded: actual={actual:.3f}ms > limit={limit:.3f}ms")
    return violations


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-config", action="store_true", help="Validate thresholds.toml schema only")
    parser.add_argument("--probe-output", type=str, help="Path to text file containing perf_probe output")
    args = parser.parse_args()

    # Always validate thresholds configuration
    for cfg in [THRESHOLDS_BENCH_FILE, THRESHOLDS_ROOT_FILE]:
        if cfg.exists():
            thresholds = validate_thresholds_config(cfg)
            print(f"[OK] Validated {cfg.relative_to(ROOT)}: {thresholds}")

    if args.check_config and not args.probe_output:
        return 0

    if args.probe_output:
        content = Path(args.probe_output).read_text(encoding="utf-8")
        metrics = parse_probe_output(content)
        print(f"Extracted probe metrics: {metrics}")
        violations = check_probe_against_thresholds(metrics, thresholds)
        if violations:
            for v in violations:
                print(f"[FAIL] {v}", file=sys.stderr)
            return 1
        print("[OK] All probe metrics satisfy performance threshold gates.")

    return 0


if __name__ == "__main__":
    sys.exit(main())
