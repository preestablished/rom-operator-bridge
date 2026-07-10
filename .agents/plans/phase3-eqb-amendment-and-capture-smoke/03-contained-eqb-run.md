# Contained 200M EQB Run

## 1. Freeze The Main-Run Build Pair

Prepare this build as an intermediate commit: rider plus tested telemetry, with
`PLAY_STREAM_SEGMENT_ICOUNT_BUDGET` still `200_000_000`. The raised-budget
successor may be coded and tested immediately afterward, but preserve the
intermediate commit so it can be built/deployed first in the private window.
Deploy it against a release worker containing `c0337ab` or later. Privately
record both immutable build identities and confirm the effective budget is
200M.

The fixed worker plus contained bridge is the `l1w` closure stack. A pre-fix
worker is not acceptable, and an already-raised bridge must be temporarily
clamped per the rider before proceeding.

## 2. Execute Round-1 EQB Plus Rider Requirements

Use the authenticated scripted client as the measurement source and the browser
only as a human-visible sanity pass:

1. Start the real session and confirm the real non-blank frame/capabilities.
2. Start Play and measure a continuous window of at least 60 seconds.
3. Inject the scheduled pad action and record input-to-visible-effect frames.
4. Stop or pause cleanly only after the measurement window.
5. Perform the executable `VerifyReplay` flow below against the played segment.
6. Exercise `ROM_OPERATOR_BRIDGE_PLAY_STREAMING=false` once, as required by
   round 1, in a separate short fallback check; do not mix it into segmented
   metrics.

## 3. Main-Run Pass Bar

The sanitized result passes only when all of these are true:

- at least 60 seconds and at least 8 delivered fps;
- no unintended WebSocket reconnect and no `frame_counter` gap;
- at least three successful segment reopens;
- observed boundary count agrees with the rider formula/tolerance;
- every derived boundary stall is at most 250 ms and p95 boundary stall is at
  most approximately 100 ms;
- scheduled input is honored and the round-1 telemetry fields are present;
- verifier/chain-replay spot-check is green;
- rollback-toggle path works; session and worker remain healthy after stop;
- redaction/forbidden-literal sweeps are green.

If the expected boundary count is below three, extend the observation window;
do not lower the floor. If fps or stall misses, retain private diagnostics and
publish only the aggregate miss and named bottleneck.

## 4. Executable Determinism Spot-Check

`service/src/verifier.rs` produces Phase-4 score-plan inputs; it is not the
worker replay verifier. For both the main and delta runs, follow the checked-in
worker RPC shape in
`../determinism-hypervisor/docs/ops/m6-grpcurl-metrics-smoke.md`:

1. retain the base snapshot reference for the played segment privately;
2. at the terminal boundary, use the approved bridge capture/snapshot path with
   `seal_input_log=true` and retain the returned input-log id and terminal state
   hash privately;
3. invoke worker `VerifyReplay` with `{base, inputLogId,
   bisectOnDivergence:false}` over the private endpoint;
4. consume the stream to `Done`, require zero `Divergence`, and require
   `Done.end_state_hash` to equal the sealed terminal state hash;
5. store raw refs/hashes privately and publish only build ids, zero-divergence,
   Done-seen, end-hash-match, and overall pass booleans.

If the live bridge path cannot expose the base/log/terminal tuple needed for
this check, stop and add the smallest private operator harness or aggregate
handoff required; do not substitute `service/src/verifier.rs` or a synthetic
test for replaying the played segment.

## 5. Record And Disposition

Write a sanitized `eqb` main-run record in the round-1 resolution area (or the
single resolution file chosen by its executor) containing only aggregate
metrics, public-safe build SHAs, effective budget, pass/fail booleans, and the
private evidence label—not private paths.

Append the record reference and result to `eqb`. On a complete pass, close
`eqb` to its own B1/B2/determinism acceptance and close `l1w` with the pinned
reason: fixed worker + 200M contained-stack `eqb` passed. The later `9bx` delta
may cite both closed beads as confirmation; it does not add a new closure
condition. A main-run failure keeps both open with a named residual.
