use std::time::{Duration, SystemTime};

use reqwest::header::{
    ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue, RETRY_AFTER, USER_AGENT,
};
use reqwest::{Client, Method, StatusCode, Url};
use serde::de::DeserializeOwned;
use thiserror::Error;

use super::auth::AccessToken;
use super::dto::{
    AuditResponse, ContactsResponse, DeviceDto, DevicePostureAttributesDto, DeviceRoutesDto,
    DevicesResponse, DnsPreferencesDto, KeyDto, KeysResponse, NameserversResponse,
    PolicyPreviewDto, PolicyValidationDto, SearchPathsDto, SettingsDto, UserDto, UsersResponse,
};
use super::key_mutations::AuthKeyCreateRequest;
use crate::domain::Timestamp;
use crate::domain::policy_workflow::PolicySelectorType;

pub const API_ORIGIN: &str = "https://api.tailscale.com";
const API_PREFIX: &str = "/api/v2";
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_RETRIES: usize = 2;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Endpoint {
    Devices,
    Device,
    DeviceDelete,
    DeviceAuthorized,
    DeviceExpire,
    DeviceKey,
    DeviceName,
    DeviceTags,
    Posture,
    Routes,
    DeviceRoutesSet,
    Users,
    User,
    UserApprove,
    UserRole,
    UserSuspend,
    UserRestore,
    UserDelete,
    Nameservers,
    NameserversSet,
    DnsPreferences,
    DnsPreferencesSet,
    SearchPaths,
    SearchPathsSet,
    SplitDns,
    SplitDnsPatch,
    Policy,
    PolicyValidate,
    PolicyPreview,
    PolicySave,
    CredentialList,
    CredentialDetail,
    CredentialCreate,
    CredentialRevoke,
    Settings,
    Contacts,
    Audit,
}

