#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT_DIR/target/rom-operator-bridge-release-manifest"

if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
  printf 'build-release: FAIL run builds as the unprivileged operator, not root\n' >&2
  exit 1
fi

need_command() {
  local command_name=$1
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'build-release: FAIL missing required command: %s\n' "$command_name" >&2
    exit 127
  fi
}

need_command cargo
need_command git
need_command npm

if [[ -n "$(git -C "$ROOT_DIR" status --porcelain)" ]]; then
  printf 'build-release: FAIL working tree must be clean before deployment build\n' >&2
  exit 1
fi

commit=$(git -C "$ROOT_DIR" rev-parse HEAD)
branch=$(git -C "$ROOT_DIR" branch --show-current 2>/dev/null || true)
built_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)

printf 'build-release: building commit %.12s\n' "$commit"

(cd "$ROOT_DIR" && cargo build --manifest-path service/Cargo.toml --release)
(cd "$ROOT_DIR" && npm --prefix ui ci)
(cd "$ROOT_DIR" && npm --prefix ui run build)

mkdir -p "$(dirname "$MANIFEST")"
cat >"$MANIFEST" <<EOF
commit=$commit
branch=$branch
built_at=$built_at
service_binary=service/target/release/rom-operator-bridge-service
static_dist=ui/dist
EOF

printf 'build-release: PASS service binary and static UI are ready\n'
printf 'build-release: manifest written to target/rom-operator-bridge-release-manifest\n'
