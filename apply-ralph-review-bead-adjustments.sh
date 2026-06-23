#!/bin/bash
# Apply Ralph/Sonnet execution review adjustments to the live Beads graph.
# Generated: 2026-06-23

set -e

echo "Applying Ralph review adjustments to Beads graph..."

# The initial umbrella bead is redundant with the concrete Phase 0 discovery beads.
# Closing it makes the first Ralph ready set actionable.
bd close rom-operator-bridge-sl4 \
  --reason "Replaced by concrete Phase 0 discovery beads for Ralph execution."

# Ralph's UI pass looks for the exact label "ui", not "ui-mvp".
bd update \
  rom-operator-bridge-f6j \
  rom-operator-bridge-9gy \
  rom-operator-bridge-68r \
  rom-operator-bridge-bb3 \
  rom-operator-bridge-zsn \
  rom-operator-bridge-233 \
  rom-operator-bridge-u3d \
  rom-operator-bridge-c43 \
  rom-operator-bridge-5eo \
  rom-operator-bridge-im6 \
  rom-operator-bridge-8x4 \
  rom-operator-bridge-eqi \
  rom-operator-bridge-0i9 \
  rom-operator-bridge-b6v \
  rom-operator-bridge-ft8 \
  rom-operator-bridge-su8 \
  rom-operator-bridge-xek \
  --add-label ui

# Mark private/operator-host tasks so Ralph autopilot does not treat them as ordinary
# agent-runnable work until the operator undefer them with the required environment.
bd update \
  rom-operator-bridge-0wo \
  rom-operator-bridge-r77 \
  rom-operator-bridge-opw \
  rom-operator-bridge-kut \
  rom-operator-bridge-38v \
  --add-label human-private \
  --append-notes "Ralph review: this task requires private operator data, private host/network state, or deployment access. Keep deferred until those prerequisites are explicitly available."
bd defer \
  rom-operator-bridge-0wo \
  rom-operator-bridge-r77 \
  rom-operator-bridge-opw \
  rom-operator-bridge-kut \
  rom-operator-bridge-38v

# Correct capture/UI dependency inversion and keep optional privileged feature work
# out of the core synthetic capture/label path.
bd dep remove rom-operator-bridge-c43 rom-operator-bridge-u3d
bd dep remove rom-operator-bridge-b6v rom-operator-bridge-5eo
bd dep add rom-operator-bridge-b6v rom-operator-bridge-c43
bd dep add rom-operator-bridge-u3d rom-operator-bridge-c43
bd dep add rom-operator-bridge-u3d rom-operator-bridge-b6v
bd dep add rom-operator-bridge-5eo rom-operator-bridge-b6v

# Remove avoidable post-freeze serialization so Ralph can work independent branches.
bd dep remove rom-operator-bridge-7xo rom-operator-bridge-o4w
bd dep add rom-operator-bridge-7xo rom-operator-bridge-ft8

bd dep remove rom-operator-bridge-15d rom-operator-bridge-i9b
bd dep add rom-operator-bridge-15d rom-operator-bridge-7xo
bd dep add rom-operator-bridge-t32 rom-operator-bridge-rh4
bd dep add rom-operator-bridge-3ku rom-operator-bridge-i9b

bd dep remove rom-operator-bridge-9gy rom-operator-bridge-xek
bd dep add rom-operator-bridge-9gy rom-operator-bridge-ft8
bd dep remove rom-operator-bridge-bb3 rom-operator-bridge-3ku
bd dep add rom-operator-bridge-bb3 rom-operator-bridge-7xo

bd dep remove rom-operator-bridge-xsg rom-operator-bridge-3ku
bd dep add rom-operator-bridge-xsg rom-operator-bridge-o4w

bd dep remove rom-operator-bridge-bp8 rom-operator-bridge-3ku
bd dep add rom-operator-bridge-bp8 rom-operator-bridge-o4w
bd dep add rom-operator-bridge-bp8 rom-operator-bridge-ft8

bd dep remove rom-operator-bridge-0i9 rom-operator-bridge-3dr
bd dep add rom-operator-bridge-0i9 rom-operator-bridge-bp8
bd dep add rom-operator-bridge-0i9 rom-operator-bridge-su8
bd dep add rom-operator-bridge-0wo rom-operator-bridge-3dr

bd dep remove rom-operator-bridge-q63 rom-operator-bridge-0wo
bd dep add rom-operator-bridge-q63 rom-operator-bridge-bp8

bd dep remove rom-operator-bridge-gpb rom-operator-bridge-r77
bd dep add rom-operator-bridge-gpb rom-operator-bridge-m6x

bd dep remove rom-operator-bridge-kut rom-operator-bridge-opw

# Optional validation runner should not block bundle validation if Phase 0 chooses
# runbook-only validation.
bd dep remove rom-operator-bridge-opw rom-operator-bridge-r3z
bd dep add rom-operator-bridge-opw rom-operator-bridge-im6
bd dep add rom-operator-bridge-opw rom-operator-bridge-r77
bd update rom-operator-bridge-r3z \
  --append-notes "Ralph review: this is optional. Undefer only if Phase 0 chooses service-side validation runner automation."
bd defer rom-operator-bridge-r3z

# Clarify the contract freeze bead so a Sonnet-level agent does not silently
# self-approve unresolved operator decisions.
bd update rom-operator-bridge-8m5 \
  --append-notes "Ralph review: if bridge-discovery-note.md leaves any operator decision unresolved, do not self-approve this freeze; mark the relevant follow-up blocked or request operator input."

