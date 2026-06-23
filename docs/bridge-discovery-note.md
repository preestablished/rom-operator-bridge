# Bridge Discovery Note

Date: 2026-06-23
Agent: Codex / Ralph iteration 4

## Scope

This draft freezes the current `reference-workload` contracts needed by the ROM
operator bridge. It is intentionally focused on the `rom-operator-bridge-z8z`
bead. The next Phase 0 aggregation bead should merge this material with:

- `docs/deployment-security-shape.md`
- `docs/control-plane-integration-options.md`
- `docs/hypervisor-runtime-contracts.md`

## Checkouts

Reference workload:

```text
path: /home/infra-admin/git/preestablished/reference-workload
commit: 1292b52e0aeb78ff42ef1a31660035a2f7d2da59
branch: codex/phase4-corpus-guide...origin/codex/phase4-corpus-guide
status: dirty before this inspection
dirty paths:
  M .agents/plans/guest-sdk-unblock-reference-workload/m4-in-vm-first-room-evidence.md
  ?? .agents/plans/phase4-requsts/
```

The dirty files above were pre-existing and were not modified by this discovery
iteration.

## Reference Files Inspected

Padlog and pad layout:

```text
crates/refwork-script/FORMAT.md
crates/refwork-script/src/lib.rs
crates/refwork-emu/src/joypad.rs
xtask/src/image.rs
```

Phase 4 verifier and bundle contracts:

```text
crates/refwork-verify/src/main.rs
crates/refwork-verify/src/phase4_bundle_check.rs
crates/refwork-verify/src/phase4_context_check.rs
crates/refwork-verify/src/phase4_layout.rs
crates/refwork-verify/src/phase4_score_plan.rs
crates/refwork-verify/src/phase4_trace.rs
crates/refwork-verify/src/phase4_checksum_manifest.rs
crates/refwork-verify/src/phase4_private_intake.rs
crates/refwork-verify/src/redaction_scan.rs
crates/refwork-verify/tests/integration.rs
crates/refwork-featuremap/src/main.rs
```

## Padlog Contract

Authoritative parser/writer:

```text
crates/refwork-script/src/lib.rs
```

Important items:

```text
PAD_MASK = 0x0FFF
MAX_FRAMES = 10_000_000
struct PadLog { rom_blake3: Option<[u8; 32]>, frames: Vec<u16> }
PadLog::from_frames(frames)
parse(text)
write(log)
PadLogError::{ReservedBitsSet, ReservedBitsInFrames, TooManyFrames, ...}
```

File format:

```text
header: padlog v1 [rom=<64 lowercase hex chars>]
single frame: HHHH
run length: NxHHHH
canonical writer: lowercase hex, run-length lines for runs > 1,
                  no comments, one trailing newline
```

`HHHH` is exactly four hex digits. Input accepts uppercase or lowercase hex, but
`write()` emits lowercase. Comments and blank lines parse, but the canonical
writer never emits comments.

Reserved-bit rule:

```text
bits 0..11: valid buttons
bits 12..15: reserved, must be zero
```

The parser treats reserved bits as an error. It does not mask them. The bridge
must do the same before writing `.padlog` or mapping a pad word into hypervisor
`PadSet.buttons`.

Bridge writer rule:

```text
PadLog::from_frames(frames) -> write(&padlog) -> parse(&text)
```

Real backend acceptance should require the service-written `.padlog` to round
trip through `refwork-script::parse` and `refwork-script::write` before treating
the artifact as replay evidence.

No current function that converts `.padlog` directly to hypervisor
`ScheduledEvent`/`PadSet` was found in `reference-workload`. The bridge owns the
conversion from validated `u16` pad words to hypervisor `PadSet.buttons`.

## Layout Mapping

Pad layout id:

```text
console16-12btn-v1
layout_version: 1
```

Authoritative button bit mapping:

```text
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
bits 12..15: reserved
```

Confirmed sources:

- `crates/refwork-script/FORMAT.md` names `console16-12btn-v1`, exact mixed-case
  button names, and reserved-bit parse failure behavior.
- `crates/refwork-emu/src/joypad.rs` translates platform-order `pad: u16` into
  SNES-style JOY1 registers via `Joypad::native_word()`, `auto_read()`, and
  `read_serial()`.
