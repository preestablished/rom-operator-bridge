# Private Artifact Writer

## 1. Add Capture Index Types

Extend `service/src/artifacts.rs` with bridge-owned real capture artifact types
that match the reference workload schema:

- `CaptureIndexRow`
- separate feature-byte and framebuffer payload/ref structs
- public-safe capture provenance fields required by the schema

Keep all private paths represented as relative `PrivateArtifactRef` values
inside private files. Public structs should expose only stable public metadata.

Validation rules:

- `schema_version` must equal `ARTIFACT_SCHEMA_VERSION`.
- `run_id`, `capture_id`, and job ids must pass the same path segment checks
  used elsewhere in `PrivateArtifactStore`.
- Payload filenames must be relative and stay below the private root.
- Payload filenames written by the bridge must be generated opaque names or
  sanitized through an allowlist. Do not preserve exporter path basenames if
  they encode private values.
- Raw payload bytes must not be serializable through public response types.
- The bridge-local Rust types may carry private artifact refs only inside
  private artifact modules/backend state. Public API projection types must not
  derive from or serialize `CaptureIndexRow` directly.

## 2. Add Durable Payload Writes

Add methods to `PrivateArtifactStore` for real capture artifacts. Suggested
shape:

```rust
pub struct CapturePayloadArtifact {
    pub artifact_ref: PrivateArtifactRef,
    pub len: u64,
    pub blake3: String,
    pub encoding: String,
    pub uncompressed_len: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub stride: Option<u32>,
    pub pixel_format: Option<String>,
}

pub fn write_capture_payload(
    &self,
    run_id: &str,
    capture_id: &str,
    payload_name: &str,
    bytes: &[u8],
) -> Result<CapturePayloadArtifact, ArtifactError>
```

Use `write_private_file_atomic` for payloads and preserve `0600` file mode.
Do not write payloads to `tmp` without an atomic final rename.
If the schema requires original exporter refs or basenames, store them only in
private manifests with redacted `Debug`/`Display` behavior and never expose them
through public structs, logs, websocket events, or bead notes.

Use the authoritative reference workload payload split:

```text
artifacts/feature-bytes/<opaque-payload-name>
artifacts/framebuffer/<opaque-payload-name>
captures/<capture_id>/capture-manifest.json
captures/<capture_id>/label-draft.json
captures/index.jsonl
captures/recent-captures.json
```

`captures/<capture_id>/capture-manifest.json` is optional bridge-private
provenance. `captures/<capture_id>/label-draft.json` is bridge-local UI state,
not the reference workload Phase 4 trace-label artifact.

Write feature bytes and framebuffer bytes as distinct artifacts:

- feature bytes: raw `feature_bytes` returned by the worker;
- framebuffer: raw `fb_lz4` bytes returned by the worker;
- framebuffer metadata: `encoding = "fb_lz4"`, `width`, `height`, `stride`,
  `pixel_format`, and decompressed `uncompressed_len` derived from `fb_info` and
  lz4 validation.

Compute BLAKE3 over the exact bytes stored in each payload file. Verify
`feature_bytes.len == layout.json.total_len` before writing a completed row.
Reject missing `fb_info`, malformed lz4 payloads, or hash/length mismatches.

## 3. Append And Fsync `captures/index.jsonl`

Add an append method similar to:

```rust
pub fn append_capture_index_row(
    &self,
    row: &CaptureIndexRow,
) -> Result<PrivateArtifactRef, ArtifactError>
```

The append must go through `append_private_file`, which fsyncs and keeps the file
private. The job may be reported as completed only after this method succeeds.

`CaptureIndexRow` must include the authoritative reference workload fields. If
the schema confirms different names, use those names, but do not omit these
classes of data:

- `capture_id`;
- `node_ref` or public-safe source identifier;
- `capture_source`;
- `frame_index` or `frame_counter`;
- `layout_hash`;
- feature payload metadata, not inline feature bytes;
- decoded feature order and decoded values;
- framebuffer metadata and payload metadata, not inline screenshots.

`decoded_order` and `decoded_values` are mandatory for q63 because the
reference workload trace path requires them. Decode from packed
`feature_bytes` in feature-map order. Support only approved numeric scalar
encodings needed by the current feature map, such as unsigned/signed little-endian
integers, bitflags with explicit numeric projection, or BCD fields with an
approved numeric mapping. Fail closed for opaque `bytes`, strings, compound
records, or any feature type whose public numeric value is not specified by the
private feature-map contract.

Idempotency and partial-failure rules:

- Generate deterministic per-job capture ids from public job/session/idempotency
  inputs, not from private paths or payload names.
- Treat payload files as immutable. On retry, either create fresh opaque payload
  names for a fresh job or verify existing bytes by hash/length before reuse.
- Do not append a duplicate `captures/index.jsonl` row on idempotent replay.
- If payload writes succeed but index append fails, leave the job failed or
  retryable without public completion, recent-capture projection, or
  labelability.
- Always clear the active capture lock after terminal success or failure.

## 4. Keep Recent Captures And Label Drafts In Sync

When a real capture completes, keep ownership boundaries explicit:

- backend/private artifact code writes private payloads, optional private
  capture manifests, and `captures/index.jsonl`;
- API `CaptureState` owns sanitized public recent/detail/labelability
  projection;
- `LabelState::apply` owns `captures/<capture-id>/label-draft.json` creation and
  updates after backend completion.

If any write fails, return `BackendUnavailable` or a sanitized capture failure
and do not mark the job completed.

Do not claim reference workload trace-label compatibility from q63. If a YAML
trace-label artifact writer is required, defer it to a separate bead.

## 5. Tests For Artifact Writer

Add focused tests in `service/tests/artifacts/main.rs` or a new
`service/tests/real-capture/main.rs`:

- payload file mode is `0600`;
- parent directories are `0700`;
- `captures/index.jsonl` contains one valid schema row;
- a second append preserves both rows;
- debug/display output of artifact refs stays redacted;
- invalid capture ids and payload names are rejected;
- missing `fb_info`, malformed `fb_lz4`, feature length mismatch, framebuffer
  length mismatch, and hash mismatch keep the job non-completed;
- idempotent replay does not duplicate index rows or rewrite immutable payloads.
