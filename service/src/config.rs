use crate::{backend::BackendMode, private_config};
use std::{collections::BTreeMap, fmt, net::SocketAddr, str::FromStr};
use thiserror::Error;

pub const DEFAULT_BIND_ADDR: &str = "10.0.0.106:7410";
pub const DEFAULT_BACKEND_MODE: BackendMode = BackendMode::Synthetic;
pub const ENV_BIND_ADDR: &str = "ROM_OPERATOR_BRIDGE_BIND_ADDR";
pub const ENV_BACKEND_MODE: &str = "ROM_OPERATOR_BRIDGE_BACKEND";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    bind_addr: SocketAddr,
    backend_mode: BackendMode,
    service_version: String,
    private_config: private_config::BridgePrivateConfig,
}

impl ServiceConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_pairs(std::env::vars())
    }

    pub fn from_pairs<I, K, V>(pairs: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let values: BTreeMap<String, String> = pairs
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        let values = private_config::merge_file_values(values)?;

        let bind_addr = values
            .get(ENV_BIND_ADDR)
            .map(String::as_str)
            .unwrap_or(DEFAULT_BIND_ADDR)
            .parse()
            .map_err(|_| ConfigError::InvalidBindAddr { env: ENV_BIND_ADDR })?;

        let backend_mode = values
            .get(ENV_BACKEND_MODE)
            .map(String::as_str)
            .unwrap_or(DEFAULT_BACKEND_MODE.as_str());
        let backend_mode =
            BackendMode::from_str(backend_mode).map_err(|_| ConfigError::InvalidBackendMode {
                env: ENV_BACKEND_MODE,
            })?;
        let private_config =
            private_config::BridgePrivateConfig::from_values(&values, backend_mode)?;

        Ok(Self {
            bind_addr,
            backend_mode,
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            private_config,
        })
    }

    pub fn synthetic_for_addr(bind_addr: SocketAddr) -> Self {
        Self {
            bind_addr,
            backend_mode: BackendMode::Synthetic,
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            private_config: private_config::BridgePrivateConfig::placeholder(),
        }
    }

    pub const fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    pub const fn backend_mode(&self) -> BackendMode {
        self.backend_mode
    }

    pub fn service_version(&self) -> &str {
        &self.service_version
    }

    pub fn private_config(&self) -> &private_config::BridgePrivateConfig {
        &self.private_config
    }
}

#[derive(Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{env} must be a valid socket address")]
    InvalidBindAddr { env: &'static str },
    #[error("{env} must be synthetic or real")]
    InvalidBackendMode { env: &'static str },
    #[error(transparent)]
    PrivateConfig(#[from] private_config::PrivateConfigError),
}

impl fmt::Debug for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}
