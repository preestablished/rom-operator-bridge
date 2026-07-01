# ROM Operator Bridge Deployment

This directory contains sanitized deployment material for
`https://rombridge.birb.homes/`. Do not commit instantiated private env files,
endpoint manifests, cookie files, operator-approved refs, raw probe output, or
host logs.

## Recommended Host Layout

```text
/opt/rom-operator-bridge/releases/<release-id>/
/opt/rom-operator-bridge/current -> /opt/rom-operator-bridge/releases/<release-id>
/opt/rom-operator-bridge/previous -> /opt/rom-operator-bridge/releases/<old-release-id>
/var/lib/rom-operator-bridge/private/
/var/lib/rom-operator-bridge/static/releases/<release-id>/
/var/lib/rom-operator-bridge/static/current -> /var/lib/rom-operator-bridge/static/releases/<release-id>
/etc/rom-operator-bridge/rom-operator-bridge.env
```

Use a dedicated `rombridge` system user or another operator-approved service
account. Ensure the private runtime root is mode `0700`, and the env file plus
private validation artifacts are mode `0600`.

## Private Env Shape

The installed env file is:

```text
/etc/rom-operator-bridge/rom-operator-bridge.env
```

The service loads this file through systemd `EnvironmentFile`; use plain
`KEY=value` assignments only. Do not use shell `export KEY=value` syntax or
whitespace around `=` in the deployed env file.

It must contain placeholders replaced with operator-approved private values:

```sh
ROM_OPERATOR_BRIDGE_BIND_ADDR=<bridge-private-ip>:7410
ROM_OPERATOR_BRIDGE_BACKEND=<synthetic-or-real>
ROM_OPERATOR_BRIDGE_PRIVATE_ROOT=<absolute-private-runtime-root>
ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT=<absolute-static-publish-root>
ROM_OPERATOR_BRIDGE_SESSION_SECRET=<session-secret>
```

Real backend mode also requires the real backend handoff values documented in
`docs/runbook.md` and `service/src/private_config.rs`.

## Two-Phase Build And Install

Builds must run as the unprivileged operator. Privileged install/restart work
must run from a root-owned copy of the installer, not from this user-writable
checkout.

Build:

```sh
scripts/build-release.sh
```

Install the audited root installer once, or whenever its committed content
changes:

```sh
sudo install -d -o root -g root -m 0755 \
  /usr/local/libexec/rom-operator-bridge
sudo install -o root -g root -m 0755 \
  deploy/admin/install-release-root.sh \
  /usr/local/libexec/rom-operator-bridge/install-release
```

Optional narrow passwordless sudo for future Codex-driven deploys:

```sh
printf '%s\n' \
  'infra-admin ALL=(root) NOPASSWD: /usr/local/libexec/rom-operator-bridge/install-release' \
  | sudo tee /etc/sudoers.d/rom-operator-bridge-deploy >/dev/null
sudo chmod 0440 /etc/sudoers.d/rom-operator-bridge-deploy
sudo visudo -cf /etc/sudoers.d/rom-operator-bridge-deploy
```

Do not add `SETENV`, wildcards, arbitrary arguments, or a sudoers entry for a
script in this repository checkout. The installer accepts no arguments and uses
fixed deployment paths. If the checkout path changes, update and reinstall the
root-owned copy after review.

Deploy the already-built release:

```sh
sudo -n /usr/local/libexec/rom-operator-bridge/install-release
```

The installer copies the built service binary and static UI into timestamped
release directories, updates `previous` and `current`, backs up the private env
file, points `ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT` at the resolved static
release directory, installs the hardened systemd unit from embedded content,
restarts the service, and prints only sanitized PASS/FAIL status.

## Manual Build And Install

Build:

```sh
cargo build --manifest-path service/Cargo.toml --release
npm --prefix ui ci
npm --prefix ui run build
```

Install into clean release directories. Record the resolved previous release
target before switching symlinks; do not point `previous` at the mutable
`current` symlink.

```sh
release_id=<release-id>
old_service_release="$(readlink -f /opt/rom-operator-bridge/current 2>/dev/null || true)"
old_static_release="$(readlink -f /var/lib/rom-operator-bridge/static/current 2>/dev/null || true)"

sudo install -d -m 0755 /opt/rom-operator-bridge/releases/"$release_id"
sudo install -m 0755 service/target/release/rom-operator-bridge-service \
  /opt/rom-operator-bridge/releases/"$release_id"/rom-operator-bridge

sudo install -d -m 0755 /var/lib/rom-operator-bridge/static/releases/"$release_id"
sudo cp -rf ui/dist/. /var/lib/rom-operator-bridge/static/releases/"$release_id"/

if [ -n "$old_service_release" ]; then
  sudo ln -sfn "$old_service_release" /opt/rom-operator-bridge/previous
fi
if [ -n "$old_static_release" ]; then
  sudo ln -sfn "$old_static_release" /var/lib/rom-operator-bridge/static/previous
fi
sudo ln -sfn /opt/rom-operator-bridge/releases/"$release_id" /opt/rom-operator-bridge/current
sudo ln -sfn /var/lib/rom-operator-bridge/static/releases/"$release_id" \
  /var/lib/rom-operator-bridge/static/current
```

Install and start systemd:

```sh
sudo install -d -m 0755 /etc/rom-operator-bridge
sudo install -m 0600 <private-env-source> \
  /etc/rom-operator-bridge/rom-operator-bridge.env
sudo install -m 0644 deploy/systemd/rom-operator-bridge.service \
  /etc/systemd/system/rom-operator-bridge.service
sudo systemctl daemon-reload
sudo systemctl restart rom-operator-bridge.service
```

Inspect service status and logs privately. Do not paste raw output into shared
docs or bead notes.

## K3s Endpoint And Ingress

`deploy/k8s/rombridge-ingress.yaml` defines the sanitized Namespace, Service,
and Ingress. The actual endpoint address must be supplied through an
operator-private manifest outside the repo.

Private endpoint shape:

```yaml
apiVersion: v1
kind: Endpoints
metadata:
  name: rom-operator-bridge
  namespace: rom-operator-bridge
subsets:
  - addresses:
      - ip: <bridge-private-ip>
    ports:
      - name: http
        port: 7410
        protocol: TCP
```

Apply order:

```sh
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl apply -f deploy/k8s/rombridge-ingress.yaml
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl apply -f <private-endpoint-manifest>
```

## Tailscale HTTP Route

The optional Tailscale-only HTTP route is a separate Traefik `web` entrypoint
Ingress:

```text
deploy/k8s/rombridge-tailscale-http-ingress.yaml
```

It routes `http://tailrombridge.birb.homes/` to the existing
`rom-operator-bridge` Service on port `7410`. It intentionally does not enable
TLS. Apply it only after the base Namespace, Service, and private Endpoints
exist:

```sh
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl apply -f deploy/k8s/rombridge-ingress.yaml
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl apply -f <private-endpoint-manifest>
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl apply -f deploy/k8s/rombridge-tailscale-http-ingress.yaml
```

If `tailrombridge.birb.homes` serves the Apache `birb.homes` page from the
`birb-homes` namespace, Traefik is still routing that Host to the fallback site
instead of this bridge Ingress. Check Traefik route state and Apache access logs
for the request while keeping raw logs private.

Rollback only the Tailscale HTTP route:

```sh
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl delete -f deploy/k8s/rombridge-tailscale-http-ingress.yaml
```

## Validation

Run validation only with private cookie, forbid-file, static-root, and network
evidence inputs:

```sh
ROM_BRIDGE_VALIDATION_DIR=<private-validation-dir>/deployment-network-kut/<run-id> \
ROM_BRIDGE_COOKIE_CURL_CONFIG_FILE=<private-cookie-curl-config> \
ROM_BRIDGE_STATIC_PUBLISH_ROOT=<absolute-static-publish-root> \
ROM_BRIDGE_NETWORK_EVIDENCE_FILE=<private-network-evidence-file> \
ROM_BRIDGE_NETWORK_EVIDENCE_REVIEWED=1 \
ROM_BRIDGE_OUTSIDE_PROBE_RESULT_FILE=<private-outside-probe-file> \
ROM_BRIDGE_OUTSIDE_PROBE_REVIEWED=1 \
ROM_BRIDGE_FORBID_FILE=<private-forbid-file> \
scripts/deployment-network-check.sh
```

For the Tailscale HTTP route, first prepare private validation inputs, then run
the route-specific checker:

```sh
python3 scripts/prepare-tailscale-http-validation-inputs.py \
  --start-session-json <private-start-session-json> \
  --cookie-jar <private-cookie-jar> \
  --session-response <private-session-response-json> \
  --network-evidence <private-network-evidence-file>

ROM_BRIDGE_TAILSCALE_VALIDATION_DIR=<private-validation-dir>/tailscale-http/<run-id> \
ROM_BRIDGE_TAILSCALE_SESSION_COOKIE_FILE=<private-cookie-jar> \
ROM_BRIDGE_TAILSCALE_NETWORK_EVIDENCE_FILE=<private-network-evidence-file> \
ROM_BRIDGE_TAILSCALE_NETWORK_EVIDENCE_REVIEWED=1 \
ROM_BRIDGE_TAILSCALE_OUTSIDE_PROBE_RESULT_FILE=<private-outside-probe-file> \
ROM_BRIDGE_TAILSCALE_OUTSIDE_PROBE_REVIEWED=1 \
ROM_BRIDGE_TAILSCALE_FORBID_FILE=<private-forbid-file> \
scripts/tailscale-http-check.sh
```

Run the redaction gate separately with the same private forbid file:

```sh
ROM_OPERATOR_BRIDGE_FORBID_FILE=<private-forbid-file> \
ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1 \
bash scripts/redaction-gate.sh
```

If no `ROM_BRIDGE_RESOLVE_IP` is supplied, also provide
`ROM_BRIDGE_HOST_SNI_EVIDENCE_FILE` and
`ROM_BRIDGE_HOST_SNI_EVIDENCE_REVIEWED=1`.

## Rollback

```sh
sudo systemctl stop rom-operator-bridge.service
sudo ln -sfn /opt/rom-operator-bridge/previous /opt/rom-operator-bridge/current
sudo ln -sfn /var/lib/rom-operator-bridge/static/previous \
  /var/lib/rom-operator-bridge/static/current
sudo systemctl restart rom-operator-bridge.service
```

If the env file changed, restore the operator-private backup before restart.
The root installer stores env backups under `/etc/rom-operator-bridge/backups/`
using the release id; restore the matching backup when rolling back a release
that changed `ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT`.
