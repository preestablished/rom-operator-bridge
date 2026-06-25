# Private Artifact Writer

## 1. Add Capture Index Types

Extend `service/src/artifacts.rs` with bridge-owned real capture artifact types
that match the reference workload schema:

- `CaptureIndexRow`
- any required payload/ref structs
- any capture provenance fields required by the schema

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

Suggested private layout:

```text
captures/<capture_id>/payloads/<payload-name>
captures/<capture_id>/capture-manifest.json
captures/<capture_id>/label-draft.json
captures/index.jsonl
captures/recent-captures.json
```

If the reference workload requires a different layout, use the authoritative
layout and update tests accordingly.

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
- decoded feature order and decoded values when required by the schema;
- framebuffer metadata and payload metadata, not inline screenshots.

## 4. Keep Recent Captures And Label Drafts In Sync

When a real capture completes:

- write private payloads;
- write capture manifest if required;
- append `captures/index.jsonl`;
- update `captures/recent-captures.json`;
- initialize an empty label draft only as a no-overwrite pre-publication step if
  the API requires the file to exist. `LabelState::apply` remains authoritative
  for transactional draft updates and rollback behavior.

If any write fails, return `BackendUnavailable` or a sanitized capture failure
and do not mark the job completed.

## 5. Tests For Artifact Writer

Add focused tests in `service/tests/artifacts/main.rs` or a new
`service/tests/real-capture/main.rs`:

- payload file mode is `0600`;
- parent directories are `0700`;
- `captures/index.jsonl` contains one valid schema row;
- a second append preserves both rows;
- debug/display output of artifact refs stays redacted;
- invalid capture ids and payload names are rejected.
