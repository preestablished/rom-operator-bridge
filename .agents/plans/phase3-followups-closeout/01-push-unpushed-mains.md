# Step 01 — Push The Three Unpushed Verification-Note Commits

## Authorization

Pushing to `main` on these repos normally requires explicit operator
approval. **The operator (Matt) authorized these three specific pushes on
2026-07-03 when commissioning this plan.** The authorization covers
exactly the commits listed below — if `origin/main..main` shows anything
else at execution time, stop and ask; do not push extra commits under
this authorization.

## Why This Matters (Not Just Hygiene)

`reference-workload/image/guest-sdk.lock` pins guest-sdk rev
`c03e90b` and `xtask image build` refuses on mismatch. While that commit
is unpushed, any fresh clone or CI checkout of guest-sdk sits at
`604cd41` and every reference-workload image build fails the rev check.
Pushing guest-sdk is the actual unblock; the other two keep the
cross-repo request/verification trails (which the repos' agents navigate
by) visible to fresh clones.

## The Commits (verified 2026-07-03)

| Repo | Unpushed commit | Content |
|---|---|---|
| guest-sdk | `c03e90b` "Add rom-bridge verification note for Ms4 acceptance" | docs only (`.agents/requests/...`) |
| determinism-hypervisor | `4c44263` "Add rom-bridge deployed-verification note for GetFramebuffer fix" | docs only |
| reference-workload | `0a9726c` "Add rom-bridge verification note for the M4 first-room unblock plan" | docs only |

## Procedure (guest-sdk first — it is the blocker)

For each repo, in order guest-sdk → determinism-hypervisor →
reference-workload:

```sh
cd ~/git/preestablished/<repo>
pwd && git remote -v          # confirm repo
git log --oneline origin/main..main   # MUST show exactly the one commit above
git pull --rebase             # pick up any remote movement
git push
git status --short --branch   # MUST show up to date with origin
```

Notes:

- determinism-hypervisor's dirty working tree does not block a push and
  must not be touched. `git pull --rebase` refuses up front on unstaged
  changes to files the incoming commits touch — if it refuses, stop and
  report rather than stashing someone's in-flight work.
- If `git pull --rebase` brings in new remote commits and rebases
  cleanly, push the rebased result. If the rebase stops with
  **conflicts**, run `git rebase --abort` (restores pre-pull state,
  touches nothing else) and report — conflict resolution is not covered
  by this authorization.
- **Match the authorized commit by subject line and diff content**
  (docs-only, `.agents/` paths), not by SHA — a rebase rewrites hashes.
  The exit criterion is likewise "a commit with that subject reachable
  from `origin/main`", not the literal SHA.

## Exit Criteria

- All three repos: `git status --short --branch` reports
  `## main...origin/main` with no ahead/behind.
- `git fetch && git merge-base --is-ancestor <local main> origin/main`
  (or `git rev-parse origin/main`) in guest-sdk confirms the
  verification-note commit reached origin — no scratch clone needed.
- Caveat on proving "satisfiable from origin": `xtask image build`'s rev
  check reads the **local** sibling checkout, so running it here proves
  nothing new after the push. The push itself plus the fetch check above
  is the exit evidence; a fresh-clone build proof is optional and only
  meaningful from a scratch clone of both repos.
