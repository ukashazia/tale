use crate::domain::device::AdminDevice;
use crate::domain::user::AdminUser;

pub const DOCUMENTED_ROLES: &[&str] = &[
    "owner",
    "member",
    "admin",
    "it-admin",
    "network-admin",
    "billing-admin",
    "auditor",
];

pub fn validate_role(value: &str) -> Result<String, String> {
    if DOCUMENTED_ROLES.contains(&value) {
        Ok(value.to_owned())
    } else {
        Err(format!(
            "role must be one of: {}",
            DOCUMENTED_ROLES.join(", ")
        ))
    }
}

pub fn verify_status(user: &AdminUser, expected: &[&str]) -> Result<(), String> {
    let status = user.status.as_deref().unwrap_or("unknown");
    if expected
        .iter()
        .any(|value| status.eq_ignore_ascii_case(value))
    {
        Ok(())
    } else {
        Err(format!(
            "server returned user status {status}, expected {}",
            expected.join(" or ")
        ))
    }
}

pub fn verify_role(user: &AdminUser, expected: &str) -> Result<(), String> {
    if user.role.as_deref() == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "server returned user role {}, requested {expected}",
            user.role.as_deref().unwrap_or("unknown")
        ))
    }
}

pub fn owned_devices<'a>(user: &AdminUser, devices: &'a [AdminDevice]) -> Vec<&'a AdminDevice> {
    devices
        .iter()
        .filter(|device| device.user_id.as_deref() == Some(user.id.as_str()))
        .collect()
}

pub fn owned_device_context(user: &AdminUser, devices: &[AdminDevice]) -> Vec<String> {
    let owned = owned_devices(user, devices);
    let mut lines = vec![format!("owned devices: {}", owned.len())];
    for device in owned {
        let advertised_routes = if device.advertised_routes.is_empty() {
            "none".to_owned()
        } else {
            device.advertised_routes.join(", ")
        };
        let enabled_routes = if device.enabled_routes.is_empty() {
            "none".to_owned()
        } else {
            device.enabled_routes.join(", ")
        };
        lines.push(format!(
            "  {} · online:{} · advertised-routes:{} · approved-routes:{} · key-expiry:{}",
            device.display_name(),
            device
                .connected_to_control
                .map_or("unknown", |value| { if value { "yes" } else { "no" } }),
            advertised_routes,
            enabled_routes,
            device.key_expiry_disabled.map_or("unknown", |value| {
                if value { "disabled" } else { "enabled" }
            })
        ));
    }
    lines
}
