# Hypervisor Runtime Contracts

Date: 2026-06-23
Agent: Codex / Ralph iteration 3

## Scope

This is a Phase 0 discovery note for how the ROM operator bridge can attach to
the `determinism-hypervisor` worker runtime for lifecycle, input injection,
frame alignment, preview, snapshot, and capture/export behavior.

## Checkout

```text
path: /home/infra-admin/git/preestablished/determinism-hypervisor
commit: b9737538f5fc2708d9cb09979df775c0ab388390
status: clean on main...origin/main
```

## Decision

Use `dh-workerd` as the real hypervisor boundary for a future non-synthetic
bridge backend, but keep the bridge's first implementation behind an interface
that can still run the synthetic backend.

The bridge should attach to the worker over the Unix domain socket by default:

```text
/run/dh/grpc.sock
```

Use TCP only as an explicit operator deployment choice. The worker binary's
defaults include TCP `0.0.0.0:7400`, HTTP `0.0.0.0:7401`, and UDS
`/run/dh/grpc.sock`, and the service serves TCP even when UDS is enabled. For
this private bridge, start `dh-workerd` with TCP and HTTP rebound to loopback,
or firewall those ports from every network except the trusted host path:

```sh
dh-workerd serve \
  --tcp 127.0.0.1:7400 \
  --http 127.0.0.1:7401 \
  --uds /run/dh/grpc.sock
```

The bridge should use the UDS path and socket filesystem permissions for its
same-host worker client. Do not expose `dh-workerd` directly to the LAN; the
browser-facing surface is the authenticated bridge service, not the worker gRPC
or worker HTTP endpoint.

## Worker Surface

The worker service is:

```text
determinism.hypervisor.v1.HypervisorWorker
```

Implemented lifecycle and runtime RPCs relevant to the bridge:

```text
CreateVm
RestoreSnapshot
Fork
DestroyVm
InjectInputs
Run
Pause
TakeSnapshot
ReadGuestMemory
GetFramebuffer
StreamGuestEvents
VerifyReplay
GetWorkerInfo
ListSlots
WatchSlots
```

Important status of phase-later RPCs:

```text
RunWithFrameCapture: UNIMPLEMENTED in dh-worker
Quiesce: UNIMPLEMENTED in dh-worker
```

Bridge implication: do not design the MVP preview loop around
`RunWithFrameCapture`. The implemented paths are `Run` with optional boundary
capture, `TakeSnapshot` with optional boundary capture, and `GetFramebuffer` on
a paused slot.

## Slot And Lease Ownership

`CreateVm` and `RestoreSnapshot` allocate a `Lease` with:

```text
slot_id: uint64
token: 16 bytes
```

Every state-changing or introspection call validates that lease. The worker owns
slot allocation, runtime-table publication, slot actor startup, and rollback if
allocation fails. `DestroyVm` checks the lease, removes the runtime actor,
destroys the manager slot, and shuts the actor down.

Bridge session shape:

- one bridge session owns one worker lease;
- the session records `slot_id`, token, current absolute frame counter, current
  cumulative icount, and last preview frame;
- the stop path always calls `DestroyVm`;
- if the worker reports a faulted slot or lease validation failure, the bridge
  marks the session failed, stops accepting browser input for that session, and
  requires a fresh `RestoreSnapshot` or `CreateVm`;
- use `WatchSlots` for operator status updates, and resync with `ListSlots` if
  the watch stream reports lag.

`Pause` accepts Paused or Running slots, requests a pause through the slot
actor, and reports cumulative `icount`, `vns`, and state hash. Introspection
helpers such as `GetFramebuffer`, `ReadGuestMemory`, and `StreamGuestEvents`
require the slot to be Paused and abort if the boundary changes before the
introspection runs.

## One Pad Word To A Running ROM

The pad layout for the bridge remains the planned `console16-12btn-v1` word:

```text
bits 0..11: A, B, X, Y, L, R, Up, Down, Left, Right, Start, Select
bits 12..15: reserved, must be zero
```

The hypervisor input event carries a wider `uint32 buttons`, but the bridge
should reject any browser pad word with reserved high bits before it reaches the
worker.

`InjectInputs` is not a live mid-run control path. `Run` clones the pending
inputs at the start of the call, and worker lifecycle calls serialize through
the slot runtime. An input sent while a long `Run` is already active will not
land inside that `Run` and may be stale by the next boundary. The bridge should
therefore run in short bounded steps: refresh or track the frame boundary, queue
pad input for a future absolute frame, then start the next `Run`.

