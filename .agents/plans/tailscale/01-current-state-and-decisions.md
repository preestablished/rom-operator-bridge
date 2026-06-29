# Current State And Decisions

## Known Inputs

- Operator HTTP hostname: `tailrombridge.birb.homes`.
- DNS currently resolves that hostname to a Tailscale address for this machine.
- The existing public operator route remains
  `https://rombridge.birb.homes/`.
- The bridge service default runtime port is `7410`.

Use `<tailscale-ip>` in committed material. Do not commit the concrete Tailscale
address or any private endpoint manifest.

## Current Code Assumptions

The current service is HTTPS-origin specific:

- `service/src/auth.rs` hard-codes `ALLOWED_ORIGIN` as
  `https://rombridge.birb.homes`.
- `session_cookie_header()` always emits `Secure`.
- `expired_session_cookie_header()` always emits `Secure`.
- runtime response headers always use the HTTPS allowed origin.
- `service/src/api.rs` hard-codes a static CSP with
  `wss://rombridge.birb.homes`.
- many tests assert the current HTTPS cookie and CSP behavior.

The UI is mostly ready for HTTP:

- `RuntimeApiClient` uses same-origin fetch paths.
- `RuntimeWebSocketClient` picks `ws:` when `location.protocol` is `http:`.
- runtime config rejects cross-origin API and WebSocket paths, which is still
  desirable.

The deployment scripts are HTTPS-specific:

- `scripts/deployment-network-check.sh` defaults to
  `https://rombridge.birb.homes`.
- `scripts/prepare-deployment-validation-inputs.py` hard-codes HTTPS, Host, and
  Origin values for the current deployment.
- `scripts/validate-operator-env.py` requires port `7410`, rejects wildcard
  binds, and rejects loopback binds.

## Browser And Security Consequences

Plain HTTP cannot use the existing `Secure` session cookie in normal browsers.
If the service keeps `Secure`, login may return a cookie header but the browser
will not store it for `http://tailrombridge.birb.homes`.

HTTP mode therefore requires a deliberate cookie policy:

```text
HttpOnly; SameSite=Strict
```

Do not include `Secure` for the Tailscale HTTP route. Only allow this when the
configured public origin has `http://` and the exposure mode is explicitly
Tailscale-only.

Before implementing, verify that browsers will not force HTTPS for this
subdomain through HSTS. At the time this plan was written, command-line probes
of `https://birb.homes/` and `https://rombridge.birb.homes/` did not show a
`Strict-Transport-Security` header. Recheck this from the implementation
machine, and also check the browser HSTS preload list and a fresh browser
profile. If the parent domain later enables `includeSubDomains`, or if a
browser profile has cached HSTS for the name, browser HTTP access to
`tailrombridge.birb.homes` may be impossible without changing the hostname,
clearing operator-local HSTS state, or using TLS.

## Coexistence Decision

The implementation must preserve `https://rombridge.birb.homes/`. Do not change
the single existing service env to:

```text
ROM_OPERATOR_BRIDGE_PUBLIC_ORIGIN=http://tailrombridge.birb.homes
ROM_OPERATOR_BRIDGE_COOKIE_SECURE=false
```

unless the HTTPS route has first been moved to a separate validated service
instance. A single global HTTP profile would break the current route's cookie,
CSP, and Origin contract.

Preferred model:

```text
one bridge service process
  -> profile selected from validated Host for static responses
  -> profile selected from validated Origin for runtime HTTP and WebSockets
  -> HTTPS profile keeps Secure cookies and wss CSP
  -> Tailscale HTTP profile uses non-Secure cookies and ws CSP
```

Fallback model:

```text
existing HTTPS bridge service remains unchanged
separate Tailscale bridge service binds a distinct loopback port
separate private env, session secret, validation directory, and rollback path
```

If the fallback model shares the real backend, add an operator policy that only
one service instance may hold a real session at a time. Prefer separate private
runtime roots so capture, label, event, and validation files cannot collide.

## Recommended Routing Decision

Use a local HTTP reverse proxy instead of binding the bridge service directly to
port `80`.

Recommended:

```text
Nginx or Traefik listens on <tailscale-ip>:80
rom-operator-bridge listens on <bridge-upstream>:<bridge-port>
proxy forwards Host, Origin, WebSocket Upgrade, and X-Forwarded-Proto
```

Reasons:

- no need to grant the Rust service `CAP_NET_BIND_SERVICE`;
- one small proxy owns the low port and host matching;
- the bridge service can stay unreachable from non-local sockets;
- rollback can remove the proxy without changing private runtime data;
- the same bridge binary continues to serve static UI, API, and WebSockets.

If the chosen topology moves a bridge service behind a loopback-only proxy, the
operator env validator must allow that only for the explicit Tailscale proxy
mode or a documented full-proxy migration. Do not silently make loopback valid
for every deployment mode.

## Origin Policy Decision

Support multiple deployments without weakening either one. Exact env names can
change, but the model should support route profiles rather than a single global
HTTP-or-HTTPS switch:

```text
ROM_OPERATOR_BRIDGE_DEPLOYMENT_PROFILES=https-origin,tailscale-http
ROM_OPERATOR_BRIDGE_PROFILE_HTTPS_PUBLIC_ORIGIN=https://rombridge.birb.homes
ROM_OPERATOR_BRIDGE_PROFILE_HTTPS_ALLOWED_ORIGINS=https://rombridge.birb.homes
ROM_OPERATOR_BRIDGE_PROFILE_HTTPS_COOKIE_SECURE=true
ROM_OPERATOR_BRIDGE_PROFILE_TAIL_PUBLIC_ORIGIN=http://tailrombridge.birb.homes
ROM_OPERATOR_BRIDGE_PROFILE_TAIL_ALLOWED_ORIGINS=http://tailrombridge.birb.homes
ROM_OPERATOR_BRIDGE_PROFILE_TAIL_COOKIE_SECURE=false
```

For a separate Tailscale-only service instance, the same profile can be reduced
to a single-process HTTP configuration:

```text
ROM_OPERATOR_BRIDGE_PUBLIC_ORIGIN=http://tailrombridge.birb.homes
ROM_OPERATOR_BRIDGE_ALLOWED_ORIGINS=http://tailrombridge.birb.homes
ROM_OPERATOR_BRIDGE_COOKIE_SECURE=false
ROM_OPERATOR_BRIDGE_EXPOSURE_MODE=tailscale-http
```

Do not use the reduced single-process HTTP configuration for the existing HTTPS
service.

The implementation should keep these concerns separate:

- public origin used for docs, CSP, static headers, and Host matching;
- allowed runtime origins used for Origin validation;
- cookie secure policy selected from the accepted profile;
- exposure mode used by validators and deployment scripts.

## Validation Decision

Do not bend the existing HTTPS deployment checker until it becomes unreadable.
Either:

- add scheme/host/protocol parameters to `scripts/deployment-network-check.sh`;
  or
- create `scripts/tailscale-http-check.sh` for this route and leave the HTTPS
  checker focused on `rombridge.birb.homes`.

Prefer a separate checker if it keeps the trust model clear. The Tailscale
checker should prove tailnet reachability, HTTP root/API/WS behavior, wrong
Origin rejection, no public interface listener, and redaction safety.
