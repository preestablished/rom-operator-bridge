use crate::private_config::{BridgePrivateConfig, PrivateConfigError};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use uuid::Uuid;

const SCHEMA_VERSION: u32 = 1;
const LEASE_TOKEN_BYTES: usize = 16;
const INTENTS_DIR: &str = "leases/intents";
const ACTIVE_DIR: &str = "leases/active";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LeaseIntent {
    pub schema_version: u32,
    pub operation_id: String,
    pub session_id: String,
    pub run_id: String,
    pub source: String,
    pub created_at: String,
    pub allocation_kind: AllocationKind,
}

impl fmt::Debug for LeaseIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LeaseIntent")
            .field("schema_version", &self.schema_version)
            .field("operation_id", &self.operation_id)
            .field("allocation_kind", &self.allocation_kind)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AllocationKind {
    RestoreSnapshot,
    CreateVm,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LeaseRecord {
    pub schema_version: u32,
    pub operation_id: String,
    pub session_id: String,
    pub run_id: String,
    pub source: String,
    pub created_at: String,
    pub allocation_kind: AllocationKind,
    pub slot_id: u64,
    token_hex: String,
    pub lease_recorded_at: String,
}

impl fmt::Debug for LeaseRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LeaseRecord")
            .field("schema_version", &self.schema_version)
            .field("operation_id", &self.operation_id)
            .field("slot_id", &self.slot_id)
            .finish_non_exhaustive()
    }
}

impl LeaseIntent {
    pub fn new(
        session_id: String,
        run_id: String,
        source: String,
        allocation_kind: AllocationKind,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            operation_id: Uuid::new_v4().to_string(),
            session_id,
            run_id,
            source,
            created_at: now(),
            allocation_kind,
        }
    }
    pub fn promote(&self, lease: &dh_proto::v1::Lease) -> LeaseRecord {
        LeaseRecord {
            schema_version: SCHEMA_VERSION,
            operation_id: self.operation_id.clone(),
            session_id: self.session_id.clone(),
            run_id: self.run_id.clone(),
            source: self.source.clone(),
            created_at: self.created_at.clone(),
            allocation_kind: self.allocation_kind,
            slot_id: lease.slot_id,
            token_hex: hex_encode(&lease.token),
            lease_recorded_at: now(),
        }
    }
}

impl LeaseRecord {
    pub fn lease(&self) -> Result<dh_proto::v1::Lease, LeaseStoreError> {
        Ok(dh_proto::v1::Lease {
            slot_id: self.slot_id,
            token: hex_decode(&self.token_hex)?,
        })
    }
}

#[derive(Clone)]
pub struct LeaseStore {
    config: BridgePrivateConfig,
}
pub struct LoadedLeaseStore {
    pub intents: Vec<LeaseIntent>,
    pub leases: Vec<LeaseRecord>,
    pub invalid: usize,
}

impl LeaseStore {
    pub fn new(config: BridgePrivateConfig) -> Self {
        Self { config }
    }
    pub fn write_intent(&self, value: &LeaseIntent) -> Result<(), LeaseStoreError> {
        self.write(INTENTS_DIR, &value.operation_id, value)
    }
    pub fn write_lease(&self, value: &LeaseRecord) -> Result<(), LeaseStoreError> {
        self.write(ACTIVE_DIR, &value.operation_id, value)
    }
    pub fn remove_intent(&self, id: &str) -> Result<(), LeaseStoreError> {
        self.remove(INTENTS_DIR, id)
    }
    pub fn remove_lease(&self, id: &str) -> Result<(), LeaseStoreError> {
        self.remove(ACTIVE_DIR, id)
    }
    pub fn load(&self) -> Result<LoadedLeaseStore, LeaseStoreError> {
        let (intents, a) = self.load_dir::<LeaseIntent>(INTENTS_DIR)?;
        let (leases, b) = self.load_dir::<LeaseRecord>(ACTIVE_DIR)?;
        Ok(LoadedLeaseStore {
            intents,
            leases,
            invalid: a + b,
        })
    }
    pub fn clear_dangling_intents(&self, selected: &[String]) -> Result<usize, LeaseStoreError> {
        if selected.is_empty() {
            return Err(LeaseStoreError::InvalidSelection);
        }
        let unique: std::collections::BTreeSet<_> = selected.iter().collect();
        if unique.len() != selected.len() {
            return Err(LeaseStoreError::InvalidSelection);
        }
        let loaded = self.load()?;
        if loaded.invalid != 0 || !loaded.leases.is_empty() {
            return Err(LeaseStoreError::UnsafeStoreState);
        }
        for id in selected {
            validate_operation_id(id)?;
            if !loaded
                .intents
                .iter()
                .any(|intent| &intent.operation_id == id)
            {
                return Err(LeaseStoreError::InvalidSelection);
            }
        }
        for id in selected {
            self.remove_intent(id)?;
        }
        Ok(selected.len())
    }
    pub fn dangling_intent_ids(&self) -> Result<Vec<String>, LeaseStoreError> {
        let loaded = self.load()?;
        if loaded.invalid != 0 || !loaded.leases.is_empty() {
            return Err(LeaseStoreError::UnsafeStoreState);
        }
        Ok(loaded
            .intents
            .into_iter()
            .map(|intent| intent.operation_id)
            .collect())
    }
    fn write<T: Serialize + RecordValidation>(
        &self,
        dir: &str,
        id: &str,
        value: &T,
    ) -> Result<(), LeaseStoreError> {
        validate_operation_id(id)?;
        if !value.valid_for(id) {
            return Err(LeaseStoreError::InvalidRecord);
        }
        let bytes = serde_json::to_vec(value).map_err(|_| LeaseStoreError::InvalidRecord)?;
        self.config
            .write_private_file_atomic(record_path(dir, id), &bytes)?;
        Ok(())
    }
    fn remove(&self, dir: &str, id: &str) -> Result<(), LeaseStoreError> {
        validate_operation_id(id)?;
        self.config
            .remove_private_file_durable(record_path(dir, id))?;
        Ok(())
    }
    fn load_dir<T: for<'de> Deserialize<'de> + RecordValidation>(
        &self,
        dir: &str,
    ) -> Result<(Vec<T>, usize), LeaseStoreError> {
        let mut records = Vec::new();
        let mut invalid = 0;
        for path in self.config.list_private_files(dir)? {
            if is_atomic_temp_path(&path) {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                invalid += 1;
                continue;
            };
            if path.extension().and_then(|s| s.to_str()) != Some("json")
                || validate_operation_id(stem).is_err()
            {
                invalid += 1;
                continue;
            }
            let bytes = match self.config.read_private_file(&path) {
                Ok(v) => v,
                Err(_) => {
                    invalid += 1;
                    continue;
                }
            };
            match serde_json::from_slice::<T>(&bytes) {
                Ok(v) if v.valid_for(stem) => records.push(v),
                _ => invalid += 1,
            }
        }
        Ok((records, invalid))
    }
}

