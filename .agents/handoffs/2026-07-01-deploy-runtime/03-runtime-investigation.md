# Runtime Investigation So Far

## User-Visible Symptom

After the UI became visible, the user reported:

```text
Session
faulted
synthetic
Runtime unavailable.
```

This is now tracked by bead `rom-operator-bridge-xta`.

## API Probes

Unauthenticated API probes without an `Origin` header return `origin_rejected`,
which is expected. With the Tailscale origin header:

```sh
curl -i \
  -H 'Host: tailrombridge.birb.homes' \
  -H 'Origin: http://tailrombridge.birb.homes' \
  http://10.0.0.106:7410/api/session
```

Result:

```json
{"schema_version":1,"error":{"code":"session_inactive","message":"Session inactive.","retryable":false,"details":{}}}
```

Through the public Tailscale URL:

```sh
curl -i \
  -H 'Origin: http://tailrombridge.birb.homes' \
  http://tailrombridge.birb.homes/api/session
```

Result: same `401 session_inactive`.

Through the primary HTTPS URL:

```sh
curl -k -i \
  -H 'Origin: https://rombridge.birb.homes' \
  https://rombridge.birb.homes/api/session
```

Result: same `401 session_inactive`.

## Start Session Probe

A direct API start probe against the Tailscale URL succeeded in real mode:

```text
HTTP/1.1 200 OK
session_id: real-session-0000
run_id: real-run-0000
state: paused
capabilities.input: true
capabilities.preview: true
capabilities.capture: false
capabilities.labels: false
capabilities.privileged_features: false
```

That probe-created session was stopped immediately afterward via
`/api/session/stop`, and `/api/session` returned `session_inactive` again.

Do not reuse or record the probe cookie. It was intentionally omitted from this
handoff.

## Current Interpretation

The backend service itself appears healthy:

- `/health` returns `backend_mode: real`.
- `/api/session` returns structured JSON when addressed through the correct
  Host/Origin.
- `/api/session/start` can create a real paused session.

Therefore, the reported `faulted synthetic Runtime unavailable` is likely one
of these:

1. Browser was loaded through the bare Tailscale IP or another host that routes
   `/api/session` to Apache/HTML, causing the frontend fetch/json parse to fall
   into `backend_unavailable`.
2. Browser had stale app state or stale static assets from before the final
   deployment/profile update.
3. Frontend startup handles a transient network/error path poorly and leaves
   the initial `synthetic` fallback model visible with `faulted`.
4. A browser-only issue such as mixed origin, cookie, or service worker/cache
   behavior. No service worker has been confirmed, but browser DevTools or
   Playwright should be used next.

Relevant frontend code:

- `ui/src/authSession.ts`
  - `stateFromRuntimeError()` maps non-`RuntimeApiError` failures to
    `faulted` with `Runtime unavailable.`
  - `initialRuntimeSessionModel()` defaults to `backend_mode: "synthetic"`.
- `ui/src/app.ts`
  - Startup calls `refreshSession(auth, client)` after first render.
  - `refreshServiceBackendMode()` updates the backend mode from `/health` only
    when starting a session, not necessarily before the first visible startup
    state.
- `ui/src/runtimeClient.ts`
  - Fetch/network failures and JSON parse failures become
    `backend_unavailable`.