# Add smaller beads that reduce ambiguity and rework for Ralph iterations.
QUALITY_GATE=$(bd create "Define root Ralph quality gate command" \
  -p 0 \
  --labels tests,docs,handoff \
  --description "Define one documented verification command or script that Ralph agents can run after each branch, covering service tests, UI tests, build checks, and redaction/static checks as they become available." \
  --acceptance "docs or repo scripts name the exact VERIFY command; the command degrades clearly when service/UI scaffolds do not exist yet; later beads reference it instead of inventing local test commands." \
  --context "Review source: Ralph skill VERIFY step and Sonnet-level graph review. This should happen after service and UI scaffolds exist." \
  --notes "Reserve docs/quality-gate.md, scripts/quality-gate.*, package/Cargo test wiring as selected by Phase 0." \
  --silent)
bd dep add "$QUALITY_GATE" rom-operator-bridge-zok
bd dep add "$QUALITY_GATE" rom-operator-bridge-f6j
bd dep add rom-operator-bridge-3ku "$QUALITY_GATE"
bd dep add rom-operator-bridge-u3d "$QUALITY_GATE"
bd dep add rom-operator-bridge-b6v "$QUALITY_GATE"
bd dep add rom-operator-bridge-25u "$QUALITY_GATE"

SANITIZE_UTILS=$(bd create "Implement shared public sanitization utilities" \
  -p 0 \
  --labels service-synthetic,privacy,tests \
  --description "Create shared sanitization and public error helpers for browser-visible errors, events, capture metadata, validation summaries, and auth/input rejection payloads." \
  --acceptance "Tests reject absolute private paths, configured private roots, command output, feature bytes, raw payload snippets, validation report excerpts, and forbidden literals in public responses/events." \
  --context "Review source: multiple beads repeat sanitized-error requirements; central helpers reduce drift." \
  --notes "Reserve service sanitization/error modules and tests/sanitization/**." \
  --silent)
bd dep add "$SANITIZE_UTILS" rom-operator-bridge-zok
bd dep add "$SANITIZE_UTILS" rom-operator-bridge-ft8
bd dep add rom-operator-bridge-rh4 "$SANITIZE_UTILS"
bd dep add rom-operator-bridge-t32 "$SANITIZE_UTILS"
bd dep add rom-operator-bridge-xek "$SANITIZE_UTILS"
bd dep add rom-operator-bridge-xsg "$SANITIZE_UTILS"
bd dep add rom-operator-bridge-im6 "$SANITIZE_UTILS"

PRIVATE_ARTIFACT_SCHEMAS=$(bd create "Define private run artifact schemas and writers" \
  -p 0 \
  --labels service-synthetic,capture-labels,verifier,privacy,tests \
  --description "Define schemas and append-only writer behavior for run manifest, bridge events JSONL, input rejection log, recent captures, label draft, validation runs, and file-mode expectations." \
  --acceptance "Tests cover schema_version, append-only event rows, 0700/0600 creation expectations, atomic/durable writes where needed, and no public exposure of private references." \
  --context "Review source: session, capture, label, validation, and privacy beads all need consistent private artifact behavior." \
  --notes "Reserve service artifact schema/writer modules and tests/artifacts/**." \
  --silent)
bd dep add "$PRIVATE_ARTIFACT_SCHEMAS" rom-operator-bridge-121
bd dep add "$PRIVATE_ARTIFACT_SCHEMAS" rom-operator-bridge-ft8
bd dep add rom-operator-bridge-o4w "$PRIVATE_ARTIFACT_SCHEMAS"
bd dep add rom-operator-bridge-xsg "$PRIVATE_ARTIFACT_SCHEMAS"
bd dep add rom-operator-bridge-m6x "$PRIVATE_ARTIFACT_SCHEMAS"
bd dep add rom-operator-bridge-im6 "$PRIVATE_ARTIFACT_SCHEMAS"

UI_AUTH_SCREEN=$(bd create "Implement UI auth and session screen" \
  -p 1 \
  --labels ui,ui-mvp,privacy,tests \
  --description "Implement the locked/session screen, credential submission, active/expired/logout/session-active-elsewhere flows, and no browser persistence for credentials." \
  --acceptance "UI tests cover successful login, auth_rejected, expired session, logout, session_active_elsewhere, no credentials in URLs, and no localStorage/IndexedDB persistence." \
  --context "Review source: Ralph/Sonnet graph review; this is a smaller prerequisite between typed UI client and play surface." \
  --notes "Reserve UI auth/session components and tests/ui-auth/**." \
  --silent)
bd dep add "$UI_AUTH_SCREEN" rom-operator-bridge-9gy
bd dep add rom-operator-bridge-68r "$UI_AUTH_SCREEN"

REAL_BACKEND_GATE=$(bd create "Record real backend availability decision" \
  -p 0 \
  --labels phase0-discovery,real-backend,capture-labels,docs \
  --description "After Phase 0 freeze, record whether real backend and real capture exporter work is available now, deferred, or blocked, and update/defer downstream real-host beads accordingly." \
  --acceptance "If real backend or exporter is unavailable, the corresponding real-backend/capture smoke and integration beads are explicitly deferred or blocked instead of inviting synthetic fake acceptance." \
  --context "Review source: real backend and real capture tasks require private host/API availability." \
  --notes "Reserve bridge-discovery-note.md follow-up section or docs/real-backend-availability.md." \
  --silent)
bd dep add "$REAL_BACKEND_GATE" rom-operator-bridge-8m5
bd dep add rom-operator-bridge-bp8 "$REAL_BACKEND_GATE"
bd dep add rom-operator-bridge-q63 "$REAL_BACKEND_GATE"

bd dep add rom-operator-bridge-13h rom-operator-bridge-0wo

echo ""
echo "Ralph review adjustments applied."
echo "Next useful checks:"
echo "  bd ready"
echo "  bd dep cycles"
echo "  bd list --label ui"
echo "  bd list --label human-private"
