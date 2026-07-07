# Current State (Evidence-Based)

Repo HEAD `fbd38d1` ("Harden streaming Play against worker OOM and fix
two stop-path bugs"), assessed 2026-07-07. Round-1
(`phase3-play-validation-and-residuals/`) is unexecuted (no resolution
file; its target beads open).

## What Moved Since Round-1 Froze Its Terms

- **The OOM incident** (2026-07-07 ~03:29Z): first live streaming Play
  session; `dh-workerd` grew to ~26 GB anon RSS and was OOM-killed
  (snapstore collateral); diagnosis = per-Run accumulation in the
  worker, not the (capacity-2, backpressured) stream channel.
- **`fbd38d1` containment**: `PLAY_STREAM_SEGMENT_ICOUNT_BUDGET`
  ≈200M instructions (~4 epochs), seamless segment reopen (5×50 ms
  retry riding out in-flight input RPCs), plus two stop-path fixes
  (`stop_any()` before DestroyVm; `FailedPrecondition` preserved so
  stop-park polling retries). Cost: ~50 ms hash-link stall per segment
  boundary. New tests model the cancel window and reopen continuity.
- **New beads**: `l1w` (P1 incident record — *closes when eqb passes
  on the fixed stack*), `9bx` (P2 — raise the segment budget when the
  hypervisor green-lights).
- **The hypervisor owes the fix** under their round-2 request
  (`phase4-oom-fix-and-capture-engine-proving/`), which also answers
  `9bx` with a number + build.

## Why Round-1's eqb Terms Are Now Stale

Round-1's item 1 defined the bar pre-incident: sustained ≥8 fps /
≥60 s / no drops, scripted WS client, determinism spot-check. Still
right — but the stack it validates now streams in bounded segments
with a per-boundary hash-link stall (~50 ms per the play-60fps plan's
release-build measurement — note that figure predates `fbd38d1`, whose
commit records no stall number, and the live worker may exceed it;
don't confuse it with the *unrelated* 5×50 ms reopen retry sleep). At
today's 200M budget a 60 s window contains ~60–70 reopens (~200M /
~27.8M instr/frame ≈ 7 frames ≈ 0.85 s per segment), aggregating
~3+ s of stall — a real fps tax the bar must absorb knowingly, not a
"drop" (round-1's drop definition — WS disconnects and frame_counter
gaps — was never at risk of counting stalls; the risk is fps
arithmetic, and the collision between validating at 200M and `9bx`
raising the budget days later with no re-check on record).

## The Capture Smoke: What Exists And What Never Happened

- `q63` landed 2026-06-25 (`bd5f834`, hardened `01966f2`): real
  `trigger_capture` (idempotency, capture-spec resolution),
  `captures/index.jsonl`, truthful capabilities, redaction-gated.
- **No real capture has ever flowed through it** — the deferred
  operator-private smokes (`0wo`/`r77`/`opw`) exist precisely because
  that requires approved private runtime data. The `13h` final
  acceptance chain waits behind them.
- Reference-workload's round-2 corpus request names this repo's export
  path as a **contingency** route for corpus rows ("if the direct
  harness route stalls" — their words); a proven smoke is what makes
  that fallback real rather than theoretical, and it is independently
  the `r77` step the `13h` final-acceptance chain needs next.

## Dependencies

- The rider (item 1) has none — it's a documentation-and-then-run
  change to round-1's own execution.
- The smoke (item 2) gates on: refwork round-1's regenerated snapshot
  + real ROM cut over (a non-blank frame exists), and an
  operator-private window (the same class as `r77`).
- `9bx` follow-through gates on the hypervisor's green light.
