# Prerequisites And Privacy Boundary

## 1. Confirm Current Bead State

Start the future session with:

```sh
bd prime
bd show rom-operator-bridge-kut
bd show rom-operator-bridge-eqi
bd ready
git status --short --branch
```

Expected current state:

- `kut` is deferred/open and blocks `eqi`;
- `eqi` is open and blocked by `kut`;
- there is no ready work before private/deployment evidence is available.

If the bead graph has changed, update this plan before executing host checks.

## 2. Required Operator Inputs

The executing agent needs operator approval for:

- the selected deployment URL, expected to remain
  `https://rombridge.birb.homes/`;
- whether the bridge service should be reachable only through the proxy or also
  on a documented trusted interface;
- the private validation directory for raw command output;
- a throwaway authenticated session cookie in a `0600` private cookie file for
  authenticated Origin and WebSocket checks;
- whether outside-network probing is available from the current host or must be
  represented by firewall/listener evidence.

Never commit or paste the concrete private values. Use placeholders:

- `<bridge-private-ip>`;
- `<private-validation-dir>`;
- `<redacted-session-cookie>`;
- `<deployment-host>`;
- `<operator-approved-network>`.

Never put cookie values in command argv, environment variables, stdout, committed
docs, plan updates, or bead notes. Commands may reference only the private cookie
file path, and raw outputs that contain cookies must stay in the private
validation directory.

## 3. Private Evidence Storage

Store raw outputs outside the repo, for example:

```sh
export ROM_BRIDGE_VALIDATION_DIR=<private-validation-dir>/deployment-network-kut
install -d -m 0700 "$ROM_BRIDGE_VALIDATION_DIR"
```

The raw output directory may contain:

- full `curl -i` outputs;
- proxy header dumps;
- `ss`/listener output;
- private DNS resolution output;
- WebSocket handshake output;
- browser or network probe notes.

Only sanitized pass/fail summaries belong in committed files.

## 4. Redaction Rules

Before committing docs, search touched public deliverables for private values and
common leak shapes. Use filename/count-only output and write it to the private
validation directory. Do not print matching lines to the terminal or paste them
into chat, final responses, docs, or bead notes.

```sh
SCAN_TARGETS=(docs/deployment-checks.md)
if [[ -f scripts/deployment-network-check.sh ]]; then
  SCAN_TARGETS+=(scripts/deployment-network-check.sh)
fi
rg -l '<actual-private-ip>|<actual-private-path>|<actual-cookie>|Bearer |Cookie|Set-Cookie|10\\.|192\\.168\\.|172\\.(1[6-9]|2[0-9]|3[0-1])\\.' \
  "${SCAN_TARGETS[@]}" \
  >"$ROM_BRIDGE_VALIDATION_DIR/redaction-candidates.txt" || true
test ! -s "$ROM_BRIDGE_VALIDATION_DIR/redaction-candidates.txt"
```

Replace any real values with placeholders. Do not rely on the static redaction
gate alone; it may not know all operator-local values.

## 5. Fallback If Access Is Still Missing

If host or operator access is unavailable, do not close `kut`. Instead:

- append a sanitized blocker note to `kut`;
- create or update a private request under the appropriate project request
  directory only if the operator asked for request-file handoff;
- leave `kut` deferred;
- commit only sanitized plan/doc updates.

Suggested bead note:

```text
kut remains deferred. Missing operator-approved deployment host/network evidence:
<one short sanitized reason>. No private values were recorded.
```
