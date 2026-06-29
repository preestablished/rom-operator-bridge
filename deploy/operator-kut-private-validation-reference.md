# Operator kut Private Validation Reference

This note expands step 5 of
`deploy/operator-kut-deployment-runbook.md`. It is written for the operator who
already has an approved private env file. Keep instantiated paths, env values,
cookie files, endpoint addresses, raw logs, and evidence contents outside the
repository and out of chat.

Run commands from the repository checkout:

```sh
cd /home/infra-admin/git/preestablished/rom-operator-bridge
set +x
set -euo pipefail
```

## 1. Load The Private Env File

Set `PRIVATE_ENV` to your private env file. The file must be shell-compatible
`KEY=value` assignments.

```sh
PRIVATE_ENV=/absolute/path/to/private.env
PRIVATE_ENDPOINT_MANIFEST=/absolute/path/to/private-endpoint-manifest.yaml
PRIVATE_ENV_DIR=$(cd "$(dirname "$PRIVATE_ENV")" && pwd -P)

set -a
. "$PRIVATE_ENV"
set +a

: "${ROM_OPERATOR_BRIDGE_SESSION_SECRET:?missing ROM_OPERATOR_BRIDGE_SESSION_SECRET}"
: "${ROM_OPERATOR_BRIDGE_BIND_ADDR:?missing ROM_OPERATOR_BRIDGE_BIND_ADDR}"
: "${ROM_OPERATOR_BRIDGE_BACKEND:?missing ROM_OPERATOR_BRIDGE_BACKEND}"

for command_name in awk cargo curl find getent install ip kubectl node npm rg ss stat; do
  command -v "$command_name" >/dev/null 2>&1 || {
    printf 'missing required command: %s\n' "$command_name" >&2
    exit 127
  }
done

[ -f "$PRIVATE_ENDPOINT_MANIFEST" ] || {
  printf 'private endpoint manifest not found\n' >&2
  exit 2
}
```

The loaded env must include the values from runbook step 1. For real backend
runs, it must also include the approved real backend handoff values. The private
endpoint manifest is the file applied in runbook step 4.

## 2. Prepare Private Validation Paths

```sh
RUN_ID=$(date -u +%Y%m%dT%H%M%SZ)
PRIVATE_ROOT="${ROM_OPERATOR_BRIDGE_PRIVATE_ROOT:-${BRIDGE_PRIVATE_ROOT:-$PRIVATE_ENV_DIR}}"
VALIDATION_DIR="$PRIVATE_ROOT/validation/deployment-network-kut/$RUN_ID"
PRIVATE_ARTIFACT_DIR="$PRIVATE_ROOT/tmp/deployment-network-kut-$RUN_ID"
COOKIE_JAR="$PRIVATE_ARTIFACT_DIR/rombridge-session.cookiejar"
COOKIE_CURL_CONFIG_FILE="$PRIVATE_ARTIFACT_DIR/private-cookie.curl"
START_SESSION_JSON="$PRIVATE_ARTIFACT_DIR/start-session.json"
SESSION_RESPONSE="$PRIVATE_ARTIFACT_DIR/start-session.response.json"
NETWORK_EVIDENCE="$PRIVATE_ARTIFACT_DIR/kut-network-evidence.txt"
OUTSIDE_PROBE="$PRIVATE_ARTIFACT_DIR/kut-outside-probe.txt"
FORBID_FILE="$PRIVATE_ARTIFACT_DIR/kut-forbid.txt"
ENDPOINT_IP=$(awk '/^[[:space:]]*- ip:[[:space:]]*/ {print $3; exit}' "$PRIVATE_ENDPOINT_MANIFEST")
BIND_HOST="${ROM_OPERATOR_BRIDGE_BIND_ADDR%:*}"
if [ "$BIND_HOST" != "$ROM_OPERATOR_BRIDGE_BIND_ADDR" ] && [ -n "$BIND_HOST" ]; then
  BRIDGE_IP="$BIND_HOST"
else
  BRIDGE_IP=$(ip -4 route get 1.1.1.1 | awk '{for (i=1;i<=NF;i++) if ($i=="src") {print $(i+1); exit}}')
fi

case "$BRIDGE_IP" in
  ""|0.0.0.0|127.*|localhost)
    printf 'refusing unsafe bridge IP candidate\n' >&2
    exit 2
    ;;
esac

if printf '%s\n' "$BRIDGE_IP" | rg -q '\*'; then
  printf 'refusing wildcard bridge IP candidate\n' >&2
  exit 2
fi

[ -n "$ENDPOINT_IP" ] || {
  printf 'could not read endpoint IP from private endpoint manifest\n' >&2
  exit 2
}

[ "$BRIDGE_IP" = "$ENDPOINT_IP" ] || {
  printf 'bridge IP does not match reviewed private endpoint manifest\n' >&2
  exit 2
}

mkdir -p "$VALIDATION_DIR" "$PRIVATE_ARTIFACT_DIR"
chmod 700 "$VALIDATION_DIR" "$PRIVATE_ARTIFACT_DIR"
```

`BRIDGE_IP` must match the trusted private bridge address used by the private
endpoint manifest from runbook step 4. The preflight refuses to send the
session-start request if the address is empty, wildcard-like, loopback, or
different from the reviewed manifest.

