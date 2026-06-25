# Deployment Network Isolation Checks

Date: 2026-06-25

This document records sanitized deployment-network verification for
`rom-operator-bridge-kut`. Raw command output is stored only in an
operator-private validation directory outside this repository.

## Status

`rom-operator-bridge-kut` is still blocked.

The deployment hostname resolves and a private cookie-source file is available,
but the current deployment route does not yet provide trusted TLS or serve the
bridge runtime surfaces. The bridge service is also not active on the expected
host service path, so Origin, no-store, WSS, mixed-content, and outside-network
isolation checks cannot pass yet.

Additional host inspection found no bridge-specific K3s ingress, service, or
certificate resource for `rombridge.birb.homes`, and no installed
`rom-operator-bridge` systemd unit. The active blocker is missing deployment
prerequisites, not just a failing network-isolation assertion.

## Evidence Boundary

Raw evidence label:

```text
private evidence: deployment-network-kut/20260625T181209Z
```

Private request label:

```text
private request: rom-operator-bridge/kut-deployment-route-prerequisites
```

This label identifies private evidence outside the repository. It is not a
filesystem path and must not be replaced with a concrete private path in public
docs, commits, chat, or bead notes.

## Environment

| Field | Sanitized Value |
|---|---|
| Deployment origin | `https://rombridge.birb.homes/` |
| Runtime API | `https://rombridge.birb.homes/api/...` |
| WebSocket routes | `wss://rombridge.birb.homes/ws/events`, `wss://rombridge.birb.homes/ws/input` |
| Expected service bind | `<bridge-private-ip>:7410` or documented loopback proxy topology |
| Evidence directory | `<private-validation-dir>` |

## Results

| Check | Result | Sanitized Evidence |
|---|---|---|
| Private cookie source | PASS | A private `0600` curl header config exists for authenticated probes. |
| DNS resolution | PASS | `rombridge.birb.homes` resolves on this host. |
| Trusted TLS | FAIL | Curl certificate verification fails for the deployment origin. |
| Service bind | FAIL | No expected active bridge service/listener was proven for port `7410`. |
| Health sanitization | FAIL | `/health` could not be verified through the trusted deployment route. |
| Unauthenticated rejection | FAIL | Runtime auth rejection could not be verified through the trusted deployment route. |
| Origin rejection | FAIL | Absent, `null`, and unrelated Origin rejection could not be verified through the trusted deployment route. |
| Runtime no-store | FAIL | The runtime route matrix could not be verified through the trusted deployment route. |
| WSS origin/auth | FAIL | `/ws/events` and `/ws/input` handshakes could not be verified through the trusted deployment route. |
| Mixed-content absence | FAIL | Browser root/assets are not serving the bridge UI through the trusted deployment route. |
| Outside-network isolation | FAIL | No technical outside-network/firewall/ingress evidence was supplied for this run. |

Diagnostic-only insecure HTTPS probes returned `404` for the root, API, and WSS
paths. Those probes are not acceptance evidence; they only confirm that the
hostname is not currently serving the bridge runtime.

## Commands

Repeatable sanitized check:

```sh
ROM_BRIDGE_VALIDATION_DIR=<private-validation-dir>/deployment-network-kut/<run-id> \
ROM_BRIDGE_COOKIE_CURL_CONFIG_FILE=<private-cookie-curl-config> \
scripts/deployment-network-check.sh
```

The script writes raw evidence under the private validation directory and emits
only sanitized PASS/FAIL lines. It must not print cookies, private addresses,
raw headers, response bodies, or private paths.

## Remaining Blockers

`kut` should remain deferred until all of the following are true:

- trusted TLS verification succeeds for `https://rombridge.birb.homes/`;
- the bridge service is active and bound only to the documented trusted
  interface or a documented loopback proxy topology;
- `/health` returns sanitized health over the trusted deployment route;
- allowed-Origin/no-cookie runtime requests produce sanitized auth rejection;
- valid-cookie requests with absent, `null`, and unrelated Origins are rejected;
- reachable runtime GET/POST and private preview/image routes include no-store
  headers;
- both `/ws/events` and `/ws/input` reject unauthenticated and wrong-Origin
  handshakes and accept allowed-Origin authenticated handshakes;
- browser-facing assets contain no `http://` or `ws://` runtime endpoints;
- outside-network isolation is backed by a technical artifact, such as an
  outside-network probe, firewall/ingress policy, Host/SNI routing evidence plus
  listener evidence, or equivalent network ACL proof.

A private request has been created for the deployment/operator agent to provide
these prerequisites before the next `kut` run. The request asks for a running
bridge service, trusted TLS, Host/SNI-routed ingress, runtime API and WSS
proxying, and technical outside-network isolation evidence.

## References

- `docs/deployment-note.md`
- `docs/deployment-security-shape.md`
- `docs/runtime-api.md`
- `docs/redaction.md`
- `scripts/deployment-network-check.sh`
