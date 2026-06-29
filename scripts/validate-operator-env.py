#!/usr/bin/env python3
"""Validate the private operator env file without printing secret values."""

from __future__ import annotations

import argparse
import grp
import ipaddress
import os
import pwd
import re
import stat
import sys
from pathlib import Path


DEFAULT_ENV_FILE = Path("/etc/rom-operator-bridge/rom-operator-bridge.env")
PRIVATE_DIR_MODE = 0o700
PRIVATE_FILE_MODE = 0o600
PRIVATE_ROOT_MARKER = ".rom-operator-bridge-private-root"
SERVICE_USER = "rombridge"
SERVICE_GROUP = "rombridge"

REQUIRED_KEYS = (
    "ROM_OPERATOR_BRIDGE_BIND_ADDR",
    "ROM_OPERATOR_BRIDGE_BACKEND",
    "ROM_OPERATOR_BRIDGE_PRIVATE_ROOT",
    "ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT",
    "ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL",
    "ROM_OPERATOR_BRIDGE_SESSION_SECRET",
)

REAL_REQUIRED_KEYS = (
    "BRIDGE_WORKLOAD_IMAGE_REF",
    "BRIDGE_CAPTURE_SPEC_REF",
    "BRIDGE_REFERENCE_WORKLOAD_CHECKOUT",
)

PLACEHOLDER_VALUES = {
    "changeme",
    "change-me",
    "placeholder",
    "replace-me",
    "example",
}

KEY_RE = re.compile(r"^[A-Z0-9_]+$")


class Reporter:
    def __init__(self) -> None:
        self.failures = 0
        self.warnings = 0

    def pass_(self, label: str) -> None:
        print(f"operator-env-check: PASS {label}")

    def warn(self, label: str, detail: str | None = None) -> None:
        self.warnings += 1
        if detail:
            print(f"operator-env-check: WARN {label}: {detail}", file=sys.stderr)
        else:
            print(f"operator-env-check: WARN {label}", file=sys.stderr)

    def fail(self, label: str, detail: str | None = None) -> None:
        self.failures += 1
        if detail:
            print(f"operator-env-check: FAIL {label}: {detail}", file=sys.stderr)
        else:
            print(f"operator-env-check: FAIL {label}", file=sys.stderr)


def mode_of(path: Path) -> int:
    return stat.S_IMODE(path.stat().st_mode)


def path_component_count(path: Path) -> int:
    return sum(1 for part in path.parts if part not in (path.anchor, os.sep))


def same_or_under(path: Path, parent: Path) -> bool:
    return path == parent or parent in path.parents


def user_ids(reporter: Reporter) -> tuple[int | None, int | None]:
    uid = None
    gid = None
    try:
        uid = pwd.getpwnam(SERVICE_USER).pw_uid
        reporter.pass_("service_user_exists")
    except KeyError:
        reporter.fail("service_user_exists", f"missing {SERVICE_USER}")
    try:
        gid = grp.getgrnam(SERVICE_GROUP).gr_gid
        reporter.pass_("service_group_exists")
    except KeyError:
        reporter.fail("service_group_exists", f"missing {SERVICE_GROUP}")
    return uid, gid


