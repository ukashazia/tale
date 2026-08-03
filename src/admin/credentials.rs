use std::collections::BTreeMap;

use crate::admin::dto::{
    DtoError, KeyDto, MAX_RECORDS_PER_REFRESH, parse_timestamp, required_collection,
};
use crate::domain::Timestamp;
use crate::domain::credential::{CredentialMetadata, CredentialSnapshot};

pub fn decode_credentials(
    keys: Option<Vec<KeyDto>>,
    observed_at: Timestamp,
) -> Result<CredentialSnapshot, DtoError> {
    let keys = required_collection(keys, "keys")?;
    if keys.len() > MAX_RECORDS_PER_REFRESH {
        return Err(DtoError::RecordLimit { field: "keys" });
    }
    let mut positions = BTreeMap::new();
    let mut records = Vec::with_capacity(keys.len());
    for key in keys {
        let record = decode_credential(key)?;
        if let Some(position) = positions.insert(record.id.clone(), records.len()) {
            records[position] = record;
        } else {
            records.push(record);
        }
    }
    Ok(CredentialSnapshot {
        records,
        partial: true,
        partial_reason: Some(
            "only credential kinds permitted by the configured narrow scopes are shown".to_owned(),
        ),
        observed_at,
    })
}

pub fn decode_credential(key: KeyDto) -> Result<CredentialMetadata, DtoError> {
    if key.key.is_some() {
        return Err(DtoError::SecretFieldReturned);
    }
    Ok(CredentialMetadata {
        id: key
            .id
            .ok_or(DtoError::MissingCollection { field: "key.id" })?,
        key_type: match key.key_type {
            Some(key_type) => key_type,
            None => "not returned".to_owned(),
        },
        created_at: parse_timestamp(key.created.as_deref(), "key.created")?,
        updated_at: parse_timestamp(key.updated.as_deref(), "key.updated")?,
        expires_at: parse_timestamp(key.expires.as_deref(), "key.expires")?,
        revoked_at: parse_timestamp(key.revoked.as_deref(), "key.revoked")?,
        scopes: key.scopes.unwrap_or_default(),
        tags: key.tags.unwrap_or_default(),
        description: key.description,
        invalid: key.invalid,
        user_id: key.user_id,
        capability_summary: match key.capabilities {
            Some(serde_json::Value::Object(map)) => map.keys().cloned().collect(),
            None | Some(_) => Vec::new(),
        },
    })
}
