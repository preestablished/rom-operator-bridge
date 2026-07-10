# Evidence, Privacy, And Closeout

## 1. Public Record Shape

Use stable, sanitized labels so another reviewer can cross-check the run
without learning private values:

```text
run_label, date_utc, bridge_sha, worker_sha, release_build,
effective_budget, window_seconds, delivered_frames, delivered_fps,
disconnects, frame_gaps, observed_boundaries, expected_boundaries,
boundary_tolerance, boundary_max_ms, boundary_p95_excess_ms,
input_check, determinism_check, rollback_check, redaction_check, result
```

For the public `r77` record, use the approved alias/public-safe hashes and
booleans rather than the raw capture id/path; retain the exact tuple privately:

```text
real_nonblank_frame, capture_completed, index_row_unique,
payload_hashes_match, needs_review_written, clean_stop, redaction_check
```

Keep the main contained record, raised-budget delta addendum, and capture smoke
record distinguishable. Cross-reference them; do not merge their acceptance
criteria.

## 2. Leak Sweeps

Before staging any public file or bead note, use a 0600 forbidden-literals file
under the private root and quiet scans. Never print matches:

```bash
test -d "$PRIVATE_RUN_ROOT"
test -f "$FORBIDDEN_LITERALS_FILE"
set +e
rg -q -F -f "$FORBIDDEN_LITERALS_FILE" service docs .agents
source_status=$?
git grep --cached -q -F -f "$FORBIDDEN_LITERALS_FILE" -- service docs .agents
staged_status=$?
set -e
test "$source_status" -eq 1
test "$staged_status" -eq 1
```

Run `scripts/redaction-gate.sh`/`scripts/quality-gate.sh` with the approved
private forbid file as documented by the deployment runbook. Scan any modified
sibling-repository note under that repository's own gate before committing it.

## 3. Acceptance Audit

Before resolution, verify:

- rider and pointer landed before the main run;
- main record proves at least three 200M boundaries and both stall caps;
- main record proves fixed deployed worker, 60-second/fps/no-drop bar, input,
  determinism, fallback, and redaction;
- `l1w` was closed from that contained-stack record only;
- code default changed from 200M to effectively unbounded, local tests passed,
  the fixed worker was redeployed, and the delta determinism check ran;
- delta addendum exists because the deployed budget changed and records zero or
  more actual boundaries honestly;
- `9bx` closes only after its deployed delta pass;
- `eqb` closed on its own complete contained-run acceptance; the delta merely
  cites it and `l1w` as post-raise confirmation;
- `r77` either closes with all capture/label/index/stop checks and both required
  citations, or names one residual and stays open/deferred;
- `13h` is noted but not incorrectly closed; `0wo`/`opw` remain separate.

Add `.agents/requests/phase3-eqb-amendment-and-capture-smoke/04-resolution.md`
only after all currently executable items are complete. If `r77` remains
operator-gated, record the completed rider/code state in bead notes and leave
the request open instead of writing a misleading final resolution.

## 4. Repository Quality And Session Close

Inspect the dirty tree and preserve unrelated user changes. Run the focused and
full gates from `04-budget-raise-and-delta.md` when code changes. For docs-only
intermediate commits, at minimum run:

```bash
git diff --check
PATH="$HOME/.nvm/versions/node/v22.22.0/bin:$PATH" bash scripts/quality-gate.sh
```

Use `bd update --append-notes` with sanitized references before closing each
bead. File a new bead only for a newly discovered residual not already owned by
`eqb`, `l1w`, `9bx`, `r77`, `13h`, `0wo`, or `opw`.

Finish every repository touched by the implementation according to its own
`AGENTS.md`. For this repository the mandatory sequence is:

```bash
git status --short --branch
git add <intentional-files>
git commit -m "..."
git pull --rebase
bd dolt push
git push
git status
git stash list
git remote prune origin
git status
```

Require the final status to report the branch up to date with its origin.
Inspect stashes, but never clear one that this work did not create/own. Do not
stash, reset, or include unrelated changes to force a clean tree. Repeat the
full close protocol separately in every repository touched; a failed sibling
push means the overall implementation is incomplete.
