# Command Reference

## Deployment Build

Use Node 22 through nvm if the shell resolves `node` to system Node 18:

```sh
cd /home/infra-admin/git/preestablished/rom-operator-bridge
source ~/.nvm/nvm.sh
nvm use 22
node --version
scripts/build-release.sh
```

## Root Installer Refresh And Deploy

```sh
sudo install -o root -g root -m 0755 \
  deploy/admin/install-release-root.sh \
  /usr/local/libexec/rom-operator-bridge/install-release

sudo /usr/local/libexec/rom-operator-bridge/install-release
```

## Safe Public Checks

```sh
curl -i http://tailrombridge.birb.homes/ | head -40
curl -k -i https://rombridge.birb.homes/ | head -40
curl -i http://tailrombridge.birb.homes/health
curl -k -i https://rombridge.birb.homes/health
```

## Host Header Checks

```sh
curl -i \
  -H 'Host: tailrombridge.birb.homes' \
  http://100.82.43.93/ | head -40

curl -i \
  -H 'Host: tailrombridge.birb.homes' \
  -H 'Origin: http://tailrombridge.birb.homes' \
  http://100.82.43.93/api/session | head -40
```

Bare IP check that should not be used as the app URL:

```sh
curl -i http://100.82.43.93/api/session | head -40
```

Expected: Apache fallback, not bridge API.

## API Start/Stop Smoke

Only run this when it is acceptable to briefly create a real session. If a
session is created, stop it before handing back to the user. Do not store or
paste the returned cookie into committed files.

```sh
curl -i \
  -H 'Origin: http://tailrombridge.birb.homes' \
  -H 'Content-Type: application/json' \
  -X POST http://tailrombridge.birb.homes/api/session/start \
  --data '{"schema_version":1,"backend_mode":"real","requested_capabilities":["input","preview","capture","labels","privileged_features"]}'
```

If this returns `session_active_elsewhere`, an operator/browser session is
already active. Do not force-clear it without operator approval.

## Kubernetes Route Checks

```sh
KUBECONFIG=/etc/rancher/k3s/k3s.yaml \
  kubectl -n rom-operator-bridge get ingress,svc,endpoints -o wide

KUBECONFIG=/etc/rancher/k3s/k3s.yaml \
  kubectl -n traefik logs deploy/traefik --since=10m --tail=200
```

Keep raw logs private if they include request details beyond sanitized status
or routing metadata.

