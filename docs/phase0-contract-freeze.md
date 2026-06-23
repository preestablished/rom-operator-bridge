# Phase 0 Contract Freeze

Date: 2026-06-23
Agent: Codex / Ralph iteration 6

## Decision

Phase 0 is accepted. Implementation work may proceed from
`docs/bridge-discovery-note.md` as the frozen contract, with the scope limits and
blockers recorded below.

The frozen implementation location and stack are:

```text
repo: /home/infra-admin/git/preestablished/rom-operator-bridge
service: Rust host-control service
ui: TypeScript single-page/static web UI
backend strategy: synthetic backend first, real hypervisor backend behind the same interface
```

The exact Rust HTTP framework and TypeScript build tooling are still choices for
the service/UI scaffold bead. Those choices must preserve the frozen runtime
routes, privacy boundary, deployment origin, and backend split.

## Gate Result

All Phase 0 gate bullets pass:

- `docs/bridge-discovery-note.md` exists.
- Required checkout paths, commits, and dirty-worktree notes are recorded.
- Implementation location and high-level service/UI stack are frozen.
- Host-control lifecycle is frozen around attaching to existing `dh-workerd`
  through `/run/dh/grpc.sock`, one lease per session, one active MVP operator
  session, `Pause`/bounded `Run` resume, `DestroyVm` cleanup, and faulted-session
  failure handling.
- Browser input flow is frozen around `console16-12btn-v1`, player 1
  `PadSet.port = 0`, `PadSet.buttons = pad_word as u32`, absolute
  `FRAME_COUNTER` bases, `lead_frames = 1`, pre-run `InjectInputs`, stale-input
  retry once, and private dropped-input status.
- Exact pad mapping and padlog writer behavior are confirmed from
  `reference-workload`; reserved bits 12 through 15 are errors, not masked.
- Framebuffer preview is frozen as boundary samples from paused `GetFramebuffer`
  or capture responses; live streaming is deferred because `RunWithFrameCapture`
  is `UNIMPLEMENTED`.
- Real capture mechanism is named as hypervisor `Run` or `TakeSnapshot` with
  `CaptureSpec`, followed by a bridge-owned private artifact/index writer.
- Real capture completion is explicitly blocked until that writer exists and can
  fsync payloads, append/fsync `captures/index.jsonl`, and keep payload refs
  private.
- Label, verifier, dedup, score-plan, trace, bundle, context, checksum,
  private-intake, and redaction command shapes are exact about their checkout
  context.
- Deployment is frozen to `https://rombridge.birb.homes/`,
  `https://rombridge.birb.homes/api/...`, and
  `wss://rombridge.birb.homes/ws/...`, with DNS already pointing at
  `10.0.0.106`.
- Deployment/security deviations are explicit: only DNS exists today; service
  port, unit, route, TLS, auth secret storage, private env path, and rollback
  artifact path are later deployment blockers.

## Accepted Scope Limits

The following are not unresolved Phase 0 decisions; they are accepted scope
limits for downstream beads:

- First implementation may use synthetic backend plus UI/API tests while real
  runtime inputs remain unavailable.
- Real session start needs either an operator-provided private snapshot or a
  later exact `CreateVm` ROM startup config.
- Real capture jobs must not report `completed` until the bridge-owned durable
  artifact/index writer is implemented.
- Full real Phase 4 bundle acceptance requires at least 1,000 real capture rows
  plus the full private bundle shape.
- Optional `StateScorer` automation is deferred and disabled by default unless a
  later bead configures endpoint, auth, timeout, privacy, and fallback behavior.
- `InputSynthesizer` is out of MVP scope.

## Quality Gate

This acceptance is docs-only. The committed-branch checks are:

```sh
git diff --check main...HEAD
git show --check --stat HEAD
```
