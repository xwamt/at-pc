#!/usr/bin/env python3
"""Fail if at-pc-agent and at-pc-server form a Cargo dependency cycle.

Parses the two crate manifests. A cycle exists when agent depends on
at-pc-server and server depends on at-pc-agent (any of dependencies,
dev-dependencies, or build-dependencies, including target-specific tables).
The desired layout is a third crate (e.g. at-pc-e2e-tests) depending on both.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
AGENT_MANIFEST = ROOT / "crates" / "agent" / "Cargo.toml"
SERVER_MANIFEST = ROOT / "crates" / "server" / "Cargo.toml"

AGENT_PKG = "at-pc-agent"
SERVER_PKG = "at-pc-server"
DEP_TABLE_NAMES = frozenset(
    {"dependencies", "dev-dependencies", "build-dependencies"}
)
TABLE_HEADER = re.compile(r"^\[([^\]]+)\]\s*$")
ASSIGN_KEY = re.compile(r"^([A-Za-z0-9_-]+)\s*=")


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
        pass
    return load_toml_dep_tables_fallback(raw)


def _dep_table_kind(header: str) -> str | None:
    name = header.strip()
    if name in DEP_TABLE_NAMES:
        return name
    # [target.'cfg(windows)'.dev-dependencies]
    last = name.rsplit(".", 1)[-1]
    if last in DEP_TABLE_NAMES:
        return last
    return None


def load_toml_dep_tables_fallback(raw: str) -> dict:
    """Collect dependency tables without a TOML library."""
    parsed: dict = {}
    current: str | None = None
    for line in raw.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        header = TABLE_HEADER.match(stripped)
        if header:
            current = _dep_table_kind(header.group(1))
            if current is not None:
                parsed.setdefault(current, {})
            continue
        if current is None:
            continue
        assign = ASSIGN_KEY.match(stripped)
        if assign:
            parsed[current][assign.group(1)] = True
    return parsed


def iter_dep_tables(obj: object, parent_key: str = "") -> list[tuple[str, dict]]:
    tables: list[tuple[str, dict]] = []
    if not isinstance(obj, dict):
        return tables
    kind = parent_key if parent_key in DEP_TABLE_NAMES else None
    if kind is not None:
        tables.append((kind, obj))
        return tables
    for key, value in obj.items():
        if isinstance(value, dict):
            tables.extend(iter_dep_tables(value, key))
    return tables


def package_depends_on(parsed: dict, dep_name: str) -> list[str]:
    kinds: list[str] = []
    seen: set[str] = set()
    for kind, table in iter_dep_tables(parsed):
        if dep_name in table and kind not in seen:
            seen.add(kind)
            kinds.append(kind)
    return kinds


def check_manifest_texts(agent_toml: str, server_toml: str) -> str | None:
    try:
        agent = load_toml(agent_toml)
        server = load_toml(server_toml)
    except (ValueError, OSError) as exc:
        return f"not valid TOML: {exc}"

    agent_on_server = package_depends_on(agent, SERVER_PKG)
    server_on_agent = package_depends_on(server, AGENT_PKG)
    if not agent_on_server and not server_on_agent:
        return None

    parts: list[str] = []
    if agent_on_server:
        parts.append(
            f"{AGENT_PKG} depends on {SERVER_PKG} "
            f"({', '.join(agent_on_server)})"
        )
    if server_on_agent:
        parts.append(
            f"{SERVER_PKG} depends on {AGENT_PKG} "
            f"({', '.join(server_on_agent)})"
        )
    if agent_on_server and server_on_agent:
        return "dev-dependency cycle: " + "; ".join(parts)
    return "forbidden agent/server dependency: " + "; ".join(parts)


def check_manifest_files(
    agent_path: Path = AGENT_MANIFEST,
    server_path: Path = SERVER_MANIFEST,
) -> str | None:
    if not agent_path.is_file():
        return f"missing {agent_path}"
    if not server_path.is_file():
        return f"missing {server_path}"
    return check_manifest_texts(
        agent_path.read_text(encoding="utf-8"),
        server_path.read_text(encoding="utf-8"),
    )


def main() -> int:
    error = check_manifest_files()
    if error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print(
        f"OK: {AGENT_PKG} does not depend on {SERVER_PKG}; "
        f"{SERVER_PKG} does not depend on {AGENT_PKG}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
