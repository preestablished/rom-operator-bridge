# Tests, Privacy, and Smoke

## Real Backend Mock Tests

Extend `service/tests/real-backend/main.rs` mock worker with `inject_inputs`.

The mock should record:

- call order;
- lease slot id and token internally;
- `ScheduledEvent.at_frame`;
- `PadSet.port`;
- `PadSet.buttons`;
- returned scheduled count;
- configurable tonic status code for failure tests;
- mock current frame for stale rejection tests.

Add tests for these behaviors:

- real start requesting `input` grants `capabilities.input = true`;
- real start not requesting input keeps it false and input is rejected;
- `RealBackend::inject_input` sends one `InjectInputs` request with
  `AtFrame(current_frame + 1)`, `port = 0`, and the validated pad word;
- worker scheduled count other than `1` returns sanitized unavailable and does
  not write padlog artifacts;
- worker `InvalidArgument` stale maps to scheduler retry and succeeds after a
  refreshed frame base;
- target frame `u32::MAX` is rejected locally and is not sent to the worker;
- worker `FailedPrecondition` maps to sanitized unavailable and does not write
  padlog artifacts;
- artifact persistence failure after worker scheduled count `1` quarantines or
  removes the real session and prevents a later bounded run from consuming an
  unlogged input;
- private padlog text and padlog event rows contain assigned frames and pad
  words, not lease tokens, socket paths, snapshot refs, or worker messages;
- `last_applied_input_frame` updates only after successful worker scheduling and
  private artifact persistence.

## Scheduler Tests

Update `service/tests/input_scheduler/main.rs` for real boundary semantics.

Add tests for:

- synthetic paused input remains queued;
- real paused input with input capability applies immediately;
- real paused input without input capability is rejected, not queued;
- real running input is rejected and is not injected behind an in-flight run;
- stale retry uses the refreshed frame base and does not duplicate frame
  assignment;
- stale retry failure records one dropped input with `frame_stale`.

Use fake backends rather than real worker setup for scheduler-only behavior.

## Websocket/API Flow Tests

Add or extend `service/tests/ws_input/main.rs` or `service/tests/real-backend/main.rs`
to cover browser-facing behavior:

- input sent over websocket while real session is paused returns `input_ack`
  with `status = "applied"`;
- any already-pending real input is flushed before `/api/run/resume` starts the
  bounded run;
- the worker sees `InjectInputs` before `Run` in call order;
- a pending input is not scheduled twice by pre- and post-resume flushing;
- public websocket rejection for stale-after-retry uses the existing sanitized
  `Input rejected.` message.

If full websocket + real mock-worker setup is too large, keep direct real
backend tests for worker call order and a focused scheduler/API test proving
pre-resume flush ordering.

## Private Artifact Assertions

Use existing artifact readers where possible.

Assertions should prove:

- `padlog.txt` parses through `PadLog::parse`;
- every applied frame has reserved bits 12..15 clear;
- event rows include `client_seq`, `source_id`, `assigned_frame`, `pad_word`,
  `status = "applied"`, and a safe message;
- failed or stale-dropped inputs do not create applied padlog rows;
- stale-dropped inputs create private input rejection rows through the scheduler
  rejection sink in both websocket submit and API flush paths.

Do not put private transcripts, worker sockets, lease tokens, or raw worker
status text in public docs or test snapshots.

## Redaction Gate

Run the publish-blocking redaction wrapper with a private forbid file. Include
the actual private test canaries used in real-backend tests, for example:

```bash
ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1 \
ROM_OPERATOR_BRIDGE_FORBID_FILE=<private-forbid-file> \
bash scripts/redaction-gate.sh
```

The documented default worker socket path is a public contract in current docs;
operator-specific endpoints, private roots, lease tokens, snapshot refs, and
worker messages are private forbid material.

## Live Smoke Handoff

Reserve `docs/real-backend-smoke.md` for the follow-on `0wo` handoff. For this
bead, add only sanitized implementation notes needed by the future smoke agent:

- how to start a real session with input capability;
- how to send one press and one release;
- where private padlog and event artifacts are expected under the private root;
- what public response fields are safe to record.

Keep actual host transcripts, screenshots, and raw worker output under the
private run directory, not in the repo.
