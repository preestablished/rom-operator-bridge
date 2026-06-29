use crate::{
    config::DeploymentSecurityConfig,
    private_config::{BridgePrivateConfig, PrivateConfigError},
};
use axum::http::{HeaderMap, HeaderValue, Uri, header};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

pub const ALLOWED_ORIGIN: &str = "https://rombridge.birb.homes";
pub const SESSION_COOKIE_NAME: &str = "rom_operator_bridge_session";
pub const SESSION_TTL_SECONDS: u64 = 4 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorSession {
    pub token: String,
    pub expires_at_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAuthContext {
    pub profile_id: String,
    pub origin: HeaderValue,
    pub cookie_secure: bool,
}

#[derive(Debug, Clone)]
pub struct AuthState {
    inner: Arc<Mutex<AuthInner>>,
    clock: AuthClock,
}

impl Default for AuthState {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(AuthInner::default())),
            clock: AuthClock::System,
        }
    }

    pub fn fixed_for_tests(now_unix_seconds: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AuthInner::default())),
            clock: AuthClock::Fixed(Arc::new(AtomicU64::new(now_unix_seconds))),
        }
    }

    pub fn advance_for_tests(&self, seconds: u64) {
        if let AuthClock::Fixed(now) = &self.clock {
            now.fetch_add(seconds, Ordering::SeqCst);
        }
    }

    pub fn start_session(
        &self,
        private_config: &BridgePrivateConfig,
    ) -> Result<OperatorSession, AuthError> {
        let mut inner = self.inner.lock().expect("auth mutex poisoned");
        let now = self.clock.now_unix_seconds();
        inner.clear_expired(now);

        if inner.active.is_some() {
            return Err(AuthError::SessionActiveElsewhere);
        }

        let nonce = inner.next_nonce;
        inner.next_nonce += 1;
        let token = private_config.sign_session_token(now, nonce)?;
        let session = OperatorSession {
            token,
            expires_at_unix_seconds: now + SESSION_TTL_SECONDS,
        };
        inner.active = Some(ActiveSession {
            token: session.token.clone(),
            expires_at_unix_seconds: session.expires_at_unix_seconds,
        });

        Ok(session)
    }

    pub fn authenticate_headers(&self, headers: &HeaderMap) -> Result<(), AuthError> {
        let token = session_cookie(headers).ok_or(AuthError::MissingSession)?;
        self.authenticate_token(token)
    }

    pub fn clear_session_headers(&self, headers: &HeaderMap) -> Result<(), AuthError> {
        let token = session_cookie(headers).ok_or(AuthError::MissingSession)?;
        self.authenticate_token(token)?;
        self.clear_session_token(token);
        Ok(())
    }

    pub fn authenticate_token(&self, token: &str) -> Result<(), AuthError> {
        let mut inner = self.inner.lock().expect("auth mutex poisoned");
        let now = self.clock.now_unix_seconds();
        let active = inner.active.as_ref().ok_or(AuthError::MissingSession)?;
        if active.expires_at_unix_seconds <= now {
            inner.active = None;
            return Err(AuthError::ExpiredSession);
        }
        if constant_time_eq(token.as_bytes(), active.token.as_bytes()) {
            Ok(())
        } else {
            Err(AuthError::BadSession)
        }
    }

    pub fn clear_session_token(&self, token: &str) {
        let mut inner = self.inner.lock().expect("auth mutex poisoned");
        if inner
            .active
            .as_ref()
            .is_some_and(|session| constant_time_eq(token.as_bytes(), session.token.as_bytes()))
        {
            inner.active = None;
        }
    }
}

#[derive(Debug, Clone)]
enum AuthClock {
    System,
    Fixed(Arc<AtomicU64>),
}

impl AuthClock {
    fn now_unix_seconds(&self) -> u64 {
        match self {
            Self::System => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or_default(),
            Self::Fixed(now) => now.load(Ordering::SeqCst),
        }
    }
}

