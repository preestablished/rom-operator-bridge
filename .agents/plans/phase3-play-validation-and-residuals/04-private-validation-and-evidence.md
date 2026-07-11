# Private Validation And Evidence

## Authority

Execute private validation from
`.agents/plans/phase3-eqb-amendment-and-capture-smoke/`, especially its
preflight, contained-run, raised-budget delta, privacy, and closeout files.
That plan and `02a-eqb-rider-2026-07-07.md` are normative where they are more
specific than this integration summary.

Commit `9c36909` checked in the rider and satisfied the stale status file's
missing-amendment prerequisite. Before live work, diff `02a` against
`phase3-eqb-amendment-and-capture-smoke/01-rider-and-contract.md`. If their
normative requirements differ materially, reconcile them before execution;
do not invent a second rider or choose the less strict version silently.

## Entry Gates

Require explicit operator authorization for deployment/restart, host/network
access, private workload data, and evidence handling. Verify:

- the release bridge SHA and deployed worker SHA/build;
- worker includes `c0337ab` or its proven successor;
- no competing snapshot-store 1000x run shares the host;
- a real non-blank reference workload is active;
- sanitized/public and private evidence locations are distinct; and
- rollback commands and post-rollback readiness checks are prepared.

Create the approved private evidence root with mode `0700`; keep cookie jars,
temporary forbidden-literal files, and other credential-bearing files at
`0600`. Run sensitive scans quietly so a match is not echoed into logs. Name
the operator responsible for retention and cleanup before collection begins.

If any gate is absent, stop private execution and note the blocker on the
gated bead. Local tests and metrics implementation remain ungated.

## Required Runs

1. Run the contained 200M stream for at least 60 seconds and at least three
   completed segment boundaries. Apply the rider's expected-boundary math,
   baseline-subtracted stall caps, no-disconnect/no-counter-gap definition,
   scheduled-input evidence, and executable `VerifyReplay` check.
2. Run the raised-budget delta against the deployed fixed worker. Record zero
   boundaries as a valid measured result with stall statistics not applicable;
   repeat fps/no-drop and `VerifyReplay` evidence.
3. Set `ROM_OPERATOR_BRIDGE_PLAY_STREAMING=false`, redeploy/restart only as
   authorized, and perform a bounded fallback Play check. Assert frames advance
   without counter gaps, scheduled input is honored, Pause/Stop clear frames,
   and service/session/slot readiness is restored. Re-enable streaming and
   verify readiness afterward.

The authenticated websocket client must request capabilities the same way as
`ui/src/authSession.ts`. The browser is a human-visible sanity check, not the
measurement authority.

## Evidence Contract

Private evidence retains only the minimum EQB data required: raw inter-arrival
series, first/last/Done icount aggregates, replay tuples, exact builds, and the
small log extracts needed to substantiate the metrics. Do not retain
framebuffer/capture payloads for EQB unless the normative verifier strictly
requires them and the operator explicitly authorizes it. All `r77` capture
material belongs exclusively to the sibling amendment plan.
Committed evidence contains only aggregate measurements, public commit/build
identifiers approved for publication, pass/fail statements, and aliases or
attestations where IDs are forbidden by the redaction gate.

An immutable SHA/build ID is not automatically public-safe. Preserve exact
values privately and publish one only after affirmative approval.

Before committing the resolution:

- recompute fps and Mbps from published aggregate counts;
- verify boundary count/tolerance and stall sample math;
- state the measurement point and whether it matches the historical ~8.5 fps
  comparison point;
- run `scripts/redaction-gate.sh` and inspect the staged diff manually; and
- cite the private evidence location only in the approved non-secret form.

Close `eqb` and `l1w` only after the complete contained pass. Dispose `9bx`
according to the existing amendment plan after the raised-budget code,
deployment, and delta evidence. A failed bar stays open with the bottleneck and
next owner named.
