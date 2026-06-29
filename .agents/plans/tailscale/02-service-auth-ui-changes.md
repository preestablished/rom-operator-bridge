# Service Auth And UI Changes

## Service Config

Add deployment security settings to `ServiceConfig` or a nested config struct.
Keep defaults equivalent to today's HTTPS route so existing tests and operator
deployments remain safe.

Suggested fields:

```rust
pub struct DeploymentSecurityConfig {
    pub public_origin: Origin,
    pub allowed_origins: Vec<Origin>,
    pub cookie_secure: bool,
    pub exposure_mode: ExposureMode,
}
```

Suggested env keys:

```text
ROM_OPERATOR_BRIDGE_PUBLIC_ORIGIN=<scheme://host[:port]>
ROM_OPERATOR_BRIDGE_ALLOWED_ORIGINS=<comma-separated scheme://host[:port] list>
ROM_OPERATOR_BRIDGE_COOKIE_SECURE=<true-or-false>
ROM_OPERATOR_BRIDGE_EXPOSURE_MODE=<https-origin-or-tailscale-http>
```

Default values should preserve existing behavior:

```text
ROM_OPERATOR_BRIDGE_PUBLIC_ORIGIN=https://rombridge.birb.homes
ROM_OPERATOR_BRIDGE_ALLOWED_ORIGINS=https://rombridge.birb.homes
ROM_OPERATOR_BRIDGE_COOKIE_SECURE=true
ROM_OPERATOR_BRIDGE_EXPOSURE_MODE=https-origin
```

Validation rules:

- each origin must be absolute `http://` or `https://`;
- no path, query, fragment, userinfo, wildcard host, or empty host;
- `cookie_secure=false` is valid only when every allowed origin is `http://`
  and `exposure_mode=tailscale-http`;
- `cookie_secure=true` is required for any `https://` allowed origin;
- do not permit mixed HTTP and HTTPS allowed origins in one process unless the
  implementation has explicit tests proving cookie behavior is safe.

## Origin Validation

Refactor `service/src/auth.rs` so origin validation receives the configured
allowed-origin set.

Current shape:

```rust
validate_runtime_request(headers, uri)
validate_origin(headers)
```

Target shape:

```rust
validate_runtime_request(headers, uri, origin_policy)
validate_origin(headers, origin_policy) -> Result<AllowedOrigin, AuthError>
```

The returned allowed origin should be used when writing CORS headers. Runtime
responses must echo the accepted request origin:

```text
Access-Control-Allow-Origin: http://tailrombridge.birb.homes
Vary: Origin
```

Do not return wildcard CORS for credentialed runtime routes.

Keep these rejection rules:

- absent Origin is rejected for browser runtime requests;
- `Origin: null` is rejected;
- unrelated origins are rejected;
- credentials in URLs remain rejected.

## Cookie Headers

Change cookie formatting to take the configured secure policy:

```rust
session_cookie_header(session, cookie_secure)
expired_session_cookie_header(cookie_secure)
```

Expected HTTPS cookie:

```text
rom_operator_bridge_session=<value>; Path=/; Max-Age=14400; HttpOnly; Secure; SameSite=Strict
```

Expected Tailscale HTTP cookie:

```text
rom_operator_bridge_session=<value>; Path=/; Max-Age=14400; HttpOnly; SameSite=Strict
```

Keep one active operator session and the existing session TTL.

## Static CSP

Replace the hard-coded `STATIC_CSP` with a generated policy derived from the
configured public origin.

For `http://tailrombridge.birb.homes`, expected CSP:

```text
default-src 'self'; connect-src 'self' ws://tailrombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'
```

For `https://rombridge.birb.homes`, keep:

```text
default-src 'self'; connect-src 'self' wss://rombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'
```

If multiple allowed origins are supported later, include only the WebSocket
origins that correspond to the selected public origin for that process. Avoid
one static CSP that authorizes unrelated deployment hosts.

## Runtime Headers

Change `apply_runtime_headers` so it can use a dynamic origin string:

```rust
fn apply_runtime_headers(headers: &mut HeaderMap, origin: Option<&str>)
```

Use `HeaderValue::from_str` with sanitized config values. Fail config loading
early if an origin cannot become a header value.

Static responses do not need CORS, but they do need the generated CSP and the
existing no-store, referrer, frame, and nosniff headers.

## UI Changes

The UI WebSocket client already chooses `ws:` for HTTP pages and `wss:` for
HTTPS pages. Keep same-origin runtime config paths.

Expected UI work:

- add tests proving `RuntimeWebSocketClient` creates
  `ws://tailrombridge.birb.homes/ws/input` when `location.protocol` is `http:`;
- update security-header tests to cover both generated CSP variants;
- verify that login and subsequent API calls use `credentials: "same-origin"`;
- do not introduce absolute runtime URLs in `runtime-config.json`.

## Service Tests

Add focused tests before broad quality gates:

- HTTPS default still accepts only `https://rombridge.birb.homes`.
- HTTPS default still emits `Secure`.
- Tailscale HTTP config accepts only `http://tailrombridge.birb.homes`.
- Tailscale HTTP config emits no `Secure` attribute.
- Tailscale HTTP config returns the HTTP origin in
  `Access-Control-Allow-Origin`.
- wrong, absent, and `null` origins fail in both modes.
- static CSP changes between HTTPS and HTTP modes.
- WebSocket handshake accepts the configured HTTP Origin with a valid cookie.
- WebSocket handshake rejects HTTPS, absent, `null`, and unrelated origins in
  Tailscale mode.

Run at least:

```sh
cargo test --manifest-path service/Cargo.toml --test auth
cargo test --manifest-path service/Cargo.toml --test ws_events
cargo test --manifest-path service/Cargo.toml --test service
npm --prefix ui test -- --run tests/runtimeClient.test.ts tests/securityHeaders.test.ts
```
