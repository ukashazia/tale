use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use super::client::{AdminClient, AdminError};

const KEYRING_SERVICE: &str = "tale";
const RECORD_VERSION: u8 = 1;
const REFRESH_LEAD: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialKind {
    OAuthClient,
    AccessToken,
}

impl CredentialKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::OAuthClient => "oauth_client",
            Self::AccessToken => "access_token",
        }
    }
}

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("credential is not available")]
    Unauthenticated,
    #[error("the OAuth client was rejected")]
    InvalidClient,
    #[error("the requested OAuth scope was denied")]
    ScopeDenied,
    #[error("the OAuth token response was malformed")]
    MalformedToken,
    #[error("the system clock is earlier than the token issue time")]
    ClockAnomaly,
    #[error("the OS keyring is unavailable")]
    Keyring,
    #[error("credential input was cancelled")]
    PromptCancelled,
    #[error("authentication transport failed")]
    Transport,
    #[error("authentication request timed out")]
    TimedOut,
    #[error("authentication request was cancelled")]
    Cancelled,
    #[error("authentication configuration is invalid")]
    Configuration,
    #[error("the requested scope has no verified Phase 5 validation read")]
    Unsupported,
    #[error("OAuth scopes must be narrow read scopes")]
    InvalidScopes,
}

pub struct SecretValue(Zeroizing<String>);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

#[derive(Debug)]
pub struct OAuthClientRecord {
    pub version: u8,
    pub client_id: SecretValue,
    pub client_secret: SecretValue,
    pub requested_scopes: Vec<String>,
}

#[derive(Debug)]
pub struct AccessTokenRecord {
    pub version: u8,
    pub access_token: SecretValue,
}

#[derive(Debug)]
pub enum CredentialRecord {
    OAuthClient(OAuthClientRecord),
    AccessToken(AccessTokenRecord),
}

impl CredentialRecord {
    pub fn kind(&self) -> CredentialKind {
        match self {
            Self::OAuthClient(_) => CredentialKind::OAuthClient,
            Self::AccessToken(_) => CredentialKind::AccessToken,
        }
    }
}

pub trait CredentialStore: Send + Sync {
    fn get(&self, reference: &str) -> Result<Option<Zeroizing<String>>, AuthError>;
    fn set(&self, reference: &str, value: &str) -> Result<(), AuthError>;
    fn delete(&self, reference: &str) -> Result<bool, AuthError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct OsCredentialStore;

impl CredentialStore for OsCredentialStore {
    fn get(&self, reference: &str) -> Result<Option<Zeroizing<String>>, AuthError> {
        let entry =
            keyring::Entry::new(KEYRING_SERVICE, reference).map_err(|_| AuthError::Keyring)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(AuthError::Keyring),
        }
    }

    fn set(&self, reference: &str, value: &str) -> Result<(), AuthError> {
        let entry =
            keyring::Entry::new(KEYRING_SERVICE, reference).map_err(|_| AuthError::Keyring)?;
        entry.set_password(value).map_err(|_| AuthError::Keyring)
    }

    fn delete(&self, reference: &str) -> Result<bool, AuthError> {
        let entry =
            keyring::Entry::new(KEYRING_SERVICE, reference).map_err(|_| AuthError::Keyring)?;
        match entry.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(_) => Err(AuthError::Keyring),
        }
    }
}

#[derive(Default)]
pub struct MemoryCredentialStore {
    values: std::sync::Mutex<BTreeMap<String, Zeroizing<String>>>,
}

