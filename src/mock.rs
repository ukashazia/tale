use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::admin::AdminSnapshot;
use crate::admin::routes::AdminRouteObservation;
use crate::domain::Timestamp;
use crate::domain::activity::{AuditEvent, AuditPrincipal, AuditSnapshot, AuditTarget};
use crate::domain::credential::{CredentialMetadata, CredentialSnapshot};
use crate::domain::device::{
    ConnectionPath, Device, DeviceCapabilities, DeviceId, Liveness, LocalDevice, OperatingSystem,
};
use crate::domain::diagnostic::{
    DiagnosticResult, DiagnosticState, DnsAnswer, DnsQueryResult, DnsStatus,
};
use crate::domain::dns::{AdminDnsPreferences, AdminNameservers, AdminSearchPaths, AdminSplitDns};
use crate::domain::policy::PolicySnapshot;
use crate::domain::preference::{LocalPreferences, ObservedPreference};
use crate::domain::source::{LocalResource, LocalSnapshot};
use crate::task::TaskId;

pub const MOCK_NOW: Timestamp = 1_754_000_000;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum MockLoadScenario {
    Initial,
    Success,
    Failure,
    Stale,
}

pub type MockScenario = MockLoadScenario;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MockTaskBehavior {
    DelayedSuccess,
    DelayedFailure,
    CancellableLong,
    NonCancellable,
}

pub fn load_devices(scenario: MockLoadScenario) -> Result<(Vec<Device>, Timestamp), String> {
    match scenario {
        MockLoadScenario::Failure => {
            Err("mock refresh failed: fictional source timeout".to_owned())
        }
        MockLoadScenario::Stale => Ok((devices(), MOCK_NOW.saturating_sub(240))),
        MockLoadScenario::Initial | MockLoadScenario::Success => Ok((devices(), MOCK_NOW)),
    }
}

pub fn local_resource() -> LocalResource {
    let mut resource = LocalResource::new();
    let _ = resource.succeed(1, local_snapshot());
    resource
}

pub fn local_snapshot() -> LocalSnapshot {
    LocalSnapshot {
        observed_at: MOCK_NOW,
        client_version: "1.98.9".to_owned(),
        daemon_version: Some("1.98.9".to_owned()),
        backend_state: crate::domain::source::LocalState::Mock,
        health_messages: Vec::new(),
        current_tailnet: Some("example.test".to_owned()),
        magic_dns_suffix: Some("example.test".to_owned()),
        cert_domains: vec!["build-01.example.test".to_owned()],
        self_node: local_device(
            "node-local",
            "build-01",
            "build-01.example.test",
            vec!["100.64.0.10", "fd7a:115c:a1e0::10"],
            vec!["192.0.2.0/24"],
            true,
        ),
        peers: vec![local_device(
            "node-peer",
            "studio-mac",
            "studio-mac.example.test",
            vec!["100.64.0.11"],
            Vec::new(),
            true,
        )],
    }
}

pub fn local_preferences() -> LocalPreferences {
    let mut preferences = LocalPreferences::empty(MOCK_NOW);
    preferences.want_running = ObservedPreference::known(true, MOCK_NOW);
    preferences.logged_out = ObservedPreference::known(false, MOCK_NOW);
    preferences.accept_dns = ObservedPreference::known(true, MOCK_NOW);
    preferences.accept_routes = ObservedPreference::known(true, MOCK_NOW);
    preferences.shields_up = ObservedPreference::known(false, MOCK_NOW);
    preferences.ssh = ObservedPreference::known(true, MOCK_NOW);
    preferences.update_check = ObservedPreference::known(true, MOCK_NOW);
    preferences.automatic_update = ObservedPreference::known(true, MOCK_NOW);
    preferences.report_posture = ObservedPreference::known(true, MOCK_NOW);
    preferences.hostname = ObservedPreference::known("build-01".to_owned(), MOCK_NOW);
    preferences.nickname = ObservedPreference::known("Build host".to_owned(), MOCK_NOW);
    preferences.web_client = ObservedPreference::known(false, MOCK_NOW);
    preferences.advertised_routes =
        ObservedPreference::known(vec!["192.0.2.0/24".to_owned()], MOCK_NOW);
    preferences.advertised_exit_node = ObservedPreference::known(false, MOCK_NOW);
    preferences.app_connector = ObservedPreference::known(false, MOCK_NOW);
    preferences.relay_server_port_disabled = ObservedPreference::known(true, MOCK_NOW);
    preferences.relay_server_static_endpoints = ObservedPreference::known(Vec::new(), MOCK_NOW);
    preferences
}

