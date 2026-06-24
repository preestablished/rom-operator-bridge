use crate::{backend::BackendMode, sanitization::PublicSanitizer};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
};
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

pub const ENV_CONFIG_FILE: &str = "ROM_OPERATOR_BRIDGE_CONFIG_FILE";
pub const ENV_PRIVATE_ROOT: &str = "ROM_OPERATOR_BRIDGE_PRIVATE_ROOT";
pub const ENV_PRIVATE_ROOT_ALIAS: &str = "BRIDGE_PRIVATE_ROOT";
pub const ENV_STATIC_PUBLISH_ROOT: &str = "ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT";
pub const ENV_OPERATOR_CREDENTIAL: &str = "ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL";
pub const ENV_SESSION_SECRET: &str = "ROM_OPERATOR_BRIDGE_SESSION_SECRET";
pub const ENV_HYPERVISOR_ENDPOINT: &str = "BRIDGE_HYPERVISOR_ENDPOINT";
pub const ENV_WORKLOAD_IMAGE_REF: &str = "BRIDGE_WORKLOAD_IMAGE_REF";
pub const ENV_CAPTURE_SPEC_REF: &str = "BRIDGE_CAPTURE_SPEC_REF";
pub const ENV_REFERENCE_WORKLOAD_CHECKOUT: &str = "BRIDGE_REFERENCE_WORKLOAD_CHECKOUT";
pub const ENV_REAL_SNAPSHOT_REF: &str = "BRIDGE_REAL_SNAPSHOT_REF";
pub const ENV_CREATE_VM_CONFIG_REF: &str = "BRIDGE_CREATE_VM_CONFIG_REF";
pub const DEFAULT_HYPERVISOR_ENDPOINT: &str = "unix:///run/dh/grpc.sock";

pub const PRIVATE_ROOT_MARKER: &str = ".rom-operator-bridge-private-root";
pub const PRIVATE_DIR_MODE: u32 = 0o700;
pub const PRIVATE_FILE_MODE: u32 = 0o600;
pub const PRIVATE_RUN_DIRS: &[&str] = &["runs", "captures", "events", "validation", "tmp"];
const MIN_PRIVATE_ROOT_COMPONENTS: usize = 3;

#[derive(Clone, PartialEq, Eq)]
pub struct BridgePrivateConfig {
    root: Option<PrivateRootConfig>,
    operator_credential: Option<SecretValue>,
    session_secret: Option<SecretValue>,
    real_runtime: Option<RealRuntimeConfig>,
}

impl BridgePrivateConfig {
    pub fn placeholder() -> Self {
        Self {
            root: None,
            operator_credential: None,
            session_secret: None,
            real_runtime: None,
        }
    }

    pub fn from_values(
        values: &BTreeMap<String, String>,
        backend_mode: BackendMode,
    ) -> Result<Self, PrivateConfigError> {
        let has_private_value = [
            ENV_PRIVATE_ROOT,
            ENV_PRIVATE_ROOT_ALIAS,
            ENV_STATIC_PUBLISH_ROOT,
            ENV_OPERATOR_CREDENTIAL,
            ENV_SESSION_SECRET,
        ]
        .iter()
        .any(|env| {
            values
                .get(*env)
                .is_some_and(|value| !value.trim().is_empty())
        });

        if !has_private_value {
            if backend_mode == BackendMode::Synthetic {
                return Ok(Self::placeholder());
            }

            return Err(PrivateConfigError::MissingEnv {
                env: ENV_PRIVATE_ROOT,
            });
        }

        let root = required_path_any(values, &[ENV_PRIVATE_ROOT, ENV_PRIVATE_ROOT_ALIAS])?;
        let static_publish_root = optional_path(values, ENV_STATIC_PUBLISH_ROOT)?;
        validate_private_root(&root, static_publish_root.as_deref())?;

        let operator_credential = SecretValue::new(
            ENV_OPERATOR_CREDENTIAL,
            required_value(values, ENV_OPERATOR_CREDENTIAL)?,
        )?;
        let session_secret = SecretValue::new(
            ENV_SESSION_SECRET,
            required_value(values, ENV_SESSION_SECRET)?,
        )?;
        let real_runtime = if backend_mode == BackendMode::Real {
            Some(RealRuntimeConfig::from_values(values)?)
        } else {
            None
        };

        let config = Self {
            root: Some(PrivateRootConfig { path: root }),
            operator_credential: Some(operator_credential),
            session_secret: Some(session_secret),
            real_runtime,
        };
        config.prepare_runtime_dirs()?;
        Ok(config)
    }

