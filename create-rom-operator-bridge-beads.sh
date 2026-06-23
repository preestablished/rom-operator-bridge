#!/bin/bash
# Project: rom-operator-bridge
# Generated: 2026-06-23
# Purpose: create a Beads task graph for the private ROM operator bridge plan.

set -e

if [ ! -d ".beads" ]; then
  bd init
fi

echo "Creating rom-operator-bridge Beads task graph..."

# Phase 0 source: plans/initial/README.md, 09-implementation-sequence.md, 10-phase0-discovery-and-contract-freeze.md
PHASE0_DISCOVERY=$(bd create "Phase 0: discover bridge integration points" \
  -p 0 \
  --labels phase0-discovery,docs \
  --description "Inspect local planning docs and live preestablished checkouts to locate authoritative bridge integration points before any service or UI implementation starts." \
  --acceptance "Checkout inventory, commit ids, dirty-worktree notes, implementation location candidates, and known blockers are recorded in notes for bridge-discovery-note.md." \
  --context "Read plans/initial/README.md first, then follow 00 through 13 in order. Use /home/infra-admin/git/preestablished/reference-workload, determinism-hypervisor, and control-plane when present." \
  --notes "Reserve only discovery notes under docs/ or prompts/docs/. Do not edit service/UI code in Phase 0." \
  --silent)

REFWORK_DISCOVERY=$(bd create "Inspect reference-workload contracts" \
  -p 0 \
  --labels phase0-discovery,input-padlog,verifier,privacy,docs \
  --description "Locate the padlog parser/writer, console16-12btn-v1 mapping, verifier CLI commands, redaction scanner, Phase 4 bundle expectations, capture index schema, label and dedup expectations." \
  --acceptance "Discovery notes name exact files/functions/commands for padlog parsing, layout mapping, phase4 layout/score-plan/bundle/checksum/redaction commands, and agent-runnable synthetic checks." \
  --context "Source: 04-input-padlog-contract.md, 08-validation-runbook.md, 10-phase0-discovery-and-contract-freeze.md. Inspect reference-workload required files and rg searches listed there." \
  --notes "Reserve docs/bridge-discovery-note.md or prompts/docs/bridge-discovery-note.md only." \
  --silent)
bd dep add "$REFWORK_DISCOVERY" "$PHASE0_DISCOVERY"

HYPERVISOR_DISCOVERY=$(bd create "Inspect hypervisor runtime contracts" \
  -p 0 \
  --labels phase0-discovery,real-backend,input-padlog,capture-labels,docs \
  --description "Inspect determinism-hypervisor for worker lifecycle, input injection, frame counter, framebuffer, snapshot, capture/export APIs, lease or slot ownership, pause/resume, and crash cleanup." \
  --acceptance "Discovery notes explain exactly how one pad word reaches the running ROM, how frame bases align, how stale preview is detected, and how one capture becomes a durable captures/index.jsonl row or why real capture is blocked." \
  --context "Source: 02-host-control-service.md, 04-input-padlog-contract.md, 05-capture-export-and-labeling.md, 10-phase0-discovery-and-contract-freeze.md." \
  --notes "Reserve discovery notes only. Do not reset dirty checkouts." \
  --silent)
bd dep add "$HYPERVISOR_DISCOVERY" "$PHASE0_DISCOVERY"

CONTROL_PLANE_DISCOVERY=$(bd create "Inspect control-plane integration options" \
  -p 0 \
  --labels phase0-discovery,real-backend,capture-labels,verifier,docs \
  --description "Check whether control-plane already provides scorer, capture, snapshot, feature-map, or service contracts the bridge should reuse." \
  --acceptance "Discovery notes record whether the bridge should call control-plane APIs, avoid them, or mark a docs/code gap with exact follow-up." \
  --context "Source: 01-architecture.md, 05-capture-export-and-labeling.md, 10-phase0-discovery-and-contract-freeze.md." \
  --notes "Reserve discovery notes only." \
  --silent)
bd dep add "$CONTROL_PLANE_DISCOVERY" "$PHASE0_DISCOVERY"

DEPLOYMENT_DISCOVERY=$(bd create "Freeze deployment and security shape" \
  -p 0 \
  --labels phase0-discovery,deploy-security,privacy,docs \
  --description "Decide whether runtime same-origin proxying is available under https://birb.homes/rom-bridge/ or whether the bridge serves a same-network HTTPS endpoint with strict CORS." \
  --acceptance "Discovery notes name static UI path, runtime API path, WebSocket path, service bind address, Origin allowlist, no-cache policy, restart command, rollback command, and credential rotation shape." \
  --context "Source: 06-hosting-on-birb-homes.md, 07-security-and-privacy.md, 13-deployment-security-checklist.md." \
  --notes "Reserve discovery/deployment notes only; do not publish or run deployment commands." \
  --silent)
bd dep add "$DEPLOYMENT_DISCOVERY" "$PHASE0_DISCOVERY"

DISCOVERY_NOTE=$(bd create "Write bridge-discovery-note.md" \
  -p 0 \
  --labels phase0-discovery,docs \
  --description "Create the Phase 0 discovery note using the required template, answering every contract question or marking explicit blockers." \
  --acceptance "bridge-discovery-note.md exists; implementation location, host-control lifecycle, input, framebuffer, capture, label/verifier, deployment, gaps, and exact test commands are filled in." \
  --context "Source: 10-phase0-discovery-and-contract-freeze.md Discovery Note Template and Gate." \
  --notes "Reserve docs/bridge-discovery-note.md or prompts/docs/bridge-discovery-note.md." \
  --silent)
