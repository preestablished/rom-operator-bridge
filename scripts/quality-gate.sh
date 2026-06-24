#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BASE_BRANCH="main"

if [[ -f "$ROOT_DIR/.ralph" ]]; then
  while IFS='=' read -r key value; do
    if [[ "$key" == "main_branch" && -n "${value:-}" ]]; then
      BASE_BRANCH="$value"
      break
    fi
  done < "$ROOT_DIR/.ralph"
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

if ! git -C "$ROOT_DIR" rev-parse --verify --quiet "${BASE_BRANCH}^{commit}" >/dev/null; then
  printf 'ERROR: base branch `%s` was not found locally; fetch or check out the configured Ralph main branch before running this gate.\n' "$BASE_BRANCH" >&2
  exit 1
fi

CURRENT_BRANCH=$(git -C "$ROOT_DIR" branch --show-current 2>/dev/null || true)
if [[ "$CURRENT_BRANCH" == "$BASE_BRANCH" ]]; then
  skip_step "currently on base branch ${BASE_BRANCH}; branch diff whitespace check is empty after merge"
else
  run_step "git branch whitespace check against ${BASE_BRANCH}...HEAD" git diff --check "${BASE_BRANCH}...HEAD"
fi

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
