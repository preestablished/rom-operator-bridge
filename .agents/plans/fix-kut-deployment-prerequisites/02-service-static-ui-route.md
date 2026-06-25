# Service Static UI Route

## Why This Is Required

The bridge service currently routes `/health`, `/api/...`, and `/ws/...`, then
returns a sanitized not-found response for everything else. The deployment
checks require the browser-facing root and assets to be served at
`https://rombridge.birb.homes/`, and the mixed-content check cannot pass while
the root route returns a default 404.

The future agent must either add static UI serving to the bridge service or
deploy a separate static-file service behind the same trusted HTTPS origin. The
recommended repo-local fix is to serve the built UI from the Rust service when
`ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT` is configured.

## Recommended Implementation Shape

1. Extend `BridgePrivateConfig` to retain the optional static publish root after
   validation.
2. Add an accessor such as `static_publish_root()` that returns an optional
   path.
3. Include the static publish root in the public sanitizer as a private root or
   forbidden path source so public errors cannot leak it.
4. In `api::router`, keep the existing exact routes for `/health`, `/api/...`,
   and `/ws/...` ahead of any static fallback.
5. If a static publish root is configured, serve `index.html`,
   `runtime-config.json`, and asset files from that directory.
6. For SPA fallback, return `index.html` for browser navigation paths that are
   not API, WSS, or health routes.
7. If no static publish root is configured, preserve the existing sanitized
   not-found behavior.

Use an established Axum/Tower static-file helper if it keeps the implementation
small and testable. If a new dependency is added, keep it narrowly scoped and
covered by tests.

## Header Requirements

Static UI responses from the deployment origin must include:

- `X-Content-Type-Options: nosniff`;
- `Referrer-Policy: no-referrer`;
- `X-Frame-Options: DENY`;
- a CSP compatible with `connect-src 'self' wss://rombridge.birb.homes`;
- `Cache-Control: no-store` for `index.html` and `runtime-config.json`.

Until a later bead deliberately allows cacheable immutable assets, prefer
`Cache-Control: no-store` for all files served by the bridge route. This is
conservative and keeps `kut` focused on proving no private browser persistence.

Runtime API and WebSocket behavior must remain unchanged:

- unauthenticated runtime routes reject without private details;
- wrong, absent, and `null` browser Origins are rejected where required;
- runtime responses include no-store and no-cache headers;
- WSS handshakes require authenticated session plus allowed Origin.

## UI Build Output

Before deploying static files, build the UI through the existing gate:

```sh
npm --prefix ui ci
npm --prefix ui run build
```

The deployed static publish root should receive the contents of `ui/dist/`, not
source files. Do not commit host-specific static publish output unless it is
already part of the repo's normal build artifact policy.

## Tests To Add Or Update

Add focused tests for:

- configured static root serves `/` as HTML;
- configured static root serves `/runtime-config.json` with no-store headers;
- unknown browser paths fall back to `index.html`;
- `/api/...`, `/ws/...`, and `/health` are not shadowed by static fallback;
- no static-root path appears in public errors or debug output;
- missing static root keeps the existing sanitized not-found behavior.

If static serving is implemented by a separate service instead of Rust code,
document why that choice is safer and add deployment-level checks proving the
same headers and route separation.

## Acceptance For This Step

This step is complete when a local configured static root can serve the built UI
without weakening API/WSS auth, Origin rejection, runtime headers, or public
sanitization.
