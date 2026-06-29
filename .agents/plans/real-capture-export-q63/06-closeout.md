# Closeout

## 1. Completion Audit

Before closing `rom-operator-bridge-q63`, verify every acceptance criterion with
current evidence:

- real capture returns sanitized job metadata;
- private artifacts are durable before completed status;
- `captures/index.jsonl` has the confirmed schema;
- decoded values match feature-map order and payload hashes/lengths match stored
  artifacts;
- real preview/features endpoints do not expose raw payloads unless an approved
  public derivative was implemented;
- no raw captures, screenshots, feature bytes, private paths, refs, or worker
  endpoints leak to UI/API/websocket/bead notes;
- tests cover worker success, worker failure, private write failure, idempotency,
  and labelability.

If any evidence is missing or indirect, leave `q63` open and append a sanitized
blocker.

## 2. Bead Updates

On success:

```bash
git status --short
git add <changed files>
git commit -m "Implement real capture export integration"
git pull --rebase
COMMIT="$(git rev-parse --short HEAD)"
bd update rom-operator-bridge-q63 --append-notes "<sanitized summary>"
bd close rom-operator-bridge-q63 --reason "Real capture export integration writes durable private capture artifacts and sanitized public job metadata" --suggest-next
bd dolt push
git push
git status --short --branch
```

Then check what became ready:

```bash
bd ready
bd show rom-operator-bridge-r77
```

If `r77` is still deferred only because private host/operator data is required,
append a sanitized note saying `q63` is no longer its code blocker. Do not
undefer or close `r77` unless its owner/operator prerequisites are satisfied.
The final status must show the branch with no ahead/behind state relative to
origin.

## 3. Sanitized Success Note Template

```text
Real capture export integration completed.

Bridge commit: <short sha>

Results:
- real backend capture capability enabled only with validated private capture config;
- trigger_capture calls the real hypervisor capture RPC for the active lease;
- private capture payloads are written under the private root;
- captures/index.jsonl is appended before completed status;
- public job metadata is sanitized;
- label draft flow works for completed real capture ids;
- public response/websocket/bead sweeps found no private values.

Tests:
- <focused commands>: pass
- <broad gates>: pass
```

Never include private paths, capture ids from private runs, refs, payload names
that encode private values, screenshots, raw worker errors, or raw JSON.

## 4. Failed Or Blocked Outcome

If the capture row contract, capture-spec resolver, or required private inputs
are still absent:

```text
q63 remains blocked on real capture row contract or private capture input availability.

Observed:
- real backend lifecycle available: yes;
- durable capture writer support: <pass/fail>;
- hypervisor capture RPC contract: <available/missing/insufficient>;
- reference workload captures/index.jsonl schema: <available/missing>;
- private artifact policy: <available/missing>.

Next unblock step:
- <single concrete component/request>
```

Apply the blocked/deferred state explicitly:

```bash
bd create --title="<sanitized missing q63 prerequisite>" --description="<why the prerequisite is needed, without private values>" --type=task --priority=1
bd dep add rom-operator-bridge-q63 <new-blocking-bead-id>
bd update rom-operator-bridge-q63 --status open --append-notes "<sanitized blocker summary>"
bd defer rom-operator-bridge-q63 --until="+14d"
bd dolt push
git status --short --branch
```

If another repo or operator handoff must supply the missing contract, create or
update an explicit bead dependency or human/private handoff bead. Create or
update a narrow request under `$PRIVATE_REQUEST_DIR` only as supporting context.
Keep the request sanitized and reference it from the bead. Do not commit or copy
concrete operator-private local paths into bead notes.

If any code, docs, plan files, or request files changed while reaching the
blocked outcome, run the applicable gates/sweeps for those changes, commit them,
and push both Git and bead state:

```bash
git status --short
git add <changed files>
git commit -m "<sanitized blocked q63 handoff>"
git pull --rebase
bd dolt push
git push
git status --short --branch
```

## 5. Repository Close Protocol

If code changed and `q63` is complete, the commit must already exist before the
success bead note is appended so the note can include the real commit SHA.
Follow `AGENTS.md` exactly:

```bash
git status --short
git add <changed files>
git commit -m "Implement real capture export integration"
git pull --rebase
bd dolt push
git push
git status --short --branch
```

Work is not complete until both bead data and git commits are pushed.
