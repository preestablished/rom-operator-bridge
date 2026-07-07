use crate::{backend::BackendMode, private_config};
use axum::http::HeaderValue;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    net::SocketAddr,
    str::FromStr,
};
use thiserror::Error;

pub const DEFAULT_BIND_ADDR: &str = "10.0.0.106:7410";
pub const DEFAULT_PUBLIC_ORIGIN: &str = "https://rombridge.birb.homes";
pub const DEFAULT_BACKEND_MODE: BackendMode = BackendMode::Synthetic;
pub const ENV_BIND_ADDR: &str = "ROM_OPERATOR_BRIDGE_BIND_ADDR";
/// Rollback toggle for the streaming Play path (B2): `false` selects the
/// per-frame captured-Run path (B1) at `play_run` time. Committed default is
/// streaming; keep the toggle for at least one release after soak.
pub const ENV_PLAY_STREAMING: &str = "ROM_OPERATOR_BRIDGE_PLAY_STREAMING";
pub const DEFAULT_PLAY_STREAMING: bool = true;
pub const ENV_BACKEND_MODE: &str = "ROM_OPERATOR_BRIDGE_BACKEND";
pub const ENV_PUBLIC_ORIGIN: &str = "ROM_OPERATOR_BRIDGE_PUBLIC_ORIGIN";
pub const ENV_ALLOWED_ORIGINS: &str = "ROM_OPERATOR_BRIDGE_ALLOWED_ORIGINS";
pub const ENV_COOKIE_SECURE: &str = "ROM_OPERATOR_BRIDGE_COOKIE_SECURE";
pub const ENV_EXPOSURE_MODE: &str = "ROM_OPERATOR_BRIDGE_EXPOSURE_MODE";
pub const ENV_DEPLOYMENT_PROFILES: &str = "ROM_OPERATOR_BRIDGE_DEPLOYMENT_PROFILES";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    bind_addr: SocketAddr,
    backend_mode: BackendMode,
    service_version: String,
    private_config: private_config::BridgePrivateConfig,
    deployment_security: DeploymentSecurityConfig,
    play_streaming: bool,
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
        let deployment_security = DeploymentSecurityConfig::from_values(&values)?;
        let play_streaming = values
            .get(ENV_PLAY_STREAMING)
            .map(|value| parse_bool(ENV_PLAY_STREAMING, value))
            .transpose()?
            .unwrap_or(DEFAULT_PLAY_STREAMING);

        Ok(Self {
            bind_addr,
            backend_mode,
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            private_config,
            deployment_security,
            play_streaming,
        })
    }

    pub fn synthetic_for_addr(bind_addr: SocketAddr) -> Self {
        Self {
            bind_addr,
            backend_mode: BackendMode::Synthetic,
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            private_config: private_config::BridgePrivateConfig::placeholder(),
            deployment_security: DeploymentSecurityConfig::default(),
            play_streaming: DEFAULT_PLAY_STREAMING,
        }
    }

    pub const fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    pub const fn backend_mode(&self) -> BackendMode {
        self.backend_mode
    }

    pub const fn play_streaming(&self) -> bool {
        self.play_streaming
    }

    pub fn service_version(&self) -> &str {
        &self.service_version
    }

    pub fn private_config(&self) -> &private_config::BridgePrivateConfig {
        &self.private_config
    }

    pub fn deployment_security(&self) -> &DeploymentSecurityConfig {
        &self.deployment_security
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentSecurityConfig {
    profiles: Vec<DeploymentProfile>,
}

impl Default for DeploymentSecurityConfig {
    fn default() -> Self {
        Self {
            profiles: vec![DeploymentProfile::default_https()],
        }
    }
}

impl DeploymentSecurityConfig {
    fn from_values(values: &BTreeMap<String, String>) -> Result<Self, ConfigError> {
        if let Some(profile_list) = values
            .get(ENV_DEPLOYMENT_PROFILES)
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return Self::from_profile_list(values, profile_list);
        }

        let public_origin = values
            .get(ENV_PUBLIC_ORIGIN)
            .map(String::as_str)
            .unwrap_or(DEFAULT_PUBLIC_ORIGIN);
        let public_origin = Origin::parse(ENV_PUBLIC_ORIGIN, public_origin)?;
        let allowed_origins = values
            .get(ENV_ALLOWED_ORIGINS)
            .map(String::as_str)
            .map(|value| parse_origin_list(ENV_ALLOWED_ORIGINS, value))
            .transpose()?
            .unwrap_or_else(|| vec![public_origin.clone()]);
        let cookie_secure = values
            .get(ENV_COOKIE_SECURE)
            .map(String::as_str)
            .map(|value| parse_bool(ENV_COOKIE_SECURE, value))
            .transpose()?
            .unwrap_or(true);
        let exposure_mode = values
            .get(ENV_EXPOSURE_MODE)
            .map(String::as_str)
            .map(|value| parse_exposure_mode(ENV_EXPOSURE_MODE, value))
            .transpose()?
            .unwrap_or(ExposureMode::HttpsOrigin);
        let profile = DeploymentProfile::new(
            "default".to_string(),
            public_origin,
            allowed_origins,
            cookie_secure,
            exposure_mode,
        )?;
        Ok(Self {
            profiles: vec![profile],
        })
    }

    fn from_profile_list(
        values: &BTreeMap<String, String>,
        profile_list: &str,
    ) -> Result<Self, ConfigError> {
        let mut profiles = Vec::new();
        let mut profile_ids = BTreeSet::new();
        let mut profile_env_prefixes = BTreeSet::new();
        let mut public_hosts = BTreeSet::new();
        let mut allowed_origins_seen = BTreeSet::new();
        for raw_id in profile_list.split(',') {
            let id = raw_id.trim();
            if id.is_empty() {
                return Err(ConfigError::EmptyDeploymentProfiles {
                    env: ENV_DEPLOYMENT_PROFILES,
                });
            }
            let env_prefix = profile_env_prefix(id);
            if !profile_ids.insert(id.to_ascii_lowercase())
                || !profile_env_prefixes.insert(env_prefix.clone())
            {
                return Err(ConfigError::DuplicateDeploymentProfile {
                    env: ENV_DEPLOYMENT_PROFILES,
                });
            }
            let public_origin_env = format!("{env_prefix}_PUBLIC_ORIGIN");
            let public_origin_value =
                required_profile_value(values, ENV_DEPLOYMENT_PROFILES, &public_origin_env)?;
            let public_origin = Origin::parse(ENV_DEPLOYMENT_PROFILES, public_origin_value)?;
            for host_match_key in public_origin.host_match_keys() {
                if !public_hosts.insert(host_match_key) {
                    return Err(ConfigError::DuplicateDeploymentProfile {
                        env: ENV_DEPLOYMENT_PROFILES,
                    });
                }
            }
            let allowed_origins_env = format!("{env_prefix}_ALLOWED_ORIGINS");
            let allowed_origins = values
                .get(&allowed_origins_env)
                .map(String::as_str)
                .map(|value| parse_origin_list(ENV_DEPLOYMENT_PROFILES, value))
                .transpose()?
                .unwrap_or_else(|| vec![public_origin.clone()]);
            for allowed_origin in &allowed_origins {
                if !allowed_origins_seen.insert(allowed_origin.as_str().to_ascii_lowercase()) {
                    return Err(ConfigError::DuplicateDeploymentProfile {
                        env: ENV_DEPLOYMENT_PROFILES,
                    });
                }
            }
            let cookie_secure_env = format!("{env_prefix}_COOKIE_SECURE");
            let cookie_secure = values
                .get(&cookie_secure_env)
                .map(String::as_str)
                .map(|value| parse_bool(ENV_DEPLOYMENT_PROFILES, value))
                .transpose()?
                .unwrap_or(true);
            let exposure_mode_env = format!("{env_prefix}_EXPOSURE_MODE");
            let exposure_mode = values
                .get(&exposure_mode_env)
                .map(String::as_str)
                .map(|value| parse_exposure_mode(ENV_DEPLOYMENT_PROFILES, value))
                .transpose()?
                .unwrap_or_else(|| {
                    if cookie_secure {
                        ExposureMode::HttpsOrigin
                    } else {
                        ExposureMode::TailscaleHttp
                    }
                });
            profiles.push(DeploymentProfile::new(
                id.to_string(),
                public_origin,
                allowed_origins,
                cookie_secure,
                exposure_mode,
            )?);
        }
        if profiles.is_empty() {
            return Err(ConfigError::EmptyDeploymentProfiles {
                env: ENV_DEPLOYMENT_PROFILES,
            });
        }
        Ok(Self { profiles })
    }

    pub fn default_profile(&self) -> &DeploymentProfile {
        self.profiles
            .first()
            .expect("deployment security has at least one profile")
    }

    pub fn profile_for_origin(&self, origin: &str) -> Option<&DeploymentProfile> {
        self.profiles
            .iter()
            .find(|profile| profile.allows_origin(origin))
    }

    pub fn profile_for_host_header(&self, host: Option<&str>) -> Option<&DeploymentProfile> {
        let host = host.map(str::trim).filter(|host| !host.is_empty())?;
        self.profiles
            .iter()
            .find(|profile| profile.matches_host_header(host))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentProfile {
    id: String,
    public_origin: Origin,
    allowed_origins: Vec<Origin>,
    cookie_secure: bool,
    exposure_mode: ExposureMode,
}

impl DeploymentProfile {
    fn default_https() -> Self {
        Self {
            id: "default".to_string(),
            public_origin: Origin::parse(ENV_PUBLIC_ORIGIN, DEFAULT_PUBLIC_ORIGIN)
                .expect("default origin parses"),
            allowed_origins: vec![
                Origin::parse(ENV_PUBLIC_ORIGIN, DEFAULT_PUBLIC_ORIGIN)
                    .expect("default origin parses"),
            ],
            cookie_secure: true,
            exposure_mode: ExposureMode::HttpsOrigin,
        }
    }

    fn new(
        id: String,
        public_origin: Origin,
        allowed_origins: Vec<Origin>,
        cookie_secure: bool,
        exposure_mode: ExposureMode,
    ) -> Result<Self, ConfigError> {
        if allowed_origins.is_empty() {
            return Err(ConfigError::InvalidOrigin {
                env: ENV_ALLOWED_ORIGINS,
            });
        }
        let mut origins_seen = BTreeSet::new();
        for allowed_origin in &allowed_origins {
            if !origins_seen.insert(allowed_origin.as_str().to_ascii_lowercase())
                || allowed_origin.scheme != public_origin.scheme
            {
                return Err(ConfigError::InvalidOrigin {
                    env: ENV_ALLOWED_ORIGINS,
                });
            }
        }
        if exposure_mode == ExposureMode::TailscaleHttp
            && (public_origin.is_https() || allowed_origins.iter().any(Origin::is_https))
        {
            return Err(ConfigError::InvalidCookiePolicy {
                env: ENV_COOKIE_SECURE,
            });
        }
        if !cookie_secure {
            if exposure_mode != ExposureMode::TailscaleHttp
                || public_origin.is_https()
                || allowed_origins.iter().any(Origin::is_https)
            {
                return Err(ConfigError::InvalidCookiePolicy {
                    env: ENV_COOKIE_SECURE,
                });
            }
        }
        Ok(Self {
            id,
            public_origin,
            allowed_origins,
            cookie_secure,
            exposure_mode,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub const fn cookie_secure(&self) -> bool {
        self.cookie_secure
    }

    pub fn public_origin(&self) -> &str {
        self.public_origin.as_str()
    }

    pub fn static_csp(&self) -> String {
        format!(
            "default-src 'self'; connect-src 'self' {}; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'",
            self.public_origin.websocket_origin()
        )
    }

    fn allows_origin(&self, origin: &str) -> bool {
        self.allowed_origins
            .iter()
            .any(|allowed| allowed.matches_origin(origin))
    }

    fn matches_host_header(&self, host: &str) -> bool {
        self.public_origin.matches_host_header(host)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Origin {
    raw: String,
    scheme: OriginScheme,
    host: String,
    port: Option<u16>,
}

impl Origin {
    fn parse(env: &'static str, value: &str) -> Result<Self, ConfigError> {
        let value = value.trim();
        let (scheme, rest) = if let Some(rest) = value.strip_prefix("https://") {
            (OriginScheme::Https, rest)
        } else if let Some(rest) = value.strip_prefix("http://") {
            (OriginScheme::Http, rest)
        } else {
            return Err(ConfigError::InvalidOrigin { env });
        };
        if rest.is_empty()
            || rest.contains('/')
            || rest.contains('?')
            || rest.contains('#')
            || rest.contains('@')
            || HeaderValue::from_str(value).is_err()
        {
            return Err(ConfigError::InvalidOrigin { env });
        }
        let (host, port) = parse_host_port(env, rest)?;
        if host.is_empty() || host == "*" {
            return Err(ConfigError::InvalidOrigin { env });
        }
        Ok(Self {
            raw: value.to_string(),
            scheme,
            host: host.to_ascii_lowercase(),
            port,
        })
    }

    fn as_str(&self) -> &str {
        &self.raw
    }

    fn is_https(&self) -> bool {
        self.scheme == OriginScheme::Https
    }

    fn matches_origin(&self, origin: &str) -> bool {
        self.raw == origin
    }

    fn matches_host_header(&self, host: &str) -> bool {
        let host = host.trim().to_ascii_lowercase();
        if host == self.host_header() {
            return true;
        }
        self.port.is_none()
            && (host == self.host
                || host == format!("{}:{}", self.host, self.scheme.default_port()))
    }

    fn host_header(&self) -> String {
        match self.port {
            Some(port) => format!("{}:{port}", self.host),
            None => self.host.clone(),
        }
    }

    fn host_match_keys(&self) -> Vec<String> {
        match self.port {
            Some(port) if port == self.scheme.default_port() => {
                vec![self.host.clone(), self.host_header()]
            }
            Some(_) => vec![self.host_header()],
            None => vec![
                self.host.clone(),
                format!("{}:{}", self.host, self.scheme.default_port()),
            ],
        }
    }

    fn websocket_origin(&self) -> String {
        let scheme = match self.scheme {
            OriginScheme::Http => "ws",
            OriginScheme::Https => "wss",
        };
        format!("{scheme}://{}", self.host_header())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OriginScheme {
    Http,
    Https,
}

impl OriginScheme {
    const fn default_port(self) -> u16 {
        match self {
            Self::Http => 80,
            Self::Https => 443,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExposureMode {
    HttpsOrigin,
    TailscaleHttp,
}

#[derive(Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{env} must be a valid socket address")]
    InvalidBindAddr { env: &'static str },
    #[error("{env} must be synthetic or real")]
    InvalidBackendMode { env: &'static str },
    #[error("{env} must contain one or more deployment profiles")]
    EmptyDeploymentProfiles { env: &'static str },
    #[error("{env} contains duplicate or ambiguous deployment profiles")]
    DuplicateDeploymentProfile { env: &'static str },
    #[error(
        "{env} must be an absolute http(s) origin without path, query, credentials, or wildcard host"
    )]
    InvalidOrigin { env: &'static str },
    #[error("{env} must be true or false")]
    InvalidBoolean { env: &'static str },
    #[error("{env} must be https-origin or tailscale-http")]
    InvalidExposureMode { env: &'static str },
    #[error("{env} may be false only for an explicit tailscale-http profile with http origins")]
    InvalidCookiePolicy { env: &'static str },
    #[error(transparent)]
    PrivateConfig(#[from] private_config::PrivateConfigError),
}

impl fmt::Debug for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

fn parse_origin_list(env: &'static str, value: &str) -> Result<Vec<Origin>, ConfigError> {
    let mut origins = Vec::new();
    for origin in value.split(',') {
        let origin = origin.trim();
        if origin.is_empty() {
            return Err(ConfigError::InvalidOrigin { env });
        }
        origins.push(Origin::parse(env, origin)?);
    }
    if origins.is_empty() {
        return Err(ConfigError::InvalidOrigin { env });
    }
    Ok(origins)
}

fn parse_bool(env: &'static str, value: &str) -> Result<bool, ConfigError> {
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ConfigError::InvalidBoolean { env }),
    }
}

fn parse_exposure_mode(env: &'static str, value: &str) -> Result<ExposureMode, ConfigError> {
    match value.trim() {
        "https-origin" => Ok(ExposureMode::HttpsOrigin),
        "tailscale-http" => Ok(ExposureMode::TailscaleHttp),
        _ => Err(ConfigError::InvalidExposureMode { env }),
    }
}

fn parse_host_port(
    env: &'static str,
    authority: &str,
) -> Result<(String, Option<u16>), ConfigError> {
    if authority.starts_with('[') {
        let Some((host, rest)) = authority
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(']'))
        else {
            return Err(ConfigError::InvalidOrigin { env });
        };
        let port = if rest.is_empty() {
            None
        } else {
            Some(parse_port(
                env,
                rest.strip_prefix(':')
                    .ok_or(ConfigError::InvalidOrigin { env })?,
            )?)
        };
        return Ok((format!("[{host}]"), port));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, Some(parse_port(env, port)?)),
        Some(_) => return Err(ConfigError::InvalidOrigin { env }),
        None => (authority, None),
    };
    Ok((host.to_string(), port))
}

fn parse_port(env: &'static str, value: &str) -> Result<u16, ConfigError> {
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .ok_or(ConfigError::InvalidOrigin { env })
}

fn required_profile_value<'a>(
    values: &'a BTreeMap<String, String>,
    env: &'static str,
    key: &str,
) -> Result<&'a str, ConfigError> {
    values
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(ConfigError::InvalidOrigin { env })
}

fn profile_env_prefix(id: &str) -> String {
    let normalized: String = id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("ROM_OPERATOR_BRIDGE_PROFILE_{normalized}")
}