## 3. Create A Throwaway Session Cookie

Generate the session-start JSON and request a cookie jar for the checker:

```sh
python3 scripts/prepare-deployment-validation-inputs.py \
  --start-session-json "$START_SESSION_JSON" \
  --cookie-jar "$COOKIE_JAR" \
  --session-response "$SESSION_RESPONSE" \
  --network-evidence "$NETWORK_EVIDENCE" \
  --bridge-ip "$BRIDGE_IP"
```

If you already have a private curl config containing the `Cookie:` header, you
can use that instead of `ROM_BRIDGE_SESSION_COOKIE_FILE` in the final checker
command.

## 4. Create Reviewed Private Evidence Files

The previous helper also creates local network evidence.

Review this file before setting `ROM_BRIDGE_NETWORK_EVIDENCE_REVIEWED=1`. It
must prove the service is not exposed on a wildcard listener such as `0.0.0.0`,
`*`, or `[::]`, and that the route points at the trusted private bridge
address.

Create or install the outside-network probe result:

```sh
install -m 0600 /path/to/operator-outside-probe-result "$OUTSIDE_PROBE"
```

Only set `ROM_BRIDGE_OUTSIDE_PROBE_REVIEWED=1` after reviewing that file. If the
network evidence file already includes equivalent firewall or ACL proof, you may
set `OUTSIDE_PROBE="$NETWORK_EVIDENCE"` instead of installing a separate file.
Do not create a placeholder outside-probe file and mark it reviewed.

Create the forbidden-literals file:

```sh
install -m 0600 /dev/null "$FORBID_FILE"
{
  for value in \
    "${ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL:-}" \
    "${ROM_OPERATOR_BRIDGE_SESSION_SECRET:-}" \
    "${ROM_OPERATOR_BRIDGE_PRIVATE_ROOT:-}" \
    "${BRIDGE_PRIVATE_ROOT:-}" \
    "${BRIDGE_HYPERVISOR_ENDPOINT:-}" \
    "${BRIDGE_WORKLOAD_IMAGE_REF:-}" \
    "${BRIDGE_CAPTURE_SPEC_REF:-}" \
    "${BRIDGE_REAL_SNAPSHOT_REF:-}" \
    "${BRIDGE_CREATE_VM_CONFIG_REF:-}" \
    "$PRIVATE_ENV" \
    "$PRIVATE_ENDPOINT_MANIFEST" \
    "$PRIVATE_ROOT" \
    "$VALIDATION_DIR" \
    "$PRIVATE_ARTIFACT_DIR" \
    "$COOKIE_JAR" \
    "$COOKIE_CURL_CONFIG_FILE" \
    "$START_SESSION_JSON" \
    "$SESSION_RESPONSE" \
    "$NETWORK_EVIDENCE" \
    "$OUTSIDE_PROBE" \
    "$FORBID_FILE" \
    "$BRIDGE_IP"
  do
    if [ -n "$value" ]; then
      printf '%s\n' "$value"
    fi
  done
} >> "$FORBID_FILE"
chmod 600 "$FORBID_FILE"
```

## 5. Run The Deployment-Network Checker

```sh
ROM_BRIDGE_VALIDATION_DIR="$VALIDATION_DIR" \
ROM_BRIDGE_SESSION_COOKIE_FILE="$COOKIE_JAR" \
ROM_BRIDGE_RESOLVE_IP="$BRIDGE_IP" \
ROM_BRIDGE_STATIC_PUBLISH_ROOT="${ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT:-/var/lib/rom-operator-bridge/static/current}" \
ROM_BRIDGE_NETWORK_EVIDENCE_FILE="$NETWORK_EVIDENCE" \
ROM_BRIDGE_NETWORK_EVIDENCE_REVIEWED=1 \
ROM_BRIDGE_OUTSIDE_PROBE_RESULT_FILE="$OUTSIDE_PROBE" \
ROM_BRIDGE_OUTSIDE_PROBE_REVIEWED=1 \
ROM_BRIDGE_FORBID_FILE="$FORBID_FILE" \
scripts/deployment-network-check.sh
```

If you use a private curl config instead of a cookie jar, replace
`ROM_BRIDGE_SESSION_COOKIE_FILE="$COOKIE_JAR"` with:

```sh
ROM_BRIDGE_COOKIE_CURL_CONFIG_FILE="$COOKIE_CURL_CONFIG_FILE" \
```

Because this command includes `ROM_BRIDGE_RESOLVE_IP`, the checker performs the
wrong-SNI probe itself. If you omit `ROM_BRIDGE_RESOLVE_IP`, provide
`ROM_BRIDGE_HOST_SNI_EVIDENCE_FILE` and
`ROM_BRIDGE_HOST_SNI_EVIDENCE_REVIEWED=1` as described in the runbook.

## 6. Run The Redaction Gate

```sh
ROM_OPERATOR_BRIDGE_FORBID_FILE="$FORBID_FILE" \
ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1 \
bash scripts/redaction-gate.sh
```

## 7. Safe Handoff

Report only:

- whether `scripts/deployment-network-check.sh` passed;
- whether `bash scripts/redaction-gate.sh` passed;
- the sanitized private evidence label, for example
  `deployment-network-kut/<run-id>`.

Do not report raw outputs, private paths, env values, endpoint addresses,
cookies, private refs, or logs.
