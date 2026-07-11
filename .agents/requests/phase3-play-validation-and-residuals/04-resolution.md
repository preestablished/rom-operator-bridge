# Resolution In Progress - 2026-07-11

The ungated code and test residuals are complete. The request remains open for
the operator-private EQB runs and the measurement-dependent bandwidth decision.
No private runtime data was collected or committed during this work.

## Landed Work

Bridge commit `a4f9f98` provides:

- API-level Play lifecycle coverage for stream faults, authentication TTL
  expiry, Stop teardown, retained-frame clearing, and the startup registration
  race;
- an extracted UI live-frame controller with exact `u64`/`bigint` ordering,
  newest-received async decode semantics, cross-run cleanup, bitmap lifetime
  tests, and a mount-level playing-input assertion;
- a pure 250 ms event-throttle boundary policy plus a live streaming websocket
  test proving a conservative approximately 4 Hz update bound and exactly one
  stopped terminal update;
- aggregate `play_loop_summary` and `play_frame_socket_summary` tracing for
  producer frames/bytes, pacer misses/resyncs, per-socket sink sends/bytes,
  inferred counter gaps, retained depth, and subscriber count, documented in
  `docs/play-metrics.md`; and
- the real resume-response cosmetic: when a plain frame-budget Run omits
  `fb_info`, the adapter resolves the authoritative post-run counter from
  `GetFramebuffer` rather than returning cached pre-run progress.

Determinism-hypervisor commit `6e348e5` (closed bead
`determinism-hypervisor-ttqm`) adds the worker-side comparison proving
`Run{FrameBudget(1), CaptureSpec}` returns feature bytes and decoded
framebuffer pixels/metadata consistently with the icount-budget fixture.

## Verification

The following checks passed on 2026-07-11:

- `cargo test --manifest-path service/Cargo.toml --all-targets`;
- UI typecheck, all 15 Vitest files / 89 tests, and Vite production build;
- focused UI run after the final mount assertion: 2 files / 14 tests;
- `bash scripts/redaction-gate.sh` (`redaction-scan: PASS`, 334708 bytes / 6837
  lines scanned);
- deployment helper shell syntax checks; and
- determinism-hypervisor focused
  `service::tests::run_capture_spec_reads_manifest_ranges_and_lz4_framebuffer`.

## Bead Disposition

| Bead | Disposition |
|---|---|
| `4zn` | closed against `a4f9f98` lifecycle regressions |
| `y4g` | closed; decode/paint seam chosen and tested in `a4f9f98` |
| `k1b` | closed against bridge `a4f9f98` and worker `6e348e5` |
| `9mk` | closed against landed feature commits plus `a4f9f98` residual coverage |
| `pea` | metrics half landed; bandwidth/encoding decision remains open pending EQB aggregates |
| `qh4` | retained only for zero-copy fanout and LIVE/buffering UI polish; duplicate metrics scope removed |
| `bvq` | resume cosmetic fixed; deployed content/cutover proof remains unresolved in the published handoff |
| `9xo` | kept open pending deployed bridge cutover/advancing-frame proof; historical root-cause headline is stale |
| `aaw` | kept open: active release observed, but no immutable build manifest proves it includes `54eb016` |
| `eqb`, `l1w`, `9bx` | operator-private contained/delta evidence still required |

## Operator-Gated Residual

The contained 200M EQB run, raised-budget delta, rollback-toggle check,
scheduled-input observation, executable `VerifyReplay` checks, and real-link
fps/latency/bandwidth measurements require explicit authorization for
deployment/restart, host/network access, private workload data, and evidence
handling. Those grants were not supplied in this session, so the live run was
not attempted.

When authorized, execute
`.agents/plans/phase3-eqb-amendment-and-capture-smoke/` and
`02a-eqb-rider-2026-07-07.md` without weakening their boundary-count, stall,
no-drop, privacy, determinism, or rollback criteria. Append only sanitized
aggregates here. Use those measurements to close `pea` with a numeric PNG
bandwidth decision and revisit trigger; do not substitute the historical
172 KB/frame estimate.
