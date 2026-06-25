# Contract And Preflight

## 1. Confirm Current Bead Context

Start from the bead and existing docs:

```bash
cd /home/infra-admin/git/preestablished/rom-operator-bridge
bd show rom-operator-bridge-q63
sed -n '1,220p' docs/real-backend-availability.md
```

Confirm `rom-operator-bridge-2sn`, `bp8`, and `mdo` remain closed. If any
dependency has reopened or changed acceptance scope, stop and reconcile the bead
graph before coding.

## 2. Verify Hypervisor Capture RPC Shape

Inspect the generated or source proto in `determinism-hypervisor`:

```bash
cd /home/infra-admin/git/preestablished/determinism-hypervisor
rg -n "RunWithFrameCapture|FrameCaptureEvent|TakeSnapshot|CaptureSpec|Capture" proto crates src -S
```

Record the exact request and response fields privately if they include operator
paths or refs. Public notes may mention only the RPC name and sanitized field
classes.

Expected bridge-owned decision:

- Prefer the RPC that returns capture/export artifacts from the active lease and
  configured `BRIDGE_CAPTURE_SPEC_REF`.
- If the available RPC streams multiple events, complete a job only after the
  terminal success event and durable private writes.
- If only `TakeSnapshot` is available, treat it as a private capture payload
  source only when it has enough metadata to produce a valid capture index row.

## 3. Confirm Reference Workload Bundle Contract

Inspect `reference-workload` under the configured checkout or sibling repo:

```bash
cd /home/infra-admin/git/preestablished
find . -maxdepth 3 -type f | rg 'reference-workload|capture|index|bundle|schema'
```

Find the authoritative schema for `captures/index.jsonl` and any required
payload files. The implementation must follow that schema rather than inventing
bridge-only fields.

If no authoritative schema or exporter exists, stop here:

- Append a sanitized blocker to `rom-operator-bridge-q63`.
- Keep `q63` deferred/open.
- Do not implement synthetic or placeholder completion.

## 4. Preflight Current Bridge Seams

Inspect these files before edits:

```bash
rg -n "trigger_capture|capture_job|CaptureJob|CaptureJobStatus|PrivateArtifactStore|captures/index|RealBackend" service/src service/tests -S
```

Key current seams:

- `service/src/backend.rs` owns `BridgeBackend`, `RealBackend`, real worker
  commands, and real session state.
- `service/src/artifacts.rs` owns private artifact writing.
- `service/src/api.rs` owns API-side capture job records and recent capture
  projection.
- `service/tests/capture/main.rs`, `service/tests/labels/main.rs`, and
  `service/tests/real-backend/main.rs` are the primary focused test surfaces.

## 5. Privacy Preflight

Before coding, prepare a private forbidden-literals file for any live smoke
evidence. Include:

- private root;
- worker endpoint;
- snapshot/config refs;
- capture spec ref;
- raw payload refs;
- any worker error text that includes private values.

Use quiet `rg -q -F -f` sweeps. Never print matching lines.
