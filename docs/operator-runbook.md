# Operator Runbook

Date: 2026-06-29

This is the operator entry point for running `rom-operator-bridge` without
reading service source. It intentionally uses placeholders. Keep instantiated
operator credentials, session secrets, cookie files, endpoint manifests, private
artifact paths, raw screenshots, capture payloads, verifier reports, and private
evidence contents outside the repository and out of chat.

## Current Deployment

The Phase 0 runtime target is:

```text
https://rombridge.birb.homes/
```

Runtime API and WebSocket traffic use the same origin:

```text
https://rombridge.birb.homes/api/...
wss://rombridge.birb.homes/ws/...
```

The service listens on a trusted private interface behind the sanitized K3s
Ingress in `deploy/k8s/rombridge-ingress.yaml`. The concrete endpoint address is
operator-private and belongs in an uncommitted endpoint manifest.

## Privacy Rules

Shared docs and bead notes may include command names, placeholder command
templates, sanitized pass/fail status, and approved evidence labels. They must
not include:

- operator credentials, session secrets, cookies, auth headers, or tokens;
- private endpoint addresses, private env file contents, or host logs;
- absolute private roots, ROM paths, bundle paths, screenshot paths, or report
  paths;
- raw screenshots, framebuffer payloads, decoded feature values, capture ids,
  raw verifier reports, or raw command transcripts.

Use a private forbid-literals file for deployment and publish validation:

```sh
ROM_OPERATOR_BRIDGE_FORBID_FILE=<private-forbid-file> \
ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1 \
bash scripts/redaction-gate.sh
```

## 1. Prepare Private Configuration

Use the deployment runbook for the full install procedure:

```text
deploy/operator-kut-deployment-runbook.md
```

The private env file is installed outside the repository:

```text
/etc/rom-operator-bridge/rom-operator-bridge.env
```

It must be mode `0600` and contain operator-approved values for:

```sh
ROM_OPERATOR_BRIDGE_BIND_ADDR=<bridge-private-ip>:7410
ROM_OPERATOR_BRIDGE_BACKEND=<synthetic-or-real>
ROM_OPERATOR_BRIDGE_PRIVATE_ROOT=<absolute-private-runtime-root>
ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT=<absolute-static-release-dir>
ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL=<operator-credential>
ROM_OPERATOR_BRIDGE_SESSION_SECRET=<session-secret>
```

For `ROM_OPERATOR_BRIDGE_BACKEND=real`, also provide the approved real backend
handoff values from `docs/runbook.md` and `service/src/private_config.rs`:

```sh
BRIDGE_HYPERVISOR_ENDPOINT=<hypervisor-endpoint>
BRIDGE_WORKLOAD_IMAGE_REF=<operator-approved-workload-image-ref>
BRIDGE_CAPTURE_SPEC_REF=<operator-approved-capture-spec-ref>
BRIDGE_REFERENCE_WORKLOAD_CHECKOUT=<absolute-reference-workload-checkout>
BRIDGE_REAL_SNAPSHOT_REF=<operator-approved-snapshot-ref>
BRIDGE_CREATE_VM_CONFIG_REF=<operator-approved-create-vm-config-ref>
```

Use exactly one real start source: either
`BRIDGE_REAL_SNAPSHOT_REF` or `BRIDGE_CREATE_VM_CONFIG_REF`.

Generate missing auth values and validate the env shape without printing
secrets:

```sh
sudo python3 scripts/generate-operator-auth.py \
  /etc/rom-operator-bridge/rom-operator-bridge.env
sudo python3 scripts/validate-operator-env.py \
  /etc/rom-operator-bridge/rom-operator-bridge.env
```

## 2. Build, Install, Start, And Stop

Build the service and UI:

```sh
cargo build --manifest-path service/Cargo.toml --release
npm --prefix ui ci
npm --prefix ui run build
```

Install the release, static UI, systemd unit, private env, and K3s route using:

```text
deploy/operator-kut-deployment-runbook.md
deploy/README.md
```

Start or restart the service:

```sh
sudo systemctl daemon-reload
sudo systemctl restart rom-operator-bridge.service
sudo systemctl status --no-pager rom-operator-bridge.service
```

Stop the service:

```sh
sudo systemctl stop rom-operator-bridge.service
```

Inspect systemd status and journal output privately. Do not paste raw output
into public docs or bead notes.

## 3. Synthetic Validation

Synthetic validation is agent-runnable and does not prove real Phase 4
acceptance. It is useful for checking UI, auth, runtime state, capture retry,
label conflict, input, preview, and stop behavior without private ROM data.

Run the focused UI smoke:

```sh
npm --prefix ui test -- --run tests/synthetic-smoke/syntheticOperatorSmoke.test.ts
```

Run the service synthetic capture and label artifact check:

```sh
cargo test --manifest-path service/Cargo.toml --test capture synthetic_capture_labels_round_trip_private_files_and_event_refreshes
```

Run the full project quality gate before handoff:

```sh
bash scripts/quality-gate.sh
```

See `docs/synthetic-smoke.md` for the browser smoke checklist and public
recording rules.

## 4. Real-Host Operation

