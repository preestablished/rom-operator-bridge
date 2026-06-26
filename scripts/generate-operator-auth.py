#!/usr/bin/env python3
"""Generate operator secrets into the private env file without printing values."""

from __future__ import annotations

import argparse
import os
import re
import secrets
import sys
from pathlib import Path


DEFAULT_ENV_FILE = Path("/etc/rom-operator-bridge/rom-operator-bridge.env")
ENV_DIR_MODE = 0o755
ENV_FILE_MODE = 0o600

SECRET_SPEC_ITEMS = (
    ("ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL", 48),
    ("ROM_OPERATOR_BRIDGE_SESSION_SECRET", 64),
)
SECRET_SPECS = dict(SECRET_SPEC_ITEMS)
ASSIGN = "="

PLACEHOLDER_VALUES = {
    "",
    "changeme",
    "change-me",
    "placeholder",
    "replace-me",
    "example",
}

KEY_RE = re.compile(r"^[A-Z0-9_]+$")


def parse_assignment(line: str) -> tuple[str, str] | None:
    stripped = line.strip()
    if not stripped or stripped.startswith("#") or "=" not in stripped:
        return None
    if stripped.startswith("export "):
        stripped = stripped.removeprefix("export ").strip()
    key, value = stripped.split("=", 1)
    key = key.strip()
    if not KEY_RE.fullmatch(key):
        return None
    return key, value.strip()


def unquote(value: str) -> str:
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        return value[1:-1]
    return value


def needs_update(value: str, rotate: bool) -> bool:
    if rotate:
        return True
    normalized = unquote(value).strip()
    lowered = normalized.lower()
    return lowered in PLACEHOLDER_VALUES or "<" in normalized or ">" in normalized


def generated_value(byte_count: int) -> str:
    return secrets.token_urlsafe(byte_count)


def write_env_file(path: Path, lines: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.parent.chmod(ENV_DIR_MODE)

    tmp_path = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
    fd = os.open(tmp_path, flags, ENV_FILE_MODE)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write("\n".join(lines).rstrip("\n") + "\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.chown(tmp_path, 0, 0)
        os.chmod(tmp_path, ENV_FILE_MODE)
        os.replace(tmp_path, path)
    except Exception:
        try:
            tmp_path.unlink()
        except FileNotFoundError:
            pass
        raise
    os.chown(path, 0, 0)
    os.chmod(path, ENV_FILE_MODE)


def update_lines(lines: list[str], rotate: bool) -> tuple[list[str], list[str], list[str]]:
    updated: list[str] = []
    present: set[str] = set()
    changed: list[str] = []
    unchanged: list[str] = []

    for line in lines:
        parsed = parse_assignment(line)
        if parsed is None:
            updated.append(line)
            continue

        key, value = parsed
        if key not in SECRET_SPECS:
            updated.append(line)
            continue

        present.add(key)
        if needs_update(value, rotate):
            prefix = "export " if line.strip().startswith("export ") else ""
            updated.append(
                "".join((prefix, key, ASSIGN, generated_value(SECRET_SPECS[key])))
            )
            changed.append(key)
        else:
            updated.append(line)
            unchanged.append(key)

    for key, byte_count in SECRET_SPECS.items():
        if key not in present:
            updated.append("".join((key, ASSIGN, generated_value(byte_count))))
            changed.append(key)

    return updated, changed, unchanged


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Generate missing rom-operator-bridge operator secrets without printing values."
    )
    parser.add_argument(
        "env_file",
        nargs="?",
        default=str(DEFAULT_ENV_FILE),
        help=f"private env file path; default: {DEFAULT_ENV_FILE}",
    )
    parser.add_argument(
        "--rotate",
        action="store_true",
        help="replace existing non-placeholder secrets; this invalidates active sessions",
    )
    args = parser.parse_args(argv)

    path = Path(args.env_file)
    if os.geteuid() != 0:
        print("operator-secret-gen: FAIL must be run with sudo/root", file=sys.stderr)
        return 1

    if path.exists():
        if path.is_symlink():
            print("operator-secret-gen: FAIL env file must not be a symlink", file=sys.stderr)
            return 1
        lines = path.read_text(encoding="utf-8").splitlines()
    else:
        lines = []

    updated, changed, unchanged = update_lines(lines, args.rotate)
    if not changed:
        print("operator-secret-gen: PASS secrets already present; no values printed")
        return 0

    write_env_file(path, updated)
    for key in changed:
        print(f"operator-secret-gen: PASS updated {key}")
    for key in unchanged:
        print(f"operator-secret-gen: PASS preserved {key}")
    print("operator-secret-gen: PASS values were written without printing them")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