pub fn local_diagnostics() -> BTreeMap<TaskId, DiagnosticState> {
    let mut diagnostics = BTreeMap::new();
    let mut status = DiagnosticState::new("dns status");
    status.result = Some(DiagnosticResult::DnsStatus(DnsStatus {
        forwarder_enabled: Some(true),
        magic_dns_enabled: Some(true),
        magic_dns_suffix: Some("example.test".to_owned()),
        current_node_dns_name: Some("build-01.example.test".to_owned()),
        resolvers: vec!["100.100.100.100".to_owned(), "9.9.9.9".to_owned()],
        split_routes: BTreeMap::from([(
            "corp.example.test".to_owned(),
            vec!["10.0.0.53".to_owned()],
        )]),
        cert_domains: vec!["build-01.example.test".to_owned()],
        observed_at: MOCK_NOW,
    }));
    diagnostics.insert(TaskId(1), status);
    let mut query = DiagnosticState::new("dns query");
    query.result = Some(DiagnosticResult::DnsQuery(DnsQueryResult {
        name: "studio-mac.example.test".to_owned(),
        record_type: "A".to_owned(),
        answers: vec![DnsAnswer {
            value: "100.64.0.11".to_owned(),
            record_type: Some("A".to_owned()),
            ttl: Some(30),
            raw_detail: None,
        }],
        resolvers: vec!["100.100.100.100".to_owned()],
        latency_ms: Some(12),
        result_class: "answered".to_owned(),
        observed_at: MOCK_NOW,
        raw_detail: String::new(),
    }));
    diagnostics.insert(TaskId(2), query);
    diagnostics
}