- `xtask/src/image.rs` freezes `PAD_LAYOUT_ID` and `PAD_BUTTONS`, and validates
  generated workload image manifests through `validate_pad_layout()`.

`xtask/src/image.rs` also fixes workload image region expectations relevant to
capture metadata:

```text
wram:        size 131072, layout_version 1
framebuffer: size 229376, layout_version 1,
             format xrgb8888-256x224-stride1024
meta:        size 4096, layout_version 1
```

## Capture Index Contract

The current checkout does not provide a bridge-ready capture exporter that writes
`captures/index.jsonl`. It provides the validator contract the bridge output must
satisfy.

Authoritative validator:

```text
crates/refwork-verify/src/phase4_bundle_check.rs
function: check_phase4_bundle()
method: Checker::check_captures()
```

Required bundle file:

```text
captures/index.jsonl
```

Each non-empty JSONL row must include or satisfy:

```text
capture_id: non-empty unique string
node_ref or source_id: string
capture_source: string
frame_index or frame_counter
layout_hash: must match layout.json blake3 when known
feature_bytes.ref: private artifact ref
feature_bytes.len: equals layout.json total_len when known
feature_bytes.blake3: blake3 hash/ref
decoded_order: non-empty array matching feature-map order when known
decoded_values: array same length as decoded_order
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

Real Phase 4 bundle threshold:

```text
MIN_REAL_CAPTURE_COUNT = 1000
```

Bridge implication: a single bridge capture can be durable for the operator UI
once its private payload files and one `captures/index.jsonl` row are written and
fsynced, but it will not pass `phase4-bundle-check` as a real Phase 4 bundle
until the bundle reaches at least 1,000 capture rows and the rest of the bundle
contract is present.

## Dedup Contract

Authoritative validator:

```text
crates/refwork-verify/src/phase4_bundle_check.rs
method: Checker::check_dedup_groups()
required file: dedup-groups.jsonl
```

Each row must include:

```text
group_id: string
expected_relation: same_canonical_state | distinct_stable_state
capture_ids: array with at least 2 known capture ids
changed_features or changed_offset_ranges: non-empty
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

## Label And Trace Contract

Score-plan labels:

```text
first_boss: at least one known capture id
goal_positive: at least one known capture id
goal_negative: at least one known capture id
```

Authoritative writer:

```text
crates/refwork-verify/src/phase4_score_plan.rs
function: write_phase4_score_plan()
K = 32
```

`write_phase4_score_plan()` reads `captures/index.jsonl`, validates label ids
against known capture ids, emits K=32 batches, defaults
`client_batch_prefix = "phase4-k32"`, and defaults `restore_control_batch_ids`
to `checkpoint_after_batch`.

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

Authoritative trace emitter:

```text
crates/refwork-verify/src/phase4_trace.rs
function: emit_phase4_trace()
```

Trace output rows include:

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

## Context Smoke Contract

Authoritative validator:

```text
crates/refwork-verify/src/phase4_context_check.rs
function: check_phase4_context_bundle()
required files:
  manifest.json
  contexts.jsonl
  validation/context-export-report.json
optional file:
  recent-input.padlog
```

The context manifest expects:

```text
kind: phase4-context-smoke
evidence_type: live | synthetic
pad_layout.layout_id: console16-12btn-v1
pad_layout.layout_version: 1
recent_input_available: bool
```

`recent-input.padlog`, when present, is parsed with `refwork_script::parse()` and
must contain at least one frame. Context rows must include `recent_input`; if
`recent_input.available` is true, they must include `padlog_ref` or `words`.
Inline `recent_input.words[]` must be integers no greater than `0x0fff`.

## Verifier Commands

Current exact `refwork-verify` command shapes:

```sh
cargo run --locked -p refwork-verify -- phase4-layout \
  --map <feature-map.yaml> \
  --out <layout.json> \
  --capture-spec-hash <blake3-or-ref> \
  --compiler-or-exporter-commit <commit>
```

```sh
cargo run --locked -p refwork-verify -- phase4-score-plan \
  --captures <captures/index.jsonl> \
  --out <score-plan.json> \
  --first-boss <capture-id> \
  --goal-positive <capture-id> \
  --goal-negative <capture-id>
```

