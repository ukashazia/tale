use std::fmt;
use std::sync::Arc;

use serde_json::Value;

use crate::domain::Timestamp;
use crate::domain::credential::CredentialMetadata;
use crate::domain::secret_result::SecretBuffer;

use super::dto::{DtoError, KeyDto, parse_timestamp};

pub const MIN_AUTH_KEY_EXPIRY_SECONDS: u64 = 24 * 60 * 60;
pub const MAX_AUTH_KEY_EXPIRY_SECONDS: u64 = 90 * 24 * 60 * 60;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthKeyCreateRequest {
    pub description: Option<String>,
    pub expiry_seconds: u64,
    pub reusable: bool,
    pub ephemeral: bool,
    pub preauthorized: bool,
    pub tags: Vec<String>,
}

impl AuthKeyCreateRequest {
    pub fn validate(&self) -> Result<(), AuthKeyRequestError> {
        if !(MIN_AUTH_KEY_EXPIRY_SECONDS..=MAX_AUTH_KEY_EXPIRY_SECONDS)
            .contains(&self.expiry_seconds)
        {
            return Err(AuthKeyRequestError::ExpiryOutOfRange);
        }
        if self.description.as_ref().is_some_and(|description| {
            description.is_empty()
                || description.len() > 50
                || !description.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | ' ')
                })
        }) {
            return Err(AuthKeyRequestError::InvalidDescription);
        }
        if self.tags.iter().any(|tag| tag.is_empty()) {
            return Err(AuthKeyRequestError::EmptyTag);
        }
        Ok(())
    }

    pub fn json_body(&self) -> Result<Value, AuthKeyRequestError> {
        self.validate()?;
        let capabilities = serde_json::json!({
            "devices": {
                "create": {
                    "reusable": self.reusable,
                    "ephemeral": self.ephemeral,
                    "preauthorized": self.preauthorized,
                    "tags": self.tags,
                }
            }
        });
        let mut body = serde_json::Map::new();
        body.insert("keyType".to_owned(), Value::String("auth".to_owned()));
        if let Some(description) = self.description.as_ref() {
            body.insert("description".to_owned(), Value::String(description.clone()));
        }
        body.insert(
            "expirySeconds".to_owned(),
            Value::Number(self.expiry_seconds.into()),
        );
        body.insert("capabilities".to_owned(), capabilities);
        Ok(Value::Object(body))
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AuthKeyRequestError {
    ExpiryOutOfRange,
    InvalidDescription,
    EmptyTag,
}

impl fmt::Display for AuthKeyRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ExpiryOutOfRange => "auth-key expiry must be between one and ninety days",
            Self::InvalidDescription => "auth-key description is invalid",
            Self::EmptyTag => "auth-key tags cannot be empty",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AuthKeyRequestError {}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RemoteCredentialType {
    AuthKey,
    ApiAccessToken,
    ClientCredential,
    Federated,
    Unknown,
}

impl RemoteCredentialType {
    pub fn parse(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "auth" | "auth_key" => Self::AuthKey,
            "api" | "api_access_token" | "access_token" => Self::ApiAccessToken,
            "client" | "client_credential" => Self::ClientCredential,
            "federated" => Self::Federated,
            _ => Self::Unknown,
        }
    }

    pub const fn read_scope(self) -> Option<&'static str> {
        match self {
            Self::AuthKey => Some("auth_keys:read"),
            Self::ApiAccessToken => Some("api_access_tokens:read"),
            Self::ClientCredential => Some("oauth_keys:read"),
            Self::Federated | Self::Unknown => None,
        }
    }

    pub const fn write_scope(self) -> Option<&'static str> {
        match self {
            Self::AuthKey => Some("auth_keys:write"),
            Self::ApiAccessToken => Some("api_access_tokens:write"),
            Self::ClientCredential => Some("oauth_keys:write"),
            Self::Federated | Self::Unknown => None,
        }
    }

    pub const fn supported_for_revoke(self) -> bool {
        matches!(
            self,
            Self::AuthKey | Self::ApiAccessToken | Self::ClientCredential
        )
    }
}

pub struct CreatedAuthKey {
    pub metadata: CredentialMetadata,
    pub secret: Arc<SecretBuffer>,
    pub created_at: Timestamp,
}

impl fmt::Debug for CreatedAuthKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreatedAuthKey")
            .field("metadata", &self.metadata)
            .field("secret", &"<redacted>")
            .field("created_at", &self.created_at)
            .finish()
    }
}

pub fn decode_created_auth_key(
    mut key: KeyDto,
    observed_at: Timestamp,
) -> Result<CreatedAuthKey, DtoError> {
    if !key
        .key_type
        .as_deref()
        .is_some_and(|value| RemoteCredentialType::parse(value) == RemoteCredentialType::AuthKey)
    {
        return Err(DtoError::InvalidCredentialType);
    }
    let secret = key.key.take().ok_or(DtoError::MissingCredentialSecret)?;
    if secret.is_empty() {
        return Err(DtoError::MissingCredentialSecret);
    }
    let metadata = CredentialMetadata {
        id: key
            .id
            .take()
            .ok_or(DtoError::MissingCollection { field: "key.id" })?,
        key_type: key.key_type.take().unwrap_or_else(|| "auth".to_owned()),
        created_at: parse_timestamp(key.created.as_deref(), "key.created")?,
        updated_at: parse_timestamp(key.updated.as_deref(), "key.updated")?,
        expires_at: parse_timestamp(key.expires.as_deref(), "key.expires")?,
        revoked_at: parse_timestamp(key.revoked.as_deref(), "key.revoked")?,
        last_used_at: parse_timestamp(key.last_used.as_deref(), "key.lastUsed")?,
        scopes: key.scopes.take().unwrap_or_default(),
        tags: key.tags.take().unwrap_or_default(),
        description: key.description.take(),
        invalid: key.invalid,
        user_id: key.user_id.take(),
        capability_summary: match key.capabilities.take() {
            Some(Value::Object(map)) => map.keys().cloned().collect(),
            None | Some(_) => Vec::new(),
        },
        known_dependents: key.known_dependents.take().unwrap_or_default(),
    };
    Ok(CreatedAuthKey {
        created_at: metadata.created_at.map_or(observed_at, |value| value),
        metadata,
        secret: Arc::new(SecretBuffer::new(secret.as_bytes())),
    })
}

pub fn remote_credential_type(metadata: &CredentialMetadata) -> RemoteCredentialType {
    RemoteCredentialType::parse(&metadata.key_type)
}
