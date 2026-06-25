#!/usr/bin/env bash
set -euo pipefail
umask 077

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)

ORIGIN=${ROM_BRIDGE_ORIGIN:-https://rombridge.birb.homes}
BASE_URL=${ROM_BRIDGE_BASE_URL:-https://rombridge.birb.homes}
VALIDATION_DIR=${ROM_BRIDGE_VALIDATION_DIR:-}
RESOLVE_IP=${ROM_BRIDGE_RESOLVE_IP:-}
COOKIE_FILE=${ROM_BRIDGE_SESSION_COOKIE_FILE:-}
COOKIE_CURL_CONFIG_FILE=${ROM_BRIDGE_COOKIE_CURL_CONFIG_FILE:-}
SERVICE_PORT=${ROM_BRIDGE_SERVICE_PORT:-7410}
NETWORK_EVIDENCE_FILE=${ROM_BRIDGE_NETWORK_EVIDENCE_FILE:-}
NETWORK_EVIDENCE_REVIEWED=${ROM_BRIDGE_NETWORK_EVIDENCE_REVIEWED:-}
OUTSIDE_PROBE_FILE=${ROM_BRIDGE_OUTSIDE_PROBE_RESULT_FILE:-}
OUTSIDE_PROBE_REVIEWED=${ROM_BRIDGE_OUTSIDE_PROBE_REVIEWED:-}
HOST_SNI_EVIDENCE_FILE=${ROM_BRIDGE_HOST_SNI_EVIDENCE_FILE:-}
HOST_SNI_EVIDENCE_REVIEWED=${ROM_BRIDGE_HOST_SNI_EVIDENCE_REVIEWED:-}
WRONG_HOST=${ROM_BRIDGE_WRONG_HOST:-not-rombridge.invalid}
STATIC_PUBLISH_ROOT=${ROM_BRIDGE_STATIC_PUBLISH_ROOT:-}
FORBID_FILE=${ROM_BRIDGE_FORBID_FILE:-}

FAILURES=0

usage() {
  cat >&2 <<'EOF'
Usage: ROM_BRIDGE_VALIDATION_DIR=/private/dir \
       ROM_BRIDGE_SESSION_COOKIE_FILE=/private/cookie.jar \
       scripts/deployment-network-check.sh

Required:
  ROM_BRIDGE_VALIDATION_DIR         private raw evidence directory, outside repo
  ROM_BRIDGE_SESSION_COOKIE_FILE    0600 cookie jar/file for throwaway session,
                                    or set ROM_BRIDGE_COOKIE_CURL_CONFIG_FILE

Optional:
  ROM_BRIDGE_ORIGIN                 default: https://rombridge.birb.homes
  ROM_BRIDGE_BASE_URL               default: https://rombridge.birb.homes
  ROM_BRIDGE_RESOLVE_IP             private IP for curl --resolve
  ROM_BRIDGE_COOKIE_CURL_CONFIG_FILE 0600 curl config containing cookie header
  ROM_BRIDGE_SERVICE_PORT           default: 7410
  ROM_BRIDGE_NETWORK_EVIDENCE_FILE  private listener/firewall/ingress/ACL
                                    evidence file; requires
                                    ROM_BRIDGE_NETWORK_EVIDENCE_REVIEWED=1
  ROM_BRIDGE_OUTSIDE_PROBE_RESULT_FILE private outside-network probe result file
                                    requires
                                    ROM_BRIDGE_OUTSIDE_PROBE_REVIEWED=1
  ROM_BRIDGE_HOST_SNI_EVIDENCE_FILE private Host/SNI isolation evidence file;
                                    requires
                                    ROM_BRIDGE_HOST_SNI_EVIDENCE_REVIEWED=1
  ROM_BRIDGE_WRONG_HOST             default: not-rombridge.invalid
  ROM_BRIDGE_STATIC_PUBLISH_ROOT    deployed static root to scan, outside repo
  ROM_BRIDGE_FORBID_FILE            private forbidden-literals file
EOF
}

pass() {
  printf 'deployment-network-check: PASS %s\n' "$1"
}

fail() {
  printf 'deployment-network-check: FAIL %s\n' "$1" >&2
  FAILURES=$((FAILURES + 1))
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'deployment-network-check: ERROR missing command %s\n' "$1" >&2
    exit 127
  fi
}

contains_placeholder() {
  [[ "$1" == *"<"* || "$1" == *">"* ]]
}

ensure_private_path() {
  local path=$1
  local label=$2
  local root_real path_real
  root_real=$(cd "$ROOT_DIR" && pwd -P)
  mkdir -p "$path"
  chmod 700 "$path"
  path_real=$(cd "$path" && pwd -P)
  if [[ "$path_real" == "$root_real" || "$path_real" == "$root_real"/* ]]; then
    printf 'deployment-network-check: ERROR %s must be outside repository\n' "$label" >&2
    exit 2
  fi
}

check_private_file() {
  local path=$1
  local label=$2
  if [[ -z "$path" || ! -f "$path" ]]; then
    fail "$label"
    return
  fi
  if contains_placeholder "$path"; then
    fail "$label"
    return
  fi
  local mode
  mode=$(stat -c '%a' "$path" 2>/dev/null || true)
  if [[ "$mode" != "600" ]]; then
    fail "$label"
    return
  fi
  pass "$label"
}

reviewed_evidence_file() {
  local path=$1
  local reviewed=$2
  local label=$3
  if [[ -z "$path" || ! -f "$path" ]]; then
    fail "$label"
    return 1
  fi
  if contains_placeholder "$path"; then
    fail "$label"
    return 1
  fi
  local mode
  mode=$(stat -c '%a' "$path" 2>/dev/null || true)
  if [[ "$mode" != "600" ]]; then
    fail "$label"
    return 1
  fi
  if [[ "$reviewed" != "1" ]]; then
    fail "${label}_operator_reviewed"
    return 1
  fi
  pass "$label"
}

check_cookie_source() {
  if [[ -n "$COOKIE_CURL_CONFIG_FILE" ]]; then
    check_private_file "$COOKIE_CURL_CONFIG_FILE" cookie_curl_config_private
    return
  fi
  check_private_file "$COOKIE_FILE" cookie_file_private
}

curl_args_common=()
if [[ -n "$RESOLVE_IP" ]]; then
  curl_args_common+=(--resolve "rombridge.birb.homes:443:$RESOLVE_IP")
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
  if ! rg -qi '^cache-control: no-store\r?$' "$headers"; then
    fail "${label}_no_store"
    return 1
  fi
  if ! rg -qi '^pragma: no-cache\r?$' "$headers"; then
    fail "${label}_no_store"
    return 1
  fi
  pass "${label}_no_store"
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

check_dns_tls() {
  if getent hosts rombridge.birb.homes >"$VALIDATION_DIR/dns.txt" 2>"$VALIDATION_DIR/dns.stderr"; then
    pass dns_resolution
  else
    fail dns_resolution
  fi

  if http_probe tls_root "200 301 302" -I "$BASE_URL/"; then
    pass tls_root
  fi
}

check_service_bind() {
  if ! ss -ltnp >"$VALIDATION_DIR/listeners.txt" 2>"$VALIDATION_DIR/listeners.stderr"; then
    fail service_bind
    return
  fi

  local port_listeners="$VALIDATION_DIR/listeners-${SERVICE_PORT}.txt"
  rg "(:|\\])${SERVICE_PORT}\\b" "$VALIDATION_DIR/listeners.txt" >"$port_listeners" || true
  if [[ ! -s "$port_listeners" ]]; then
    fail service_bind
    return
  fi

  if rg -q "(^|[[:space:]])(0\\.0\\.0\\.0|\\*|\\[::\\]|::):${SERVICE_PORT}\\b" "$port_listeners"; then
    fail service_bind_wildcard
    return
  fi

  if rg -q "(127\\.0\\.0\\.1|\\[::1\\]|::1):${SERVICE_PORT}\\b" "$port_listeners"; then
    pass service_bind
    return
  fi

  if reviewed_evidence_file \
    "$NETWORK_EVIDENCE_FILE" \
    "$NETWORK_EVIDENCE_REVIEWED" \
    network_evidence_private; then
    pass service_bind_trusted_interface
    return
  fi

  fail service_bind
}

check_health() {
  if http_probe health "200" "$BASE_URL/health"; then
    scan_forbidden health "$VALIDATION_DIR/health.body" "$VALIDATION_DIR/health.headers" || true
    pass health_reachable
  fi
}

check_auth_origin() {
  local origin_failed=0
  if http_probe session_unauthenticated "401" \
    -H "Origin: $ORIGIN" \
    "$BASE_URL/api/session"; then
    require_no_store session_unauthenticated || true
    scan_forbidden session_unauthenticated \
      "$VALIDATION_DIR/session_unauthenticated.body" \
      "$VALIDATION_DIR/session_unauthenticated.headers" || true
    pass unauthenticated_rejection
  fi

  if http_probe session_wrong_origin "403" \
    -H "Origin: https://example.invalid" \
    "${curl_cookie_args[@]}" \
    "$BASE_URL/api/session"; then
    require_no_store session_wrong_origin || true
  else
    origin_failed=1
  fi
  if http_probe session_null_origin "403" \
    -H "Origin: null" \
    "${curl_cookie_args[@]}" \
    "$BASE_URL/api/session"; then
    require_no_store session_null_origin || true
  else
    origin_failed=1
  fi
  if http_probe session_absent_origin "403" \
    "${curl_cookie_args[@]}" \
    "$BASE_URL/api/session"; then
    require_no_store session_absent_origin || true
  else
    origin_failed=1
  fi

  if [[ $origin_failed -eq 0 ]]; then
    pass wrong_origin_rejection
  fi
}

check_runtime_no_store() {
  local probes=(
    "api_session_unauth|401|-H|Origin: $ORIGIN|$BASE_URL/api/session"
    "run_status_unauth|401|-H|Origin: $ORIGIN|$BASE_URL/api/run/status"
    "validation_status_unauth|401|-H|Origin: $ORIGIN|$BASE_URL/api/validation/status"
    "frame_current_unauth|401|-H|Origin: $ORIGIN|$BASE_URL/api/frame/current"
    "frame_image_unauth|401|-H|Origin: $ORIGIN|$BASE_URL/api/frame/current/image"
    "capture_recent_unauth|401|-H|Origin: $ORIGIN|$BASE_URL/api/capture/recent"
  )
  local failed=0
  local item label expected url
  for item in "${probes[@]}"; do
    IFS='|' read -r label expected _ header url <<<"$item"
    if http_probe "$label" "$expected" -H "$header" "$url"; then
      require_no_store "$label" || failed=1
    else
      failed=1
    fi
  done

  if http_probe run_pause_unauth "401" \
    -H "Origin: $ORIGIN" \
    -H "Content-Type: application/json" \
    -X POST \
    --data '{"schema_version":1,"session_id":"deployment-check"}' \
    "$BASE_URL/api/run/pause"; then
    require_no_store run_pause_unauth || failed=1
  else
    failed=1
  fi

  if [[ $failed -eq 0 ]]; then
    pass runtime_no_store
  else
    fail runtime_no_store
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
  local key="dGhlIHNhbXBsZSBub25jZQ=="
  local args=(
    curl -sS --http1.1 --max-time 8 "${curl_args_common[@]}"
    -H "Connection: Upgrade"
    -H "Upgrade: websocket"
    -H "Sec-WebSocket-Key: $key"
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
    code=$(ws_probe "$ws_path" null_origin "null" cookie)
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

check_mixed_content() {
  local failed=0
  if ! http_probe root_index "200 301 302" "$BASE_URL/"; then
    fail mixed_content_absent
    return
  fi
  rg -l 'http://|ws://' "$VALIDATION_DIR/root_index.body" >"$VALIDATION_DIR/mixed-content-candidates.txt" || true
  if [[ -s "$VALIDATION_DIR/mixed-content-candidates.txt" ]]; then
    failed=1
  fi

  if ! check_static_publish_root; then
    failed=1
  fi

  if [[ $failed -eq 0 ]]; then
    pass mixed_content_absent
  else
    fail mixed_content_absent
  fi
}

check_static_publish_root() {
  if [[ -z "$STATIC_PUBLISH_ROOT" ]]; then
    fail static_publish_root_scan
    return 1
  fi
  if contains_placeholder "$STATIC_PUBLISH_ROOT"; then
    fail static_publish_root_scan
    return 1
  fi
  if [[ ! -d "$STATIC_PUBLISH_ROOT" ]]; then
    fail static_publish_root_scan
    return 1
  fi

  local root_real static_real failed=0
  root_real=$(cd "$ROOT_DIR" && pwd -P)
  static_real=$(cd "$STATIC_PUBLISH_ROOT" && pwd -P)
  if [[ "$static_real" == "$root_real" || "$static_real" == "$root_real"/* ]]; then
    fail static_publish_root_outside_repo
    return 1
  fi

  find "$static_real" -type l -print -quit >"$VALIDATION_DIR/static-symlink-candidates.txt"
  if [[ -s "$VALIDATION_DIR/static-symlink-candidates.txt" ]]; then
    fail static_publish_root_no_symlinks
    failed=1
  fi

  find "$static_real" -type f -name '*.map' -print -quit >"$VALIDATION_DIR/static-sourcemap-candidates.txt"
  if [[ -s "$VALIDATION_DIR/static-sourcemap-candidates.txt" ]]; then
    fail static_publish_root_no_sourcemaps
    failed=1
  fi

  rg -l 'http://|ws://' "$static_real" >"$VALIDATION_DIR/static-mixed-content-candidates.txt" || true
  if [[ -s "$VALIDATION_DIR/static-mixed-content-candidates.txt" ]]; then
    fail static_publish_root_mixed_content_absent
    failed=1
  fi

  scan_forbidden static_publish_root "$static_real" || failed=1

  if [[ $failed -eq 0 ]]; then
    pass static_publish_root_scan
    return 0
  fi
  return 1
}

check_host_sni() {
  local failed=0
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
    failed=1
  fi

  if [[ -n "$RESOLVE_IP" ]]; then
    local sni_headers="$VALIDATION_DIR/wrong-sni.headers"
    local sni_body="$VALIDATION_DIR/wrong-sni.body"
    local sni_stderr="$VALIDATION_DIR/wrong-sni.stderr"
    set +e
    code=$(curl -k -sS --max-time 15 \
      --resolve "$WRONG_HOST:443:$RESOLVE_IP" \
      -D "$sni_headers" \
      -o "$sni_body" \
      -w '%{http_code}' \
      "https://$WRONG_HOST/" 2>"$sni_stderr")
    status=$?
    set -e
    printf '%s\n' "$code" >"$VALIDATION_DIR/wrong-sni.status"
    if [[ $status -eq 0 && "$code" =~ ^(200|301|302)$ ]]; then
      failed=1
    fi
  elif ! reviewed_evidence_file \
    "$HOST_SNI_EVIDENCE_FILE" \
    "$HOST_SNI_EVIDENCE_REVIEWED" \
    host_sni_evidence_private; then
    failed=1
  fi

  if [[ $failed -eq 0 ]]; then
    pass host_sni_isolation
  else
    fail host_sni_isolation
  fi
}

check_outside_network() {
  if reviewed_evidence_file \
    "$OUTSIDE_PROBE_FILE" \
    "$OUTSIDE_PROBE_REVIEWED" \
    outside_probe_private; then
    pass outside_network_rejected
    return
  fi
  if reviewed_evidence_file \
    "$NETWORK_EVIDENCE_FILE" \
    "$NETWORK_EVIDENCE_REVIEWED" \
    network_evidence_private; then
    pass outside_network_rejected_with_network_artifact
    return
  fi
  fail outside_network_rejected
}

main() {
  require_command curl
  require_command getent
  require_command rg
  require_command ss
  require_command awk
  require_command find
  require_command stat

  if [[ -z "$VALIDATION_DIR" || ( -z "$COOKIE_FILE" && -z "$COOKIE_CURL_CONFIG_FILE" ) ]]; then
    usage
    exit 2
  fi
  for value in "$ORIGIN" "$BASE_URL" "$VALIDATION_DIR" "${COOKIE_FILE:-${COOKIE_CURL_CONFIG_FILE:-}}"; do
    if contains_placeholder "$value"; then
      usage
      exit 2
    fi
  done
  ensure_private_path "$VALIDATION_DIR" "validation directory"

  check_cookie_source
  check_dns_tls
  check_service_bind
  check_health
  check_auth_origin
  check_runtime_no_store
  check_websockets
  check_mixed_content
  check_host_sni
  check_outside_network

  if [[ $FAILURES -eq 0 ]]; then
    pass "all evidence written privately"
    exit 0
  fi

  printf 'deployment-network-check: FAIL summary count=%s\n' "$FAILURES" >&2
  exit 1
}

main "$@"
