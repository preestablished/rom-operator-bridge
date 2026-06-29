# Tailscale HTTP Bridge Overview

## Target

Expose the ROM operator bridge at:

```text
http://tailrombridge.birb.homes/
```

The name already resolves to the host's Tailscale address. Keep the concrete
Tailscale address out of committed docs, bead notes, and chat transcripts; use
`<tailscale-ip>` in repo material.

The requested route is intentionally plain HTTP. Treat this as a tailnet-only
operator route, not as a public Internet route. Tailscale supplies the network
encryption and access control. The browser origin remains insecure HTTP, so the
bridge must explicitly support that mode instead of reusing the HTTPS-only
cookie and Origin assumptions.

## Recommendation

Keep the current Rust service and TypeScript/Vite UI. The existing stack already
serves the static UI, `/api/...`, `/ws/...`, auth, private-root writes, capture
state, and redaction controls. Replacing it with Flutter, Java, or Tauri would
not remove the backend work required for HTTP Origin and cookie handling.

Recommended topology:

```text
tailnet browser
  -> http://tailrombridge.birb.homes:80
  -> reverse proxy listening only on <tailscale-ip>:80
  -> rom-operator-bridge on 127.0.0.1:7410
```

This keeps the bridge process off the tailnet interface while still presenting
the operator URL on the Tailscale name. If the existing validator must keep
port `7410`, only the proxy listens on port `80`.

## Required Product Changes

- Make allowed browser origins configurable rather than hard-coding
  `https://rombridge.birb.homes`.
- Make session cookie `Secure` configurable and reject unsafe combinations.
- Generate static CSP from the configured public origin so `ws://` is allowed
  for the Tailscale HTTP route.
- Echo the accepted request Origin on runtime responses instead of always
  returning the HTTPS origin.
- Add a deployment validation path for HTTP over Tailscale. Do not weaken the
  existing HTTPS deployment checks for `rombridge.birb.homes`.
- Add sanitized docs for the Tailscale route, rollback, and evidence boundary.

## End State

The future agent should leave the repository and host in this state:

- `http://tailrombridge.birb.homes/` serves the bridge UI from a tailnet client.
- Runtime API paths are same-origin under
  `http://tailrombridge.birb.homes/api/...`.
- WebSockets use `ws://tailrombridge.birb.homes/ws/events` and
  `ws://tailrombridge.birb.homes/ws/input`.
- The bridge accepts `Origin: http://tailrombridge.birb.homes` and rejects
  absent, `null`, HTTPS-only, and unrelated origins for browser runtime routes.
- HTTP mode sets `HttpOnly; SameSite=Strict` session cookies without `Secure`.
- HTTPS mode still sets `HttpOnly; Secure; SameSite=Strict` cookies.
- Only the Tailscale route uses insecure cookies. The current HTTPS route keeps
  the stricter behavior.
- Raw private evidence, credentials, cookies, endpoint addresses, capture IDs,
  screenshots, and logs stay outside the repository.

## Non-Goals

- Do not expose the bridge on a public non-Tailscale interface.
- Do not add TLS to `tailrombridge.birb.homes`; the requested route is HTTP.
- Do not remove or weaken the existing `https://rombridge.birb.homes/` route.
- Do not replace the backend stack unless the implementation cannot satisfy
  HTTP cookie, Origin, and WebSocket requirements in the current service.
- Do not commit instantiated Nginx, Traefik, systemd, or env files containing
  concrete private addresses or secrets.

## File Map

| File | Purpose |
|---|---|
| `00-overview.md` | Target, recommendation, required changes, end state |
| `01-current-state-and-decisions.md` | Current assumptions and design choices |
| `02-service-auth-ui-changes.md` | Rust service, cookie, Origin, CSP, UI changes |
| `03-routing-and-host-deployment.md` | HTTP proxy, systemd/env, firewall, rollback |
| `04-validation-and-evidence.md` | Tests, private validation, redaction, r77 use |
| `05-tech-stack-options.md` | When to stay Rust/Vite or switch to Tauri/Java/Flutter |
| `06-closeout.md` | Beads, docs, quality gates, commit and push protocol |
