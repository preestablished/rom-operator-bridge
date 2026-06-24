# Subagent Review Summary

Two subagents reviewed this plan after the initial draft.

## Backend and Hypervisor Review

Findings applied:

- The draft did not explicitly reject the reserved `FRAME_HINT_NONE` value before
  constructing `ScheduledEvent.at_frame`. The plan now requires local rejection
  of `target_frame >= u32::MAX` so worker `InvalidArgument` can safely represent
  stale input for bridge-constructed requests.
- The draft returned sanitized unavailable if private padlog persistence failed
  after worker acceptance, but did not prevent the queued worker input from later
  running unlogged. The plan now requires quarantining or removing the real
  session and preventing later `resume()`.
- The draft asked the backend to mark a session `Running` during bounded `Run`
  but did not specify cleanup on worker failure. The plan now requires faulting
  or removing the session instead of leaving public status stuck at `running`.

## Scheduler, API, and Privacy Review

Findings applied:

- Runtime websocket and API flush paths currently use `NoopInputRejectionSink`.
  The plan now explicitly requires wiring a private rejection sink through
  websocket submit and API `flush_pending`, with tests proving stale drops write
  private rejection rows.
- Queued real input behavior was ambiguous. The plan now chooses one policy:
  real paused input applies immediately, real running input is rejected, and
  pre-resume flush is a safety net for already-pending entries rather than the
  normal real input path.
- Required `cargo` commands now run from `service/`, where the crate
  `Cargo.toml` lives.
- Sibling `determinism-hypervisor` contract paths now use `../` from the repo
  root.

The reviewers did not edit files directly.
