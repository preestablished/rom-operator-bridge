# Subagent Review Summary

Two subagents reviewed this plan after the first draft.

## Architecture and Lifecycle Review

Reviewer: `019ef9e1-8454-79e0-b243-7f61164cf1d6` (`Ptolemy`)

Findings addressed:

- Stop cleanup needed an API-side path because `api.rs` only clears
  browser-facing runtime state after `backend.stop_session(...)` succeeds.
  `04-lifecycle-methods.md` and `06-acceptance-checklist.md` now require the
  stop handler and `cleanup_runtime_session` to clear public runtime state even
  when real `DestroyVm` fails.
- Resume semantics conflicted with the hypervisor contract. The plan now says a
  bounded `Run` returns to a paused boundary and must not compute an absolute
  frame by adding `frames_elapsed`.
- Status synchronization was too weak. The plan now requires `WatchSlots` and
  `ListSlots` resync on lag or missing cache, and it ends the bridge session on
  faulted, missing, `DATA_LOSS`, or lease-invalid slots.
- CreateVm JSON mapping was under-specified.
  `02-private-config-and-start-sources.md` now pins serde field mappings, enum
  values, oneof handling, byte decoding, sorted uniqueness rules, and
  validation constraints.
- The worker command-loop plan now states that backend locks must not be held
  while blocking on worker replies.

## Tests, Privacy, and Acceptance Review

Reviewer: `019ef9e1-85df-70e1-b7cb-df63eed0a23b` (`Dewey`)

Findings addressed:

- CreateVm acceptance now has explicit mock-worker tests for private JSON
  parsing, `CreateVmRequest`, entropy seed handling, returned lease privacy,
  `current_frame = 0`, and bad JSON/path/mode redaction.
- Runtime acceptance now separates socket readiness from real `RestoreSnapshot`
  and real `CreateVm` RPC acceptance.
- Privacy coverage now includes HTTP response bodies, websocket/UI event
  payloads, and public evidence snippets after lifecycle success and failure
  paths.
- The static privacy gate now uses a temporary forbidden-literals file with
  configured private values instead of a broad noisy search for words such as
  `token`.
- Tests now cover default lease non-persistence, private-only persisted
  artifacts if persistence is later required, and destroy-failure cleanup.

No subagent made direct file edits. The changes above were integrated into the
plan by the parent agent.
