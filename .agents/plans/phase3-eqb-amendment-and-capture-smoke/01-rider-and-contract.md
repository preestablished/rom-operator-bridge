# Rider And Contract Amendment

## 1. Make The Round-1 Pointer Unmissable

Edit
`.agents/requests/phase3-play-validation-and-residuals/02-requested-work.md`.
Immediately below item 1's `eqb` heading, add one bracketed line:

```md
> [Amended — read `02a-eqb-rider-2026-07-07.md` first.]
```

Do not rewrite the round-1 request. The rider narrows and supplements item 1;
all non-conflicting round-1 requirements remain in force.

## 2. Add The Rider

Create
`.agents/requests/phase3-play-validation-and-residuals/02a-eqb-rider-2026-07-07.md`
with these normative sections.

### Main contained-stack run

- Run for at least 60 seconds with a stream budget that produces at least three
  completed segment boundaries. Use 200,000,000 instructions for the current
  implementation. If the deployed default was raised before execution, use a
  temporary run-specific clamp no greater than 200M; do not misrepresent the
  raised default as segmented evidence.
- Record, as variables rather than assumptions: bridge SHA/build, worker
  SHA/build, effective stream budget, observation-window seconds, delivered
  frame count/fps, measured instructions per frame, observed boundary count,
  expected boundary count, and stall samples.
- Preserve round-1's drop definition: a drop is an unintended WebSocket
  disconnect/reconnect or a client-visible `frame_counter` gap. A segment
  boundary stall is a pacing perturbation, not a drop.

### Reopen and stall acceptance

- Compute expected completed boundaries from measured values, not the stale
  27.8M estimate:

  ```text
  expected_boundaries = floor(window_seconds * delivered_fps
                              * measured_instructions_per_frame / budget)
  ```

- Preserve the first/last `CapturedFrame.icount` and terminal `Done.icount` as
  private aggregate telemetry. Derive measured instructions/frame from those
  values and express the expected boundary count as a value or confidence
  range. Use the mathematically justified window-edge uncertainty (normally
  plus or minus one completed boundary); expand it only by a predeclared bound
  derived from measured icount/frame variance. A blanket percentage tolerance
  is not acceptable because it could hide many early-ending segments.
- Define one normative series:

  ```text
  boundary_stall_ms = max(0, boundary_interarrival_ms
                             - ordinary_interarrival_baseline_ms)
  ```

  Predeclare the baseline as the median/p50 of nearby non-boundary intervals.
  Retain both raw inter-arrival and derived stall values privately.
- Pass only if every value in the boundary-stall series is at most 250 ms and
  its p95 is at most approximately twice the historical 50 ms baseline
  (100 ms). Report the sample count; do not claim a percentile when no
  boundaries occurred.

### Determinism and closure definitions

- Perform the executable worker `VerifyReplay` spot-check described in
  `03-contained-eqb-run.md` after both the main run and post-raise delta run. A
  budget change moves the chain-continuity seam.
- `l1w` closes when the main `eqb` run passes on the fixed worker while the
  bridge is still using the contained 200M budget. The post-fix, raised-budget
  confirmation belongs in the delta addendum and must not become a second,
  contradictory closure condition for `l1w`.
- The later delta run repeats fps/no-drop, boundary count, applicable stall
  caps, and determinism checks at the raised budget. It need not contain three
  boundaries. If it contains zero boundaries, explicitly record boundary count
  zero and stall statistics as not applicable, rather than inventing a pass.

## 3. Verify The Amendment

Before any private run:

```bash
test -f .agents/requests/phase3-play-validation-and-residuals/02a-eqb-rider-2026-07-07.md
rg -n "Amended.*02a-eqb-rider" \
  .agents/requests/phase3-play-validation-and-residuals/02-requested-work.md
```

Check the rider explicitly contains: 200M/three-boundary rule, measured reopen
cross-check, 250 ms and 100 ms caps, live budget/build variables, both
determinism checks, and the single `l1w` closure definition.
