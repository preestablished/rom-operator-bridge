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

- determinism-hypervisor's dirty working tree (`m9_handoff.rs`,
  `service.rs`, `Cargo.lock`) does not block a push and must not be
  touched. `git pull --rebase` with dirty *unrelated* files is fine here
  because the unpushed commit touches only `.agents/`; if rebase
  complains about local changes anyway, stop and report rather than
  stashing someone's in-flight work.
- If `git pull --rebase` brings in new remote commits, that is fine —
  push the rebased result; the authorization concern is only about
  *local* commits beyond the listed ones.

## Exit Criteria

- All three repos: `git status --short --branch` reports
  `## main...origin/main` with no ahead/behind.
- From a scratch directory, `git clone --depth 1` of guest-sdk (or
  `git fetch && git rev-parse origin/main` in the existing checkout)
  shows `c03e90b` reachable, and
  `cd ~/git/preestablished/reference-workload && cargo run --locked -p
  xtask -- image validate dist/workload-image-0.1.0/workload-image.yaml`
  still passes (the rev check now satisfiable from origin).
