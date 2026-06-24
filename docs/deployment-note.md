# Deployment Note

Date: 2026-06-24

This note records the deployment contract for the private ROM operator bridge.
It is a handoff document, not evidence that the service has been deployed. Do
not paste live command output, private network values, credentials, request
bodies, capture ids, or validation report excerpts into this file.

## Status

The selected deployment shape is a dedicated same-network HTTPS origin:

```text
https://rombridge.birb.homes/
```

DNS for `rombridge.birb.homes` is already configured to resolve to
`<bridge-private-ip>`. The bridge service, TLS route, reverse-proxy manifest,
systemd unit, release artifact, and private env file are deployment outputs and
are not present in this repository yet.

The older static-only publishing shape under `https://birb.homes/rom-bridge/`
is not the Phase 0 runtime target. It remains only a fallback static path shape;
it must not host the runtime API unless a later bead deliberately changes the
Origin/CORS and proxy plan.

## Route Contract

| Surface | Public route |
| --- | --- |
| Static UI | `https://rombridge.birb.homes/` |
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

The repo ingress manifest path is reserved for the later deployment bead. Until
that file exists and is applied, the hostname should be treated as unavailable
for bridge operation.

## Service Install Paths

```text
repo systemd unit: deploy/systemd/rom-operator-bridge.service
installed systemd unit: /etc/systemd/system/rom-operator-bridge.service
private env file: /etc/rom-operator-bridge/rom-operator-bridge.env
current release symlink: /opt/rom-operator-bridge/current
previous release symlink: /opt/rom-operator-bridge/previous
service binary: /opt/rom-operator-bridge/current/rom-operator-bridge
```

The private env file is the credential source for the operator credential and
session secret. It must be mode `0600`, must stay out of source control, and
must not be copied into shared logs or public handoff text.

## Auth, TTL, And Rotation

- Allowed browser origin: `https://rombridge.birb.homes`.
- Reject absent, `null`, and unrelated browser `Origin` values.
- Do not use wildcard CORS with credentials.
- Authenticate HTTP runtime routes and WebSocket handshakes.
- Credentials are accepted only in the session-start request body; never in URLs.
- Auth uses `HttpOnly; Secure; SameSite=Strict` cookies scoped to `/`.
- Default session TTL is 4 hours.
- MVP concurrency is one active operator session.

Credential rotation procedure:

1. Generate a new operator credential outside source control.
2. Update `/etc/rom-operator-bridge/rom-operator-bridge.env`.
3. Rotate session-signing or cookie secrets if applicable.
4. Clear or expire active session state.
5. Restart `rom-operator-bridge.service`.
6. Confirm the old credential fails and the new credential works.

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

Rollback service artifact:

```sh
sudo ln -sfn /opt/rom-operator-bridge/previous /opt/rom-operator-bridge/current
sudo systemctl restart rom-operator-bridge.service
```

If the systemd unit changes during a deployment, run `sudo systemctl
daemon-reload` before restart.

## Deployment Checks

Run these only after the service artifact, private env file, systemd unit, and
proxy manifest exist:

```sh
getent hosts rombridge.birb.homes
curl -I --resolve rombridge.birb.homes:443:<bridge-private-ip> https://rombridge.birb.homes/
curl -i -H 'Origin: https://example.invalid' https://rombridge.birb.homes/api/session
curl -i https://rombridge.birb.homes/api/session
curl -I https://rombridge.birb.homes/api/session
```

Expected sanitized results:

- Hostname resolves to `<bridge-private-ip>`.
- TLS is served for `rombridge.birb.homes`.
- Unrelated origins are rejected.
- Unauthenticated API requests are rejected without private details.
- Runtime responses include `Cache-Control: no-store`.
