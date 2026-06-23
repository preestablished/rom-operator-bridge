use serde::Serialize;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendMode {
    Synthetic,
    Real,
}

impl FromStr for BackendMode {
    type Err = BackendModeParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "synthetic" => Ok(Self::Synthetic),
            "real" => Ok(Self::Real),
            _ => Err(BackendModeParseError),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendModeParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BackendCapabilities {
    pub input: bool,
    pub preview: bool,
    pub capture: bool,
    pub labels: bool,
    pub privileged_features: bool,
    pub validation_runner: bool,
}

impl BackendCapabilities {
    pub const fn synthetic_mvp() -> Self {
        Self {
            input: true,
            preview: true,
            capture: true,
            labels: true,
            privileged_features: false,
            validation_runner: false,
        }
    }

    pub const fn unavailable_real() -> Self {
        Self {
            input: false,
            preview: false,
            capture: false,
            labels: false,
            privileged_features: false,
            validation_runner: false,
        }
    }
}

pub trait BridgeBackend: Send + Sync {
    fn mode(&self) -> BackendMode;
    fn capabilities(&self) -> BackendCapabilities;
}

#[derive(Debug, Default)]
pub struct SyntheticBackend;

impl BridgeBackend for SyntheticBackend {
    fn mode(&self) -> BackendMode {
        BackendMode::Synthetic
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::synthetic_mvp()
    }
}

#[derive(Debug, Default)]
pub struct RealBackendUnavailable;

impl BridgeBackend for RealBackendUnavailable {
    fn mode(&self) -> BackendMode {
        BackendMode::Real
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::unavailable_real()
    }
}
