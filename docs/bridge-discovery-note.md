# Bridge Discovery Note

Date: 2026-06-23
Agent: Codex / Ralph iteration 5

## Private Operations Note

This is a private implementation contract. It contains local paths, hostnames,
network addresses, command shapes, and private-workflow decisions. Do not publish
it outside the trusted operator environment without removing or sanitizing those
details.

## Checkouts

- reference-workload:

  ```text
  path: /home/infra-admin/git/preestablished/reference-workload
  commit: 1292b52e0aeb78ff42ef1a31660035a2f7d2da59
  branch: codex/phase4-corpus-guide...origin/codex/phase4-corpus-guide
  status: dirty before inspection and still dirty
  dirty paths:
    M .agents/plans/guest-sdk-unblock-reference-workload/m4-in-vm-first-room-evidence.md
    ?? .agents/plans/phase4-requsts/
  ```

  The dirty paths were pre-existing and were not modified by this discovery work.

- determinism-hypervisor:

  ```text
  path: /home/infra-admin/git/preestablished/determinism-hypervisor
  commit: b9737538f5fc2708d9cb09979df775c0ab388390
  branch: main...origin/main
  status at inspection start: clean
  current dirty path:
    M Cargo.lock
  ```

  The `Cargo.lock` change appeared after agent-run hypervisor tests resolved the
  sibling checkout's dependencies. It was left in place because the Phase 0
  instructions explicitly say not to clean or reset dirty worktrees unless asked.

- control-plane:

  ```text
  path: /home/infra-admin/git/preestablished/control-plane
  commit: 261141b3bbaa4371a7dd4147ac6626e0f4918e53
  branch: main...origin/main
  status: clean
  ```

## Implementation Location

Decision:

Implement both the bridge service and operator UI in this repository,
`/home/infra-admin/git/preestablished/rom-operator-bridge`, as a standalone
project. Phase 1 should add the concrete service/UI stack here, with a
bridge-owned backend interface that supports the synthetic backend first and the
real hypervisor backend later.

Rationale:

- `control-plane` currently provides useful generated protobuf contracts, but no
  live capture, artifact, run, or UI service contract for this bridge.
- `determinism-hypervisor` owns the worker runtime and should remain a worker
  dependency, not the browser-facing app.
- `reference-workload` owns padlog, layout, verification, and bundle contracts,
  but does not provide a bridge-ready web service or capture exporter.
- Keeping service and UI in this repo lets the bridge own auth, same-origin
  routing, browser input timing, private artifact durability, and sanitized UI
  status without coupling unrelated repos to the operator workflow.

The MVP bridge must not depend on a live control-plane API. Optional private
service-side scoring through `determinism.scorer.v1.StateScorer` may be added by
separate work after the bridge service scaffold exists, and should be disabled by
default until its endpoint, auth, timeout, privacy, and fallback behavior are
explicitly configured.

## Host-Control API

Launch or attach:

The bridge attaches to an existing `dh-workerd` worker. It does not launch or
supervise the worker in the MVP. The default worker endpoint is the Unix domain
socket:

```text
/run/dh/grpc.sock
```

Recommended operator worker command shape:

```sh
dh-workerd serve \
  --tcp 127.0.0.1:7400 \
  --http 127.0.0.1:7401 \
  --uds /run/dh/grpc.sock
```

The worker defaults include TCP `0.0.0.0:7400`, HTTP `0.0.0.0:7401`, and UDS
`/run/dh/grpc.sock`, and TCP remains served even when UDS is enabled. For this
private bridge, TCP and HTTP must be rebound to loopback or firewalled so the
browser-facing bridge is the only LAN-exposed control surface.

Lease/slot ownership:

The bridge service owns one worker lease per bridge session. The worker allocates
the slot and returns:

```text
Lease.slot_id: uint64
Lease.token: 16 bytes
```

MVP concurrency is one active operator session. The service records `slot_id`,
lease token, current absolute frame counter, cumulative icount, and last preview
frame counter. The browser never receives the token and never talks directly to
`dh-workerd`.

