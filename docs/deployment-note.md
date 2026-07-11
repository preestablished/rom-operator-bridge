# Deployment Note

Date: 2026-06-26

This note records the deployment contract for the private ROM operator bridge.
It is also the sanitized handoff record for the deployed same-origin route. Do
not paste live command output, private network values, credentials, request
bodies, capture ids, or validation report excerpts into this file.

## Status

The selected deployment shape is a dedicated same-network HTTPS origin:

```text
https://rombridge.birb.homes/
```

Exact operator URL:

```text
https://rombridge.birb.homes/
```

DNS for `rombridge.birb.homes` resolves to `<bridge-private-ip>`. The bridge
service, static UI, TLS route, systemd unit, K3s ingress/service/endpoints, and
private env file were deployed and validated from the trusted network.

Sanitized evidence label:

```text
deployment-network-kut/20260626T212016Z
```

That private validation proved the deployed route serves the static UI over
HTTPS, runtime API routes work without mixed-content errors, both WebSocket
routes enforce authenticated same-origin WSS handshakes, unrelated Origins are
rejected, and no-store headers are present on runtime/private preview routes.
Operator-private raw evidence remains outside this repository.

The older static-only publishing shape under `https://birb.homes/rom-bridge/`
is not the Phase 0 runtime target. It remains only a fallback static path shape;
it must not host the runtime API unless a later bead deliberately changes the
Origin/CORS and proxy plan.

## Route Contract

| Surface | Public route |
| --- | --- |
| Static UI | `https://rombridge.birb.homes/` |
| Static-only fallback publish path | `https://birb.homes/rom-bridge/` (no runtime API) |
| Runtime API | `https://rombridge.birb.homes/api/...` |
| WebSockets | `wss://rombridge.birb.homes/ws/...` |

The runtime API includes `/api/session`, `/api/run/...`, `/api/frame/...`,
`/api/capture/...`, and `/api/labels`. WebSocket endpoints are `/ws/input` and
`/ws/events`.

## Bind And Proxy

Runtime bind address:

```text
<bridge-private-ip>:7410
```

This is a documented trusted-interface bind, not a localhost bind. The service
must not bind to `0.0.0.0`. A loopback bind such as `127.0.0.1:7410` is allowed
only if a later deployment changes the topology to a host-local reverse proxy
that can reach loopback.

TLS termination and WebSocket upgrade handling belong at the K3s
Traefik/cert-manager edge for `rombridge.birb.homes`. The proxy must enforce
Host/SNI routing for `rombridge.birb.homes` only, so the bridge is not served
under another host that resolves to the same private address.

Proxy config paths:

```text
repo ingress manifest: deploy/k8s/rombridge-ingress.yaml
k3s kubeconfig: /etc/rancher/k3s/k3s.yaml
```

The repo ingress manifest is the sanitized template. The operator-private
endpoint manifest supplies the concrete trusted address outside the repository.

## Service Install Paths

```text
repo systemd unit: deploy/systemd/rom-operator-bridge.service
installed systemd unit: /etc/systemd/system/rom-operator-bridge.service
private env file: /etc/rom-operator-bridge/rom-operator-bridge.env
current release symlink: /opt/rom-operator-bridge/current
previous release symlink: /opt/rom-operator-bridge/previous
service binary: /opt/rom-operator-bridge/current/rom-operator-bridge
```

The private env file is the source for the session secret. It must be mode `0600`, must stay out of source control, and
must not be copied into shared logs or public handoff text.

## Auth, TTL, And Rotation

- Allowed browser origin: `https://rombridge.birb.homes`.
- Reject absent, `null`, and unrelated browser `Origin` values.
- Do not use wildcard CORS with credentials.
- If responses vary by request `Origin`, include `Vary: Origin`.
- Authenticate HTTP runtime routes and WebSocket handshakes.
- Password-based operator auth is not accepted; credentials are never accepted in URLs.
- Auth uses `HttpOnly; Secure; SameSite=Strict` cookies scoped to `/`.
- Default session TTL is 4 hours.
- MVP concurrency is one active operator session.
- Return sanitized public auth errors without credentials, private paths, stack
  traces, host-control details, or artifact identifiers.

Session secret rotation procedure:

1. Generate a new session secret outside source control.
2. Update `/etc/rom-operator-bridge/rom-operator-bridge.env` with the new
   session secret.