bd dep add "$DISCOVERY_NOTE" "$REFWORK_DISCOVERY"
bd dep add "$DISCOVERY_NOTE" "$HYPERVISOR_DISCOVERY"
bd dep add "$DISCOVERY_NOTE" "$CONTROL_PLANE_DISCOVERY"
bd dep add "$DISCOVERY_NOTE" "$DEPLOYMENT_DISCOVERY"

CONTRACT_FREEZE=$(bd create "Accept Phase 0 contract freeze" \
  -p 0 \
  --labels phase0-discovery,docs \
  --description "Review bridge-discovery-note.md and freeze runtime API, browser input flow, deployment/security deviations, and chosen stack/repo locations before service or UI work." \
  --acceptance "All Phase 0 gate bullets pass; exact pad mapping and padlog writer behavior are confirmed; real capture is named or explicitly deferred; runbook commands are exact." \
  --context "No service, UI, real-backend, capture, verifier, deployment, handoff, or implementation-docs bead may be ready before this bead completes." \
  --notes "This bead is the hard dependency gate for all implementation work." \
  --silent)
bd dep add "$CONTRACT_FREEZE" "$DISCOVERY_NOTE"

# Phase 1 and shared contracts source: 01-architecture.md, 02-host-control-service.md, 09-implementation-sequence.md, 11-runtime-api-contract.md
API_CONTRACT=$(bd create "Define typed runtime API and backend traits" \
  -p 0 \
  --labels service-synthetic,ui-mvp,real-backend,capture-labels,verifier,docs \
  --description "Codify schema_version 1 HTTP routes, WebSocket envelopes, common error envelope, event model, auth/session transport, capabilities, and synthetic/real backend traits." \
  --acceptance "Service and UI share synchronized types or generated schemas; major schema mismatch is rejected; documented deviations from 11-runtime-api-contract.md match Phase 0." \
  --context "Source: 01-architecture.md, 02-host-control-service.md, 11-runtime-api-contract.md." \
  --notes "Reserve service API contract files, generated type surfaces, UI API type files, and docs/runtime-api.md." \
  --silent)
bd dep add "$API_CONTRACT" "$CONTRACT_FREEZE"

SERVICE_SCAFFOLD=$(bd create "Scaffold host bridge service" \
  -p 0 \
  --labels service-synthetic,tests \
  --description "Create the chosen Rust or Phase-0-selected service app with config loading, health route, common error envelope, tracing/log setup, and synthetic backend wiring." \
  --acceptance "Service starts locally in synthetic mode; GET /health returns schema_version 1 without private paths; package test/build commands are recorded." \
  --context "Source: 02-host-control-service.md and 09 Phase 1. Current repo is thin, so follow Phase 0 stack decision." \
  --notes "Likely reservation: service/**, crates/**, Cargo.toml, tests/service/**, docs/runbook.md." \
  --silent)
bd dep add "$SERVICE_SCAFFOLD" "$API_CONTRACT"

CONFIG_PRIVATE_ROOT=$(bd create "Implement private config and root validation" \
  -p 0 \
  --labels service-synthetic,deploy-security,privacy,tests \
  --description "Load bridge config from environment or uncommitted files, create private run directories, enforce 0700 directories and 0600 files, and reject world-writable roots or roots inside static publish directories." \
  --acceptance "Tests cover placeholder config, missing secrets, private file modes, world-writable root rejection, static-publish-root rejection, and no committed ROM paths/tokens/private roots." \
  --context "Source: 02-host-control-service.md, 07-security-and-privacy.md, 13-deployment-security-checklist.md." \
  --notes "Reserve service config/private-root modules and tests/config/**." \
  --silent)
bd dep add "$CONFIG_PRIVATE_ROOT" "$SERVICE_SCAFFOLD"

AUTH_SESSION_ORIGIN=$(bd create "Implement auth, session TTL, and origin controls" \
  -p 0 \
  --labels service-synthetic,deploy-security,privacy,tests \
  --description "Implement HTTP and WebSocket authentication, no credentials in URLs, 4-hour session TTL, one active operator session, Origin/CORS allowlisting, auth rate limiting, and sanitized auth errors." \
  --acceptance "Tests cover missing/expired/bad credentials, credential-in-query rejection, unrelated Origin rejection, WebSocket handshake auth, one-session lock, rate limiting, and no private path leakage." \
  --context "Source: 07-security-and-privacy.md, 11-runtime-api-contract.md, 13-deployment-security-checklist.md. Prefer HttpOnly Secure SameSite=Strict cookie auth unless Phase 0 chooses HTTPS-only header auth." \
  --notes "Reserve service auth/session middleware and tests/auth/**." \
  --silent)
bd dep add "$AUTH_SESSION_ORIGIN" "$CONFIG_PRIVATE_ROOT"

SYNTH_SESSION_STATUS=$(bd create "Implement synthetic session and status routes" \
  -p 0 \
  --labels service-synthetic,tests \
  --description "Implement /api/session, /api/session/start, /api/session/stop, /api/run/status, pause, resume, event append rows, synthetic frame counter, and run manifest skeleton." \
  --acceptance "Synthetic session can start, report running/paused/stopped/faulted states, stop cleanly, write final events, and create private output only under configured root." \
  --context "Source: 01-architecture.md, 02-host-control-service.md, 09 Phase 1, 11-runtime-api-contract.md." \
  --notes "Reserve service session/status/event modules and tests/session/**." \
  --silent)
bd dep add "$SYNTH_SESSION_STATUS" "$AUTH_SESSION_ORIGIN"

