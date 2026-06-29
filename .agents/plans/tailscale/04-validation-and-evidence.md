# Validation And Evidence

## Local Quality Gates

After service and UI changes, run:

```sh
cargo fmt --manifest-path service/Cargo.toml
cargo test --manifest-path service/Cargo.toml
npm --prefix ui ci
npm --prefix ui run typecheck
npm --prefix ui test
npm --prefix ui run build
bash scripts/redaction-gate.sh
```

If the change is large, run the repository gate:

```sh
bash scripts/quality-gate.sh
```

Also run:

```sh
git diff --check
```

## Tailscale HTTP Validation Script

Add a focused checker, preferably:

```text
scripts/tailscale-http-check.sh
```

Inputs:

```sh
ROM_BRIDGE_TAILSCALE_BASE_URL=http://tailrombridge.birb.homes
ROM_BRIDGE_TAILSCALE_ORIGIN=http://tailrombridge.birb.homes
ROM_BRIDGE_TAILSCALE_VALIDATION_DIR=<private-validation-dir>/tailscale-http/<run-id>
ROM_BRIDGE_TAILSCALE_START_SESSION_JSON=<private-start-session-json>
ROM_BRIDGE_TAILSCALE_SESSION_RESPONSE=<private-session-response-json>
ROM_BRIDGE_TAILSCALE_COOKIE_FILE=<private-cookie-file-written-by-checker>
ROM_BRIDGE_TAILSCALE_NETWORK_EVIDENCE_FILE=<private-network-evidence-file>
ROM_BRIDGE_TAILSCALE_NETWORK_EVIDENCE_REVIEWED=1
ROM_BRIDGE_FORBID_FILE=<private-forbid-file>
```

The script should reject repo-local validation directories, symlinked private
inputs, and files with unsafe modes, following the existing deployment checker
style.

The checker must create or refresh its own throwaway HTTP session. Either
parameterize `scripts/prepare-deployment-validation-inputs.py` for HTTP, port
`80`, and `tailrombridge.birb.homes`, or create
`scripts/prepare-tailscale-http-validation-inputs.py`. The prep path must:

- send `Host: tailrombridge.birb.homes`;
- send `Origin: http://tailrombridge.birb.homes`;
- store raw headers and response bodies only under the private validation
  directory;
- require a `rom_operator_bridge_session` cookie on 2xx login;
- assert the session cookie includes `HttpOnly` and `SameSite=Strict`;
- assert the session cookie omits `Secure`.

## Expected HTTP Route Checks

The checker should prove:

- DNS resolves `tailrombridge.birb.homes` to the expected Tailscale class of
  address without printing the concrete address;
- parent-domain HSTS and browser preload checks do not force HTTPS for this
  hostname, or a fallback hostname is selected before continuing;
- `http://tailrombridge.birb.homes/` returns the static UI;
- no HTTPS redirect is returned;
- no `Strict-Transport-Security` header is returned by the tail route;
- static responses include no-store, referrer, frame, nosniff, and HTTP-mode
  CSP headers;
- static CSP contains the exact `ws://tailrombridge.birb.homes` connection
  target and does not contain the old `wss://rombridge.birb.homes` target;
- `/health` is reachable and sanitized;
- unauthenticated `/api/session` returns the sanitized inactive or unauthorized
  shape expected by the current API;
- session start succeeds with the operator credential and an allowed Origin;
- session start sets the session cookie without `Secure`;
- authenticated runtime routes require `Origin: http://tailrombridge.birb.homes`;
- authenticated runtime routes reject absent, `null`, HTTPS, and unrelated
  Origins;
- `/api/frame/current/image` and capture preview routes return no-store headers;
- `/ws/events` and `/ws/input` accept authenticated same-origin `ws://`
  handshakes;
- `/ws/events` and `/ws/input` reject unauthenticated and wrong-Origin
  handshakes;
- bridge listeners are not wildcard and the bridge upstream port is not
  reachable from another tailnet client unless it is the already validated
  HTTPS upstream;
- wrong Host and direct IP-literal HTTP requests do not serve the bridge UI,
  `/health`, `/api/...`, or `/ws/...`;
- outside-network access is unavailable or rejected.

If the implementation preserves the HTTPS route, also rerun the existing
deployment-network checker or a focused equivalent for
`https://rombridge.birb.homes/` after the Tailscale route is active.

## Private Browser Smoke

From an approved tailnet client, open:

```text
http://tailrombridge.birb.homes/
```

Private smoke sequence:

- authenticate with the operator credential;
- start a real or synthetic session according to the private env mode;
- confirm run status updates;
- confirm the current-frame preview path works;
- open event and input WebSockets;
- stop the session;
- verify the browser did not upgrade the URL to HTTPS.
- verify this from a fresh browser profile or a profile with no cached HSTS
  state for the hostname.

Do not capture screenshots into the repository. If screenshots are needed, keep
them under the private validation directory and refer only to a sanitized
evidence label.

## Relationship To `rom-operator-bridge-r77`

The Tailscale route can be used to run `rom-operator-bridge-r77`, but the route
itself does not complete `r77`.

After the HTTP route is validated, `r77` still requires private real backend
evidence:

- start a real backend session through `http://tailrombridge.birb.homes/`;
- trigger one real capture from the UI;
- confirm the private capture index row exists;
- add a `needs_review` label;
- stop the session;
- record only sanitized pass/fail status and an approved private evidence label.

Do not mark `r77` complete from synthetic-only Tailscale validation.

## Docs To Update

Add or update sanitized docs after validation:

- `docs/operator-runbook.md` with the optional Tailscale HTTP route;
- `docs/deployment-note.md` or a new `docs/tailscale-http-deployment.md`;
- `docs/runtime-api.md` cookie section for HTTPS and Tailscale HTTP modes;
- `docs/redaction.md` if a new Tailscale checker has forbid-file requirements;
- `deploy/README.md` only if the Tailscale route becomes an official deploy
  option.

Keep the HTTPS deployment docs intact. Present the Tailscale route as a separate
operator-private access path with a different trust model.

## Route-Specific Redaction Rules

The existing HTTPS publish rules treat arbitrary `http://` and `ws://` runtime
links as failures. The Tailscale route intentionally needs these exact strings:

```text
http://tailrombridge.birb.homes
ws://tailrombridge.birb.homes
```

Add route-specific scan rules instead of globally allowing insecure runtime
links. The Tailscale checker may allowlist only those exact origins in
Tailscale-mode docs or generated static output. It must still fail:

- any other `http://` or `ws://` runtime endpoint;
- `https://rombridge.birb.homes` or `wss://rombridge.birb.homes` leaking into
  the Tailscale static CSP/runtime config;
- `http://tailrombridge.birb.homes` or `ws://tailrombridge.birb.homes` leaking
  into the existing HTTPS deployment output.

## Redaction Boundary

Never commit:

- operator credential or session secret;
- cookie jar or curl config;
- concrete Tailscale IP;
- private endpoint manifest;
- real ROM paths or artifact refs;
- raw headers, service logs, packet captures, screenshots, or capture IDs.

Allowed public wording shape:

```text
Tailscale HTTP route validation passed for http://tailrombridge.birb.homes/.
Raw evidence remains private under label <private-evidence-label>. The route is
tailnet-only, uses HTTP-mode same-origin cookies, and rejects unrelated Origins.
```