impl Endpoint {
    pub const fn operation(self) -> &'static str {
        match self {
            Self::Devices => "list devices",
            Self::Device => "get device",
            Self::DeviceDelete => "delete device",
            Self::DeviceAuthorized => "set device approval",
            Self::DeviceExpire => "expire device key",
            Self::DeviceKey => "configure device key expiry",
            Self::DeviceName => "rename device",
            Self::DeviceTags => "replace device tags",
            Self::Posture => "get device posture",
            Self::Routes => "get device routes",
            Self::DeviceRoutesSet => "replace device route approvals",
            Self::Users => "list users",
            Self::User => "get user",
            Self::UserApprove => "approve user",
            Self::UserRole => "change user role",
            Self::UserSuspend => "suspend user",
            Self::UserRestore => "restore user",
            Self::UserDelete => "delete user",
            Self::Nameservers => "get DNS nameservers",
            Self::NameserversSet => "replace DNS nameservers",
            Self::DnsPreferences => "get DNS preferences",
            Self::DnsPreferencesSet => "edit DNS preferences",
            Self::SearchPaths => "get DNS search paths",
            Self::SearchPathsSet => "replace DNS search paths",
            Self::SplitDns => "get split DNS",
            Self::SplitDnsPatch => "edit split DNS",
            Self::Policy => "get policy source",
            Self::PolicyValidate => "validate policy candidate",
            Self::PolicyPreview => "preview policy permissions",
            Self::PolicySave => "save policy source",
            Self::CredentialList => "list credential metadata",
            Self::CredentialDetail => "get credential metadata",
            Self::CredentialCreate => "create auth key",
            Self::CredentialRevoke => "revoke credential",
            Self::Settings => "get tailnet settings",
            Self::Contacts => "get tailnet contacts",
            Self::Audit => "get configuration audit",
        }
    }

    pub const fn required_scope(self) -> &'static str {
        match self {
            Self::Devices | Self::Device => "devices:core:read",
            Self::DeviceDelete
            | Self::DeviceAuthorized
            | Self::DeviceExpire
            | Self::DeviceKey
            | Self::DeviceName
            | Self::DeviceTags => "devices:core",
            Self::Posture => "devices:posture_attributes:read",
            Self::Routes => "devices:routes:read",
            Self::DeviceRoutesSet => "devices:routes",
            Self::Users => "users:read",
            Self::User => "users:read",
            Self::UserApprove
            | Self::UserRole
            | Self::UserSuspend
            | Self::UserRestore
            | Self::UserDelete => "users",
            Self::Nameservers | Self::DnsPreferences | Self::SearchPaths | Self::SplitDns => {
                "dns:read"
            }
            Self::NameserversSet
            | Self::DnsPreferencesSet
            | Self::SearchPathsSet
            | Self::SplitDnsPatch => "dns",
            Self::Policy => "policy_file:read",
            Self::PolicyValidate | Self::PolicyPreview => "policy_file:read",
            Self::PolicySave => "policy_file",
            Self::CredentialList | Self::CredentialDetail => "credential-specific read scope",
            Self::CredentialCreate | Self::CredentialRevoke => "credential-specific write scope",
            Self::Settings => "feature_settings:read",
            Self::Contacts => "account_settings:read",
            Self::Audit => "logs:configuration:read",
        }
    }

    const fn retry_safe(self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RateLimit {
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub reset_at: Option<u64>,
    pub retry_after_seconds: Option<u64>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResponseMeta {
    pub request_id: Option<String>,
    pub observed_at: Timestamp,
    pub status: u16,
    pub rate_limit: Option<RateLimit>,
    pub page_count: u32,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ApiResponse<T> {
    pub value: T,
    pub meta: ResponseMeta,
}

pub type MutationResponse<T> = ApiResponse<T>;

#[derive(Clone, Eq, PartialEq)]
pub struct PolicyBody {
    pub source_bytes: Vec<u8>,
    pub content_type: String,
    pub etag: Option<String>,
}

impl std::fmt::Debug for PolicyBody {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PolicyBody")
            .field(
                "source_bytes",
                &format_args!("<{} bytes>", self.source_bytes.len()),
            )
            .field("content_type", &self.content_type)
            .field("etag", &self.etag)
            .finish()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct MutationBody {
    source_bytes: Vec<u8>,
    content_type: String,
}

#[derive(Clone, Copy)]
struct RawMediaTypes<'a> {
    content_type: &'a str,
    accept: &'a str,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum AdminError {
    #[error("admin authentication is required")]
    Unauthenticated,
    #[error("the selected credential is forbidden for {operation}")]
    Forbidden { operation: String, detail: String },
    #[error("the selected tailnet plan does not permit {operation}")]
    PlanRestricted { operation: String, detail: String },
    #[error("the requested admin resource was not found")]
    NotFound { operation: String, detail: String },
    #[error("the admin request was rejected as invalid")]
    ValidationFailed { operation: String, detail: String },
    #[error("the admin request conflicted with current state")]
    Conflict { operation: String, detail: String },
    #[error("the admin service rate limit was reached")]
    RateLimited {
        operation: String,
        retry_after_seconds: Option<u64>,
        detail: String,
    },
    #[error("the admin service failed while reading the resource")]
    ServerFailure { operation: String, detail: String },
    #[error("the admin transport failed")]
    Transport { operation: String, detail: String },
    #[error("the admin request timed out")]
    TimedOut { operation: String },
    #[error("the admin request was cancelled")]
    Cancelled { operation: String },
    #[error("the admin service returned an unexpected status")]
    UnexpectedStatus {
        operation: String,
        status: u16,
        detail: String,
    },
    #[error("the admin response could not be decoded")]
    DecodeFailed { operation: String, detail: String },
    #[error("the admin response exceeded the byte limit")]
    BodyTooLarge { operation: String },
    #[error("the admin capability is not supported by the verified API contract")]
    Unsupported { operation: String, detail: String },
}

#[derive(Clone)]
pub struct AdminClient {
    http: Client,
    base_url: Url,
    request_timeout: Duration,
}

impl std::fmt::Debug for AdminClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdminClient")
            .field("base_url", &"<fixed origin>")
            .field("request_timeout", &self.request_timeout)
            .finish()
    }
}

impl AdminClient {
    pub fn new(request_timeout: Duration) -> Result<Self, AdminError> {
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(request_timeout)
            .build()
            .map_err(|_| AdminError::Transport {
                operation: "create HTTPS client".to_owned(),
                detail: "the HTTPS client could not be initialized".to_owned(),
            })?;
        let base_url = Url::parse(&format!("{API_ORIGIN}{API_PREFIX}")).map_err(|_| {
            AdminError::Transport {
                operation: "create HTTPS client".to_owned(),
                detail: "the fixed API origin is invalid".to_owned(),
            }
        })?;
        Ok(Self {
            http,
            base_url,
            request_timeout,
        })
    }

    /// Builds a client against a deterministic test server.
    #[doc(hidden)]
    pub fn with_base_url(base_url: Url, request_timeout: Duration) -> Result<Self, AdminError> {
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(request_timeout)
            .build()
            .map_err(|_| AdminError::Transport {
                operation: "create HTTPS test client".to_owned(),
                detail: "the HTTPS client could not be initialized".to_owned(),
            })?;
        Ok(Self {
            http,
            base_url,
            request_timeout,
        })
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub async fn list_devices(
        &self,
        token: &AccessToken,
        tailnet: &str,
    ) -> Result<ApiResponse<DevicesResponse>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "devices"], &[])?;
        self.json(Endpoint::Devices, token, url, None).await
    }

    pub async fn get_device(
        &self,
        token: &AccessToken,
        device_id: &str,
    ) -> Result<ApiResponse<DeviceDto>, AdminError> {
        let url = self.path(&["device", device_id], &[("fields", "all")])?;
        self.json(Endpoint::Device, token, url, None).await
    }

    pub async fn delete_device(
        &self,
        token: &AccessToken,
        device_id: &str,
    ) -> Result<MutationResponse<()>, AdminError> {
        let url = self.path(&["device", device_id], &[])?;
        self.mutation_empty(Endpoint::DeviceDelete, Method::DELETE, token, url, None)
            .await
    }

    pub async fn set_device_authorized(
        &self,
        token: &AccessToken,
        device_id: &str,
        authorized: bool,
    ) -> Result<MutationResponse<()>, AdminError> {
        let url = self.path(&["device", device_id, "authorized"], &[])?;
        self.mutation_empty(
            Endpoint::DeviceAuthorized,
            Method::POST,
            token,
            url,
            Some(serde_json::json!({"authorized": authorized})),
        )
        .await
    }

    pub async fn expire_device_key(
        &self,
        token: &AccessToken,
        device_id: &str,
    ) -> Result<MutationResponse<()>, AdminError> {
        let url = self.path(&["device", device_id, "expire"], &[])?;
        self.mutation_empty(Endpoint::DeviceExpire, Method::POST, token, url, None)
            .await
    }

    pub async fn set_device_key_expiry(
        &self,
        token: &AccessToken,
        device_id: &str,
        key_expiry_disabled: bool,
    ) -> Result<MutationResponse<()>, AdminError> {
        let url = self.path(&["device", device_id, "key"], &[])?;
        self.mutation_empty(
            Endpoint::DeviceKey,
            Method::POST,
            token,
            url,
            Some(serde_json::json!({"keyExpiryDisabled": key_expiry_disabled})),
        )
        .await
    }

    pub async fn set_device_name(
        &self,
        token: &AccessToken,
        device_id: &str,
        name: &str,
    ) -> Result<MutationResponse<()>, AdminError> {
        let url = self.path(&["device", device_id, "name"], &[])?;
        self.mutation_empty(
            Endpoint::DeviceName,
            Method::POST,
            token,
            url,
            Some(serde_json::json!({"name": name})),
        )
        .await
    }

    pub async fn set_device_tags(
        &self,
        token: &AccessToken,
        device_id: &str,
        tags: &[String],
    ) -> Result<MutationResponse<()>, AdminError> {
        let url = self.path(&["device", device_id, "tags"], &[])?;
        self.mutation_empty(
            Endpoint::DeviceTags,
            Method::POST,
            token,
            url,
            Some(serde_json::json!({"tags": tags})),
        )
        .await
    }

    pub async fn get_posture(
        &self,
        token: &AccessToken,
        device_id: &str,
    ) -> Result<ApiResponse<DevicePostureAttributesDto>, AdminError> {
        let url = self.path(&["device", device_id, "attributes"], &[])?;
        self.json(Endpoint::Posture, token, url, None).await
    }

    pub async fn get_routes(
        &self,
        token: &AccessToken,
        device_id: &str,
    ) -> Result<ApiResponse<DeviceRoutesDto>, AdminError> {
        let url = self.path(&["device", device_id, "routes"], &[])?;
        self.json(Endpoint::Routes, token, url, None).await
    }

    pub async fn set_device_routes(
        &self,
        token: &AccessToken,
        device_id: &str,
        routes: &[String],
    ) -> Result<MutationResponse<DeviceRoutesDto>, AdminError> {
        let url = self.path(&["device", device_id, "routes"], &[])?;
        self.mutation_json(
            Endpoint::DeviceRoutesSet,
            Method::POST,
            token,
            url,
            Some(serde_json::json!({"routes": routes})),
        )
        .await
    }

    pub async fn list_users(
        &self,
        token: &AccessToken,
        tailnet: &str,
    ) -> Result<ApiResponse<UsersResponse>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "users"], &[])?;
        self.json(Endpoint::Users, token, url, None).await
    }

    pub async fn get_user(
        &self,
        token: &AccessToken,
        user_id: &str,
    ) -> Result<ApiResponse<UserDto>, AdminError> {
        let url = self.path(&["users", user_id], &[])?;
        self.json(Endpoint::User, token, url, None).await
    }

    pub async fn approve_user(
        &self,
        token: &AccessToken,
        user_id: &str,
    ) -> Result<MutationResponse<()>, AdminError> {
        self.user_empty(Endpoint::UserApprove, token, user_id, "approve")
            .await
    }

    pub async fn set_user_role(
        &self,
        token: &AccessToken,
        user_id: &str,
        role: &str,
    ) -> Result<MutationResponse<()>, AdminError> {
        let url = self.path(&["users", user_id, "role"], &[])?;
        self.mutation_empty(
            Endpoint::UserRole,
            Method::POST,
            token,
            url,
            Some(serde_json::json!({"role": role})),
        )
        .await
    }

    pub async fn suspend_user(
        &self,
        token: &AccessToken,
        user_id: &str,
    ) -> Result<MutationResponse<()>, AdminError> {
        self.user_empty(Endpoint::UserSuspend, token, user_id, "suspend")
            .await
    }

    pub async fn restore_user(
        &self,
        token: &AccessToken,
        user_id: &str,
    ) -> Result<MutationResponse<()>, AdminError> {
        self.user_empty(Endpoint::UserRestore, token, user_id, "restore")
            .await
    }

    pub async fn delete_user(
        &self,
        token: &AccessToken,
        user_id: &str,
    ) -> Result<MutationResponse<()>, AdminError> {
        self.user_empty(Endpoint::UserDelete, token, user_id, "delete")
            .await
    }

    pub async fn get_nameservers(
        &self,
        token: &AccessToken,
        tailnet: &str,
    ) -> Result<ApiResponse<NameserversResponse>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "dns", "nameservers"], &[])?;
        self.json(Endpoint::Nameservers, token, url, None).await
    }

    pub async fn set_nameservers(
        &self,
        token: &AccessToken,
        tailnet: &str,
        dns: &[String],
    ) -> Result<MutationResponse<NameserversResponse>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "dns", "nameservers"], &[])?;
        self.mutation_json(
            Endpoint::NameserversSet,
            Method::POST,
            token,
            url,
            Some(serde_json::json!({"dns": dns})),
        )
        .await
    }

    pub async fn get_dns_preferences(
        &self,
        token: &AccessToken,
        tailnet: &str,
    ) -> Result<ApiResponse<DnsPreferencesDto>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "dns", "preferences"], &[])?;
        self.json(Endpoint::DnsPreferences, token, url, None).await
    }

    pub async fn set_dns_preferences(
        &self,
        token: &AccessToken,
        tailnet: &str,
        magic_dns: bool,
    ) -> Result<MutationResponse<DnsPreferencesDto>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "dns", "preferences"], &[])?;
        self.mutation_json(
            Endpoint::DnsPreferencesSet,
            Method::POST,
            token,
            url,
            Some(serde_json::json!({"magicDNS": magic_dns})),
        )
        .await
    }

    pub async fn get_search_paths(
        &self,
        token: &AccessToken,
        tailnet: &str,
    ) -> Result<ApiResponse<SearchPathsDto>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "dns", "searchpaths"], &[])?;
        self.json(Endpoint::SearchPaths, token, url, None).await
    }

    pub async fn set_search_paths(
        &self,
        token: &AccessToken,
        tailnet: &str,
        search_paths: &[String],
    ) -> Result<MutationResponse<SearchPathsDto>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "dns", "searchpaths"], &[])?;
        self.mutation_json(
            Endpoint::SearchPathsSet,
            Method::POST,
            token,
            url,
            Some(serde_json::json!({"searchPaths": search_paths})),
        )
        .await
    }

    pub async fn get_split_dns(
        &self,
        token: &AccessToken,
        tailnet: &str,
    ) -> Result<ApiResponse<serde_json::Map<String, serde_json::Value>>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "dns", "split-dns"], &[])?;
        self.json(Endpoint::SplitDns, token, url, None).await
    }

    pub async fn patch_split_dns(
        &self,
        token: &AccessToken,
        tailnet: &str,
        body: serde_json::Value,
    ) -> Result<MutationResponse<serde_json::Map<String, serde_json::Value>>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "dns", "split-dns"], &[])?;
        self.mutation_json(
            Endpoint::SplitDnsPatch,
            Method::PATCH,
            token,
            url,
            Some(body),
        )
        .await
    }

    pub async fn get_policy(
        &self,
        token: &AccessToken,
        tailnet: &str,
    ) -> Result<ApiResponse<PolicyBody>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "acl"], &[])?;
        self.bytes(
            Endpoint::Policy,
            token,
            url,
            Some("application/hujson"),
            true,
        )
        .await
    }

    pub async fn validate_policy(
        &self,
        token: &AccessToken,
        tailnet: &str,
        candidate: &[u8],
    ) -> Result<ApiResponse<PolicyValidationDto>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "acl", "validate"], &[])?;
        let response = self
            .mutation_raw(
                Endpoint::PolicyValidate,
                Method::POST,
                token,
                url,
                candidate,
                RawMediaTypes {
                    content_type: "application/hujson",
                    accept: "application/json",
                },
            )
            .await?;
        decode_json_mutation(response, Endpoint::PolicyValidate)
    }

    pub async fn preview_policy(
        &self,
        token: &AccessToken,
        tailnet: &str,
        selector_type: PolicySelectorType,
        selector: &str,
        candidate: &[u8],
    ) -> Result<ApiResponse<PolicyPreviewDto>, AdminError> {
        let url = self.path(
            &["tailnet", tailnet, "acl", "preview"],
            &[
                ("type", selector_type.api_value()),
                ("previewFor", selector),
            ],
        )?;
        let response = self
            .mutation_raw(
                Endpoint::PolicyPreview,
                Method::POST,
                token,
                url,
                candidate,
                RawMediaTypes {
                    content_type: "application/hujson",
                    accept: "application/json",
                },
            )
            .await?;
        decode_json_mutation(response, Endpoint::PolicyPreview)
    }

    pub async fn save_policy(
        &self,
        token: &AccessToken,
        tailnet: &str,
        candidate: &[u8],
    ) -> Result<MutationResponse<PolicyBody>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "acl"], &[])?;
        self.mutation_policy(
            Endpoint::PolicySave,
            token,
            url,
            candidate,
            "application/hujson",
            "application/hujson",
        )
        .await
    }

    pub async fn list_keys(
        &self,
        token: &AccessToken,
        tailnet: &str,
    ) -> Result<ApiResponse<KeysResponse>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "keys"], &[("all", "false")])?;
        self.json(Endpoint::CredentialList, token, url, None).await
    }

    pub async fn get_key(
        &self,
        token: &AccessToken,
        tailnet: &str,
        key_id: &str,
    ) -> Result<ApiResponse<KeyDto>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "keys", key_id], &[])?;
        self.json(Endpoint::CredentialDetail, token, url, None)
            .await
    }

    pub async fn create_auth_key(
        &self,
        token: &AccessToken,
        tailnet: &str,
        request: &AuthKeyCreateRequest,
    ) -> Result<MutationResponse<KeyDto>, AdminError> {
        let body = request
            .json_body()
            .map_err(|error| AdminError::ValidationFailed {
                operation: Endpoint::CredentialCreate.operation().to_owned(),
                detail: error.to_string(),
            })?;
        self.mutation_json(
            Endpoint::CredentialCreate,
            Method::POST,
            token,
            self.path(&["tailnet", tailnet, "keys"], &[])?,
            Some(body),
        )
        .await
    }

    pub async fn revoke_credential(
        &self,
        token: &AccessToken,
        tailnet: &str,
        key_id: &str,
    ) -> Result<MutationResponse<()>, AdminError> {
        self.mutation_empty(
            Endpoint::CredentialRevoke,
            Method::DELETE,
            token,
            self.path(&["tailnet", tailnet, "keys", key_id], &[])?,
            None,
        )
        .await
    }

    pub async fn get_settings(
        &self,
        token: &AccessToken,
        tailnet: &str,
    ) -> Result<ApiResponse<SettingsDto>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "settings"], &[])?;
        self.json(Endpoint::Settings, token, url, None).await
    }

    pub async fn get_contacts(
        &self,
        token: &AccessToken,
        tailnet: &str,
    ) -> Result<ApiResponse<ContactsResponse>, AdminError> {
        let url = self.path(&["tailnet", tailnet, "contacts"], &[])?;
        self.json(Endpoint::Contacts, token, url, None).await
    }

    pub async fn get_audit(
        &self,
        token: &AccessToken,
        tailnet: &str,
        start: &str,
        end: &str,
    ) -> Result<ApiResponse<AuditResponse>, AdminError> {
        let url = self.path(
            &["tailnet", tailnet, "logging", "configuration"],
            &[("start", start), ("end", end)],
        )?;
        self.json(Endpoint::Audit, token, url, None).await
    }

    fn path(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<Url, AdminError> {
        let mut url = self.base_url.clone();
        {
            let mut path = url.path_segments_mut().map_err(|_| AdminError::Transport {
                operation: "build admin URL".to_owned(),
                detail: "the API origin cannot accept path segments".to_owned(),
            })?;
            for segment in segments {
                path.push(segment);
            }
        }
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        Ok(url)
    }

    async fn json<T: DeserializeOwned>(
        &self,
        endpoint: Endpoint,
        token: &AccessToken,
        url: Url,
        accept: Option<&str>,
    ) -> Result<ApiResponse<T>, AdminError> {
        let response = self.bytes(endpoint, token, url, accept, false).await?;
        if !json_content_type(&response.value.content_type) {
            return Err(AdminError::DecodeFailed {
                operation: endpoint.operation().to_owned(),
                detail: "the response did not use a JSON content type".to_owned(),
            });
        }
        serde_json::from_slice::<T>(&response.value.source_bytes)
            .map(|value| ApiResponse {
                value,
                meta: response.meta,
            })
            .map_err(|error| AdminError::DecodeFailed {
                operation: endpoint.operation().to_owned(),
                detail: bounded_detail(&error.to_string()),
            })
    }

    async fn user_empty(
        &self,
        endpoint: Endpoint,
        token: &AccessToken,
        user_id: &str,
        operation: &str,
    ) -> Result<MutationResponse<()>, AdminError> {
        let url = self.path(&["users", user_id, operation], &[])?;
        self.mutation_empty(endpoint, Method::POST, token, url, None)
            .await
    }

    async fn mutation_json<T: DeserializeOwned>(
        &self,
        endpoint: Endpoint,
        method: Method,
        token: &AccessToken,
        url: Url,
        body: Option<serde_json::Value>,
    ) -> Result<MutationResponse<T>, AdminError> {
        let response = self
            .mutation_bytes(endpoint, method, token, url, body)
            .await?;
        if !json_content_type(&response.value.content_type) {
            return Err(AdminError::DecodeFailed {
                operation: endpoint.operation().to_owned(),
                detail: "the mutation response did not use a JSON content type".to_owned(),
            });
        }
        serde_json::from_slice::<T>(&response.value.source_bytes)
            .map(|value| ApiResponse {
                value,
                meta: response.meta,
            })
            .map_err(|error| AdminError::DecodeFailed {
                operation: endpoint.operation().to_owned(),
                detail: bounded_detail(&error.to_string()),
            })
    }

    async fn mutation_empty(
        &self,
        endpoint: Endpoint,
        method: Method,
        token: &AccessToken,
        url: Url,
        body: Option<serde_json::Value>,
    ) -> Result<MutationResponse<()>, AdminError> {
        let response = self
            .mutation_bytes(endpoint, method, token, url, body)
            .await?;
        if !response.value.source_bytes.is_empty() {
            return Err(AdminError::DecodeFailed {
                operation: endpoint.operation().to_owned(),
                detail: "the mutation response was documented as empty but returned bytes"
                    .to_owned(),
            });
        }
        Ok(ApiResponse {
            value: (),
            meta: response.meta,
        })
    }

    async fn mutation_bytes(
        &self,
        endpoint: Endpoint,
        method: Method,
        token: &AccessToken,
        url: Url,
        body: Option<serde_json::Value>,
    ) -> Result<ApiResponse<MutationBody>, AdminError> {
        let mut request = self
            .http
            .request(method, url)
            .header(AUTHORIZATION, format!("Bearer {}", token.as_str()))
            .header(USER_AGENT, crate::VERSION_USER_AGENT)
            .header(ACCEPT, "application/json");
        if let Some(body) = body {
            let bytes = serde_json::to_vec(&body).map_err(|error| AdminError::DecodeFailed {
                operation: endpoint.operation().to_owned(),
                detail: bounded_detail(&error.to_string()),
            })?;
            request = request.header(CONTENT_TYPE, "application/json").body(bytes);
        }
        let response = request.send().await.map_err(|error| {
            if error.is_timeout() {
                AdminError::TimedOut {
                    operation: endpoint.operation().to_owned(),
                }
            } else {
                AdminError::Transport {
                    operation: endpoint.operation().to_owned(),
                    detail: bounded_detail(&error.to_string()),
                }
            }
        })?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = read_bounded(response, endpoint.operation(), MAX_BODY_BYTES, token).await?;
        if status == StatusCode::OK {
            return Ok(ApiResponse {
                value: MutationBody {
                    source_bytes: body,
                    content_type: header_text(&headers, CONTENT_TYPE),
                },
                meta: response_meta(status, &headers),
            });
        }
        let detail = bounded_detail(&redact_body(&body, token.as_str()));
        Err(classify_status(
            endpoint,
            status,
            retry_after_seconds(&headers),
            detail,
        ))
    }

    async fn mutation_raw(
        &self,
        endpoint: Endpoint,
        method: Method,
        token: &AccessToken,
        url: Url,
        body: &[u8],
        media: RawMediaTypes<'_>,
    ) -> Result<ApiResponse<MutationBody>, AdminError> {
        let response = self
            .http
            .request(method, url)
            .header(AUTHORIZATION, format!("Bearer {}", token.as_str()))
            .header(USER_AGENT, crate::VERSION_USER_AGENT)
            .header(ACCEPT, media.accept)
            .header(CONTENT_TYPE, media.content_type)
            .body(body.to_vec())
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    AdminError::TimedOut {
                        operation: endpoint.operation().to_owned(),
                    }
                } else {
                    AdminError::Transport {
                        operation: endpoint.operation().to_owned(),
                        detail: bounded_detail(&error.to_string()),
                    }
                }
            })?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = read_bounded(response, endpoint.operation(), MAX_BODY_BYTES, token).await?;
        if status == StatusCode::OK {
            return Ok(ApiResponse {
                value: MutationBody {
                    source_bytes: body,
                    content_type: header_text(&headers, CONTENT_TYPE),
                },
                meta: response_meta(status, &headers),
            });
        }
        let detail = bounded_detail(&redact_body(&body, token.as_str()));
        Err(classify_status(
            endpoint,
            status,
            retry_after_seconds(&headers),
            detail,
        ))
    }

    async fn mutation_policy(
        &self,
        endpoint: Endpoint,
        token: &AccessToken,
        url: Url,
        body: &[u8],
        content_type: &str,
        accept: &str,
    ) -> Result<MutationResponse<PolicyBody>, AdminError> {
        let response = self
            .mutation_raw(
                endpoint,
                Method::POST,
                token,
                url,
                body,
                RawMediaTypes {
                    content_type,
                    accept,
                },
            )
            .await?;
        if !policy_content_type_string(&response.value.content_type) {
            return Err(AdminError::DecodeFailed {
                operation: endpoint.operation().to_owned(),
                detail: "the policy mutation response did not use HuJSON content type".to_owned(),
            });
        }
        Ok(ApiResponse {
            value: PolicyBody {
                source_bytes: response.value.source_bytes,
                content_type: response.value.content_type,
                etag: None,
            },
            meta: response.meta,
        })
    }

    async fn bytes(
        &self,
        endpoint: Endpoint,
        token: &AccessToken,
        url: Url,
        accept: Option<&str>,
        policy: bool,
    ) -> Result<ApiResponse<PolicyBody>, AdminError> {
        let mut attempt = 0usize;
        loop {
            let request = self
                .http
                .request(Method::GET, url.clone())
                .header(AUTHORIZATION, format!("Bearer {}", token.as_str()))
                .header(USER_AGENT, crate::VERSION_USER_AGENT)
                .header(ACCEPT, accept.unwrap_or("application/json"));
            let response = request.send().await.map_err(|error| {
                if error.is_timeout() {
                    AdminError::TimedOut {
                        operation: endpoint.operation().to_owned(),
                    }
                } else {
                    AdminError::Transport {
                        operation: endpoint.operation().to_owned(),
                        detail: bounded_detail(&error.to_string()),
                    }
                }
            });
            let response = match response {
                Ok(response) => response,
                Err(error)
                    if attempt < MAX_RETRIES
                        && endpoint.retry_safe()
                        && is_retryable_error(&error) =>
                {
                    sleep_before_retry(attempt, None).await;
                    attempt = attempt.saturating_add(1);
                    continue;
                }
                Err(error) => return Err(error),
            };
            let status = response.status();
            let headers = response.headers().clone();
            let body = read_bounded(response, endpoint.operation(), MAX_BODY_BYTES, token).await?;
            if status == StatusCode::OK {
                if policy && !policy_content_type(&headers) {
                    return Err(AdminError::DecodeFailed {
                        operation: endpoint.operation().to_owned(),
                        detail: "the policy response did not use HuJSON content type".to_owned(),
                    });
                }
                let meta = response_meta(status, &headers);
                return Ok(ApiResponse {
                    value: PolicyBody {
                        source_bytes: body,
                        content_type: header_text(&headers, CONTENT_TYPE),
                        etag: headers
                            .get(reqwest::header::ETAG)
                            .and_then(header_value)
                            .map(str::to_owned),
                    },
                    meta,
                });
            }
            let detail = bounded_detail(&redact_body(&body, token.as_str()));
            if status == StatusCode::TOO_MANY_REQUESTS
                && attempt < MAX_RETRIES
                && endpoint.retry_safe()
            {
                let retry_after = retry_after_seconds(&headers);
                sleep_before_retry(attempt, retry_after).await;
                attempt = attempt.saturating_add(1);
                continue;
            }
            if matches!(
                status,
                StatusCode::INTERNAL_SERVER_ERROR
                    | StatusCode::BAD_GATEWAY
                    | StatusCode::SERVICE_UNAVAILABLE
                    | StatusCode::GATEWAY_TIMEOUT
            ) && attempt < MAX_RETRIES
                && endpoint.retry_safe()
            {
                sleep_before_retry(attempt, retry_after_seconds(&headers)).await;
                attempt = attempt.saturating_add(1);
                continue;
            }
            return Err(classify_status(
                endpoint,
                status,
                retry_after_seconds(&headers),
                detail,
            ));
        }
    }
}

