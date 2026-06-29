# Tests and Smoke Plan

## Conversion Tests

Add focused tests for `service/src/framebuffer.rs`.

Cover:

1. `synthetic_frame_png(frame)` output stays unchanged for at least one known
   frame hash or byte equality fixture.
2. XRGB8888 conversion channel order is frozen by a colored fixture. Use the
   determinism-hypervisor nanokernel framebuffer fixture or an equivalent
   documented byte-order fixture so the test proves which byte is X, R, G, and
   B.
3. XRGB8888 conversion ignores row padding.
4. Zero width or height is rejected.
5. `stride < width * 4` is rejected.
6. `pixels.len() != stride * height` is rejected.
7. Oversized dimensions or overflow cases are rejected without panic.
8. Real route-facing conversion rejects non-`256x224` framebuffers unless the
   runtime schema is updated in the same change.

Use small fixtures such as `2x2` so byte expectations are readable.

## Mock Worker Integration Tests

Extend `service/tests/real-backend/main.rs` or add
`service/tests/framebuffer/main.rs` with a test-local tonic worker.

The fake worker should implement:

- existing lifecycle RPCs needed to start and stop a real session;
- `ListSlots`;
- `WatchSlots`;
- `GetFramebuffer`.

Recommended real-preview cases:

1. Start real session with `RestoreSnapshot` or `CreateVm`, request
   `/api/frame/current`, assert `200`, `width`, `height`, `format=image/png`,
   `stale=false`, `preview_hash`, and a frame image URL. Validate the JSON
   against `contracts/runtime-api.schema.json`.
2. Fetch the advertised image URL, assert `200`, `Content-Type: image/png`,
   no-store headers, PNG signature, and hash equality with metadata.
3. Assert start capabilities grant `preview: true` and keep `input: false`,
   `capture: false`.
4. Start real mode without requesting preview and assert status/events keep
   `capabilities.preview=false`.
5. Fake status current frame ahead of framebuffer frame and assert metadata
   returns `stale=true`.
6. Fake `GetFramebuffer` `FAILED_PRECONDITION` and assert public
   `backend_unavailable` without worker text.
7. Fake unsupported pixel format, non-`256x224` dimensions, and malformed
   stride/length and assert public
   `backend_unavailable`.
8. Confirm a preview RPC failure does not destroy the active session by checking
   a later `/api/session` call still reports active.
9. Confirm no public JSON body contains private root, worker socket, lease
   token, snapshot ref, create-vm config ref, or raw worker status. Run
   `PublicSanitizer` with those forbidden literals over success and error JSON.
10. After a real `Run` response without `fb_info`, assert `/api/run/status` and
    websocket `run_updated` payloads report `preview_stale=true` until a
    successful framebuffer refresh.
11. Invalid image query tests include extra query keys, empty `frame`,
    credential-like query values, oversized numeric frame, and no `image/png`
    content type on errors.

For fake framebuffer bytes, prefer a tiny non-reference size such as `2x2` in
unit tests and one M9-shaped `256x224 stride=1024` response in an integration
test to guard the expected real dimensions.

## Existing Route Tests

Keep `service/tests/frame/main.rs` passing. It already covers:

- metadata schema safety;
- stale calculation;
- image route cache consistency;
- session mismatch handling;
- JSON-safe frame counter rejection;
- frame query hint validation;
- no-store headers.

If route changes are needed, update these tests without weakening their privacy
or header assertions.

## Live Smoke

Mock worker tests are mandatory for closing implementation risk. Live smoke is
recommended when private runtime assets are available.

Level 1, worker source availability:

```bash
cd /home/infra-admin/git/preestablished/determinism-hypervisor
grpcurl -plaintext -import-path proto -proto hypervisor.proto \
  -d '{}' 127.0.0.1:7400 \
  determinism.hypervisor.v1.HypervisorWorker/GetWorkerInfo
```

Level 2, real bridge CreateVm or RestoreSnapshot:

- start `rom-operator-bridge-service` in real mode with a private
  `BRIDGE_CREATE_VM_CONFIG_REF` or `BRIDGE_REAL_SNAPSHOT_REF`;
- `POST /api/session/start` requests `preview`;
- response grants `preview: true`;
- worker free slot count decreases by one.

Level 3, real preview transcript:

- use a paused session boundary known to publish a framebuffer;
- use `curl` with `Origin: https://rombridge.birb.homes` and a cookie jar;
- `POST /api/session/start` with `backend_mode=real`;
- `GET /api/frame/current` returns `200`;
- validate metadata against `contracts/runtime-api.schema.json`;
- metadata reports `width=256`, `height=224`, `format=image/png`, and a
  browser-safe `preview_hash`;
- download the advertised image URL;
- verify PNG signature and `preview_hash`;
- verify `Cache-Control: no-store`, `Pragma: no-cache`,
  `X-Content-Type-Options: nosniff`, and `Vary: Origin`;
- `POST /api/session/stop` frees the slot.

If the available private session does not publish a framebuffer yet and
`GetFramebuffer` returns `FAILED_PRECONDITION`, do not fake success. Record the
worker condition privately and file or update a blocker with the needed
snapshot/run-boundary prerequisite.

## Quality Gates

Run from repository root or `service/` as appropriate:

```bash
cargo fmt --check
cargo test --test framebuffer
cargo test --test frame
cargo test --test real-backend
cargo test
ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1 ROM_OPERATOR_BRIDGE_FORBID_FILE=<private-forbid-file> bash scripts/redaction-gate.sh
```

If a new integration test binary is added, include it explicitly in the gate
list before the full `cargo test`.