Real-host operation requires approved private runtime data and host access.
Before starting a real session, verify:

- `ROM_OPERATOR_BRIDGE_BACKEND=real`;
- the private env validator passes;
- `dh-workerd` or the approved hypervisor endpoint is reachable from the
  service host;
- the approved snapshot or CreateVm config resolves through private operator
  policy;
- the private runtime root is service-accessible, mode `0700`, and outside the
  static publish root.

Open the operator UI at:

```text
https://rombridge.birb.homes/
```

Authenticate with the operator credential. Runtime responses and WebSocket
handshakes must remain same-origin, authenticated, and `no-store`. Use the UI
to start, pause/resume, inspect sanitized run status, drive input, trigger
captures, label captures, and stop the session. Public notes may record only
sanitized status and approved evidence labels.

## 5. Capture Labeling

The UI supports capture review and label drafting. Private label notes and
capture payloads are stored server-side under the private root. Public API
responses and UI state must not expose private paths, raw feature values, raw
framebuffer bytes, artifact refs, or real private capture ids.

For synthetic runs, `docs/synthetic-smoke.md` documents the expected capture and
label behavior. For real runs, `rom-operator-bridge-r77` remains deferred until
approved private operator evidence is available.

## 6. Verifier Flow

Verifier command shapes are recorded in `docs/runbook.md`. Operator-private
bundle validation uses placeholders for feature maps, score plans, labels,
private bundles, validation reports, and forbid files.

The deployment-private validation flow is:

```text
deploy/operator-kut-private-validation-reference.md
```

For bridge-produced private bundles, run the reference-workload verifier
commands privately and publish only sanitized aggregate status:

```sh
(cd <reference-workload-checkout> && cargo run --locked -p refwork-verify -- phase4-bundle-check \
  --bundle <private-bundle-dir> \
  --report <private-validation-dir>/phase4-bundle-check.json)

(cd <reference-workload-checkout> && cargo run --locked -p refwork-verify -- phase4-checksum-manifest \
  --bundle <private-bundle-dir> \
  --out <private-validation-dir>/checksums.json)
```

`rom-operator-bridge-opw` remains deferred until an operator-approved real
capture bundle exists.

## 7. Deployment Validation

Run deployment validation with private cookie, network evidence, outside-probe,
static-root, and forbid-file inputs:

```sh
ROM_BRIDGE_VALIDATION_DIR=<private-validation-dir>/deployment-network-kut/<run-id> \
ROM_BRIDGE_COOKIE_CURL_CONFIG_FILE=<private-cookie-curl-config> \
ROM_BRIDGE_STATIC_PUBLISH_ROOT=<absolute-static-release-dir> \
ROM_BRIDGE_NETWORK_EVIDENCE_FILE=<private-network-evidence-file> \
ROM_BRIDGE_NETWORK_EVIDENCE_REVIEWED=1 \
ROM_BRIDGE_OUTSIDE_PROBE_RESULT_FILE=<private-outside-probe-file> \
ROM_BRIDGE_OUTSIDE_PROBE_REVIEWED=1 \
ROM_BRIDGE_FORBID_FILE=<private-forbid-file> \
scripts/deployment-network-check.sh
```

If no `ROM_BRIDGE_RESOLVE_IP` is supplied, also provide reviewed Host/SNI
evidence as documented in `deploy/operator-kut-deployment-runbook.md`.

The current sanitized deployment evidence is recorded in:

```text
docs/deployment-checks.md
docs/publish-readiness.md
docs/handoff.md
```

## 8. Restart, Rollback, And Rotation

Restart:

```sh
sudo systemctl daemon-reload
sudo systemctl restart rom-operator-bridge.service
```

Rollback service and static releases:

```sh
sudo systemctl stop rom-operator-bridge.service
sudo ln -sfn /opt/rom-operator-bridge/previous /opt/rom-operator-bridge/current
sudo ln -sfn /var/lib/rom-operator-bridge/static/previous \
  /var/lib/rom-operator-bridge/static/current
sudo systemctl restart rom-operator-bridge.service
```

If the private env file changed, restore the operator-private backup before
restart. If rotating credentials, update the private env file, rotate the
session secret when needed, restart the service, and verify old credentials and
old sessions fail.

## 9. Remaining Gaps

The following beads are intentionally deferred because they need private
operator runtime data or host evidence:

- `rom-operator-bridge-0wo` - document and run real backend smoke.
- `rom-operator-bridge-r77` - run one real capture and label smoke.
- `rom-operator-bridge-opw` - validate a bridge-produced private bundle.

Synthetic validation, deployment-network validation, and redaction gates must
not be represented as proof that a real private capture exporter or
bridge-produced private bundle has passed Phase 4 acceptance.

## References

- `deploy/operator-kut-deployment-runbook.md`
- `deploy/operator-kut-private-validation-reference.md`
- `deploy/README.md`
- `docs/runbook.md`
- `docs/synthetic-smoke.md`
- `docs/deployment-note.md`
- `docs/deployment-checks.md`
- `docs/publish-readiness.md`
- `docs/redaction.md`
- `docs/handoff.md`
