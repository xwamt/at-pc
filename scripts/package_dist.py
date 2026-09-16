#!/usr/bin/env python3
"""Build at-pc from source and assemble dist artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import zipfile

NOISE_FILES = [
    "at-pc-agent.log",
    "at-pc-server.log",
    "audit.jsonl",
    "terminals_meta.json",
    ".DS_Store",
]
STALE_ALIASES = ["at-pc.exe", "at-pc-macos"]
EXTRA_FILES = ["agent_config.toml", "server_config.example.toml", "README.md"]


def sha256_file(filepath):
    h = hashlib.sha256()
    with open(filepath, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()


def _returncode(completed):
    if isinstance(completed, int) or completed is None:
        return completed or 0
    return completed.returncode


def _die(message):
    print(message, file=sys.stderr)
    raise SystemExit(1)


def _die_cmd(cmd, completed):
    rc = _returncode(completed)
    stderr = ""
    if not isinstance(completed, int) and completed is not None:
        stderr = completed.stderr or ""
    printable = cmd if isinstance(cmd, str) else " ".join(str(part) for part in cmd)
    print(f"{printable} exited {rc}", file=sys.stderr)
    if stderr:
        print(stderr, file=sys.stderr, end="" if stderr.endswith("\n") else "\n")
    raise SystemExit(1)


def load_metadata(base_dir, run):
    cmd = ["cargo", "metadata", "--format-version", "1", "--no-deps"]
    completed = run(cmd, cwd=base_dir, capture_output=True, text=True)
    if _returncode(completed) != 0:
        _die_cmd(cmd, completed)
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        _die(f"{' '.join(cmd)}: invalid JSON: {exc}")


def resolve_version(base_dir, *, env=None, run=subprocess.run, metadata=None):
    env = os.environ if env is None else env
    data = metadata if metadata is not None else load_metadata(base_dir, run)
    try:
        packages = data["packages"]
        by_name = {pkg["name"]: pkg["version"] for pkg in packages}
    except (KeyError, TypeError) as exc:
        _die(f"cargo metadata: unexpected schema: {exc}")
    server = by_name.get("at-pc-server")
    agent = by_name.get("at-pc-agent")
    if not server or not agent:
        _die("cargo metadata: missing at-pc-server or at-pc-agent version")
    if server != agent:
        _die(f"cargo metadata: at-pc-server {server} != at-pc-agent {agent}")
    pinned = env.get("CARGO_PKG_VERSION")
    if pinned and pinned != server:
        _die(f"CARGO_PKG_VERSION={pinned} != cargo metadata {server}")
    return server


def write_version_file(dist_dir, version):
    path = os.path.join(dist_dir, "VERSION")
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(f"{version}\n")
    return path


def host_triple(run):
    cmd = ["rustc", "-vV"]
    completed = run(cmd, capture_output=True, text=True)
    if _returncode(completed) != 0:
        _die_cmd(cmd, completed)
    for line in completed.stdout.splitlines():
        if line.startswith("host:"):
            return line.split(":", 1)[1].strip()
    _die("rustc -vV: missing host triple")


def cargo_build(base_dir, triple, *, use_target_flag, run):
    cmd = ["cargo", "build", "--release"]
    if use_target_flag:
        cmd.extend(["--target", triple])
    completed = run(cmd, cwd=base_dir)
    if _returncode(completed) != 0:
        _die_cmd(cmd, completed)


def release_dir(target_root, triple, *, use_target_flag):
    if use_target_flag:
        return os.path.join(target_root, triple, "release")
    return os.path.join(target_root, "release")


def is_windows(triple):
    return "windows" in triple


def is_apple(triple):
    return "apple" in triple or "darwin" in triple


def arch_label(triple):
    cpu = triple.split("-", 1)[0]
    if cpu in ("aarch64", "arm64"):
        return "arm64"
    if cpu in ("x86_64", "amd64"):
        return "x86_64"
    return cpu


def os_label(triple):
    if is_windows(triple):
        return "windows"
    if is_apple(triple):
        return "macos"
    if "linux" in triple:
        return "linux"
    return "unknown"


def dist_binary_names(triple):
    if is_windows(triple):
        return "at-pc-agent.exe", "at-pc-server.exe"
    if is_apple(triple):
        return "at-pc-agent-macos", "at-pc-server-macos"
    return "at-pc-agent", "at-pc-server"


def source_binary_names(triple):
    if is_windows(triple):
        return "at-pc-agent.exe", "at-pc-server.exe"
    return "at-pc-agent", "at-pc-server"


def sign_macos_binary(path, run):
    for cmd in (
        ["xattr", "-cr", path],
        ["codesign", "--force", "--deep", "--sign", "-", path],
    ):
        completed = run(cmd)
        if _returncode(completed) != 0:
            _die_cmd(cmd, completed)


def write_sha256sums(dist_dir, artifacts):
    path = os.path.join(dist_dir, "SHA256SUMS")
    with open(path, "w", encoding="utf-8") as fh:
        for name in artifacts:
            digest = sha256_file(os.path.join(dist_dir, name))
            fh.write(f"{digest}  {name}\n")
    return path


def _copy_extras(base_dir, dist_dir):
    present = []
    for name in EXTRA_FILES:
        dest = os.path.join(dist_dir, name)
        src = os.path.join(base_dir, name)
        if os.path.isfile(src):
            shutil.copy2(src, dest)
        if os.path.isfile(dest):
            present.append(name)
    return present


def _pack_zip(zip_path, dist_dir, members):
    if os.path.exists(zip_path):
        os.remove(zip_path)
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as zf:
        for name in members:
            zf.write(os.path.join(dist_dir, name), arcname=name)


def package_target(base_dir, dist_dir, triple, *, target_root, use_target_flag, extras, run):
    src_dir = release_dir(target_root, triple, use_target_flag=use_target_flag)
    src_agent, src_server = source_binary_names(triple)
    dist_agent, dist_server = dist_binary_names(triple)
    agent_src = os.path.join(src_dir, src_agent)
    server_src = os.path.join(src_dir, src_server)
    missing = [path for path in (agent_src, server_src) if not os.path.isfile(path)]
    if missing:
        _die("missing binary: " + ", ".join(missing))

    agent_dest = os.path.join(dist_dir, dist_agent)
    server_dest = os.path.join(dist_dir, dist_server)
    shutil.copy2(agent_src, agent_dest)
    shutil.copy2(server_src, server_dest)
    if not is_windows(triple):
        os.chmod(agent_dest, 0o755)
        os.chmod(server_dest, 0o755)

    if is_apple(triple):
        sign_macos_binary(agent_dest, run)
        sign_macos_binary(server_dest, run)

    zip_basename = f"at-pc-{os_label(triple)}-{arch_label(triple)}.zip"
    zip_path = os.path.join(dist_dir, zip_basename)
    members = [dist_server, dist_agent, *extras]
    _pack_zip(zip_path, dist_dir, members)
    return [dist_agent, dist_server, zip_basename]


def main(argv=None, *, run=None, base_dir=None, env=None):
    run = run or subprocess.run
    env = os.environ if env is None else env
    if base_dir is None:
        base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

    parser = argparse.ArgumentParser(description="Build and package at-pc dist artifacts")
    parser.add_argument(
        "--target",
        dest="targets",
        action="append",
        help="Rust target triple (repeatable). Omit to build the host triple.",
    )
    args = parser.parse_args(argv)

    dist_dir = os.path.join(base_dir, "dist")
    os.makedirs(dist_dir, exist_ok=True)
    for nf in NOISE_FILES + STALE_ALIASES:
        path = os.path.join(dist_dir, nf)
        if os.path.exists(path):
            os.remove(path)

    data = load_metadata(base_dir, run)
    version = resolve_version(base_dir, env=env, run=run, metadata=data)
    target_root = data.get("target_directory")
    if not target_root:
        _die("cargo metadata: missing target_directory")
    write_version_file(dist_dir, version)
    extras = ["VERSION", *_copy_extras(base_dir, dist_dir)]
    artifacts = ["VERSION"]

    if args.targets:
        jobs = [(triple, True) for triple in args.targets]
    else:
        jobs = [(host_triple(run), False)]

    for triple, use_target_flag in jobs:
        cargo_build(base_dir, triple, use_target_flag=use_target_flag, run=run)
        artifacts.extend(
            package_target(
                base_dir,
                dist_dir,
                triple,
                target_root=target_root,
                use_target_flag=use_target_flag,
                extras=extras,
                run=run,
            )
        )

    write_sha256sums(dist_dir, artifacts)
    print(f"=== at-pc v{version} Packaging Complete ===")
    for art in artifacts:
        path = os.path.join(dist_dir, art)
        size_mb = os.path.getsize(path) / (1024 * 1024)
        print(f"{art:<25} {size_mb:6.2f} MB  SHA256: {sha256_file(path)}")
    print(f"SHA256SUMS written to {os.path.join(dist_dir, 'SHA256SUMS')}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
