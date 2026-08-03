use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;
use time::format_description::well_known::Rfc3339;

use crate::domain::Timestamp;

#[derive(Debug, Error)]
pub enum DtoError {
    #[error("required stable device identifier was not returned")]
    MissingDeviceId,
    #[error("invalid RFC3339 timestamp in {field}")]
    InvalidTimestamp { field: &'static str },
    #[error("invalid device route {value}")]
    InvalidRoute { value: String },
    #[error("credential response contained secret material")]
    SecretFieldReturned,
    #[error("required response collection was not returned: {field}")]
    MissingCollection { field: &'static str },
    #[error("response collection exceeded the 50,000 record limit: {field}")]
    RecordLimit { field: &'static str },
    #[error("audit log response exceeded the 50,000 event limit")]
    AuditLimit,
}

pub const MAX_RECORDS_PER_REFRESH: usize = 50_000;

#[derive(Debug, Deserialize)]
pub struct DevicesResponse {
    pub devices: Option<Vec<DeviceDto>>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceDto {
    pub id: Option<String>,
    #[serde(rename = "nodeId")]
    pub node_id: Option<String>,
    pub addresses: Option<Vec<String>>,
    pub user: Option<String>,
    pub name: Option<String>,
    pub hostname: Option<String>,
    #[serde(rename = "clientVersion")]
    pub client_version: Option<String>,
    #[serde(rename = "updateAvailable")]
    pub update_available: Option<bool>,
    pub os: Option<String>,
    pub created: Option<String>,
    #[serde(rename = "connectedToControl")]
    pub connected_to_control: Option<bool>,
    #[serde(rename = "lastSeen")]
    pub last_seen: Option<String>,
    #[serde(rename = "keyExpiryDisabled")]
    pub key_expiry_disabled: Option<bool>,
    pub expires: Option<String>,
    pub authorized: Option<bool>,
    #[serde(rename = "isExternal")]
    pub is_external: Option<bool>,
    #[serde(rename = "multipleConnections")]
    pub multiple_connections: Option<bool>,
    #[serde(rename = "advertisedRoutes")]
    pub advertised_routes: Option<Vec<String>>,
    #[serde(rename = "enabledRoutes")]
    pub enabled_routes: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    #[serde(rename = "isEphemeral")]
    pub is_ephemeral: Option<bool>,
    #[serde(rename = "sshEnabled")]
    pub ssh_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceRoutesDto {
    #[serde(rename = "advertisedRoutes")]
    pub advertised_routes: Option<Vec<String>>,
    #[serde(rename = "enabledRoutes")]
    pub enabled_routes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct DevicePostureAttributesDto {
    pub attributes: Option<Map<String, Value>>,
    pub expiries: Option<Map<String, Value>>,
}

#[derive(Debug, Deserialize)]
pub struct UsersResponse {
    pub users: Option<Vec<UserDto>>,
}

#[derive(Debug, Deserialize)]
pub struct UserDto {
    pub id: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "loginName")]
    pub login_name: Option<String>,
    #[serde(rename = "tailnetId")]
    pub tailnet_id: Option<String>,
    pub created: Option<String>,
    #[serde(rename = "type")]
    pub relation_type: Option<String>,
    pub role: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "deviceCount")]
    pub device_count: Option<u64>,
    #[serde(rename = "lastSeen")]
    pub last_seen: Option<String>,
    #[serde(rename = "currentlyConnected")]
    pub currently_connected: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct NameserversResponse {
    pub dns: Option<Vec<String>>,
    #[serde(rename = "magicDNS")]
    pub magic_dns: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct DnsPreferencesDto {
    #[serde(rename = "magicDNS")]
    pub magic_dns: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SearchPathsDto {
    #[serde(rename = "searchPaths")]
    pub search_paths: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct PolicyDetailsDto {
    pub acl: Option<String>,
    pub warnings: Option<Vec<String>>,
    pub errors: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct KeysResponse {
    pub keys: Option<Vec<KeyDto>>,
}

#[derive(Deserialize)]
pub struct KeyDto {
    pub id: Option<String>,
    pub key: Option<String>,
    #[serde(rename = "keyType")]
    pub key_type: Option<String>,
    #[serde(rename = "created")]
    pub created: Option<String>,
    pub updated: Option<String>,
    pub expires: Option<String>,
    pub revoked: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub description: Option<String>,
    pub invalid: Option<bool>,
    #[serde(rename = "userId")]
    pub user_id: Option<String>,
    pub capabilities: Option<Value>,
}

impl fmt::Debug for KeyDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KeyDto")
            .field("id", &self.id)
            .field("key", &self.key.as_ref().map(|_| "<redacted>"))
            .field("key_type", &self.key_type)
            .field("created", &self.created)
            .field("updated", &self.updated)
            .field("expires", &self.expires)
            .field("revoked", &self.revoked)
            .field("scopes", &self.scopes)
            .field("tags", &self.tags)
            .field("description", &self.description)
            .field("invalid", &self.invalid)
            .field("user_id", &self.user_id)
            .field(
                "capabilities",
                &self.capabilities.as_ref().map(redact_json_value),
            )
            .finish()
    }
}

#[derive(Debug, Deserialize)]
pub struct SettingsDto {
    #[serde(rename = "aclsExternallyManagedOn")]
    pub acls_externally_managed_on: Option<bool>,
    #[serde(rename = "aclsExternalLink")]
    pub acls_external_link: Option<String>,
    #[serde(rename = "devicesApprovalOn")]
    pub devices_approval_on: Option<bool>,
    #[serde(rename = "devicesAutoUpdatesOn")]
    pub devices_auto_updates_on: Option<bool>,
    #[serde(rename = "devicesKeyDurationDays")]
    pub devices_key_duration_days: Option<i64>,
    #[serde(rename = "usersApprovalOn")]
    pub users_approval_on: Option<bool>,
    #[serde(rename = "networkFlowLoggingOn")]
    pub network_flow_logging_on: Option<bool>,
    #[serde(rename = "regionalRoutingOn")]
    pub regional_routing_on: Option<bool>,
    #[serde(rename = "postureIdentityCollectionOn")]
    pub posture_identity_collection_on: Option<bool>,
    #[serde(rename = "httpsEnabled")]
    pub https_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ContactsResponse {
    pub account: Option<ContactDto>,
    pub support: Option<ContactDto>,
    pub security: Option<ContactDto>,
}

#[derive(Debug, Deserialize)]
pub struct ContactDto {
    pub email: Option<String>,
    #[serde(rename = "fallbackEmail")]
    pub fallback_email: Option<String>,
    #[serde(rename = "needsVerification")]
    pub needs_verification: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct AuditResponse {
    pub version: Option<String>,
    pub tailnet: Option<String>,
    pub logs: Option<Vec<AuditEventDto>>,
}

#[derive(Deserialize)]
pub struct AuditEventDto {
    #[serde(rename = "eventTime")]
    pub event_time: Option<String>,
    #[serde(rename = "type")]
    pub event_type: Option<String>,
    #[serde(rename = "deferredAt")]
    pub deferred_at: Option<String>,
    #[serde(rename = "eventGroupID")]
    pub event_group_id: Option<String>,
    pub origin: Option<String>,
    pub actor: Option<Value>,
    pub target: Option<Value>,
    pub action: Option<String>,
    pub old: Option<Value>,
    pub new: Option<Value>,
    #[serde(rename = "actionDetails")]
    pub action_details: Option<String>,
    pub error: Option<String>,
}

impl fmt::Debug for AuditEventDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuditEventDto")
            .field("event_time", &self.event_time)
            .field("event_type", &self.event_type)
            .field("deferred_at", &self.deferred_at)
            .field("event_group_id", &self.event_group_id)
            .field("origin", &self.origin)
            .field("actor", &self.actor.as_ref().map(redact_json_value))
            .field("target", &self.target.as_ref().map(redact_json_value))
            .field("action", &self.action)
            .field("old", &self.old.as_ref().map(redact_json_value))
            .field("new", &self.new.as_ref().map(redact_json_value))
            .field(
                "action_details",
                &self.action_details.as_ref().map(|value| redact_text(value)),
            )
            .field(
                "error",
                &self.error.as_ref().map(|value| redact_text(value)),
            )
            .finish()
    }
}

pub type SplitDnsEntry = (String, Option<Vec<String>>);

pub fn parse_timestamp(
    value: Option<&str>,
    field: &'static str,
) -> Result<Option<Timestamp>, DtoError> {
    match value.filter(|text| !text.is_empty()) {
        None => Ok(None),
        Some(text) => time::OffsetDateTime::parse(text, &Rfc3339)
            .map_err(|_| DtoError::InvalidTimestamp { field })
            .and_then(|date| {
                u64::try_from(date.unix_timestamp())
                    .map(Some)
                    .map_err(|_| DtoError::InvalidTimestamp { field })
            }),
    }
}

pub fn required_collection<T>(
    collection: Option<Vec<T>>,
    field: &'static str,
) -> Result<Vec<T>, DtoError> {
    collection.ok_or(DtoError::MissingCollection { field })
}

pub fn route_values(values: Option<Vec<String>>) -> Result<Vec<String>, DtoError> {
    let routes = values.unwrap_or_default();
    for route in &routes {
        if route.parse::<crate::domain::route::IpNet>().is_err() {
            return Err(DtoError::InvalidRoute {
                value: route.clone(),
            });
        }
    }
    Ok(routes)
}

pub fn split_dns_values(value: Map<String, Value>) -> Result<Vec<SplitDnsEntry>, DtoError> {
    let mut entries = Vec::with_capacity(value.len());
    for (domain, resolver_value) in value {
        let resolvers = if resolver_value.is_null() {
            None
        } else {
            let array = resolver_value.as_array().ok_or(DtoError::InvalidRoute {
                value: domain.clone(),
            })?;
            let mut values = Vec::with_capacity(array.len());
            for resolver in array {
                let text = resolver.as_str().ok_or(DtoError::InvalidRoute {
                    value: domain.clone(),
                })?;
                values.push(text.to_owned());
            }
            Some(values)
        };
        entries.push((domain, resolvers));
    }
    Ok(entries)
}

pub fn object_string(value: &Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str).map(str::to_owned))
}

pub fn object_kind(value: &Value) -> Option<String> {
    object_string(value, &["type", "kind"])
}

pub fn value_map(value: Option<Value>) -> BTreeMap<String, Value> {
    match value {
        Some(Value::Object(map)) => map.into_iter().collect(),
        None | Some(_) => BTreeMap::new(),
    }
}

pub fn redact_json_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, child)| {
                    let lower = key.to_ascii_lowercase();
                    let redacted = lower.contains("secret")
                        || lower.contains("token")
                        || lower.contains("authorization")
                        || key.eq_ignore_ascii_case("key");
                    (
                        key.clone(),
                        if redacted {
                            Value::String("<redacted>".to_owned())
                        } else {
                            redact_json_value(child)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(redact_json_value).collect()),
        other => other.clone(),
    }
}

fn redact_text(text: &str) -> String {
    let mut value = text.to_owned();
    for key in ["client_secret", "access_token", "authorization"] {
        value = redact_text_field(&value, key);
    }
    value
}

fn redact_text_field(text: &str, key: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while let Some(relative) = text[cursor..].find(key) {
        let start = cursor.saturating_add(relative);
        output.push_str(&text[cursor..start]);
        output.push_str(key);
        let after_key = start.saturating_add(key.len());
        let tail = &text[after_key..];
        if let Some(separator) = tail.find(':').or_else(|| tail.find('=')) {
            output.push_str(&tail[..=separator]);
            output.push_str("\"<redacted>\"");
            cursor = after_key.saturating_add(separator + 1);
            if let Some(comma) = text[cursor..].find(',') {
                cursor = cursor.saturating_add(comma);
            } else {
                cursor = text.len();
            }
        } else {
            cursor = after_key;
        }
    }
    output.push_str(&text[cursor..]);
    output
}
