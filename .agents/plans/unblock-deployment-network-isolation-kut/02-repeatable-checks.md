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
ROM_BRIDGE_ORIGIN=https://rombridge.birb.homes
ROM_BRIDGE_BASE_URL=https://rombridge.birb.homes
ROM_BRIDGE_VALIDATION_DIR=<private-validation-dir>/deployment-network-kut
ROM_BRIDGE_RESOLVE_IP=<bridge-private-ip>
ROM_BRIDGE_SESSION_COOKIE_FILE=<private-cookie-file>
ROM_BRIDGE_EXPECT_OUTSIDE_BLOCKED=1
```

`ROM_BRIDGE_SESSION_COOKIE_FILE` should contain only a private throwaway cookie
value. The script must never echo it.

## 2. Required Checks

Implement or manually run equivalent checks.

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
curl -fsS "$ROM_BRIDGE_BASE_URL/health" \
  >"$ROM_BRIDGE_VALIDATION_DIR/health.json"
```

Sanitized result to record:

- status is successful;
- body contains only schema/version/backend mode health fields;
- no private paths, endpoints, credentials, refs, or operator values appear.

### Unauthenticated Runtime Rejection

```sh
curl -i "$ROM_BRIDGE_BASE_URL/api/session" \
  >"$ROM_BRIDGE_VALIDATION_DIR/session-unauthenticated.http"
```

Sanitized result to record:

- request is rejected;
- body uses the common sanitized error envelope;
- response has `Cache-Control: no-store` and `Pragma: no-cache`.

### Wrong Origin Rejection

```sh
curl -i \
  -H "Origin: https://example.invalid" \
  "$ROM_BRIDGE_BASE_URL/api/session" \
  >"$ROM_BRIDGE_VALIDATION_DIR/session-wrong-origin.http"
```

If an authenticated cookie is required to reach the relevant branch, load it from
a private file and keep output private:

```sh
COOKIE_VALUE=$(cat "$ROM_BRIDGE_SESSION_COOKIE_FILE")
curl -i \
  -H "Origin: https://example.invalid" \
  --cookie "$COOKIE_VALUE" \
  "$ROM_BRIDGE_BASE_URL/api/session" \
  >"$ROM_BRIDGE_VALIDATION_DIR/session-auth-wrong-origin.http"
unset COOKIE_VALUE
```

Sanitized result to record:

- absent/null/wrong origins are rejected according to runtime contract;
- no private session state is served.

### Runtime No-Store Headers

Check each runtime route that can be reached without creating private capture
data:

```sh
curl -I "$ROM_BRIDGE_BASE_URL/api/session" \
  >"$ROM_BRIDGE_VALIDATION_DIR/session.headers"
curl -I "$ROM_BRIDGE_BASE_URL/api/run/status" \
  >"$ROM_BRIDGE_VALIDATION_DIR/run-status.headers"
```

Sanitized result to record:

- `Cache-Control: no-store`;
- `Pragma: no-cache`;
- `X-Content-Type-Options: nosniff` where applicable;
- `Vary: Origin` when origin-specific CORS is emitted.

### WebSocket Origin/Auth

Use `websocat` if available. If not available, document the missing local tool
and use the repo’s WebSocket integration tests plus proxy config evidence.

```sh
COOKIE_VALUE=$(cat "$ROM_BRIDGE_SESSION_COOKIE_FILE")
websocat \
  -H "Origin: $ROM_BRIDGE_ORIGIN" \
  -H "Cookie: $COOKIE_VALUE" \
  "wss://rombridge.birb.homes/ws/events" \
  >"$ROM_BRIDGE_VALIDATION_DIR/ws-events.txt"
unset COOKIE_VALUE
```

Also attempt a wrong-origin WebSocket connection and require rejection.

Sanitized result to record:

- allowed-origin authenticated WebSocket connects;
- wrong-origin or unauthenticated WebSocket is rejected;
- raw event payloads stay private.

### Outside-Network Access

If the executing host can probe from outside the trusted network, record a
private result file and summarize only pass/fail.

If outside probing is not available, acceptable evidence can be:

- firewall policy showing only trusted ingress;
- proxy Host/SNI routing for `rombridge.birb.homes`;
- no listener on a public interface;
- operator-provided network boundary statement.

Do not include firewall rule dumps with private addresses in committed docs.

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
deployment-network-check: PASS outside_network_rejected
deployment-network-check: PASS all evidence written privately
```

Failures should identify the check name only, for example:

```text
deployment-network-check: FAIL runtime_no_store
```

Do not print raw response bodies or headers to stdout.