SYNTH_FRAME_PREVIEW=$(bd create "Implement synthetic framebuffer preview routes" \
  -p 1 \
  --labels service-synthetic,ui-mvp,privacy,tests \
  --description "Provide generated synthetic frame metadata and PNG preview bytes with stable dimensions, preview_hash, stale flag, and no-store headers." \
  --acceptance "GET /api/frame/current and /api/frame/current/image match schema, return no-store/nosniff headers, never expose private artifacts, and mark stale previews by the contract threshold." \
  --context "Source: 02-host-control-service.md, 03-web-operator-ui.md, 11-runtime-api-contract.md." \
  --notes "Reserve service frame/preview modules and tests/frame/**." \
  --silent)
bd dep add "$SYNTH_FRAME_PREVIEW" "$SYNTH_SESSION_STATUS"

EVENT_STREAM=$(bd create "Implement sanitized event WebSocket stream" \
  -p 1 \
  --labels service-synthetic,ui-mvp,privacy,tests \
  --description "Implement /ws/events with authenticated handshakes, monotonically increasing server_seq, sanitized status/capture/label/validation events, and old-event handling guidance for the UI." \
  --acceptance "Tests cover authenticated event connections, ordered server_seq, sanitized payloads, session mismatch rejection, and no private paths/raw payload fields in UI-visible events." \
  --context "Source: 01-architecture.md and 11-runtime-api-contract.md WebSocket Envelope." \
  --notes "Reserve service ws/events modules and tests/ws/**." \
  --silent)
bd dep add "$EVENT_STREAM" "$SYNTH_SESSION_STATUS"

# Phase 2 source: 04-input-padlog-contract.md, 09-implementation-sequence.md, 11-runtime-api-contract.md, 12-browser-input-and-operator-flow.md
PAD_MAPPING=$(bd create "Implement console16-12btn-v1 mapping" \
  -p 0 \
  --labels input-padlog,service-synthetic,tests \
  --description "Implement isolated constants and conversion for all twelve console16-12btn-v1 buttons, opposite-direction neutralization, merge policy, and reserved-bit rejection." \
  --acceptance "Unit tests cover every button bit, keyboard/gamepad merge, opposite D-pad neutralization, sorted button names, and reserved bits 12-15 always zero or rejected." \
  --context "Source: 04-input-padlog-contract.md and 12-browser-input-and-operator-flow.md." \
  --notes "Reserve service input mapping modules and tests/input_mapping/**." \
  --silent)
bd dep add "$PAD_MAPPING" "$SYNTH_SESSION_STATUS"

PADLOG_WRITER=$(bd create "Implement padlog writer and parser round trip" \
  -p 0 \
  --labels input-padlog,service-synthetic,privacy,tests \
  --description "Write canonical .padlog output with required header, lowercase hex pad words, run-length rows, trailing newline, and sidecar private event JSONL for rich diagnostics." \
  --acceptance "Round-trip test passes through the existing refwork parser when accessible; reserved-bit padlogs are rejected; applied frame rows match expected pad words; rich fields stay out of .padlog." \
  --context "Source: 04-input-padlog-contract.md and Phase 0 reference-workload discovery." \
  --notes "Reserve service padlog modules, private event log modules, tests/padlog/**." \
  --silent)
bd dep add "$PADLOG_WRITER" "$PAD_MAPPING"

INPUT_SCHEDULER=$(bd create "Implement frame-boundary input scheduler" \
  -p 0 \
  --labels input-padlog,service-synthetic,tests \
  --description "Assign browser input states to replay frames using current_frame + 1, retry once on late backend rejection, preserve ordering, queue paused input, and record private input rejections." \
  --acceptance "Tests cover fake frame counter assignment, pause/resume pending state, late retry, failed retry input_reject, every applied frame has exactly one pad word, and u64 frame serialization." \
  --context "Source: 02-host-control-service.md, 04-input-padlog-contract.md, 12-browser-input-and-operator-flow.md." \
  --notes "Reserve service input scheduler modules and tests/input_scheduler/**." \
  --silent)
bd dep add "$INPUT_SCHEDULER" "$PADLOG_WRITER"

INPUT_WEBSOCKET=$(bd create "Implement input WebSocket ack and idempotency" \
  -p 0 \
  --labels input-padlog,service-synthetic,privacy,tests \
  --description "Implement /ws/input envelope validation, authenticated handshake, per-source monotonic client_seq, duplicate client_seq idempotency, queue limit 120, input_ack, input_reject, and reconnect zero-state behavior." \
  --acceptance "Tests cover duplicate client_seq acknowledged with original result and not applied twice, queue overflow rejection, schema_version mismatch, invalid buttons, reconnect zero input, and sanitized input_reject errors." \
  --context "Source: 11-runtime-api-contract.md WebSocket Envelope and 12-browser-input-and-operator-flow.md Synthetic Edge Tests." \
  --notes "Reserve service ws/input modules and tests/ws_input/**." \
  --silent)
bd dep add "$INPUT_WEBSOCKET" "$INPUT_SCHEDULER"

SYNTH_INPUT_INTEGRATION=$(bd create "Add synthetic input and padlog integration tests" \
  -p 0 \
  --labels input-padlog,service-synthetic,tests \
  --description "Start the synthetic service, open input channel, send button sequences, advance fake frames, and assert padlog rows, events, acks, duplicate handling, and zero-input edge cases." \
  --acceptance "Integration tests cover all-button mapping, reserved bits, parser round trip, duplicate client_seq, focus/page-hidden/reconnect/gamepad-disconnect zero-input behavior, and private event log diagnostics." \
  --context "Source: 04-input-padlog-contract.md Tests, 08-validation-runbook.md Synthetic Backend Validation, 12-browser-input-and-operator-flow.md Synthetic Edge Tests." \
  --notes "Reserve integration tests and fixtures; do not require ROM or real hypervisor." \
  --silent)
bd dep add "$SYNTH_INPUT_INTEGRATION" "$INPUT_WEBSOCKET"