pub fn admin_snapshot() -> AdminSnapshot {
    let mut admin = AdminSnapshot::new(
        None,
        Some("example.test".to_owned()),
        true,
        vec![
            "devices:routes:read".to_owned(),
            "dns:read".to_owned(),
            "policy_file:read".to_owned(),
            "auth_keys:read".to_owned(),
            "logs:configuration:read".to_owned(),
        ],
    );
    admin.routes.begin(1);
    admin.routes.succeed(
        1,
        vec![
            AdminRouteObservation {
                device_id: "subnet-gateway".to_owned(),
                advertised: vec!["192.0.2.0/24".to_owned()],
                enabled: vec!["192.0.2.0/24".to_owned()],
                observed_at: MOCK_NOW,
                complete: true,
            },
            AdminRouteObservation {
                device_id: "edge-relay".to_owned(),
                advertised: vec!["0.0.0.0/0".to_owned(), "::/0".to_owned()],
                enabled: Vec::new(),
                observed_at: MOCK_NOW,
                complete: true,
            },
        ],
        MOCK_NOW,
    );
    admin.nameservers.begin(1);
    admin.nameservers.succeed(
        1,
        AdminNameservers {
            values: vec!["9.9.9.9".to_owned(), "149.112.112.112".to_owned()],
            observed_at: MOCK_NOW,
        },
        MOCK_NOW,
    );
    admin.dns_preferences.begin(1);
    admin.dns_preferences.succeed(
        1,
        AdminDnsPreferences {
            magic_dns: Some(true),
            observed_at: MOCK_NOW,
        },
        MOCK_NOW,
    );
    admin.search_paths.begin(1);
    admin.search_paths.succeed(
        1,
        AdminSearchPaths {
            values: vec!["example.test".to_owned()],
            observed_at: MOCK_NOW,
        },
        MOCK_NOW,
    );
    admin.split_dns.begin(1);
    admin.split_dns.succeed(
        1,
        AdminSplitDns {
            entries: vec![(
                "corp.example.test".to_owned(),
                Some(vec!["10.0.0.53".to_owned()]),
            )],
            observed_at: MOCK_NOW,
        },
        MOCK_NOW,
    );
    admin.policy.begin(1);
    admin.policy.succeed(
        1,
        PolicySnapshot {
            source_bytes: br#"{
  // Fictional mock policy
  "groups": { "group:ops": ["alice@example.test"] },
  "grants": [{ "src": ["group:ops"], "dst": ["tag:server"], "ip": ["tcp:22"] }]
}
"#
            .to_vec(),
            content_type: "application/hujson".to_owned(),
            fetched_at: MOCK_NOW,
            content_hash: "sha256:fictional-policy".to_owned(),
            etag: None,
        },
        MOCK_NOW,
    );
    admin.credentials.begin(1);
    admin.credentials.succeed(
        1,
        CredentialSnapshot {
            records: vec![
                CredentialMetadata {
                    id: "key-fictional-deploy".to_owned(),
                    key_type: "auth key".to_owned(),
                    created_at: Some(MOCK_NOW.saturating_sub(86_400)),
                    updated_at: None,
                    expires_at: Some(MOCK_NOW.saturating_add(604_800)),
                    revoked_at: None,
                    last_used_at: Some(MOCK_NOW.saturating_sub(3_600)),
                    scopes: vec!["tag:server".to_owned()],
                    tags: vec!["tag:server".to_owned()],
                    description: Some("CI deployment".to_owned()),
                    invalid: Some(false),
                    user_id: Some("alice@example.test".to_owned()),
                    capability_summary: vec!["preauthorized".to_owned(), "ephemeral".to_owned()],
                    known_dependents: vec!["build pipeline".to_owned()],
                },
                CredentialMetadata {
                    id: "key-fictional-expired".to_owned(),
                    key_type: "auth key".to_owned(),
                    created_at: Some(MOCK_NOW.saturating_sub(2_592_000)),
                    updated_at: None,
                    expires_at: Some(MOCK_NOW.saturating_sub(86_400)),
                    revoked_at: None,
                    last_used_at: None,
                    scopes: Vec::new(),
                    tags: vec!["tag:lab".to_owned()],
                    description: Some("Retired lab enrollment".to_owned()),
                    invalid: Some(true),
                    user_id: Some("bob@example.test".to_owned()),
                    capability_summary: Vec::new(),
                    known_dependents: Vec::new(),
                },
            ],
            partial: false,
            partial_reason: None,
            observed_at: MOCK_NOW,
        },
        MOCK_NOW,
    );
    admin.activity.begin(1);
    admin.activity.succeed(
        1,
        AuditSnapshot {
            version: Some("1.0".to_owned()),
            tailnet: Some("example.test".to_owned()),
            events: vec![
                audit_event(
                    MOCK_NOW.saturating_sub(120),
                    "alice@example.test",
                    "Approved route",
                    "subnet-gateway",
                ),
                audit_event(
                    MOCK_NOW.saturating_sub(3_600),
                    "bob@example.test",
                    "Changed DNS settings",
                    "example.test",
                ),
            ],
            start: "fictional-window-start".to_owned(),
            end: "fictional-window-end".to_owned(),
            observed_at: MOCK_NOW,
            delayed: false,
        },
        MOCK_NOW,
    );
    admin
}

