# ROM Bridge Deployment And Security Shape

Date: 2026-06-23
Agent: Codex / Ralph iteration 1

## Private Operations Note

This note contains local hostnames, network addresses, absolute local paths, and
deployment command shapes. Do not publish it outside the trusted operator
environment without sanitizing hostnames, IP addresses, local paths, ports, and
command transcripts.

## Decision

Use the dedicated same-network HTTPS origin `https://rombridge.birb.homes/`
for the private operator bridge.

The operator added DNS for `rombridge.birb.homes` pointing at `10.0.0.106`.
Local resolution confirms:

```text
10.0.0.106      rombridge.birb.homes
```

This avoids the mixed-content risk called out in the initial plan while avoiding
the need to share the existing `https://birb.homes/plans/<slug>/` static-publish
surface with a runtime control API.

Only DNS exists at this point. This branch does not deploy a bridge service,
configure TLS, or install a proxy route for `rombridge.birb.homes`. Until the
later deployment bead creates the route and smoke checks pass, the hostname
should be treated as unavailable for bridge operation and may return default
virtual-host behavior.

## URLs

Static UI:

```text
https://rombridge.birb.homes/
```

Runtime API:

```text
https://rombridge.birb.homes/api/...
```

WebSocket:

```text
wss://rombridge.birb.homes/ws/...
```

The earlier plan path remains useful as a fallback shape, but it is not the
chosen Phase 0 target:

```text
https://birb.homes/rom-bridge/
https://birb.homes/rom-bridge/api/...
wss://birb.homes/rom-bridge/ws/...
```

## Evidence

- `06-hosting-on-birb-homes.md` documents static publishing under
  `https://birb.homes/plans/<slug>/` and says that helper only publishes static
  files.
- `13-deployment-security-checklist.md` requires HTTPS, WSS, Origin checks,
  no-store runtime headers, credential rotation, and rollback/restart notes.
- `/home/infra-admin/.agents/projects/forgejo/README.md` records the current
  local service-style HTTPS pattern as K3s Traefik plus cert-manager.
- `/home/infra-admin/gitea/k8s-forgejo-ingress.yaml` shows the active pattern
  for routing a service on `10.0.0.106` through Traefik with TLS.
- Repository and planning-doc searches found no existing committed
  `rombridge.birb.homes` or `/rom-bridge` route before this note.

## Service Bind Address

Chosen Phase 0 target for the dedicated hostname:

```text
10.0.0.106:<bridge-port>
```

Use K3s Traefik/cert-manager as the HTTPS/WSS edge, following the current local
service-style pattern recorded for Forgejo. The Traefik route must match only
`rombridge.birb.homes` and proxy to the bridge service on the trusted host
interface above, or to an equivalent Kubernetes Service if the bridge is deployed
inside the cluster.

Do not bind the bridge to `0.0.0.0`. Do not use a `127.0.0.1:<bridge-port>`
service bind for the dedicated-hostname deployment unless the later deployment
bead also replaces the Traefik external-endpoint shape with a host-local reverse
proxy that can reach loopback.

The exact port is unresolved because the bridge service does not exist yet. The
deployment bead must fill in `<bridge-port>` before any publish/deploy step.

## Origin And CORS Allowlist

Runtime HTTP and WebSocket requests allow only:

```text
https://rombridge.birb.homes
```

Reject unrelated origins, including `https://example.invalid`. Do not allow
credentials from wildcard origins. Add `https://birb.homes` only if a later
transition deliberately serves the UI from the legacy `/rom-bridge/` path.

For browser-originated runtime API and WebSocket requests, reject absent, `null`,
and wrong `Origin` values unless a later local CLI/admin endpoint explicitly
documents a non-browser exception. If responses vary by request origin, include
`Vary: Origin`.

The proxy route must also enforce Host/SNI routing for `rombridge.birb.homes` so
the bridge is not accidentally served under another host that resolves to
`10.0.0.106`.

## Runtime Cache Policy

Every runtime route, private preview route, and WebSocket handshake path must
emit or enforce:

```text
Cache-Control: no-store
Pragma: no-cache
X-Content-Type-Options: nosniff
```

Operational `index.html` and runtime config must be `Cache-Control: no-store`.
Hashed static assets may be cacheable only after the redaction scan passes and
only if they contain no runtime state, private paths, screenshots, capture ids,
credentials, or source maps with private local paths.

## SPA Security Headers

The HTTPS route for the UI must include at least:

```text
Content-Security-Policy: default-src 'self'; connect-src 'self' wss://rombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'
Referrer-Policy: no-referrer
X-Frame-Options: DENY
X-Content-Type-Options: nosniff
```

Do not add a service worker unless it explicitly excludes all runtime API,
WebSocket, preview, capture, validation, and private artifact routes from caching.

## Auth And Credential Rotation

- Store the operator credential outside source control, in a private env file,
  secret manager, or systemd environment file.
- Do not accept credentials in URLs.
- Prefer `HttpOnly; Secure; SameSite=Strict` cookie auth scoped to `/`.
- Authenticate HTTP and WebSocket handshakes.
- Default session TTL: 4 hours.
- MVP concurrency: one active operator session.
- Rate-limit failed auth attempts, log auth failures only to private service logs,
  and return sanitized public auth errors without credential, path, or stack
  details.
- Rotation shape: generate a new credential, update the private secret source,
  rotate session-signing/cookie secrets if applicable, clear or expire the active
  session store, restart the bridge service, invalidate existing sessions, then
  confirm old credentials fail and new credentials work.

## Restart And Rollback Commands

These commands are notes for the later deployment bead. They were not run during
Phase 0 discovery.

Bridge service restart:

```sh
sudo systemctl restart rom-operator-bridge.service
```

Emergency bridge service shutdown:

```sh
sudo systemctl stop rom-operator-bridge.service
```

K3s Traefik route apply, if that deployment shape is chosen:

```sh
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl apply -f <rombridge-ingress.yaml>
```

K3s Traefik route rollback:

```sh
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl delete -f <rombridge-ingress.yaml>
```

Service artifact rollback remains a deployment-bead blocker until the service
unit, binary/artifact location, and private env file path exist. The deployment
note must replace `<bridge-port>` and `<rombridge-ingress.yaml>` with exact
values and add the concrete service rollback command, such as restoring the
previous service artifact and env file, running `systemctl daemon-reload` if the
unit changed, and restarting `rom-operator-bridge.service`.

## Future Deployment Checks

These command shapes are for the later deployment bead. They were not run during
Phase 0 discovery because no bridge service or proxy route exists yet.

DNS and TLS route:

```sh
getent hosts rombridge.birb.homes
curl -I --resolve rombridge.birb.homes:443:10.0.0.106 https://rombridge.birb.homes/
```

Origin rejection:

```sh
curl -i -H 'Origin: https://example.invalid' https://rombridge.birb.homes/api/session
```

Unauthenticated rejection and no-store headers:

```sh
curl -i https://rombridge.birb.homes/api/session
curl -I https://rombridge.birb.homes/api/session
```

The expected results are: hostname resolves to `10.0.0.106`, TLS is served only
for `rombridge.birb.homes`, unrelated origins are rejected, unauthenticated API
requests are rejected without private details, and runtime responses include
`Cache-Control: no-store`.

## Deployment Blockers For Later Beads

- No committed `rombridge.birb.homes` Traefik/Ingress/Apache/Caddy config exists
  yet in this repo.
- No bridge service exists yet, so the exact runtime port is still a later
  implementation decision.
- Publish/deploy commands must remain blocked until the redaction scan, auth
  rejection, origin rejection, no-store header checks, and browser no-persistence
  checks are green.