3. Clear or expire active session state.
4. Restart `rom-operator-bridge.service`.
5. Confirm old sessions fail.

## Headers And Cache Policy

Runtime API routes, private preview routes, private status routes, and WebSocket
handshake responses must include or enforce:

```text
Cache-Control: no-store
Pragma: no-cache
X-Content-Type-Options: nosniff
```

The static UI route must include at least:

```text
Content-Security-Policy: default-src 'self'; connect-src 'self' wss://rombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'
Referrer-Policy: no-referrer
X-Frame-Options: DENY
X-Content-Type-Options: nosniff
```

`index.html` and runtime config must be `Cache-Control: no-store`. Hashed
static assets may be cacheable only after the redaction gate passes and only if
they contain no runtime state, private paths, capture ids, credentials, or
source maps with private local paths.

## Restart And Rollback

Service restart:

```sh
sudo systemctl daemon-reload
sudo systemctl restart rom-operator-bridge.service
```

Post-restart verification, inspected privately:

```sh
sudo systemctl status --no-pager rom-operator-bridge.service
sudo journalctl -u rom-operator-bridge.service -n 50 --no-pager
curl -fsS http://<bridge-private-ip>:7410/health
```

Do not paste service status or journal output into shared handoff text unless it
has been sanitized for credentials, private paths, host-control details, and
artifact refs.

For the real backend, also confirm the numeric lease-reconciliation summary
reports `ready_for_real_sessions=true`. A dangling intent is not repaired by a
bridge restart or worker restart alone; follow the stopped-bridge, worker
restart, full-capacity verification, and selected-intent acknowledgement
procedure in `docs/operator-runbook.md`.

Emergency shutdown:

```sh
sudo systemctl stop rom-operator-bridge.service
```

Apply proxy route:

```sh
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl apply -f deploy/k8s/rombridge-ingress.yaml
```

Rollback proxy route:

```sh
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl delete -f deploy/k8s/rombridge-ingress.yaml
```

Before changing the private env file during a deployment, save a private backup:

```sh
sudo install -m 0600 -o root -g root \
  /etc/rom-operator-bridge/rom-operator-bridge.env \
  /etc/rom-operator-bridge/rom-operator-bridge.env.previous
```

Rollback service artifact and private env file:

```sh
sudo ln -sfn /opt/rom-operator-bridge/previous /opt/rom-operator-bridge/current
sudo install -m 0600 -o root -g root \
  /etc/rom-operator-bridge/rom-operator-bridge.env.previous \
  /etc/rom-operator-bridge/rom-operator-bridge.env
sudo systemctl daemon-reload
sudo systemctl restart rom-operator-bridge.service
```

If the private env file did not change during the failed deployment, the env
restore step can be skipped. Do not copy env file contents into shared logs or
handoff text.

An artifact rollback to a release that predates durable `leases/` awareness can
leak a newly allocated worker slot. Quiesce real sessions and verify full
worker capacity before such a rollback; prefer a forward fix.

## Deployment Checks

The live deployment passed the full private checker under sanitized evidence
label `deployment-network-kut/20260626T212016Z`. The following command shapes
are for revalidation after changing the service artifact, private env file,
systemd unit, static root, or proxy manifest:

```sh
getent hosts rombridge.birb.homes
curl -I --resolve rombridge.birb.homes:443:<bridge-private-ip> https://rombridge.birb.homes/
curl -i https://rombridge.birb.homes/api/session
curl -I https://rombridge.birb.homes/api/session
curl -i \
  -H 'Origin: https://example.invalid' \
  -H 'Cookie: <redacted-valid-session-cookie>' \
  https://rombridge.birb.homes/api/session
websocat \
  -H 'Origin: https://rombridge.birb.homes' \
  -H 'Cookie: <redacted-valid-session-cookie>' \
  wss://rombridge.birb.homes/ws/events
```

Expected sanitized results:

- Hostname resolves to `<bridge-private-ip>`.
- TLS is served for `rombridge.birb.homes`.
- Unauthenticated API requests are rejected without private details.
- Runtime responses include `Cache-Control: no-store`.
- Wrong-origin authenticated requests are rejected before serving session state.
- WebSocket upgrade succeeds only with an authenticated session and allowed
  Origin.
