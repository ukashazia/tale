use tale::admin::devices::{apply_posture, decode_devices};
use tale::admin::dto::{DevicePostureAttributesDto, DevicesResponse};
use tale::admin::{AdminResource, AdminResourceState, AdminSnapshot};
use tale::domain::SourceHealth;
use tale::domain::device::{ConnectionPath, DeviceId, LocalDevice, OperatingSystem};

#[test]
fn admin_device_display_name_uses_the_device_name_not_the_hostname() -> Result<(), String> {
    let dto: DevicesResponse = serde_json::from_str(
        r#"{
            "devices": [{
                "nodeId": "node-fictional-001",
                "name": "fixture-device.fixture-net.ts.net",
                "hostname": "fixture-device.local"
            }]
        }"#,
    )
    .map_err(|error| error.to_string())?;
    let devices = decode_devices(dto.devices, 1).map_err(|error| error.to_string())?;
    let device = devices
        .first()
        .ok_or_else(|| "decoded device is missing".to_owned())?;

    assert_eq!(device.display_name(), "fixture-device");
    assert_eq!(device.to_display_device().display_name, "fixture-device");
    assert_eq!(
        device.name.as_deref(),
        Some("fixture-device.fixture-net.ts.net")
    );
    assert_eq!(device.hostname.as_deref(), Some("fixture-device.local"));
    Ok(())
}

#[test]
fn posture_enrichment_preserves_opaque_values_and_distinguishes_empty() -> Result<(), String> {
    let dto: DevicesResponse = serde_json::from_str(include_str!("fixtures/admin/devices.json"))
        .map_err(|error| error.to_string())?;
    let mut devices = decode_devices(dto.devices, 1).map_err(|error| error.to_string())?;
    let device = devices
        .first_mut()
        .ok_or_else(|| "decoded device is missing".to_owned())?;
    let posture: DevicePostureAttributesDto =
        serde_json::from_str(include_str!("fixtures/admin/posture.json"))
            .map_err(|error| error.to_string())?;

    apply_posture(device, posture.attributes, posture.expiries);
    assert_eq!(device.posture_present, Some(true));
    assert_eq!(
        device.posture_attributes.get("node:fictional"),
        Some(&serde_json::Value::String("observed".to_owned()))
    );
    assert!(device.posture_expiries.contains_key("node:fictional"));

    apply_posture(device, Some(serde_json::Map::new()), None);
    assert_eq!(device.posture_present, Some(false));
    assert!(device.posture_attributes.is_empty());
    assert!(device.posture_expiries.is_empty());
    Ok(())
}

#[test]
fn exact_id_composition_keeps_unmatched_records() -> Result<(), String> {
    let local = vec![LocalDevice {
        id: DeviceId::new("node-fictional-001"),
        public_key: None,
        display_name: "local".to_owned(),
        hostname: "local".to_owned(),
        dns_name: None,
        os: OperatingSystem::Linux,
        version: None,
        owner_label: None,
        user_id: None,
        tags: Vec::new(),
        tailscale_ips: Vec::new(),
        advertised_routes: Vec::new(),
        current_endpoint: None,
        relay_region: None,
        path: ConnectionPath::Unknown("fixture".to_owned()),
        online: None,
        active: false,
        rx_bytes: None,
        tx_bytes: None,
        created_at: None,
        last_seen: None,
        last_handshake: None,
        exit_node: false,
        exit_node_option: false,
        ssh_host_keys_present: false,
        shared: false,
        capabilities: std::collections::BTreeMap::new(),
    }];
    let dto: DevicesResponse = serde_json::from_str(include_str!("fixtures/admin/devices.json"))
        .map_err(|error| error.to_string())?;
    let admin = decode_devices(dto.devices, 1).map_err(|error| error.to_string())?;
    let composed = tale::domain::device::compose_exact_id(&local, &admin);
    assert_eq!(composed.len(), 2);
    assert!(composed[0].local.is_some());
    assert!(composed[0].admin.is_some());
    assert!(composed[1].local.is_none());
    assert!(composed[1].admin.is_some());
    Ok(())
}

#[test]
fn overview_queues_are_pure_snapshot_derivations() -> Result<(), String> {
    let dto: DevicesResponse = serde_json::from_str(include_str!("fixtures/admin/devices.json"))
        .map_err(|error| error.to_string())?;
    let devices = decode_devices(dto.devices, 1).map_err(|error| error.to_string())?;
    let mut snapshot = AdminSnapshot::new(
        Some("fictional".to_owned()),
        Some("example.test".to_owned()),
        true,
        vec!["devices:core:read".to_owned()],
    );
    snapshot.devices.begin(1);
    snapshot.devices.succeed(1, devices, 1);
    let queues = snapshot.overview_queues(1_785_751_200);
    assert_eq!(queues.devices_awaiting_approval.len(), 1);
    assert_eq!(queues.expired_device_keys.len(), 1);
    assert_eq!(queues.soon_expiring_device_keys.len(), 1);
    assert_eq!(queues.unapproved_routes.len(), 2);
    Ok(())
}

#[test]
fn resource_failures_preserve_successful_snapshot_as_stale() {
    let mut resource = AdminResource::new(Some("fictional".to_owned()));
    resource.begin(1);
    resource.succeed(1, vec!["observed".to_owned()], 1);
    resource.fail(2, AdminResourceState::Failed, "transport failed".to_owned());
    assert_eq!(resource.state, AdminResourceState::Ready);
    resource.begin(2);
    resource.fail(2, AdminResourceState::Failed, "transport failed".to_owned());
    assert_eq!(resource.state, AdminResourceState::Stale);
    assert_eq!(
        SourceHealth::from_admin_state(resource.state),
        SourceHealth::Stale
    );
}
