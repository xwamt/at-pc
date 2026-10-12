#!/usr/bin/env python3
"""Regression tests for the route-security structural guard."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_route_security as chk


class RouteSecurityCheckTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.mcp = chk.MCP.read_text(encoding="utf-8")
        cls.dashboard = chk.DASHBOARD.read_text(encoding="utf-8")
        cls.config = chk.CONFIG.read_text(encoding="utf-8")

    def test_current_sources_pass(self) -> None:
        self.assertEqual(chk.check_sources(self.mcp, self.dashboard, self.config), [])

    def test_missing_global_auth_layer_fails(self) -> None:
        weakened = self.mcp.replace(
            ".route_layer(axum::middleware::from_fn_with_state(",
            ".layer(axum::middleware::from_fn_with_state(",
            1,
        )
        errors = chk.check_sources(weakened, self.dashboard, self.config)
        self.assertTrue(any("global auth route layer" in error for error in errors))

    def test_broad_api_exemption_fails(self) -> None:
        widened = self.mcp.replace(
            'if path == "/api/auth/login" && *method == axum::http::Method::POST {',
            'if path.starts_with("/api") {',
            1,
        )
        errors = chk.check_sources(widened, self.dashboard, self.config)
        self.assertTrue(any("broad auth exemption" in error for error in errors))

    def test_missing_quota_header_fails(self) -> None:
        weakened = self.mcp.replace('"x-ratelimit-reset"', '"x-rate-limit-reset"')
        errors = chk.check_sources(weakened, self.dashboard, self.config)
        self.assertTrue(any("x-ratelimit-reset" in error for error in errors))

    def test_missing_desktop_input_denied_audit_fails(self) -> None:
        weakened = self.dashboard.replace(
            'action: "api:desktop_input".to_string()',
            'action: "api:desktop-input".to_string()',
        )
        errors = chk.check_sources(self.mcp, weakened, self.config)
        self.assertTrue(any("desktop-input denied audit action" in error for error in errors))

    def test_missing_static_canonical_guard_fails(self) -> None:
        weakened = self.dashboard.replace(
            "canonical_asset.starts_with(&canonical_root)",
            "canonical_asset.ends_with(&canonical_root)",
            1,
        )
        errors = chk.check_sources(self.mcp, weakened, self.config)
        self.assertTrue(any("canonical-root" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