# Phase 3 source: 03-web-operator-ui.md, 09-implementation-sequence.md, 11-runtime-api-contract.md, 12-browser-input-and-operator-flow.md
UI_SCAFFOLD=$(bd create "Scaffold browser operator UI" \
  -p 1 \
  --labels ui-mvp,tests \
  --description "Create the Phase-0-selected TypeScript SPA/static UI app with test/build commands, sanitized runtime config, no service worker, and same-origin API base support." \
  --acceptance "UI builds as a static bundle; runtime config contains no secrets; no service worker caches runtime endpoints; package commands are documented." \
  --context "Source: 03-web-operator-ui.md, 06-hosting-on-birb-homes.md, 13-deployment-security-checklist.md." \
  --notes "Likely reservation: ui/**, package.json, tsconfig.json, vite config, tests/ui/**." \
  --silent)
bd dep add "$UI_SCAFFOLD" "$API_CONTRACT"

UI_API_CLIENT=$(bd create "Implement typed UI API and WebSocket client" \
  -p 1 \
  --labels ui-mvp,tests,privacy \
  --description "Implement typed HTTP client, input/event WebSocket clients, schema_version checking, common error handling, session state model, mocked client test seams, and no credential-in-URL behavior." \
  --acceptance "Tests cover start/stop/status/pause/resume/frame/capture/labels clients, schema mismatch rejection, auth/origin errors, event ordering, WebSocket reconnect, and sanitized error display data." \
  --context "Source: 03-web-operator-ui.md API Client and 11-runtime-api-contract.md." \
  --notes "Reserve ui API client/state modules and tests/api-client/**." \
  --silent)
bd dep add "$UI_API_CLIENT" "$UI_SCAFFOLD"
bd dep add "$UI_API_CLIENT" "$EVENT_STREAM"

UI_SESSION_PLAY=$(bd create "Build session and play surface views" \
  -p 1 \
  --labels ui-mvp,service-synthetic,tests \
  --description "Implement session controls, status panel, pause/resume, trigger capture button shell, live framebuffer preview, focus state, current guest frame, pressed buttons, and recent padlog tail." \
  --acceptance "UI works against synthetic backend; preview has stable dimensions and stale controls disable input/capture; no private paths/raw payloads/screenshots are rendered from static data." \
  --context "Source: 03-web-operator-ui.md Required Views and 12-browser-input-and-operator-flow.md Error Recovery." \
  --notes "Reserve ui session/play components and component tests." \
  --silent)
bd dep add "$UI_SESSION_PLAY" "$UI_API_CLIENT"
bd dep add "$UI_SESSION_PLAY" "$SYNTH_FRAME_PREVIEW"

UI_INPUT_HANDLING=$(bd create "Implement keyboard and gamepad input UX" \
  -p 1 \
  --labels ui-mvp,input-padlog,tests \
  --description "Implement focused keyboard capture, fixed key mapping, key-repeat ignore, preventDefault, Standard Gamepad polling, analog deadzone, source merge, opposite-direction neutralization display, and zero-button release behavior." \
  --acceptance "UI tests cover focus loss, blur, page hidden, socket close, session stop, reconnect, gamepad disconnect, key repeat, all keyboard mappings, all gamepad mappings, and displayed console16-12btn-v1 names." \
  --context "Source: 03-web-operator-ui.md Input UX and 12-browser-input-and-operator-flow.md." \
  --notes "Reserve ui input hooks/modules and tests/input/**." \
  --silent)
bd dep add "$UI_INPUT_HANDLING" "$UI_SESSION_PLAY"
bd dep add "$UI_INPUT_HANDLING" "$SYNTH_INPUT_INTEGRATION"

UI_ERROR_RECOVERY=$(bd create "Implement browser-safe recovery states" \
  -p 1 \
  --labels ui-mvp,privacy,tests \
  --description "Implement sanitized UI states for unavailable bridge, auth rejected, origin rejected, backend unavailable, session active elsewhere, capture in progress/failed, label conflict, validation failed, frame stale, gamepad disconnect, and WebSocket reconnect." \
  --acceptance "Tests verify each recovery state follows 12-browser-input-and-operator-flow.md and never displays private paths, raw command output, feature bytes, validation reports, or screenshots from static data." \
  --context "Source: 03-web-operator-ui.md Error States and 12-browser-input-and-operator-flow.md Error Recovery." \
  --notes "Reserve ui error/state modules and tests/error-states/**." \
  --silent)
bd dep add "$UI_ERROR_RECOVERY" "$UI_INPUT_HANDLING"

UI_NO_PERSISTENCE=$(bd create "Verify browser no-persistence policy" \
  -p 0 \
  --labels ui-mvp,privacy,deploy-security,tests \
  --description "Add automated checks that real-run payload surfaces are not stored in localStorage, IndexedDB, Cache API/service workers, browser downloads, static fixtures, source maps with private paths, or preview caches." \
  --acceptance "Tests and static inspection fail if private-like payloads, preview blobs, feature values, exact private paths, credentials, capture ids from real runs, or validation excerpts are persisted or embedded." \
  --context "Source: 03-web-operator-ui.md, 07-security-and-privacy.md Browser Storage, 13-deployment-security-checklist.md Static Publish Rules." \
  --notes "Reserve ui privacy tests, build config, and redaction test fixtures." \
  --silent)
bd dep add "$UI_NO_PERSISTENCE" "$UI_ERROR_RECOVERY"