impl fmt::Debug for MemoryCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryCredentialStore")
            .field(
                "entries",
                &self.values.lock().map_or(0, |values| values.len()),
            )
            .finish()
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn get(&self, reference: &str) -> Result<Option<Zeroizing<String>>, AuthError> {
        self.values
            .lock()
            .map_err(|_| AuthError::Keyring)
            .map(|values| {
                values
                    .get(reference)
                    .map(|value| Zeroizing::new(value.as_str().to_owned()))
            })
    }

    fn set(&self, reference: &str, value: &str) -> Result<(), AuthError> {
        self.values
            .lock()
            .map_err(|_| AuthError::Keyring)
            .map(|mut values| {
                values.insert(reference.to_owned(), Zeroizing::new(value.to_owned()));
            })
    }

    fn delete(&self, reference: &str) -> Result<bool, AuthError> {
        self.values
            .lock()
            .map_err(|_| AuthError::Keyring)
            .map(|mut values| values.remove(reference).is_some())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StoredRecord {
    OAuthClient {
        version: u8,
        client_id: String,
        client_secret: String,
        requested_scopes: Vec<String>,
    },
    AccessToken {
        version: u8,
        access_token: String,
    },
}

pub fn encode_record(record: &CredentialRecord) -> Result<Zeroizing<String>, AuthError> {
    let stored = match record {
        CredentialRecord::OAuthClient(record) => StoredRecord::OAuthClient {
            version: record.version,
            client_id: record.client_id.as_str().to_owned(),
            client_secret: record.client_secret.as_str().to_owned(),
            requested_scopes: record.requested_scopes.clone(),
        },
        CredentialRecord::AccessToken(record) => StoredRecord::AccessToken {
            version: record.version,
            access_token: record.access_token.as_str().to_owned(),
        },
    };
    serde_json::to_string(&stored)
        .map(Zeroizing::new)
        .map_err(|_| AuthError::Configuration)
}

pub fn decode_record(value: &str) -> Result<CredentialRecord, AuthError> {
    let stored =
        serde_json::from_str::<StoredRecord>(value).map_err(|_| AuthError::Configuration)?;
    match stored {
        StoredRecord::OAuthClient {
            version,
            client_id,
            client_secret,
            requested_scopes,
        } if version == RECORD_VERSION && !client_id.is_empty() && !client_secret.is_empty() => {
            Ok(CredentialRecord::OAuthClient(OAuthClientRecord {
                version,
                client_id: SecretValue::new(client_id),
                client_secret: SecretValue::new(client_secret),
                requested_scopes,
            }))
        }
        StoredRecord::AccessToken {
            version,
            access_token,
        } if version == RECORD_VERSION && !access_token.is_empty() => {
            Ok(CredentialRecord::AccessToken(AccessTokenRecord {
                version,
                access_token: SecretValue::new(access_token),
            }))
        }
        _ => Err(AuthError::Configuration),
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CredentialStatus {
    pub kind: CredentialKind,
    pub requested_scopes: Vec<String>,
    pub keyring_available: bool,
}

#[derive(Debug)]
pub struct AccessToken(SecretValue);

impl AccessToken {
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

struct CachedToken {
    value: SecretValue,
    expires_at: Option<SystemTime>,
}

pub struct TokenManager {
    store: Arc<dyn CredentialStore>,
    client: Option<Client>,
    token_url: String,
    environment_token: Option<Arc<SecretValue>>,
    cache: Mutex<BTreeMap<String, CachedToken>>,
}

impl fmt::Debug for TokenManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenManager")
            .field(
                "environment_token",
                &self.environment_token.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "cached_profiles",
                &self.cache.try_lock().ok().map_or(0, |cache| cache.len()),
            )
            .finish()
    }
}

impl TokenManager {
    pub fn new(store: Arc<dyn CredentialStore>, environment_token: Option<String>) -> Self {
        Self::new_with_override(
            store,
            environment_token.map(|value| Arc::new(SecretValue::new(value))),
        )
    }

    pub(crate) fn new_with_override(
        store: Arc<dyn CredentialStore>,
        environment_token: Option<Arc<SecretValue>>,
    ) -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .ok();
        Self {
            store,
            client,
            token_url: "https://api.tailscale.com/api/v2/oauth/token".to_owned(),
            environment_token,
            cache: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn with_client(
        store: Arc<dyn CredentialStore>,
        environment_token: Option<String>,
        client: Client,
        token_url: impl Into<String>,
    ) -> Self {
        Self {
            store,
            client: Some(client),
            token_url: token_url.into(),
            environment_token: environment_token.map(|value| Arc::new(SecretValue::new(value))),
            cache: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn credential_status(
        &self,
        reference: &str,
    ) -> Result<Option<CredentialStatus>, AuthError> {
        let value = self.store.get(reference)?;
        let Some(value) = value else {
            return Ok(None);
        };
        let record = decode_record(value.as_str())?;
        let requested_scopes = match &record {
            CredentialRecord::OAuthClient(record) => record.requested_scopes.clone(),
            CredentialRecord::AccessToken(_) => Vec::new(),
        };
        Ok(Some(CredentialStatus {
            kind: record.kind(),
            requested_scopes,
            keyring_available: true,
        }))
    }

    pub async fn access_token(
        &self,
        profile: &str,
        reference: &str,
    ) -> Result<AccessToken, AuthError> {
        let mut cache = self.cache.lock().await;
        if let Some(environment_token) = self.environment_token.as_ref() {
            if environment_token.is_empty() {
                return Err(AuthError::Unauthenticated);
            }
            return Ok(AccessToken(SecretValue::new(environment_token.as_str())));
        }
        if let Some(cached) = cache.get(profile)
            && token_is_fresh(cached.expires_at)
        {
            return Ok(AccessToken(SecretValue::new(cached.value.as_str())));
        }
        let encoded = self.store.get(reference)?;
        let Some(encoded) = encoded else {
            return Err(AuthError::Unauthenticated);
        };
        let record = decode_record(encoded.as_str())?;
        match record {
            CredentialRecord::AccessToken(record) => {
                cache.insert(
                    profile.to_owned(),
                    CachedToken {
                        value: SecretValue::new(record.access_token.as_str()),
                        expires_at: None,
                    },
                );
                Ok(AccessToken(record.access_token))
            }
            CredentialRecord::OAuthClient(record) => {
                validate_requested_scopes(&record.requested_scopes)?;
                let response = self.exchange(&record).await?;
                let expires_at = SystemTime::now()
                    .checked_add(Duration::from_secs(response.expires_in))
                    .ok_or(AuthError::ClockAnomaly)?;
                cache.insert(
                    profile.to_owned(),
                    CachedToken {
                        value: SecretValue::new(response.access_token.as_str()),
                        expires_at: Some(expires_at),
                    },
                );
                Ok(AccessToken(response.access_token))
            }
        }
    }

    pub async fn clear_profile(&self, profile: &str) {
        self.cache.lock().await.remove(profile);
    }

    pub async fn clear_all(&self) {
        self.cache.lock().await.clear();
    }

    pub async fn refresh_after_unauthenticated(
        &self,
        profile: &str,
        reference: &str,
        original: &AccessToken,
    ) -> Result<Option<AccessToken>, AuthError> {
        if self.environment_token.is_some() {
            return Ok(None);
        }
        let mut cache = self.cache.lock().await;
        let Some(cached) = cache.get(profile) else {
            return Ok(None);
        };
        if cached.value.as_str() != original.as_str() {
            return Ok(Some(AccessToken(SecretValue::new(cached.value.as_str()))));
        }
        if token_is_fresh(cached.expires_at) {
            return Ok(None);
        }
        let encoded = self.store.get(reference)?;
        let Some(encoded) = encoded else {
            return Err(AuthError::Unauthenticated);
        };
        let record = decode_record(encoded.as_str())?;
        let CredentialRecord::OAuthClient(record) = record else {
            return Ok(None);
        };
        let response = self.exchange(&record).await?;
        let expires_at = SystemTime::now()
            .checked_add(Duration::from_secs(response.expires_in))
            .ok_or(AuthError::ClockAnomaly)?;
        let refreshed = AccessToken(SecretValue::new(response.access_token.as_str()));
        cache.insert(
            profile.to_owned(),
            CachedToken {
                value: SecretValue::new(response.access_token.as_str()),
                expires_at: Some(expires_at),
            },
        );
        Ok(Some(refreshed))
    }

    async fn exchange(&self, record: &OAuthClientRecord) -> Result<OAuthResponse, AuthError> {
        let client = self.client.as_ref().ok_or(AuthError::Configuration)?;
        let scopes = record.requested_scopes.join(" ");
        let response = client
            .post(&self.token_url)
            .header(reqwest::header::USER_AGENT, crate::VERSION_USER_AGENT)
            .form(&[
                ("client_id", record.client_id.as_str()),
                ("client_secret", record.client_secret.as_str()),
                ("grant_type", "client_credentials"),
                ("scope", scopes.as_str()),
            ])
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    AuthError::TimedOut
                } else {
                    AuthError::Transport
                }
            })?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let mut body = Zeroizing::new(Vec::new());
        let mut response = response;
        while let Some(chunk) = response.chunk().await.map_err(|_| AuthError::Transport)? {
            if body.len().saturating_add(chunk.len()) > 64 * 1024 {
                return Err(AuthError::MalformedToken);
            }
            body.extend_from_slice(&chunk);
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AuthError::InvalidClient);
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(AuthError::ScopeDenied);
        }
        if status == reqwest::StatusCode::BAD_REQUEST {
            let error_code = serde_json::from_slice::<OAuthErrorBody>(&body)
                .ok()
                .and_then(|body| body.error);
            return Err(match error_code.as_deref() {
                Some("invalid_client") => AuthError::InvalidClient,
                Some("invalid_scope") | Some("unauthorized_client") => AuthError::ScopeDenied,
                _ => AuthError::Transport,
            });
        }
        if !status.is_success() {
            return Err(AuthError::Transport);
        }
        if status != reqwest::StatusCode::OK {
            return Err(AuthError::MalformedToken);
        }
        if !content_type.is_some_and(|value| {
            let value = value.to_ascii_lowercase();
            value.starts_with("application/json") || value.starts_with("application/problem+json")
        }) {
            return Err(AuthError::MalformedToken);
        }
        let payload = serde_json::from_slice::<OAuthResponseBody>(&body)
            .map_err(|_| AuthError::MalformedToken)?;
        if payload.access_token.is_empty()
            || payload.expires_in == 0
            || !payload
                .token_type
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("bearer"))
        {
            return Err(AuthError::MalformedToken);
        }
        Ok(OAuthResponse {
            access_token: SecretValue::new(payload.access_token),
            expires_in: payload.expires_in,
        })
    }
}

