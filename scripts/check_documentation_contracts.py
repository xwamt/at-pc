#!/usr/bin/env python3
"""Check that public documentation and shipped configuration stay truthful.

The check deliberately covers only current public contracts and shipped samples.
Historical plans are not product promises; they remain outside this scope.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
AGENT_CONFIG = ROOT / "agent_config.toml"
SERVER_CONFIG = ROOT / "server_config.example.toml"
ROOT_THRESHOLDS = ROOT / "thresholds.toml"
BENCH_THRESHOLDS = ROOT / "crates" / "benchmarks" / "thresholds.toml"
README = ROOT / "README.md"
RELEASE_NOTES = ROOT / "docs" / "releases" / "v1.0.0.md"
HISTORICAL_CHECKLIST = ROOT / "docs" / "testing" / "2026-09-01-cursor-mcp-e2e-checklist.md"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"

FORBIDDEN_CURRENT_CLAIMS = (
    "指数退避",
    "不可篡改",
    "硬件级",
    "O(1)",
    "0.48 ms",
    "70%+",
    "13x",
    "0.26ms",
    "< 15 µs",
)
REAL_CREDENTIAL = re.compile(r"atpc_(?:sk|dv)_[A-Za-z0-9]{8}_[A-Za-z0-9_-]{20,}")
PRIVATE_IPV4 = re.compile(
    r"(?<![A-Za-z0-9<])(?:10(?:\.\d{1,3}){3}|192\.168(?:\.\d{1,3}){2}|172\.(?:1[6-9]|2\d|3[01])(?:\.\d{1,3}){2})(?![A-Za-z0-9>])"
)


def load_toml(raw: str) -> dict:
    try:
        import tomllib

        return tomllib.loads(raw)
    except ImportError:
        pass
    try:
        import tomli  # type: ignore[import-not-found]

        return tomli.loads(raw)
    except ImportError:
        return load_toml_fallback(raw)


def load_toml_fallback(raw: str) -> dict:
    """Parse the scalar/table subset used by shipped TOML samples and thresholds."""
    result: dict = {}
    current = result
    for raw_line in raw.splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            current = result
            for part in line[1:-1].split("."):
                current = current.setdefault(part, {})
            continue
        if "=" not in line:
            raise ValueError(f"invalid TOML assignment: {raw_line!r}")
        key, value = (part.strip() for part in line.split("=", 1))
        if value in ("true", "false"):
            current[key] = value == "true"
        elif len(value) >= 2 and value[0] == value[-1] == '"':
            current[key] = value[1:-1]
        else:
            try:
                current[key] = float(value) if "." in value else int(value)
            except ValueError as error:
                raise ValueError(f"unsupported TOML value for {key!r}: {value!r}") from error
    return result


def check_agent_config(parsed: dict) -> list[str]:
    errors: list[str] = []
    if parsed.get("enable_computer_use") is not False:
        errors.append("agent sample must set root enable_computer_use = false")
    device = parsed.get("device")
    if isinstance(device, dict) and "enable_computer_use" in device:
        errors.append("agent sample must not nest enable_computer_use under [device]")
    server = parsed.get("server")
    if not isinstance(server, dict) or server.get("url") != "ws://127.0.0.1:9801/ws":
        errors.append("agent sample must use the loopback WebSocket default")
    return errors


def check_server_config(parsed: dict) -> list[str]:
    errors: list[str] = []
    if parsed.get("listen_host") != "127.0.0.1":
        errors.append("server sample must default listen_host to 127.0.0.1")
    if parsed.get("allow_plaintext") is not False:
        errors.append("server sample must default allow_plaintext to false")
    return errors


def check_current_claims(name: str, text: str) -> list[str]:
    return [f"{name} contains unverified current claim: {claim}" for claim in FORBIDDEN_CURRENT_CLAIMS if claim in text]


def check_sensitive_text(name: str, text: str) -> list[str]:
    errors: list[str] = []
    if REAL_CREDENTIAL.search(text):
        errors.append(f"{name} contains a credential-shaped secret")
    if PRIVATE_IPV4.search(text):
        errors.append(f"{name} contains a non-placeholder private IP address")
    return errors


def check_ci_text(text: str) -> list[str]:
    required = (
        "python3 scripts/check_route_security.py",
        "python3 scripts/test_check_route_security.py",
        "python3 scripts/check_documentation_contracts.py",
        "python3 scripts/test_check_documentation_contracts.py",
    )
    return [f"CI is missing {command}" for command in required if command not in text]


def check_files() -> list[str]:
    required_paths = (
        AGENT_CONFIG,
        SERVER_CONFIG,
        ROOT_THRESHOLDS,
        BENCH_THRESHOLDS,
        README,
        RELEASE_NOTES,
        HISTORICAL_CHECKLIST,
        CI_WORKFLOW,
    )
    errors = [f"missing required file: {path}" for path in required_paths if not path.is_file()]
    if errors:
        return errors

    try:
        agent = load_toml(AGENT_CONFIG.read_text(encoding="utf-8"))
        server = load_toml(SERVER_CONFIG.read_text(encoding="utf-8"))
        root_thresholds = load_toml(ROOT_THRESHOLDS.read_text(encoding="utf-8"))
        bench_thresholds = load_toml(BENCH_THRESHOLDS.read_text(encoding="utf-8"))
    except (ValueError, OSError) as exc:
        return [f"invalid TOML sample or threshold file: {exc}"]

    errors.extend(check_agent_config(agent))
    errors.extend(check_server_config(server))
    if root_thresholds != bench_thresholds:
        errors.append("root thresholds.toml and benchmark thresholds.toml have drifted")

    for path in (README, RELEASE_NOTES):
        text = path.read_text(encoding="utf-8")
        errors.extend(check_current_claims(path.name, text))
        errors.extend(check_sensitive_text(path.name, text))

    historical = HISTORICAL_CHECKLIST.read_text(encoding="utf-8")
    if not historical.startswith("> **历史归档：不适用于 v1.x 当前产品行为。**"):
        errors.append("historical v0.2 checklist must carry an archive warning")

    errors.extend(check_ci_text(CI_WORKFLOW.read_text(encoding="utf-8")))
    return errors


def main() -> int:
    errors = check_files()
    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print("OK: public documentation and shipped configuration match M0 contracts")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
