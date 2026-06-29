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
ROM_BRIDGE_TAILSCALE_COOKIE_FILE=<private-cookie-file>
ROM_BRIDGE_TAILSCALE_NETWORK_EVIDENCE_FILE=<private-network-evidence-file>
ROM_BRIDGE_TAILSCALE_NETWORK_EVIDENCE_REVIEWED=1
ROM_BRIDGE_FORBID_FILE=<private-forbid-file>
```

The script should reject repo-local validation directories, symlinked private
inputs, and files with unsafe modes, following the existing deployment checker
style.

## Expected HTTP Route Checks

The checker should prove:

- DNS resolves `tailrombridge.birb.homes` to the expected Tailscale class of
  address without printing the concrete address;
- `http://tailrombridge.birb.homes/` returns the static UI;
- no HTTPS redirect is returned;
- no `Strict-Transport-Security` header is returned by the tail route;
- static responses include no-store, referrer, frame, nosniff, and HTTP-mode
  CSP headers;
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
- bridge listeners are not wildcard and the upstream `7410` port is not
  reachable from another tailnet client;
- outside-network access is unavailable or rejected.

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
