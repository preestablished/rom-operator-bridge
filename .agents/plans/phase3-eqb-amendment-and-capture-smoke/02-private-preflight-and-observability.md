# Private Preflight And Observability

## 1. Claim Existing Work; Do Not Duplicate It

Use `bd show` first. The expected lifecycle is:

- `eqb`: claim for the main and delta validation records;
- `l1w`: leave open until the contained-stack pass;
- `9bx`: claim only when beginning the budget code/deployment change;
- `r77`: remove deferral and claim only after operator authorization is active;
- `13h`: add a sanitized progress note after `r77`; do not close it here.

If another agent owns any bead, coordinate rather than overwriting its state.
Use the installed `bd` version's help for the exact deferred-to-open transition;
append a sanitized note before every close or re-deferral. Never discard prior
notes when changing status.

Record authorization as separate booleans for private data access, host/network
access, bridge/worker deployment or restart, capture and label mutation, and
sanitized sibling-repository publication. A generic private-data window does
not imply every one of these permissions. Skip an ungranted operation. If a
grant expires, begin no new mutation and perform only the already-authorized,
bounded stop/restore cleanup.

## 2. Stage All Sensitive Material Outside Git

Create a mode-0700 operator-private run root outside every checkout. Store in it:

- the approved bridge env/config and endpoint manifest;
- release-worker build/readiness output;
- private curl cookie config (0600), raw API bodies, and WebSocket samples;
- service/worker logs and PIDs;
- capture/index/label verification results;
- a 0600 forbidden-literals file and redaction reports.

Never put a cookie, lease, private path/ref, raw capture id, raw index row,
payload, framebuffer, decoded feature, screenshot, or worker stderr in git,
commit messages, bead notes, or the cross-repo mirror note. Commands should
write raw output privately and print only pass/fail or aggregate numbers.

Follow `deploy/operator-kut-private-validation-reference.md`,
`docs/operator-runbook.md`, and the restore/restart ownership established by
the o73 runbook. Use non-interactive `sudo -n`, bounded curl timeouts, private
log redirection, and PID/process-group cleanup.

## 3. Preflight The Live Stack

Record private evidence and sanitized booleans for:

- the deployed worker is a release build containing `c0337ab` or later;
- worker and bridge readiness pass and there is no competing 1000x store run;
- a real backend session can start with `input`, `preview`, `capture`, and
  `labels` capabilities true;
- the current frame endpoint returns a non-blank real frame;
- the configured private root and capture spec resolve without printing them;
- the static/redaction tooling has an operator-private forbid file;
- rollback artifacts and a clean stop/restart path are available.

If build identity, private authorization, or the non-blank frame check fails,
stop before live work and append only the named sanitized residual to the
relevant bead.

## 4. Add Minimal Aggregate Reopen Telemetry If Needed

The current bridge silently reopens in `service/src/api.rs`; a `/ws/frames`
client alone cannot distinguish a budget boundary from ordinary jitter.
Before the main run, make the seam measurable without exposing private state:

- preserve a sanitized end classification and the aggregate first/last frame
  icount plus terminal `Done.icount`; the current `Ended { faulted: false }`
  collapses budget completion and other clean endings, so do not label every
  clean end as budget exhaustion without carrying the reason;
- time each confirmed clean budget end to successful `play_stream_start`
  transition with `Instant`;
- count clean segment ends, successful reopens, and failed reopen attempts;
- emit structured aggregate fields at a safe log level: monotonic segment
  ordinal, reopen duration milliseconds, and success/failure class only;
- do not log session ids, leases, worker endpoints, refs, frame bytes, or raw
  errors;
- keep the public API/schema unchanged unless the round-1 `pea` work has
  already introduced an approved metrics surface.

Extract the observation/classification logic into a small tested helper and add
an API/play-loop integration test that actually runs `play_stream_loop` to prove
the counters advance once and faulted/cancelled/clean-EOF endings are not
misclassified as budget reopens. The backend-only
`real_streaming_segment_budget_end_supports_seamless_reopen` test does not
exercise API-loop instrumentation; keep it as separate continuity coverage. If
the round-1 executor already supplies equivalent tested telemetry, reuse it.

## 5. Measurement Harness Contract

Use a scripted same-origin, cookie-authenticated `/ws/frames` client whose raw
output stays private. Each binary message is `[u64 frame_counter LE][PNG]`.
The harness must record monotonic timestamps and frame counters, reject an
unauthenticated/wrong-origin handshake during preflight, and calculate:

- observation duration, frames, delivered fps, inter-arrival distribution;
- disconnect count and frame-counter gaps;
- boundary-aligned raw intervals using the aggregate service log;
- measured instructions/frame from preserved aggregate icount values;
- expected versus observed boundary count;
- input-to-visible-effect frames and the round-1 latency/overrun/depth fields.

Do not add a private-data-bearing harness fixture to the repository. A generic
script may be committed only if it contains synthetic defaults, no operator
values, and tests for framing/math; otherwise keep it under the private root.
