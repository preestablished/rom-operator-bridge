# Project Planning with Beads

## Agent Instructions

You are an expert software architect creating a comprehensive task breakdown. This task graph will be executed by AI agents working in parallel, coordinated through MCP Agent Mail with file reservations to prevent conflicts.

<quality_expectations>
Create a thorough, production-ready task graph. Include all necessary setup, implementation, testing, and documentation tasks. Go beyond the basics - consider edge cases, error handling, security considerations, and integration points. Each task should be specific enough for an agent to execute independently without ambiguity.
</quality_expectations>

## Project Information

### Links to Relevant Documentation
<initial-plan-dir>/
<determinism-plan-dir>/

### Project Description
Build a private ROM operator bridge that supports the determinism project by letting a user on a macOS machine operate a ROM running on a Linux hypervisor host. The Mac should only need trusted-network access, a browser, keyboard input, and optionally a gamepad. The Linux host remains the source of truth for the ROM, hypervisor worker, deterministic frame-boundary input, framebuffer/capture path, private corpus root, labels, validation reports, and verifier commands.

The bridge should provide a browser-based operator UI and a host-side control service. The UI must let an operator start or attach to a private session, send keyboard/gamepad input using the stable `console16-12btn-v1` pad layout, observe current run status and framebuffer previews, trigger Phase 3/Phase 4 captures, review recent capture metadata, draft labels for first-boss, goal-positive, goal-negative, and dedup candidates, and see sanitized validation status. Private ROM bytes, screenshots, raw captures, save RAM, decoded feature values, exact private corpus paths, and validation reports must never be exposed in static hosted files or browser persistence. No real-run data may be written to localStorage, IndexedDB, service workers, browser downloads, preview caches, source maps with private local paths, static docs, public command transcripts, or validation excerpts.

The implementation should follow the existing initial planning material in `<initial-plan-dir>/`, including its hard Phase 0 discovery and contract-freeze gate. Before service or UI implementation, inspect the real `~/git/preestablished/` checkouts to confirm the authoritative integration points for padlog parsing, `console16-12btn-v1`, hypervisor input injection, frame counters, framebuffer access, capture/export APIs, existing control-plane contracts, verifier commands, redaction scanners, deployment conventions, and test commands.

Read `<initial-plan-dir>/README.md` first and follow its listed reading order. In shell comments, map each phase or bead group to the source plan file(s), and ensure the validation and acceptance material from `08-validation-runbook.md` and `09-implementation-sequence.md` is represented. Implementation/planning agents may use only local project docs, live checked-out repos, public platform/language/hardware docs, and operator-supplied artifacts. They must not consult third-party deterministic-testing platforms, external case studies, proprietary SDKs/APIs, or non-public implementation notes; file a docs gap instead.

The task graph must encode Phase 0 as a hard dependency gate. Create a Phase 0 discovery bead and a contract-freeze acceptance bead. Every service, UI, real-backend, capture, verifier, deployment, handoff, and implementation docs bead must depend on the freeze bead. Only probe and note-writing tasks may be ready before `bridge-discovery-note.md` exists and answers the contracts from `10-phase0-discovery-and-contract-freeze.md`.

### Technical Stack
Use Rust where appropriate and TypeScript where not. Decide the service and UI stack based on the live repository conventions under `/home/infra-admin/git/preestablished/`. Prefer existing patterns, build tools, test frameworks, deployment scripts, formatting conventions, and shared contracts from those repositories over introducing new architecture.

Expected direction unless discovery shows a better local convention:

- Rust for the host-side bridge service, backend traits, API contracts, input scheduling, private file writers, validation command wrappers, and integration with hypervisor/control-plane code.
- TypeScript for the browser operator UI, API client, WebSocket clients, state modeling, input mapping, and UI tests.
- Synthetic backend first, with fake frame counter, generated framebuffer, padlog writer, fake capture ids, in-memory status, temp private root, and tests that keep service/UI work unblocked before real hypervisor attachment.
- Real backend later, implementing the same service-facing abstraction as the synthetic backend.
- HTTP plus WebSocket runtime API following `<initial-plan-dir>/11-runtime-api-contract.md`.
- Browser input and operator flow following `<initial-plan-dir>/12-browser-input-and-operator-flow.md`.
- Private deployment and security checks following `<initial-plan-dir>/13-deployment-security-checklist.md`.
- Mandatory verifier runbook/status integration. A service-side validation runner is optional and should be created only if Phase 0 chooses it. Both paths must keep private reports private and expose only sanitized status.

