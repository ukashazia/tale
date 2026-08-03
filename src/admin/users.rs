use std::collections::BTreeMap;

use crate::admin::dto::{
    DtoError, MAX_RECORDS_PER_REFRESH, UserDto, parse_timestamp, required_collection,
};
use crate::domain::Timestamp;
use crate::domain::user::AdminUser;

pub fn decode_users(
    users: Option<Vec<UserDto>>,
    _observed_at: Timestamp,
) -> Result<Vec<AdminUser>, DtoError> {
    let users = required_collection(users, "users")?;
    if users.len() > MAX_RECORDS_PER_REFRESH {
        return Err(DtoError::RecordLimit { field: "users" });
    }
    let mut positions = BTreeMap::new();
    let mut decoded = Vec::with_capacity(users.len());
    for user in users {
        let user = decode_user(user)?;
        if let Some(position) = positions.insert(user.id.clone(), decoded.len()) {
            decoded[position] = user;
        } else {
            decoded.push(user);
        }
    }
    Ok(decoded)
}

pub fn decode_user(user: UserDto) -> Result<AdminUser, DtoError> {
    Ok(AdminUser {
        id: user
            .id
            .ok_or(DtoError::MissingCollection { field: "user.id" })?,
        display_name: user.display_name,
        login_name: user.login_name,
        tailnet_id: user.tailnet_id,
        created_at: parse_timestamp(user.created.as_deref(), "user.created")?,
        relation_type: user.relation_type,
        role: user.role,
        status: user.status,
        device_count: user.device_count,
        last_seen: parse_timestamp(user.last_seen.as_deref(), "user.lastSeen")?,
        currently_connected: user.currently_connected,
    })
}
