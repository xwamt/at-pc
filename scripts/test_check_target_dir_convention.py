#!/usr/bin/env python3
"""Tests for host/cross target-dir convention checks.

Locks structured YAML/TOML parsing: comments and echo strings must not count.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_target_dir_convention as chk

COMMENT_ONLY_RUST_CACHE = """
name: CI
jobs:
  quality:
    runs-on: ubuntu-24.04
    env:
      CARGO_TARGET_DIR: target/linux
    steps:
      - uses: actions/checkout@v4
      # uses: Swatinem/rust-cache@v2
      - run: cargo test --locked
  platform-check:
    runs-on: macos-14
    env:
      CARGO_TARGET_DIR: target/mac
    steps:
      - uses: Swatinem/rust-cache@v2
"""

ECHO_ONLY_TOKENS = """
name: CI
jobs:
  quality:
    runs-on: ubuntu-24.04
    env:
      CARGO_TARGET_DIR: target/linux
    steps:
      - run: echo CARGO_TARGET_DIR Swatinem/rust-cache
  platform-check:
    runs-on: windows-2022
    env:
      CARGO_TARGET_DIR: target/win
    steps:
      - uses: Swatinem/rust-cache@v2
"""

DOCUMENTED_ALIASES = """
# Isolation: CARGO_TARGET_DIR=target/mac | target/linux | target/win
[alias]
build-win-gnu = "build --target x86_64-pc-windows-gnu"
build-win-msvc = "build --target x86_64-pc-windows-msvc"
"""

BAKED_ENV = """
# Isolation: CARGO_TARGET_DIR=target/mac | target/linux | target/win
[env]
CARGO_TARGET_DIR = "target"
[alias]
build-win-gnu = "build --target x86_64-pc-windows-gnu"
build-win-msvc = "build --target x86_64-pc-windows-msvc"
"""

BAKED_BUILD_TARGET_DIR = """
# Isolation: CARGO_TARGET_DIR=target/mac | target/linux | target/win
[build]
target-dir = "target"
[alias]
build-win-gnu = "build --target x86_64-pc-windows-gnu"
build-win-msvc = "build --target x86_64-pc-windows-msvc"
"""


class CompileJobYamlTests(unittest.TestCase):
    def test_comment_only_rust_cache_fails(self):
        err = chk.check_compile_jobs_yaml(COMMENT_ONLY_RUST_CACHE)
        self.assertIsNotNone(err)
        self.assertIn("quality", err)

    def test_echo_only_tokens_fail(self):
        err = chk.check_compile_jobs_yaml(ECHO_ONLY_TOKENS)
        self.assertIsNotNone(err)
        self.assertIn("quality", err)

    def test_current_ci_yml_passes(self):
        raw = Path(chk.CI_WORKFLOW).read_text(encoding="utf-8")
        self.assertIsNone(chk.check_compile_jobs_yaml(raw))


class CargoConfigTests(unittest.TestCase):
    def test_baked_env_cargo_target_dir_fails(self):
        err = chk.check_cargo_config_text(BAKED_ENV)
        self.assertIsNotNone(err)
        self.assertRegex(err.lower(), r"\[env\]|env")

    def test_baked_build_target_dir_fails(self):
        err = chk.check_cargo_config_text(BAKED_BUILD_TARGET_DIR)
        self.assertIsNotNone(err)
        self.assertIn("target-dir", err)

    def test_documented_aliases_without_baked_env_pass(self):
        self.assertIsNone(chk.check_cargo_config_text(DOCUMENTED_ALIASES))

    def test_current_config_toml_passes(self):
        raw = Path(chk.CONFIG).read_text(encoding="utf-8")
        self.assertIsNone(chk.check_cargo_config_text(raw))


if __name__ == "__main__":
    unittest.main()
