# Worker InjectInputs Wiring

## Command Types

Extend the real worker command loop in `service/src/backend.rs`.

Add a command and outcome:

```rust
RealWorkerCommand::InjectInput {
    lease: dh::Lease,
    target_frame: u32,
    pad_word: PadWord,
    reply: mpsc::Sender<RealWorkerResult<RealInjectInputOutcome>>,
}

struct RealInjectInputOutcome {
    scheduled: u32,
}
```

Add a public thread method:

```rust
fn inject_input(
    &self,
    lease: dh::Lease,
    target_frame: u32,
    pad_word: PadWord,
) -> RealWorkerResult<RealInjectInputOutcome>
```

Wire it through:

- `run_real_worker_thread`
- `reply_unavailable`
- `RealWorkerState::inject_input(...).await`

## Tonic Call

`RealWorkerState::inject_input` should:

1. Ensure the client is connected.
2. Build a single `InjectInputsRequest`.
3. Call `client.inject_inputs(...)`.
4. Map `tonic::Code::InvalidArgument` to a stale-frame worker failure only after
   local validation has excluded bridge-side malformed requests, especially
   `target_frame >= u32::MAX`.
5. Map all other status codes to backend unavailable unless the code has an
   existing more specific internal variant.
6. Return the scheduled count.

Recommended failure enum extension:

```rust
enum RealWorkerFailure {
    BackendUnavailable,
    FailedPrecondition,
    FrameStale,
}
```

Do not include worker status messages in `RealWorkerFailure`; the public error
path must stay sanitized.

## Backend Error Mapping

`RealBackend::inject_input` should translate:

- `RealWorkerFailure::FrameStale` to `BackendError::FrameStale`
- `BackendUnavailable` and `FailedPrecondition` to `BackendUnavailable`

When returning `FrameStale`, include:

```rust
requested_frame: request.target_frame,
current_frame: active.current_frame,
```

If the backend refreshed the frame base before returning, use the refreshed
`current_frame`.

Do not map local validation failures such as `target_frame == u32::MAX` to
`FrameStale`. Those are bridge-side request construction failures and should
stay sanitized as unavailable unless a more precise internal error is added.

## Scheduled Count

The bridge sends exactly one event. Treat any response other than
`scheduled == 1` as `BackendUnavailable`.

Do not write padlog artifacts when scheduled count is not exactly one.

## Request Privacy

The worker request contains:

- private lease;
- target frame;
- pad word.

Keep the lease private. It must not appear in:

- public JSON responses;
- websocket replies;
- padlog text;
- bridge events intended for public documentation;
- test failure messages copied into docs.

Pad words and assigned frame numbers are allowed in private padlog artifacts and
browser acks.

## Refresh Command

If the implementation adds `RealWorkerThread::frame_counter`, use a separate
command instead of overloading `Framebuffer`.

Suggested shape:

```rust
RealWorkerCommand::FrameCounter {
    lease: dh::Lease,
    reply: mpsc::Sender<RealWorkerResult<RealFrameCounterOutcome>>,
}

struct RealFrameCounterOutcome {
    frame: FrameCounter,
    icount: u64,
}
```

Keep raw framebuffer pixels, snapshot refs, input log ids, and worker status
strings inside the worker method. Return only the frame counter and icount to
`RealBackend`.