SYNTH_UI_SMOKE=$(bd create "Validate synthetic UI operator flow" \
  -p 1 \
  --labels ui-mvp,service-synthetic,input-padlog,capture-labels,tests \
  --description "Run or document a synthetic Mac-browser smoke where the UI starts a session, sends keyboard/gamepad input, receives frame updates, triggers synthetic capture, writes labels, and stops cleanly." \
  --acceptance "Smoke or automated E2E covers connection, input, preview, stale preview, capture retry, label conflict, reconnect, auth failure redaction, and private temp output inspection." \
  --context "Source: 08-validation-runbook.md Synthetic Backend Validation and 09 Phase 3." \
  --notes "Reserve e2e tests and docs/synthetic-smoke.md." \
  --silent)
bd dep add "$SYNTH_UI_SMOKE" "$UI_NO_PERSISTENCE"

# Phase 4 source: 02-host-control-service.md, 04-input-padlog-contract.md, 09-implementation-sequence.md, 10-phase0-discovery-and-contract-freeze.md
REAL_BACKEND_TRAIT=$(bd create "Implement real backend attachment lifecycle" \
  -p 1 \
  --labels real-backend,tests \
  --description "Implement the Phase-0-frozen real backend behind the same service interface as SyntheticBackend, including launch or attach, lease/slot ownership, pause/resume, stop, crash cleanup, and backend_unavailable errors." \
  --acceptance "Real backend can start or attach on the Linux host per discovery note; synthetic tests still pass; missing real backend fails with sanitized machine-readable errors." \
  --context "Source: 02-host-control-service.md, 09 Phase 4, bridge-discovery-note.md." \
  --notes "Reserve service real backend modules and tests/real-backend/**." \
  --silent)
bd dep add "$REAL_BACKEND_TRAIT" "$SYNTH_INPUT_INTEGRATION"

REAL_INPUT_MAPPING=$(bd create "Wire real frame-boundary input injection" \
  -p 1 \
  --labels real-backend,input-padlog,tests \
  --description "Send scheduled pad words into the authoritative hypervisor/control input API with confirmed port, at_frame, lead-frame policy, frame-base alignment, and rejection handling." \
  --acceptance "Real-host smoke logs privately prove a browser input maps to a hypervisor scheduled event and to the service-written padlog frame without reserved bits or duplicate application." \
  --context "Source: 04-input-padlog-contract.md Hypervisor Input Log Distinction, 12-browser-input-and-operator-flow.md Input Timing, bridge-discovery-note.md." \
  --notes "Reserve real input adapter modules and docs/real-backend-smoke.md." \
  --silent)
bd dep add "$REAL_INPUT_MAPPING" "$REAL_BACKEND_TRAIT"

REAL_FRAMEBUFFER=$(bd create "Wire real framebuffer preview source" \
  -p 1 \
  --labels real-backend,ui-mvp,privacy,tests \
  --description "Read or request the current real framebuffer source, convert to browser-safe preview PNG, apply stale threshold, and keep raw framebuffer payloads under private roots only." \
  --acceptance "Real preview routes return no-store/nosniff headers, stable metadata, sanitized errors, no raw payload fields, and stale status when behind threshold." \
  --context "Source: 01-architecture.md Framebuffer flow, 02-host-control-service.md Framebuffer Preview, bridge-discovery-note.md." \
  --notes "Reserve real framebuffer adapter modules and tests/framebuffer/**." \
  --silent)
bd dep add "$REAL_FRAMEBUFFER" "$REAL_INPUT_MAPPING"

REAL_BACKEND_SMOKE=$(bd create "Document and run real backend smoke" \
  -p 1 \
  --labels real-backend,input-padlog,handoff,tests \
  --description "Create a private real-host smoke runbook and, when operator data is available, verify Mac browser control, current frame, framebuffer preview, button press/release padlog rows, pause/resume, and clean stop." \
  --acceptance "Smoke notes avoid private screenshots/output, record pass/fail and sanitized paths only, and prove the operator can drive the real ROM from a Mac browser through the Linux host." \
  --context "Source: 08-validation-runbook.md Real Backend Smoke and 09 Phase 4." \
  --notes "Reserve docs/real-backend-smoke.md; private transcripts stay under private run directory." \
  --silent)
bd dep add "$REAL_BACKEND_SMOKE" "$REAL_FRAMEBUFFER"

# Phase 5 source: 05-capture-export-and-labeling.md, 09-implementation-sequence.md, 11-runtime-api-contract.md, 12-browser-input-and-operator-flow.md
SYNTH_CAPTURE=$(bd create "Implement synthetic capture job lifecycle" \
  -p 1 \
  --labels capture-labels,service-synthetic,tests \
  --description "Implement asynchronous synthetic capture trigger/jobs/recent/detail endpoints with idempotency keys, requested/capturing/completed/failed/not_labelable states, pagination, retryable failures, and sanitized provenance." \
  --acceptance "Tests cover idempotent trigger, active capture rejection, failure retry with new idempotency key, newest-first recent list, not_labelable rows, and completed only after synthetic private index row is durable." \
  --context "Source: 05-capture-export-and-labeling.md, 11-runtime-api-contract.md capture endpoints, 12-browser-input-and-operator-flow.md Capture Review State Machine." \
  --notes "Reserve service capture modules and tests/capture/**." \
  --silent)
bd dep add "$SYNTH_CAPTURE" "$SYNTH_FRAME_PREVIEW"
bd dep add "$SYNTH_CAPTURE" "$SYNTH_INPUT_INTEGRATION"

LABEL_STORE=$(bd create "Implement private label draft store" \
  -p 1 \
  --labels capture-labels,privacy,tests \
  --description "Implement versioned label draft schema, upsert/delete operations, idempotency keys, label_revision increments, role cardinality, conflict rules, dedup groups, note validation, and active-run capture id validation." \
  --acceptance "Tests cover first_boss/goal_positive/goal_negative uniqueness, rejected conflicts, needs_review rules, dedup group update/delete, note length/control-character rejection, escaped render data, schema rejection, and labels outside active run blocked." \
  --context "Source: 05-capture-export-and-labeling.md Labels and 12-browser-input-and-operator-flow.md Label Semantics." \
  --notes "Reserve service label modules, schema files, tests/labels/**." \
  --silent)
