# Fix kut Deployment Prerequisites

## Target Bead

Fix the active blocker for `rom-operator-bridge-kut`: deployment network
isolation cannot be verified because the bridge is not actually deployed at the
documented route.

`kut` is not blocked on another assertion script. The current branch already
has `scripts/deployment-network-check.sh` and `docs/deployment-checks.md`. The
remaining work is to make the documented deployment route real, then rerun the
checks and record sanitized pass/fail evidence.

## Current Blocker

The most recent sanitized evidence says:

- DNS and private cookie-source setup are available;
- trusted TLS for `https://rombridge.birb.homes/` fails;
- root, API, and WSS routes do not serve the bridge runtime;
- no `rom-operator-bridge` systemd unit is installed or active;
- no expected listener is proven on port `7410`;
- K3s has no bridge-specific ingress, service, certificate, or Traefik route;
- no technical outside-network evidence exists yet.

The service binary currently exposes `/health`, `/api/...`, and `/ws/...`.
It does not yet serve the static UI root from the deployment origin. A
deployment-only change will still leave the root route and mixed-content checks
failing unless the future agent also adds a static UI serving path or a separate
static-file service behind the same HTTPS origin.

## Intended End State

The future agent should leave this repo and host in a state where:

- `https://rombridge.birb.homes/` serves the bridge UI over trusted TLS;
- `https://rombridge.birb.homes/api/...` proxies to the bridge service;
- `wss://rombridge.birb.homes/ws/...` upgrades to the bridge service;
- the bridge binds only to the documented trusted interface or a documented
  loopback proxy topology;
- Host/SNI routing serves the bridge only for `rombridge.birb.homes`;
- runtime responses and private preview routes use no-store headers;
- browser-facing assets do not reference `http://` or `ws://` runtime endpoints;
- outside-network access is unavailable or rejected with technical evidence;
- `scripts/deployment-network-check.sh` passes with private inputs;
- `docs/deployment-checks.md` records only sanitized PASS results and evidence
  labels.

## Planned Outputs

The implementation should produce:

- service/static-route changes if needed for the same-origin UI root;
- `deploy/systemd/rom-operator-bridge.service`;
- `deploy/k8s/rombridge-ingress.yaml` or a clearly documented equivalent
  K3s/Traefik route manifest;
- optional `deploy/README.md` with sanitized install and rollback commands;
- updated `docs/deployment-checks.md` after validation;
- a bead note on `rom-operator-bridge-kut` with sanitized status and commit ids.

## Non-Goals

- Do not publish private evidence into this repository.
- Do not commit real credentials, cookie files, private env files, private
  paths, private IPs, raw command output, logs, capture ids, screenshots, or
  operator-approved bridge values.
- Do not change the public route contract away from
  `https://rombridge.birb.homes/` unless a new bead explicitly changes the
  deployment design.
- Do not close `rom-operator-bridge-eqi`; closing `kut` only makes `eqi`
  eligible for its own publish-readiness work.

## File Map

| File | Purpose |
|---|---|
| `00-overview.md` | Target, blocker, end state, outputs, non-goals |
| `01-current-state-and-inputs.md` | Existing evidence, private inputs, safety boundary |
| `02-service-static-ui-route.md` | Required code/service changes for the root UI route |
| `03-systemd-install-and-runtime.md` | Install material, private env shape, release flow |
| `04-k3s-traefik-tls-route.md` | K3s, Traefik, TLS, Host/SNI, and WSS routing |
| `05-validation-and-evidence.md` | Private validation run and sanitized evidence update |
| `06-closeout.md` | Quality gates, bead updates, commit and push protocol |
