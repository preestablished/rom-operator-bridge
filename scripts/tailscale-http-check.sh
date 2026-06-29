#!/usr/bin/env bash
set -euo pipefail
umask 077

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)

HOSTNAME=${ROM_BRIDGE_TAILSCALE_HOST:-tailrombridge.birb.homes}
ORIGIN=${ROM_BRIDGE_TAILSCALE_ORIGIN:-http://$HOSTNAME}
BASE_URL=${ROM_BRIDGE_TAILSCALE_BASE_URL:-http://$HOSTNAME}
VALIDATION_DIR=${ROM_BRIDGE_TAILSCALE_VALIDATION_DIR:-}
RESOLVE_IP=${ROM_BRIDGE_TAILSCALE_RESOLVE_IP:-}
COOKIE_FILE=${ROM_BRIDGE_TAILSCALE_SESSION_COOKIE_FILE:-${ROM_BRIDGE_TAILSCALE_COOKIE_FILE:-}}
COOKIE_CURL_CONFIG_FILE=${ROM_BRIDGE_TAILSCALE_COOKIE_CURL_CONFIG_FILE:-}
WRONG_HOST=${ROM_BRIDGE_TAILSCALE_WRONG_HOST:-not-tailrombridge.invalid}
NETWORK_EVIDENCE_FILE=${ROM_BRIDGE_TAILSCALE_NETWORK_EVIDENCE_FILE:-}
NETWORK_EVIDENCE_REVIEWED=${ROM_BRIDGE_TAILSCALE_NETWORK_EVIDENCE_REVIEWED:-}
OUTSIDE_PROBE_FILE=${ROM_BRIDGE_TAILSCALE_OUTSIDE_PROBE_RESULT_FILE:-}
OUTSIDE_PROBE_REVIEWED=${ROM_BRIDGE_TAILSCALE_OUTSIDE_PROBE_REVIEWED:-}
FORBID_FILE=${ROM_BRIDGE_TAILSCALE_FORBID_FILE:-${ROM_BRIDGE_FORBID_FILE:-}}

FAILURES=0

usage() {
  cat >&2 <<'EOF'
Usage: ROM_BRIDGE_TAILSCALE_VALIDATION_DIR=/private/dir \
       ROM_BRIDGE_TAILSCALE_SESSION_COOKIE_FILE=/private/cookie.jar \
       scripts/tailscale-http-check.sh

Required:
  ROM_BRIDGE_TAILSCALE_VALIDATION_DIR       private raw evidence directory outside repo
  ROM_BRIDGE_TAILSCALE_SESSION_COOKIE_FILE  0600 cookie jar/file for a throwaway session,
                                            or set ROM_BRIDGE_TAILSCALE_COOKIE_CURL_CONFIG_FILE

Optional:
  ROM_BRIDGE_TAILSCALE_HOST                 default: tailrombridge.birb.homes
  ROM_BRIDGE_TAILSCALE_ORIGIN               default: http://tailrombridge.birb.homes
  ROM_BRIDGE_TAILSCALE_BASE_URL             default: http://tailrombridge.birb.homes
  ROM_BRIDGE_TAILSCALE_RESOLVE_IP           private IP for curl --resolve, not for commits
  ROM_BRIDGE_TAILSCALE_NETWORK_EVIDENCE_FILE  private listener/firewall/ACL evidence file;
                                              requires ROM_BRIDGE_TAILSCALE_NETWORK_EVIDENCE_REVIEWED=1
  ROM_BRIDGE_TAILSCALE_OUTSIDE_PROBE_RESULT_FILE private outside-network probe result file;
                                                 requires ROM_BRIDGE_TAILSCALE_OUTSIDE_PROBE_REVIEWED=1
  ROM_BRIDGE_TAILSCALE_WRONG_HOST           default: not-tailrombridge.invalid
  ROM_BRIDGE_TAILSCALE_FORBID_FILE          private forbidden-literals file
EOF
}

pass() {
  printf 'tailscale-http-check: PASS %s\n' "$1"
}

fail() {
  printf 'tailscale-http-check: FAIL %s\n' "$1" >&2
  FAILURES=$((FAILURES + 1))
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'tailscale-http-check: ERROR missing command %s\n' "$1" >&2
    exit 127
  fi
}

contains_placeholder() {
  [[ "$1" == *"<"* || "$1" == *">"* ]]
}

ensure_private_path() {
  local path=$1
  local root_real path_real
  mkdir -p "$path"
  chmod 700 "$path"
  root_real=$(cd "$ROOT_DIR" && pwd -P)
  path_real=$(cd "$path" && pwd -P)
  if [[ "$path_real" == "$root_real" || "$path_real" == "$root_real"/* ]]; then
    printf 'tailscale-http-check: ERROR validation directory must be outside repository\n' >&2
    exit 2
  fi
}

check_private_file() {
  local path=$1
  local label=$2
  if [[ -z "$path" || ! -f "$path" || -L "$path" || "$(stat -c '%a' "$path" 2>/dev/null || true)" != "600" ]]; then
    fail "$label"
    return 1
  fi
  local root_real path_dir_real path_real
  root_real=$(cd "$ROOT_DIR" && pwd -P)
  path_dir_real=$(cd "$(dirname "$path")" && pwd -P) || {
    fail "$label"
    return 1
  }
  path_real="$path_dir_real/${path##*/}"
  if [[ "$path_real" == "$root_real" || "$path_real" == "$root_real"/* ]]; then
    fail "$label"
    return 1
  fi
  pass "$label"
}

reviewed_private_file() {
  local path=$1
  local reviewed=$2
  local label=$3
  if check_private_file "$path" "$label" && [[ "$reviewed" == "1" ]]; then
    return 0
  fi
  if [[ "$reviewed" != "1" ]]; then
    fail "${label}_operator_reviewed"
  fi
  return 1
}

curl_args_common=()
if [[ -n "$RESOLVE_IP" ]]; then
  curl_args_common+=(--resolve "$HOSTNAME:80:$RESOLVE_IP")
fi

curl_cookie_args=()
if [[ -n "$COOKIE_CURL_CONFIG_FILE" ]]; then
  curl_cookie_args+=(--config "$COOKIE_CURL_CONFIG_FILE")
elif [[ -n "$COOKIE_FILE" ]]; then
  curl_cookie_args+=(--cookie "$COOKIE_FILE")
fi

http_probe() {
  local label=$1
  local expected=$2
  shift 2
  local headers="$VALIDATION_DIR/${label}.headers"
  local body="$VALIDATION_DIR/${label}.body"
  local stderr="$VALIDATION_DIR/${label}.stderr"
  local code status
  set +e
  code=$(curl -sS --max-time 15 "${curl_args_common[@]}" \
    -D "$headers" \
    -o "$body" \
    -w '%{http_code}' \
    "$@" 2>"$stderr")
  status=$?
  set -e
  printf '%s\n' "$code" >"$VALIDATION_DIR/${label}.status"
  if [[ $status -ne 0 ]]; then
    fail "$label"
    return 1
  fi
  case " $expected " in
    *" $code "*) return 0 ;;
    *) fail "$label"; return 1 ;;
  esac
}

require_no_store() {
  local label=$1
  local headers="$VALIDATION_DIR/${label}.headers"
  if rg -qi '^cache-control: no-store\r?$' "$headers" \
    && rg -qi '^pragma: no-cache\r?$' "$headers"; then
    pass "${label}_no_store"
    return 0
  fi
  fail "${label}_no_store"
  return 1
}

scan_forbidden() {
  local label=$1
  shift
  local output="$VALIDATION_DIR/${label}-forbidden-candidates.txt"
  : >"$output"
  if [[ -n "$FORBID_FILE" && -f "$FORBID_FILE" ]]; then
    rg -l --fixed-strings --file "$FORBID_FILE" "$@" >>"$output" 2>/dev/null || true
  fi
  rg -l 'Bearer |Authorization|Set-Cookie|operator-secret|/home/|/run/dh' "$@" \
    >>"$output" 2>/dev/null || true
  sort -u "$output" -o "$output"
  if [[ -s "$output" ]]; then
    fail "${label}_sanitized"
    return 1
  fi
  pass "${label}_sanitized"
}

check_dns_and_root() {
  if getent hosts "$HOSTNAME" >"$VALIDATION_DIR/dns.txt" 2>"$VALIDATION_DIR/dns.stderr"; then
    pass dns_resolution
  else
    fail dns_resolution
  fi

  if http_probe root "200" "$BASE_URL/"; then
    require_no_store root || true
    local escaped_host=${HOSTNAME//./\\.}
    if rg -qi "^content-security-policy: .*connect-src .*ws://${escaped_host}" "$VALIDATION_DIR/root.headers"; then
      pass root_csp_ws
    else
      fail root_csp_ws
    fi
    scan_forbidden root "$VALIDATION_DIR/root.body" "$VALIDATION_DIR/root.headers" || true
  fi
}

check_health_and_origin() {
  if http_probe health "200" "$BASE_URL/health"; then
    require_no_store health || true
    scan_forbidden health "$VALIDATION_DIR/health.body" "$VALIDATION_DIR/health.headers" || true
  fi

  if http_probe session_unauthenticated "401" \
    -H "Origin: $ORIGIN" \
    "$BASE_URL/api/session"; then
    require_no_store session_unauthenticated || true
  fi
  if http_probe session_wrong_origin "403" \
    -H "Origin: https://example.invalid" \
    "${curl_cookie_args[@]}" \
    "$BASE_URL/api/session"; then
    require_no_store session_wrong_origin || true
  fi
  if http_probe session_null_origin "403" \
    -H "Origin: null" \
    "${curl_cookie_args[@]}" \
    "$BASE_URL/api/session"; then
    require_no_store session_null_origin || true
  fi
  if http_probe session_absent_origin "403" \
    "${curl_cookie_args[@]}" \
    "$BASE_URL/api/session"; then
    require_no_store session_absent_origin || true
  fi
}

ws_probe() {
  local ws_path=$1
  local label=$2
  local origin_value=$3
  local cookie_mode=$4
  local ws_label=${ws_path//\//-}
  ws_label=${ws_label#-}
  local headers="$VALIDATION_DIR/ws-${ws_label}-${label}.headers"
  local body="$VALIDATION_DIR/ws-${ws_label}-${label}.body"
  local stderr="$VALIDATION_DIR/ws-${ws_label}-${label}.stderr"
  local args=(
    curl -sS --http1.1 --max-time 8 "${curl_args_common[@]}"
    -H "Connection: Upgrade"
    -H "Upgrade: websocket"
    -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ=="
    -H "Sec-WebSocket-Version: 13"
    -D "$headers"
    -o "$body"
  )
  if [[ "$origin_value" != "absent" ]]; then
    args+=(-H "Origin: $origin_value")
  fi
  if [[ "$cookie_mode" == "cookie" ]]; then
    args+=("${curl_cookie_args[@]}")
  fi
  set +e
  "${args[@]}" "$BASE_URL$ws_path" >"$VALIDATION_DIR/ws-${ws_label}-${label}.stdout" 2>"$stderr"
  set -e
  awk 'BEGIN{code=0} /^HTTP\// {code=$2} END{print code}' "$headers" \
    >"$VALIDATION_DIR/ws-${ws_label}-${label}.status"
  cat "$VALIDATION_DIR/ws-${ws_label}-${label}.status"
}

check_websockets() {
  local failed=0
  local ws_path code
  for ws_path in /ws/events /ws/input; do
    code=$(ws_probe "$ws_path" allowed "$ORIGIN" cookie)
    [[ "$code" == "101" ]] || failed=1
    code=$(ws_probe "$ws_path" unauth "$ORIGIN" no-cookie)
    [[ "$code" != "101" && "$code" != "0" ]] || failed=1
    code=$(ws_probe "$ws_path" wrong_origin "https://example.invalid" cookie)
    [[ "$code" == "403" ]] || failed=1
    code=$(ws_probe "$ws_path" absent_origin "absent" cookie)
    [[ "$code" == "403" ]] || failed=1
  done
  if [[ $failed -eq 0 ]]; then
    pass websocket_origin_auth
  else
    fail websocket_origin_auth
  fi
}

check_wrong_host_and_network() {
  local headers="$VALIDATION_DIR/wrong-host.headers"
  local body="$VALIDATION_DIR/wrong-host.body"
  local stderr="$VALIDATION_DIR/wrong-host.stderr"
  local code status
  set +e
  code=$(curl -sS --max-time 15 "${curl_args_common[@]}" \
    -H "Host: $WRONG_HOST" \
    -D "$headers" \
    -o "$body" \
    -w '%{http_code}' \
    "$BASE_URL/" 2>"$stderr")
  status=$?
  set -e
  printf '%s\n' "$code" >"$VALIDATION_DIR/wrong-host.status"
  if [[ $status -eq 0 && "$code" =~ ^(200|301|302)$ ]]; then
    fail wrong_host_rejected
  else
    pass wrong_host_rejected
  fi

  reviewed_private_file \
    "$NETWORK_EVIDENCE_FILE" \
    "$NETWORK_EVIDENCE_REVIEWED" \
    network_evidence_private || true
  reviewed_private_file \
    "$OUTSIDE_PROBE_FILE" \
    "$OUTSIDE_PROBE_REVIEWED" \
    outside_probe_private || true
}

main() {
  require_command curl
  require_command getent
  require_command rg
  require_command awk
  require_command stat

  if [[ -z "$VALIDATION_DIR" || ( -z "$COOKIE_FILE" && -z "$COOKIE_CURL_CONFIG_FILE" ) ]]; then
    usage
    exit 2
  fi
  for value in "$HOSTNAME" "$ORIGIN" "$BASE_URL" "$VALIDATION_DIR" "${COOKIE_FILE:-${COOKIE_CURL_CONFIG_FILE:-}}"; do
    if contains_placeholder "$value"; then
      usage
      exit 2
    fi
  done
  ensure_private_path "$VALIDATION_DIR"
  if [[ -n "$COOKIE_CURL_CONFIG_FILE" ]]; then
    check_private_file "$COOKIE_CURL_CONFIG_FILE" cookie_curl_config_private || true
  else
    check_private_file "$COOKIE_FILE" cookie_file_private || true
  fi

  check_dns_and_root
  check_health_and_origin
  check_websockets
  check_wrong_host_and_network

  if [[ $FAILURES -eq 0 ]]; then
    pass "all evidence written privately"
    exit 0
  fi
  printf 'tailscale-http-check: FAIL summary count=%s\n' "$FAILURES" >&2
  exit 1
}

main "$@"
