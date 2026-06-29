# Service Auth And UI Changes

## Service Config

Add deployment security settings to `ServiceConfig` or a nested config struct.
Keep defaults equivalent to today's HTTPS route so existing tests and operator
deployments remain safe.

Suggested fields:

```rust
pub struct DeploymentSecurityConfig {
    pub profiles: Vec<DeploymentProfile>,
}

pub struct DeploymentProfile {
    pub id: String,
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
- a profile's public-origin host must match the Host values that may serve its
  static UI;
- do not use one profile with mixed HTTP and HTTPS allowed origins;
- multi-route coexistence must use separate profiles, not one process-wide
  `cookie_secure=false` switch.

For the MVP, non-Secure cookies are valid only for the
`http://tailrombridge.birb.homes` profile and only when deployment validation
proves either a Tailscale-bound proxy or an explicitly reviewed direct-tailnet
listener. HTTPS origins must never inherit the non-Secure cookie setting.

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
validate_runtime_request(headers, uri, deployment_profiles)
validate_origin(headers, deployment_profiles) -> Result<RuntimeAuthContext, AuthError>
```

The auth context should carry the accepted profile and a header-safe Origin:

```rust
pub struct RuntimeAuthContext {
    pub profile_id: DeploymentProfileId,
    pub origin: HeaderValue,
    pub cookie_secure: bool,
}
```

The returned allowed origin should be used when writing CORS headers. Runtime
responses must echo the accepted request origin:

```text
Access-Control-Allow-Origin: http://tailrombridge.birb.homes
Vary: Origin
```

Do not return wildcard CORS for credentialed runtime routes.

Carry this context through all runtime handlers that currently call
`authenticate_runtime_request(...) -> Result<(), Response>`. That includes
session status, run status, frame/image routes, capture routes, labels,
validation status, pause/resume, stop, and frame-hint query paths. WebSocket
handshakes should use the same context for Origin validation and response
headers.

Keep these rejection rules:

- absent Origin is rejected for browser runtime requests;
- `Origin: null` is rejected;
- unrelated origins are rejected;
- credentials in URLs remain rejected.

## Cookie Headers

Change cookie formatting to take the accepted profile's secure policy:

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

In a multi-profile service, login should format `Set-Cookie` from the accepted
Origin profile. A login from the HTTPS origin must set `Secure`; a login from
the Tailscale HTTP origin must omit `Secure`. Do not infer this from
`X-Forwarded-Proto` alone.

## Static CSP

Replace hard-coded CSP values with generated policies derived from the selected
public origin. There are currently two CSP sources:

- the Rust service's `STATIC_CSP`;
- the Vite preview/test `SPA_RESPONSE_HEADERS` in `ui/vite.config.ts`.

Update both, or explicitly keep Vite preview on the HTTPS profile while the
service-served static route is profile-aware. Tests must document whichever
choice is made.

For `http://tailrombridge.birb.homes`, expected CSP:

```text
default-src 'self'; connect-src 'self' ws://tailrombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'
```

For `https://rombridge.birb.homes`, keep:

```text
default-src 'self'; connect-src 'self' wss://rombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'
```

For static responses, select the CSP by validated Host/public origin, not by
untrusted query string or request body. Avoid one static CSP that authorizes
both unrelated deployment hosts.

## Runtime Headers

Change `apply_runtime_headers` so it can use the accepted auth context:

```rust
fn apply_runtime_headers(headers: &mut HeaderMap, context: Option<&RuntimeAuthContext>)
```

Use `HeaderValue::from_str` with sanitized config values while loading config,
then store header-safe values in the profile/context. Fail config loading early
if an origin cannot become a header value.

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
- a multi-profile process keeps HTTPS `Secure` cookies while HTTP login omits
  `Secure`.
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
