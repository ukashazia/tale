use std::collections::BTreeMap;

use crate::admin::dto::{
    DeviceDto, DtoError, MAX_RECORDS_PER_REFRESH, parse_timestamp, required_collection,
    route_values,
};
use crate::domain::Timestamp;
use crate::domain::device::{AdminDevice, OperatingSystem};

pub fn decode_devices(
    devices: Option<Vec<DeviceDto>>,
    observed_at: Timestamp,
) -> Result<Vec<AdminDevice>, DtoError> {
    let devices = required_collection(devices, "devices")?;
    if devices.len() > MAX_RECORDS_PER_REFRESH {
        return Err(DtoError::RecordLimit { field: "devices" });
    }
    let mut positions = BTreeMap::new();
    let mut decoded = Vec::with_capacity(devices.len());
    for device in devices {
        let device = decode_device(device, observed_at)?;
        if let Some(position) = positions.insert(device.stable_id.clone(), decoded.len()) {
            decoded[position] = device;
        } else {
            decoded.push(device);
        }
    }
    Ok(decoded)
}

pub fn decode_device(device: DeviceDto, observed_at: Timestamp) -> Result<AdminDevice, DtoError> {
    let stable_id = device
        .node_id
        .clone()
        .or(device.id.clone())
        .ok_or(DtoError::MissingDeviceId)?;
    let advertised_routes_returned = device.advertised_routes.is_some();
    let enabled_routes_returned = device.enabled_routes.is_some();
    Ok(AdminDevice {
        stable_id,
        legacy_id: device.id,
        node_id: device.node_id,
        addresses: device.addresses.unwrap_or_default(),
        user_id: device.user,
        name: device.name,
        hostname: device.hostname,
        client_version: device.client_version,
        update_available: device.update_available,
        os: device.os.as_deref().map(parse_os),
        created_at: parse_timestamp(device.created.as_deref(), "device.created")?,
        connected_to_control: device.connected_to_control,
        last_seen: parse_timestamp(device.last_seen.as_deref(), "device.lastSeen")?,
        key_expiry_disabled: device.key_expiry_disabled,
        expires_at: parse_timestamp(device.expires.as_deref(), "device.expires")?,
        authorized: device.authorized,
        is_external: device.is_external,
        multiple_connections: device.multiple_connections,
        advertised_routes_returned,
        advertised_routes: route_values(device.advertised_routes)?,
        enabled_routes_returned,
        enabled_routes: route_values(device.enabled_routes)?,
        tags: device.tags.unwrap_or_default(),
        is_ephemeral: device.is_ephemeral,
        ssh_enabled: device.ssh_enabled,
        posture_present: None,
        source_observed_at: observed_at,
    })
}

pub fn apply_routes(
    device: &mut AdminDevice,
    advertised: Option<Vec<String>>,
    enabled: Option<Vec<String>>,
) -> Result<(), DtoError> {
    device.advertised_routes_returned = advertised.is_some();
    device.enabled_routes_returned = enabled.is_some();
    device.advertised_routes = route_values(advertised)?;
    device.enabled_routes = route_values(enabled)?;
    Ok(())
}

pub fn apply_posture(device: &mut AdminDevice, present: bool) {
    device.posture_present = Some(present);
}

fn parse_os(value: &str) -> OperatingSystem {
    match value.to_ascii_lowercase().as_str() {
        "linux" => OperatingSystem::Linux,
        "darwin" | "macos" | "mac" => OperatingSystem::MacOS,
        "windows" => OperatingSystem::Windows,
        "ios" => OperatingSystem::IOS,
        "android" => OperatingSystem::Android,
        _ => OperatingSystem::Unknown(value.to_owned()),
    }
}