### Specific Requirements
- The graph must include blocking security beads for HTTP and WebSocket auth, no credentials in URLs, `HttpOnly Secure SameSite=Strict` cookie auth or a documented HTTPS-only header alternative, 4-hour session TTL, one active operator session in the MVP, Origin/CORS allowlisting, auth rate limiting, sanitized auth errors, restrictive private file permissions (`0700` directories, `0600` files), and rejection of private roots that are world-writable or inside static publish directories.
- Deployment beads must block publish until reverse-proxy/TLS shape is documented, runtime binds only localhost or a documented trusted interface, unauthenticated and unrelated-origin requests are rejected, all runtime/private preview routes return `Cache-Control: no-store`, SPA headers include CSP/referrer/frame protections, redaction scan passes, and rollback/restart commands are recorded.
- Test beads must cover synthetic no-ROM integration, all-button `console16-12btn-v1` mapping, reserved bits always zero, padlog parser round trip, duplicate `client_seq` and idempotency handling, focus/page-hidden/reconnect/gamepad-disconnect zero-input behavior, stale preview disabling controls, capture retry, label conflict/schema rejection, browser no-persistence expectations, static redaction scan, and auth failure redaction.
- A real capture job may be marked `completed` only after the private capture index row and required payload references are durable. If Phase 0 finds no real exporter, real capture work must be marked blocked or deferred; synthetic capture must not be represented as Phase 4 acceptance.

---

## Your Task

Analyze this project and create a comprehensive **Beads task graph** using the `bd` CLI. Beads provides dependency-aware, conflict-free task management for multi-agent execution.

---

<critical_constraint>
Your ONLY output is a bash shell script that creates a Beads task graph. Do NOT use `bd add` - the correct command to create a bead is `bd create`. Use `bd dep add` for dependencies. Do not implement anything yourself. The script may run only `bd init`, `bd create`, `bd dep add`, and harmless `echo` commands. It must not run tests, curls, publish scripts, verifier commands, deployment commands, read private files, or write implementation files.
</critical_constraint>

## Output Format

Generate a shell script that creates the full task graph. The script should:

1. **Initialize Beads** (if not already initialized)
2. **Create all beads** with appropriate priorities
3. **Establish dependencies** between beads
4. **Add labels** for phase grouping
5. **End with echoed verification commands** for the user to run after the graph is created

### Example Output

```bash
#!/bin/bash
# Project: rom-operator-bridge
# Generated: 2026-06-23

set -e

# Initialize beads if needed
if [ ! -d ".beads" ]; then
    bd init
fi

echo "Creating project beads..."

# Source: plans/initial/10-phase0-discovery-and-contract-freeze.md
PHASE0_DISCOVERY=$(bd create "Phase 0: locate bridge integration points" \
  -p 0 \
  --labels phase0-discovery,docs \
  --description "Inspect reference-workload, determinism-hypervisor, control-plane, and related local checkouts without changing implementation code." \
  --acceptance "Checkout paths, commit ids, padlog parser, hypervisor input API, framebuffer source, capture/export mechanism, verifier commands, and deployment constraints are recorded." \
  --context "Use <initial-plan-dir>/10-phase0-discovery-and-contract-freeze.md." \
  --notes "Reserve only discovery notes and read-only inspection surfaces." \
  --silent)

CONTRACT_FREEZE=$(bd create "Freeze ROM bridge implementation contracts" \
  -p 0 \
  --labels phase0-discovery,docs \
  --description "Write bridge-discovery-note.md and freeze deviations from the runtime API, browser input flow, and deployment/security checklist." \
  --acceptance "bridge-discovery-note.md exists and answers every required contract or marks blockers explicitly." \
  --context "No service, UI, real-backend, capture, verifier, deployment, or implementation-docs bead may be ready before this bead completes." \
  --notes "Only probe/note work may precede this gate." \
  --silent)
bd dep add $CONTRACT_FREEZE $PHASE0_DISCOVERY

# Source: plans/initial/11-runtime-api-contract.md
API_CONTRACT=$(bd create "Define typed bridge runtime API and backend interface" \
  -p 0 \
  --labels service-synthetic,ui-mvp,docs \
  --description "Codify the HTTP/WebSocket route schemas, common error envelope, event model, and synthetic/real backend interface selected in Phase 0." \
  --acceptance "Service and UI can share generated or manually synchronized types; deviations from the plan are documented." \
  --context "This is the parallelization point for synthetic service and typed UI client work." \
  --notes "Reserve API contract files and generated type surfaces." \
  --silent)
bd dep add $API_CONTRACT $CONTRACT_FREEZE

SYNTH_SERVICE=$(bd create "Implement synthetic bridge service skeleton" \
  -p 0 \
  --labels service-synthetic,tests \
  --description "Create the Rust service skeleton, config loading, health/session/status routes, event stream, synthetic frame counter, private temp root, and auth/origin middleware." \
  --acceptance "Synthetic service starts locally; /health works; synthetic session can start/stop; private output is created only under configured roots." \
  --context "Use the same backend interface that the real hypervisor backend will implement later." \
  --notes "Reserve service source, service tests, and synthetic backend files." \
  --silent)
bd dep add $SYNTH_SERVICE $API_CONTRACT

UI_CLIENT=$(bd create "Implement typed TypeScript UI client and state model" \
  -p 0 \
  --labels ui-mvp,tests \
  --description "Build the browser API/WebSocket client, typed state model, and session connection flow against the synthetic backend contract." \
  --acceptance "UI tests cover connection, auth failure, event updates, and sanitized error handling." \
  --context "May run in parallel with synthetic service implementation after API contract freeze." \
  --notes "Reserve UI client, state model, and related tests." \
  --silent)
bd dep add $UI_CLIENT $API_CONTRACT

# ... continue for all phases ...

echo ""
echo "Bead graph created! View with:"
echo "  bd ready              # List unblocked tasks"
echo "  bd list --label phase0-discovery"
```

