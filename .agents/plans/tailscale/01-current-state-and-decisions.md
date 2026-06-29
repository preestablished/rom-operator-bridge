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

Before implementing, also verify that browsers will not force HTTPS for this
subdomain through HSTS. At the time this plan was written, command-line probes
of `https://birb.homes/` and `https://rombridge.birb.homes/` did not show a
`Strict-Transport-Security` header. Recheck this from the implementation
machine. If the parent domain later enables `includeSubDomains`, browser HTTP
access to `tailrombridge.birb.homes` may be impossible without changing the
hostname or using TLS.

## Recommended Routing Decision

Use a local HTTP reverse proxy instead of binding the bridge service directly to
port `80`.

Recommended:

```text
Nginx or Traefik listens on <tailscale-ip>:80
rom-operator-bridge listens on 127.0.0.1:7410
proxy forwards Host, Origin, WebSocket Upgrade, and X-Forwarded-Proto
```

Reasons:

- no need to grant the Rust service `CAP_NET_BIND_SERVICE`;
- one small proxy owns the low port and host matching;
- the bridge service can stay unreachable from non-local sockets;
- rollback can remove the proxy without changing private runtime data;
- the same bridge binary continues to serve static UI, API, and WebSockets.

The current operator env validator rejects loopback binds. The implementation
should add an explicit Tailscale HTTP proxy mode that permits
`127.0.0.1:7410`, or it should document a safer equivalent. Do not silently make
loopback valid for every deployment mode.

## Origin Policy Decision

Support multiple deployments without weakening either one:

```text
ROM_OPERATOR_BRIDGE_PUBLIC_ORIGIN=http://tailrombridge.birb.homes
ROM_OPERATOR_BRIDGE_ALLOWED_ORIGINS=http://tailrombridge.birb.homes
ROM_OPERATOR_BRIDGE_COOKIE_SECURE=false
ROM_OPERATOR_BRIDGE_EXPOSURE_MODE=tailscale-http
```

For the existing HTTPS route:

```text
ROM_OPERATOR_BRIDGE_PUBLIC_ORIGIN=https://rombridge.birb.homes
ROM_OPERATOR_BRIDGE_ALLOWED_ORIGINS=https://rombridge.birb.homes
ROM_OPERATOR_BRIDGE_COOKIE_SECURE=true
ROM_OPERATOR_BRIDGE_EXPOSURE_MODE=https-origin
```

The exact env names can change during implementation, but the model should keep
these concerns separate:

- public origin used for docs, CSP, and static headers;
- allowed runtime origins used for Origin validation;
- cookie secure policy;
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
