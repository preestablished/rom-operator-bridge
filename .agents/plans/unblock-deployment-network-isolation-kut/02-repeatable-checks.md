# Repeatable Network Isolation Checks

## 1. Prefer A Small Script

If the deployment route exists, add a script such as:

```text
scripts/deployment-network-check.sh
```

The script should:

- use `set -euo pipefail` and `umask 077`;
- require explicit environment variables instead of embedding private values;
- write raw command output only under `$ROM_BRIDGE_VALIDATION_DIR`;
- print only sanitized `PASS`/`FAIL` lines to stdout;
- exit non-zero if any required check fails;
- avoid printing cookies, concrete IPs, private paths, or full headers.

Suggested environment variables:

```sh
export ROM_BRIDGE_ORIGIN="https://rombridge.birb.homes"
export ROM_BRIDGE_BASE_URL="https://rombridge.birb.homes"
export ROM_BRIDGE_VALIDATION_DIR="/path/to/private-validation/deployment-network-kut"
export ROM_BRIDGE_RESOLVE_IP="<bridge-private-ip>"
export ROM_BRIDGE_SESSION_COOKIE_FILE="/path/to/private/session-cookie.jar"
export ROM_BRIDGE_EXPECT_OUTSIDE_BLOCKED="1"
```

Replace placeholders before running. The script must reject unset variables and
values that still contain `<` or `>`.

`ROM_BRIDGE_SESSION_COOKIE_FILE` must be a `0600` private cookie jar or cookie
file. The script may pass this filename to tools that read cookies from a file,
but it must never read the cookie value into an environment variable, echo it,
place it in argv, print it to stdout, or write it to bead notes.

Before any authenticated probe, fail closed if the cookie file is absent or not
private:

```sh
test -f "$ROM_BRIDGE_SESSION_COOKIE_FILE"
test "$(stat -c '%a' "$ROM_BRIDGE_SESSION_COOKIE_FILE")" = "600"
```

## 2. Required Checks

Implement or manually run equivalent checks. The script should use this assertion
pattern for HTTP probes instead of treating captured output as success:

```sh
code=$(curl -sS \
  -D "$ROM_BRIDGE_VALIDATION_DIR/<check>.headers" \
  -o "$ROM_BRIDGE_VALIDATION_DIR/<check>.body" \
  -w '%{http_code}' \
  <curl-args>)
case "$code" in
  <expected-codes>) ;;
  *) printf 'deployment-network-check: FAIL <check>\n' >&2; exit 1 ;;
esac
```

Every PASS must be backed by explicit assertions for status code, required
headers, and forbidden-value scans over the private body/header files. Do not
print raw bodies or headers to stdout.

### DNS And TLS

```sh
getent hosts rombridge.birb.homes >"$ROM_BRIDGE_VALIDATION_DIR/dns.txt"
curl -fsSIL \
  --resolve "rombridge.birb.homes:443:$ROM_BRIDGE_RESOLVE_IP" \
  "$ROM_BRIDGE_BASE_URL/" \
  >"$ROM_BRIDGE_VALIDATION_DIR/tls-root.headers"
```

Sanitized result to record:

- hostname resolves;
- HTTPS responds for the selected host;
- response is for the expected origin.

### Service Bind

Use a host-local listener command and store raw output privately:

```sh
ss -ltnp >"$ROM_BRIDGE_VALIDATION_DIR/listeners.txt"
```

Sanitized result to record:

- bridge bind is `127.0.0.1:<port>`, `::1:<port>`, or the documented trusted
  interface;
- if bound to a trusted interface, proxy/firewall policy rejects unauthorized
  origins and outside-network access.

### Health Sanitization

```sh
code=$(curl -sS \
  -D "$ROM_BRIDGE_VALIDATION_DIR/health.headers" \
  -o "$ROM_BRIDGE_VALIDATION_DIR/health.json" \
  -w '%{http_code}' \
  "$ROM_BRIDGE_BASE_URL/health")
test "$code" = "200"
```

Sanitized result to record:

- status is successful;
- body contains only schema/version/backend mode health fields;
- no private paths, endpoints, credentials, refs, or operator values appear.

### Unauthenticated Runtime Rejection

Use the allowed browser origin and no cookie. This proves session auth rejection
rather than missing-Origin rejection.

```sh
code=$(curl -sS \
  -H "Origin: $ROM_BRIDGE_ORIGIN" \
  -D "$ROM_BRIDGE_VALIDATION_DIR/session-unauthenticated.headers" \
  -o "$ROM_BRIDGE_VALIDATION_DIR/session-unauthenticated.body" \
  -w '%{http_code}' \
  "$ROM_BRIDGE_BASE_URL/api/session")
test "$code" = "401"
```

Sanitized result to record:

- request is rejected;
- body uses the common sanitized error envelope;
- response has `Cache-Control: no-store` and `Pragma: no-cache`.

### Wrong Origin Rejection

