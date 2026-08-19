use std::collections::BTreeSet;

use crate::domain::device::AdminDevice;

pub fn validate_machine_name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("device name cannot be empty".to_owned());
    }
    if value
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("device name cannot contain whitespace or control characters".to_owned());
    }
    Ok(value.to_owned())
}

pub fn canonical_tags(values: &[String]) -> Result<Vec<String>, String> {
    let mut tags = BTreeSet::new();
    for value in values {
        let value = value.trim();
        let (Some(prefix), Some(tag_value)) = (value.get(..4), value.get(4..)) else {
            return Err(format!("tag must use the documented tag: prefix: {value}"));
        };
        if !prefix.eq_ignore_ascii_case("tag:") {
            return Err(format!("tag must use the documented tag: prefix: {value}"));
        }
        if tag_value.is_empty()
            || value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(format!(
                "tag contains invalid whitespace or control characters: {value}"
            ));
        }
        tags.insert(format!("tag:{tag_value}").to_ascii_lowercase());
    }
    Ok(tags.into_iter().collect())
}

pub fn device_fields_for_name(device: &AdminDevice) -> Vec<String> {
    vec![
        format!("machine name: {}", device.display_name()),
        format!("stable device ID: {}", device.stable_id),
        format!(
            "owner: {}",
            device.user_id.as_deref().unwrap_or("not returned")
        ),
        format!(
            "tags: {}",
            if device.tags.is_empty() {
                "none".to_owned()
            } else {
                device.tags.join(", ")
            }
        ),
        "MagicDNS name changes immediately; existing URLs using the old name may stop working"
            .to_owned(),
    ]
}

pub fn verify_name(device: &AdminDevice, requested: &str) -> Result<(), String> {
    if device.display_name() == requested
        || device.name.as_deref() == Some(requested)
        || device.hostname.as_deref() == Some(requested)
    {
        Ok(())
    } else {
        Err(format!(
            "server returned canonical name {} instead of {}",
            device.display_name(),
            requested
        ))
    }
}

pub fn verify_tags(device: &AdminDevice, requested: &[String]) -> Result<(), String> {
    let actual = canonical_tags(&device.tags)?;
    let requested = canonical_tags(requested)?;
    if actual == requested {
        Ok(())
    } else {
        Err(format!(
            "server returned tags [{}], requested [{}]",
            actual.join(", "),
            requested.join(", ")
        ))
    }
}

pub fn verify_approval(device: &AdminDevice, requested: bool) -> Result<(), String> {
    if device.authorized == Some(requested) {
        Ok(())
    } else {
        Err(format!(
            "server returned approval {}, requested {requested}",
            device
                .authorized
                .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
        ))
    }
}

pub fn verify_key_expiry(device: &AdminDevice, requested_disabled: bool) -> Result<(), String> {
    if device.key_expiry_disabled == Some(requested_disabled) {
        Ok(())
    } else {
        Err(format!(
            "server returned key expiry disabled {}, requested {requested_disabled}",
            device
                .key_expiry_disabled
                .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
        ))
    }
}

pub fn device_delete_context(device: &AdminDevice) -> Vec<String> {
    vec![
        format!("name: {}", device.display_name()),
        format!("stable ID: {}", device.stable_id),
        format!(
            "owner: {}",
            device.user_id.as_deref().unwrap_or("not returned")
        ),
        format!(
            "approval: {}",
            device
                .authorized
                .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
        ),
        format!(
            "online observation: {}",
            device.connected_to_control.map_or("unknown", |value| {
                if value { "online" } else { "offline" }
            })
        ),
        format!(
            "advertised routes: {}",
            if device.advertised_routes.is_empty() {
                "none".to_owned()
            } else {
                device.advertised_routes.join(", ")
            }
        ),
        format!(
            "approved routes: {}",
            if device.enabled_routes.is_empty() {
                "none".to_owned()
            } else {
                device.enabled_routes.join(", ")
            }
        ),
        format!(
            "key expiry: {}",
            device.key_expiry_disabled.map_or_else(
                || "unknown".to_owned(),
                |value| {
                    if value {
                        "disabled".to_owned()
                    } else {
                        "enabled".to_owned()
                    }
                }
            )
        ),
    ]
}

pub fn verify_expire_now(device: &AdminDevice, observed_at: u64) -> Result<(), String> {
    if device.key_expiry_disabled == Some(true) {
        return Err("server still reports key expiry disabled after expire-now".to_owned());
    }
    if device
        .expires_at
        .is_some_and(|expires| expires <= observed_at)
    {
        Ok(())
    } else {
        Err("server did not return an expired key timestamp".to_owned())
    }
}
