# Validation And Evidence

## Private Validation Run

After service and K3s route activation, run the existing validation script with
operator-approved private inputs:

```sh
ROM_BRIDGE_VALIDATION_DIR=<private-validation-dir>/deployment-network-kut/<run-id> \
ROM_BRIDGE_COOKIE_CURL_CONFIG_FILE=<private-cookie-curl-config> \
ROM_BRIDGE_NETWORK_EVIDENCE_FILE=<private-network-evidence-file> \
scripts/deployment-network-check.sh
```

Use `ROM_BRIDGE_SESSION_COOKIE_FILE` instead of
`ROM_BRIDGE_COOKIE_CURL_CONFIG_FILE` only if the operator provides a private
cookie file. Do not print cookie contents.

If the deployment uses a private resolve override, set:

```sh
ROM_BRIDGE_RESOLVE_IP=<bridge-private-ip>
```

Do not include the instantiated command line in committed docs.

## Expected PASS Matrix

The script and docs should prove:

- private cookie source is mode-safe;
- DNS resolves;
- trusted TLS succeeds;
- service bind is proven by private listener evidence;
- `/health` is reachable and sanitized;
- unauthenticated runtime requests reject without private details;
- allowed-Origin/no-cookie runtime requests reject;
- valid-cookie requests with absent, `null`, and wrong Origins reject;
- valid-cookie requests with allowed Origin can reach expected runtime status;
- runtime GET and POST routes include no-store headers;
- private preview/image routes include no-store headers;
- `/ws/events` and `/ws/input` reject unauthenticated or wrong-Origin
  handshakes;
- `/ws/events` and `/ws/input` accept allowed-Origin authenticated handshakes;
- UI root and browser-facing assets contain no `http://` or `ws://` runtime
  endpoints;
- outside-network isolation is backed by a private technical artifact.

## Updating `docs/deployment-checks.md`

Convert the current blocked status to a sanitized PASS status only after the
script succeeds. Keep:

- the date of the run;
- a private evidence label, not a path;
- a result table with PASS entries;
- a short summary of the chosen topology;
- references to `docs/deployment-note.md`,
  `docs/deployment-security-shape.md`, and the validation script.

Do not paste:

- raw command output;
- private env names with values;
- private paths or addresses;
- cookies or request bodies;
- service logs;
- kubectl output;
- certificate serials or private issuer details if those are sensitive in this
  environment.

## Blocked Outcome

If any check fails, do not close `kut`. Update `docs/deployment-checks.md` with
a sanitized blocked summary and keep the exact raw failure private.

Acceptable public blocker examples:

- trusted TLS route is still not issuing a valid certificate;
- service is active but root UI route is not serving expected static files;
- WSS upgrade fails through Traefik;
- wrong-Origin rejection fails for authenticated requests;
- no outside-network evidence was supplied;
- real backend mode cannot start with the operator-approved private inputs.

## Leak Scan

Before committing docs or deploy artifacts, run targeted scans over changed
files:

```sh
git diff --check
bash scripts/redaction-gate.sh
rg -n -f <private-forbidden-pattern-file> \
  docs deploy .agents/plans/fix-kut-deployment-prerequisites
```

The private forbidden-pattern file should include the operator's real secret,
cookie, private path, private address, and artifact-ref patterns. Investigate
every hit. Some placeholders or existing test fixtures may be intentional, but
no new private deployment value should appear in the diff.

## Evidence Handoff

Use this public wording shape:

```text
Sanitized deployment evidence for kut is recorded in docs/deployment-checks.md.
Raw evidence remains private under label <private-evidence-label>. The route is
served by the documented systemd plus K3s Traefik topology, and the validation
script passed all kut checks.
```
