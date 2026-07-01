#!/usr/bin/env bash
set -euo pipefail

PATH=/usr/sbin:/usr/bin:/sbin:/bin
umask 022

REPO_ROOT=/home/infra-admin/git/preestablished/rom-operator-bridge
SERVICE_ROOT=/opt/rom-operator-bridge
STATIC_ROOT=/var/lib/rom-operator-bridge/static
PRIVATE_ROOT=/var/lib/rom-operator-bridge/private
ENV_DIR=/etc/rom-operator-bridge
ENV_FILE=$ENV_DIR/rom-operator-bridge.env
BACKUP_DIR=$ENV_DIR/backups
SYSTEMD_UNIT=/etc/systemd/system/rom-operator-bridge.service
SERVICE_NAME=rom-operator-bridge.service
SERVICE_USER=rombridge
SERVICE_GROUP=rombridge

die() {
  printf 'install-release: FAIL %s\n' "$1" >&2
  exit 1
}

pass() {
  printf 'install-release: PASS %s\n' "$1"
}

need_command() {
  local command_name=$1
  command -v "$command_name" >/dev/null 2>&1 || die "missing required command: $command_name"
}

require_root() {
  [[ "${EUID:-$(id -u)}" -eq 0 ]] || die "must run as root"
}

require_no_args() {
  [[ "$#" -eq 0 ]] || die "this installer accepts no arguments"
}

require_bootstrap() {
  getent passwd "$SERVICE_USER" >/dev/null || die "missing service user: $SERVICE_USER"
  getent group "$SERVICE_GROUP" >/dev/null || die "missing service group: $SERVICE_GROUP"
  [[ -d "$PRIVATE_ROOT" ]] || die "missing private root: $PRIVATE_ROOT"
  [[ "$(stat -c '%a' "$PRIVATE_ROOT")" == "700" ]] || die "private root must be mode 0700"
  [[ -d "$ENV_DIR" ]] || die "missing env dir: $ENV_DIR"
  [[ -f "$ENV_FILE" && ! -L "$ENV_FILE" ]] || die "missing regular env file: $ENV_FILE"
  [[ "$(stat -c '%a' "$ENV_FILE")" == "600" ]] || die "env file must be mode 0600"
}

require_build_outputs() {
  local service_binary=$REPO_ROOT/service/target/release/rom-operator-bridge-service
  local static_dist=$REPO_ROOT/ui/dist

  [[ -x "$service_binary" ]] || die "missing built service binary; run scripts/build-release.sh first"
  [[ -d "$static_dist" ]] || die "missing UI dist; run scripts/build-release.sh first"
  [[ -f "$static_dist/index.html" ]] || die "UI dist is missing index.html"
  if find "$static_dist" -type l -print -quit | grep -q .; then
    die "UI dist contains symlinks"
  fi
}