fn decode_json_mutation<T: DeserializeOwned>(
    response: ApiResponse<MutationBody>,
    endpoint: Endpoint,
) -> Result<ApiResponse<T>, AdminError> {
    if !json_content_type(&response.value.content_type) {
        return Err(AdminError::DecodeFailed {
            operation: endpoint.operation().to_owned(),
            detail: "the policy operation response did not use a JSON content type".to_owned(),
        });
    }
    serde_json::from_slice::<T>(&response.value.source_bytes)
        .map(|value| ApiResponse {
            value,
            meta: response.meta,
        })
        .map_err(|error| AdminError::DecodeFailed {
            operation: endpoint.operation().to_owned(),
            detail: bounded_detail(&error.to_string()),
        })
}

fn response_meta(status: StatusCode, headers: &HeaderMap) -> ResponseMeta {
    let rate_limit = rate_limit(headers);
    ResponseMeta {
        request_id: [
            "x-tailscale-request-id",
            "tailscale-request-id",
            "x-request-id",
        ]
        .iter()
        .find_map(|name| headers.get(*name).and_then(header_value))
        .map(str::to_owned),
        observed_at: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs()),
        status: status.as_u16(),
        rate_limit,
        page_count: 1,
    }
}

fn rate_limit(headers: &HeaderMap) -> Option<RateLimit> {
    let limit =
        header_u64(headers, "x-ratelimit-limit").or_else(|| header_u64(headers, "ratelimit-limit"));
    let remaining = header_u64(headers, "x-ratelimit-remaining")
        .or_else(|| header_u64(headers, "ratelimit-remaining"));
    let reset_at =
        header_u64(headers, "x-ratelimit-reset").or_else(|| header_u64(headers, "ratelimit-reset"));
    let retry_after_seconds = retry_after_seconds(headers);
    if limit.is_none() && remaining.is_none() && reset_at.is_none() && retry_after_seconds.is_none()
    {
        None
    } else {
        Some(RateLimit {
            limit,
            remaining,
            reset_at,
            retry_after_seconds,
        })
    }
}