bd dep add "$LABEL_STORE" "$SYNTH_CAPTURE"

UI_CAPTURE_LABELS=$(bd create "Build capture review and label drawer UI" \
  -p 1 \
  --labels ui-mvp,capture-labels,tests \
  --description "Implement recent capture list, job status rows, detail view, sanitized provenance, retry affordance, privileged feature panel shell, label drawer, role conflicts, dedup editing, and private note entry." \
  --acceptance "UI tests cover requested/capturing/completed/failed/not_labelable states, label writes/deletes, conflict warnings, capture retry, no raw private paths, and no browser persistence for notes/features/previews." \
  --context "Source: 03-web-operator-ui.md Capture Review and Label Drawer, 12-browser-input-and-operator-flow.md." \
  --notes "Reserve ui capture/label components and tests/capture-labels/**." \
  --silent)
bd dep add "$UI_CAPTURE_LABELS" "$LABEL_STORE"
bd dep add "$UI_CAPTURE_LABELS" "$SYNTH_UI_SMOKE"

PRIVILEGED_FEATURES=$(bd create "Implement privileged capture feature view" \
  -p 2 \
  --labels capture-labels,privacy,ui-mvp,tests \
  --description "Expose decoded feature names and values only through authenticated privileged runtime routes and render them in the operator session without static or browser persistence." \
  --acceptance "Tests verify no-store headers, auth required, no absolute private paths by default, feature values never enter static build/browser storage, and UI handles unavailable feature maps." \
  --context "Source: 05-capture-export-and-labeling.md Knowing A State Was Hit, 07-security-and-privacy.md Server Responses, 11-runtime-api-contract.md features endpoint." \
  --notes "Reserve privileged feature service/UI modules and privacy tests." \
  --silent)
bd dep add "$PRIVILEGED_FEATURES" "$UI_CAPTURE_LABELS"

CAPTURE_LABEL_INTEGRATION=$(bd create "Add synthetic capture and label integration tests" \
  -p 1 \
  --labels capture-labels,service-synthetic,ui-mvp,tests \
  --description "Exercise the full synthetic capture and label flow through service and UI client, including private capture rows, label draft file, conflicts, retry, and event stream updates." \
  --acceptance "Integration tests prove synthetic capture/labels round trip through private files and UI state, but do not represent synthetic capture as real Phase 4 acceptance." \
  --context "Source: 08-validation-runbook.md Synthetic Backend Validation and 09 Phase 5." \
  --notes "Reserve integration/e2e capture-label tests." \
  --silent)
bd dep add "$CAPTURE_LABEL_INTEGRATION" "$PRIVILEGED_FEATURES"

REAL_CAPTURE_EXPORT=$(bd create "Wire real capture export integration" \
  -p 1 \
  --labels real-backend,capture-labels,privacy,tests \
  --description "Call the Phase-0-frozen real capture/export mechanism, write/update captures/index.jsonl with confirmed schema, and mark jobs completed only after durable private index row and required payload references exist." \
  --acceptance "Real capture returns sanitized job metadata; private artifact references are durable before completed; no raw captures/screenshots/feature bytes/private paths leak to UI; if no exporter exists, this bead is marked blocked/deferred instead of faking acceptance." \
  --context "Source: 05-capture-export-and-labeling.md Export Integration and 09 Phase 5." \
  --notes "Reserve real capture adapter modules, capture index writer, tests/real-capture/**." \
  --silent)
bd dep add "$REAL_CAPTURE_EXPORT" "$REAL_BACKEND_SMOKE"
bd dep add "$REAL_CAPTURE_EXPORT" "$CAPTURE_LABEL_INTEGRATION"

REAL_CAPTURE_SMOKE=$(bd create "Run real one-capture label smoke" \
  -p 1 \
  --labels real-backend,capture-labels,handoff,tests \
  --description "On the Linux host with private ROM/config, trigger one real capture from the UI, confirm private captures/index.jsonl row, add a needs_review label, and stop the session." \
  --acceptance "Private smoke evidence confirms real capture and label draft were written; public notes contain only sanitized status and no screenshots/private paths/raw reports." \
  --context "Source: 08-validation-runbook.md Real Backend Smoke and 09 Phase 5." \
  --notes "Reserve docs/real-capture-smoke.md; private evidence stays under private run directory." \
  --silent)
bd dep add "$REAL_CAPTURE_SMOKE" "$REAL_CAPTURE_EXPORT"

# Phase 6 source: 05-capture-export-and-labeling.md, 08-validation-runbook.md, 09-implementation-sequence.md
VERIFIER_RUNBOOK=$(bd create "Update verifier runbook commands" \
  -p 1 \
  --labels verifier,docs,handoff \
  --description "Replace placeholder verifier and bridge commands with exact Phase-0-confirmed commands for layout, score-plan, bundle-check, checksum manifest, padlog validation, redaction scan, synthetic tests, and private real-host checks." \
  --acceptance "Runbook distinguishes agent-runnable synthetic commands from private operator-only commands and never prints secret config values or private paths in shared logs." \
  --context "Source: 08-validation-runbook.md and bridge-discovery-note.md." \
  --notes "Reserve docs/runbook.md, docs/validation-runbook.md, or prompts/docs/runbook.md." \
  --silent)
bd dep add "$VERIFIER_RUNBOOK" "$CONTRACT_FREEZE"

