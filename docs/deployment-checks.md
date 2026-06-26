# Deployment Network Isolation Checks

Date: 2026-06-26

This document records sanitized deployment-network verification for
`rom-operator-bridge-kut`. Raw command output is stored only in an
operator-private validation directory outside this repository.

## Status

`rom-operator-bridge-kut` passed with sanitized live operator evidence.

The deployment route now has reviewed private evidence for trusted TLS,
trusted-interface bind, runtime authentication/origin rejection, no-store
runtime routes, WSS origin/auth handling, mixed-content absence, Host/SNI
isolation, deployed static-root scanning, and outside-network rejection.
Operator-private raw evidence remains outside this repository.

## Evidence Boundary

Raw evidence label:

```text
private evidence: deployment-network-kut/20260626T212016Z
```

Superseded private request label:

```text
private request: rom-operator-bridge/kut-deployment-route-prerequisites
```

The evidence label identifies private evidence outside the repository. It is
not a filesystem path and must not be replaced with a concrete private path in
public docs, commits, chat, or bead notes.

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
| Private cookie source | PASS | A private `0600` session cookie source existed for authenticated probes. |
| DNS resolution | PASS | `rombridge.birb.homes` resolves on this host. |
| Trusted TLS | PASS | The deployment origin passed TLS verification. |
| Service bind | PASS | Reviewed private evidence proved bind on the trusted interface, not a wildcard listener. |
| Health sanitization | PASS | `/health` was reachable and sanitized. |
| Unauthenticated rejection | PASS | Runtime API rejected unauthenticated probes without storing private values. |
| Origin rejection | PASS | Absent, `null`, and unrelated Origins were rejected with a valid session cookie. |
| Runtime no-store | PASS | Runtime GET/POST and private preview/image routes returned no-store headers. |
| WSS origin/auth | PASS | `/ws/events` and `/ws/input` enforced authentication and allowed Origin. |
| Mixed-content absence | PASS | Static root and deployed root did not expose `http://` or `ws://` runtime endpoints. |
| Host/SNI isolation | PASS | Wrong-host and wrong-SNI probes did not serve the bridge route. |
| Outside-network isolation | PASS | Reviewed private outside-network evidence showed rejection or non-reachability. |

## Commands

Repeatable sanitized check:

```sh
ROM_BRIDGE_VALIDATION_DIR=<private-validation-dir>/deployment-network-kut/<run-id> \
ROM_BRIDGE_COOKIE_CURL_CONFIG_FILE=<private-cookie-curl-config> \
ROM_BRIDGE_STATIC_PUBLISH_ROOT=<absolute-static-publish-root> \
ROM_BRIDGE_NETWORK_EVIDENCE_FILE=<private-network-evidence-file> \
ROM_BRIDGE_NETWORK_EVIDENCE_REVIEWED=1 \
ROM_BRIDGE_OUTSIDE_PROBE_RESULT_FILE=<private-outside-probe-file> \
ROM_BRIDGE_OUTSIDE_PROBE_REVIEWED=1 \
ROM_BRIDGE_FORBID_FILE=<private-forbid-file> \
scripts/deployment-network-check.sh
```

The script writes raw evidence under the private validation directory and emits
only sanitized PASS/FAIL lines. It must not print cookies, private addresses,
raw headers, response bodies, or private paths.

If the run does not provide `ROM_BRIDGE_RESOLVE_IP`, also provide a reviewed
private Host/SNI evidence file:

```sh
ROM_BRIDGE_HOST_SNI_EVIDENCE_FILE=<private-host-sni-evidence-file> \
ROM_BRIDGE_HOST_SNI_EVIDENCE_REVIEWED=1
```

Deployment redaction checks must require the operator-private forbid file:

```sh
ROM_OPERATOR_BRIDGE_FORBID_FILE=<private-forbid-file> \
ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1 \
bash scripts/redaction-gate.sh
```

## Revalidation Triggers

Rerun the deployment-network checker and redaction gate before publishing a new
static release or after changing any of the following:

- service binary, systemd unit, private env schema, bind address, or static
  publish root;
- K3s ingress/service/endpoints, TLS certificate, Host/SNI route, or private
  endpoint manifest;
- runtime authentication, Origin/CORS, WebSocket, cache headers, static asset
  generation, or redaction rules;
- operator credential, session secret, private forbid file, or real backend
  handoff values.

## References

- `docs/deployment-note.md`
- `docs/deployment-security-shape.md`
- `deploy/README.md`
- `deploy/k8s/rombridge-ingress.yaml`
- `deploy/systemd/rom-operator-bridge.service`
- `docs/runtime-api.md`
- `docs/redaction.md`
- `scripts/deployment-network-check.sh`
