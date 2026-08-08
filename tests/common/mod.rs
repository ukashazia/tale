//! Fixtures shared by the tests that care which tailnet a row came from.
//!
//! Tale identifies a tailnet by the MagicDNS suffix its two sources agree on, so
//! a fixture that wants them composed has to make them agree the same way a real
//! pair of sources would.
//!
//! Each test binary compiles this module separately and uses only the part it
//! needs, so the unused half is expected rather than dead.
#![allow(dead_code)]

use std::collections::BTreeMap;

use tale::app::App;
use tale::domain::Timestamp;
use tale::domain::device::{AdminDevice, ConnectionPath, DeviceId, LocalDevice, OperatingSystem};
use tale::domain::source::{LocalSnapshot, LocalState};

pub const OBSERVED_AT: Timestamp = 1_785_751_200;

pub fn local_device(id: &str, suffix: &str) -> LocalDevice {
    LocalDevice {
        id: DeviceId::new(id),
        public_key: None,
        display_name: id.to_owned(),
        hostname: id.to_owned(),
        dns_name: Some(format!("{id}.{suffix}")),
        os: OperatingSystem::Linux,
        version: Some("1.98.0".to_owned()),
        owner_label: None,
        user_id: None,
        tags: Vec::new(),
        tailscale_ips: vec!["100.64.0.1".to_owned()],
        advertised_routes: Vec::new(),
        current_endpoint: None,
        relay_region: None,
        path: ConnectionPath::Direct {
            latency_ms: Some(1),
        },
        online: Some(true),
        active: true,
        rx_bytes: None,
        tx_bytes: None,
        created_at: None,
        last_seen: Some(OBSERVED_AT),
        last_handshake: Some(OBSERVED_AT),
        exit_node: false,
        exit_node_option: false,
        ssh_host_keys_present: true,
        shared: false,
        capabilities: BTreeMap::new(),
    }
}

pub fn admin_device(id: &str, suffix: &str) -> AdminDevice {
    AdminDevice {
        stable_id: id.to_owned(),
        legacy_id: None,
        node_id: Some(id.to_owned()),
        addresses: vec!["100.64.0.1".to_owned()],
        user_id: None,
        // The fully-qualified name is what carries the tailnet.
        name: Some(format!("{id}.{suffix}")),
        hostname: Some(id.to_owned()),
        client_version: None,
        update_available: None,
        os: Some(OperatingSystem::Linux),
        created_at: None,
        connected_to_control: Some(true),
        last_seen: Some(OBSERVED_AT),
        key_expiry_disabled: None,
        expires_at: None,
        authorized: Some(true),
        is_external: None,
        multiple_connections: None,
        advertised_routes_returned: false,
        advertised_routes: Vec::new(),
        enabled_routes_returned: false,
        enabled_routes: Vec::new(),
        tags: Vec::new(),
        is_ephemeral: None,
        ssh_enabled: None,
        posture_present: None,
        source_observed_at: OBSERVED_AT,
    }
}

/// A running client on `suffix`, whose first id is this machine.
pub fn install_local(app: &mut App, suffix: &str, ids: &[&str]) {
    let mut nodes = ids.iter().map(|id| local_device(id, suffix));
    let Some(self_node) = nodes.next() else {
        return;
    };
    let snapshot = LocalSnapshot {
        observed_at: OBSERVED_AT,
        client_version: "1.98.0".to_owned(),
        daemon_version: Some("1.98.0".to_owned()),
        backend_state: LocalState::Running,
        health_messages: Vec::new(),
        current_tailnet: Some(suffix.to_owned()),
        magic_dns_suffix: Some(suffix.to_owned()),
        cert_domains: Vec::new(),
        self_node,
        peers: nodes.collect(),
    };
    let generation = app.local_resource.generation.saturating_add(1);
    app.local_resource.begin(generation, OBSERVED_AT);
    let _ = app.local_resource.succeed(generation, snapshot);
    app.refresh_device_view();
}

pub fn install_admin(app: &mut App, devices: Vec<AdminDevice>) {
    let generation = app.admin.devices.generation.saturating_add(1);
    app.admin.devices.begin(generation);
    app.admin.devices.succeed(generation, devices, OBSERVED_AT);
    app.refresh_device_view();
}

/// Both sources reading one tailnet, which is the only arrangement in which a
/// row carries local and admin detail at once.
pub fn install_aligned_sources(app: &mut App, suffix: &str, ids: &[&str]) {
    install_local(app, suffix, ids);
    install_admin(app, ids.iter().map(|id| admin_device(id, suffix)).collect());
}