#[derive(Debug, Default)]
struct AuthInner {
    active: Option<ActiveSession>,
    next_nonce: u64,
}

impl AuthInner {
    fn clear_expired(&mut self, now_unix_seconds: u64) {
        if self
            .active
            .as_ref()
            .is_some_and(|session| session.expires_at_unix_seconds <= now_unix_seconds)
        {
            self.active = None;
        }
    }
}

#[derive(Debug, Clone)]
struct ActiveSession {
    token: String,
    expires_at_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuthError {
    #[error("credentials are not accepted in URLs")]
    CredentialInUrl,
    #[error("origin is required")]
    MissingOrigin,
    #[error("origin is not allowed")]
    OriginRejected,
    #[error("an operator session is already active")]
    SessionActiveElsewhere,
    #[error("session cookie is missing")]
    MissingSession,
    #[error("session cookie is expired")]
    ExpiredSession,
    #[error("session cookie is invalid")]
    BadSession,
    #[error(transparent)]
    PrivateConfig(#[from] PrivateConfigError),
}

pub fn validate_runtime_request(
    headers: &HeaderMap,
    uri: &Uri,
    deployment_security: &DeploymentSecurityConfig,
) -> Result<RuntimeAuthContext, AuthError> {
    reject_credentials_in_url(uri)?;
    validate_runtime_headers(headers, deployment_security)
}

pub fn reject_credentials_in_url(uri: &Uri) -> Result<(), AuthError> {
    if uri.query().is_some() {
        return Err(AuthError::CredentialInUrl);
    }

    Ok(())
}

pub fn validate_origin(
    headers: &HeaderMap,
    deployment_security: &DeploymentSecurityConfig,
) -> Result<RuntimeAuthContext, AuthError> {
    let origin_header = headers
        .get(header::ORIGIN)
        .ok_or(AuthError::MissingOrigin)?;
    let origin = origin_header
        .to_str()
        .ok()
        .ok_or(AuthError::MissingOrigin)?;

    let profile = deployment_security
        .profile_for_origin(origin)
        .ok_or(AuthError::OriginRejected)?;
    Ok(RuntimeAuthContext {
        profile_id: profile.id().to_string(),
        origin: origin_header.clone(),
        cookie_secure: profile.cookie_secure(),
    })
}

pub fn validate_runtime_headers(
    headers: &HeaderMap,
    deployment_security: &DeploymentSecurityConfig,
) -> Result<RuntimeAuthContext, AuthError> {
    let context = validate_origin(headers, deployment_security)?;
    let Some(host_header) = headers.get(header::HOST) else {
        return Ok(context);
    };
    let host = host_header
        .to_str()
        .map_err(|_| AuthError::OriginRejected)?;
    let host_profile = deployment_security
        .profile_for_host_header(Some(host))
        .ok_or(AuthError::OriginRejected)?;
    if host_profile.id() != context.profile_id {
        return Err(AuthError::OriginRejected);
    }
    Ok(context)
}

pub fn session_cookie_header(session: &OperatorSession, secure: bool) -> String {
    cookie_header(
        &format!(
            "{SESSION_COOKIE_NAME}={}; Path=/; Max-Age={SESSION_TTL_SECONDS}; HttpOnly",
            session.token
        ),
        secure,
    )
}

pub fn expired_session_cookie_header(secure: bool) -> String {
    cookie_header(
        &format!("{SESSION_COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly"),
        secure,
    )
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    for cookie in cookies.split(';') {
        let cookie = cookie.trim();
        if let Some(value) = cookie
            .strip_prefix(SESSION_COOKIE_NAME)
            .and_then(|value| value.strip_prefix('='))
        {
            return Some(value);
        }
    }

    None
}

fn cookie_header(prefix: &str, secure: bool) -> String {
    if secure {
        format!("{prefix}; Secure; SameSite=Strict")
    } else {
        format!("{prefix}; SameSite=Strict")
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
