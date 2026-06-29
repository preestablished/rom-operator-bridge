# Routing And Host Deployment

## Recommended Host Topology

Use a local reverse proxy bound to the Tailscale interface:

```text
tailnet client
  -> http://tailrombridge.birb.homes/
  -> <tailscale-ip>:80
  -> local proxy
  -> 127.0.0.1:7410
  -> rom-operator-bridge
```

This plan uses Nginx examples because HTTP-only host routing and WebSocket
upgrade forwarding are explicit. Traefik is also acceptable if the host already
uses K3s and the no-TLS `web` entrypoint is known to listen on the Tailscale
interface only.

## Private Env Shape

Install or update the private env file outside the repository:

```text
/etc/rom-operator-bridge/rom-operator-bridge.env
```

Tailscale HTTP proxy mode should use placeholders like:

```sh
ROM_OPERATOR_BRIDGE_BIND_ADDR=127.0.0.1:7410
ROM_OPERATOR_BRIDGE_BACKEND=<synthetic-or-real>
ROM_OPERATOR_BRIDGE_PRIVATE_ROOT=<absolute-private-runtime-root>
ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT=<absolute-static-release-dir>
ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL=<operator-credential>
ROM_OPERATOR_BRIDGE_SESSION_SECRET=<session-secret>
ROM_OPERATOR_BRIDGE_PUBLIC_ORIGIN=http://tailrombridge.birb.homes
ROM_OPERATOR_BRIDGE_ALLOWED_ORIGINS=http://tailrombridge.birb.homes
ROM_OPERATOR_BRIDGE_COOKIE_SECURE=false
ROM_OPERATOR_BRIDGE_EXPOSURE_MODE=tailscale-http
```

For real backend mode, keep the existing private real backend handoff values
from `docs/operator-runbook.md`. Do not paste them into this plan.

The implementation must update `scripts/validate-operator-env.py` so
`127.0.0.1:7410` is allowed only when exposure mode is `tailscale-http`. Keep
wildcard binds invalid.

## Nginx Route Shape

Create an operator-private Nginx site outside the repository first. After it is
validated, commit only a sanitized template if useful.

Sanitized shape:

```nginx
server {
    listen <tailscale-ip>:80;
    server_name tailrombridge.birb.homes;

    access_log off;

    location / {
        proxy_pass http://127.0.0.1:7410;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Host $host;
        proxy_set_header X-Forwarded-Proto http;
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        proxy_read_timeout 3600s;
        proxy_send_timeout 3600s;
    }
}
```

If using this exact Nginx style, define `connection_upgrade` in the http block:

```nginx
map $http_upgrade $connection_upgrade {
    default upgrade;
    '' close;
}
```

Do not enable TLS, redirects to HTTPS, HSTS, request buffering changes, or
response caching for this route.

## Traefik Alternative

If the operator prefers K3s/Traefik, add a separate sanitized manifest instead
of modifying the HTTPS route in place.

Expected shape:

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: rom-operator-bridge-tailscale-http
  namespace: rom-operator-bridge
  annotations:
    traefik.ingress.kubernetes.io/router.entrypoints: web
spec:
  ingressClassName: traefik
  rules:
    - host: tailrombridge.birb.homes
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: rom-operator-bridge
                port:
                  number: 7410
```

Only use this if Traefik's `web` entrypoint is bound to the Tailscale interface
or otherwise firewall-restricted to the tailnet. Do not let this add an
Internet-facing HTTP listener.

## Service Install

Keep the existing service release flow:

```sh
cargo build --manifest-path service/Cargo.toml --release
npm --prefix ui ci
npm --prefix ui run build
```

Install release artifacts as described by `deploy/README.md`, then update the
private env and restart:

```sh
sudo python3 scripts/generate-operator-auth.py \
  /etc/rom-operator-bridge/rom-operator-bridge.env
sudo python3 scripts/validate-operator-env.py \
  /etc/rom-operator-bridge/rom-operator-bridge.env
sudo systemctl daemon-reload
sudo systemctl restart rom-operator-bridge.service
```

Inspect service status and journal output privately.

## Firewall And Tailscale ACLs

The implementation must prove:

- port `80` is reachable only on the Tailscale interface;
- port `7410` is not reachable from other machines;
- no wildcard listener serves the bridge;
- tailnet ACLs allow only approved operator identities or devices;
- outside-network probes fail or are not routable.

Use private evidence files for raw listener, firewall, and ACL proof. Commit
only sanitized pass/fail summaries.

## Rollback

HTTP route rollback should be independent from private runtime data:

```sh
sudo rm -f /etc/nginx/sites-enabled/<tailscale-http-site>
sudo nginx -t
sudo systemctl reload nginx
```

If the bridge env changed, restore the previous private env backup and restart:

```sh
sudo install -m 0600 <previous-private-env-backup> \
  /etc/rom-operator-bridge/rom-operator-bridge.env
sudo systemctl restart rom-operator-bridge.service
```

If the service release changed, use the existing release symlink rollback from
`deploy/README.md`.