    pub fn is_placeholder(&self) -> bool {
        self.root.is_none()
            && self.operator_credential.is_none()
            && self.session_secret.is_none()
            && self.real_runtime.is_none()
    }

    pub fn private_root(&self) -> Option<&Path> {
        self.root.as_ref().map(PrivateRootConfig::path)
    }

    pub fn operator_credential_configured(&self) -> bool {
        self.operator_credential.is_some()
    }

    pub fn session_secret_configured(&self) -> bool {
        self.session_secret.is_some()
    }

    pub fn real_runtime_config(&self) -> Option<&RealRuntimeConfig> {
        self.real_runtime.as_ref()
    }

    pub fn public_sanitizer(&self) -> PublicSanitizer {
        let mut sanitizer = PublicSanitizer::new();
        if let Some(root) = self.private_root() {
            sanitizer = sanitizer.with_private_root(root);
        }
        if let Some(secret) = &self.operator_credential {
            sanitizer = sanitizer.with_forbidden_literal(secret.as_str());
        }
        if let Some(secret) = &self.session_secret {
            sanitizer = sanitizer.with_forbidden_literal(secret.as_str());
        }
        if let Some(real_runtime) = &self.real_runtime {
            sanitizer = real_runtime.add_to_sanitizer(sanitizer);
        }
        sanitizer
    }

    pub fn verify_operator_credential(&self, candidate: &str) -> bool {
        self.operator_credential
            .as_ref()
            .is_some_and(|credential| constant_time_eq(candidate.as_bytes(), credential.as_bytes()))
    }

    pub fn sign_session_token(
        &self,
        issued_at_unix_seconds: u64,
        nonce: u64,
    ) -> Result<String, PrivateConfigError> {
        let secret = self
            .session_secret
            .as_ref()
            .ok_or(PrivateConfigError::MissingEnv {
                env: ENV_SESSION_SECRET,
            })?;
        let payload = format!("{issued_at_unix_seconds:x}.{nonce:x}");
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|_| PrivateConfigError::InvalidSessionSecret)?;
        mac.update(payload.as_bytes());
        let digest = mac.finalize().into_bytes();

