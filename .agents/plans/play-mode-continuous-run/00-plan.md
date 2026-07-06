# Plan: "Play" mode — continuous auto-advance run with live frame streaming + input

Tracking: **rom-operator-bridge-9mk**. Scope: `rom-operator-bridge` only
(`service/` + `ui/`). **No cross-repo request needed** — see §9.
Revised after two adversarial code-verified reviews (architecture + requirements).

## Goal / requirements

Once **Start** is pressed, the browser shows **Play** alongside
**Pause / Resume / Stop**:
- **Play** — the ROM continuously emits frames; the browser displays them live;
  input can be sent while Play is in effect.
- Frames may arrive out of order (network) → always display the **newest** frame
  and drop earlier ones.
- **Pause** — pauses emulator processing **and** frame emission.
- **Resume** — advances exactly **one** frame (and you can keep single-stepping).
- **Play from paused** — returns to continuous Play.

## Current architecture (verified) — the constraints that shape this

- **Session/auth:** single active session; cookie `rom_operator_bridge_session`,
  `SESSION_TTL_SECONDS = 4h` (auth.rs:17). Two WebSockets today: `/ws/input`
  (input scheduler) and `/ws/events` (broadcast state pushes). Frames are
  pull-only (`GET /api/frame/current[/image]`).
- **State (backend.rs:143-152):** `Idle/Starting/Running/Paused/CapturePending/
  Stopping/Stopped/Faulted`. **`start_session` lands in `Running`** (backend.rs:373).
  **`resume` ends in `Running` (Synthetic, backend.rs:515) or `Paused` (Real,
  backend.rs:1269)** — asymmetric today. `RunTransition = {Pause, Resume}`
  (api.rs:2332).
- **Worker command path:** `RealWorkerThread` is a dedicated `std::thread` with a
  1-worker tokio runtime; all worker RPCs go through **one FIFO mpsc channel**
  (backend.rs:1756, 2019-2064). FIFO **yields a queue slot between each recv and
  the next send** — so a command enqueued by an API thread is serviced after at
  most one in-flight Run (~150 ms); the 20 s per-RPC timeout is never threatened.
- **Input (service/src/input/scheduler.rs):** `input_acceptance` (642-650) only
  has arms `(Synthetic,Running)`, `(Synthetic,Paused)=>Queue`, `(Real,Paused)=>Apply`;
  everything else → **Reject**. `submit` Applies-or-Queues on arrival; `flush`
  Applies queued. `DEFAULT_INPUT_LEAD_FRAMES = 1` (scheduler.rs:16). Target frame
  = `current + lead`; worker requires `at_frame > current` (dh service.rs:1655),
  bridge requires `target > current` else `FrameStale` → auto re-target
  (scheduler.rs:491). `RealBackend::inject_input` **hard-requires `Paused`**
  (backend.rs:1311).
- **Worker (v1):** `Run{frame_budget=N, capture}` stops on the Nth pv-pad
  `FRAME_COUNTER`; with `CaptureSpec{framebuffer:true}` returns `fb_lz4 + fb_info`
  at the boundary (proto:98-112,202,239-240; read-only, ARCH §6.10 C5). Bridge
  **already** decodes lz4 (`lz4_flex::decompress_size_prepended`, backend.rs:911).
  `InjectInputs at_frame` = absolute `FRAME_COUNTER`, must be queued before the
  Run reaches it. **`RunWithFrameCapture` streaming is UNIMPLEMENTED** (dh
  service.rs:4709) and forbids mid-Run input (single-actor slot, runtime.rs:166-250).
  `frame_counter` **restarts at 0 each run**. No wall-clock pacing — compute-bound
  (~25M instr/frame ≈ single-digit fps for the deployed game).

## Design

### Engine: a dedicated-thread per-frame Run loop (not streaming) — SOLID, verified

`RunWithFrameCapture` is unimplemented **and** cannot interleave input, so the
per-frame loop is the *only* v1 model that satisfies "input during Play." The
loop, per iteration:
1. **Flush** any input the client buffered for upcoming frames (see §Input).
2. `Run{frame_budget=1, capture=CaptureSpec{framebuffer:true, ranges:[]}}` —
   advance one frame and get `fb_lz4 + fb_info` in the **same** RPC (also fixes
   the `resume` `fb_info=None`/`current_frame` bug: today `resume` passes
   `capture:None`, backend.rs:2221). **Do not** reuse `capture_spec()`
   (backend.rs:792) — that is the *snapshot* spec and **seals the input log**; use
   the plain `Run.capture` field.
3. lz4-decode → PNG → hand `{frame_counter, png}` to the frames publisher.
4. Update `current_frame`; publish `run_updated`; check the stop flag; repeat.

**Thread home (precise):** a **dedicated `std::thread` per Play session** calling
the blocking backend methods — **not** a tokio task on the axum runtime (would
block an axum worker for minutes) and **not** inside `RealWorkerThread` (it must
stay free to service Status/Stop). The loop uses the same FIFO command channel;
because FIFO yields each frame, Stop/Status/Pause enqueued elsewhere are serviced
within ~1 frame.

