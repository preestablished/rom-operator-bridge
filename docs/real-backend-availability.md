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
BRIDGE_PRIVATE_ROOT
BRIDGE_WORKLOAD_IMAGE_REF
BRIDGE_CAPTURE_SPEC_REF
BRIDGE_REFERENCE_WORKLOAD_CHECKOUT
BRIDGE_REAL_SNAPSHOT_REF or BRIDGE_CREATE_VM_CONFIG_REF
```

The real backend attachment bead must stay deferred until those inputs are
available or until an operator explicitly supplies the equivalent private config
through the uncommitted service configuration.

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
bridge-owned durable capture writer
private capture root
feature-map.yaml
scoring-program.yaml
layout.json
real backend session inputs
operator-approved private artifact policy
```

The real capture export bead must stay deferred until the writer exists and the
private operator inputs are available. Synthetic capture work must not be
represented as Phase 4 real acceptance.

## Downstream Bead Status

Defer now:

- `rom-operator-bridge-bp8` (`Implement real backend attachment lifecycle`)
- `rom-operator-bridge-q63` (`Wire real capture export integration`)

Already deferred and still correct:

- `rom-operator-bridge-r77` (`Run real one-capture label smoke`)
- `rom-operator-bridge-opw` (`Validate bridge-produced private bundle`)

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
