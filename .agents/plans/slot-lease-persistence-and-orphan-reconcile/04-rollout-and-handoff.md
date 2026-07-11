# Rollout And Handoff

## Compatibility And Deployment

- `schema_version: 1` records are new; an empty `leases/` directory is fully
  backward compatible. Unknown future versions fail closed and remain intact.
- Deploy the worker contract version that provides `ErrorDetail` before or
  with the bridge. The bridge must still fail safely if details are absent.
- The first upgraded bridge startup may find no records for legacy leaked
  slots. Persistence cannot discover those historical orphans; inspect
  `ListSlots` during the deployment window and use the documented worker
  restart recovery if necessary before declaring baseline capacity clean.
- For a dangling intent, the complete recovery is: stop the bridge, restart the
  worker, verify `ListSlots` is empty/full-capacity, run the audited bridge tool
  to clear only the selected intent, and restart/resume the bridge. Never claim
  that worker restart alone clears bridge persistence.
- Back up neither lease records nor their tokens into ordinary artifact or log
  pipelines. Treat the validated private root as secret-bearing operational
  state and preserve its ownership/modes across deploys.
- Rollback to an older bridge loses awareness of the new files and can leak a
  newly allocated lease. Prefer forward-fix. If rollback is unavoidable,
  quiesce sessions and verify the worker has full capacity first.

## Documentation Deliverables

Update `docs/operator-runbook.md`, `docs/runbook.md`, and any deployment
runbook whose restart/rollback steps change, with:

- record purpose and private-root location without showing contents;
- numeric reconciliation summary fields;
- fail-closed real-session behavior and retry trigger;
- the complete stopped-bridge, worker-restart, capacity-verification, audited
  intent-clear recovery under the accepted hypervisor deferral;
- live verification procedure and secret-handling constraints;
- destroy-vs-re-adopt decision and its three disqualifiers.

Append, do not overwrite, the request handback
`.agents/requests/slot-lease-persistence-and-orphan-reconcile/04-resolution.md`
with commits, decisions, crash-window outcomes, all nine automated cases,
quality-gate evidence, live evidence, and bead dispositions. Note the paired
hypervisor request's completed deferral decision.

## Completion And Beads

- File the separate session/run sequence persistence bead early and link it to
  `72o` as related, not blocking.
- Keep `72o` open until the live SIGKILL evidence proves capacity recovery
  without worker restart. Close it citing the resolution and verification.
- In the resolution, list sibling request choreography sections whose
  restart-orphan caveat is retired; do not rewrite their historical files.
- If the live window or required external deployment is unavailable, update
  `72o` with the exact remaining verification, file/record the owned-window
  follow-up, and leave it in progress. Commit and push implementation evidence
  without describing it as full acceptance.
- Run `bd show rom-operator-bridge-72o` before claiming it, record the related
  sequence-persistence bead ID in the resolution, and run `bd preflight` (or at
  minimum `bd lint` plus `bd orphans`) before closure.

At session end follow `AGENTS.md`: file remaining work, run gates, update bead
status, commit intentionally, `git pull --rebase`, `bd dolt push`, `git push`,
and verify `git status` reports the branch up to date with origin.
