# Control-Plane Integration Options

Date: 2026-06-23
Agent: Codex / Ralph iteration 2

## Scope

This is a Phase 0 discovery note for whether the ROM operator bridge should call
the existing `control-plane` checkout for scorer, capture, snapshot, feature-map,
or service contracts.

## Checkout

```text
path: /home/infra-admin/git/preestablished/control-plane
commit: 261141b3bbaa4371a7dd4147ac6626e0f4918e53
status: clean on main...origin/main
```

The repository README says this checkout hosts the platform-wide `proto/` tree
and publishes the generated `determinism-proto` crate.

## Decision

Do not make the MVP bridge service depend on a live control-plane API.

Use `control-plane` only as a source of shared Rust/protobuf contracts when the
contracts are concrete. The MVP bridge should keep the synthetic backend, real
hypervisor attachment, capture/export path, label draft writer, and verifier
runbook behind bridge-owned interfaces selected by the other Phase 0 discovery
notes.

## Concrete Contracts To Reuse

The useful concrete surface is the `determinism-proto` crate:

```text
crates/determinism-proto
version: 0.2.0
PROTO_VERSION: proto-v0.2.0
features of interest: scorer, inputsynth, common
```

### Scorer

The concrete scorer contract lives in:

```text
proto/determinism/scorer/v1/scorer.proto
crates/determinism-proto/proto/determinism/scorer/v1/scorer.proto
```

`StateScorer` provides:

```text
ScoreBatch
LoadFeatureMap
LoadScoringProgram
CheckpointArchive
RestoreArchive
ReplayCommits
Stats
Health
```

Important bridge-relevant fields:

- `ScoreBatchRequest.states[].feature_bytes`
- `ScoreBatchRequest.states[].fb_lz4`
- `ScoreBatchRequest.states[].fb_raw`
- `ScoreBatchRequest.states[].fb_blob_ref`
- `FramebufferMeta.width`
- `FramebufferMeta.height`
- `FramebufferMeta.format`
- `LoadFeatureMapRequest.inline_yaml`
- `LoadFeatureMapRequest.artifact_ref`
- `LoadFeatureMapRequest.layout`
- `LoadFeatureMapResponse.feature_map_hash`
- `LoadFeatureMapResponse.feature_bytes_len`
- `ScoreResult.goal_hit`
- `ScoreResult.duplicate`
- `ScoreResult.decoded`

Bridge implication: this is a private service-side scoring contract, not a
browser contract. If a later Phase 0 or implementation decision chooses
service-side validation automation, the bridge may call `StateScorer` from the
trusted service process and expose only sanitized status, hashes, role names,
and pass/fail summaries to the UI. Do not send `feature_bytes`, decoded feature
values, raw framebuffer bytes, or scorer error details to the browser.

### InputSynth

The concrete input synthesizer contract lives in:

```text
proto/determinism/inputsynth/v1/synthesizer.proto
crates/determinism-proto/proto/determinism/inputsynth/v1/synthesizer.proto
```

`InputSynthesizer` provides:

```text
ProposeBursts
LoadMacroPack
MineMacros
Health
```

Bridge implication: this is not part of the manual operator MVP. The bridge's
browser input path should continue to use explicit keyboard/gamepad state and
the frozen `console16-12btn-v1` pad layout. Avoid calling `InputSynthesizer`
from the MVP bridge unless a future non-operator-assisted exploration feature is
opened as separate work.

## Contracts To Avoid For MVP

The `determinism.controlplane.v1` service files are placeholders in the current
checkout:

```text
proto/determinism/controlplane/v1/scoring.proto: service ScoringService {}
proto/determinism/controlplane/v1/audit.proto: service AuditService {}
proto/determinism/controlplane/v1/featuremaps.proto: service FeatureMapService {}
proto/determinism/controlplane/v1/artifacts.proto: service ArtifactService {}
proto/determinism/controlplane/v1/runs.proto: service RunService {}
proto/determinism/controlplane/v1/render.proto: service RenderService {}
proto/determinism/controlplane/v1/images.proto: service ImageService {}
proto/determinism/controlplane/v1/registry.proto: service RegistryService {}
proto/determinism/controlplane/v1/tree.proto: service TreeService {}
proto/determinism/controlplane/v1/experiments.proto: service ExperimentService {}
```

Because these services have no RPC methods or message contracts, the bridge
should not depend on them for capture jobs, recent captures, run lifecycle,
framebuffer previews, feature-map loading, registry lookups, or labels.

## Capture And Snapshot Gap

No usable control-plane capture/export service contract was found in this
checkout. The only snapshot-adjacent surfaces are:

```text
proto/determinism/snapstore/v1/snapshot_store.proto
crates/determinism-proto/src/lib.rs handwritten SnapshotRef and PutSnapshotRequest facades
```

These are not enough to mark a real ROM capture job `completed`, write
`captures/index.jsonl`, or expose recent capture metadata. Real capture must be
resolved by `rom-operator-bridge-7a6` (Inspect hypervisor runtime contracts) and
`rom-operator-bridge-z8z` (Inspect reference-workload contracts). If those beads
find no real exporter, bridge capture work must remain synthetic or be deferred;
do not represent a synthetic capture as Phase 4 real acceptance.

## Feature-Map And Verifier Decision

For the first implementation, prefer the reference-workload verifier/runbook
path for final Phase 4 validation. Use control-plane scorer contracts only as an
optional later service-side integration point.

Reasoning:

- `StateScorer.LoadFeatureMap` and `LoadScoringProgram` are concrete.
- `ScoreBatch` can carry the private feature bytes and framebuffer data the
  bridge must keep server-side.
- The control-plane checkout does not include a deployed scorer endpoint,
  credentials, runtime URL, or operator-approved service topology.
- The plan already requires `refwork-verify` commands and private report
  handling.

## Exact Follow-Up If Service-Side Scoring Is Chosen Later

Create a follow-up implementation bead before wiring `StateScorer` into the
bridge:

```text
Title: Integrate optional private StateScorer client
Dependency: after Phase 0 contract freeze and after bridge service scaffold
Acceptance:
- config names scorer endpoint, TLS/auth policy, timeout, and disabled-by-default behavior;
- synthetic test sends fake feature bytes/framebuffer metadata and receives sanitized status only;
- UI never receives decoded feature values, feature bytes, raw framebuffer bytes, or private scorer errors;
- UI never receives `fb_blob_ref`, snapshot refs, artifact refs, component
  breakdowns, novelty details, decoded arrays, scorer warning strings, or raw
  scorer error details unless a later privacy review explicitly approves a
  sanitized subset;
- bridge falls back to runbook-only validation when scorer endpoint is unset.
```

## Agent-Runnable Checks

The control-plane checkout has generated-contract tests for the concrete scorer
and input synthesizer surfaces:

```sh
cargo test -p determinism-proto --features scorer,inputsynth
```

This command validates generated type availability and message round trips. It
does not prove that a live scorer, capture, snapshot, artifact, run, or
feature-map service exists.