Optional `phase4-score-plan` flags:

```text
--client-batch-prefix <prefix>
--checkpoint-after-batch <client-batch-id>
--restore-control-batch <client-batch-id>   # repeatable
```

```sh
cargo run --locked -p refwork-verify -- trace \
  --captures <captures/index.jsonl> \
  --map <feature-map.yaml> \
  --scoring <scoring-program.yaml> \
  --labels <phase4-trace-labels.yaml> \
  --out <trajectory.jsonl> \
  --report <trace-report.json>
```

```sh
cargo run --locked -p refwork-verify -- phase4-bundle-check \
  --bundle <private-bundle-dir> \
  --report <validation/phase4-bundle-check.json>
```

```sh
cargo run --locked -p refwork-verify -- phase4-checksum-manifest \
  --bundle <private-bundle-dir> \
  --out <validation/checksums.json>
```

```sh
cargo run --locked -p refwork-verify -- phase4-context-check \
  --bundle <private-context-dir> \
  --report <validation/phase4-context-check.json>
```

```sh
cargo run --locked -p refwork-verify -- phase4-private-intake \
  --private-root <private-root> \
  --operator-approved \
  --rom-dir <private-rom-dir>
```

Optional `phase4-private-intake` flags:

```text
--operator-metadata-policy <text>
--operator-label <text>
```

```sh
cargo run --locked -p refwork-verify -- redaction-scan \
  --input <public-note.md> \
  --report <validation/redaction-scan.json> \
  --forbid-file <private-forbid-literals.txt>
```

Optional redaction flags:

```text
--forbid <literal>       # repeatable
--forbid-file <file>     # repeatable
```

Feature-map validation command:

```sh
cargo run --locked -p refwork-featuremap -- validate \
  <feature-map.yaml> \
  --scoring <scoring-program.yaml>
```

Important drift from the initial runbook: current `phase4-layout` uses
`--map`, not `--feature-map`, and requires `--capture-spec-hash`.

## Redaction Contract

Authoritative scanner:

```text
crates/refwork-verify/src/redaction_scan.rs
function: scan_redactions()
```

It reports finding kind, line, and column only. It does not echo matched private
literals or source excerpts.

Built-in finding classes include:

```text
private_payload_field
private_capture_id
long_base64_like_payload
private_file_name
private_retrieval_or_secret_detail
operator_forbidden_literal
```

Forbidden public-note fields include:

```text
decoded_values
raw_wram
wram_bytes
rom_bytes
save_ram
framebuffer.bytes
framebuffer_bytes
raw_capture_bytes
```

Bridge implication: public handoff notes should run `redaction-scan` with
operator-specific forbidden literals in addition to the built-in checks.

## Agent-Runnable Synthetic Checks

Commands run during this discovery:

```sh
cargo test --locked -p refwork-script
cargo test --locked -p refwork-verify phase4 -- --nocapture
cargo test --locked -p xtask pad_layout
```

Observed result:

```text
refwork-script: 12 passed
refwork-verify phase4 filter: 20 passed
xtask pad_layout filter: 4 passed
```

These are agent-runnable and use synthetic fixtures; they do not require private
ROM bytes.

Operator-only checks still require private data:

```sh
cargo run --locked -p refwork-verify -- phase4-private-intake ...
cargo run --locked -p refwork-verify -- phase4-bundle-check ...
```

The first can be exercised synthetically by tests, but real intake and real
bundle acceptance require operator-approved private ROM metadata and private
capture artifacts.

## Gaps For Final Phase 0 Note

The bridge still needs an implementation-side writer for:

```text
captures/index.jsonl
dedup-groups.jsonl
phase4-trace-labels.yaml or equivalent label draft source
private artifact payload files referenced by feature_bytes.ref and framebuffer.ref
```

`reference-workload` validates these artifacts but does not currently provide a
bridge-owned capture exporter that consumes hypervisor `Run`/`TakeSnapshot`
capture bytes and writes the durable bridge capture index.

The final aggregation note should decide the bridge output root and atomic write
policy. The durable condition should be: payload files are written and fsynced,
`captures/index.jsonl` is appended and fsynced, and the containing directories
are fsynced before the UI reports a capture job as `completed`.
