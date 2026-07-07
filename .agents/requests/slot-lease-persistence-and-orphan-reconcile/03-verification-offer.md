# Choreography And Handback

## With The Hypervisor Round-3 (The Other Half Of The Cluster)

Their `lease-semantics-doc-and-orphan-slot-warn/` delivers: the
accurate lease-semantics doc this repo's reconcile design reads
(StaleLease behavior, no-expiry-today, validation rules), the WARN that
makes future leaks loud, and the activation decision (TTL / disconnect
hook / admin RPC / defer). Two-way coupling, stated on both sides:

- **This request fed theirs at filing**: the window-2 requirement
  (destroy-by-slot-id for dangling intents, narrowly scoped) is
  already delivered — `06-bridge-requirement.md` in their round-3
  dir — so no ordering race exists against their decision. If their
  ruling somehow predates a needed revision, request a revisit note.
- **Theirs feeds this**: if the admin/reconcile RPC is approved and
  lands, item 2's reconcile grows a second pass (destroy slots the
  worker holds that the bridge has no record of). Not gated on it —
  the persisted-lease pass ships regardless.
- Whichever request resolves first notes it in the other's dir.

## Phases-Track Verification

1. Mock-worker matrix re-run from a clean checkout.
2. Redaction audit: no token material in git or sanitized logs
   (static gate + a grep of the reconcile summary output).
3. The live verification record's internal consistency (ListSlots
   before/after, no worker restart in the interval), and `72o`'s close
   reason cites it.

## Handback Shape

Append `04-resolution.md` (commits, the destroy-vs-re-adopt decision,
crash-window doc, matrix evidence, live verification record, `72o`
disposition, the caveat-retirement list); we respond with
`05-verification.md`.

## Contact / Tracking

- Bead: `rom-operator-bridge-72o` (this request closes it).
- Sibling half: determinism-hypervisor
  `lease-semantics-doc-and-orphan-slot-warn/` (umay/w1v/decision).
- Provenance: hypervisor
  `requests/rom-bridge-getframebuffer-region-contract/04-related-slot-leak.md`;
  the 2026-07-01 four-orphan incident.
- Calendar note: item 5's restart window should ride an already-booked
  window (refwork cutover / eqb / snapstore session) rather than
  claiming a new one.
