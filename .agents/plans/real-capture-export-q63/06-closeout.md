# Closeout

## 1. Completion Audit

Before closing `rom-operator-bridge-q63`, verify every acceptance criterion with
current evidence:

- real capture returns sanitized job metadata;
- private artifacts are durable before completed status;
- `captures/index.jsonl` has the confirmed schema;
- no raw captures, screenshots, feature bytes, private paths, refs, or worker
  endpoints leak to UI/API/websocket/bead notes;
- tests cover worker success, worker failure, private write failure, idempotency,
  and labelability.

If any evidence is missing or indirect, leave `q63` open and append a sanitized
blocker.

## 2. Bead Updates

On success:

```bash
bd update rom-operator-bridge-q63 --append-notes "<sanitized summary>"
bd close rom-operator-bridge-q63 --reason "Real capture export integration writes durable private capture artifacts and sanitized public job metadata"
```

Then check what became ready:

```bash
bd ready
bd show rom-operator-bridge-r77
```

If `r77` is still deferred only because private host/operator data is required,
append a sanitized note saying `q63` is no longer its code blocker. Do not
undefer or close `r77` unless its owner/operator prerequisites are satisfied.

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

If the real exporter or schema is still absent:

```text
q63 remains blocked on real capture exporter/schema availability.

Observed:
- real backend lifecycle available: yes;
- durable capture writer support: <pass/fail>;
- hypervisor capture RPC contract: <available/missing/insufficient>;
- reference workload captures/index.jsonl schema: <available/missing>;
- private artifact policy: <available/missing>.

Next unblock step:
- <single concrete component/request>
```

Create or update a narrow request under
`~/.agents/projects/<repo-name>/requests/` only if another repo must supply the
missing contract. Keep the request sanitized.

## 5. Repository Close Protocol

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