fn local_device(
    id: &str,
    name: &str,
    dns_name: &str,
    addresses: Vec<&str>,
    advertised_routes: Vec<&str>,
    online: bool,
) -> LocalDevice {
    LocalDevice {
        id: DeviceId::new(id),
        public_key: None,
        display_name: name.to_owned(),
        hostname: name.to_owned(),
        dns_name: Some(dns_name.to_owned()),
        os: OperatingSystem::Linux,
        version: Some("1.98.9".to_owned()),
        owner_label: Some("Alice".to_owned()),
        user_id: Some("user-alice".to_owned()),
        tags: vec!["tag:server".to_owned()],
        tailscale_ips: addresses.into_iter().map(str::to_owned).collect(),
        advertised_routes: advertised_routes.into_iter().map(str::to_owned).collect(),
        current_endpoint: None,
        relay_region: None,
        path: ConnectionPath::Direct {
            latency_ms: Some(18),
        },
        online: Some(online),
        active: online,
        rx_bytes: Some(8_400_000),
        tx_bytes: Some(2_100_000),
        created_at: Some(MOCK_NOW.saturating_sub(7_776_000)),
        last_seen: Some(MOCK_NOW.saturating_sub(4)),
        last_handshake: Some(MOCK_NOW.saturating_sub(4)),
        exit_node: false,
        exit_node_option: false,
        ssh_host_keys_present: true,
        shared: false,
        capabilities: BTreeMap::from([("ssh".to_owned(), true)]),
    }
}

fn audit_event(timestamp: Timestamp, actor: &str, action: &str, target: &str) -> AuditEvent {
    AuditEvent {
        event_time: timestamp,
        event_time_text: timestamp.to_string(),
        event_type: Some("configuration".to_owned()),
        deferred_at: None,
        event_group_id: None,
        origin: Some("control API".to_owned()),
        actor: Some(AuditPrincipal {
            id: Some(actor.to_owned()),
            display: Some(actor.to_owned()),
            kind: Some("user".to_owned()),
        }),
        target: Some(AuditTarget {
            id: Some(target.to_owned()),
            display: Some(target.to_owned()),
            kind: Some("tailnet resource".to_owned()),
        }),
        action: Some(action.to_owned()),
        old: Some(Value::String("previous fictional value".to_owned())),
        new: Some(Value::String("current fictional value".to_owned())),
        action_details: Some("Fictional mock event".to_owned()),
        error: None,
    }
}

