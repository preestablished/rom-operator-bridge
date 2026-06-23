# Real Backend Availability Decision

Date: 2026-06-23
Agent: Codex / Ralph iteration 7

## Decision

Real backend and real capture exporter work are not available for agent-runnable
implementation now.

Implementation may proceed on synthetic backend behavior, shared runtime API
types, backend traits, service scaffolding, and fail-closed real-backend
configuration surfaces. Work that must prove a real ROM session, real
framebuffer, real input injection, or real capture export remains deferred until
the private operator prerequisites below are explicitly available.

## Real Backend Status

Available contract:

- Attach to existing `dh-workerd`.
- Default worker socket: `/run/dh/grpc.sock`.
- Worker RPC surface includes `RestoreSnapshot`, `CreateVm`, `Pause`, `Run`,
  `DestroyVm`, `InjectInputs`, `GetFramebuffer`, `TakeSnapshot`, `WatchSlots`,
  and `ListSlots`.
- Bridge real mode must fail closed unless uncommitted service configuration
  provides required private runtime inputs.

Unavailable prerequisite:

- No operator-approved private snapshot is configured.
- No exact `CreateVm` ROM startup config is recorded.
- No service implementation exists yet to load the real-mode config and return
  sanitized `backend_unavailable` errors.

Required undefer inputs:

```text
BRIDGE_HYPERVISOR_ENDPOINT, default unix:///run/dh/grpc.sock
BRIDGE_PRIVATE_ROOT
BRIDGE_WORKLOAD_IMAGE_REF
BRIDGE_CAPTURE_SPEC_REF
BRIDGE_REFERENCE_WORKLOAD_CHECKOUT
BRIDGE_REAL_SNAPSHOT_REF or BRIDGE_CREATE_VM_CONFIG_REF
```

The real backend attachment bead must stay deferred until all of these proof
steps are available:

1. Operator records the private config source that supplies the `BRIDGE_*`
   values above.
2. `dh-workerd` is running at the configured endpoint, or at the default
   `unix:///run/dh/grpc.sock`.
3. The bridge service user can open the worker socket.
4. The configured snapshot ref or `CreateVm` startup ref resolves through the
   private config validator.
5. A sanitized probe records that missing config returns `backend_unavailable`
   without private details, and complete config can distinguish an attachable
   real session from an unavailable backend.

## Real Capture Export Status

Available contract:

- Hypervisor `Run` or `TakeSnapshot` can return boundary capture bytes through
  `CaptureSpec`.
- `reference-workload` validates `captures/index.jsonl` and private bundle
  artifacts.
- Real capture completion requires fsynced private payload files, an appended and
  fsynced `captures/index.jsonl` row, and no UI exposure of raw payloads,
  private paths, decoded arrays, capture ids, or artifact refs.

Unavailable prerequisite:

- No bridge-ready exporter exists in `reference-workload`.
- No bridge-owned writer exists yet for private payload files and
  `captures/index.jsonl`.
- No private feature map, scoring program, layout, private root, or real ROM
  session is configured for an agent-runnable capture.

Required undefer inputs:

```text
rom-operator-bridge-2sn closed with bridge-owned durable capture writer support
private capture root
feature-map.yaml
scoring-program.yaml
layout.json
real backend session inputs
operator-approved private artifact policy
```

The real capture export bead must stay deferred until `rom-operator-bridge-2sn`
or its successor provides the durable private payload and `captures/index.jsonl`
writer. `rom-operator-bridge-q63` owns the remaining real hypervisor capture
adapter after that writer exists and the private operator inputs are available.
Synthetic capture work must not be represented as Phase 4 real acceptance.

## Downstream Bead Status

Defer now:

- `rom-operator-bridge-bp8` (`Implement real backend attachment lifecycle`)
- `rom-operator-bridge-q63` (`Wire real capture export integration`)

Already deferred and still correct:

- `rom-operator-bridge-0wo` (`Document and run real backend smoke`)
- `rom-operator-bridge-r77` (`Run real one-capture label smoke`)
- `rom-operator-bridge-opw` (`Validate bridge-produced private bundle`)

Blocked by deferred real backend attachment:

- `rom-operator-bridge-0i9` (`Wire real framebuffer preview source`)
- `rom-operator-bridge-3dr` (`Wire real frame-boundary input injection`)

Still unblocked:

- Runtime API and backend trait design.
- Host service scaffold.
- Synthetic session/status routes.
- Synthetic capture and label behavior.
- Sanitized `backend_unavailable` and `capture unavailable` status paths.

## Non-Negotiable Guardrails

- Real-mode endpoints must fail closed when private config is absent.
- Real capture jobs must not report `completed` until private artifacts and the
  capture index row are durable.
- Synthetic captures must stay visibly synthetic in API and UI state.
- Public notes must not contain private paths, screenshots, raw reports, capture
  ids, decoded values, or artifact refs.
- Private operator evidence for real-backend undefer or real-capture acceptance
  must stay under the private root. Public summaries may state only sanitized
  status, command shape, and non-sensitive failure class.