        Ok(format!("v1.{payload}.{}", hex_lower(&digest)))
    }

    pub fn prepare_runtime_dirs(&self) -> Result<(), PrivateConfigError> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        let root = root.path();

        ensure_private_root_dir(root)?;
        for dir in PRIVATE_RUN_DIRS {
            ensure_private_descendant_dir(root, Path::new(dir))?;
        }

        Ok(())
    }

    pub fn write_private_file(
        &self,
        relative_path: impl AsRef<Path>,
        contents: &[u8],
    ) -> Result<PathBuf, PrivateConfigError> {
        self.write_private_file_with_mode(
            relative_path.as_ref(),
            contents,
            PrivateWriteMode::Truncate,
        )
    }

    pub fn append_private_file(
        &self,
        relative_path: impl AsRef<Path>,
        contents: &[u8],
    ) -> Result<PathBuf, PrivateConfigError> {
        self.write_private_file_with_mode(
            relative_path.as_ref(),
            contents,
            PrivateWriteMode::Append,
        )
    }

    pub fn read_private_file(
        &self,
        relative_path: impl AsRef<Path>,
    ) -> Result<Vec<u8>, PrivateConfigError> {
        let root = self
            .root
            .as_ref()
            .ok_or(PrivateConfigError::MissingEnv {
                env: ENV_PRIVATE_ROOT,
            })?
            .path();
        ensure_private_root_dir(root)?;
        let relative_path = validate_relative_path(relative_path.as_ref())?;
        let path = root.join(&relative_path);
        validate_private_file(&path)?;
        fs::read(&path).map_err(|error| io_error("read private file", &path, error))
    }

    pub fn write_private_file_atomic(
        &self,
        relative_path: impl AsRef<Path>,
        contents: &[u8],
    ) -> Result<PathBuf, PrivateConfigError> {
        let root = self
            .root
            .as_ref()
            .ok_or(PrivateConfigError::MissingEnv {
                env: ENV_PRIVATE_ROOT,
            })?
            .path();
        ensure_private_root_dir(root)?;
        let relative_path = validate_relative_path(relative_path.as_ref())?;

        if let Some(parent) = relative_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            ensure_private_descendant_dir(root, parent)?;
        }
        let path = root.join(&relative_path);
        if path_metadata(&path)?.is_some() {
            validate_private_file(&path)?;
        }

        let file_name = relative_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| PrivateConfigError::UnsafeRelativePath {
                path: relative_path.clone(),
            })?;
        let parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
        let temp_name = format!(
            ".tmp-{file_name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        );
        let temp_relative_path = parent.join(temp_name);
        let temp_path = root.join(&temp_relative_path);

        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        options.mode(PRIVATE_FILE_MODE);

        let mut file = options
            .open(&temp_path)
            .map_err(|error| io_error("create temporary private file", &temp_path, error))?;
        file.write_all(contents)
            .map_err(|error| io_error("write temporary private file", &temp_path, error))?;
        file.sync_all()
            .map_err(|error| io_error("sync temporary private file", &temp_path, error))?;
        drop(file);
        set_private_file_mode(&temp_path)?;
        fs::rename(&temp_path, &path)
            .map_err(|error| io_error("rename private file", &path, error))?;
        sync_parent_dir(&path)?;
        validate_private_file(&path)?;

        Ok(path)
    }

    fn write_private_file_with_mode(
        &self,
        relative_path: &Path,
        contents: &[u8],
        mode: PrivateWriteMode,
    ) -> Result<PathBuf, PrivateConfigError> {
        let root = self
            .root
            .as_ref()
            .ok_or(PrivateConfigError::MissingEnv {
                env: ENV_PRIVATE_ROOT,
            })?
            .path();
        ensure_private_root_dir(root)?;
        let relative_path = validate_relative_path(relative_path)?;

        if let Some(parent) = relative_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            ensure_private_descendant_dir(root, parent)?;
        }
        let path = root.join(&relative_path);
        let file_exists = path_metadata(&path)?.is_some();
        if file_exists {
            validate_private_file(&path)?;
        }

        let mut options = OpenOptions::new();
        options.create(true).write(true);
        match mode {
            PrivateWriteMode::Truncate => {
                options.truncate(true);
            }
            PrivateWriteMode::Append => {
                options.append(true);
            }
        }
        #[cfg(unix)]
        options.mode(PRIVATE_FILE_MODE);

        let mut file = options
            .open(&path)
            .map_err(|error| io_error("create private file", &path, error))?;
        file.write_all(contents)
            .map_err(|error| io_error("write private file", &path, error))?;
        file.sync_all()
            .map_err(|error| io_error("sync private file", &path, error))?;
        set_private_file_mode(&path)?;
        if matches!(mode, PrivateWriteMode::Append) && !file_exists {
            sync_parent_dir(&path)?;
        }

        Ok(path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateWriteMode {
    Truncate,
    Append,
}

impl fmt::Debug for BridgePrivateConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BridgePrivateConfig")
            .field("root_configured", &self.root.is_some())
            .field(
                "operator_credential_configured",
                &self.operator_credential.is_some(),
            )
            .field("session_secret_configured", &self.session_secret.is_some())
            .field("real_runtime_configured", &self.real_runtime.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RealRuntimeConfig {
    hypervisor_endpoint: HypervisorEndpoint,
    workload_image_ref: PrivateValue,
    capture_spec_ref: PrivateValue,
    reference_workload_checkout: PathBuf,
    start_source: RealStartSource,
}

impl RealRuntimeConfig {
    fn from_values(values: &BTreeMap<String, String>) -> Result<Self, PrivateConfigError> {
        let hypervisor_endpoint = HypervisorEndpoint::from_values(values)?;
        let workload_image_ref = PrivateValue::new(
            ENV_WORKLOAD_IMAGE_REF,
            required_value(values, ENV_WORKLOAD_IMAGE_REF)?,
        )?;
        let capture_spec_ref = PrivateValue::new(
            ENV_CAPTURE_SPEC_REF,
            required_value(values, ENV_CAPTURE_SPEC_REF)?,
        )?;
        let reference_workload_checkout = required_path(values, ENV_REFERENCE_WORKLOAD_CHECKOUT)?;
        let snapshot_ref = optional_private_value(values, ENV_REAL_SNAPSHOT_REF)?;
        let create_vm_config_ref = optional_private_value(values, ENV_CREATE_VM_CONFIG_REF)?;
        let start_source = match (snapshot_ref, create_vm_config_ref) {
            (Some(snapshot_ref), None) => RealStartSource::Snapshot { snapshot_ref },
            (None, Some(config_ref)) => RealStartSource::CreateVm { config_ref },
            (None, None) => {
                return Err(PrivateConfigError::MissingAnyEnv {
                    envs: &[ENV_REAL_SNAPSHOT_REF, ENV_CREATE_VM_CONFIG_REF],
                });
            }
            (Some(_), Some(_)) => {
                return Err(PrivateConfigError::ConflictingRealStartRefs {
                    envs: &[ENV_REAL_SNAPSHOT_REF, ENV_CREATE_VM_CONFIG_REF],
                });
            }
        };

        Ok(Self {
            hypervisor_endpoint,
            workload_image_ref,
            capture_spec_ref,
            reference_workload_checkout,
            start_source,
        })
    }

    pub fn hypervisor_endpoint(&self) -> &HypervisorEndpoint {
        &self.hypervisor_endpoint
    }

    pub fn start_source(&self) -> &RealStartSource {
        &self.start_source
    }

    fn add_to_sanitizer(&self, mut sanitizer: PublicSanitizer) -> PublicSanitizer {
        sanitizer = self.hypervisor_endpoint.add_to_sanitizer(sanitizer);
        sanitizer = sanitizer
            .with_forbidden_literal(self.workload_image_ref.as_str())
            .with_forbidden_literal(self.capture_spec_ref.as_str())
            .with_private_root(&self.reference_workload_checkout);
        match &self.start_source {
            RealStartSource::Snapshot { snapshot_ref } => {
                sanitizer.with_forbidden_literal(snapshot_ref.as_str())
            }
            RealStartSource::CreateVm { config_ref } => {
                sanitizer.with_forbidden_literal(config_ref.as_str())
            }
        }
    }
}

impl fmt::Debug for RealRuntimeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RealRuntimeConfig")
            .field("hypervisor_endpoint", &self.hypervisor_endpoint)
            .field("workload_image_ref_configured", &true)
            .field("capture_spec_ref_configured", &true)
            .field("reference_workload_checkout_configured", &true)
            .field("start_source", &self.start_source)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum HypervisorEndpoint {
    Unix { path: PathBuf },
    Http { uri: PrivateValue },
}

impl HypervisorEndpoint {
    fn from_values(values: &BTreeMap<String, String>) -> Result<Self, PrivateConfigError> {
        let endpoint = values
            .get(ENV_HYPERVISOR_ENDPOINT)
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(DEFAULT_HYPERVISOR_ENDPOINT)
            .trim();

        if let Some(path) = endpoint.strip_prefix("unix://") {
            let path = parse_absolute_path(ENV_HYPERVISOR_ENDPOINT, path)?;
            return Ok(Self::Unix { path });
        }

        if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            return Ok(Self::Http {
                uri: PrivateValue::new(ENV_HYPERVISOR_ENDPOINT, endpoint)?,
            });
        }

        Err(PrivateConfigError::InvalidEndpoint {
            env: ENV_HYPERVISOR_ENDPOINT,
        })
    }

    pub fn is_unix(&self) -> bool {
        matches!(self, Self::Unix { .. })
    }

    pub fn unix_path(&self) -> Option<&Path> {
        match self {
            Self::Unix { path } => Some(path),
            Self::Http { .. } => None,
        }
    }

    pub fn http_uri(&self) -> Option<&str> {
        match self {
            Self::Unix { .. } => None,
            Self::Http { uri } => Some(uri.as_str()),
        }
    }

    fn add_to_sanitizer(&self, sanitizer: PublicSanitizer) -> PublicSanitizer {
        match self {
            Self::Unix { path } => sanitizer.with_private_root(path),
            Self::Http { uri } => sanitizer.with_forbidden_literal(uri.as_str()),
        }
    }
}

impl fmt::Debug for HypervisorEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unix { .. } => formatter.write_str("HypervisorEndpoint::Unix([redacted])"),
            Self::Http { .. } => formatter.write_str("HypervisorEndpoint::Http([redacted])"),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum RealStartSource {
    Snapshot { snapshot_ref: PrivateValue },
    CreateVm { config_ref: PrivateValue },
}

impl RealStartSource {
    pub fn is_snapshot(&self) -> bool {
        matches!(self, Self::Snapshot { .. })
    }

    pub fn is_create_vm(&self) -> bool {
        matches!(self, Self::CreateVm { .. })
    }

    pub fn snapshot_hash(&self) -> Result<Option<[u8; 32]>, PrivateConfigError> {
        match self {
            Self::Snapshot { snapshot_ref } => {
                parse_hex32(ENV_REAL_SNAPSHOT_REF, snapshot_ref.as_str()).map(Some)
            }
            Self::CreateVm { .. } => Ok(None),
        }
    }

    pub fn create_vm_config_relative_path(&self) -> Result<Option<PathBuf>, PrivateConfigError> {
        match self {
            Self::Snapshot { .. } => Ok(None),
            Self::CreateVm { config_ref } => {
                validate_relative_path(Path::new(config_ref.as_str())).map(Some)
            }
        }
    }
}

impl fmt::Debug for RealStartSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Snapshot { .. } => formatter.write_str("RealStartSource::Snapshot([redacted])"),
            Self::CreateVm { .. } => formatter.write_str("RealStartSource::CreateVm([redacted])"),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PrivateRootConfig {
    path: PathBuf,
}

impl PrivateRootConfig {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl fmt::Debug for PrivateRootConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrivateRootConfig")
            .field("configured", &true)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    fn new(env: &'static str, value: &str) -> Result<Self, PrivateConfigError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(PrivateConfigError::MissingEnv { env });
        }

        let lowered = value.to_ascii_lowercase();
        if matches!(
            lowered.as_str(),
            "changeme" | "change-me" | "placeholder" | "replace-me" | "example"
        ) {
            return Err(PrivateConfigError::PlaceholderSecret { env });
        }

        Ok(Self(value.to_string()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }

    fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PrivateValue(String);

impl PrivateValue {
    fn new(env: &'static str, value: &str) -> Result<Self, PrivateConfigError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(PrivateConfigError::MissingEnv { env });
        }

        let lowered = value.to_ascii_lowercase();
        if matches!(
            lowered.as_str(),
            "changeme" | "change-me" | "placeholder" | "replace-me" | "example"
        ) {
            return Err(PrivateConfigError::PlaceholderPrivateValue { env });
        }

        Ok(Self(value.to_string()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PrivateValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PrivateValue([redacted])")
    }
}

pub fn merge_file_values(
    env_values: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, PrivateConfigError> {
    let Some(config_path) = env_values.get(ENV_CONFIG_FILE) else {
        return Ok(env_values);
    };

    let config_path = parse_absolute_path(ENV_CONFIG_FILE, config_path)?;
    let mut merged = read_private_env_file(&config_path)?;
    for (key, value) in env_values {
        merged.insert(key, value);
    }

    Ok(merged)
}

pub fn read_private_env_file(path: &Path) -> Result<BTreeMap<String, String>, PrivateConfigError> {
    validate_private_file(path)?;
    let contents =
        fs::read_to_string(path).map_err(|error| io_error("read config file", path, error))?;
    parse_env_file(&contents)
}

fn parse_env_file(contents: &str) -> Result<BTreeMap<String, String>, PrivateConfigError> {
    let mut values = BTreeMap::new();
    for (line_index, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, value)) = line.split_once('=') else {
            return Err(PrivateConfigError::InvalidConfigLine {
                line: line_index + 1,
            });
        };

        let key = key.trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
        {
            return Err(PrivateConfigError::InvalidConfigLine {
                line: line_index + 1,
            });
        }

        values.insert(key.to_string(), unquote_env_value(value.trim()).to_string());
    }

    Ok(values)
}

fn unquote_env_value(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn required_value<'a>(
    values: &'a BTreeMap<String, String>,
    env: &'static str,
) -> Result<&'a str, PrivateConfigError> {
    let value = values
        .get(env)
        .map(String::as_str)
        .ok_or(PrivateConfigError::MissingEnv { env })?;
    if value.trim().is_empty() {
        Err(PrivateConfigError::MissingEnv { env })
    } else {
        Ok(value)
    }
}

fn required_path(
    values: &BTreeMap<String, String>,
    env: &'static str,
) -> Result<PathBuf, PrivateConfigError> {
    parse_absolute_path(env, required_value(values, env)?)
}

fn required_path_any(
    values: &BTreeMap<String, String>,
    envs: &[&'static str],
) -> Result<PathBuf, PrivateConfigError> {
    for env in envs {
        if values
            .get(*env)
            .is_some_and(|value| !value.trim().is_empty())
        {
            return parse_absolute_path(env, required_value(values, env)?);
        }
    }

    Err(PrivateConfigError::MissingEnv { env: envs[0] })
}

fn optional_path(
    values: &BTreeMap<String, String>,
    env: &'static str,
) -> Result<Option<PathBuf>, PrivateConfigError> {
    values
        .get(env)
        .filter(|value| !value.trim().is_empty())
        .map(|value| parse_absolute_path(env, value))
        .transpose()
}

fn optional_private_value(
    values: &BTreeMap<String, String>,
    env: &'static str,
) -> Result<Option<PrivateValue>, PrivateConfigError> {
    values
        .get(env)
        .filter(|value| !value.trim().is_empty())
        .map(|value| PrivateValue::new(env, value))
        .transpose()
}

fn parse_absolute_path(env: &'static str, value: &str) -> Result<PathBuf, PrivateConfigError> {
    let path = PathBuf::from(value.trim());
    if !path.is_absolute() {
        return Err(PrivateConfigError::PathNotAbsolute { env, path });
    }
    normalize_absolute_path(env, &path)
}

fn normalize_absolute_path(env: &'static str, path: &Path) -> Result<PathBuf, PrivateConfigError> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                return Err(PrivateConfigError::PathContainsParent {
                    env,
                    path: path.to_path_buf(),
                });
            }
        }
    }

    Ok(normalized)
}

