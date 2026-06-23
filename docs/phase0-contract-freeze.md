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
service: Rust host-control service at service/Cargo.toml
ui: TypeScript Vite/Vitest SPA/static web UI at ui/package.json
service bind: 10.0.0.106:7410
backend strategy: synthetic backend first, real hypervisor backend behind the same interface
```

The exact Rust HTTP framework remains a choice for the service scaffold bead.
The service/UI package paths, npm UI toolchain, command contract, runtime routes,
privacy boundary, deployment origin, and backend split are frozen.

Frozen bridge-stack commands:

```sh
cargo fmt --manifest-path service/Cargo.toml -- --check
cargo test --manifest-path service/Cargo.toml --all-targets
npm --prefix ui ci
npm --prefix ui run typecheck
npm --prefix ui test -- --run
npm --prefix ui run build
scripts/quality-gate.sh
```

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
- Exact pad mapping and padlog writer behavior are confirmed in
  `docs/bridge-discovery-note.md` under "Input Contract"; reserved bits 12
  through 15 are errors, not masked.
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
- Bridge-stack service/UI commands are exact and are frozen for the scaffold and
  quality-gate beads.
- Deployment is frozen to `https://rombridge.birb.homes/`,
  `https://rombridge.birb.homes/api/...`, and
  `wss://rombridge.birb.homes/ws/...`, with DNS already pointing at
  `10.0.0.106`.
- Deployment/security deviations are explicit: only DNS exists today; the service
  bind is `10.0.0.106:7410`; exact systemd/K3s paths and restart/rollback
  commands are frozen; deployment files and install material are later
  implementation outputs.

## Accepted Scope Limits

The following are not unresolved Phase 0 decisions; they are accepted scope
limits for downstream beads:

- First implementation may use synthetic backend plus UI/API tests while real
  runtime inputs remain unavailable.
- Real backend availability is not approved by this freeze. Real mode remains
  blocked until the operator supplies a private snapshot or a later bead records
  the exact `CreateVm` ROM startup config. Implementation may proceed only for
  the synthetic backend and real-backend interfaces until that decision is
  recorded.
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
