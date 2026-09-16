#!/usr/bin/env python3
"""Tests for agent ↔ server Cargo dependency cycle checks."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_no_agent_server_dev_cycle as chk

CYCLE_AGENT = """
[package]
name = "at-pc-agent"

[dev-dependencies]
at-pc-server = { path = "../server" }
"""

CYCLE_SERVER = """
[package]
name = "at-pc-server"

[dev-dependencies]
at-pc-agent = { path = "../agent", default-features = false }
"""

CLEAN_AGENT = """
[package]
name = "at-pc-agent"

[dependencies]
at-pc-protocol = { path = "../protocol" }

[dev-dependencies]
tokio = { workspace = true }
"""

CLEAN_SERVER = """
[package]
name = "at-pc-server"

[dependencies]
at-pc-protocol = { path = "../protocol" }

[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
"""

ONE_WAY_AGENT = """
[package]
name = "at-pc-agent"

[dev-dependencies]
at-pc-server = { path = "../server" }
"""

TARGET_SPECIFIC_CYCLE_AGENT = """
[package]
name = "at-pc-agent"

[target.'cfg(unix)'.dev-dependencies]
at-pc-server = { path = "../server" }
"""

TARGET_SPECIFIC_CYCLE_SERVER = """
[package]
name = "at-pc-server"

[target.'cfg(unix)'.dependencies]
at-pc-agent = { path = "../agent" }
"""


class CycleCheckTests(unittest.TestCase):
    def test_detects_mutual_dev_dependency_cycle(self):
        err = chk.check_manifest_texts(CYCLE_AGENT, CYCLE_SERVER)
        self.assertIsNotNone(err)
        assert err is not None
        self.assertIn("cycle", err)
        self.assertIn("at-pc-agent depends on at-pc-server", err)
        self.assertIn("at-pc-server depends on at-pc-agent", err)

    def test_clean_manifests_pass(self):
        self.assertIsNone(chk.check_manifest_texts(CLEAN_AGENT, CLEAN_SERVER))

    def test_one_way_dependency_is_also_forbidden(self):
        err = chk.check_manifest_texts(ONE_WAY_AGENT, CLEAN_SERVER)
        self.assertIsNotNone(err)
        assert err is not None
        self.assertIn("at-pc-agent depends on at-pc-server", err)
        self.assertNotIn("cycle", err)

    def test_target_specific_tables_count(self):
        err = chk.check_manifest_texts(
            TARGET_SPECIFIC_CYCLE_AGENT, TARGET_SPECIFIC_CYCLE_SERVER
        )
        self.assertIsNotNone(err)
        assert err is not None
        self.assertIn("cycle", err)


if __name__ == "__main__":
    unittest.main()
