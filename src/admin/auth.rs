use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use super::client::{AdminClient, AdminError};

// Secret material and its storage live in `crate::secrets`; they are re-exported here so
// callers keep a single import for "the credential a profile authenticates with".
pub use crate::secrets::{
    AccessTokenRecord, CredentialBackend, CredentialKind, CredentialRecord, CredentialStore,
    FileCredentialStore, MemoryCredentialStore, OAuthClientRecord, SecretValue, SecretsError,
};

const REFRESH_LEAD: Duration = Duration::from_secs(300);

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("no credential is stored for this profile")]
    Unauthenticated,
    #[error("the credential was rejected by the Tailscale API")]
    Rejected,
    #[error("the tailnet was not found; check the tailnet ID")]
    TailnetNotFound,
    #[error("this tailnet's plan does not permit the read used to validate a credential")]
    PlanRestricted,
    #[error("the Tailscale API rate limit was reached; retry shortly")]
    RateLimited,
    #[error("the Tailscale API failed while validating the credential")]
    ServerFailure,
    #[error("the Tailscale API rejected the validation request as invalid")]
    ApiRejected,
    #[error("the Tailscale API response could not be decoded")]
    MalformedResponse,
    #[error("the requested scopes include no read that can validate them")]
    NoValidationProbe,
    #[error("the OAuth client was rejected")]
    InvalidClient,
    #[error("the requested OAuth scope was denied")]
    ScopeDenied,
    #[error("the OAuth token response was malformed")]
    MalformedToken,
    #[error("the system clock is earlier than the token issue time")]
    ClockAnomaly,
    #[error("{0}")]
    Store(#[from] SecretsError),
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
    #[error("the Tailscale API does not support the capability used for validation")]
    Unsupported,
    #[error("OAuth scopes must be narrow read scopes")]
    InvalidScopes,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CredentialStatus {
    pub kind: CredentialKind,
    pub requested_scopes: Vec<String>,
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
    cache: Mutex<BTreeMap<String, CachedToken>>,
    /// Credential metadata keyed by store reference, remembered from the record
    /// `access_token` already read so that describing a credential does not cost a
    /// second trip to the backend.
    statuses: std::sync::Mutex<BTreeMap<String, CredentialStatus>>,
}

impl fmt::Debug for TokenManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenManager")
            .field(
                "cached_profiles",
                &self.cache.try_lock().ok().map_or(0, |cache| cache.len()),
            )
            .finish()
    }
}

impl TokenManager {
    pub fn new(store: Arc<dyn CredentialStore>) -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .ok();
        Self {
            store,
            client,
            token_url: "https://api.tailscale.com/api/v2/oauth/token".to_owned(),
            cache: Mutex::new(BTreeMap::new()),
            statuses: std::sync::Mutex::new(BTreeMap::new()),
        }
    }

    pub fn with_client(
        store: Arc<dyn CredentialStore>,
        client: Client,
        token_url: impl Into<String>,
    ) -> Self {
        Self {
            store,
            client: Some(client),
            token_url: token_url.into(),
            cache: Mutex::new(BTreeMap::new()),
            statuses: std::sync::Mutex::new(BTreeMap::new()),
        }
    }

    fn status_of(record: &CredentialRecord) -> CredentialStatus {
        CredentialStatus {
            kind: record.kind(),
            requested_scopes: record.requested_scopes(),
        }
    }

    fn remember_status(&self, reference: &str, record: &CredentialRecord) {
        if let Ok(mut statuses) = self.statuses.lock() {
            statuses.insert(reference.to_owned(), Self::status_of(record));
        }
    }

    pub fn credential_status(
        &self,
        reference: &str,
    ) -> Result<Option<CredentialStatus>, AuthError> {
        if let Ok(statuses) = self.statuses.lock()
            && let Some(status) = statuses.get(reference)
        {
            return Ok(Some(status.clone()));
        }
        let Some(record) = self.store.get(reference)? else {
            return Ok(None);
        };
        self.remember_status(reference, &record);
        Ok(Some(Self::status_of(&record)))
    }

    pub async fn access_token(
        &self,
        profile: &str,
        reference: &str,
    ) -> Result<AccessToken, AuthError> {
        let mut cache = self.cache.lock().await;
        if let Some(cached) = cache.get(profile)
            && token_is_fresh(cached.expires_at)
        {
            return Ok(AccessToken(SecretValue::new(cached.value.as_str())));
        }
        let Some(record) = self.store.get(reference)? else {
            return Err(AuthError::Unauthenticated);
        };
        self.remember_status(reference, &record);
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
        self.clear_statuses();
    }

    pub async fn clear_all(&self) {
        self.cache.lock().await.clear();
        self.clear_statuses();
    }

    /// Drops every remembered status. The token cache is keyed by profile and the status
    /// cache by store reference, so a profile-scoped eviction cannot target one entry;
    /// discarding all of them only costs a re-read.
    fn clear_statuses(&self) {
        if let Ok(mut statuses) = self.statuses.lock() {
            statuses.clear();
        }
    }

    pub async fn refresh_after_unauthenticated(
        &self,
        profile: &str,
        reference: &str,
        original: &AccessToken,
    ) -> Result<Option<AccessToken>, AuthError> {
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
        let Some(record) = self.store.get(reference)? else {
            return Err(AuthError::Unauthenticated);
        };
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
    let store = Arc::new(MemoryCredentialStore::default());
    store.set("validation", record)?;
    let manager = TokenManager::new(store);
    let token = manager.access_token(profile, "validation").await?;
    let client = AdminClient::new(Duration::from_secs(15)).map_err(map_admin_error)?;
    probe_record(&client, &token, tailnet, Some(record)).await
}