def unquote_env_value(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in ("'", '"'):
        return value[1:-1]
    return value


def parse_env_file(path: Path, reporter: Reporter) -> dict[str, str]:
    values: dict[str, str] = {}
    try:
        contents = path.read_text(encoding="utf-8")
    except PermissionError:
        reporter.fail("env_file_readable", "rerun with sudo")
        return values
    except OSError as error:
        reporter.fail("env_file_readable", error.__class__.__name__)
        return values

    for line_number, raw_line in enumerate(contents.splitlines(), start=1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("export "):
            line = line.removeprefix("export ").strip()
        if "=" not in line:
            reporter.fail("env_file_line_syntax", f"line {line_number}")
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        if not key or not KEY_RE.fullmatch(key):
            reporter.fail("env_file_key_syntax", f"line {line_number}")
            continue
        if key in values:
            reporter.warn("duplicate_key", key)
        values[key] = unquote_env_value(value)

    if reporter.failures == 0:
        reporter.pass_("env_file_parse")
    return values


def validate_env_file_metadata(path: Path, reporter: Reporter) -> None:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        reporter.fail("env_file_exists")
        return
    except OSError as error:
        reporter.fail("env_file_stat", error.__class__.__name__)
        return

    if stat.S_ISLNK(metadata.st_mode):
        reporter.fail("env_file_not_symlink")
        return
    reporter.pass_("env_file_not_symlink")

    if not stat.S_ISREG(metadata.st_mode):
        reporter.fail("env_file_regular")
        return
    reporter.pass_("env_file_regular")

    mode = stat.S_IMODE(metadata.st_mode)
    if mode != PRIVATE_FILE_MODE:
        reporter.fail("env_file_mode_0600", f"mode {mode:04o}")
    else:
        reporter.pass_("env_file_mode_0600")

    if metadata.st_uid != 0:
        reporter.warn("env_file_owner_root", "recommended owner is root")
    else:
        reporter.pass_("env_file_owner_root")


def require_nonempty(values: dict[str, str], key: str, reporter: Reporter) -> str | None:
    value = values.get(key)
    if value is None or value.strip() == "":
        reporter.fail("missing_key", key)
        return None
    reporter.pass_(f"key_present {key}")
    return value.strip()


def reject_placeholder(values: dict[str, str], key: str, reporter: Reporter) -> None:
    value = values.get(key, "").strip().lower()
    if value in PLACEHOLDER_VALUES:
        reporter.fail("placeholder_value", key)


def parse_absolute_path(key: str, value: str | None, reporter: Reporter) -> Path | None:
    if value is None:
        return None
    path = Path(value.strip())
    if not path.is_absolute():
        reporter.fail("path_absolute", key)
        return None
    if ".." in path.parts:
        reporter.fail("path_no_parent_components", key)
        return None
    reporter.pass_(f"path_absolute {key}")
    return path


def reject_existing_symlink_components(path: Path, key: str, reporter: Reporter) -> None:
    current = Path(path.anchor)
    for part in path.parts[1:]:
        current = current / part
        try:
            metadata = current.lstat()
        except FileNotFoundError:
            return
        except OSError as error:
            reporter.fail("path_component_stat", f"{key}: {error.__class__.__name__}")
            return
        if stat.S_ISLNK(metadata.st_mode):
            reporter.fail("path_no_symlink_components", key)
            return
        if current != path and not stat.S_ISDIR(metadata.st_mode):
            reporter.fail("path_parent_directory", key)
            return
    reporter.pass_(f"path_no_symlink_components {key}")


def validate_bind_addr(values: dict[str, str], reporter: Reporter) -> None:
    value = require_nonempty(values, "ROM_OPERATOR_BRIDGE_BIND_ADDR", reporter)
    if value is None:
        return
    host: str
    port_text: str
    if value.startswith("["):
        match = re.fullmatch(r"\[([^\]]+)\]:(\d+)", value)
        if not match:
            reporter.fail("bind_addr_parse")
            return
        host, port_text = match.group(1), match.group(2)
    elif ":" in value:
        host, port_text = value.rsplit(":", 1)
    else:
        reporter.fail("bind_addr_parse")
        return

    try:
        port = int(port_text, 10)
    except ValueError:
        reporter.fail("bind_addr_port")
        return
    if port != 7410:
        reporter.fail("bind_addr_port_7410")
    else:
        reporter.pass_("bind_addr_port_7410")

    try:
        ip = ipaddress.ip_address(host)
    except ValueError:
        reporter.fail("bind_addr_ip_literal")
        return

    if ip.is_unspecified:
        reporter.fail("bind_addr_not_wildcard")
    else:
        reporter.pass_("bind_addr_not_wildcard")
    if ip.is_loopback:
        reporter.fail("bind_addr_not_loopback")
    else:
        reporter.pass_("bind_addr_not_loopback")
    if not (ip.is_private or ip.is_link_local):
        reporter.warn("bind_addr_private_or_link_local")


def validate_private_root(
    root: Path | None,
    static_root: Path | None,
    service_uid: int | None,
    service_gid: int | None,
    reporter: Reporter,
) -> None:
    if root is None:
        return
    if path_component_count(root) < 3:
        reporter.fail("private_root_not_broad")
    else:
        reporter.pass_("private_root_not_broad")

    if len(root.parts) > 1 and root.parts[1] in {"home", "root"}:
        reporter.fail(
            "private_root_service_accessible",
            "avoid home directories with ProtectHome=true",
        )
    else:
        reporter.pass_("private_root_service_accessible")

    reject_existing_symlink_components(root, "ROM_OPERATOR_BRIDGE_PRIVATE_ROOT", reporter)

    if static_root is not None:
        if same_or_under(root, static_root):
            reporter.fail("private_root_not_inside_static")
        else:
            reporter.pass_("private_root_not_inside_static")
        if same_or_under(static_root, root):
            reporter.fail("static_root_not_inside_private")
        else:
            reporter.pass_("static_root_not_inside_private")

    try:
        root_metadata = root.stat()
    except FileNotFoundError:
        reporter.warn("private_root_exists", "create before starting service")
        return
    except PermissionError:
        reporter.fail("private_root_stat", "rerun with sudo")
        return
    except OSError as error:
        reporter.fail("private_root_stat", error.__class__.__name__)
        return

    if not stat.S_ISDIR(root_metadata.st_mode):
        reporter.fail("private_root_directory")
        return
    reporter.pass_("private_root_directory")

    mode = stat.S_IMODE(root_metadata.st_mode)
    if mode != PRIVATE_DIR_MODE:
        reporter.fail("private_root_mode_0700", f"mode {mode:04o}")
    else:
        reporter.pass_("private_root_mode_0700")

    if service_uid is not None:
        if root_metadata.st_uid != service_uid:
            reporter.fail("private_root_owner_rombridge")
        else:
            reporter.pass_("private_root_owner_rombridge")
    if service_gid is not None:
        if root_metadata.st_gid != service_gid:
            reporter.warn("private_root_group_rombridge")
        else:
            reporter.pass_("private_root_group_rombridge")

    try:
        entries = [entry.name for entry in os.scandir(root)]
    except PermissionError:
        reporter.fail("private_root_readable", "rerun with sudo")
        return
    except OSError as error:
        reporter.fail("private_root_readable", error.__class__.__name__)
        return
    marker = root / PRIVATE_ROOT_MARKER
    if entries and not marker.exists():
        reporter.fail("private_root_empty_or_marked")
    else:
        reporter.pass_("private_root_empty_or_marked")

    if marker.exists():
        if not marker.is_file():
            reporter.fail("private_root_marker_file")
        else:
            reporter.pass_("private_root_marker_file")
        marker_mode = mode_of(marker)
        if marker_mode != PRIVATE_FILE_MODE:
            reporter.fail("private_root_marker_mode_0600", f"mode {marker_mode:04o}")
        else:
            reporter.pass_("private_root_marker_mode_0600")


def validate_static_root(static_root: Path | None, reporter: Reporter) -> None:
    if static_root is None:
        return
    reject_existing_symlink_components(static_root, "ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT", reporter)
    if not static_root.exists():
        reporter.warn("static_publish_root_exists", "step 2 may create it")
        return
    if not static_root.is_dir():
        reporter.fail("static_publish_root_directory")
        return
    reporter.pass_("static_publish_root_directory")
    if not (static_root / "index.html").is_file():
        reporter.warn("static_publish_root_index_html")
    else:
        reporter.pass_("static_publish_root_index_html")


def validate_backend(values: dict[str, str], reporter: Reporter) -> None:
    backend = require_nonempty(values, "ROM_OPERATOR_BRIDGE_BACKEND", reporter)
    if backend is None:
        return
    if backend not in {"synthetic", "real"}:
        reporter.fail("backend_value", "expected synthetic or real")
        return
    reporter.pass_("backend_value")

    if backend != "real":
        return

    for key in REAL_REQUIRED_KEYS:
        require_nonempty(values, key, reporter)
        reject_placeholder(values, key, reporter)

    checkout = parse_absolute_path(
        "BRIDGE_REFERENCE_WORKLOAD_CHECKOUT",
        values.get("BRIDGE_REFERENCE_WORKLOAD_CHECKOUT"),
        reporter,
    )
    if checkout is not None:
        reject_existing_symlink_components(checkout, "BRIDGE_REFERENCE_WORKLOAD_CHECKOUT", reporter)
        if not checkout.exists():
            reporter.warn("reference_workload_checkout_exists")
        elif not checkout.is_dir():
            reporter.fail("reference_workload_checkout_directory")
        else:
            reporter.pass_("reference_workload_checkout_directory")

    snapshot = values.get("BRIDGE_REAL_SNAPSHOT_REF", "").strip()
    create_vm = values.get("BRIDGE_CREATE_VM_CONFIG_REF", "").strip()
    if bool(snapshot) == bool(create_vm):
        reporter.fail("real_start_source_exactly_one", "set snapshot or create-vm config")
    else:
        reporter.pass_("real_start_source_exactly_one")
    for key in ("BRIDGE_REAL_SNAPSHOT_REF", "BRIDGE_CREATE_VM_CONFIG_REF"):
        if values.get(key, "").strip():
            reject_placeholder(values, key, reporter)

    endpoint = values.get("BRIDGE_HYPERVISOR_ENDPOINT", "unix:///run/dh/grpc.sock").strip()
    if endpoint.startswith("unix://"):
        unix_path = parse_absolute_path(
            "BRIDGE_HYPERVISOR_ENDPOINT",
            endpoint.removeprefix("unix://"),
            reporter,
        )
        if unix_path is not None:
            reject_existing_symlink_components(unix_path, "BRIDGE_HYPERVISOR_ENDPOINT", reporter)
            if not unix_path.exists():
                reporter.warn("hypervisor_unix_socket_exists")
            elif not stat.S_ISSOCK(unix_path.stat().st_mode):
                reporter.fail("hypervisor_unix_socket_type")
            else:
                reporter.pass_("hypervisor_unix_socket_type")
    elif endpoint.startswith(("http://", "https://")):
        reject_placeholder({"BRIDGE_HYPERVISOR_ENDPOINT": endpoint}, "BRIDGE_HYPERVISOR_ENDPOINT", reporter)
        reporter.pass_("hypervisor_endpoint_scheme")
    else:
        reporter.fail("hypervisor_endpoint_scheme", "expected unix://, http://, or https://")


def validate_values(values: dict[str, str], reporter: Reporter) -> None:
    for key in REQUIRED_KEYS:
        require_nonempty(values, key, reporter)
    for key in ("ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL", "ROM_OPERATOR_BRIDGE_SESSION_SECRET"):
        reject_placeholder(values, key, reporter)

    validate_bind_addr(values, reporter)

    private_root = parse_absolute_path(
        "ROM_OPERATOR_BRIDGE_PRIVATE_ROOT",
        values.get("ROM_OPERATOR_BRIDGE_PRIVATE_ROOT"),
        reporter,
    )
    static_root = parse_absolute_path(
        "ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT",
        values.get("ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT"),
        reporter,
    )
    service_uid, service_gid = user_ids(reporter)
    validate_private_root(private_root, static_root, service_uid, service_gid, reporter)
    validate_static_root(static_root, reporter)
    validate_backend(values, reporter)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Validate rom-operator-bridge private env shape without printing values."
    )
    parser.add_argument(
        "env_file",
        nargs="?",
        default=str(DEFAULT_ENV_FILE),
        help=f"private env file path; default: {DEFAULT_ENV_FILE}",
    )
    parser.add_argument(
        "--strict-warnings",
        action="store_true",
        help="exit nonzero when warnings are present",
    )
    args = parser.parse_args(argv)

    reporter = Reporter()
    env_file = Path(args.env_file)
    validate_env_file_metadata(env_file, reporter)
    values = parse_env_file(env_file, reporter)
    if values:
        validate_values(values, reporter)

    if reporter.failures:
        print(
            f"operator-env-check: FAIL summary failures={reporter.failures} warnings={reporter.warnings}",
            file=sys.stderr,
        )
        return 1
    if args.strict_warnings and reporter.warnings:
        print(
            f"operator-env-check: FAIL summary failures=0 warnings={reporter.warnings}",
            file=sys.stderr,
        )
        return 1
    print(f"operator-env-check: PASS summary warnings={reporter.warnings}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
