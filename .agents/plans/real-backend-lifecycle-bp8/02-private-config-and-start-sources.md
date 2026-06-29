# Private Config and Start Sources

## Snapshot Ref Parsing

`BRIDGE_REAL_SNAPSHOT_REF` should be accepted as a 32-byte BLAKE3 snapshot ref
encoded as 64 lowercase or uppercase hex characters. Implement a private parser:

```rust
fn parse_hex32(value: &str) -> Result<[u8; 32], PrivateConfigError>
```

Add a new sanitized error variant if needed, for example:

```rust
InvalidPrivateRef { env: &'static str }
```

The Display and Debug output must not include the supplied value.

Expose only typed, redacted accessors:

```rust
impl RealStartSource {
    pub fn snapshot_hash(&self) -> Option<[u8; 32]>;
}
```

Do not expose `PrivateValue::as_str` publicly.

## CreateVm Config Ref Resolution

`BRIDGE_CREATE_VM_CONFIG_REF` currently stores an opaque private value. For bp8,
define it as a private-root-relative JSON file path, for example:

```text
real/create-vm-config.json
```

Validation rules:

- path must be relative;
- path must stay below `BRIDGE_PRIVATE_ROOT`;
- file must be mode `0600`;
- file contents are private and must never be copied into public errors.

Recommended helper:

```rust
impl BridgePrivateConfig {
    pub fn read_private_file(&self, relative_path: impl AsRef<Path>) -> Result<Vec<u8>, PrivateConfigError>;
}
```

Use the existing private path validation code. If needed, factor the internal
relative-path validation so read and write paths share it.

## CreateVm JSON Shape

Create a bridge-owned private JSON schema that maps exactly to
`dh_proto::v1::MachineConfig`. Keep the shape close to the proto, but encode
hash and seed `bytes` fields as 64-hex strings and command lines as UTF-8
strings:

```json
{
  "schema_version": 1,
  "machine_config": {
    "version": 1,
    "mem_bytes": 134217728,
    "vcpus": 1,
    "clock_num": 1,
    "clock_den": 1,
    "base_image_hash": "<64 hex>",
    "boot": {
      "elf": {
        "kernel_hash": "<64 hex>",
        "cmdline": "1000000"
      }
    },
    "epoch_len": 50000000,
    "hash_epochs": "epochs_on",
    "skid_margin": 8192,
    "cpuid_table": [],
    "device_set": []
  },
  "entropy_seed": "<64 hex>"
}
```

Pinned mappings and validation:

- `schema_version` must be `1`.
- `machine_config.version` must be `1`.
- `machine_config.mem_bytes` must be a positive multiple of 2 MiB.
- `machine_config.vcpus` must be `1` for the current worker contract.
- `clock_num`, `clock_den`, `epoch_len`, and `skid_margin` are copied as
  unsigned integers. Reject zero `clock_den`.
- `base_image_hash`, `boot.*.kernel_hash`, optional
  `boot.bzimage.initramfs_hash`, and `entropy_seed` decode from exactly 64 hex
  characters to 32 bytes.
- `boot` is a oneof. Accept exactly one of `elf` or `bzimage`; support
  `boot.elf` first and add `boot.bzimage` only if the private operator config
  actually needs it.
- `boot.*.cmdline` is encoded to UTF-8 bytes for the proto.
- `hash_epochs` accepts only `"epochs_on"` and `"final_only"` and maps to
  `EPOCHS_ON` and `FINAL_ONLY`. Do not emit `HASH_EPOCHS_UNSPECIFIED` from a
  valid config.
- `cpuid_table` entries use exact field names from the proto: `function`,
  `index`, `flags`, `eax`, `ebx`, `ecx`, `edx`. Sort by
  `(function, index)` and reject duplicates after sorting.
- `device_set` entries must fit in `u16`; sort ascending and reject duplicates.
- reject unknown top-level fields and unknown `machine_config` fields with
  `#[serde(deny_unknown_fields)]`.

The proto supports more shapes than bp8 needs. The bridge should not invent ROM
startup defaults beyond what the private config file supplies.

Implementation types can live in `service/src/backend.rs` initially:

```rust
#[derive(Deserialize)]
struct PrivateCreateVmFile { ... }
```

Convert to:

```rust
dh_proto::v1::CreateVmRequest {
    config: Some(proto_config),
    entropy_seed,
}
```

Invalid config maps to `BackendUnavailable` from the backend. Config parse
tests can assert the private parser rejects invalid shapes without asserting
public error text that includes values.

## Availability Semantics

When `BRIDGE_REAL_SNAPSHOT_REF` is configured:

- `start_session` must call `RestoreSnapshot`;
- if no lease is returned, fail with `BackendUnavailable`;
- current frame comes from `RestoreSnapshotResponse.frame_counter`.

When `BRIDGE_CREATE_VM_CONFIG_REF` is configured:

- `start_session` must read the private JSON file;
- convert it to `MachineConfig`;
- call `CreateVm`;
- current frame starts at `0`.

The existing config rule that rejects both start sources at once should remain.