LABEL_TO_SCORE_PLAN=$(bd create "Transform labels to verifier inputs" \
  -p 1 \
  --labels verifier,capture-labels,privacy,tests \
  --description "Translate target labels and dedup groups from the private draft file into phase4-score-plan arguments or equivalent private config without creating a second source of truth." \
  --acceptance "Tests cover first_boss, goal_positive, goal_negative, dedup artifact generation, missing required labels, conflicting labels, and private config/report paths kept server-side." \
  --context "Source: 05-capture-export-and-labeling.md Label Acceptance Criteria and 12-browser-input-and-operator-flow.md Verifier transformations." \
  --notes "Reserve verifier transform modules and tests/verifier/**." \
  --silent)
bd dep add "$LABEL_TO_SCORE_PLAN" "$REAL_CAPTURE_SMOKE"
bd dep add "$LABEL_TO_SCORE_PLAN" "$VERIFIER_RUNBOOK"

VALIDATION_STATUS=$(bd create "Expose sanitized validation status" \
  -p 1 \
  --labels verifier,ui-mvp,privacy,tests \
  --description "Record private validation runs and expose only pass/fail, command class, timestamps, and sanitized issue summaries to the UI." \
  --acceptance "Tests prove validation reports, stdout/stderr, private paths, feature values, and exact command transcripts remain private; UI displays sanitized validation_failed and pass states." \
  --context "Source: 02-host-control-service.md Validation Runner, 07-security-and-privacy.md Server Responses, 08-validation-runbook.md." \
  --notes "Reserve service validation status modules, ui validation components, tests/validation-status/**." \
  --silent)
bd dep add "$VALIDATION_STATUS" "$LABEL_TO_SCORE_PLAN"

OPTIONAL_VALIDATION_RUNNER=$(bd create "Optionally implement service-side validation runner" \
  -p 2 \
  --labels verifier,privacy,tests \
  --description "If Phase 0 chooses service-side automation, invoke refwork-verify from the configured checkout with private paths from config/environment, capture reports privately, and return sanitized status only." \
  --acceptance "Runner is implemented only if selected in bridge-discovery-note.md; tests cover command failure, timeout/cancel if supported, private report files, sanitized stderr, and no UI-supplied private path arguments." \
  --context "Source: 02-host-control-service.md Validation Runner and 08-validation-runbook.md. If not selected, document runbook-only validation and close/defer this bead." \
  --notes "Reserve validation runner modules and tests/validation-runner/**." \
  --silent)
bd dep add "$OPTIONAL_VALIDATION_RUNNER" "$VALIDATION_STATUS"

BUNDLE_CHECK_ACCEPTANCE=$(bd create "Validate bridge-produced private bundle" \
  -p 1 \
  --labels verifier,capture-labels,handoff,tests \
  --description "Run or document the private Phase 4 verifier path over a bridge-produced bundle, including layout, score plan, bundle check, checksum manifest, and redaction reports." \
  --acceptance "A private bridge-produced bundle passes phase4-bundle-check, or blockers are recorded with exact failing sanitized status; public notes contain no private reports or paths." \
  --context "Source: 08-validation-runbook.md Acceptance Checklist and 09 Phase 6." \
  --notes "Reserve docs/private-bundle-validation-summary.md for sanitized summary only." \
  --silent)
bd dep add "$BUNDLE_CHECK_ACCEPTANCE" "$OPTIONAL_VALIDATION_RUNNER"

# Phase 7 and blocking security source: 06-hosting-on-birb-homes.md, 07-security-and-privacy.md, 13-deployment-security-checklist.md
RUNTIME_HEADERS=$(bd create "Enforce runtime and SPA security headers" \
  -p 0 \
  --labels deploy-security,privacy,service-synthetic,ui-mvp,tests \
  --description "Ensure runtime API, WebSocket, preview routes, and SPA responses include required no-store/nosniff/CSP/referrer/frame headers according to route class." \
  --acceptance "Tests verify runtime/private preview routes return Cache-Control: no-store, Pragma: no-cache, X-Content-Type-Options: nosniff; SPA includes CSP, Referrer-Policy, and X-Frame-Options." \
  --context "Source: 11-runtime-api-contract.md Status and 13-deployment-security-checklist.md Runtime Headers." \
  --notes "Reserve service header middleware, UI hosting config, tests/headers/**." \
  --silent)
bd dep add "$RUNTIME_HEADERS" "$AUTH_SESSION_ORIGIN"
bd dep add "$RUNTIME_HEADERS" "$UI_NO_PERSISTENCE"

STATIC_REDACTION_GATE=$(bd create "Implement static redaction gate" \
  -p 0 \
  --labels deploy-security,privacy,verifier,tests \
  --description "Add a publish-blocking redaction scan over static UI/docs output using the Phase-0-confirmed refwork-verify redaction-scan command and private forbidden-literals file." \
  --acceptance "Static scan fails on ROM paths, private corpus roots, operator credentials, real capture ids, screenshots, preview caches, source maps with private paths, validation excerpts, and private network literals." \
  --context "Source: 07-security-and-privacy.md Redaction Gate and 13-deployment-security-checklist.md Redaction Gate." \
  --notes "Reserve scripts/redaction-gate.*, tests/redaction/**, docs/redaction.md. Reports go to private validation dir." \
  --silent)
bd dep add "$STATIC_REDACTION_GATE" "$RUNTIME_HEADERS"
bd dep add "$STATIC_REDACTION_GATE" "$VERIFIER_RUNBOOK"

DEPLOY_PROXY_DOC=$(bd create "Document reverse proxy and TLS deployment" \
  -p 1 \
  --labels deploy-security,docs,handoff \
  --description "Document the actual birb.homes static path, runtime API path, WebSocket path, TLS termination, bind address, proxy config path, restart command, rollback command, credential source, TTL, and rotation." \
  --acceptance "Deployment note follows the 13-deployment-security-checklist.md template and records whether runtime binds localhost or a documented trusted interface." \
  --context "Source: 06-hosting-on-birb-homes.md and 13-deployment-security-checklist.md Deployment Note Template." \
  --notes "Reserve docs/deployment-note.md or prompts/docs/deployment-note.md." \
  --silent)
