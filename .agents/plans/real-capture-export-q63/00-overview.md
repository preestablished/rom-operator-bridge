# Real Capture Export Integration For q63

## Target Bead

Implement `rom-operator-bridge-q63`: wire real capture export integration.

Completing this bead unblocks `rom-operator-bridge-r77` (`Run real one-capture
label smoke`). That then unblocks `rom-operator-bridge-opw` and one of the
remaining inputs to final acceptance.

## Current State

Already available in this repo:

- Real backend session lifecycle is implemented and live RestoreSnapshot
  acceptance passed.
- Real framebuffer preview and real input paths have established backend seams.
- Synthetic capture and labeling APIs already exist.
- `PrivateArtifactStore` writes private run artifacts, recent capture metadata,
  and label drafts with private file permissions.
- `service/src/verifier.rs` expects `captures/index.jsonl` as the capture index
  path.

Still missing for `q63`:

- `RealBackend::trigger_capture` and `RealBackend::capture_job` currently return
  `BackendUnavailable`.
- There is no bridge-owned durable writer for real capture payload files plus
  `captures/index.jsonl` rows.
- Real capture completion is not connected to the hypervisor capture RPC surface.
- The real backend capabilities still report `capture: false`.

## Success Criteria

`q63` is complete when:

- A real session can trigger one capture through the bridge capture API.
- The bridge calls the real hypervisor capture/export path for the active lease.
- Private payload artifacts are written durably under the private root.
- `captures/index.jsonl` is appended and fsynced before the job reports
  `completed`.
- Public API responses expose only sanitized job metadata and stable public
  capture ids.
- Raw capture bytes, private paths, feature bytes, refs, worker endpoints, and
  private artifact refs never appear in public JSON, websocket events, logs, or
  bead notes.
- If the capture row contract, capture-spec resolver, or operator-approved
  private layout/map inputs are unavailable, the executing agent records a
  sanitized blocker and leaves `q63` deferred/open instead of faking acceptance.

## Non-Goals

- Do not run `r77`; that bead owns the operator-private one-capture label smoke.
- Do not implement validation runner behavior for `r3z`.
- Do not expose screenshots or raw capture payloads through the public API.
- Do not count synthetic capture behavior as real capture acceptance.

## Plan Files

| File | Purpose |
|---|---|
| `00-overview.md` | Target, scope, success criteria |
| `01-contract-and-preflight.md` | Confirm real hypervisor and capture-row contracts before coding |
| `02-private-artifact-writer.md` | Add durable capture payload/index writing |
| `03-real-backend-capture.md` | Wire `RealBackend` capture lifecycle |
| `04-api-state-and-privacy.md` | Align API job state, capabilities, and sanitization |
| `05-tests-and-smoke.md` | Focused tests and optional private host smoke |
| `06-closeout.md` | Bead update, undefer downstream, and push protocol |
