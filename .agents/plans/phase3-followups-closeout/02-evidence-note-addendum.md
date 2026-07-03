# Step 02 — Evidence-Note Addendum: The Runner Label Is Decided

Repo: `~/git/preestablished/reference-workload`.

## The Staleness

`.agents/plans/guest-sdk-unblock-reference-workload/m4-in-vm-first-room-evidence.md`
contains, in its "Step 06 — CI: PARTIAL" section and again under "Open
Items For The Operator", the statement that the `vm-gates.yaml` runner
label "needs an operator decision" (guest-sdk vs determinism-hypervisor
label conventions). That was true when written, but commit `e08e522`
("Lock in the vm-gates runner label") subsequently set
`runs-on: [self-hosted, intel, kvm]`, annotated as operator-confirmed on
2026-07-02. The evidence note is now behind the repo it documents, and
that file is load-bearing — agents navigate M4 state by it.

## The Fix (Append, Never Rewrite)

Follow the file's own convention: append a short dated section at the
end rather than editing the stale text in place (history stays honest).
Content shape:

```markdown
### 2026-07-03 — Runner Label Addendum

The "runner label needs an operator decision" items above (Step 06
PARTIAL; Open Items #4) are resolved: `e08e522` locked
`vm-gates.yaml` to `runs-on: [self-hosted, intel, kvm]`,
operator-confirmed 2026-07-02. Remaining vm-gates work is unchanged:
the real-worker legs still wait on the coordinated boot/READY step.
Open Items #1 and #3 remain open as written; #2 is superseded by the
later dated sections above.
```

Also strike-or-annotate nothing else — the other Open Items (real-image
prerequisites are now largely closed too) were superseded by *later
sections of the same file*, which already record that; only the runner
label lacks its closing entry.

## Exit Criteria

- Addendum appended, committed to `main` with a message explaining the
  staleness, pushed (push authorized as part of this plan's step-01
  authorization pattern — same repo, docs-only; verify
  `origin/main..main` shows only your commit before pushing).
- A bead is unnecessary for this step if done in the same session as
  step 03 — fold it into that session's closeout; otherwise track per
  repo convention.
