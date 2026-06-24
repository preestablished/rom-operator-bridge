#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BASE_BRANCH="main"

if [[ -f "$ROOT_DIR/.ralph" ]]; then
  configured_base=$(grep -oP '(?<=^main_branch=).*' "$ROOT_DIR/.ralph" 2>/dev/null | head -1 || true)
  if [[ -n "$configured_base" ]]; then
    BASE_BRANCH="$configured_base"
  fi
fi

run_step() {
  local label=$1
  shift

  printf '\n==> %s\n' "$label"
  (cd "$ROOT_DIR" && "$@")
}

skip_step() {
  printf '\n==> SKIP: %s\n' "$1"
}

need_command() {
  local command_name=$1
  local context=$2

  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'ERROR: %s requires `%s`, but it is not installed or not on PATH.\n' "$context" "$command_name" >&2
    exit 127
  fi
}

printf 'Ralph quality gate\n'
printf 'Repository: %s\n' "$ROOT_DIR"
printf 'Base branch: %s\n' "$BASE_BRANCH"

run_step "git unstaged whitespace check" git diff --check
run_step "git staged whitespace check" git diff --cached --check
run_step "git branch whitespace check against ${BASE_BRANCH}...HEAD" git diff --check "${BASE_BRANCH}...HEAD"
run_step "git commit whitespace/stat check for HEAD" git show --check --stat --oneline HEAD

if [[ -f "$ROOT_DIR/service/Cargo.toml" ]]; then
  need_command cargo "service quality gate"
  run_step "service format check" cargo fmt --manifest-path service/Cargo.toml -- --check
  run_step "service tests" cargo test --manifest-path service/Cargo.toml --all-targets
else
  skip_step "service/Cargo.toml not present; service scaffold gate is not available yet"
fi

if [[ -f "$ROOT_DIR/ui/package.json" ]]; then
  need_command npm "UI quality gate"
  if [[ ! -f "$ROOT_DIR/ui/package-lock.json" ]]; then
    printf 'ERROR: ui/package.json exists but ui/package-lock.json is missing; `npm --prefix ui ci` cannot run reproducibly.\n' >&2
    exit 1
  fi

  run_step "UI dependency sync" npm --prefix ui ci
  run_step "UI typecheck" npm --prefix ui run typecheck
  run_step "UI tests" npm --prefix ui test -- --run
  run_step "UI static build" npm --prefix ui run build
else
  skip_step "ui/package.json not present; UI scaffold gate is not available yet"
fi

if [[ -f "$ROOT_DIR/scripts/redaction-gate.sh" ]]; then
  need_command bash "static redaction quality gate"
  run_step "static redaction gate" bash scripts/redaction-gate.sh
else
  skip_step "scripts/redaction-gate.sh not present; static redaction gate is deferred to bead rom-operator-bridge-25u"
fi

printf '\nQuality gate passed.\n'
