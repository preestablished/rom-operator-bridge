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
  `Cargo.lock`, `tests/nanokernel/tests/elf_shape.rs` — list may grow)
  belonging to a concurrent session. Push only committed
  history; never add, stash, or reset those files.
- **Push authorization (standing grant for this plan):** the operator
  authorized (a) the three listed catch-up pushes in step 01, and
  (b) pushes of commits *created by executing this plan* on
  reference-workload `main` (docs, tests, CI wiring only — expect
  multiple pushes there; that is normal). Before any push,
  `origin/main..main` must contain only step-01's listed commit and/or
  commits this plan created; anything else → stop and ask.
- **Task tracking:** reference-workload has no CLAUDE.md — its
  conventions live in `bd prime` (run it there). Tracker prefix is
  `refwork-`; existing related work sits under epics `refwork-d7t.*` and
  `refwork-gp9` — parent new beads accordingly. Use **one bead with a
  per-gap checklist** for step 03 and a separate bead for step 04.
  Step 01 needs no bead — it is three `git push`es with verification —
  and no `bd dolt push` (it changes no beads; in determinism-hypervisor
  especially, do NOT dolt-push: a concurrent session owns that state).
  Steps 02–04's closeout in reference-workload includes `bd dolt push`.
- **The deployed runtime is live:** nothing in this plan requires
  restarting `dh-workerd` or the bridge. Step 04's scratch worker must
  use scratch paths, never `/run/dh/grpc.sock`.
- The full in-VM boot / READY-snapshot regeneration / snapshot-ref
  cutover is **out of scope** — it remains the operator-coordinated
  sequence recorded in reference-workload's
  `.agents/plans/phase3-m4-first-room-unblock/07-verification.md`.

## When All Steps Are Done

1. Append a dated addendum to reference-workload's
   `.agents/plans/phase3-m4-first-room-unblock/07-verification.md`: the
   pushes landed (final SHAs), gaps A–D disposition, live-smoke status —
   that note's "unpushed commits" blocker line and gap list are what this
   plan resolves, and it must not go stale the way step 02's target did.
2. Per-repo closeout: `git pull --rebase`; `bd dolt push`
   (reference-workload only); `git push`; `git status --short --branch`
   up to date.
3. Report: per-step outcome, commit SHAs, bead IDs closed, and anything
   you stopped-and-asked on.

## Context Pointers

- Verification note that produced this plan:
  `~/git/preestablished/reference-workload/.agents/plans/phase3-m4-first-room-unblock/07-verification.md`
- Reviewer findings behind step 03's gap list are summarized there; the
  underlying code locations are given per-gap in `03-…`.