Session start:

The real backend starts a session with `RestoreSnapshot` when an operator
configured private snapshot is available. `CreateVm` is the fallback RPC only
after a later implementation bead identifies the exact VM/ROM startup config.
Until one of those private runtime inputs is configured, real session start is
blocked and the bridge scope is synthetic backend plus UI.

Pause/resume semantics:

- Pause uses `HypervisorWorker.Pause`.
- Resume is the next bounded `HypervisorWorker.Run` call.
- `Quiesce` is not available; it is `UNIMPLEMENTED`.
- `GetFramebuffer`, `ReadGuestMemory`, and `StreamGuestEvents` require the slot
  to be paused.
- `RunWithFrameCapture` is not available; it is `UNIMPLEMENTED`.

Crash cleanup:

- Normal stop always calls `DestroyVm`.
- A lease validation failure, faulted slot, `DATA_LOSS`, or runtime fault marks
  the bridge session failed and stops accepting browser input for that session.
- Failed `DestroyVm` is an operator-visible cleanup failure, not a successful
  stop.
- The service should subscribe to `WatchSlots` for operator-visible status and
  resync with `ListSlots` when the watch reports lag.

## Input Contract

Reference padlog parser:

The authoritative parser and writer live in:

```text
/home/infra-admin/git/preestablished/reference-workload/crates/refwork-script/src/lib.rs
```

Important API and constants:

```text
PAD_MASK = 0x0FFF
MAX_FRAMES = 10_000_000
PadLog { rom_blake3: Option<[u8; 32]>, frames: Vec<u16> }
PadLog::from_frames(frames)
parse(text)
write(log)
```

Padlog format:

```text
header: padlog v1 [rom=<64 lowercase hex chars>]
single frame: HHHH
run length: NxHHHH
canonical writer: lowercase hex, run-length rows for runs > 1, one trailing newline
```

Reserved bits 12 through 15 are parse errors. The parser does not mask them, and
the bridge must not mask them either.

Pad bit mapping:

```text
layout_id: console16-12btn-v1
layout_version: 1

bit 0: A
bit 1: B
bit 2: X
bit 3: Y
bit 4: L
bit 5: R
bit 6: Up
bit 7: Down
bit 8: Left
bit 9: Right
bit 10: Start
bit 11: Select
bits 12..15: reserved, must be zero
```

Confirmed sources:

```text
crates/refwork-script/FORMAT.md
crates/refwork-script/src/lib.rs
crates/refwork-emu/src/joypad.rs
xtask/src/image.rs
```

Hypervisor input API:

The bridge maps one validated browser pad word to
`determinism.hypervisor.v1.HypervisorWorker.InjectInputs`.

```text
ScheduledEvent.at_frame = target_frame
ScheduledEvent.pad_set.port = 0
ScheduledEvent.pad_set.buttons = pad_word as u32
```

There is no current `reference-workload` helper that converts `.padlog` directly
to hypervisor `ScheduledEvent` or `PadSet`; the bridge owns that conversion.

Port:

Player 1 uses `PadSet.port = 0`.

Frame base:

Frame scheduling uses absolute pv-pad `FRAME_COUNTER` values, never
segment-relative values. Authoritative frame base sources are:

```text
RestoreSnapshotResponse.frame_counter
TakeSnapshotResponse.frame_counter
GetFramebufferResponse.frame_counter
RunResponse.fb_info.frame_counter when boundary framebuffer capture is requested
```

Do not derive the next absolute frame counter from
`RunResponse.frames_elapsed`. It reports elapsed frame marks for that run, not
the final absolute `FRAME_COUNTER`.

Lead-frame policy:

MVP default:

```text
lead_frames = 1
target_frame = last_known_frame_counter + max(lead_frames, 1)
```

The bridge injects input before the next bounded `Run`. `InjectInputs` is not a
mid-run live control path because `Run` clones pending inputs at call start.

Late-input policy:

If `InjectInputs` rejects a stale frame, the bridge refreshes the session frame
counter from the latest paused or boundary state and retries once with a future
frame. If the retry is still stale, it records a private operator status such as
`input dropped: stale frame`. The browser event log should retain:

```text
browser_event_id
source
assigned_frame
pad_word
status
```

The UI must not imply that a dropped or late input landed.

Parser round-trip test:

Padlog output is accepted only after:

```text
PadLog::from_frames(frames) -> write(&padlog) -> parse(&text)
```

Agent-runnable command covering the parser/writer contract:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo test --locked -p refwork-script)
```

## Framebuffer Contract

Source:

The bridge uses boundary samples, not live streaming:

- `GetFramebuffer` for paused slots.
- `Run` with `CaptureSpec.framebuffer = true` for a run boundary sample.
- `TakeSnapshot` with `CaptureSpec.framebuffer = true` for a paused boundary
  sample.

`RunWithFrameCapture` is `UNIMPLEMENTED`, so the MVP must not depend on it.

Format:

Reference workload image expectations:

```text
framebuffer size: 229376
layout_version: 1
format: xrgb8888-256x224-stride1024
width: 256
height: 224
stride: 1024
```

Hypervisor response shapes:

- `GetFramebufferResponse` returns raw `pixels`, `width`, `height`, `stride`,
  `format`, `frame_counter`, and `icount`.
- `Run` and `TakeSnapshot` capture responses can return `fb_lz4` plus
  `FbInfo.frame_counter`.

Preview conversion:

The server validates the format and dimensions, decompresses LZ4 when needed,
and serves a no-store private preview representation to the browser. A PNG or
blob URL is acceptable, but raw private artifact refs, private paths, raw
feature bytes, and raw framebuffer storage refs must not be exposed to the UI.

Stale threshold:

The bridge records `session.current_frame_counter` and
`session.preview_frame_counter`. A preview is stale whenever:

```text
preview_frame_counter < session.current_frame_counter
```

Additional stale states:

- after any `Run` that may advance frames but does not return
  `fb_info.frame_counter`;
- while a run is in progress unless the displayed image came from the same
  reported boundary;
- when `GetFramebuffer` returns `FAILED_PRECONDITION` because the slot is not
  paused or no framebuffer region is published.

## Capture Contract

Mechanism:

Real bridge capture uses `Run` or `TakeSnapshot` with `CaptureSpec`, then a
bridge-owned private artifact writer. The hypervisor can return capture bytes,
but it does not write the reference-workload bundle artifacts or
`captures/index.jsonl`. The `reference-workload` checkout validates these files;
it does not currently provide a bridge-ready exporter.

Request schema:

The capture request uses:

```text
CaptureSpec.ranges[] from the selected layout and detchannel-published regions
CaptureSpec.framebuffer = true
```

Workload image region expectations from `xtask/src/image.rs`:

```text
wram:        size 131072, layout_version 1
framebuffer: size 229376, layout_version 1, xrgb8888-256x224-stride1024
meta:        size 4096, layout_version 1
```

Durability condition:

A real capture job may be marked `completed` only after all of these have
succeeded:

1. Private payload files are written.
2. Payload files are fsynced.
3. Containing directories for payload files are fsynced.
4. One `captures/index.jsonl` row is appended.
5. The index file is fsynced.
6. The containing directory for the index is fsynced after first creation or
   rotation.

Index row fields:

Each non-empty capture row must include or satisfy:

```text
capture_id: non-empty unique string
node_ref or source_id: string
capture_source: string
frame_index or frame_counter
layout_hash: required blake3 hash; equals layout.json blake3 when layout.json is present
feature_bytes.ref: private artifact ref
feature_bytes.len: equals layout.json total_len when known
feature_bytes.blake3: blake3 hash/ref
decoded_order: non-empty array matching feature-map order when known
decoded_values: array with the same length as decoded_order
framebuffer.ref: private artifact ref
framebuffer.blake3: blake3 hash/ref
framebuffer.encoding
framebuffer.pixel_format
framebuffer.width
framebuffer.height
framebuffer.stride
framebuffer.uncompressed_len
```

Forbidden inline payload fields include:

```text
feature_bytes.bytes
framebuffer.bytes
raw_wram
wram_bytes
framebuffer_bytes
screenshot
save_ram
rom_bytes
raw_capture_bytes
```

Fallback if unavailable:

If the bridge-owned writer, private capture root, feature map, scoring program,
layout, or real worker session is unavailable, the implementation scope is
synthetic/demo capture only. Synthetic capture must be labeled synthetic in the
API and UI and must not count as Phase 4 real acceptance.

Real Phase 4 bundle acceptance is also blocked until the bundle has at least
1,000 capture rows and the rest of the bundle contract exists.

## Label And Verifier Contract

Label draft path:

The bridge-owned private label draft path is:

```text
<private-bundle-dir>/labels/phase4-trace-labels.yaml
```

That file is not a browser artifact. The UI may edit sanitized label state, but
the service writes the private YAML. The verifier invocation passes this path as
`--labels`.

Trace label input:

```text
schema_version: 1
kind: phase4-trace-labels
labels:
  - capture_id: <capture-id>
    expected_highest_stage: <stage>
    prune: <bool>
    goal: <bool>
    first_boss_coverage: <bool>
    active_stages: [<stage>, ...]   # optional