Use the private throwaway cookie file so the check proves Origin rejection occurs
before serving authenticated session state. Cover absent, `null`, and unrelated
origins.

```sh
code=$(curl -sS \
  -H "Origin: https://example.invalid" \
  --cookie "$ROM_BRIDGE_SESSION_COOKIE_FILE" \
  -D "$ROM_BRIDGE_VALIDATION_DIR/session-wrong-origin.headers" \
  -o "$ROM_BRIDGE_VALIDATION_DIR/session-wrong-origin.body" \
  -w '%{http_code}' \
  "$ROM_BRIDGE_BASE_URL/api/session")
test "$code" = "403"

code=$(curl -sS \
  -H "Origin: null" \
  --cookie "$ROM_BRIDGE_SESSION_COOKIE_FILE" \
  -D "$ROM_BRIDGE_VALIDATION_DIR/session-null-origin.headers" \
  -o "$ROM_BRIDGE_VALIDATION_DIR/session-null-origin.body" \
  -w '%{http_code}' \
  "$ROM_BRIDGE_BASE_URL/api/session")
test "$code" = "403"

code=$(curl -sS \
  --cookie "$ROM_BRIDGE_SESSION_COOKIE_FILE" \
  -D "$ROM_BRIDGE_VALIDATION_DIR/session-absent-origin.headers" \
  -o "$ROM_BRIDGE_VALIDATION_DIR/session-absent-origin.body" \
  -w '%{http_code}' \
  "$ROM_BRIDGE_BASE_URL/api/session")
test "$code" = "403"
```

Do not read the cookie file into a shell variable.

Sanitized result to record:

- absent/null/wrong origins are rejected according to runtime contract;
- no private session state is served.

### Runtime No-Store Headers

Do not use `HEAD` as the primary evidence, because route fallbacks may not
mirror `GET`/`POST` behavior. Use real requests with `-D` header capture and
`-o` body capture.

Cover this route/method matrix:

| Route | Method | Auth State | Expected |
|---|---|---|---|
| `/api/session` | `GET` | allowed Origin, no cookie | `401`, no-store |
| `/api/run/status` | `GET` | allowed Origin, no cookie | `401`, no-store |
| `/api/validation/status` | `GET` | allowed Origin, no cookie | `401`, no-store |
| `/api/frame/current` | `GET` | allowed Origin, no cookie | `401`, no-store |
| `/api/frame/current/image` | `GET` | allowed Origin, no cookie | `401`, no-store |
| `/api/capture/recent` | `GET` | allowed Origin, no cookie | `401`, no-store |
| `/api/capture/jobs/<invalid-id>` | `GET` | allowed Origin, valid cookie | sanitized error, no-store |
| `/api/capture/<invalid-id>` | `GET` | allowed Origin, valid cookie | sanitized error, no-store |
| `/api/run/pause` | `POST` | allowed Origin, valid JSON body, no cookie | `401`, no-store |

If a live authenticated session exists, also check reachable success responses
without creating new private capture data:

| Route | Method | Expected |
|---|---|---|
| `/api/session` | `GET` | `200`, no-store |
| `/api/run/status` | `GET` | `200`, no-store |
| `/api/frame/current` | `GET` | `200`, no-store |
| `/api/frame/current/image?frame=<current-frame>` | `GET` | `200`, no-store, PNG |
| `/api/capture/<capture-id>/preview` | `GET` | no-store if an approved capture id exists |

Example unauthenticated route check:

```sh
code=$(curl -sS \
  -H "Origin: $ROM_BRIDGE_ORIGIN" \
  -D "$ROM_BRIDGE_VALIDATION_DIR/run-status-unauth.headers" \
  -o "$ROM_BRIDGE_VALIDATION_DIR/run-status-unauth.body" \
  -w '%{http_code}' \
  "$ROM_BRIDGE_BASE_URL/api/run/status")
test "$code" = "401"
rg -qi '^cache-control: no-store\r?$' "$ROM_BRIDGE_VALIDATION_DIR/run-status-unauth.headers"
rg -qi '^pragma: no-cache\r?$' "$ROM_BRIDGE_VALIDATION_DIR/run-status-unauth.headers"
```

Sanitized result to record:

- `Cache-Control: no-store`;
- `Pragma: no-cache`;
- `X-Content-Type-Options: nosniff` where applicable;
- `Vary: Origin` when origin-specific CORS is emitted.

### WebSocket Origin/Auth

Test the deployed WSS handshake for both WebSocket routes. Do not allow repo
integration tests alone to replace deployed WSS evidence.

Use bounded HTTP Upgrade probes with `curl` so the cookie can be read from a
private file instead of being placed in argv. A successful allowed-origin,
authenticated handshake should return `101`; rejection cases should return a
sanitized non-`101` response such as `401` or `403`.