pub fn devices() -> Vec<Device> {
    vec![
        device(
            "dev-a01",
            "build-01",
            "build-01.example.com",
            Some("alice@example.com"),
            Some("Alice"),
            OperatingSystem::Linux,
            Liveness::Online,
            ConnectionPath::Direct {
                latency_ms: Some(18),
            },
            vec!["192.0.2.10", "2001:db8:42::10"],
            vec!["server", "prod"],
            Some(MOCK_NOW.saturating_sub(3)),
            DeviceCapabilities {
                exit_node: true,
                exit_node_option: false,
                subnet_router: true,
                ssh: true,
                funnel: false,
                shared: false,
                expired: false,
                approved: true,
            },
        ),
        device(
            "dev-b02",
            "studio-mac",
            "studio-mac.example.com",
            Some("alice@example.com"),
            Some("Alice"),
            OperatingSystem::MacOS,
            Liveness::Online,
            ConnectionPath::Derp {
                region: "fra".to_owned(),
            },
            vec!["192.0.2.11"],
            vec!["design"],
            Some(MOCK_NOW.saturating_sub(12)),
            DeviceCapabilities {
                exit_node: false,
                exit_node_option: false,
                subnet_router: false,
                ssh: true,
                funnel: true,
                shared: false,
                expired: false,
                approved: true,
            },
        ),
        device(
            "dev-c03",
            "win-lab",
            "win-lab.example.com",
            Some("bob@example.com"),
            Some("Bob"),
            OperatingSystem::Windows,
            Liveness::Offline,
            ConnectionPath::PeerRelay {
                peer: "build-01".to_owned(),
            },
            vec!["192.0.2.12"],
            vec!["lab", "shared"],
            Some(MOCK_NOW.saturating_sub(7_200)),
            DeviceCapabilities {
                exit_node: false,
                exit_node_option: false,
                subnet_router: true,
                ssh: false,
                funnel: false,
                shared: true,
                expired: false,
                approved: true,
            },
        ),
        device(
            "dev-d04",
            "pocket",
            "pocket.example.com",
            Some("carol@example.com"),
            Some("Carol"),
            OperatingSystem::IOS,
            Liveness::Unknown,
            ConnectionPath::NoPath,
            Vec::new(),
            vec!["mobile"],
            None,
            DeviceCapabilities {
                exit_node: false,
                exit_node_option: false,
                subnet_router: false,
                ssh: false,
                funnel: false,
                shared: false,
                expired: false,
                approved: true,
            },
        ),
        device(
            "dev-e05",
            "tablet",
            "tablet.example.com",
            Some("carol@example.com"),
            Some("Carol"),
            OperatingSystem::Android,
            Liveness::Online,
            ConnectionPath::Direct {
                latency_ms: Some(42),
            },
            vec!["198.51.100.5", "2001:db8:42::5"],
            vec!["mobile", "personal"],
            Some(MOCK_NOW.saturating_sub(60)),
            DeviceCapabilities {
                exit_node: false,
                exit_node_option: false,
                subnet_router: false,
                ssh: false,
                funnel: false,
                shared: false,
                expired: false,
                approved: true,
            },
        ),
        device(
            "dev-f06",
            "edge-relay",
            "edge-relay.example.com",
            Some("dana@example.com"),
            Some("Dana"),
            OperatingSystem::Unknown("plan9".to_owned()),
            Liveness::Online,
            ConnectionPath::PeerRelay {
                peer: "build-01".to_owned(),
            },
            vec!["203.0.113.6"],
            vec!["relay", "edge"],
            Some(MOCK_NOW.saturating_sub(90)),
            DeviceCapabilities {
                exit_node: true,
                exit_node_option: false,
                subnet_router: true,
                ssh: true,
                funnel: false,
                shared: false,
                expired: false,
                approved: true,
            },
        ),
        device(
            "dev-g07",
            "archive-node-with-a-deliberately-long-fictional-name",
            "archive.example.com",
            Some("erin@example.com"),
            Some("Erin"),
            OperatingSystem::Linux,
            Liveness::Offline,
            ConnectionPath::Derp {
                region: "syd".to_owned(),
            },
            vec!["203.0.113.7"],
            vec!["archive"],
            Some(MOCK_NOW.saturating_sub(86_400)),
            DeviceCapabilities {
                exit_node: false,
                exit_node_option: false,
                subnet_router: true,
                ssh: true,
                funnel: false,
                shared: false,
                expired: true,
                approved: true,
            },
        ),
        device(
            "dev-h08",
            "审批-标签",
            "pending.example.com",
            Some("faye@example.com"),
            Some("Faye"),
            OperatingSystem::Linux,
            Liveness::Unknown,
            ConnectionPath::NoPath,
            vec!["192.0.2.18"],
            vec!["pending", "新"],
            Some(MOCK_NOW.saturating_sub(400)),
            DeviceCapabilities {
                exit_node: false,
                exit_node_option: false,
                subnet_router: false,
                ssh: false,
                funnel: false,
                shared: false,
                expired: false,
                approved: false,
            },
        ),
        device(
            "dev-i09",
            "funnel-demo",
            "funnel.example.com",
            Some("glen@example.com"),
            Some("Glen"),
            OperatingSystem::MacOS,
            Liveness::Online,
            ConnectionPath::Direct { latency_ms: None },
            vec!["198.51.100.9"],
            vec!["demo", "public"],
            Some(MOCK_NOW.saturating_sub(15)),
            DeviceCapabilities {
                exit_node: false,
                exit_node_option: false,
                subnet_router: false,
                ssh: true,
                funnel: true,
                shared: false,
                expired: false,
                approved: true,
            },
        ),
        device(
            "dev-j10",
            "shared-laptop",
            "shared.example.com",
            None,
            Some("Shared workspace"),
            OperatingSystem::Windows,
            Liveness::Offline,
            ConnectionPath::Derp {
                region: "nyc".to_owned(),
            },
            Vec::new(),
            vec!["shared"],
            Some(MOCK_NOW.saturating_sub(12_000)),
            DeviceCapabilities {
                exit_node: false,
                exit_node_option: false,
                subnet_router: false,
                ssh: false,
                funnel: false,
                shared: true,
                expired: false,
                approved: true,
            },
        ),
        device(
            "dev-k11",
            "subnet-gateway",
            "gateway.example.com",
            Some("hugo@example.com"),
            Some("Hugo"),
            OperatingSystem::Linux,
            Liveness::Online,
            ConnectionPath::Direct {
                latency_ms: Some(7),
            },
            vec!["192.0.2.21", "2001:db8:42::21"],
            vec!["router", "office"],
            Some(MOCK_NOW.saturating_sub(5)),
            DeviceCapabilities {
                exit_node: true,
                exit_node_option: false,
                subnet_router: true,
                ssh: true,
                funnel: false,
                shared: false,
                expired: false,
                approved: true,
            },
        ),
        device(
            "dev-l12",
            "quiet-phone",
            "quiet-phone.example.com",
            Some("ivy@example.com"),
            Some("Ivy"),
            OperatingSystem::Android,
            Liveness::Offline,
            ConnectionPath::NoPath,
            vec!["192.0.2.22"],
            vec!["personal"],
            Some(MOCK_NOW.saturating_sub(172_800)),
            DeviceCapabilities {
                exit_node: false,
                exit_node_option: false,
                subnet_router: false,
                ssh: false,
                funnel: false,
                shared: false,
                expired: false,
                approved: true,
            },
        ),
        device(
            "dev-m13",
            "relay-observer",
            "relay-observer.example.com",
            Some("jules@example.com"),
            Some("Jules"),
            OperatingSystem::Unknown("haiku".to_owned()),
            Liveness::Online,
            ConnectionPath::Derp {
                region: "ams".to_owned(),
            },
            vec!["203.0.113.13"],
            vec!["relay"],
            Some(MOCK_NOW.saturating_sub(33)),
            DeviceCapabilities {
                exit_node: false,
                exit_node_option: false,
                subnet_router: false,
                ssh: true,
                funnel: false,
                shared: false,
                expired: false,
                approved: true,
            },
        ),
        device(
            "dev-n14",
            "approval-expired",
            "approval-expired.example.com",
            Some("kira@example.com"),
            Some("Kira"),
            OperatingSystem::IOS,
            Liveness::Unknown,
            ConnectionPath::NoPath,
            vec!["198.51.100.14"],
            vec!["expired", "approval"],
            Some(MOCK_NOW.saturating_sub(604_800)),
            DeviceCapabilities {
                exit_node: false,
                exit_node_option: false,
                subnet_router: false,
                ssh: false,
                funnel: false,
                shared: false,
                expired: true,
                approved: false,
            },
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn device(
    id: &str,
    display_name: &str,
    hostname: &str,
    owner: Option<&str>,
    owner_label: Option<&str>,
    os: OperatingSystem,
    liveness: Liveness,
    path: ConnectionPath,
    addresses: Vec<&str>,
    tags: Vec<&str>,
    last_seen: Option<Timestamp>,
    capabilities: DeviceCapabilities,
) -> Device {
    Device {
        id: DeviceId::new(id),
        display_name: display_name.to_owned(),
        hostname: hostname.to_owned(),
        owner: owner.map(str::to_owned),
        owner_label: owner_label.map(str::to_owned),
        os,
        version: Some("1.98.9".to_owned()),
        liveness,
        path,
        addresses: addresses.into_iter().map(str::to_owned).collect(),
        advertised_routes: Vec::new(),
        tags: tags.into_iter().map(str::to_owned).collect(),
        last_seen,
        created_at: Some(MOCK_NOW.saturating_sub(90 * 86_400)),
        rx_bytes: None,
        tx_bytes: None,
        capabilities,
    }
}
