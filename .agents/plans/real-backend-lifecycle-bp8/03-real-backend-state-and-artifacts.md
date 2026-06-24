# Real Backend State and Private Artifacts

## State Model

Replace the current stateless `RealBackend` with a stateful type:

```rust
pub struct RealBackend {
    runtime_config: RealRuntimeConfig,
    private_config: BridgePrivateConfig,
    worker: RealWorkerThread,
    inner: Arc<Mutex<RealBackendInner>>,
}

struct RealBackendInner {
    active: Option<RealSession>,
    next_sequence: u64,
    next_event_seq: u64,
}

struct RealSession {
    session_id: SessionId,
    run_id: RunId,
    lease: dh_proto::v1::Lease,
    state: SessionState,
    current_frame: FrameCounter,
    current_icount: u64,
    last_preview_frame: FrameCounter,
    last_applied_input_frame: FrameCounter,
    capabilities: BackendCapabilities,
}
```

`AppState::from_config` should pass both the real runtime config and the private
config clone:

```rust
RealBackend::new(config.private_config().clone(), real_runtime.clone())
```

The lease token must remain inside `RealSession`. Never put it in public API
responses.

## Capabilities

For bp8, real backend capabilities should reflect only lifecycle support:

```rust
BackendCapabilities {
    input: false,
    preview: false,
    capture: false,
    labels: false,
    privileged_features: false,
    validation_runner: false,
}
```

Even after real start works, keep `input`, `preview`, and `capture` false until
the downstream beads implement those paths. This means `start_session` can
succeed with no granted capabilities. API clients requesting preview/input will
receive those capabilities as false in the start response.

## Run IDs

Use deterministic local IDs that do not include private worker identifiers:

```text
real-run-0000
real-run-0001
real-session-0000
real-session-0001
```

Avoid embedding `slot_id` in public IDs. Slot id is not a secret by itself, but
keeping it out of public IDs reduces accidental correlation with worker state.

## Private Artifacts

Reuse `PrivateArtifactStore` for the normal run manifest and bridge events:

- `runs/<run_id>/run-manifest.json`
- `runs/<run_id>/bridge-events.jsonl`

Add a private-only real session file if the implementation needs durable
operator diagnostics:

```text
runs/<run_id>/real-session.json
```

Suggested schema:

```json
{
  "schema_version": 1,
  "run_id": "real-run-0000",
  "backend_mode": "real",
  "slot_id": 3,
  "start_source": "snapshot",
  "current_frame": 12,
  "current_icount": 0
}
```

Default bp8 behavior is to avoid persisting the lease token. If a later operator
requirement needs lease persistence for crash cleanup, it must be under the
private root, mode `0600`, never copied to public artifacts or logs, and covered
by sanitizer tests.

## Private Event Types

Append bridge events for:

- `session_started`
- `session_paused`
- `session_resumed`
- `session_stopped`
- `session_faulted`
- `cleanup_failed`

Messages must be generic:

```text
real backend session started
real backend cleanup failed
```

Do not include tonic error messages, endpoint paths, snapshot refs, lease
tokens, or private file paths.
