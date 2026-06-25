# Private Workspace And Bridge Config

## 1. Preflight The Handoff Env

The executing agent must obtain the actual handoff path from the private
operator channel and set it only in shell state:

```bash
set +x
export O73_HANDOFF_ENV="<operator-private handoff env path>"
test -f "$O73_HANDOFF_ENV"
printf 'handoff mode: '
stat -c '%a' "$O73_HANDOFF_ENV"
```

Expected mode: `600`.

Load it into the current private shell:

```bash
set -a
. "$O73_HANDOFF_ENV"
set +a
```

Validate presence without printing values:

```bash
for key in \
  BRIDGE_REAL_SNAPSHOT_REF \
  BRIDGE_WORKLOAD_IMAGE_REF \
  BRIDGE_CAPTURE_SPEC_REF \
  BRIDGE_HYPERVISOR_ENDPOINT \
  BRIDGE_PRIVATE_ROOT \
  BRIDGE_REFERENCE_WORKLOAD_CHECKOUT \
  SNAPSTORE_DATA_ROOT \
  SNAPSTORE_CONFIG_PATH \
  SNAPSTORE_GRPC_UDS_PATH \
  DH_M9_IMAGE_CACHE
do
  value="$(eval "printf '%s' \"\${$key:-}\"")"
  if [ -z "$value" ]; then
    echo "$key missing from private handoff" >&2
    exit 1
  fi
done

if grep -q '^BRIDGE_CREATE_VM_CONFIG_REF=' "$O73_HANDOFF_ENV"; then
  echo 'handoff must not set BRIDGE_CREATE_VM_CONFIG_REF for o73' >&2
  exit 1
fi
```

## 2. Create A Private o73 Workspace

Use a fresh private workspace outside the repository. It is acceptable to reuse
an existing private root only if the operator confirms it belongs to this o73
run.

```bash
set +x
umask 077
export O73_PRIVATE_ROOT="$HOME/.local/state/rom-operator-bridge/o73-live-restore"
install -d -m 0700 "$O73_PRIVATE_ROOT"
install -d -m 0700 "$O73_PRIVATE_ROOT"/{bridge,evidence,runtime}
install -d -m 0700 "$BRIDGE_PRIVATE_ROOT"
```

Keep raw API responses, logs, cookies, and generated secrets under
`$O73_PRIVATE_ROOT/evidence`.

## 3. Generate Bridge-Only Secrets

The bridge service needs an operator credential and session secret. These do not
come from the hypervisor handoff. Generate fresh values for this acceptance run:

```bash
export O73_OPERATOR_CREDENTIAL="$(openssl rand -hex 32)"
export O73_SESSION_SECRET="$(openssl rand -hex 64)"
test -n "$O73_OPERATOR_CREDENTIAL"
test -n "$O73_SESSION_SECRET"
```

Do not print these values.

## 4. Materialize The Bridge Env File

Pick a local bind address. Use an unoccupied loopback port:

```bash
export O73_BRIDGE_BIND_ADDR="127.0.0.1:7420"
```

Materialize the bridge config file. The file is private and may contain quoted
values; the service config parser accepts simple single or double quotes.

```bash
export O73_BRIDGE_ENV="$O73_PRIVATE_ROOT/bridge/real-restore-snapshot.env"
cat > "$O73_BRIDGE_ENV" <<EOF
ROM_OPERATOR_BRIDGE_BACKEND=real
ROM_OPERATOR_BRIDGE_BIND_ADDR='$O73_BRIDGE_BIND_ADDR'
ROM_OPERATOR_BRIDGE_PRIVATE_ROOT='$BRIDGE_PRIVATE_ROOT'
ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL='$O73_OPERATOR_CREDENTIAL'
ROM_OPERATOR_BRIDGE_SESSION_SECRET='$O73_SESSION_SECRET'

BRIDGE_HYPERVISOR_ENDPOINT='$BRIDGE_HYPERVISOR_ENDPOINT'
BRIDGE_WORKLOAD_IMAGE_REF='$BRIDGE_WORKLOAD_IMAGE_REF'
BRIDGE_CAPTURE_SPEC_REF='$BRIDGE_CAPTURE_SPEC_REF'
BRIDGE_REFERENCE_WORKLOAD_CHECKOUT='$BRIDGE_REFERENCE_WORKLOAD_CHECKOUT'
BRIDGE_REAL_SNAPSHOT_REF='$BRIDGE_REAL_SNAPSHOT_REF'
EOF
chmod 0600 "$O73_BRIDGE_ENV"
printf 'bridge env mode: '
stat -c '%a' "$O73_BRIDGE_ENV"
```

Do not add `BRIDGE_CREATE_VM_CONFIG_REF`.

## 5. Create The Start Request

```bash
jq -n \
  --arg credential "$O73_OPERATOR_CREDENTIAL" \
  '{
    schema_version: 1,
    operator_credential: $credential,
    backend_mode: "real",
    requested_capabilities: ["input", "preview", "capture"]
  }' > "$O73_PRIVATE_ROOT/evidence/start-request.private.json"
chmod 0600 "$O73_PRIVATE_ROOT/evidence/start-request.private.json"
```

## 6. Build A Private Forbidden-Literals File

This file drives leak sweeps. It is private and must not be printed.

```bash
export O73_FORBID_FILE="$O73_PRIVATE_ROOT/evidence/forbidden-literals.private.txt"
: > "$O73_FORBID_FILE"
chmod 0600 "$O73_FORBID_FILE"

for key in \
  O73_PRIVATE_ROOT \
  BRIDGE_PRIVATE_ROOT \
  O73_OPERATOR_CREDENTIAL \
  O73_SESSION_SECRET \
  BRIDGE_REAL_SNAPSHOT_REF \
  BRIDGE_WORKLOAD_IMAGE_REF \
  BRIDGE_CAPTURE_SPEC_REF \
  BRIDGE_REFERENCE_WORKLOAD_CHECKOUT \
  SNAPSTORE_DATA_ROOT \
  SNAPSTORE_CONFIG_PATH \
  SNAPSTORE_GRPC_UDS_PATH \
  DH_M9_IMAGE_CACHE \
  BRIDGE_HYPERVISOR_ENDPOINT
do
  value="$(eval "printf '%s' \"\${$key:-}\"")"
  [ -n "$value" ] && printf '%s\n' "$value" >> "$O73_FORBID_FILE"
done
```

Later steps should append the extracted session cookie and any private worker
error text before running sweeps.