pub async fn validate_record(
    profile: &str,
    tailnet: &str,
    record: &CredentialRecord,
) -> Result<(), AuthError> {
    let encoded = encode_record(record)?;
    let store = Arc::new(MemoryCredentialStore::default());
    store.set("validation", encoded.as_str())?;
    let manager = TokenManager::new(store, None);
    let token = manager.access_token(profile, "validation").await?;
    let client = AdminClient::new(Duration::from_secs(15)).map_err(map_admin_error)?;
    probe_record(&client, &token, tailnet, Some(record)).await
}

pub async fn live_check(
    profile: &str,
    tailnet: &str,
    reference: &str,
    store: Arc<dyn CredentialStore>,
) -> Result<CredentialKind, AuthError> {
    live_check_with_override(profile, tailnet, reference, store, None).await
}

pub async fn live_check_with_override(
    profile: &str,
    tailnet: &str,
    reference: &str,
    store: Arc<dyn CredentialStore>,
    environment_token: Option<String>,
) -> Result<CredentialKind, AuthError> {
    let status = if environment_token.is_some() {
        None
    } else {
        store
            .get(reference)?
            .map(|value| decode_record(value.as_str()))
            .transpose()?
    };
    let kind = status
        .as_ref()
        .map_or(CredentialKind::AccessToken, CredentialRecord::kind);
    let uses_keyring = environment_token.is_none();
    let manager = TokenManager::new(store, environment_token);
    let token = manager.access_token(profile, reference).await?;
    let client = AdminClient::new(Duration::from_secs(15)).map_err(map_admin_error)?;
    probe_record(
        &client,
        &token,
        tailnet,
        uses_keyring.then_some(status.as_ref()).flatten(),
    )
    .await
    .map(|_| kind)
}