trait RecordValidation {
    fn valid_for(&self, id: &str) -> bool;
}
impl RecordValidation for LeaseIntent {
    fn valid_for(&self, id: &str) -> bool {
        valid_common(
            self.schema_version,
            &self.operation_id,
            id,
            &self.session_id,
            &self.run_id,
            &self.source,
            &self.created_at,
            self.allocation_kind,
        )
    }
}
impl RecordValidation for LeaseRecord {
    fn valid_for(&self, id: &str) -> bool {
        valid_common(
            self.schema_version,
            &self.operation_id,
            id,
            &self.session_id,
            &self.run_id,
            &self.source,
            &self.created_at,
            self.allocation_kind,
        ) && self.lease_recorded_at.parse::<u64>().is_ok()
            && hex_decode(&self.token_hex).is_ok()
    }
}
#[allow(clippy::too_many_arguments)]
fn valid_common(
    schema_version: u32,
    operation_id: &str,
    file_id: &str,
    session_id: &str,
    run_id: &str,
    source: &str,
    created_at: &str,
    allocation_kind: AllocationKind,
) -> bool {
    let source_valid = match allocation_kind {
        AllocationKind::RestoreSnapshot => {
            source.len() == 64
                && source
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }
        AllocationKind::CreateVm => source == "create_vm_config",
    };
    schema_version == SCHEMA_VERSION
        && operation_id == file_id
        && validate_operation_id(file_id).is_ok()
        && safe_identifier(session_id)
        && safe_identifier(run_id)
        && created_at.parse::<u64>().is_ok()
        && source_valid
}
fn safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
fn record_path(dir: &str, id: &str) -> PathBuf {
    Path::new(dir).join(format!("{id}.json"))
}
fn is_atomic_temp_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(rest) = name.strip_prefix(".tmp-") else {
        return false;
    };
    let mut parts = rest.rsplitn(3, '-');
    let (Some(nanos), Some(pid), Some(record_name)) = (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    if nanos.is_empty()
        || pid.is_empty()
        || !nanos.bytes().all(|byte| byte.is_ascii_digit())
        || !pid.bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    let Some(id) = record_name.strip_suffix(".json") else {
        return false;
    };
    validate_operation_id(id).is_ok()
}
fn validate_operation_id(id: &str) -> Result<(), LeaseStoreError> {
    let value = Uuid::parse_str(id).map_err(|_| LeaseStoreError::InvalidRecord)?;
    if value.get_version_num() == 4 && value.hyphenated().to_string() == id {
        Ok(())
    } else {
        Err(LeaseStoreError::InvalidRecord)
    }
}
fn now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn hex_decode(value: &str) -> Result<Vec<u8>, LeaseStoreError> {
    if value.len() != LEASE_TOKEN_BYTES * 2
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(LeaseStoreError::InvalidRecord);
    }
    (0..value.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&value[i..i + 2], 16).map_err(|_| LeaseStoreError::InvalidRecord)
        })
        .collect()
}

#[derive(Debug, Error)]
pub enum LeaseStoreError {
    #[error("private lease storage unavailable")]
    Private(#[from] PrivateConfigError),
    #[error("invalid private lease record")]
    InvalidRecord,
    #[error("lease store state is unsafe for intent acknowledgement")]
    UnsafeStoreState,
    #[error("invalid intent acknowledgement selection")]
    InvalidSelection,
}