```

`emit_phase4_trace()` requires one label entry for every capture row it reads.
Required label fields must match the state computed from decoded capture values
and the scoring program. `active_stages` is optional, but when present it must
match exactly.

Score-plan transformation:

`write_phase4_score_plan()` reads `captures/index.jsonl`, validates label ids
against known capture ids, requires at least 32 captures, emits K=32 batches,
defaults `client_batch_prefix` to `phase4-k32`, and defaults
`restore_control_batch_ids` to `checkpoint_after_batch`.

Score-plan labels:

```text
first_boss: at least one known capture id
goal_positive: at least one known capture id
goal_negative: at least one known capture id
```

Trace transformation:

`emit_phase4_trace()` reads:

```text
captures/index.jsonl
feature-map.yaml
scoring-program.yaml
<private-bundle-dir>/labels/phase4-trace-labels.yaml
```

It writes trajectory rows containing:

```text
schema_version
frame_index
capture_id
decoded_order
decoded_values
active_stages
expected_highest_stage
prune
goal
first_boss_coverage
```

Privacy boundary:

`captures/index.jsonl`, `trajectory/*.jsonl`, `score-plan.json`,
`dedup-groups.jsonl`, validation reports, checksum manifests, label drafts,
capture ids, `decoded_values`, private artifact refs, and raw verifier or scorer
error details are operator-private server-side artifacts. Browser APIs may return
only sanitized aggregate status, counts, pass/fail booleans, and
operator-approved labels. Public handoff text must run `redaction-scan` with the
operator forbidden-literal file before publication.

Dedup artifact:

The required artifact is:

```text
<private-bundle-dir>/dedup-groups.jsonl
```

Each row must include:

```text
group_id
expected_relation: same_canonical_state | distinct_stable_state
capture_ids: at least 2 known capture ids
changed_features or changed_offset_ranges
```

Bundle-level requirements:

```text
at least one same_canonical_state group
at least one distinct_stable_state group
```

Forbidden dedup fields:

```text
canonical_hash
state_hash
scorer_hash
archive_hash
precomputed_hash
```

For `same_canonical_state`, named `changed_features` must be volatile features.
For `distinct_stable_state`, at least one named changed feature must be stable.

Validation commands:

Padlog parser/writer:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo test --locked -p refwork-script)
```

Feature-map validation:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-featuremap -- validate \
  <feature-map.yaml> \
  --scoring <scoring-program.yaml>)
```

Layout writer:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-layout \
  --map <feature-map.yaml> \
  --out <layout.json> \
  --capture-spec-hash <blake3-or-ref> \
  --compiler-or-exporter-commit <commit>)
```

Score plan:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-score-plan \
  --captures <captures/index.jsonl> \
  --out <score-plan.json> \
  --first-boss <capture-id> \
  --goal-positive <capture-id> \
  --goal-negative <capture-id>)
```

Trace:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- trace \
  --captures <captures/index.jsonl> \
  --map <feature-map.yaml> \
  --scoring <scoring-program.yaml> \
  --labels <private-bundle-dir>/labels/phase4-trace-labels.yaml \
  --out <trajectory.jsonl> \
  --report <trace-report.json>)
```

Bundle check:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-bundle-check \
  --bundle <private-bundle-dir> \
  --report <validation/phase4-bundle-check.json>)
```

Checksum manifest:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-checksum-manifest \
  --bundle <private-bundle-dir> \
  --out <validation/checksums.json>)
```

Context smoke:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-context-check \
  --bundle <private-context-dir> \
  --report <validation/phase4-context-check.json>)
```

Private intake:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-private-intake \
  --private-root <private-root> \
  --operator-approved \
  --rom-dir <private-rom-dir>)
```

Redaction scan:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- redaction-scan \
  --input <public-note.md> \
  --report <validation/redaction-scan.json> \
  --forbid-file <private-forbid-literals.txt>)
```

The redaction scanner reports finding kind, line, and column only; it must not
echo matched private literals or source excerpts. Add operator-specific
forbidden literals with repeatable `--forbid` and `--forbid-file` arguments
before producing any public handoff.

Agent-runnable synthetic checks already run during Phase 0:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo test --locked -p refwork-script)
(cd /home/infra-admin/git/preestablished/reference-workload && cargo test --locked -p refwork-verify phase4 -- --nocapture)
(cd /home/infra-admin/git/preestablished/reference-workload && cargo test --locked -p xtask pad_layout)
(cd /home/infra-admin/git/preestablished/determinism-hypervisor && cargo test -p dh-worker inject_mapper)
(cd /home/infra-admin/git/preestablished/determinism-hypervisor && cargo test -p dh-devices frame_counter_write_logs_frame_mark)
(cd /home/infra-admin/git/preestablished/control-plane && cargo test -p determinism-proto --features scorer,inputsynth)
```

Observed results:

```text
refwork-script: 12 passed
refwork-verify phase4 filter: 20 passed
xtask pad_layout filter: 4 passed
dh-worker inject_mapper filter: 4 passed
dh-devices frame_counter_write_logs_frame_mark filter: 1 passed
determinism-proto scorer,inputsynth: 19 passed
```

Operator/private-data commands:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-private-intake ...)
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-bundle-check ...)
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-context-check ...)
```

Those can be tested synthetically through existing tests, but real acceptance
requires operator-approved private ROM metadata, private capture artifacts, and
private labels.

## Deployment Contract

Static UI path:

```text
https://rombridge.birb.homes/
```

Runtime API path:

```text
https://rombridge.birb.homes/api/...
```

WebSocket path:

```text
wss://rombridge.birb.homes/ws/...
```

Same-origin proxying under `https://birb.homes/rom-bridge/` is not the Phase 0
target. The dedicated same-network HTTPS origin above is the chosen target.

Service bind address:

```text
10.0.0.106:<bridge-port>
```

The operator has added DNS for `rombridge.birb.homes` pointing to `10.0.0.106`,
and local resolution has confirmed:

```text
10.0.0.106      rombridge.birb.homes
```

Only DNS exists today. No bridge service, TLS route, proxy route, service unit,
or exact `<bridge-port>` exists yet.

Do not bind the bridge service to `0.0.0.0`. Do not use a
`127.0.0.1:<bridge-port>` bind for the dedicated-hostname deployment unless the
deployment bead adds a host-local reverse proxy that can reach loopback. The edge
route must enforce Host/SNI for `rombridge.birb.homes` only, so the bridge is not
served under another host that resolves to `10.0.0.106`.

Origin allowlist:

Runtime HTTP and WebSocket requests allow only:

```text
https://rombridge.birb.homes
```

Reject absent, `null`, and wrong browser `Origin` values unless a future local
CLI/admin endpoint documents a non-browser exception. Do not use wildcard CORS
with credentials. Add `Vary: Origin` if responses vary by request origin.

No-cache and proxy notes:

Every runtime API route, private preview route, private status route, and
WebSocket handshake path must emit or enforce:

```text
Cache-Control: no-store
Pragma: no-cache
X-Content-Type-Options: nosniff
```

`index.html` and runtime config are also `Cache-Control: no-store`. Hashed
static assets may be cacheable only after redaction scanning and only if they
contain no runtime state, private paths, screenshots, capture ids, credentials,
or source maps with private local paths.

Minimum UI route headers:

```text
Content-Security-Policy: default-src 'self'; connect-src 'self' wss://rombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'
Referrer-Policy: no-referrer
X-Frame-Options: DENY
X-Content-Type-Options: nosniff
```

Auth shape:

- Use `HttpOnly; Secure; SameSite=Strict` cookie auth scoped to `/`.
- Authenticate HTTP and WebSocket handshakes.
- Store operator credentials outside source control.
- Default session TTL is 4 hours.
- MVP concurrency is one active operator session.
- Do not put credentials in URLs.
- Rate-limit failed auth attempts.
- Log auth failures only to private service logs.
- Return sanitized auth errors without credentials, private paths, stack traces,
  host-control details, or artifact identifiers.

Restart and rollback command shapes:

```sh
sudo systemctl restart rom-operator-bridge.service
sudo systemctl stop rom-operator-bridge.service
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl apply -f <rombridge-ingress.yaml>
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl delete -f <rombridge-ingress.yaml>
```

Future deployment checks once a service and route exist:

```sh
getent hosts rombridge.birb.homes
curl -I --resolve rombridge.birb.homes:443:10.0.0.106 https://rombridge.birb.homes/
curl -i -H 'Origin: https://example.invalid' https://rombridge.birb.homes/api/session
curl -i https://rombridge.birb.homes/api/session
curl -I https://rombridge.birb.homes/api/session
```

Expected deployment check results:

- hostname resolves to `10.0.0.106`;
- TLS is served for `rombridge.birb.homes`;
- unrelated origins are rejected;
- unauthenticated API requests are rejected without private details;
- runtime responses include `Cache-Control: no-store`.

## Gaps

Blockers:

- The bridge service and UI do not exist yet, so there is no exact
  bridge-package test command beyond docs checks in this repo. Phase 1 service
  scaffold must define the stack-specific commands before implementation beads
  can claim runtime tests.
- The exact bridge service port, systemd unit contents, artifact path, private
  env file path, K3s Service/Ingress manifest, and rollback artifact path do not
  exist yet.
- Real session start needs an operator-provided private snapshot or a later
  exact `CreateVm` ROM startup config.
- No existing exporter writes private payload files plus `captures/index.jsonl`
  from hypervisor capture bytes. The bridge must implement that writer before
  real capture can be marked `completed`.
- Full real Phase 4 bundle acceptance requires at least 1,000 real capture rows
  plus `manifest.json`, workload image YAML/ref, `feature-map.yaml`,
  `scoring-program.yaml`, `layout.json`, `dedup-groups.jsonl`,
  `score-plan.json`, `validation/`, and `trajectory/`.
- Private data gates require operator-approved private ROM metadata, private
  artifact roots, private label files, and private forbidden-literal files.

Deferred work:

- Optional `StateScorer` integration is deferred until the service scaffold
  exists and the operator chooses a private scorer endpoint.
- `InputSynthesizer` is not part of the manual operator MVP.
- Live framebuffer streaming is deferred because `RunWithFrameCapture` is
  `UNIMPLEMENTED`.
- Public handoff publishing is deferred until redaction scanning is wired for
  operator-specific forbidden literals.

Current repo quality gate for this committed docs-only Phase 0 note:

```sh
git diff --check main...HEAD
git show --check --stat HEAD
```
