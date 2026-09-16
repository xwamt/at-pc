#!/usr/bin/env python3
"""Host vs cross CARGO_TARGET_DIR convention check (no compile).

Parses `.cargo/config.toml` and `.github/workflows/ci.yml` as structured
documents. Comments and `run:` echo strings do not count as rust-cache or
CARGO_TARGET_DIR. Prefers tomllib (Python 3.11+ / CI Ubuntu 24.04); tomli is
optional locally. A 3.9 fallback bans `[env]` / `[build]` tables rather than
subset-parsing arrays.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONFIG = ROOT / ".cargo" / "config.toml"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"

REQUIRED_ALIASES = (
    "build-win-gnu",
    "build-win-msvc",
)
DOC_TARGET_DIRS = (
    "target/mac",
    "target/linux",
    "target/win",
)
ALLOWED_TARGET_DIRS = frozenset(DOC_TARGET_DIRS)
COMPILE_JOBS = ("quality", "platform-check")
CACHE_ACTIONS = frozenset(
    {
        "Swatinem/rust-cache",
        "mozilla-actions/sccache-action",
    }
)
MATRIX_EXPR = re.compile(r"^\$\{\{\s*matrix\.([A-Za-z0-9_]+)\s*\}\}$")
TABLE_HEADER = re.compile(r"^\[([A-Za-z0-9._-]+)\]\s*$")


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
    return load_toml_fallback(raw)


def load_toml_fallback(raw: str) -> dict:
    """No array parsing. Ban `[env]` / `[build]` tables; read `[alias]` strings."""
    parsed: dict = {"alias": {}}
    in_alias = False
    for line in raw.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        header = TABLE_HEADER.match(stripped)
        if header:
            name = header.group(1)
            in_alias = name == "alias"
            if name == "env" or name.startswith("env."):
                parsed.setdefault("env", {})["CARGO_TARGET_DIR"] = "<fallback-bans-[env]>"
            if name == "build" or name.startswith("build."):
                parsed.setdefault("build", {})["target-dir"] = "<fallback-bans-[build]>"
            continue
        if not in_alias:
            continue
        assign = re.fullmatch(r'([A-Za-z0-9_-]+)\s*=\s*"(.*)"', stripped)
        if assign:
            parsed["alias"][assign.group(1)] = assign.group(2).encode("utf-8").decode(
                "unicode_escape"
            )
    return parsed


def _strip_inline_comment(line: str) -> str:
    in_single = in_double = False
    brace = 0
    i = 0
    while i < len(line):
        pair = line[i : i + 2]
        if brace and pair == "}}":
            brace -= 1
            i += 2
            continue
        if pair == "{{":
            brace += 1
            i += 2
            continue
        ch = line[i]
        if brace:
            i += 1
            continue
        if ch == "'" and not in_double:
            in_single = not in_single
        elif ch == '"' and not in_single:
            in_double = not in_double
        elif ch == "#" and not in_single and not in_double:
            return line[:i].rstrip()
        i += 1
    return line.rstrip()


def _yaml_lines(text: str) -> list[tuple[int, str]]:
    lines: list[tuple[int, str]] = []
    for raw in text.splitlines():
        if not raw.strip():
            continue
        indent = len(raw) - len(raw.lstrip(" "))
        content = _strip_inline_comment(raw.lstrip(" "))
        if not content or content.startswith("#"):
            continue
        lines.append((indent, content))
    return lines


def _parse_scalar(text: str) -> object:
    if text in ("true", "True", "yes", "on"):
        return True
    if text in ("false", "False", "no", "off"):
        return False
    if text in ("null", "~"):
        return None
    if len(text) >= 2 and text[0] == text[-1] and text[0] in "'\"":
        return text[1:-1]
    return text


def _parse_multiline(
    lines: list[tuple[int, str]], i: int, parent_indent: int
) -> tuple[str, int]:
    chunk: list[tuple[int, str]] = []
    while i < len(lines) and lines[i][0] > parent_indent:
        chunk.append(lines[i])
        i += 1
    if not chunk:
        return "", i
    base = min(ind for ind, _ in chunk)
    return "\n".join(" " * (ind - base) + text for ind, text in chunk), i


def _parse_map(
    lines: list[tuple[int, str]], i: int, indent: int
) -> tuple[dict, int]:
    result: dict = {}
    while i < len(lines):
        ind, text = lines[i]
        if ind < indent or text.startswith("-"):
            break
        if ind > indent:
            raise ValueError(f"unexpected YAML indent before {text!r}")
        if ":" not in text:
            raise ValueError(f"expected YAML key: {text!r}")
        key, _, value_part = text.partition(":")
        key = key.strip()
        value_part = value_part.strip()
        i += 1
        if value_part in ("|", ">"):
            result[key], i = _parse_multiline(lines, i, indent)
        elif value_part == "":
            if i < len(lines) and lines[i][0] > indent:
                result[key], i = _parse_block(lines, i, indent + 1)
            else:
                result[key] = None
        else:
            result[key] = _parse_scalar(value_part)
    return result, i


def _parse_list(
    lines: list[tuple[int, str]], i: int, indent: int
) -> tuple[list, int]:
    items: list = []
    while i < len(lines):
        ind, text = lines[i]
        if ind != indent or not text.startswith("-"):
            break
        rest = text[1:].strip()
        i += 1
        if rest == "":
            val, i = _parse_block(lines, i, indent + 1)
            items.append(val)
            continue
        if ":" not in rest:
            items.append(_parse_scalar(rest))
            continue
        key, _, value_part = rest.partition(":")
        key = key.strip()
        value_part = value_part.strip()
        item: dict = {}
        if value_part in ("|", ">"):
            item[key], i = _parse_multiline(lines, i, indent)
        elif value_part == "":
            if i < len(lines) and lines[i][0] > indent:
                item[key], i = _parse_block(lines, i, indent + 1)
            else:
                item[key] = None
        else:
            item[key] = _parse_scalar(value_part)
        if i < len(lines) and lines[i][0] > indent and not (
            lines[i][0] == indent
        ):
            extra, i = _parse_map(lines, i, lines[i][0])
            item.update(extra)
        items.append(item)
    return items, i


def _parse_block(
    lines: list[tuple[int, str]], i: int, min_indent: int
) -> tuple[object, int]:
    if i >= len(lines) or lines[i][0] < min_indent:
        return None, i
    ind, text = lines[i]
    if text.startswith("-"):
        return _parse_list(lines, i, ind)
    return _parse_map(lines, i, ind)


def parse_yaml_document(text: str) -> object:
    try:
        import yaml  # type: ignore[import-not-found]

        loaded = yaml.safe_load(text)
        if loaded is None:
            return {}
        return loaded
    except ImportError:
        pass
    lines = _yaml_lines(text)
    if not lines:
        return {}
    value, index = _parse_block(lines, 0, 0)
    if index != len(lines):
        raise ValueError(f"unconsumed YAML starting at {lines[index][1]!r}")
    return value


def _uses_action(uses: str) -> str:
    return uses.split("@", 1)[0].strip()


def _is_compiler_cache_action(uses: str) -> bool:
    name = _uses_action(uses)
    if name in CACHE_ACTIONS:
        return True
    return name.endswith("/sccache-action") or name.endswith("/sccache")


def _matrix_values(job: dict, key: str) -> list[str]:
    strategy = job.get("strategy") or {}
    if not isinstance(strategy, dict):
        return []
    matrix = strategy.get("matrix") or {}
    if not isinstance(matrix, dict):
        return []
    out: list[str] = []
    listed = matrix.get(key)
    if isinstance(listed, list):
        out.extend(str(item) for item in listed)
    include = matrix.get("include") or []
    if isinstance(include, list):
        for row in include:
            if isinstance(row, dict) and key in row:
                out.append(str(row[key]))
    return out


def _job_target_dirs(job: dict) -> tuple[str | None, list[str]]:
    env = job.get("env")
    if not isinstance(env, dict) or "CARGO_TARGET_DIR" not in env:
        return "job env.CARGO_TARGET_DIR is missing", []
    raw = env.get("CARGO_TARGET_DIR")
    if raw is None or str(raw).strip() == "":
        return "job env.CARGO_TARGET_DIR is missing", []
    text = str(raw).strip()
    match = MATRIX_EXPR.fullmatch(text)
    if match:
        vals = _matrix_values(job, match.group(1))
        if not vals:
            return f"env.CARGO_TARGET_DIR matrix.{match.group(1)} has no values", []
        return None, vals
    return None, [text]


def check_compile_jobs_yaml(raw: str) -> str | None:
    try:
        data = parse_yaml_document(raw)
    except (ValueError, TypeError) as exc:
        return f"CI YAML parse error: {exc}"
    if not isinstance(data, dict):
        return "CI YAML root must be a mapping"
    jobs = data.get("jobs")
    if not isinstance(jobs, dict):
        return "CI YAML missing jobs mapping"
    for name in COMPILE_JOBS:
        job = jobs.get(name)
        if not isinstance(job, dict):
            return f"missing compile job {name!r}"
        err, dirs = _job_target_dirs(job)
        if err:
            return f"{name} {err}"
        bad = [item for item in dirs if item not in ALLOWED_TARGET_DIRS]
        if bad:
            return (
                f"{name} env.CARGO_TARGET_DIR must be target/linux|mac|win, got {bad}"
            )
        steps = job.get("steps") or []
        if not isinstance(steps, list):
            return f"{name} steps must be a list"
        has_cache = False
        for step in steps:
            if not isinstance(step, dict):
                continue
            uses = step.get("uses")
            if not uses:
                continue
            uses_s = str(uses)
            if _is_compiler_cache_action(uses_s):
                has_cache = True
            if _uses_action(uses_s) == "actions/cache":
                return (
                    f"{name} still has overlapping actions/cache; "
                    "replace it, do not keep both"
                )
        if not has_cache:
            return (
                f"{name} must use Swatinem/rust-cache "
                "(or mozilla-actions/sccache-action / sccache)"
            )
    return None


def check_cargo_config_text(raw: str) -> str | None:
    comments = "\n".join(
        line for line in raw.splitlines() if line.lstrip().startswith("#")
    )
    missing_docs = [item for item in DOC_TARGET_DIRS if item not in comments]
    if missing_docs:
        return f"missing documented isolation dirs in comments: {missing_docs}"
    try:
        parsed = load_toml(raw)
    except (ValueError, OSError) as exc:
        return f"not valid TOML: {exc}"
    env = parsed.get("env")
    if isinstance(env, dict) and "CARGO_TARGET_DIR" in env:
        return (
            "[env] CARGO_TARGET_DIR would mix host and --target artifacts; "
            "export CARGO_TARGET_DIR per host vs cross instead"
        )
    build = parsed.get("build")
    if isinstance(build, dict) and "target-dir" in build:
        return (
            "[build] target-dir would still mix host and --target artifacts; "
            "use CARGO_TARGET_DIR per host vs cross instead"
        )
    aliases = parsed.get("alias") or {}
    if not isinstance(aliases, dict):
        return "missing [alias] table"
    missing_aliases = [name for name in REQUIRED_ALIASES if name not in aliases]
    if missing_aliases:
        return f"missing cargo aliases: {missing_aliases}"
    return None


def check_ci_workflow() -> str | None:
    if not CI_WORKFLOW.is_file():
        return f"missing {CI_WORKFLOW}"
    return check_compile_jobs_yaml(CI_WORKFLOW.read_text(encoding="utf-8"))


def main() -> int:
    if not CONFIG.is_file():
        print(f"FAIL: missing {CONFIG}", file=sys.stderr)
        return 1

    raw = CONFIG.read_text(encoding="utf-8")
    parsed_error = check_cargo_config_text(raw)
    if parsed_error:
        print(f"FAIL: {CONFIG} {parsed_error}", file=sys.stderr)
        return 1

    ci_error = check_ci_workflow()
    if ci_error:
        print(f"FAIL: {ci_error}", file=sys.stderr)
        return 1

    print(
        f"OK: {CONFIG} documents isolated CARGO_TARGET_DIR; "
        f"{CI_WORKFLOW.name} uses rust-cache/sccache on compile jobs"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