fn retry_after_seconds(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(RETRY_AFTER)
        .and_then(header_value)
        .and_then(|value| value.parse::<u64>().ok())
}

fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers
        .get(name)
        .and_then(header_value)
        .and_then(|value| value.parse::<u64>().ok())
}

fn header_text(headers: &HeaderMap, name: reqwest::header::HeaderName) -> String {
    headers
        .get(name)
        .and_then(header_value)
        .map_or_else(String::new, str::to_owned)
}

fn header_value(value: &HeaderValue) -> Option<&str> {
    value.to_str().ok()
}

fn policy_content_type(headers: &HeaderMap) -> bool {
    policy_content_type_string(&header_text(headers, CONTENT_TYPE))
}

fn policy_content_type_string(content_type: &str) -> bool {
    let content_type = content_type.to_ascii_lowercase();
    content_type.starts_with("application/hujson")
}

fn json_content_type(content_type: &str) -> bool {
    let content_type = content_type.to_ascii_lowercase();
    content_type.starts_with("application/json")
        || content_type.starts_with("application/problem+json")
}

async fn read_bounded(
    mut response: reqwest::Response,
    operation: &str,
    limit: usize,
    token: &AccessToken,
) -> Result<Vec<u8>, AdminError> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| AdminError::Transport {
            operation: operation.to_owned(),
            detail: bounded_detail(&redact_body(error.to_string().as_bytes(), token.as_str())),
        })?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(AdminError::BodyTooLarge {
                operation: operation.to_owned(),
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn classify_status(
    endpoint: Endpoint,
    status: StatusCode,
    retry_after: Option<u64>,
    detail: String,
) -> AdminError {
    let operation = endpoint.operation().to_owned();
    match status {
        StatusCode::UNAUTHORIZED => AdminError::Unauthenticated,
        StatusCode::FORBIDDEN if plan_restricted(&detail) => {
            AdminError::PlanRestricted { operation, detail }
        }
        StatusCode::FORBIDDEN => AdminError::Forbidden { operation, detail },
        StatusCode::NOT_FOUND => AdminError::NotFound { operation, detail },
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => {
            AdminError::ValidationFailed { operation, detail }
        }
        StatusCode::CONFLICT => AdminError::Conflict { operation, detail },
        StatusCode::TOO_MANY_REQUESTS => AdminError::RateLimited {
            operation,
            retry_after_seconds: retry_after,
            detail,
        },
        status if status.is_server_error() => AdminError::ServerFailure { operation, detail },
        status => AdminError::UnexpectedStatus {
            operation,
            status: status.as_u16(),
            detail,
        },
    }
}

fn plan_restricted(detail: &str) -> bool {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(detail)
        && let Some(object) = value.as_object()
    {
        for key in ["code", "type", "error", "reason"] {
            if object
                .get(key)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| value.to_ascii_lowercase().contains("plan"))
            {
                return true;
            }
        }
    }
    detail.to_ascii_lowercase().contains("plan")
}

fn is_retryable_error(error: &AdminError) -> bool {
    matches!(
        error,
        AdminError::Transport { .. } | AdminError::TimedOut { .. }
    )
}

async fn sleep_before_retry(attempt: usize, retry_after: Option<u64>) {
    let milliseconds = retry_after.map_or_else(
        || {
            let fallback = 100u64.saturating_mul(1u64 << attempt.min(4));
            let jitter = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |duration| u64::from(duration.subsec_nanos()) % 101);
            fallback.saturating_add(jitter).min(5000)
        },
        |seconds| seconds.saturating_mul(1000).min(5000),
    );
    tokio::time::sleep(Duration::from_millis(milliseconds)).await;
}

