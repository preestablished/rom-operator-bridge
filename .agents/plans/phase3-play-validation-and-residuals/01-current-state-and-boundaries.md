# Current State, Decisions, And Boundaries

## 1. Reconcile Before Editing

Run `bd prime`, inspect `git status --short --branch`, and show every scoped
bead: `eqb`, `4zn`, `y4g`, `k1b`, `pea`, `9xo`, `bvq`, `9mk`, `qh4`, `aaw`,
and `l1w`. Also inspect `9bx` and `r77` because the newer EQB plan may have
advanced them. Preserve unrelated worktree changes.

Compare the current tree with the commits named in `00-overview.md`. Do not
reapply the rider, reopen telemetry, raised budget, or stream-terminal fixes if
they remain present. Update the request resolution with the actual final SHAs,
not the historical SHAs quoted by the filing.

Before each work item, run `bd show`, coordinate rather than overwrite any
assigned/in-progress item, and atomically claim an open bead with
`bd update <id> --claim`. Append sanitized notes before every status change.
For sibling-repository work, run that repository's `bd prime` and follow its
claim/update workflow independently.

## 2. Ownership Map

| Work | Owner and disposition |
|---|---|
| Play lifecycle regression | This repo, bead `4zn` |
| UI newest-frame/run-change behavior | This repo, bead `y4g` |
| `/ws/events` throttle assertion | This repo, bridge half of `k1b` |
| `FrameBudget(1) + CaptureSpec` worker assertion | determinism-hypervisor; cite its bead/commit from `k1b` |
| Play metrics and bandwidth decision | This repo, bead `pea` |
| Contained/delta EQB and `l1w` closure | Existing `phase3-eqb-amendment-and-capture-smoke` plan |
| `r77` capture smoke | Existing amendment plan; not absorbed here |
| Snapshot/ROM/cutover | Already completed externally; verify, do not redo |
| Slot lease persistence | Bead `72o`; out of scope unless validation exposes it |

## 3. Implementation Decisions

### UI frame-order seam

Extract the stateful receive/decode decision from `mountOperatorApp` into a
small UI module, for example `ui/src/liveFrame.ts`. Keep canvas lookup and
painting in `app.ts`. The seam should own:

- the current run identity;
- the highest accepted frame counter;
- async decoding via an injected decoder;
- retention/closing of the current bitmap; and
- a callback that receives only the bitmap that is still newest for the same
  run after decode completes.

Keep counters as `bigint` throughout wire parsing and ordering; converting the
binary `u64` to `Number` aliases adjacent values above `2^53`. A run change
must synchronously close and clear the retained bitmap, reset ordering, and
notify `app.ts` to clear the old canvas before any new-run frame arrives.

This is preferable to adding the native `canvas` dependency: frame ordering is
business logic, while jsdom canvas emulation adds platform build cost and does
not make the ordering assertions clearer.

### Metrics surface

Use structured `tracing` events at the Play loop and frame websocket boundary,
not a new public API schema. The request allows an endpoint or logs; logs avoid
expanding the authenticated runtime contract for operational counters. Use
stable event names/fields so a scripted private client can correlate them.

Metrics must distinguish producer progress from websocket delivery. A Tokio
`watch` channel has no queue depth in the conventional sense: its backlog is
semantically `0` or `1` retained newest frame, and it does not expose an exact
skipped-version count. Per-socket counter gaps can indicate coalescing, but
producer counter jumps and watch overwrites are indistinguishable without a
separate producer sequence. Document that limitation rather than reporting a
fictional send queue length.

## 4. Commit Boundaries

Prefer reviewable commits in this order:

1. bridge lifecycle/event tests;
2. UI frame-order seam and tests;
3. Play metrics and their tests/documentation;
4. cheap resume-response cosmetic, if still reproducible;
5. sanitized resolution and ledger-only closeout.

The implementer may combine adjacent commits when the diff is small, but must
not mix private evidence or unrelated user changes into any commit.
