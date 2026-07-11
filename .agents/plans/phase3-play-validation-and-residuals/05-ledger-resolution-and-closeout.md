# Ledger Resolution And Closeout

## 1. Resume-Response Cosmetic (`bvq`)

Reproduce the real-backend `POST /api/run/resume` response against the current
mock worker before editing. If it still reports the pre-run frame because the
bounded Run omits capture metadata, update the real adapter from the
authoritative run outcome/frame-counter source already used by status or
frame-current. Do not issue a second Run solely to populate the response and
do not infer an absolute frame from elapsed frames.

Add a real-backend API regression asserting:

- the resume response frame equals the worker's completed boundary;
- subsequent status and frame-current agree with it; and
- missing/malformed worker frame metadata fails safely rather than fabricating
  progress.

If current code already behaves correctly, record the proving test/commit and
make no cosmetic diff. Reference-workload's real ROM/cutover is complete, so
reconcile `bvq` from current evidence: close it if all three original reports
are resolved, or re-scope it only to a concrete remaining bridge defect.

## 2. Required Bead Dispositions

Use `bd update --append-notes` (or the installed non-interactive equivalent)
with SHAs, test names, sanitized evidence, and cross-repo references before
closing or re-scoping.

| Bead | Required truth at closeout |
|---|---|
| `4zn` | lifecycle assertions landed and green |
| `y4g` | seam decision recorded; ordering tests landed and green |
| `k1b` | bridge throttle test plus landed/cited hypervisor combination test, otherwise remains open with blocker |
| `pea` | metrics observable; aggregate decision and numeric revisit trigger recorded |
| `9xo` | close as superseded/resolved with deployed-frame and completed refwork-cutover pointers; do not retain stale P0 wording |
| `bvq` | close if current evidence resolves capability, frame response, and content; otherwise name only the remaining defect |
| `9mk` | close against landed Play stack and regression coverage; do not use it as a second `pea` tail |
| `qh4` | remove/fold duplicate metrics scope; retain only unimplemented zero-copy/UI polish |
| `aaw` | verify current deployment superseded the old `54eb016` sudo blocker, then close with deployed-build evidence or re-scope to an actual remaining operator action |
| `eqb`, `l1w`, `9bx` | follow private-validation closure rules; never close from code-only or synthetic evidence |

For `9xo`, `9mk`, and `aaw`, a historical note is not closure proof. Re-run
`bd show` and cite concrete current evidence: completed refwork cutover plus a
deployed advancing-frame proof for `9xo`; exact landed Play commits and green
regressions for `9mk`; and proof that the running bridge contains or supersedes
`54eb016` for `aaw`. If proof is unavailable, append the sanitized gap and
leave open or re-scope rather than inferring success from a newer-looking SHA.

Do not create duplicate beads. File a new issue only for a newly discovered,
unowned residual with a concrete acceptance condition.

## 3. Resolution Handback

Append
`.agents/requests/phase3-play-validation-and-residuals/04-resolution.md` as the
request specifies, while retaining the dated status file as history. Include:

- implementation and test SHAs;
- focused test names and full quality-gate result;
- sanitized EQB contained/delta/rollback measurement tables or the exact
  operator gate if still pending;
- `VerifyReplay`, input, no-drop, boundary, and stall outcomes;
- `pea` decision and metric field location;
- worker-side `k1b` tracker/commit reference; and
- final disposition of every bead in the table above.

If private gates prevent final request closure, the resolution must clearly
separate completed ungated work from gated residuals. Do not claim the whole
request complete until its acceptance criteria are actually met.

## 4. Quality Gates

Run focused tests while iterating, then from the repository root:

```bash
git diff --check
PATH="$HOME/.nvm/versions/node/v22.22.0/bin:$PATH" \
  bash scripts/quality-gate.sh
```

Node 20+ is required by the current Vitest/Vite stack; the recorded project
memory identifies Node 22.22.0 as available. If a sibling hypervisor change is
made, run that repository's full prescribed gates independently.

## 5. Commit, Beads Sync, And Publication

Preserve unrelated changes. Stage explicit paths, inspect the staged diff, and
commit intentional units. Then follow the active `bd prime` close protocol and
the repository `AGENTS.md`. At minimum:

```bash
git status --short --branch
git add <intentional-files>
git diff --cached --check
git commit -m "..."
git pull --rebase
bd dolt push
git push
git status
git stash list
git remote prune origin
```

Before this sequence, inspect upstream configuration and establish or use the
repository's authorized publication branch/merge workflow without force-push.
The current feature branch may be ephemeral and lack an upstream; that is a
workflow conflict to resolve, not permission to omit the mandatory pull/push.
If no authorized upstream/publication path can be established, report a
blocker and do not claim completion. Work is handed back only after commits and
beads data are published and `git status` reports up to date with origin.
Inspect stashes, but remove only a stash demonstrably created by this work.
Repeat the complete transaction independently in determinism-hypervisor if it
was touched.