pub fn validate_requested_scopes(scopes: &[String]) -> Result<(), AuthError> {
    if scopes.is_empty() || scopes.iter().any(|scope| !is_supported_read_scope(scope)) {
        return Err(AuthError::InvalidScopes);
    }
    Ok(())
}

fn is_supported_read_scope(scope: &str) -> bool {
    matches!(
        scope,
        "devices:core:read"
            | "devices:posture_attributes:read"
            | "devices:routes:read"
            | "users:read"
            | "dns:read"
            | "policy_file:read"
            | "auth_keys:read"
            | "api_access_tokens:read"
            | "oauth_keys:read"
            | "federated_keys:read"
            | "policy_file"
            | "auth_keys"
            | "api_access_tokens"
            | "oauth_keys"
            | "federated_keys"
            | "policy_file:write"
            | "auth_keys:write"
            | "api_access_tokens:write"
            | "oauth_keys:write"
            | "federated_keys:write"
            | "feature_settings:read"
            | "account_settings:read"
            | "logs:configuration:read"
    )
}

async fn probe_record(
    client: &AdminClient,
    token: &AccessToken,
    tailnet: &str,
    record: Option<&CredentialRecord>,
) -> Result<(), AuthError> {
    let probe = match record {
        Some(CredentialRecord::OAuthClient(record)) => {
            validate_requested_scopes(&record.requested_scopes)?;
            if has_scope(&record.requested_scopes, "devices:core:read") {
                Probe::Devices
            } else if has_scope(&record.requested_scopes, "users:read") {
                Probe::Users
            } else if has_scope(&record.requested_scopes, "dns:read") {
                Probe::Nameservers
            } else if has_any_scope(
                &record.requested_scopes,
                &["policy_file:read", "policy_file"],
            ) {
                Probe::Policy
            } else if record.requested_scopes.iter().any(|scope| {
                matches!(
                    scope.as_str(),
                    "auth_keys:read"
                        | "auth_keys"
                        | "api_access_tokens:read"
                        | "api_access_tokens"
                        | "oauth_keys:read"
                        | "oauth_keys"
                        | "federated_keys:read"
                        | "federated_keys"
                )
            }) {
                Probe::Credentials
            } else if has_scope(&record.requested_scopes, "feature_settings:read") {
                Probe::Settings
            } else if has_scope(&record.requested_scopes, "account_settings:read") {
                Probe::Contacts
            } else if has_scope(&record.requested_scopes, "logs:configuration:read") {
                Probe::Audit
            } else {
                return Err(AuthError::Unsupported);
            }
        }
        Some(CredentialRecord::AccessToken(_)) | None => Probe::Devices,
    };
    match probe {
        Probe::Devices => client
            .list_devices(token, tailnet)
            .await
            .map(|_| ())
            .map_err(map_admin_error),
        Probe::Users => client
            .list_users(token, tailnet)
            .await
            .map(|_| ())
            .map_err(map_admin_error),
        Probe::Nameservers => client
            .get_nameservers(token, tailnet)
            .await
            .map(|_| ())
            .map_err(map_admin_error),
        Probe::Policy => client
            .get_policy(token, tailnet)
            .await
            .map(|_| ())
            .map_err(map_admin_error),
        Probe::Credentials => client
            .list_keys(token, tailnet)
            .await
            .map(|_| ())
            .map_err(map_admin_error),
        Probe::Settings => client
            .get_settings(token, tailnet)
            .await
            .map(|_| ())
            .map_err(map_admin_error),
        Probe::Contacts => client
            .get_contacts(token, tailnet)
            .await
            .map(|_| ())
            .map_err(map_admin_error),
        Probe::Audit => {
            let end = utc_now()?;
            let start = utc_seconds_ago(utc_timestamp()?, 24 * 60 * 60)?;
            client
                .get_audit(token, tailnet, &start, &end)
                .await
                .map(|_| ())
                .map_err(map_admin_error)
        }
    }
}

