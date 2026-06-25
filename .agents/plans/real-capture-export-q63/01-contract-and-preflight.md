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

- Do not use `RunWithFrameCapture` for `q63` unless the worker implementation is
  proven available and the RPC returns enough data for the reference workload
  capture index row.
- Resolve `BRIDGE_CAPTURE_SPEC_REF` privately into a concrete
  `dh::CaptureSpec` plus the layout/feature-map metadata required by the
  reference workload schema.
- Prefer `Run(... capture: Some(spec))` or
  `TakeSnapshot(... capture: Some(spec))` on the active lease if those are the
  implemented worker capture paths.
- Complete a job only after the terminal worker success and durable private
  writes.
- If the resolver/exporter is absent, stop and leave `q63` open/deferred.

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

Use explicit bead commands so the state stays authoritative:

```bash
bd update rom-operator-bridge-q63 --status open --append-notes "Blocked: real capture exporter/schema contract unavailable. Checked only sanitized contract availability. No synthetic, mock-only, or placeholder result was accepted as real capture."
bd defer rom-operator-bridge-q63 --until="+14d"
bd dolt push
git status --short --branch
```

If another repo or operator handoff must supply the missing contract, create or
update an explicit bead dependency or human/private handoff bead. A sanitized
request file under `~/.agents/projects/<repo-name>/requests/` may support the
handoff, but it must not replace bead state.

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
evidence. Store it outside the repository under a private run directory with
mode `0600`; never commit it. Include:

- private root;
- worker endpoint;
- snapshot/config refs;
- capture spec ref;
- raw payload refs;
- any worker error text that includes private values.

Use quiet `rg -q -F -f` sweeps. Never print matching lines.
