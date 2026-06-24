#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
REFWORK_CHECKOUT=${BRIDGE_REFERENCE_WORKLOAD_CHECKOUT:-${ROM_OPERATOR_BRIDGE_REFERENCE_WORKLOAD:-/home/infra-admin/git/preestablished/reference-workload}}
PRIVATE_FORBID_FILE=${ROM_OPERATOR_BRIDGE_FORBID_FILE:-${BRIDGE_FORBIDDEN_LITERALS_FILE:-}}
TMP_DIR=$(mktemp -d)

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

need_command() {
  local command_name=$1
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'ERROR: redaction gate requires `%s` on PATH.\n' "$command_name" >&2
    exit 127
  fi
}

need_command cargo
need_command node
need_command npm

if [[ ! -d "$REFWORK_CHECKOUT" ]]; then
  printf 'ERROR: reference workload checkout not found: %s\n' "$REFWORK_CHECKOUT" >&2
  printf 'Set BRIDGE_REFERENCE_WORKLOAD_CHECKOUT or ROM_OPERATOR_BRIDGE_REFERENCE_WORKLOAD.\n' >&2
  exit 1
fi

if [[ -n "$PRIVATE_FORBID_FILE" && ! -f "$PRIVATE_FORBID_FILE" ]]; then
  printf 'ERROR: forbidden-literals file not found: %s\n' "$PRIVATE_FORBID_FILE" >&2
  exit 1
fi

VALIDATION_DIR=${ROM_OPERATOR_BRIDGE_VALIDATION_DIR:-}
if [[ -z "$VALIDATION_DIR" && -n "${ROM_OPERATOR_BRIDGE_PRIVATE_ROOT:-}" ]]; then
  VALIDATION_DIR="$ROM_OPERATOR_BRIDGE_PRIVATE_ROOT/validation"
fi
if [[ -z "$VALIDATION_DIR" ]]; then
  VALIDATION_DIR="$TMP_DIR/validation"
fi

AGGREGATE_INPUT="$TMP_DIR/static-redaction-input.txt"
PATTERN_SUMMARY="$VALIDATION_DIR/static-redaction-patterns.json"
REDACTION_REPORT="$VALIDATION_DIR/redaction-scan.json"
DEFAULT_FORBID_FILE="$TMP_DIR/default-forbidden-literals.txt"

mkdir -p "$VALIDATION_DIR"
cat >"$DEFAULT_FORBID_FILE" <<'EOF'
operator-secret
private-lab-root-token
/srv/corpus/private
/mnt/private
/Volumes/private
/run/rom
10.0.0.106
192.168.
172.16.
172.17.
172.18.
172.19.
172.20.
172.21.
172.22.
172.23.
172.24.
172.25.
172.26.
172.27.
172.28.
172.29.
172.30.
172.31.
fd00:
fe80:
EOF

printf 'redaction-gate: helper self-test\n'
node "$ROOT_DIR/scripts/redaction-gate.mjs" self-test

printf 'redaction-gate: building static UI output\n'
(cd "$ROOT_DIR" && npm --prefix ui run build >/dev/null)

printf 'redaction-gate: scanning static UI/docs output\n'
set +e
node "$ROOT_DIR/scripts/redaction-gate.mjs" scan \
  --root "$ROOT_DIR" \
  --aggregate "$AGGREGATE_INPUT" \
  --summary "$PATTERN_SUMMARY"
pattern_status=$?
set -e

refwork_args=(
  cargo run --locked -p refwork-verify -- redaction-scan
  --input "$AGGREGATE_INPUT"
  --report "$REDACTION_REPORT"
  --forbid-file "$DEFAULT_FORBID_FILE"
)
if [[ -n "$PRIVATE_FORBID_FILE" ]]; then
  refwork_args+=(--forbid-file "$PRIVATE_FORBID_FILE")
fi

set +e
(cd "$REFWORK_CHECKOUT" && "${refwork_args[@]}")
refwork_status=$?
set -e

if [[ $pattern_status -ne 0 || $refwork_status -ne 0 ]]; then
  printf 'redaction-gate: FAIL — sanitized reports written under %s\n' "$VALIDATION_DIR" >&2
  if [[ -f "$PATTERN_SUMMARY" ]]; then
    node -e "const report=require(process.argv[1]); console.error('redaction-gate: pattern findings=' + report.finding_count + ' kinds=' + Object.keys(report.counts_by_kind).join(','));" "$PATTERN_SUMMARY"
  fi
  exit 1
fi

printf 'redaction-gate: PASS — scanned static UI/docs output; reports under %s\n' "$VALIDATION_DIR"
