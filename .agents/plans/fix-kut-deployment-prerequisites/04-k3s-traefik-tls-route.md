# K3s Traefik TLS Route

## Goal

Create and apply a bridge-specific K3s route so the deployment origin serves
only the ROM operator bridge over trusted TLS with WebSocket upgrades.

## Repo Manifest

Add `deploy/k8s/rombridge-ingress.yaml` or a documented equivalent manifest.
The manifest should be safe to commit and contain no private endpoint values
unless the repo already treats that value as sanitized. Prefer placeholders or a
documented patch step for private endpoint addresses.

The route must provide:

- host match for `rombridge.birb.homes` only;
- TLS certificate issuance or use through the existing cert-manager pattern;
- proxying for `/api/...`, `/ws/...`, `/health`, and `/`;
- WebSocket upgrade support for `/ws/events` and `/ws/input`;
- no wildcard host and no accidental service under another hostname;
- no default backend exposure of the bridge.

## Topology Choice

Use one of these topologies and document the chosen one:

| Topology | When To Use |
|---|---|
| K3s Service plus Endpoints to host service | Bridge remains a host systemd service bound to the trusted interface. |
| In-cluster Deployment and Service | Bridge runs inside K3s and does not need systemd. |
| Host-local reverse proxy plus K3s edge | Only if K3s cannot route directly to the host bind. |

The current deployment docs expect a host systemd service and K3s Traefik edge.
Prefer the Service plus Endpoints shape unless host inspection shows that a
different existing local pattern is safer.

## TLS And Certificate Checks

After applying the route:

```sh
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl apply -f deploy/k8s/rombridge-ingress.yaml
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl get ingress,svc,endpoints,certificate,secret -A
curl -I https://rombridge.birb.homes/
```

Raw kubectl output is private until sanitized. The public docs should state only
that a bridge-specific route and certificate were present and valid.

Trusted TLS acceptance:

- curl certificate verification succeeds without `-k`;
- the certificate is valid for `rombridge.birb.homes`;
- the route does not serve bridge content for unrelated Host headers;
- redirects, if any, keep HTTPS and the same host.

## Routing Checks

Private route checks:

```sh
curl -fsS https://rombridge.birb.homes/ >/dev/null
curl -fsS https://rombridge.birb.homes/health >/dev/null
curl -i https://rombridge.birb.homes/api/session
```

WebSocket checks should use the existing deployment validation script when
possible. If a manual tool is needed, use it only with a private cookie source
and keep all output private.

## Security Headers At The Edge

Prefer headers from the Rust service for API/UI consistency. If Traefik adds or
overrides headers, ensure it does not remove:

- runtime no-store and no-cache headers;
- `X-Content-Type-Options: nosniff`;
- UI CSP;
- `Referrer-Policy: no-referrer`;
- `X-Frame-Options: DENY`.

If headers differ between direct service and TLS route, fix the edge or service
until the public route is correct. `kut` is judged at the HTTPS deployment
origin, not only on the private bind.

## Outside-Network Isolation

Produce one technical artifact proving outside-network behavior. Acceptable
options:

- an operator-run probe from a network that should not reach the private bridge;
- firewall or network ACL evidence proving only the trusted network can reach
  the route;
- ingress policy plus Host/SNI routing evidence and listener evidence that
  proves the service is not broadly exposed.

Store the raw artifact privately and reference it in public docs by sanitized
label only.