fn redact_body(body: &[u8], token: &str) -> String {
    let raw = String::from_utf8_lossy(body);
    let mut text = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(value) => match serde_json::to_string(&crate::admin::dto::redact_json_value(&value)) {
            Ok(value) => value,
            Err(_) => raw.to_string(),
        },
        Err(_) => raw.to_string(),
    };
    if !token.is_empty() {
        text = text.replace(token, "<redacted>");
    }
    let lower = text.to_ascii_lowercase();
    if lower.contains("<html") {
        text = strip_html(&text);
    }
    text = redact_text(&text);
    text.chars().take(64 * 1024).collect()
}

pub(crate) fn redact_text(text: &str) -> String {
    let mut text = text.to_owned();
    for key in ["client_secret", "access_token", "authorization"] {
        text = redact_jsonish_field(&text, key);
    }
    text
}

fn redact_jsonish_field(text: &str, key: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while let Some(relative) = text[cursor..].find(key) {
        let start = cursor.saturating_add(relative);
        output.push_str(&text[cursor..start]);
        output.push_str(key);
        let after_key = start.saturating_add(key.len());
        let tail = &text[after_key..];
        if let Some(separator) = tail.find(':').or_else(|| tail.find('=')) {
            let separator_index = after_key.saturating_add(separator);
            output.push_str(&text[after_key..=separator_index]);
            let value_start = separator_index.saturating_add(1);
            let value_start = value_start.saturating_add(
                text[value_start..]
                    .chars()
                    .take_while(char::is_ascii_whitespace)
                    .map(char::len_utf8)
                    .sum::<usize>(),
            );
            output.push_str(&text[separator_index.saturating_add(1)..value_start]);
            if text.as_bytes().get(value_start) == Some(&b'"') {
                let remainder = text
                    .get(value_start.saturating_add(1)..)
                    .map_or("", |value| value);
                let mut escaped = false;
                let end = remainder
                    .char_indices()
                    .find_map(|(offset, character)| {
                        if escaped {
                            escaped = false;
                            None
                        } else if character == '\\' {
                            escaped = true;
                            None
                        } else if character == '"' {
                            Some(value_start.saturating_add(2).saturating_add(offset))
                        } else {
                            None
                        }
                    })
                    .map_or(text.len(), |value| value);
                output.push_str("\"<redacted>\"");
                cursor = end;
            } else {
                let remainder = text.get(value_start..).map_or("", |value| value);
                let end = remainder
                    .char_indices()
                    .find_map(|(offset, character)| {
                        matches!(character, ',' | '}' | '\n').then_some(value_start + offset)
                    })
                    .map_or(text.len(), |value| value);
                output.push_str("\"<redacted>\"");
                cursor = end;
            }
        } else {
            cursor = after_key;
        }
    }
    output.push_str(&text[cursor..]);
    output
}

fn strip_html(text: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for character in text.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output
}

fn bounded_detail(detail: &str) -> String {
    detail.chars().take(64 * 1024).collect()
}