---

## Bead Creation Guidelines

### Priority Levels
- `-p 0` = Critical (blocking other work)
- `-p 1` = High (important but not blocking)
- `-p 2` = Medium (standard work)
- `-p 3` = Low (nice to have)

### Labels (Phase Grouping)
Use `--labels` to group beads by phase. Use comma-separated labels when a bead belongs to multiple groups:
- `phase0-discovery` - checkout inspection, real contract discovery, and contract freeze
- `service-synthetic` - Rust service skeleton and synthetic backend
- `input-padlog` - input mapping, scheduling, acknowledgements, and padlog writer/parser work
- `ui-mvp` - TypeScript browser operator UI and API/WebSocket client
- `real-backend` - real hypervisor/control-plane attachment
- `capture-labels` - capture job lifecycle, recent captures, label drafts, and conflicts
- `verifier` - verifier runbook, score-plan/trace transformation, and sanitized validation status
- `deploy-security` - birb.homes proxy/TLS, auth/origin/no-cache checks, redaction gate, and rollback
- `handoff` - operator docs, smoke runbooks, deployment notes, and final handoff
- `tests` - unit, integration, UI, synthetic, real-host smoke, and redaction tests
- `privacy` - private data handling, file permissions, no-persistence checks, and sanitized errors
- `docs` - architecture, API, discovery, deployment, and context documentation

Canonical bead shape:

```bash
TASK_ID=$(bd create "Short imperative task title" \
  -p 1 \
  --labels phase-label,tests \
  --description "Concrete work the agent should perform." \
  --acceptance "Observable completion criteria." \
  --context "Relevant source plan files, APIs, or local paths." \
  --notes "File reservation guidance and coordination notes." \
  --silent)
```

### Dependency Rules
1. Never create cycles
2. Every bead should have a clear dependency chain back to Phase 0, the API contract, or the relevant phase parent
3. Use `bd dep add CHILD PARENT` (child depends on parent completing first)
4. Parallel work should share a common ancestor, not depend on each other
5. The Phase 0 contract-freeze bead is the common ancestor for all implementation work
6. After Phase 0, create an API-contract/service-interface bead before service/UI implementation
7. Synthetic service and typed UI client work may run in parallel after the API contract
8. Capture/labeling depends on synthetic input plus framebuffer preview
9. Real backend work depends on synthetic backend and padlog parser round-trip tests
10. Deployment depends on auth/origin/no-cache/privacy tests and the static redaction gate

### Task Granularity
- Each bead should be completable in **under 750 lines of code**
- Tasks should be atomic enough for one agent to complete without coordination
- If a task requires multiple file areas, consider splitting by file area

---

## File Reservation Planning

For each major work area, note the file patterns that will need exclusive reservation:

```bash
# Example reservation notes (add as bead descriptions)
# Auth work: src/auth/**, tests/auth/**, src/hooks/useAuth*
# API client: src/api/**, src/lib/fetch*, tests/api/**
# UI components: src/components/{ComponentName}/**, tests/components/{ComponentName}/**
```

This helps agents claim appropriate file surfaces when they start work.

---

## Context Documentation

Place any important context in `prompts/docs/` for agents to reference. This includes:
- Architecture decisions
- API documentation
- Design system specs
- External service integration guides

---

## Verification Steps

The generated script must not run these commands. It should echo them for the user to run after saving the script:

1. **Run it**: `chmod +x setup-beads.sh && ./setup-beads.sh`
2. **Check ready work**: `bd ready` should show the initial Phase 0 discovery tasks

---

## Completeness Checklist

Ensure your task graph includes:

- [ ] All setup and configuration tasks
- [ ] Core architecture and shared utilities
- [ ] Feature implementation tasks (broken into small units)
- [ ] Error handling and edge cases
- [ ] Unit and integration tests for each feature
- [ ] API documentation
- [ ] Security considerations (input validation, auth checks)
- [ ] Performance considerations where relevant
- [ ] CI/CD and deployment tasks
- [ ] Clear dependency chains with no cycles
- [ ] Phase 0 discovery and contract-freeze gate blocks all implementation beads
- [ ] Source-plan comments map bead groups to the initial plan files
- [ ] Auth, origin, no-cache, private-permission, no-persistence, and redaction gates are separate beads
- [ ] Synthetic backend acceptance cannot be confused with real Phase 4 capture acceptance