**New backend step method** — the loop **cannot reuse `resume()`** (which requires
*and* restores `Paused`, backend.rs:1212/1269). Add `RealBackend::play_step()`
(and a Synthetic equivalent): precondition `state==Playing`, run one captured
frame, leave `state==Playing`, return `{frame_counter, fb}`. Extend
`RealRunOutcome` (backend.rs:1946) to carry `fb_lz4`/`fb_info`.

### State machine (corrected against the code)

Add `SessionState::Playing` and `RunTransition::Play`. **Make single-step land in
`Paused`** in *both* backends (an explicit behavior change: Synthetic `resume`
currently leaves `Running`) so you can keep stepping. Define `Running` fate:
treat post-Start `Running` as "live-idle, ready to step or play" (or retire it by
landing Start in `Paused` — pick one and thread it through; retiring `Running` is
cleaner but touches more tests — default: **keep `Running` as the post-Start
ready state**, enable Play/Resume from it).

Full button-enablement matrix (drive off `run_updated.state`):

| State | Play | Pause | Resume | Stop |
|---|---|---|---|---|
| Starting | – | – | – | – |
| Running (post-Start ready) | ✓ | – | ✓ | ✓ |
| Paused (after a single step) | ✓ | – | ✓ | ✓ |
| Playing | – | ✓ | – | ✓ |
| CapturePending | – | – | – | ✓ |
| Stopping/Stopped | – | – | – | – |
| Faulted | – | – | – | ✓ (reset) |

`play_run` (`POST /api/run/play`) is **fire-and-forget**: auth +
`ensure_active_session`, set `state=Playing`, start the loop thread, return
`Playing` immediately (unlike synchronous `run_state_transition`). Thread
`Playing` through **every** consumer: `input_acceptance`, `inject_input` guard,
`RunStatus`/`RunBoundary` serialization, `run_status` HTTP, and the UI
`SessionState` union (runtimeClient.ts:284+, app.ts state map).

### Pause / Stop / fault (flag stops the loop — corrected rationale)

Pause/Stop/fault set a per-session **stop flag** (an `AtomicBool`/`watch`) that
the loop checks between frames and exits within one frame (~150 ms). The flag is
**not** needed to "get Pause past the loop" (FIFO already services Stop within a
frame); it exists so the loop **stops issuing Runs against a torn-down slot**
after Stop (`cleanup_runtime_session` → `stop_session`, api.rs:2147-2171) —
otherwise the next Run faults against a dead slot and spams. Stop /
`cleanup_runtime_session` / `SessionReplaced` (api.rs:2002) must **set the stop
flag and join/detach the loop thread before/around** tearing down the slot. On
**fault** during Play: stop the loop, publish `run_updated{state:Faulted}`, and
close `/ws/frames`.

### Input during Play (the part the first draft got wrong)

Three concrete bridge changes are required — a UI change alone is insufficient:
1. **`input_acceptance` (scheduler.rs:642-650):** add `Playing` arms. It is shared
   by `submit` (arrival) and `flush`, but Play needs **opposite** behavior:
   `submit` must **Queue** `(Real|Synthetic, Playing)`; `flush` must **Apply**.
   Make the function **context-aware** (pass a `Context::{Submit,Flush}` flag) or
   split it, so during Play arrivals buffer and only the loop's flush injects.
2. **`RealBackend::inject_input` guard (backend.rs:1311):** relax `state != Paused`
   to also accept `Playing`.
3. **Loop is the sole flusher during Playing.** The `/ws/input` handler only
   enqueues; the loop drains before each frame's Run, so input never contends on
   the worker channel and lands at its deterministic `at_frame` boundary.

**Lead frames:** `DEFAULT_INPUT_LEAD_FRAMES = 1` is too tight for a ~150 ms
cadence — inputs arriving after the loop advanced bounce one frame forward
(`FrameStale` → re-target; not dropped, but latency). During Play use a larger
lead `k` (`with_lead_frames(k)` sized to observed RTT-in-frames). Note the UX
cost (§UX): at low fps, `k` frames of lead is `k/fps` **seconds** of input lag —
keep `k` minimal and surface an "input queued" affordance.

### Frame delivery: a dedicated binary frames WebSocket

Add `GET /ws/frames` (authenticated like `/ws/input`/`/ws/events`; register in
`router()` api.rs:1008). Use a **`tokio::watch`** channel (loop = sender, WS task
= receiver): it inherently holds only the **latest** frame and never blocks the
producer — a cleaner "always-newest / drop-oldest" than a bounded mpsc. Each WS
message is binary `[u64 frame_counter LE][PNG bytes]`.

Client rule: keep `lastDisplayedFrame`; render a message **only if
`frame_counter > lastDisplayedFrame`**, else drop — satisfies "newest, drop
earlier." `frame_counter` (deterministic, strictly increasing within a run) **is**
the ordering key; no wall-clock timestamp needed. **Reset `lastDisplayedFrame`
to `-1` on `run_id` change** (frame_counter restarts at 0 each run, so without
this every frame after Stop→Start is dropped as "older" — the picture would
freeze).