bd dep add "$DEPLOY_PROXY_DOC" "$DEPLOYMENT_DISCOVERY"
bd dep add "$DEPLOY_PROXY_DOC" "$RUNTIME_HEADERS"

NETWORK_ISOLATION_TESTS=$(bd create "Verify deployment network isolation" \
  -p 0 \
  --labels deploy-security,privacy,tests \
  --description "Run or document checks that service bind is localhost or trusted interface, health is sanitized, unrelated Origins are rejected, unauthenticated requests are rejected, runtime responses are no-store, and outside-network access is unavailable or rejected." \
  --acceptance "Verification notes include sanitized results for bind, health, Origin rejection, unauthenticated rejection, no-store headers, WSS access, and mixed-content absence." \
  --context "Source: 13-deployment-security-checklist.md Network Isolation Tests and 08-validation-runbook.md Hosting Validation." \
  --notes "Reserve docs/deployment-checks.md. Do not include private command output." \
  --silent)
bd dep add "$NETWORK_ISOLATION_TESTS" "$DEPLOY_PROXY_DOC"
bd dep add "$NETWORK_ISOLATION_TESTS" "$STATIC_REDACTION_GATE"
bd dep add "$NETWORK_ISOLATION_TESTS" "$BUNDLE_CHECK_ACCEPTANCE"

PUBLISH_READY_GATE=$(bd create "Gate static publish readiness" \
  -p 0 \
  --labels deploy-security,privacy,ui-mvp,tests \
  --description "Block publishing until static build passes redaction, runtime/private preview routes are no-store, auth/origin checks pass, browser no-persistence checks pass, and deployment rollback/restart commands are recorded." \
  --acceptance "Publish readiness checklist passes with links to sanitized evidence; no real-run data is present in static files, public docs, source maps, browser persistence, downloads, or preview caches." \
  --context "Source: 06-hosting-on-birb-homes.md Static Build Requirements and 13-deployment-security-checklist.md Static Publish Rules." \
  --notes "Reserve docs/publish-readiness.md." \
  --silent)
bd dep add "$PUBLISH_READY_GATE" "$NETWORK_ISOLATION_TESTS"
bd dep add "$PUBLISH_READY_GATE" "$SYNTH_UI_SMOKE"

DEPLOY_BIRB_HOMES=$(bd create "Deploy private UI through birb.homes" \
  -p 1 \
  --labels deploy-security,handoff \
  --description "Publish the sanitized static UI or docs and configure same-origin reverse proxy for runtime API/WebSocket if selected by Phase 0." \
  --acceptance "Mac on trusted network opens HTTPS UI, runtime API works without mixed-content errors, WebSocket connects over WSS, unrelated origins are rejected, and exact URL is recorded." \
  --context "Source: 06-hosting-on-birb-homes.md, 08-validation-runbook.md Hosting Validation, 09 Phase 7." \
  --notes "Reserve deployment notes only unless Phase 0 selected repo-local deployment config files." \
  --silent)
bd dep add "$DEPLOY_BIRB_HOMES" "$PUBLISH_READY_GATE"

# Phase 8 source: README.md Final Success Criteria, 08-validation-runbook.md, 09-implementation-sequence.md
OPERATOR_RUNBOOK=$(bd create "Write operator runbook and handoff docs" \
  -p 1 \
  --labels handoff,docs,privacy \
  --description "Document sanitized setup, config placeholders, start/stop, synthetic validation, real-host operation, capture labeling, verifier flow, deployment URL, restart/rollback, and remaining gaps." \
  --acceptance "Another engineer can run the bridge and validate a private capture session without reading source first; docs contain placeholders instead of private roots, tokens, screenshots, reports, or exact real capture ids." \
  --context "Source: 08-validation-runbook.md Acceptance Checklist and 09 Phase 8." \
  --notes "Reserve README updates, docs/operator-runbook.md, docs/handoff.md." \
  --silent)
bd dep add "$OPERATOR_RUNBOOK" "$DEPLOY_BIRB_HOMES"

FINAL_ACCEPTANCE=$(bd create "Complete final acceptance review" \
  -p 0 \
  --labels handoff,docs,tests,privacy \
  --description "Review all synthetic, real-host, verifier, deployment, privacy, and documentation acceptance criteria before marking the project ready for handoff." \
  --acceptance "Synthetic backend suite passes; UI synthetic smoke passes; real backend smoke and real capture smoke are complete or explicitly blocked; private bundle validation status is recorded; redaction gate passes; deployment and handoff docs are complete." \
  --context "Source: README.md Final Success Criteria, 08-validation-runbook.md Acceptance Checklist, 09-implementation-sequence.md Phase 8." \
  --notes "Reserve final sanitized implementation summary and known limitations only." \
  --silent)
bd dep add "$FINAL_ACCEPTANCE" "$OPERATOR_RUNBOOK"
bd dep add "$FINAL_ACCEPTANCE" "$REAL_CAPTURE_SMOKE"
bd dep add "$FINAL_ACCEPTANCE" "$BUNDLE_CHECK_ACCEPTANCE"

echo ""
echo "Bead graph created."
echo ""
echo "Suggested verification commands:"
echo "  bd ready"
echo "  bd list --label phase0-discovery"
echo "  bd list --label deploy-security"
echo "  bd list --label privacy"
echo "  bd list --label tests"
echo "  bd deps $CONTRACT_FREEZE"
echo "  bd deps $FINAL_ACCEPTANCE"