fn validate_private_root(
    root: &Path,
    static_publish_root: Option<&Path>,
) -> Result<(), PrivateConfigError> {
    if root_component_count(root) < MIN_PRIVATE_ROOT_COMPONENTS {
        return Err(PrivateConfigError::BroadPrivateRoot {
            path: root.to_path_buf(),
        });
    }
    reject_symlink_components(root)?;

    if let Some(static_publish_root) = static_publish_root {
        reject_symlink_components(static_publish_root)?;
        if root == static_publish_root || root.starts_with(static_publish_root) {
            return Err(PrivateConfigError::PrivateRootInsideStaticPublishRoot {
                private_root: root.to_path_buf(),
                static_publish_root: static_publish_root.to_path_buf(),
            });
        }
    }

    if let Some(metadata) = path_metadata(root)? {
        if !metadata.is_dir() {
            return Err(PrivateConfigError::NotDirectory {
                path: root.to_path_buf(),
            });
        }

        let mode = path_mode(root)?;
        if mode & 0o002 != 0 {
            return Err(PrivateConfigError::WorldWritableRoot {
                path: root.to_path_buf(),
            });
        }
        if mode != PRIVATE_DIR_MODE {
            return Err(PrivateConfigError::InsecureDirectoryMode {
                path: root.to_path_buf(),
                mode,
            });
        }
        validate_existing_private_root_contents(root)?;
    }

    Ok(())
}

