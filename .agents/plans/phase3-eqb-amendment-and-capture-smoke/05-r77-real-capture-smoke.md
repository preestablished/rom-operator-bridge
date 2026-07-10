# R77 Real Capture And Label Smoke

## 1. Entry Gate And Ownership

This smoke is `r77`; do not create a shadow bead. Proceed only when the
operator explicitly grants the private host/network/data window and the live
preflight proves a real non-blank frame. Then undefer and atomically claim
`r77`: inspect it, use the installed `bd` help to clear deferral/open it, then
`bd update rom-operator-bridge-r77 --claim`. Append a sanitized note before
close or re-deferral. If authorization expires, begin no new mutation, perform
only authorized cleanup, and defer it again with a sanitized reason.

The smoke may share a scheduled window with `eqb` and reference-workload work,
but run it sequentially. Never overlap it with the snapshot-store 1000x session
on the same host.

## 2. Execute One Capture Through Q63

Use the UI for the operator-visible path while storing machine checks privately:

1. Start a real session requesting `capture` and `labels`; assert both returned
   capabilities are true.
2. Verify the current frame is real and non-blank without committing or
   screenshotting it.
3. Trigger exactly one capture with a fresh idempotency key, persist the
   key/job/capture mapping privately, and poll the job to `completed`; require
   `labelable=true`. On an ambiguous response, retry only the same key.
4. Capture the returned public runtime capture id privately. Search
   `<private-root>/captures/index.jsonl` structurally for exactly one matching
   row without printing the row.
5. Verify the row's referenced private payloads exist, are non-empty, and match
   their recorded hashes/lengths. Confirm the sanitized capability/provenance
   state is truthful.
6. Submit one `needs_review` upsert through `/api/labels` using a fresh
   idempotency key. Require `applied=true` and privately verify the capture's
   `label-draft.json` reflects it without printing the file.
7. Stop the session and assert the service releases the session/slot cleanly.

Do not use a synthetic capture or a raw hypervisor-only capture: acceptance is
specifically the bridge `q63` export path plus bridge label draft.

## 3. Private Assertions And Failure Handling

The private run record should contain command statuses and booleans for:

- real backend and non-blank frame;
- trigger/job completion and idempotency identity;
- exactly one index row resolving by capture id;
- payload existence and hash/length agreement;
- `needs_review` label draft existence;
- clean stop and slot release;
- forbidden-literal and redaction scans.

On failure, attempt a clean stop first. Preserve raw evidence privately and
publish only one specific residual class (for example capture RPC, index
durability, label draft, clean stop, or redaction). Keep `r77` open/deferred as
appropriate; do not close it on a partial capture. Once capture durability
succeeds, resume label/stop work against that same capture—never retrigger with
a new key, delete/hand-edit the index, or create a second capture without
explicit operator authorization and disposition of the partial run.

Cleanup is complete only when the session/run is halted, the slot is released,
and service/worker readiness is green. Otherwise invoke the established o73
restore/escalation path and keep `r77` open.

## 4. Sanitized Record And Required Citations

The request's ID-verifiability requirement conflicts with the repository's
publish-blocking redaction rule for real capture ids. Resolve this with a
two-tier record unless the operator/privacy owner explicitly reclassifies the
id and updates the gate policy:

- private record: raw capture id, exact matching index row, exact payload and
  provenance hashes, and the alias mapping;
- public `docs/real-capture-smoke.md`, beads, and sibling note: date,
  public-safe builds, capabilities, a stable non-secret evidence alias,
  approved public-safe provenance hashes only, booleans for unique-row/id
  resolution, payload hash agreement, label, stop, and redaction.

An authorized reviewer must resolve alias to id privately and attest that the
unique row and exact hashes were verified. Record this as the privacy-preserving
interpretation of AC2 and obtain operator/phases-track sign-off; raw-ID
publication and the current green redaction gate cannot both be satisfied.

Mirror the same sanitized summary into
`../reference-workload/.agents/requests/phase4-real-capture-corpus-fast-follow/`
using that repository's own instructions and bead workflow. The note must say
this proves the contingency route; it does not claim corpus production.
Before editing, inspect its dirty tree and applicable parent instructions, run
`bd prime` if that repository has an active beads database, and attach the note
to its existing corpus work rather than creating new scope. Add the note only
after `r77` passes, run that repository's privacy/quality gates, and commit,
sync beads, and push it separately. Do not modify corpus artifacts.

Append the sanitized record reference to `r77` and `13h`. Close `r77` only when
all acceptance checks pass. Leave `13h` open because `0wo` and `opw` remain
outside this request.
