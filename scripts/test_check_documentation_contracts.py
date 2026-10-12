#!/usr/bin/env python3
"""Regression tests for public-documentation contract checks."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_documentation_contracts as chk


class DocumentationContractTests(unittest.TestCase):
    def test_current_repository_passes(self) -> None:
        self.assertEqual(chk.check_files(), [])

    def test_nested_computer_use_fails(self) -> None:
        errors = chk.check_agent_config(
            {
                "server": {"url": "ws://127.0.0.1:9801/ws"},
                "device": {"enable_computer_use": False},
            }
        )
        self.assertTrue(any("root enable_computer_use" in error for error in errors))
        self.assertTrue(any("must not nest" in error for error in errors))

    def test_insecure_server_sample_fails(self) -> None:
        errors = chk.check_server_config(
            {"listen_host": "0.0.0.0", "allow_plaintext": True}
        )
        self.assertEqual(len(errors), 2)

    def test_unverified_claims_fail(self) -> None:
        errors = chk.check_current_claims("README", "客户端断线指数退避，注册表 O(1)")
        self.assertTrue(any("指数退避" in error for error in errors))
        self.assertTrue(any("O(1)" in error for error in errors))

    def test_secret_and_private_ip_fail_but_placeholders_pass(self) -> None:
        leaked = "atpc_sk_abcd1234_abcdefghijklmnopqrstuvwxyz123456 192.168.1.50"
        errors = chk.check_sensitive_text("README", leaked)
        self.assertEqual(len(errors), 2)
        safe = "atpc_sk_<id>_<secret> ws://<server-host>:9801/ws"
        self.assertEqual(chk.check_sensitive_text("README", safe), [])

    def test_missing_ci_command_fails(self) -> None:
        errors = chk.check_ci_text("python3 scripts/check_route_security.py")
        self.assertEqual(len(errors), 3)


if __name__ == "__main__":
    unittest.main()
