# Synthetic UI Smoke

This smoke validates the browser operator flow against the synthetic backend. It
does not count as real Phase 4 acceptance and must not be used as proof that a
real private capture exporter is available.

## Automated Smoke

Run the focused UI smoke from the repository root:

```bash
cd ui
npm test -- --run tests/synthetic-smoke/syntheticOperatorSmoke.test.ts
```

The smoke mounts the operator UI with the synthetic runtime contract and covers:

| Requirement | Automated evidence |
| --- | --- |
| Connection | Starts from the locked form, submits an operator credential, enters `session-001`, and opens runtime sockets. |
| Keyboard input | Focuses the visible `A` pad button and activates it with Enter through the input socket. |
| Gamepad input | Polls a Standard Gamepad and sends its `A` button through the same combined input path. |
| Preview | Renders the advertised synthetic frame image URL for frame 42. |
| Stale preview | Applies a live stale `run_updated` event and verifies capture/input controls are blocked. |
| Capture retry | Rejects the first capture request with a retryable `capture_failed` error, then retries and completes `capture-001`. |
| Label conflict | Submits a rejected label for a goal-positive capture, receives a sanitized `label_conflict`, and verifies private paths stay out of the DOM. |
| Reconnect | Triggers the event socket reconnect callback, verifies the recovery state, and refreshes back to a fresh run state. |
| Auth failure redaction | Exercises failed startup with an unsafe auth error and verifies private paths and credentials are not rendered. |
| Clean stop | Stops the active synthetic session and returns to the locked start form. |

Private temp output inspection is covered by the service synthetic capture/label
integration test:

```bash
cargo test --manifest-path service/Cargo.toml --test capture synthetic_capture_labels_round_trip_private_files_and_event_refreshes
```

That test creates a private temp root, completes synthetic captures, verifies the
durable `captures/recent-captures.json` artifact, writes a private label draft
with a private note, and confirms public responses and event payloads remain
sanitized.

The full project gate runs both the smoke and private artifact checks:

```bash
bash scripts/quality-gate.sh
```

## Manual Mac-Browser Smoke

Use this when a Mac browser and the local development service are available.
Keep all private roots and credentials out of screenshots, public notes, and
browser storage.

1. Start the service with a dedicated private temp root and synthetic backend
   configuration. Use a temp directory owned by the operator account.
2. Start the UI from `ui/` with `npm run dev` and open it from the allowed
   browser origin.
3. Submit the operator credential and confirm the UI enters a synthetic running
   session.
4. Focus the input surface, press a mapped keyboard key, and confirm the pressed
   button and in-memory padlog tail update.
5. Connect or emulate a Standard Gamepad, press one mapped button, and confirm
   the combined input state updates without browser persistence.
6. Confirm the current preview image loads and the frame status is fresh.
7. Force or wait for a stale preview state, then verify input and capture
   controls disable until a fresh run update arrives.
8. Trigger capture. If the first capture fails as retryable, use the Retry
   affordance and confirm a completed synthetic capture appears in review.
9. Open the label drawer, write a draft label, then intentionally submit a
   conflicting role and confirm the conflict text is sanitized.
10. Disconnect and reconnect the event socket or restart the UI while the
    session remains active. Confirm the UI refreshes runtime state and clears
    transient pressed input.
11. Stop the session and confirm the locked form returns.
12. Inspect the private temp root on the service host. Confirm synthetic capture
    and label artifacts exist under the private root, and confirm no raw private
    paths, credentials, screenshots, or notes appear in the browser DOM, static
    UI output, downloads, localStorage, sessionStorage, IndexedDB, service
    workers, or Cache API.

Record only sanitized outcomes: command names, test pass/fail status, public
synthetic capture ids, and whether private artifacts existed. Do not record the
operator credential, absolute private root, raw screenshots, feature bytes, or
private label notes.