resolved_under() {
  local path=$1
  local root=$2
  [[ "$path" == "$root"/* ]]
}

copy_static_dist() {
  local src=$1
  local dst=$2
  local path rel

  while IFS= read -r -d '' path; do
    rel=${path#"$src"}
    install -d -m 0755 "$dst$rel"
  done < <(find "$src" -type d -print0)

  while IFS= read -r -d '' path; do
    rel=${path#"$src"/}
    install -m 0644 "$path" "$dst/$rel"
  done < <(find "$src" -type f -print0)
}

update_static_root_env() {
  local release_id=$1
  local static_release=$2
  local backup=$BACKUP_DIR/rom-operator-bridge.env.$release_id.bak

  install -d -m 0700 "$BACKUP_DIR"
  install -m 0600 "$ENV_FILE" "$backup"

  python3 - "$ENV_FILE" "$static_release" <<'PY'
from __future__ import annotations

import os
import sys
from pathlib import Path

env_file = Path(sys.argv[1])
static_release = Path(sys.argv[2])
target_key = "ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT"
required = {
    "ROM_OPERATOR_BRIDGE_BIND_ADDR",
    "ROM_OPERATOR_BRIDGE_BACKEND",
    "ROM_OPERATOR_BRIDGE_PRIVATE_ROOT",
    "ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT",
    "ROM_OPERATOR_BRIDGE_SESSION_SECRET",
}

def parse_assignment(line: str) -> tuple[str, str] | None:
    stripped = line.strip()
    if not stripped or stripped.startswith("#") or "=" not in stripped:
        return None
    if stripped.startswith("export "):
        stripped = stripped.removeprefix("export ").strip()
    key, value = stripped.split("=", 1)
    key = key.strip()
    if not key or not key.replace("_", "A").isalnum() or not key.upper() == key:
        return None
    return key, value.strip()

lines = env_file.read_text(encoding="utf-8").splitlines()
values: dict[str, str] = {}
updated: list[str] = []
seen_static = False

for line in lines:
    parsed = parse_assignment(line)
    if parsed is None:
        updated.append(line)
        continue
    key, value = parsed
    values[key] = value.strip().strip("'\"")
    if key == target_key:
        updated.append(f"{target_key}={static_release}")
        seen_static = True
    else:
        updated.append(f"{key}={value}")

if not seen_static:
    updated.append(f"{target_key}={static_release}")
values[target_key] = str(static_release)

missing = sorted(key for key in required if not values.get(key, "").strip())
if missing:
    raise SystemExit(f"missing required env keys: {', '.join(missing)}")

if values.get("ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL", "").strip():
    print("install-release: WARN deprecated credential key is present and ignored", file=sys.stderr)

tmp = env_file.with_name(f".{env_file.name}.{os.getpid()}.tmp")
with tmp.open("w", encoding="utf-8") as handle:
    handle.write("\n".join(updated).rstrip("\n") + "\n")
    handle.flush()
    os.fsync(handle.fileno())
os.chmod(tmp, 0o600)
os.chown(tmp, 0, 0)
os.replace(tmp, env_file)
os.chmod(env_file, 0o600)
os.chown(env_file, 0, 0)
PY

  pass "env file updated with resolved static release"
}

install_systemd_unit() {
  local tmp
  tmp=$(mktemp "$ENV_DIR/rom-operator-bridge.service.XXXXXX")
  cat >"$tmp" <<'UNIT'
[Unit]
Description=ROM Operator Bridge
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
User=rombridge
Group=rombridge
EnvironmentFile=/etc/rom-operator-bridge/rom-operator-bridge.env
WorkingDirectory=/opt/rom-operator-bridge/current
ExecStart=/opt/rom-operator-bridge/current/rom-operator-bridge
Restart=on-failure
RestartSec=5s

NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectHome=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
LockPersonality=true
RestrictSUIDSGID=true
SystemCallArchitectures=native
ReadOnlyPaths=/opt/rom-operator-bridge
ReadWritePaths=/var/lib/rom-operator-bridge

[Install]
WantedBy=multi-user.target
UNIT
  install -m 0644 "$tmp" "$SYSTEMD_UNIT"
  rm -f "$tmp"
  pass "systemd unit installed"
}

main() {
  require_root
  require_no_args "$@"
  for command_name in awk cat date find getent grep install ln mktemp python3 readlink rm stat systemctl; do
    need_command "$command_name"
  done

  require_bootstrap
  require_build_outputs

  local release_id service_binary static_dist service_release static_release
  local old_service_release old_static_release manifest commit_label
  release_id=$(date -u +%Y%m%dT%H%M%SZ)
  service_binary=$REPO_ROOT/service/target/release/rom-operator-bridge-service
  static_dist=$REPO_ROOT/ui/dist
  service_release=$SERVICE_ROOT/releases/$release_id
  static_release=$STATIC_ROOT/releases/$release_id
  manifest=$REPO_ROOT/target/rom-operator-bridge-release-manifest
  commit_label=unknown

  if [[ -f "$manifest" ]]; then
    commit_label=$(awk -F= '$1 == "commit" {print substr($2, 1, 12); exit}' "$manifest")
    [[ -n "$commit_label" ]] || commit_label=unknown
  fi

  [[ ! -e "$service_release" ]] || die "service release already exists: $service_release"
  [[ ! -e "$static_release" ]] || die "static release already exists: $static_release"

  old_service_release=$(readlink -f "$SERVICE_ROOT/current" 2>/dev/null || true)
  old_static_release=$(readlink -f "$STATIC_ROOT/current" 2>/dev/null || true)

  install -d -m 0755 "$SERVICE_ROOT/releases" "$STATIC_ROOT/releases"
  install -d -m 0755 "$service_release" "$static_release"
  install -m 0755 "$service_binary" "$service_release/rom-operator-bridge"
  copy_static_dist "$static_dist" "$static_release"
  [[ -f "$static_release/index.html" ]] || die "installed static release is missing index.html"

  if [[ -n "$old_service_release" ]]; then
    resolved_under "$old_service_release" "$SERVICE_ROOT/releases" || die "current service target is outside release root"
    ln -sfn "$old_service_release" "$SERVICE_ROOT/previous"
  fi
  if [[ -n "$old_static_release" ]]; then
    resolved_under "$old_static_release" "$STATIC_ROOT/releases" || die "current static target is outside release root"
    ln -sfn "$old_static_release" "$STATIC_ROOT/previous"
  fi

  ln -sfn "$service_release" "$SERVICE_ROOT/current"
  ln -sfn "$static_release" "$STATIC_ROOT/current"
  update_static_root_env "$release_id" "$static_release"
  install_systemd_unit

  systemctl daemon-reload
  systemctl restart "$SERVICE_NAME"
  systemctl is-active --quiet "$SERVICE_NAME" || die "service is not active after restart"

  [[ "$(readlink -f "$SERVICE_ROOT/current")" == "$service_release" ]] || die "service current symlink mismatch"
  [[ "$(readlink -f "$STATIC_ROOT/current")" == "$static_release" ]] || die "static current symlink mismatch"

  printf 'install-release: PASS deployed release_id=%s commit=%s\n' "$release_id" "$commit_label"
  printf 'install-release: PASS service and static current symlinks updated\n'
  printf 'install-release: PASS %s active\n' "$SERVICE_NAME"
}

main "$@"
