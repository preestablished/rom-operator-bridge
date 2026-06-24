# Subagent Review Summary

Two subagents reviewed the initial plan draft. Neither made direct file edits.

## Backend Architecture Review

Reviewer: `019efa6a-cb61-7c32-af7b-c3c30eb993c5` (`Ampere`)

Findings addressed:

- Real stale behavior after `Run` without `fb_info` was under-specified. The
  plan now requires an explicit internal stale/unknown flag or an immediate
  post-run framebuffer refresh, and it forbids deriving frame counters by adding
  `frames_elapsed`.
- Capability storage was ambiguous. The plan now distinguishes supported
  backend capabilities from the granted capabilities stored in `RealSession`.
- XRGB8888 byte order needed source proof. The plan now requires a colored
  fixture test using the determinism-hypervisor framebuffer fixture or an
  equivalent documented byte-order fixture.
- Dimension enforcement was incorrectly attributed to `validate_frame_preview`.
  The plan now makes the framebuffer converter the explicit validation point.

## Tests, Privacy, and Acceptance Review

Reviewer: `019efa6a-e7dc-7a12-bc6f-0501d2303722` (`Faraday`)

Findings addressed:

- The public schema currently requires `width=256` and `height=224`. The plan
  now requires schema validation and maps non-`256x224` real framebuffers to
  sanitized `backend_unavailable` unless schema/docs change intentionally.
- The redaction gate was incomplete. The plan now requires the publish-blocking
  `bash scripts/redaction-gate.sh` wrapper with
  `ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1` and a private forbid file.
- Frame query error behavior was imprecise. The plan now records the exact
  allowed `frame=<digits>` shape and requires errors to avoid `image/png`.
- Privacy tests now require `PublicSanitizer` over success and error JSON with
  forbidden literals for private roots, worker endpoints, leases, refs, and fake
  worker status text.
- Live smoke now includes a concrete curl/cookie/header transcript shape,
  metadata schema validation, preview hash verification, no-store/nosniff header
  checks, and slot recovery.
- The acceptance checklist now includes `cargo test --test framebuffer` if that
  integration test binary is added.

Unresolved risk:

- Live preview may require a private paused session boundary that has already
  published a framebuffer. If the available real host returns
  `FAILED_PRECONDITION`, the implementation agent should record the host
  condition and block or file follow-up rather than treating it as a pass.
