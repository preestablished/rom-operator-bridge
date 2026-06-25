# Operator kut Deployment Runbook

This runbook is for the operator step that unblocks
`rom-operator-bridge-kut`. It intentionally uses placeholders. Do not commit or
paste instantiated private env values, endpoint addresses, cookie files, raw
logs, probe output, private paths, or private evidence contents.

Run commands from the repository checkout:

```sh
cd <repo-checkout>
git pull --rebase
```

## 1. Install The Private Env File

Create or approve a private env source outside the repository, then install it:

```sh
sudo install -d -m 0755 /etc/rom-operator-bridge
sudo install -m 0600 <private-env-source> \
  /etc/rom-operator-bridge/rom-operator-bridge.env
```

The env file needs real operator-approved values for these keys:

| Key | Value |
|---|---|
| `ROM_OPERATOR_BRIDGE_BIND_ADDR` | `<bridge-private-ip>:7410` |
| `ROM_OPERATOR_BRIDGE_BACKEND` | `<synthetic-or-real>` |
| `ROM_OPERATOR_BRIDGE_PRIVATE_ROOT` | `<absolute-private-runtime-root>` |
| `ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT` | `/var/lib/rom-operator-bridge/static/current` |
| `ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL` | `<operator-credential>` |
| `ROM_OPERATOR_BRIDGE_SESSION_SECRET` | `<session-secret>` |

If `ROM_OPERATOR_BRIDGE_BACKEND=real`, also include the approved real backend
handoff values documented in `docs/runbook.md` and `service/src/private_config.rs`.

## 2. Build And Install The Release

Build the service and UI:

```sh
cargo build --manifest-path service/Cargo.toml --release
npm --prefix ui ci
npm --prefix ui run build
```

Install the release and static UI into clean release directories:

```sh
release_id="$(date -u +%Y%m%dT%H%M%SZ)"
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

sudo ln -sfn /opt/rom-operator-bridge/releases/"$release_id" \
  /opt/rom-operator-bridge/current
sudo ln -sfn /var/lib/rom-operator-bridge/static/releases/"$release_id" \
  /var/lib/rom-operator-bridge/static/current
```

## 3. Start The Systemd Service

Install the committed service unit and restart:

```sh
sudo install -m 0644 deploy/systemd/rom-operator-bridge.service \
  /etc/systemd/system/rom-operator-bridge.service
sudo systemctl daemon-reload
sudo systemctl restart rom-operator-bridge.service
sudo systemctl status --no-pager rom-operator-bridge.service
```

Inspect status and journal output privately. Do not paste raw status or logs
into repo docs, bead notes, or chat.

## 4. Apply The K3s Route

Apply the committed sanitized route:

```sh
KUBECONFIG=/etc/rancher/k3s/k3s.yaml \
  kubectl apply -f deploy/k8s/rombridge-ingress.yaml
```

Create an operator-private endpoint manifest outside the repository that points
the `rom-operator-bridge` Service at the trusted bridge address:

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

Apply that private endpoint manifest:

```sh
KUBECONFIG=/etc/rancher/k3s/k3s.yaml \
  kubectl apply -f <private-endpoint-manifest>
```

## 5. Run Private Deployment Validation

Run the deployment-network checker with private evidence inputs:

```sh
ROM_BRIDGE_VALIDATION_DIR=<private-validation-dir>/deployment-network-kut/<run-id> \
ROM_BRIDGE_COOKIE_CURL_CONFIG_FILE=<private-cookie-curl-config> \
ROM_BRIDGE_STATIC_PUBLISH_ROOT=/var/lib/rom-operator-bridge/static/current \
ROM_BRIDGE_NETWORK_EVIDENCE_FILE=<private-network-evidence-file> \
ROM_BRIDGE_NETWORK_EVIDENCE_REVIEWED=1 \
ROM_BRIDGE_OUTSIDE_PROBE_RESULT_FILE=<private-outside-probe-file> \
ROM_BRIDGE_OUTSIDE_PROBE_REVIEWED=1 \
ROM_BRIDGE_FORBID_FILE=<private-forbid-file> \
scripts/deployment-network-check.sh
```

If no `ROM_BRIDGE_RESOLVE_IP` is supplied, also provide reviewed private
Host/SNI evidence:

```sh
ROM_BRIDGE_HOST_SNI_EVIDENCE_FILE=<private-host-sni-evidence-file> \
ROM_BRIDGE_HOST_SNI_EVIDENCE_REVIEWED=1 \
scripts/deployment-network-check.sh
```

Run the redaction gate with the operator-private forbid file:

```sh
ROM_OPERATOR_BRIDGE_FORBID_FILE=<private-forbid-file> \
ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1 \
bash scripts/redaction-gate.sh
```

## 6. Handoff Back To The Agent

Report only:

- whether `scripts/deployment-network-check.sh` passed;
- whether `bash scripts/redaction-gate.sh` passed;
- the sanitized private evidence label.

Do not report raw command output, private paths, env values, endpoint addresses,
cookie data, private refs, or logs.

## Rollback

If the deployment fails after switching releases:

```sh
sudo systemctl stop rom-operator-bridge.service
sudo ln -sfn /opt/rom-operator-bridge/previous /opt/rom-operator-bridge/current
sudo ln -sfn /var/lib/rom-operator-bridge/static/previous \
  /var/lib/rom-operator-bridge/static/current
sudo systemctl restart rom-operator-bridge.service
```

If the env file changed during the failed deployment, restore the private env
backup before restarting.