fn root_component_count(path: &Path) -> usize {
    path.components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .count()
}

fn validate_existing_private_root_contents(root: &Path) -> Result<(), PrivateConfigError> {
    if private_root_marker_exists(root)? {
        return Ok(());
    }

    if fs::read_dir(root)
        .map_err(|error| io_error("read private root", root, error))?
        .next()
        .transpose()
        .map_err(|error| io_error("read private root", root, error))?
        .is_some()
    {
        return Err(PrivateConfigError::UnmarkedNonEmptyRoot {
            path: root.to_path_buf(),
        });
    }

    Ok(())
}

fn ensure_private_root_dir(root: &Path) -> Result<(), PrivateConfigError> {
    reject_symlink_components(root)?;
    match path_metadata(root)? {
        Some(metadata) => {
            if !metadata.is_dir() {
                return Err(PrivateConfigError::NotDirectory {
                    path: root.to_path_buf(),
                });
            }
            let mode = path_mode(root)?;
            if mode & 0o002 != 0 {
                return Err(PrivateConfigError::WorldWritableRoot {
                    path: root.to_path_buf(),
                });
            }
            if mode != PRIVATE_DIR_MODE {
                return Err(PrivateConfigError::InsecureDirectoryMode {
                    path: root.to_path_buf(),
                    mode,
                });
            }
            validate_existing_private_root_contents(root)?;
        }
        None => create_private_dir(root)?,
    }

    ensure_private_root_marker(root)
}

