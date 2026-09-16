#!/usr/bin/env python3
"""Unit tests for check_perf_thresholds.py."""

from __future__ import annotations

import unittest
from pathlib import Path

from check_perf_thresholds import (
    check_probe_against_thresholds,
    parse_probe_output,
    validate_thresholds_config,
    ROOT,
    THRESHOLDS_BENCH_FILE,
)


class TestCheckPerfThresholds(unittest.TestCase):
    def test_validate_repo_thresholds(self):
        result = validate_thresholds_config(THRESHOLDS_BENCH_FILE)
        self.assertEqual(result["block_hash_1080p_ms_max"], 1.0)
        self.assertEqual(result["block_hash_2560_ms_max"], 2.0)
        self.assertGreater(result["relative_regression_percent"], 0.0)

    def test_parse_probe_output(self):
        sample = """
================================================================================================
2. STAGE ATTRIBUTION at 1920x1080 (q55)
================================================================================================
dirty-check block hash    0.494 ms      2.5%
RGBA -> RGB             0.573 ms      2.9%
JPEG encode q55        18.711 ms     94.6%
TOTAL per frame        19.778 ms
"""
        metrics = parse_probe_output(sample)
        self.assertAlmostEqual(metrics["block_hash_1080p_ms"], 0.494)
        self.assertAlmostEqual(metrics["rgba_to_rgb_ms"], 0.573)

    def test_check_probe_pass(self):
        thresholds = {"block_hash_1080p_ms_max": 1.00, "block_hash_2560_ms_max": 2.00}
        metrics = {"block_hash_1080p_ms": 0.55}
        violations = check_probe_against_thresholds(metrics, thresholds)
        self.assertEqual(violations, [])

    def test_check_probe_fail(self):
        thresholds = {"block_hash_1080p_ms_max": 1.00, "block_hash_2560_ms_max": 2.00}
        metrics = {"block_hash_1080p_ms": 1.45}
        violations = check_probe_against_thresholds(metrics, thresholds)
        self.assertEqual(len(violations), 1)
        self.assertIn("exceeded", violations[0])


if __name__ == "__main__":
    unittest.main()