#[derive(Clone, Copy)]
enum Probe {
    Devices,
    Users,
    Nameservers,
    Policy,
    Credentials,
    Settings,
    Contacts,
    Audit,
}

fn has_scope(scopes: &[String], expected: &str) -> bool {
    scopes.iter().any(|scope| scope == expected)
}

fn has_any_scope(scopes: &[String], expected: &[&str]) -> bool {
    expected.iter().any(|value| has_scope(scopes, value))
}

fn utc_timestamp() -> Result<u64, AuthError> {
    u64::try_from(time::OffsetDateTime::now_utc().unix_timestamp())
        .map_err(|_| AuthError::ClockAnomaly)
}

fn utc_now() -> Result<String, AuthError> {
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|_| AuthError::ClockAnomaly)
}

fn utc_seconds_ago(timestamp: u64, seconds: u64) -> Result<String, AuthError> {
    let timestamp = timestamp.saturating_sub(seconds);
    let timestamp = i64::try_from(timestamp).map_err(|_| AuthError::ClockAnomaly)?;
    time::OffsetDateTime::from_unix_timestamp(timestamp)
        .map_err(|_| AuthError::ClockAnomaly)?
        .format(&Rfc3339)
        .map_err(|_| AuthError::ClockAnomaly)
}

fn map_admin_error(error: AdminError) -> AuthError {
    match error {
        AdminError::Unauthenticated => AuthError::Unauthenticated,
        AdminError::TimedOut { .. } => AuthError::TimedOut,
        AdminError::Cancelled { .. } => AuthError::Cancelled,
        AdminError::Transport { .. } => AuthError::Transport,
        AdminError::Forbidden { .. } => AuthError::ScopeDenied,
        AdminError::PlanRestricted { .. } => AuthError::Unsupported,
        AdminError::DecodeFailed { .. }
        | AdminError::BodyTooLarge { .. }
        | AdminError::UnexpectedStatus { .. }
        | AdminError::NotFound { .. }
        | AdminError::ValidationFailed { .. }
        | AdminError::Conflict { .. }
        | AdminError::RateLimited { .. }
        | AdminError::ServerFailure { .. }
        | AdminError::Unsupported { .. } => AuthError::Unsupported,
    }
}

fn token_is_fresh(expires_at: Option<SystemTime>) -> bool {
    match expires_at {
        None => true,
        Some(expires_at) => match expires_at.duration_since(SystemTime::now()) {
            Ok(remaining) => remaining > REFRESH_LEAD,
            Err(_) => false,
        },
    }
}

#[derive(Deserialize)]
struct OAuthResponseBody {
    access_token: String,
    token_type: Option<String>,
    expires_in: u64,
}

#[derive(Deserialize)]
struct OAuthErrorBody {
    error: Option<String>,
}

struct OAuthResponse {
    access_token: SecretValue,
    expires_in: u64,
}