fn ensure_private_descendant_dir(
    root: &Path,
    relative_path: &Path,
) -> Result<(), PrivateConfigError> {
    let relative_path = validate_relative_path(relative_path)?;
    let mut current = root.to_path_buf();
    for component in relative_path.components() {
        let Component::Normal(part) = component else {
            return Err(PrivateConfigError::UnsafeRelativePath {
                path: relative_path.to_path_buf(),
            });
        };
        current.push(part);
        ensure_existing_or_new_private_dir(&current)?;
    }

    Ok(())
}

fn ensure_existing_or_new_private_dir(path: &Path) -> Result<(), PrivateConfigError> {
    match path_metadata(path)? {
        Some(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(PrivateConfigError::SymlinkPath {
                    path: path.to_path_buf(),
                });
            }
            if !metadata.is_dir() {
                return Err(PrivateConfigError::NotDirectory {
                    path: path.to_path_buf(),
                });
            }
            if path_mode(path)? & 0o002 != 0 {
                return Err(PrivateConfigError::WorldWritableDirectory {
                    path: path.to_path_buf(),
                });
            }
            let mode = path_mode(path)?;
            if mode != PRIVATE_DIR_MODE {
                return Err(PrivateConfigError::InsecureDirectoryMode {
                    path: path.to_path_buf(),
                    mode,
                });
            }
        }
        None => create_private_dir(path)?,
    }

    Ok(())
}

