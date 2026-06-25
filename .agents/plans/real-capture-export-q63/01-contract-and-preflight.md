# Contract And Preflight

## 1. Confirm Current Bead Context

Start from the bead and existing docs:

```bash
REPO_ROOT="${REPO_ROOT:-$PWD}"
cd "$REPO_ROOT"
bd show rom-operator-bridge-q63
sed -n '1,220p' docs/real-backend-availability.md
```

Confirm `rom-operator-bridge-2sn`, `bp8`, and `mdo` remain closed. If any
dependency has reopened or changed acceptance scope, stop and reconcile the bead
graph before coding.

Use variables such as `REPO_ROOT`, `HYPERVISOR_CHECKOUT`,
`REFERENCE_WORKLOAD_CHECKOUT`, and `PRIVATE_REQUEST_DIR` in notes and commands.
Do not commit concrete operator-private paths or copy them into bead notes.

## 2. Verify Hypervisor Capture RPC Shape

Inspect the generated or source proto in `determinism-hypervisor`:

```bash
cd "$HYPERVISOR_CHECKOUT"
rg -n "RunWithFrameCapture|FrameCaptureEvent|TakeSnapshot|CaptureSpec|Capture" proto crates src -S
```

Record the exact request and response fields privately if they include operator
paths or refs. Public notes may mention only the RPC name and sanitized field
classes.

Expected bridge-owned decision:

- Do not use `RunWithFrameCapture` for `q63`; the local worker implementation
  is not the capture/export path for this bead.
- Resolve `BRIDGE_CAPTURE_SPEC_REF` privately into a concrete
  `dh::CaptureSpec` plus the layout/feature-map metadata required by the
  reference workload schema.
- For `q63`, use `TakeSnapshot(... capture: Some(spec))` on the active lease.
  The implementation must set `seal_input_log` intentionally, test that value,
  and persist/update the returned snapshot/session state because `TakeSnapshot`
  is a lifecycle operation, not a read-only capture call.
- Complete a job only after the terminal worker success and durable private
  writes.
- If the capture-spec resolver, schema, or required private layout/map inputs
  are absent, stop and leave `q63` open/deferred.

Capture-spec materialization requirements:

- Add private accessors for `RealRuntimeConfig` values needed by the resolver.
- Resolve `BRIDGE_CAPTURE_SPEC_REF` only inside private config/backend code.
- Load the operator-approved `layout.json` and `feature-map.yaml` from the
  private reference workload bundle or from the private ref resolver output.
- Build `dh::CaptureSpec.ranges` from the compiled layout ranges in byte order:
  `region`, `layout_version`, `offset`, and `len` must map directly to
  `dh::ExtractRange`.
- Set `CaptureSpec.framebuffer` explicitly according to the approved capture
  contract.
- Verify `layout.json.total_len` equals the sum of range lengths, and verify the
  layout hash that will be copied into capture rows.
- Fail closed with sanitized `backend_unavailable` if any field is missing,
  unsupported, or inconsistent.

## 3. Confirm Reference Workload Bundle Contract

Inspect `reference-workload` under the configured checkout or sibling repo:

```bash
cd "$REFERENCE_WORKLOAD_CHECKOUT"
find . -maxdepth 3 -type f | rg 'reference-workload|capture|index|bundle|schema'
```

Find the authoritative schema for `captures/index.jsonl` and any required
payload files. The implementation must follow that schema rather than inventing
bridge-only fields.

The local reference workload may provide schema, validators, and layout/map
inputs rather than a bridge-ready exporter. That is enough for `q63` to proceed
with a bridge-owned writer. Stop only if the authoritative capture row contract,
validator expectations, or private layout/map inputs needed to build the row are
unavailable.

The plan must satisfy both bundle and trace checks:

- `phase4_bundle_check` requires `capture_id`, `node_ref` or `source_id`,
  `capture_source`, `frame_index` or `frame_counter`, `layout_hash`,
  `feature_bytes.ref`, `feature_bytes.len`, `feature_bytes.blake3`,
  framebuffer ref/hash/metadata, and no inline payload bytes.
- `phase4_trace` requires `decoded_order` and `decoded_values`; the order must
  match the feature-map order.

If those contracts or private inputs are missing, stop here:

- Append a sanitized blocker to `rom-operator-bridge-q63`.
- Keep `q63` deferred/open.
- Do not implement synthetic or placeholder completion.

Use explicit bead commands so the state stays authoritative:

```bash
bd update rom-operator-bridge-q63 --status open --append-notes "Blocked: real capture row contract or private capture inputs unavailable. Checked only sanitized contract availability. No synthetic, mock-only, or placeholder result was accepted as real capture."
bd defer rom-operator-bridge-q63 --until="+14d"
bd dolt push
git status --short --branch
```

If another repo or operator handoff must supply the missing contract, create or
update an explicit bead dependency or human/private handoff bead, add that bead
as a dependency of `q63`, and treat `bd defer` only as a retry reminder. A
sanitized request file under `$PRIVATE_REQUEST_DIR` may support the handoff, but
it must not replace bead state.

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
