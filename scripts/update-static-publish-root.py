#!/usr/bin/env python3
"""Point the operator env file at the resolved static release directory."""

from __future__ import annotations

import argparse
import os
import re
import stat
import sys
from pathlib import Path


DEFAULT_ENV_FILE = Path("/etc/rom-operator-bridge/rom-operator-bridge.env")
DEFAULT_STATIC_CURRENT = Path("/var/lib/rom-operator-bridge/static/current")
ENV_FILE_MODE = 0o600
ENV_DIR_MODE = 0o755
TARGET_KEY = "ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT"
ASSIGN = "="
KEY_RE = re.compile(r"^[A-Z0-9_]+$")


def parse_assignment(line: str) -> tuple[str, str] | None:
    stripped = line.strip()
    if not stripped or stripped.startswith("#") or ASSIGN not in stripped:
        return None
    if stripped.startswith("export "):
        stripped = stripped.removeprefix("export ").strip()
    key, value = stripped.split(ASSIGN, 1)
    key = key.strip()
    if not KEY_RE.fullmatch(key):
        return None
    return key, value.strip()


def resolve_static_root(static_current: Path) -> Path:
    if not static_current.exists():
        raise RuntimeError("static current path does not exist")
    resolved = static_current.resolve(strict=True)
    if not resolved.is_dir():
        raise RuntimeError("resolved static publish root is not a directory")
    if static.S_ISLNK(resolved.lstat().st_mode):
        raise RuntimeError("resolved static publish root is still a symlink")
    if not (resolved / "index.html").is_file():
        raise RuntimeError("resolved static publish root is missing index.html")
    return resolved


def read_lines(path: Path) -> list[str]:
    if not path.exists():
        return []
    if path.is_symlink():
        raise RuntimeError("env file must not be a symlink")
    return path.read_text(encoding="utf-8").splitlines()


def update_lines(lines: list[str], static_root: Path) -> tuple[list[str], bool]:
    value = str(static_root)
    out: list[str] = []
    seen = False
    changed = False

    for line in lines:
        parsed = parse_assignment(line)
        if parsed is None:
            out.append(line)
            continue

        key, current_value = parsed
        if key != TARGET_KEY:
            out.append(line)
            continue

        prefix = "export " if line.strip().startswith("export ") else ""
        replacement = "".join((prefix, key, ASSIGN, value))
        out.append(replacement)
        seen = True
        changed = changed or current_value.strip().strip("'\"") != value

    if not seen:
        out.append("".join((TARGET_KEY, ASSIGN, value)))
        changed = True

    return out, changed


def write_env_file(path: Path, lines: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.parent.chmod(ENV_DIR_MODE)

    tmp_path = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    fd = os.open(tmp_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, ENV_FILE_MODE)
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


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Update ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT in the private env "
            "file to the real directory behind static/current."
        )
    )
    parser.add_argument(
        "env_file",
        nargs="?",
        default=str(DEFAULT_ENV_FILE),
        help=f"private env file path; default: {DEFAULT_ENV_FILE}",
    )
    parser.add_argument(
        "--static-current",
        default=str(DEFAULT_STATIC_CURRENT),
        help=f"static current path to resolve; default: {DEFAULT_STATIC_CURRENT}",
    )
    args = parser.parse_args(argv)

    if os.geteuid() != 0:
        print("static-root-update: FAIL must be run with sudo/root", file=sys.stderr)
        return 1

    env_file = Path(args.env_file)
    static_current = Path(args.static_current)

    try:
        static_root = resolve_static_root(static_current)
        lines = read_lines(env_file)
        updated, changed = update_lines(lines, static_root)
        write_env_file(env_file, updated)
    except Exception as error:
        print(f"static-root-update: FAIL {error}", file=sys.stderr)
        return 1

    if changed:
        print("static-root-update: PASS env file updated")
    else:
        print("static-root-update: PASS env file already pointed at resolved static root")
    print("static-root-update: PASS values were not printed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
