# ROM Bridge Deployment And Security Shape

Date: 2026-06-23
Agent: Codex / Ralph iteration 1

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

Preferred implementation target:

```text
127.0.0.1:<bridge-port>
```

Use that bind address if a host-local reverse proxy terminates TLS and proxies to
the bridge process on the same host.

If the deployment follows the current K3s Traefik external-endpoint pattern, the
bridge process must instead either run as a Kubernetes Service or bind only to the
trusted host interface that Traefik can reach:

```text
10.0.0.106:<bridge-port>
```

The deployment note must record the exact port and which of these two bind shapes
was used. Do not bind the bridge to `0.0.0.0` unless a later deployment note
documents the firewall and trusted-network controls that make that acceptable.

## Origin And CORS Allowlist

Runtime HTTP and WebSocket requests allow only:

```text
https://rombridge.birb.homes
```

Reject unrelated origins, including `https://example.invalid`. Do not allow
credentials from wildcard origins. Add `https://birb.homes` only if a later
transition deliberately serves the UI from the legacy `/rom-bridge/` path.

## Runtime Cache Policy

Every runtime route, private preview route, and WebSocket handshake path must
emit or enforce:

```text
Cache-Control: no-store
Pragma: no-cache
X-Content-Type-Options: nosniff
```

Operational `index.html` and runtime config should be `no-store` or `no-cache`.
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
- Rotation shape: generate a new credential, update the private secret source,
  restart the bridge service, invalidate existing sessions, then confirm old
  credentials fail and new credentials work.

## Restart And Rollback Commands

These commands are notes for the later deployment bead. They were not run during
Phase 0 discovery.

Bridge service restart:

```sh
sudo systemctl restart rom-operator-bridge.service
```

Bridge service rollback/stop:

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

The final deployment note must replace `<bridge-port>` and
`<rombridge-ingress.yaml>` with exact values.

## Deployment Blockers For Later Beads

- No committed `rombridge.birb.homes` Traefik/Ingress/Apache/Caddy config exists
  yet in this repo.
- No bridge service exists yet, so the exact runtime port is still a later
  implementation decision.
- Publish/deploy commands must remain blocked until the redaction scan, auth
  rejection, origin rejection, no-store header checks, and browser no-persistence
  checks are green.