### UI

1. **Play button** in `renderSessionPanel` with the matrix above;
   `data-run-action="play"` → `POST /api/run/play`. Extend the client
   `SessionState` union with `"playing"`.
2. **Frames-WS client** built on the existing `RuntimeSocket` reconnect infra
   (runtimeClient.ts:655-767, backoff/`maxAttempts`) — a transient frames-WS drop
   must **not** stop the server loop; on reconnect the `frame_counter` rule
   auto-re-syncs to the newest.
3. **Live-frame render path (declarative):** store the current object URL in the
   model and render it in `renderPreviewImage`; on a newer frame,
   `URL.createObjectURL(new Blob([png]))`, swap, and **revoke the previous URL**.
   Do **not** route live frames through `preview.image_url` — that field is
   pattern-validated (`FRAME_IMAGE_PATTERN`, runtimeClient.ts:160) and rejects
   `blob:`. Fall back to pull `refreshPreview()` when paused/single-step.
4. **Input during Play:** widen `inputControlsDisabled()` (gates on `"running"`/
   `preview_stale`) to allow `"playing"`.
5. **Indicators:** a `LIVE / PAUSED / BUFFERING` badge + an fps/frame-counter
   readout so the compute-bound pace is honest and visible.

## Determinism (wording corrected)

The worker is deterministic **given a fixed resolved `at_frame` list**. Live Play
is **not** bit-reproducible from human button-presses, because `at_frame` is
assigned as `current + lead` at flush time and depends on the wall-clock arrival
of WS messages relative to loop speed. This is expected, not a bug. The
determinism **test** must therefore drive a fixed resolved `at_frame` schedule
(same schedule → identical frame hashes); `Run` capture is read-only and does not
perturb state.

## No cross-repo request (verified)

`Run{frame_budget, capture}` + `InjectInputs{at_frame}` + `Pause`/`GetFramebuffer`
fully cover the loop; `RunWithFrameCapture` is unimplemented **and** cannot
interleave input; `hard_icount_cap` (1e10) backstops a hung frame and each
iteration is one bounded frame. **We do not file a request.** The `frame_budget=k`
batching escape hatch is genuinely bridge-only (`RunRequest` takes any budget) but
delays the input flush by up to `k` frames — **bound `k` to a small ceiling
(e.g. ≤4) or defer it out of v1**; try minimal-lead single-frame first.

## Edge cases / lifecycle (promoted from open-questions)

- **4h cookie TTL mid-Play:** a "watch it play" session outlives the TTL. On
  expiry: set the stop flag, close `/ws/frames`, emit a terminal state, and
  surface re-auth in the UI. Add a lifecycle test.
- **Fault during Play:** stop loop, `run_updated{Faulted}`, close frames WS,
  reset buttons.
- **Stop / `SessionReplaced` during Play:** stop flag + thread join before slot
  teardown.
- **Frames-WS reconnect:** server loop keeps running; client re-syncs via the
  frame_counter rule.
- **Capture/label during Play:** they need a `Paused` frame and would queue behind
  the loop — **block them in the UI while `Playing`** (as capture already needs a
  settled frame).

## Testing

- Backend: `Playing --Pause--> Paused` within one frame; `Paused --Resume-->
  Paused, current_frame+1` (both backends); `play_step` advances + returns fb;
  input buffered during Play is Queued on submit, Applied on flush, and lands at
  its target frame; determinism (fixed resolved `at_frame` schedule → identical
  hashes); frames `watch` yields only the newest under a slow consumer; TTL/fault/
  Stop teardown stops the loop + closes the frames WS.
- UI: full button matrix across all states; frames render-if-newer drops an
  older/reordered message; `lastDisplayedFrame` resets on `run_id` change; input
  enabled during Play; frames-WS reconnect re-syncs.
- Synthetic backend first, then real; end-to-end against the deployed real ROM:
  Start → Play → picture animates; Pause freezes; Resume steps one; input affects
  the game.

## Deployment (operator-gated)

Build UI (`npm run build`) + bridge (release), install to
`/opt/rom-operator-bridge/current`, restart the service — same operator-gated
runbook as the existing deploy. **No worker/snapshot cutover** (bridge/UI only).

## Acceptance criteria

1. After Start, Play/Pause/Resume/Stop show with the matrix enablement (Play
   usable immediately from the post-Start state).
2. Play streams frames live; the browser always shows the newest frame and drops
   older/reordered ones; the picture recovers after Stop→Start (run-id reset).
3. Input works during Play (accepted in `Playing`, buffered on submit, applied by
   the loop) and deterministically affects the game at its target frame.
4. Pause halts emulation + emission within one frame; Resume advances exactly one
   and stays paused; Play resumes continuous.
5. TTL/fault/Stop lifecycle stops the loop + closes `/ws/frames`; determinism
   (fixed schedule), bounded-memory, and reconnect tests pass; guest/worker
   unchanged; no cross-repo change.
