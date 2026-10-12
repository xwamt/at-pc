#!/usr/bin/env python3
"""Fail fast when the HTTP route-security baseline is structurally weakened.

This check intentionally complements, rather than replaces, Rust integration
coverage. It protects the small set of security invariants that are easy to
accidentally bypass while adding routes or exemptions: global authentication,
exact public-route allowlisting, handler RBAC, quota headers, audit access, and
static-asset confinement.
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MCP = ROOT / "crates" / "server" / "src" / "mcp" / "mod.rs"
DASHBOARD = ROOT / "crates" / "server" / "src" / "mcp" / "dashboard.rs"
CONFIG = ROOT / "crates" / "server" / "src" / "config.rs"


def between(text: str, start: str, end: str) -> str:
    """Return the source region between two stable implementation markers."""
    begin = text.find(start)
    if begin < 0:
        return ""
    finish = text.find(end, begin + len(start))
    return text[begin:] if finish < 0 else text[begin:finish]


def require(text: str, snippet: str, label: str, errors: list[str]) -> None:
    if snippet not in text:
        errors.append(f"missing {label}")


def check_sources(mcp: str, dashboard: str, config: str) -> list[str]:
    """Return every missing or broadened route-security invariant."""
    errors: list[str] = []

    require(
        mcp,
        ".route_layer(axum::middleware::from_fn_with_state(",
        "global auth route layer",
        errors,
    )
    require(mcp, "auth_layer,", "auth_layer attachment", errors)

    exempt = between(mcp, "fn is_auth_exempt", "/// Global authentication layer")
    if not exempt:
        errors.append("missing exact auth-exemption function")
    else:
        for snippet, label in (
            (
                'path == "/api/auth/login" && *method == axum::http::Method::POST',
                "login-only exemption",
            ),
            ("*method == Method::OPTIONS", "OPTIONS exemption"),
            (
                'matches!(path, "/" | "/dashboard" | "/index.html")',
                "exact bootstrap-route exemption",
            ),
            ('path == "/health" && peer_loopback', "loopback-only health exemption"),
            (
                'path.ends_with("/desktop/ws") && *method == Method::GET',
                "desktop WebSocket ticket exemption",
            ),
        ):
            require(exempt, snippet, label, errors)
        for forbidden in (
            'path.starts_with("/api',
            'path.contains("/api',
            'path.contains("/desktop/',
            'path.starts_with("/desktop',
        ):
            if forbidden in exempt:
                errors.append(f"broad auth exemption is forbidden: {forbidden}")

    require(mcp, "check_and_record(key_id)", "per-principal quota check", errors)
    for header in (
        '"x-ratelimit-limit"',
        '"x-ratelimit-remaining"',
        '"x-ratelimit-reset"',
    ):
        require(mcp, header, f"quota response header {header}", errors)

    for snippet, label in (
        ('if trimmed_path == "/api/logs" && method == "GET"', "logs RBAC route"),
        ('if trimmed_path == "/api/keys"', "keys RBAC route"),
        ('trimmed_path.starts_with("/api/keys/")', "key-detail RBAC route"),
        ('return Some(Role::Admin);', "admin RBAC requirement"),
    ):
        require(config, snippet, label, errors)

    desktop_input = between(
        dashboard,
        "async fn api_send_desktop_input",
        "/// Handler for `POST /api/terminals/:id/desktop/ticket`",
    )
    if not desktop_input:
        errors.append("missing desktop-input handler")
    else:
        require(desktop_input, "if role != Role::Admin", "desktop-input admin gate", errors)
        require(
            desktop_input,
            'action: "api:desktop_input".to_string()',
            "desktop-input denied audit action",
            errors,
        )
        require(desktop_input, 'status: "DENIED".to_string()', "desktop-input denied audit status", errors)

    for snippet, label in (
        ("if role == Role::Viewer", "audit-read viewer denial"),
        ("if role != Role::Admin", "audit-export admin denial"),
        ("Component::Normal", "static asset normal-component guard"),
        ("canonical_asset.starts_with(&canonical_root)", "static asset canonical-root guard"),
    ):
        require(dashboard, snippet, label, errors)

    return errors


def check_files(
    mcp_path: Path = MCP,
    dashboard_path: Path = DASHBOARD,
    config_path: Path = CONFIG,
) -> list[str]:
    missing = [path for path in (mcp_path, dashboard_path, config_path) if not path.is_file()]
    if missing:
        return [f"missing source file: {path}" for path in missing]
    return check_sources(
        mcp_path.read_text(encoding="utf-8"),
        dashboard_path.read_text(encoding="utf-8"),
        config_path.read_text(encoding="utf-8"),
    )


def main() -> int:
    errors = check_files()
    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print("OK: route-security structural invariants are present")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