Exact path for one browser pad word:

1. The browser reports the current 16-bit pad word.
2. The bridge validates reserved bits 12..15 are zero and maps player 1 to
   `PadSet.port = 0`.
3. Before the next run step, the bridge reads or tracks the current absolute
   `FRAME_COUNTER` for the session.
4. Before starting that run step, the bridge sends `InjectInputs` with the
   active lease and one event:

   ```text
   ScheduledEvent.at_frame = current_frame_counter + lead_frames
   ScheduledEvent.pad_set.port = 0
   ScheduledEvent.pad_set.buttons = pad_word as u32
   ```

   `lead_frames` must make `at_frame` strictly greater than the worker's current
   frame counter. The minimum legal lead is 1 frame.

5. `dh-worker` validates the lease, validates pv-pad exists in the machine
   config, rejects the reserved frame hint, rejects stale `at_frame` values, and
   queues the input.
6. The next `Run` converts pending frame inputs into run-control scheduled frame
   inputs using the session's current absolute frame counter.
7. At the matching frame boundary, run control applies a canonical `PAD_SET`
   record through `PvPad::apply_pad_set(port, buttons)`.
8. The guest ROM reads `PAD0` from the pv-pad MMIO latch at `0xD000_1000 + 0x08`.

The pv-pad latch changes only through the canonical `PAD_SET` path. MMIO writes
to `PAD0..PAD3` are ignored. If the guest enables pv-pad IRQ delivery, current
frame-scheduled `PAD_SET` handling rejects that mode because frame-scheduled IRQ
delivery is not wired; the polling/default path is the supported bridge target.

## Frame Bases

Frame scheduling uses absolute pv-pad `FRAME_COUNTER` values, never
segment-relative frame numbers.

Sources of the base frame counter:

```text
RestoreSnapshotResponse.frame_counter
TakeSnapshotResponse.frame_counter
GetFramebufferResponse.frame_counter
RunResponse.fb_info.frame_counter when boundary framebuffer capture is requested
```

`CreateVm` starts with `icount = 0`; the pv-pad frame counter starts at 0 unless
the guest advances it. `RestoreSnapshot` restores pv-pad state from the snapshot
and returns the restored absolute frame counter. `TakeSnapshot` samples the same
device state and returns it after sealing the boundary.

The worker rejects:

```text
at_frame == FRAME_HINT_NONE
at_frame <= current_frame_counter
at_frame when the machine config has no pv-pad
at_icount <= current segment icount
```

Bridge scheduling rule:

```text
target_frame = last_known_frame_counter + max(lead_frames, 1)
```

If `InjectInputs` returns `INVALID_ARGUMENT` for a stale frame, the bridge should
refresh the session frame counter from the latest paused/frame-boundary state and
retry once with a future frame. If it is still stale, surface a private operator
status such as `input dropped: stale frame` and do not pretend the input landed.

`RunRequest.frame_budget = N` stops after N frame-boundary exits, and
`RunResponse.frames_elapsed` reports only how many frame marks elapsed. It is not
the final absolute `FRAME_COUNTER`. Current run control requires frame counter
writes to be monotonic, but it does not freeze a contiguous `+1` counter
contract. Do not derive the next absolute frame counter from `frames_elapsed`
unless a later ROM/runtime contract explicitly freezes contiguous frame numbers.

A later bridge implementation can still use short frame-budget runs to advance
the ROM in controlled steps, with input injected for future absolute frames
before each run. After a run, the bridge should treat the frame base as unknown
unless `Run` capture returned `fb_info.frame_counter`; otherwise refresh the
frame counter with `TakeSnapshot` or `GetFramebuffer` before scheduling
additional frame-based input.

## Preview And Staleness

`GetFramebuffer` is implemented only for paused slots. It returns:

```text
width
height
stride
format
frame_counter
icount
pixels
```

`Run` and `TakeSnapshot` can also return framebuffer capture data at a boundary
when a `CaptureSpec` requests `framebuffer = true`; the response uses LZ4 bytes
plus `FbInfo.frame_counter`.

Because `RunWithFrameCapture` is unimplemented, there is no implemented live
streaming framebuffer API for the bridge MVP. The bridge should treat previews
as boundary samples:

- update `session.current_frame_counter` after every successful restore,
  snapshot, framebuffer read, or run capture that reports an authoritative frame
  counter;
