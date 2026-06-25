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

Repo-side deployment prerequisites have now been implemented for the next
operator run: the service can serve the static UI root from a configured static
publish root, committed systemd/K3s templates exist under `deploy/`, and the
deployment-network checker now requires reviewed private bind, Host/SNI,
static-root, and outside-network evidence. Live host installation and K3s apply
still require operator-private env and endpoint material, so this document does
not claim a deployment PASS yet.

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

## Remaining Blockers

`kut` should remain deferred until all of the following are true:

- trusted TLS verification succeeds for `https://rombridge.birb.homes/`;
- `deploy/systemd/rom-operator-bridge.service` is installed with a private
  env file and the service is active;
- the K3s ingress manifest is applied along with an operator-private endpoint
  manifest for the trusted bridge address;
- the bridge service is active and bound only to the documented trusted
  interface or a documented loopback proxy topology, with reviewed listener
  evidence that rejects wildcard binds;
- `/health` returns sanitized health over the trusted deployment route;
- allowed-Origin/no-cookie runtime requests produce sanitized auth rejection;
- valid-cookie requests with absent, `null`, and unrelated Origins are rejected;
- reachable runtime GET/POST and private preview/image routes include no-store
  headers;
- both `/ws/events` and `/ws/input` reject unauthenticated and wrong-Origin
  handshakes and accept allowed-Origin authenticated handshakes;
- Host/SNI probes or reviewed Host/SNI artifacts prove the bridge is not served
  for unrelated hostnames;
- the deployed static publish root is scanned as a clean release directory with
  no symlinks, source maps, private values, `http://`, or `ws://` runtime
  endpoints;
- outside-network isolation is backed by a technical artifact, such as an
  outside-network probe, firewall/ingress policy, Host/SNI routing evidence plus
  listener evidence, or equivalent network ACL proof, and the artifact is
  operator-reviewed.

A private request has been created for the deployment/operator agent to provide
these prerequisites before the next `kut` run. The request asks for a running
bridge service, trusted TLS, Host/SNI-routed ingress, runtime API and WSS
proxying, and technical outside-network isolation evidence.

## References

- `docs/deployment-note.md`
- `docs/deployment-security-shape.md`
- `deploy/README.md`
- `deploy/k8s/rombridge-ingress.yaml`
- `deploy/systemd/rom-operator-bridge.service`
- `docs/runtime-api.md`
- `docs/redaction.md`
- `scripts/deployment-network-check.sh`