fn create_private_dir(path: &Path) -> Result<(), PrivateConfigError> {
    if let Some(parent) = path.parent() {
        reject_symlink_components(parent)?;
    }
    fs::create_dir(path).map_err(|error| io_error("create directory", path, error))?;
    set_private_dir_mode(path)?;
    let mode = path_mode(path)?;
    if mode != PRIVATE_DIR_MODE {
        return Err(PrivateConfigError::InsecureDirectoryMode {
            path: path.to_path_buf(),
            mode,
        });
    }
    Ok(())
}

fn validate_private_file(path: &Path) -> Result<(), PrivateConfigError> {
    reject_symlink(path)?;
    let metadata = fs::metadata(path).map_err(|error| io_error("stat config file", path, error))?;
    if !metadata.is_file() {
        return Err(PrivateConfigError::NotFile {
            path: path.to_path_buf(),
        });
    }

    let mode = path_mode(path)?;
    if mode != PRIVATE_FILE_MODE {
        return Err(PrivateConfigError::InsecureFileMode {
            path: path.to_path_buf(),
            mode,
        });
    }

    Ok(())
}

fn private_root_marker_exists(root: &Path) -> Result<bool, PrivateConfigError> {
    let marker = root.join(PRIVATE_ROOT_MARKER);
    if path_metadata(&marker)?.is_none() {
        return Ok(false);
    }

    validate_private_file(&marker)?;
    Ok(true)
}

fn ensure_private_root_marker(root: &Path) -> Result<(), PrivateConfigError> {
    let marker = root.join(PRIVATE_ROOT_MARKER);
    if private_root_marker_exists(root)? {
        return Ok(());
    }

    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(PRIVATE_FILE_MODE);

    let mut file = options
        .open(&marker)
        .map_err(|error| io_error("create private root marker", &marker, error))?;
    file.write_all(b"rom-operator-bridge private root\n")
        .map_err(|error| io_error("write private root marker", &marker, error))?;
    file.sync_all()
        .map_err(|error| io_error("sync private root marker", &marker, error))?;
    set_private_file_mode(&marker)?;
    validate_private_file(&marker)
}

fn path_metadata(path: &Path) -> Result<Option<fs::Metadata>, PrivateConfigError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error("stat path", path, error)),
    }
}

fn reject_symlink_components(path: &Path) -> Result<(), PrivateConfigError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(PrivateConfigError::UnsafeRelativePath {
                    path: path.to_path_buf(),
                });
            }
            Component::Normal(part) => {
                current.push(part);
                let Some(metadata) = path_metadata(&current)? else {
                    break;
                };
                if metadata.file_type().is_symlink() {
                    return Err(PrivateConfigError::SymlinkPath { path: current });
                }
                if !metadata.is_dir() && current != path {
                    return Err(PrivateConfigError::NotDirectory { path: current });
                }
            }
        }
    }

    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), PrivateConfigError> {
    let Some(metadata) = path_metadata(path)? else {
        return Ok(());
    };

    if metadata.file_type().is_symlink() {
        Err(PrivateConfigError::SymlinkPath {
            path: path.to_path_buf(),
        })
    } else {
        Ok(())
    }
}

fn validate_relative_path(path: &Path) -> Result<PathBuf, PrivateConfigError> {
    if path.is_absolute() {
        return Err(PrivateConfigError::UnsafeRelativePath {
            path: path.to_path_buf(),
        });
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(PrivateConfigError::UnsafeRelativePath {
                    path: path.to_path_buf(),
                });
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(PrivateConfigError::UnsafeRelativePath {
            path: path.to_path_buf(),
        });
    }

    Ok(normalized)
}

fn parse_hex32(env: &'static str, value: &str) -> Result<[u8; 32], PrivateConfigError> {
    let value = value.trim();
    if value.len() != 64 {
        return Err(PrivateConfigError::InvalidPrivateRef { env });
    }

    let mut bytes = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(chunk[0]).ok_or(PrivateConfigError::InvalidPrivateRef { env })?;
        let low = hex_nibble(chunk[1]).ok_or(PrivateConfigError::InvalidPrivateRef { env })?;
        bytes[index] = (high << 4) | low;
    }
    Ok(bytes)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(unix)]
