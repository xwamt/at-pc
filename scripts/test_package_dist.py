#!/usr/bin/env python3
"""Tests for the source-to-artifact packaging pipeline (subprocess mocked)."""

from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import subprocess
import tempfile
import unittest
import zipfile
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

_SCRIPT = Path(__file__).resolve().parent / "package_dist.py"
_SPEC = importlib.util.spec_from_file_location("package_dist", _SCRIPT)
package_dist = importlib.util.module_from_spec(_SPEC)
assert _SPEC.loader is not None
_SPEC.loader.exec_module(package_dist)

METADATA_VERSION = "2.4.0"
FAKE_METADATA = json.dumps(
    {
        "packages": [
            {"name": "at-pc-agent", "version": METADATA_VERSION},
            {"name": "at-pc-server", "version": METADATA_VERSION},
        ]
    }
)


def _write_bin(path: Path, payload: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(payload)


def _sha256(path: Path) -> str:
    h = hashlib.sha256()
    h.update(path.read_bytes())
    return h.hexdigest()


class PackageDistTests(unittest.TestCase):
    def _workspace(self) -> tempfile.TemporaryDirectory:
        tmp = tempfile.TemporaryDirectory()
        Path(tmp.name, "README.md").write_text("# AT-PC\n", encoding="utf-8")
        return tmp

    def _run_factory(
        self,
        base: Path,
        *,
        metadata_stdout: str = FAKE_METADATA,
        sign_rc: dict[str, int] | None = None,
        host: str = "aarch64-apple-darwin",
        built: list[tuple[str, ...]] | None = None,
        target_directory: Path | None = None,
        write_bins: bool = True,
    ):
        sign_rc = sign_rc or {}
        built_cmds = built if built is not None else []
        target_root = Path(target_directory) if target_directory is not None else (base / "target")
        meta = json.loads(metadata_stdout)
        meta.setdefault("target_directory", str(target_root))
        metadata_stdout = json.dumps(meta)

        def run(cmd, **kwargs):
            cmd = list(cmd)
            if cmd[:2] == ["rustc", "-vV"]:
                return subprocess.CompletedProcess(
                    cmd, 0, stdout=f"release: 1.0.0\nhost: {host}\n", stderr=""
                )
            if cmd[:2] == ["cargo", "metadata"]:
                return subprocess.CompletedProcess(
                    cmd, 0, stdout=metadata_stdout, stderr=""
                )
            if cmd[:2] == ["cargo", "build"]:
                built_cmds.append(tuple(cmd))
                if "--target" in cmd:
                    triple = cmd[cmd.index("--target") + 1]
                    release = target_root / triple / "release"
                else:
                    triple = host
                    release = target_root / "release"
                windows = "windows" in triple
                agent = "at-pc-agent.exe" if windows else "at-pc-agent"
                server = "at-pc-server.exe" if windows else "at-pc-server"
                if write_bins:
                    _write_bin(release / agent, b"agent-bin")
                    _write_bin(release / server, b"server-bin")
                return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")
            if cmd and cmd[0] in ("xattr", "codesign"):
                rc = sign_rc.get(cmd[0], 0)
                return subprocess.CompletedProcess(cmd, rc, stdout="", stderr="sign-err")
            raise AssertionError(f"unexpected command: {cmd}")

        return run

    def test_version_from_cargo_metadata(self):
        calls = []

        def run(cmd, **kwargs):
            calls.append(list(cmd))
            if list(cmd)[:2] == ["cargo", "metadata"]:
                return subprocess.CompletedProcess(
                    cmd, 0, stdout=FAKE_METADATA, stderr=""
                )
            raise AssertionError(f"unexpected command: {cmd}")

        version = package_dist.resolve_version(
            "/workspace", env={}, run=run
        )
        self.assertEqual(version, METADATA_VERSION)
        self.assertTrue(any(c[:2] == ["cargo", "metadata"] for c in calls))

    def test_matching_cargo_pkg_version_env_is_ok(self):
        calls = []

        def run(cmd, **kwargs):
            calls.append(list(cmd))
            if list(cmd)[:2] == ["cargo", "metadata"]:
                return subprocess.CompletedProcess(
                    cmd, 0, stdout=FAKE_METADATA, stderr=""
                )
            raise AssertionError(f"unexpected command: {cmd}")

        version = package_dist.resolve_version(
            "/workspace",
            env={"CARGO_PKG_VERSION": METADATA_VERSION},
            run=run,
        )
        self.assertEqual(version, METADATA_VERSION)
        self.assertTrue(any(c[:2] == ["cargo", "metadata"] for c in calls))

    def test_cargo_pkg_version_mismatch_exits(self):
        def run(cmd, **kwargs):
            if list(cmd)[:2] == ["cargo", "metadata"]:
                return subprocess.CompletedProcess(
                    cmd, 0, stdout=FAKE_METADATA, stderr=""
                )
            raise AssertionError(f"unexpected command: {cmd}")

        err = io.StringIO()
        with self.assertRaises(SystemExit) as cm:
            with redirect_stderr(err):
                package_dist.resolve_version(
                    "/workspace",
                    env={"CARGO_PKG_VERSION": "9.9.9"},
                    run=run,
                )
        self.assertEqual(cm.exception.code, 1)

    def test_agent_server_version_mismatch_exits(self):
        metadata = json.dumps(
            {
                "packages": [
                    {"name": "at-pc-agent", "version": "1.0.0"},
                    {"name": "at-pc-server", "version": "2.0.0"},
                ]
            }
        )

        def run(cmd, **kwargs):
            if list(cmd)[:2] == ["cargo", "metadata"]:
                return subprocess.CompletedProcess(cmd, 0, stdout=metadata, stderr="")
            raise AssertionError(f"unexpected command: {cmd}")

        with self.assertRaises(SystemExit) as cm:
            with redirect_stderr(io.StringIO()):
                package_dist.resolve_version("/workspace", env={}, run=run)
        self.assertEqual(cm.exception.code, 1)

    def test_sha256sums_written_for_windows_target(self):
        with self._workspace() as name:
            base = Path(name)
            built = []
            run = self._run_factory(base, built=built)
            with redirect_stdout(io.StringIO()):
                package_dist.main(
                    ["--target", "x86_64-pc-windows-msvc"],
                    run=run,
                    base_dir=str(base),
                    env={},
                )
            self.assertIn(
                ("cargo", "build", "--release", "--target", "x86_64-pc-windows-msvc"),
                built,
            )
            dist = base / "dist"
            sums_path = dist / "SHA256SUMS"
            self.assertTrue(sums_path.is_file())
            listed = {}
            for line in sums_path.read_text(encoding="utf-8").splitlines():
                if not line.strip() or line.startswith("#"):
                    continue
                digest, filename = line.split()
                listed[filename] = digest
            for artifact in (
                "at-pc-agent.exe",
                "at-pc-server.exe",
                "at-pc-windows-x86_64.zip",
                "VERSION",
            ):
                self.assertIn(artifact, listed)
                self.assertEqual(listed[artifact], _sha256(dist / artifact))
            self.assertNotIn("at-pc.exe", listed)
            self.assertFalse((dist / "at-pc.exe").exists())

    def test_version_embedded_in_artifacts(self):
        with self._workspace() as name:
            base = Path(name)
            run = self._run_factory(base)
            with redirect_stdout(io.StringIO()):
                package_dist.main(
                    ["--target", "x86_64-pc-windows-msvc"],
                    run=run,
                    base_dir=str(base),
                    env={},
                )
            dist = base / "dist"
            version_path = dist / "VERSION"
            self.assertTrue(version_path.is_file())
            self.assertEqual(
                version_path.read_text(encoding="utf-8").strip(), METADATA_VERSION
            )
            with zipfile.ZipFile(dist / "at-pc-windows-x86_64.zip") as zf:
                self.assertIn("VERSION", zf.namelist())
                self.assertEqual(
                    zf.read("VERSION").decode("utf-8").strip(), METADATA_VERSION
                )
            listed = {}
            for line in (dist / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
                if not line.strip() or line.startswith("#"):
                    continue
                digest, filename = line.split()
                listed[filename] = digest
            self.assertIn("VERSION", listed)
            self.assertEqual(listed["VERSION"], _sha256(version_path))

    def test_host_build_omits_target_flag(self):
        with self._workspace() as name:
            base = Path(name)
            built = []
            run = self._run_factory(base, built=built)
            with redirect_stdout(io.StringIO()):
                package_dist.main([], run=run, base_dir=str(base), env={})
            self.assertEqual(built, [("cargo", "build", "--release")])
            self.assertFalse((base / "dist" / "at-pc-macos").exists())
            self.assertTrue((base / "dist" / "SHA256SUMS").is_file())

    def test_nonzero_sign_raises_systemexit(self):
        with self._workspace() as name:
            base = Path(name)
            signed = []
            inner = self._run_factory(base, sign_rc={"codesign": 1})

            def run(cmd, **kwargs):
                cmd = list(cmd)
                if cmd and cmd[0] == "codesign":
                    signed.append(cmd)
                return inner(cmd, **kwargs)
            with self.assertRaises(SystemExit) as cm:
                with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                    package_dist.main(
                        ["--target", "aarch64-apple-darwin"],
                        run=run,
                        base_dir=str(base),
                        env={},
                    )
            self.assertEqual(cm.exception.code, 1)
            self.assertTrue(signed)
            self.assertFalse((base / "dist" / "SHA256SUMS").exists())

    def test_pack_uses_metadata_target_directory(self):
        with self._workspace() as name:
            base = Path(name)
            custom = base / "custom-target"
            stale = base / "target" / "x86_64-pc-windows-msvc" / "release"
            _write_bin(stale / "at-pc-agent.exe", b"STALE-AGENT")
            _write_bin(stale / "at-pc-server.exe", b"STALE-SERVER")
            run = self._run_factory(base, target_directory=custom)
            with redirect_stdout(io.StringIO()):
                package_dist.main(
                    ["--target", "x86_64-pc-windows-msvc"],
                    run=run,
                    base_dir=str(base),
                    env={},
                )
            dist = base / "dist"
            self.assertEqual((dist / "at-pc-agent.exe").read_bytes(), b"agent-bin")
            self.assertEqual((dist / "at-pc-server.exe").read_bytes(), b"server-bin")
            self.assertTrue((custom / "x86_64-pc-windows-msvc" / "release" / "at-pc-agent.exe").is_file())

    def test_metadata_nonzero_exits_with_stderr(self):
        def run(cmd, **kwargs):
            if list(cmd)[:2] == ["cargo", "metadata"]:
                return subprocess.CompletedProcess(
                    cmd,
                    2,
                    stdout="",
                    stderr="error: could not find `Cargo.toml`",
                )
            raise AssertionError(f"unexpected command: {cmd}")

        err = io.StringIO()
        with self.assertRaises(SystemExit) as cm:
            with redirect_stderr(err):
                package_dist.resolve_version("/workspace", env={}, run=run)
        self.assertEqual(cm.exception.code, 1)
        text = err.getvalue()
        self.assertIn("cargo", text)
        self.assertIn("metadata", text)
        self.assertIn("could not find `Cargo.toml`", text)

    def test_invalid_metadata_json_exits(self):
        def run(cmd, **kwargs):
            if list(cmd)[:2] == ["cargo", "metadata"]:
                return subprocess.CompletedProcess(cmd, 0, stdout="not-json {", stderr="")
            raise AssertionError(f"unexpected command: {cmd}")

        err = io.StringIO()
        with self.assertRaises(SystemExit) as cm:
            with redirect_stderr(err):
                package_dist.resolve_version("/workspace", env={}, run=run)
        self.assertEqual(cm.exception.code, 1)
        self.assertTrue(err.getvalue().strip())

    def test_missing_binaries_exits_mentioning_path(self):
        with self._workspace() as name:
            base = Path(name)
            run = self._run_factory(base, write_bins=False)
            err = io.StringIO()
            with self.assertRaises(SystemExit) as cm:
                with redirect_stdout(io.StringIO()), redirect_stderr(err):
                    package_dist.main(
                        ["--target", "x86_64-pc-windows-msvc"],
                        run=run,
                        base_dir=str(base),
                        env={},
                    )
            self.assertEqual(cm.exception.code, 1)
            text = err.getvalue()
            self.assertIn("at-pc-agent.exe", text)
            self.assertIn(str(base / "target" / "x86_64-pc-windows-msvc" / "release"), text)


if __name__ == "__main__":
    unittest.main()