- update `session.preview_frame_counter` only when `GetFramebuffer`, `Run`
  capture, or `TakeSnapshot` capture returns image data;
- mark preview stale whenever
  `preview_frame_counter < session.current_frame_counter`;
- mark preview frame unknown and stale after any `Run` that may advance frames
  but does not return `fb_info.frame_counter`;
- also mark preview stale while a run is in progress unless the displayed image
  came from the same boundary that is currently reported to the browser;
- mark preview unavailable, not fresh, if `GetFramebuffer` returns
  `FAILED_PRECONDITION` because the slot is not Paused or the fixture has no
  published framebuffer region.

This rule is conservative: a running slot may be visually ahead of the most
recent sampled image, and the bridge cannot prove freshness without a new
boundary sample.

## Capture And Export

The hypervisor can produce capture bytes, but it does not write the ROM bridge's
durable capture index.

Implemented hypervisor capture path:

1. The bridge sends `Run` or `TakeSnapshot` with `CaptureSpec`.
2. `CaptureSpec.ranges[]` are read from detchannel-published regions after
   manifest and layout-version validation.
3. `CaptureSpec.framebuffer = true` reads the published framebuffer region.
4. The worker returns `feature_bytes`, optional `fb_lz4`, and optional `FbInfo`
   at the paused boundary.
5. `TakeSnapshot` can also seal the input log and return `snapshot`,
   `input_log_id`, `state_hash`, `icount`, `vns`, and `frame_counter`.

What is missing for a real bridge capture:

```text
captures/index.jsonl schema
artifact file naming
artifact fsync/atomic-write policy
reference-workload capture/export CLI or writer
label draft schema coupling
```

Therefore real capture completion is blocked until the reference-workload
discovery bead (`rom-operator-bridge-z8z`) identifies the capture/export writer
and `captures/index.jsonl` row schema.

Required durable completion shape once that gap is closed:

1. Receive boundary capture bytes from `Run` or `TakeSnapshot`.
2. Write private capture artifacts under the configured capture root.
3. Fsync artifact files and the containing directory, or use the
   reference-workload writer if it provides equivalent durability.
4. Append one `captures/index.jsonl` row containing the capture id, boundary
   frame counter, icount, snapshot/input-log refs when present, artifact refs,
   layout versions, and label draft refs required by the frozen schema.
5. Fsync the index file.
6. Mark the browser capture job `completed` only after the durable row exists.

Until then, a bridge may expose synthetic/demo capture status only if the UI and
API clearly label it synthetic and do not count it as Phase 4 real acceptance.

## Failure Handling

Worker errors the bridge must handle explicitly:

```text
INVALID_ARGUMENT: stale frame, missing event fields, bad port, bad ranges
FAILED_PRECONDITION: wrong slot state, missing pv-pad, missing detchannel,
                     missing framebuffer region, stale lease/state
ABORTED: paused boundary changed before introspection
DATA_LOSS: determinism violation or log/capture corruption
RESOURCE_EXHAUSTED: slot watch lag or scheduled input order exhaustion
UNIMPLEMENTED: RunWithFrameCapture and Quiesce
```

Bridge policy:

- stale input errors drop or retry that input only;
- wrong-state preview errors mark preview stale/unavailable;
- `DATA_LOSS`, runtime fault, or faulted slot status ends the bridge session and
  requires a new restore/create;
- `WatchSlots` lag must trigger `ListSlots` resync;
- failed `DestroyVm` is an operator-visible cleanup failure and should not be
  hidden behind a successful browser stop response.

## Agent-Runnable Checks

The discovery was validated by reading the implementation and by relying on the
existing hypervisor tests that cover these contracts:

```sh
cargo test -p dh-worker inject_mapper_accepts_at_frame_pad_set_with_frame_hint
cargo test -p dh-worker inject_mapper_rejects_stale_frame_and_oversized_device_event
cargo test -p dh-worker inject_mapper_rejects_reserved_frame_and_missing_pv_pad
cargo test -p dh-devices frame_counter_write_logs_frame_mark
```

The hardware-gated end-to-end frame scheduling gate is documented in the
hypervisor checkout as:

```sh
cargo test -p dh-worker --test m5_frame_scheduling --release -- --ignored --nocapture
```

That ignored gate requires staged artifacts and KVM dirty-ring support, so it is
an operator gate rather than a normal bridge-repo quality gate.