fn path_mode(path: &Path) -> Result<u32, PrivateConfigError> {
    Ok(fs::metadata(path)
        .map_err(|error| io_error("stat permissions", path, error))?
        .permissions()
        .mode()
        & 0o777)
}

#[cfg(not(unix))]
fn path_mode(_path: &Path) -> Result<u32, PrivateConfigError> {
    Err(PrivateConfigError::UnsupportedPlatform)
}

#[cfg(unix)]
fn set_private_dir_mode(path: &Path) -> Result<(), PrivateConfigError> {
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIR_MODE))
        .map_err(|error| io_error("set directory mode", path, error))
}

#[cfg(not(unix))]
fn set_private_dir_mode(_path: &Path) -> Result<(), PrivateConfigError> {
    Err(PrivateConfigError::UnsupportedPlatform)
}

#[cfg(unix)]
fn set_private_file_mode(path: &Path) -> Result<(), PrivateConfigError> {
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_FILE_MODE))
        .map_err(|error| io_error("set file mode", path, error))
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &Path) -> Result<(), PrivateConfigError> {
    Err(PrivateConfigError::UnsupportedPlatform)
}

fn sync_parent_dir(path: &Path) -> Result<(), PrivateConfigError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    let directory =
        fs::File::open(parent).map_err(|error| io_error("open parent directory", parent, error))?;
    directory
        .sync_all()
        .map_err(|error| io_error("sync parent directory", parent, error))
}

fn io_error(operation: &'static str, path: &Path, error: io::Error) -> PrivateConfigError {
    PrivateConfigError::Io {
        operation,
        path: path.to_path_buf(),
        kind: error.kind(),
    }
}

#[derive(Clone, PartialEq, Eq, Error)]
pub enum PrivateConfigError {
    #[error("{env} is required for complete private bridge config")]
    MissingEnv { env: &'static str },
    #[error("one of {envs:?} is required for real backend startup")]
    MissingAnyEnv { envs: &'static [&'static str] },
    #[error("{envs:?} cannot both be configured for real backend startup")]
    ConflictingRealStartRefs { envs: &'static [&'static str] },
    #[error("{env} must not use a placeholder value")]
    PlaceholderSecret { env: &'static str },
    #[error("{env} must not use a placeholder value")]
    PlaceholderPrivateValue { env: &'static str },
    #[error("session secret could not be used")]
    InvalidSessionSecret,
    #[error("{env} must be unix://, http://, or https://")]
    InvalidEndpoint { env: &'static str },
    #[error("{env} must be a valid private reference")]
    InvalidPrivateRef { env: &'static str },
    #[error("{env} must be an absolute path")]
    PathNotAbsolute { env: &'static str, path: PathBuf },
    #[error("{env} must not contain parent directory segments")]
    PathContainsParent { env: &'static str, path: PathBuf },
    #[error("private root must not be inside the static publish root")]
    PrivateRootInsideStaticPublishRoot {
        private_root: PathBuf,
        static_publish_root: PathBuf,
    },
    #[error("private root must be a dedicated non-broad directory")]
    BroadPrivateRoot { path: PathBuf },
    #[error("existing private root must be empty or marked as a bridge private root")]
    UnmarkedNonEmptyRoot { path: PathBuf },
    #[error("private root must not be world-writable")]
    WorldWritableRoot { path: PathBuf },
    #[error("private directory must not be world-writable")]
    WorldWritableDirectory { path: PathBuf },
    #[error("private path must be a directory")]
    NotDirectory { path: PathBuf },
    #[error("private config path must be a file")]
    NotFile { path: PathBuf },
    #[error("private config paths must not be symlinks")]
    SymlinkPath { path: PathBuf },
    #[error("private config file must be mode 0600")]
    InsecureFileMode { path: PathBuf, mode: u32 },
    #[error("private directory must be mode 0700")]
    InsecureDirectoryMode { path: PathBuf, mode: u32 },
    #[error("private file path must be relative and stay below the private root")]
    UnsafeRelativePath { path: PathBuf },
    #[error("failed to {operation}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        kind: io::ErrorKind,
    },
    #[error("invalid private env file line {line}")]
    InvalidConfigLine { line: usize },
    #[error("private file modes require unix permissions")]
    UnsupportedPlatform,
}

impl fmt::Debug for PrivateConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut diff = left.len() ^ right.len();
    let max_len = left.len().max(right.len());
    for index in 0..max_len {
        let left_byte = left.get(index).copied().unwrap_or_default();
        let right_byte = right.get(index).copied().unwrap_or_default();
        diff |= (left_byte ^ right_byte) as usize;
    }

    diff == 0
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }

    out
}
