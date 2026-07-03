# Plan: Phase 3 Follow-Ups — Push Closeout, Evidence Refresh, Negative Tests

Filed 2026-07-03 from the rom-operator-bridge session that verified the
`phase3-m4-first-room-unblock` plan. Written for a coding agent. Three
items, spanning three sibling checkouts under `~/git/preestablished/`.

## The Three Items

| Step | Item | Repo(s) |
|---|---|---|
| `01-push-unpushed-mains.md` | Unblock the guest-sdk.lock rev check: push the three unpushed verification-note commits | guest-sdk, determinism-hypervisor, reference-workload |
| `02-evidence-note-addendum.md` | Fix the stale "runner label needs a decision" wording | reference-workload |
| `03-negative-tests.md` + `04-live-worker-smoke.md` | Close the disclosed test-coverage gaps from the 07-verification note | reference-workload |

## Ground Rules

- **Repo context discipline:** every git operation starts with `pwd` +
  `git remote -v`. You are working across four checkouts; wrong-repo
  commits are the classic failure here.
- **Do not touch in-flight work:** the determinism-hypervisor working
  tree has uncommitted edits (`m9_handoff.rs`, `service.rs`,
  `Cargo.lock`) belonging to a concurrent session. Push only committed
  history; never add, stash, or reset those files.
- **Task tracking:** create beads in the repo where the work lands
  (reference-workload tracker for steps 02–04), per each repo's
  conventions. Step 01 needs no bead — it is three `git push`es with
  verification.
- **The deployed runtime is live:** nothing in this plan requires
  restarting `dh-workerd` or the bridge. Step 04's scratch worker must
  use scratch paths, never `/run/dh/grpc.sock`.
- The full in-VM boot / READY-snapshot regeneration / snapshot-ref
  cutover is **out of scope** — it remains the operator-coordinated
  sequence recorded in reference-workload's
  `.agents/plans/phase3-m4-first-room-unblock/07-verification.md`.

## Context Pointers

- Verification note that produced this plan:
  `~/git/preestablished/reference-workload/.agents/plans/phase3-m4-first-room-unblock/07-verification.md`
- Reviewer findings behind step 03's gap list are summarized there; the
  underlying code locations are given per-gap in `03-…`.