pub async fn live_check(
    profile: &str,
    tailnet: &str,
    reference: &str,
    store: Arc<dyn CredentialStore>,
    timeout: Duration,
) -> Result<CredentialKind, AuthError> {
    let record = store.get(reference)?;
    let kind = record
        .as_ref()
        .map_or(CredentialKind::AccessToken, CredentialRecord::kind);
    let manager = TokenManager::new(Arc::clone(&store));
    let token = manager.access_token(profile, reference).await?;
    let client = AdminClient::new(timeout).map_err(map_admin_error)?;
    probe_record(&client, &token, tailnet, record.as_ref())
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
                return Err(AuthError::NoValidationProbe);
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

/// The admin client already distinguishes these; collapsing them here would throw away
/// the only signal that tells a user whether to fix the tailnet, the credential, or
/// nothing at all and retry.
fn map_admin_error(error: AdminError) -> AuthError {
    match error {
        AdminError::Unauthenticated => AuthError::Rejected,
        AdminError::TimedOut { .. } => AuthError::TimedOut,
        AdminError::Cancelled { .. } => AuthError::Cancelled,
        AdminError::Transport { .. } => AuthError::Transport,
        AdminError::Forbidden { .. } => AuthError::ScopeDenied,
        AdminError::PlanRestricted { .. } => AuthError::PlanRestricted,
        AdminError::NotFound { .. } => AuthError::TailnetNotFound,
        AdminError::RateLimited { .. } => AuthError::RateLimited,
        AdminError::ServerFailure { .. } | AdminError::UnexpectedStatus { .. } => {
            AuthError::ServerFailure
        }
        AdminError::ValidationFailed { .. } | AdminError::Conflict { .. } => AuthError::ApiRejected,
        AdminError::DecodeFailed { .. } | AdminError::BodyTooLarge { .. } => {
            AuthError::MalformedResponse
        }
        AdminError::Unsupported { .. } => AuthError::Unsupported,
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

#[cfg(test)]
mod tests {
    use super::{AuthError, map_admin_error};
    use crate::admin::client::AdminError;

    /// Every one of these used to render as "the requested scope has no verified validation
    /// read", which named neither the cause nor anything the reader could act on.
    #[test]
    fn each_api_failure_keeps_a_distinct_actionable_message() {
        let operation = || "read devices".to_owned();
        let detail = || "fictional detail".to_owned();
        let cases = [
            (
                AdminError::NotFound {
                    operation: operation(),
                    detail: detail(),
                },
                "tailnet was not found",
            ),
            (
                AdminError::PlanRestricted {
                    operation: operation(),
                    detail: detail(),
                },
                "plan does not permit",
            ),
            (
                AdminError::RateLimited {
                    operation: operation(),
                    retry_after_seconds: None,
                    detail: detail(),
                },
                "rate limit",
            ),
            (
                AdminError::ServerFailure {
                    operation: operation(),
                    detail: detail(),
                },
                "failed while validating",
            ),
            (
                AdminError::ValidationFailed {
                    operation: operation(),
                    detail: detail(),
                },
                "rejected the validation request",
            ),
            (
                AdminError::DecodeFailed {
                    operation: operation(),
                    detail: detail(),
                },
                "could not be decoded",
            ),
            (AdminError::Unauthenticated, "rejected by the Tailscale API"),
        ];

        let mut seen: Vec<String> = Vec::new();
        for (admin, expected) in cases {
            let message = map_admin_error(admin).to_string();
            assert!(
                message.contains(expected),
                "expected {expected:?} in {message:?}"
            );
            assert!(
                !seen.contains(&message),
                "two causes collapsed onto {message:?}"
            );
            seen.push(message);
        }
    }

    /// A stored-but-absent credential and one the API refused are different problems and
    /// must not share a message.
    #[test]
    fn a_missing_credential_reads_differently_from_a_refused_one() {
        assert_ne!(
            AuthError::Unauthenticated.to_string(),
            AuthError::Rejected.to_string()
        );
        assert!(
            AuthError::Unauthenticated
                .to_string()
                .contains("no credential is stored")
        );
    }
}