```sh
probe_ws() {
  ws_path=$1
  label=$2
  origin_value=$3
  cookie_mode=$4
  headers="$ROM_BRIDGE_VALIDATION_DIR/ws${ws_path//\//-}-${label}.headers"
  body="$ROM_BRIDGE_VALIDATION_DIR/ws${ws_path//\//-}-${label}.body"
  key="dGhlIHNhbXBsZSBub25jZQ=="
  args=(
    curl -sS --http1.1 --max-time 8
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
  if [[ "$cookie_mode" = "cookie" ]]; then
    args+=(--cookie "$ROM_BRIDGE_SESSION_COOKIE_FILE")
  fi
  "${args[@]}" "$ROM_BRIDGE_BASE_URL$ws_path" >/dev/null || true
  awk 'BEGIN{code=0} /^HTTP\// {code=$2} END{print code}' "$headers"
}

for ws_path in /ws/events /ws/input; do
  code=$(probe_ws "$ws_path" allowed "$ROM_BRIDGE_ORIGIN" cookie)
  test "$code" = "101"
done
```

Run the rejection matrix for both `/ws/events` and `/ws/input`:

```sh
for ws_path in /ws/events /ws/input; do
  code=$(probe_ws "$ws_path" unauth "$ROM_BRIDGE_ORIGIN" no-cookie)
  test "$code" != "101"

  code=$(probe_ws "$ws_path" wrong-origin "https://example.invalid" cookie)
  test "$code" = "403"

  code=$(probe_ws "$ws_path" null-origin "null" cookie)
  test "$code" = "403"

  code=$(probe_ws "$ws_path" absent-origin "absent" cookie)
  test "$code" = "403"
done
```

If the deployment requires `websocat` for an end-to-end event stream check, wrap
it in `timeout` and use only a tool path that can read private headers from a
`0600` file or stdin. Never pass the cookie value as a literal command argument.
If the available WebSocket client cannot do that, do not use it for authenticated
checks; keep the `curl` handshake matrix as the deployed WSS evidence.

```text
timeout 8s <stdin-or-header-file-capable-ws-client> \
  --origin "$ROM_BRIDGE_ORIGIN" \
  --cookie-file "$ROM_BRIDGE_SESSION_COOKIE_FILE" \
  "wss://rombridge.birb.homes/ws/events" \
  >"$ROM_BRIDGE_VALIDATION_DIR/ws-events.txt"
```

Sanitized result to record:

- allowed-origin authenticated WSS handshakes connect for both endpoints;
- wrong-origin, null-origin, absent-origin, and unauthenticated WSS handshakes
  are rejected for both endpoints;
- raw event payloads stay private.

### Mixed-Content Absence

Fetch the browser-facing root, index, runtime config if present, and built asset
references into private evidence files:

```sh
curl -sS \
  -D "$ROM_BRIDGE_VALIDATION_DIR/root.headers" \
  -o "$ROM_BRIDGE_VALIDATION_DIR/root.html" \
  "$ROM_BRIDGE_BASE_URL/"
```

For mixed-content, assert:

- root/index responses are HTTPS;
- CSP, if emitted at the edge, allows only same-origin HTTPS/WSS runtime
  connectivity;
- committed static output and fetched browser assets contain no `http://` or
  `ws://` runtime endpoint references;
- runtime URLs use `https://rombridge.birb.homes/...` and
  `wss://rombridge.birb.homes/...`.

Use filename-only scans for private evidence:

```sh
rg -l 'http://|ws://' "$ROM_BRIDGE_VALIDATION_DIR" \
  >"$ROM_BRIDGE_VALIDATION_DIR/mixed-content-candidates.txt" || true
test ! -s "$ROM_BRIDGE_VALIDATION_DIR/mixed-content-candidates.txt"
```

### Outside-Network Access

If the executing host can probe from outside the trusted network, record a
private result file and summarize only pass/fail.

If outside probing is not available, `PASS` still requires at least one technical
artifact. Acceptable alternatives include:

- firewall policy showing only trusted ingress;
- proxy Host/SNI routing for `rombridge.birb.homes`;
- no listener on a public interface;
- network ACL proof from the deployment platform.

An operator statement alone is a residual risk, not a passing result. Do not
include firewall rule dumps with private addresses in committed docs.

## 3. Script Output Shape

The script stdout should look like this:

```text
deployment-network-check: PASS dns_tls
deployment-network-check: PASS service_bind
deployment-network-check: PASS health_sanitized
deployment-network-check: PASS unauthenticated_rejection
deployment-network-check: PASS wrong_origin_rejection
deployment-network-check: PASS runtime_no_store
deployment-network-check: PASS websocket_origin_auth
deployment-network-check: PASS mixed_content_absent
deployment-network-check: PASS outside_network_rejected
deployment-network-check: PASS all evidence written privately
```

Failures should identify the check name only, for example:

```text
deployment-network-check: FAIL runtime_no_store
```

Do not print raw response bodies or headers to stdout.
