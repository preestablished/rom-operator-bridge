# Current State And Inputs

## References To Read First

The future agent should read these before editing:

- `bd show rom-operator-bridge-kut`
- `docs/deployment-checks.md`
- `docs/deployment-note.md`
- `docs/deployment-security-shape.md`
- `docs/runbook.md`
- `scripts/deployment-network-check.sh`
- `service/src/config.rs`
- `service/src/private_config.rs`
- `service/src/api.rs`

## Sanitized Current State

`docs/deployment-checks.md` records the current blocked run. Keep that document
as the public summary and keep raw validation material private.

The current public status is:

- deployment origin: `https://rombridge.birb.homes/`;
- runtime API: `https://rombridge.birb.homes/api/...`;
- WSS routes: `wss://rombridge.birb.homes/ws/events` and
  `wss://rombridge.birb.homes/ws/input`;
- expected service bind: `<bridge-private-ip>:7410` or a later documented
  loopback proxy topology;
- private request label:
  `rom-operator-bridge/kut-deployment-route-prerequisites`;
- latest failure: missing trusted TLS, missing active bridge service, missing
  bridge K3s route, missing root UI route, missing outside-network evidence.

Do not replace sanitized placeholders or labels with local filesystem paths,
private addresses, or raw evidence names in committed docs.

## Private Inputs The Operator Must Provide

The future agent needs operator-approved private inputs before live deployment:

- a private runtime root for `ROM_OPERATOR_BRIDGE_PRIVATE_ROOT`;
- an optional static publish root for `ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT`
  if the service will serve UI files directly;
- an operator credential for `ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL`;
- a session-signing secret for `ROM_OPERATOR_BRIDGE_SESSION_SECRET`;
- the intended backend mode for `ROM_OPERATOR_BRIDGE_BACKEND`;
- real backend values only if exposing real backend mode:
  `BRIDGE_WORKLOAD_IMAGE_REF`, `BRIDGE_CAPTURE_SPEC_REF`,
  `BRIDGE_REFERENCE_WORKLOAD_CHECKOUT`, and exactly one of
  `BRIDGE_REAL_SNAPSHOT_REF` or `BRIDGE_CREATE_VM_CONFIG_REF`;
- the hypervisor endpoint if real mode should use a non-default endpoint;
- a private cookie jar or curl config for authenticated deployment probes;
- an outside-network probe result or firewall/ingress policy artifact.

The env file belongs outside the repo and must be mode `0600`. The private root
must be mode `0700`. Keep all raw validation output under an operator-approved
private validation directory outside the repo.

## Backend Mode Decision

For `kut`, the acceptance target is deployment route isolation, not real capture
correctness. The future agent may validate route isolation with synthetic mode
only if the operator agrees that `kut` is proving network and browser security
for the deployed route. If the deployed operator route is meant to be production
real mode, use `ROM_OPERATOR_BRIDGE_BACKEND=real` and the approved private real
backend values.

Do not silently substitute synthetic mode for a production real-mode deployment.
If real mode cannot start, record that as a sanitized blocker instead of closing
`kut`.

## Privacy Boundary

Acceptable public content:

- pass/fail summaries;
- sanitized labels for private evidence;
- placeholders like `<private-validation-dir>` and `<bridge-private-ip>`;
- commit ids;
- route names and env var names already documented in this repo.

Forbidden public content:

- actual credentials, cookies, session ids, tokens, secret values, private refs,
  or request bodies;
- raw curl output, raw headers, service logs, systemd status, or kubectl
  transcripts before redaction;
- private IP addresses, private filesystem paths, user home paths, capture ids,
  screenshots, or artifact refs;
- exact values of operator-approved bridge inputs.

## First Safety Checks

Before editing deployment files, run:

```sh
git status --short --branch
bd show rom-operator-bridge-kut
```

If the worktree has unrelated user changes, leave them alone. If deployment
changes require host-level writes, keep repo edits and host operations separate
in the final handoff.
