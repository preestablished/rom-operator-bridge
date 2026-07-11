# Bridge Regression Tests

## 1. `4zn`: Play Lifecycle And Frame-Slot Cleanup

Extend the API-level Play harness rather than testing only `PlayController`.
The assertion must cross the route/backend/thread boundary that previously left
a stale frame visible.

Add controllable synthetic or mock-backend behavior and a bounded polling
helper. Cover these cases:

1. A Play stream emits at least one frame, then faults. Assert status leaves
   `playing`, the terminal `run_updated` is emitted, the Play handle is
   deregistered, and a fresh frames subscriber receives no retained frame.
2. Authentication TTL expires after a frame while Play is active. Advance the
   existing test clock, wait for loop exit, and assert the handle and retained
   frame are cleared without issuing more backend steps indefinitely.
3. Stop and session replacement set the stop flag and join the old loop before
   backend/session teardown. Use call-order synchronization in the fake rather
   than sleep-based timing, and assert the old loop cannot publish after the
   new session is registered.
4. An immediate `play_stream_start` failure and an already-expired auth state
   cannot deregister before registration and leave a completed stale handle.
   The current route spawns the loop thread before `PlayController::register`;
   fix this with a start barrier or an atomic controller spawn/register
   operation so the loop body cannot self-deregister first. Assert
   deterministically that no handle remains.

Test streaming and per-frame fallback where their termination paths differ.
Use short deterministic frame/read controls; never make CI wait for the real
250 ms timeout or four-hour TTL.

Likely files:

- `service/tests/ws_events/main.rs` extended with a controllable Play backend
  and frames-socket helpers, or a focused new
  `service/tests/play_lifecycle/main.rs`, for route/socket lifecycle coverage;
- `service/tests/real-backend/main.rs` only for real adapter stream-terminal
  classification; and
- the existing `AuthState::fixed_for_tests` / `advance_for_tests` clock for TTL
  coverage. Make the fake stream return `TimedOut` immediately after the clock
  advance so CI does not wait for the real 250 ms read timeout.

Close `4zn` only after the focused tests and the full service suite pass.

## 2. `y4g`: UI Live-Frame Ordering

Implement the decode/paint seam chosen in `01-current-state-and-boundaries.md`.
Keep the wire format unchanged: eight-byte little-endian `u64` followed by PNG.

Unit-test the extracted state machine with deferred decoder promises and fake
bitmaps that record `close()` calls:

- frame N paints; an already-received frame N or N-1 is rejected before
  decode;
- N+1 resolving before N cannot be painted backward, and the discarded bitmap
  is closed;
- a run-id change resets the accepted counter so a lower frame in the new run
  is allowed, synchronously closes the old retained bitmap, and clears the old
  canvas before the first new-run decode;
- a decode started for the old run is discarded and closed if it resolves
  after the run changes;
- malformed payloads and decode failures do not paint or replace the retained
  bitmap; and
- replacing a painted bitmap closes the previous retained bitmap.

Parse and compare counters as `bigint`; include adjacent values around `2^53`
to prove ordering remains exact. Treat a payload shorter than nine bytes as
malformed because a valid message needs the eight-byte prefix and nonempty PNG
body. Preserve newest-received semantics explicitly: a well-shaped frame marks
its counter accepted before async decode, decoder rejection does not replace
the painted bitmap, and a retransmission of the same counter remains ignored.

Retain one mount-level test proving the frames socket supplies the current
`run_id` to the seam and that input controls remain enabled in `playing` state.
Mock `createImageBitmap`; do not assert pixel rendering through jsdom canvas.

Likely files:

- new `ui/src/liveFrame.ts`;
- new `ui/tests/session-play/liveFrame.test.ts`; and
- a small integration addition to `ui/tests/session-play/sessionPlay.test.ts`.

Run `npm --prefix ui run typecheck`, the focused Vitest target, and the full UI
suite before closing `y4g`. Record the decode/paint-seam decision on the bead.

## 3. `k1b`: `/ws/events` Throttle Rate

Extract a pure throttle policy parameterized by supplied instants. Unit-test
the exact-before, exact-at, and exact-after 250 ms boundaries. Then add one
API/websocket integration test over the streaming Play loop; the fallback
per-frame loop is intentionally unthrottled.

Assert all of the following:

- after classifying and excluding the initial websocket snapshot and Play-start
  boundary, intermediate updates stay below a conservative upper bound over a
  bounded observation window;
- frames continue advancing faster than event publication, proving the test is
  exercising throttling rather than a slow producer; and
- Stop emits exactly one terminal boundary event even if it occurs inside the
  throttle interval.

Do not assert exact client receipt spacing: event payloads carry no monotonic
publication timestamp and scheduler/network batching makes that flaky. Do not
weaken the integration assertion to only “fewer events than frames.”

## 4. Worker-Side Half Of `k1b`

Inspect determinism-hypervisor's current tracker before filing anything. If an
existing issue covers the combination, add the bridge acceptance details to
it. Otherwise, in that repository create one issue requiring a worker test for
`Run` with `FrameBudget(1)` and `CaptureSpec`, asserting the terminal response
contains the same valid `fb_lz4`/`fb_info` contract as the icount-budget capture
path.

Record the sibling issue ID and eventual commit/test name on `k1b`. Close
`k1b` only when both the bridge assertion and the hypervisor assertion have
landed; otherwise keep it open with the bridge half completed and the explicit
cross-repo blocker. Follow the sibling repository's own `AGENTS.md`, beads,
quality, commit, and push protocol for any changes made there.

Any sibling tracker mutation is an independently complete transaction. The
issue/note must be remotely durable before citing it here; a handoff records
its ID/URL, explicit owner, and blocker. Run sibling beads sync and publication
even when no sibling source file changes.
