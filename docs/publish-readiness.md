# Static Publish Readiness

Date: 2026-06-26

This checklist gates publishing the ROM operator bridge static UI. It records
only sanitized evidence. Private cookie files, raw headers, response bodies,
network evidence, session secrets, private roots, and capture artifacts
must remain outside this repository.

## Status

Static publish readiness is passing for the currently validated deployment
shape at `https://rombridge.birb.homes/`.

Sanitized evidence label:

```text
deployment-network-kut/20260626T212016Z
```

This label is not a filesystem path. Do not replace it with a concrete private
path in public docs, commits, chat, or bead notes.

## Checklist

| Gate | Status | Evidence |
| --- | --- | --- |
| Static build exists | PASS | `scripts/redaction-gate.sh` rebuilt `ui/dist/` before scanning. |
| Static redaction | PASS | `scripts/redaction-gate.sh` passed with the operator-private forbid file for `deployment-network-kut/20260626T212016Z`. |
| Deployed static-root scan | PASS | `scripts/deployment-network-check.sh` passed static publish root checks for no symlinks, no source maps, no mixed-content runtime endpoints, and no forbidden literals. |
| Runtime no-store | PASS | `scripts/deployment-network-check.sh` passed the runtime GET/POST no-store matrix for session, run, validation, frame, image, capture, and pause routes. |
| Private preview routes | PASS | Deployment checks covered frame metadata and image routes as private runtime surfaces with no-store headers. |
| Auth rejection | PASS | Deployment checks proved unauthenticated runtime requests are rejected with sanitized responses. |
| Origin rejection | PASS | Deployment checks proved absent, `null`, and unrelated Origins are rejected with a valid session cookie. |
| WebSocket auth/origin | PASS | Deployment checks proved `/ws/events` and `/ws/input` enforce authenticated same-origin handshakes. |
| Browser no-persistence | PASS | `ui/tests/privacy.test.ts` and related UI tests cover no service worker, no local/session storage, no IndexedDB, no Cache API, no downloads, no preview-cache APIs, and `allow_persistence: false`. |
| Source maps absent | PASS | `ui/tests/privacy.test.ts` verifies no `.map` files in deployable output, and `ui/vite.config.ts` sets `sourcemap: false`. |
| Static security headers | PASS | `ui/tests/securityHeaders.test.ts`, `ui/vite.config.ts`, `docs/deployment-note.md`, and `docs/deployment-security-shape.md` cover no-store, CSP, referrer, frame, and nosniff expectations. |
| Restart and rollback recorded | PASS | `docs/deployment-note.md`, `deploy/README.md`, and `deploy/operator-kut-deployment-runbook.md` record restart, emergency stop, proxy rollback, static release rollback, and private env restore commands. |

## Publish Rule

Publishing is allowed only for the static release directory covered by the
passing evidence above. Any new static build, runtime deployment change, route
change, private env change, or redaction-rule change must rerun:

```sh
scripts/deployment-network-check.sh
bash scripts/redaction-gate.sh
```

For deployment runs, `scripts/deployment-network-check.sh` must receive private
cookie, bind/network, outside-probe, static-root, and forbid-file inputs, and
`scripts/redaction-gate.sh` must run with
`ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1`. The concrete paths and generated
reports are private evidence and must not be copied into public handoff text.

## Public Surface Boundaries

The static UI and public docs must not contain:

- real ROM bytes, screenshots, raw framebuffer payloads, or decoded feature
  values;
- cookies, session secrets, token-shaped values, or
  private endpoint addresses;
- absolute private filesystem paths, private validation report excerpts, raw
  command transcripts, or private log output;
- real capture ids, preview caches, downloaded artifacts, browser persistence,
  service workers, source maps, `http://` runtime links, or `ws://` runtime
  links.

## References

- `docs/deployment-checks.md`
- `docs/deployment-note.md`
- `docs/deployment-security-shape.md`
- `docs/redaction.md`
- `docs/synthetic-smoke.md`
- `ui/tests/privacy.test.ts`
- `ui/tests/securityHeaders.test.ts`
- `scripts/deployment-network-check.sh`
- `scripts/redaction-gate.sh`
