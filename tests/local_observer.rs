use std::fs;
use std::path::{Path, PathBuf};
use tale::app::{App, SourceMode};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::device::ConnectionPath;
use tale::domain::diagnostic::DiagnosticPath;
use tale::domain::redaction::{DiagnosticReportInput, Redactor, redact_diagnostic_report};
use tale::domain::source::{
    ExecutableSource, LocalCapabilities, LocalCliState, LocalExecutable, LocalFailure,
    LocalFailureKind, LocalResource, LocalResourceStatus, LocalState,
};
use tale::effect::Effect;
use tale::event::{Event, LocalEvent};
use tale::local::client::{
    ClientError, ExecutableError, ExecutableResolution, HostPlatform, resolve_executable,
};
use tale::local::daemon::decode_status;
use tale::local::diagnostics::{
    DnsRecordType, WhoisProtocol, format_whois_detail, parse_dns_query, parse_dns_status,
    parse_netcheck_json, parse_netcheck_lines, parse_ping_line, parse_whois, summarize_ping,
    validate_dns_query, validate_whois_target,
};
use tale::local::dto::decode_version;
use tale::local::process::LocalProcessError;
use tale::paths::{PathEnvironment, Platform};

const VERSION: &str = include_str!("fixtures/tailscale/1.98.9/linux/version.json");
const STATUS: &str = include_str!("fixtures/tailscale/1.98.9/linux/status.json");
const PING: &str = include_str!("fixtures/tailscale/1.98.9/linux/ping.txt");
const NETCHECK: &str = include_str!("fixtures/tailscale/1.98.9/linux/netcheck.json");
const NETCHECK_LINES: &str = include_str!("fixtures/tailscale/1.98.9/linux/netcheck.jsonl");
const DNS_STATUS: &str = include_str!("fixtures/tailscale/1.98.9/linux/dns-status.json");
const DNS_QUERY_A: &str = include_str!("fixtures/tailscale/1.98.9/linux/dns-query-A.json");
const WHOIS: &str = include_str!("fixtures/tailscale/1.98.9/linux/whois.json");

fn timestamp() -> u64 {
    1_754_000_000
}

#[test]
fn diagnostic_redaction_preserves_multiline_field_layout() {
    let mut redactor = Redactor::new();
    assert_eq!(
        redactor.text("device        peer-1\naddress       100.64.0.11"),
        "device        peer-1\naddress       address-1"
    );
}

#[test]
fn version_fixture_accepts_unknown_fields_and_optional_daemon_version() {
    let version = decode_version(VERSION);
    assert!(version.is_ok());
    if let Ok(version) = version {
        assert_eq!(version.version, "1.98.9");
        assert_eq!(version.daemon_version.as_deref(), Some("1.98.9"));
        assert_eq!(version.build.as_deref(), Some("fixture-commit"));
    }
    assert!(decode_version(r#"{"daemonVersion":"1.98.9"}"#).is_err());
    assert!(decode_version("not-json").is_err());
    let missing_daemon = decode_version(r#"{"version":"1.98.9","future":true}"#);
    assert!(missing_daemon.is_ok());
    if let Ok(value) = missing_daemon {
        assert_eq!(value.daemon_version, None);
    }
}

#[test]
fn status_fixture_maps_wire_dto_to_rich_local_snapshot() {
    let snapshot = decode_status(
        VERSION,
        "1.98.9".to_owned(),
        Some("1.98.9".to_owned()),
        timestamp(),
    );
    assert!(snapshot.is_err());
    let snapshot = decode_status(
        STATUS,
        "1.98.9".to_owned(),
        Some("1.98.9".to_owned()),
        timestamp(),
    );
    assert!(snapshot.is_ok());
    if let Ok(snapshot) = snapshot {
        assert_eq!(snapshot.peers.len(), 3);
        assert_eq!(
            snapshot.self_node.dns_name.as_deref(),
            Some("observer.tail.example.ts.net.")
        );
        assert_eq!(
            snapshot.self_node.owner_label.as_deref(),
            Some("Alice Example")
        );
        assert_eq!(
            snapshot.self_node.path,
            ConnectionPath::Direct { latency_ms: None }
        );
        let derp = snapshot
            .peers
            .iter()
            .find(|peer| peer.id.0 == "nodekey:derp");
        assert!(derp.is_some());
        if let Some(derp) = derp {
            assert_eq!(derp.os.label(), "plan9");
            assert_eq!(
                derp.path,
                ConnectionPath::Derp {
                    region: "fra".to_owned()
                }
            );
        }
        let peer_relay = snapshot
            .peers
            .iter()
            .find(|peer| peer.id.0 == "nodekey:peer-relay");
        assert!(peer_relay.is_some());
        if let Some(peer_relay) = peer_relay {
            assert_eq!(
                peer_relay.path,
                ConnectionPath::PeerRelay {
                    peer: "nodekey:direct".to_owned(),
                }
            );
        }
        let direct = snapshot
            .peers
            .iter()
            .find(|peer| peer.id.0 == "nodekey:direct");
        assert!(direct.is_some());
        if let Some(direct) = direct {
            assert!(!direct.shared);
            assert!(
                direct
                    .advertised_routes
                    .iter()
                    .any(|route| route == "10.30.0.0/16")
            );
        }
    }
    let missing_self = decode_status(
        r#"{"BackendState":"Running","Peer":{}}"#,
        "1.98.9".to_owned(),
        None,
        timestamp(),
    );
    assert!(missing_self.is_err());
    let empty_peers = decode_status(
        r#"{"Self":{"ID":"nodekey:self","HostName":"self"},"Peer":{}}"#,
        "1.98.9".to_owned(),
        None,
        timestamp(),
    );
    assert!(empty_peers.is_ok());
    let missing_identity = decode_status(
        r#"{"Self":{"HostName":"self"}}"#,
        "1.98.9".to_owned(),
        None,
        timestamp(),
    );
    assert!(missing_identity.is_err());
}

#[test]
fn status_preserves_capability_identifiers_verbatim() {
    let status = r#"{
        "BackendState": "Running",
        "Self": {
            "ID": "nodekey:self",
            "HostName": "self",
            "Capabilities": {
                "funnel": true,
                "https://tailscale.com/cap/file-sharing": true,
                "https://tailscale.com/cap/funnel-ports?ports=443,8443,10000": true,
                "https://tailscale.com/cap/is-admin": true
            }
        },
        "Peer": {}
    }"#;
    let snapshot = decode_status(status, "1.98.9".to_owned(), None, timestamp());
    assert!(snapshot.is_ok());
    if let Ok(snapshot) = snapshot {
        for capability in [
            "https://tailscale.com/cap/file-sharing",
            "https://tailscale.com/cap/funnel-ports?ports=443,8443,10000",
            "https://tailscale.com/cap/is-admin",
        ] {
            assert_eq!(snapshot.self_node.capabilities.get(capability), Some(&true));
        }
        assert!(
            !snapshot
                .self_node
                .capabilities
                .contains_key("httpstailscalecomcapfilesharing")
        );
        assert!(snapshot.self_node.to_display_device().capabilities.funnel);
    }
}

#[test]
fn daemon_state_classification_distinguishes_transport_auth_and_health() {
    let needs_login = decode_status(
        r#"{"BackendState":"NeedsLogin","AuthURL":"https://login.example","Self":{"ID":"self"}}"#,
        "1.98.9".to_owned(),
        None,
        timestamp(),
    );
    assert!(needs_login.is_ok());
    if let Ok(snapshot) = needs_login {
        assert!(matches!(
            snapshot.backend_state,
            LocalState::NeedsLogin { auth_url: Some(_) }
        ));
    }
    let degraded = decode_status(
        r#"{"BackendState":"Running","Health":["DNS issue"],"Self":{"ID":"self"}}"#,
        "1.98.9".to_owned(),
        None,
        timestamp(),
    );
    assert!(degraded.is_ok());
    if let Ok(snapshot) = degraded {
        assert!(matches!(
            snapshot.backend_state,
            LocalState::Degraded { .. }
        ));
    }
    let stopped = decode_status(
        r#"{"BackendState":"Stopped","Self":{"ID":"self"}}"#,
        "1.98.9".to_owned(),
        None,
        timestamp(),
    );
    assert!(stopped.is_ok());
    if let Ok(snapshot) = stopped {
        assert_eq!(snapshot.backend_state, LocalState::Stopped);
    }
    assert_eq!(
        ClientError::Process(LocalProcessError::NotFound).state("1.98.9"),
        LocalState::ExecutableMissing
    );
    assert_eq!(
        ClientError::Process(LocalProcessError::PermissionDenied).state("1.98.9"),
        LocalState::ExecutableDenied
    );
    let login_error = ClientError::NonZero {
        operation: "status".to_owned(),
        status: Some(1),
        detail: "not logged in".to_owned(),
    };
    assert!(matches!(
        login_error.state("1.98.9"),
        LocalState::NeedsLogin { .. }
    ));
    let permission_error = ClientError::NonZero {
        operation: "status".to_owned(),
        status: Some(1),
        detail: "permission denied".to_owned(),
    };
    assert!(matches!(
        permission_error.state("1.98.9"),
        LocalState::PermissionDenied { .. }
    ));
}

#[test]
fn local_resource_preserves_last_good_snapshot_after_failure() {
    let snapshot = decode_status(STATUS, "1.98.9".to_owned(), None, timestamp());
    assert!(snapshot.is_ok());
    if let Ok(snapshot) = snapshot {
        let mut resource = LocalResource::new();
        resource.begin(1, timestamp());
        assert!(resource.succeed(1, snapshot.clone()));
        resource.begin(2, timestamp().saturating_add(2));
        let failure = LocalFailure::new(
            LocalFailureKind::InvalidOutput,
            "status",
            "invalid output",
            "bounded fixture failure",
            true,
        );
        assert!(resource.fail(2, failure));
        assert_eq!(resource.snapshot, Some(snapshot));
        assert_eq!(resource.status, LocalResourceStatus::Stale);
        assert_eq!(resource.consecutive_failures, 1);
    }
}

#[test]
fn ping_fixture_captures_path_transitions_and_checked_summary() {
    let samples = PING
        .lines()
        .enumerate()
        .filter_map(|(index, line)| parse_ping_line(line, index as u64 + 1, timestamp()))
        .collect::<Vec<_>>();
    assert_eq!(samples.len(), 3);
    assert!(matches!(samples[0].path, DiagnosticPath::Derp { .. }));
    assert_eq!(samples[1].path, DiagnosticPath::Direct);
    assert!(matches!(samples[2].path, DiagnosticPath::PeerRelay { .. }));
    let summary = summarize_ping(Some(10), &samples);
    assert_eq!(summary.received, 3);
    assert_eq!(summary.loss_percent, Some(70));
    assert_eq!(summary.minimum_ms, Some(4));
    assert_eq!(summary.average_ms, Some(21));
    assert_eq!(summary.maximum_ms, Some(48));
    assert!(summary.reached_direct);
    let empty = summarize_ping(Some(10), &[]);
    assert_eq!(empty.average_ms, None);
    assert_eq!(empty.loss_percent, Some(100));
    assert!(parse_ping_line("stderr only", 1, timestamp()).is_none());
}

#[test]
fn netcheck_fixture_accepts_partial_measurements_and_malformed_live_lines() {
    let observation = parse_netcheck_json(NETCHECK, timestamp());
    assert!(observation.is_ok());
    if let Ok(observation) = observation {
        assert_eq!(observation.udp, Some(true));
        assert_eq!(observation.ipv6, Some(true));
        assert_eq!(observation.nearest_derp.as_deref(), Some("fra"));
        assert_eq!(
            observation
                .derp_latency
                .first()
                .and_then(|value| value.latency_ms),
            Some(18)
        );
        assert!(
            observation
                .sensitive_addresses
                .iter()
                .any(|value| value == "198.51.100.10:41641")
        );
    }
    let lines = NETCHECK_LINES
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (latest, errors) = parse_netcheck_lines(&lines, timestamp());
    assert!(latest.is_some());
    assert_eq!(errors.len(), 1);
    if let Some(latest) = latest {
        assert_eq!(latest.nearest_derp.as_deref(), Some("fra"));
        assert_eq!(latest.udp, Some(true));
    }
    assert!(parse_netcheck_json(r#"{"IPv6":true}"#, timestamp()).is_ok());
    assert!(parse_netcheck_json("not-json", timestamp()).is_err());
}

#[test]
fn dns_and_whois_fixtures_preserve_observable_results() {
    let status = parse_dns_status(DNS_STATUS, timestamp());
    assert!(status.is_ok());
    if let Ok(status) = status {
        assert_eq!(status.forwarder_enabled, Some(true));
        assert_eq!(
            status.resolvers.first().map(String::as_str),
            Some("100.100.100.100")
        );
        assert_eq!(status.split_routes.len(), 2);
    }
    let query = parse_dns_query(
        DNS_QUERY_A,
        "build.tail.example.ts.net".to_owned(),
        DnsRecordType::A,
        timestamp(),
    );
    assert!(query.is_ok());
    if let Ok(query) = query {
        assert_eq!(query.result_class, "NOERROR");
        assert_eq!(query.answers.len(), 1);
        assert_eq!(query.answers[0].value, "100.64.0.11");
    }
    for record_type in ["A", "AAAA", "CNAME", "MX", "NS", "PTR", "SRV", "TXT"] {
        assert!(validate_dns_query("example.test", record_type).is_ok());
    }
    assert!(validate_dns_query("example test", "A").is_err());
    assert!(validate_dns_query("example.test;$(touch)", "A").is_ok());
    assert!(validate_dns_query("example.test", "NAPTR").is_err());

    let whois = parse_whois(WHOIS, "100.64.0.11".to_owned(), timestamp());
    assert!(whois.is_ok());
    if let Ok(whois) = whois {
        assert_eq!(whois.machine_id.as_deref(), Some("nodekey:direct"));
        assert_eq!(
            whois.machine_name.as_deref(),
            Some("build-fixture.example.ts.net.")
        );
        assert_eq!(whois.addresses.len(), 2);
        assert_eq!(whois.user_identity.as_deref(), Some("alice@example.com"));
        assert_eq!(whois.capabilities, ["ssh", "subnet-router"]);
        let detail = format_whois_detail(&whois);
        assert!(detail.contains("device        build-fixture"));
        assert!(detail.contains("DNS name      build-fixture.example.ts.net"));
        assert!(detail.contains("user          alice@example.com"));
        assert!(!detail.contains("{\""));
    }
    for target in ["192.0.2.1", "::1", "[fd7a::1]:41641", "192.0.2.1:80"] {
        assert!(validate_whois_target(target).is_ok());
    }
    assert!(validate_whois_target("192.0.2.1;$(touch x)").is_err());
    assert!(validate_whois_target("[fd7a::1]:bad").is_err());
    assert!(WhoisProtocol::Tcp.label() == "tcp");
}

#[test]
fn executable_resolution_obeys_precedence_and_platform_rules() {
    let root = temp_dir("resolution");
    assert!(fs::create_dir_all(&root).is_ok());
    let cli = root.join("cli path/tailscale");
    let env = root.join("env path/tailscale");
    let config = root.join("config path/tailscale");
    let path_dir = root.join("path");
    assert!(fs::create_dir_all(cli.parent().map_or(Path::new("."), |value| value)).is_ok());
    assert!(fs::create_dir_all(env.parent().map_or(Path::new("."), |value| value)).is_ok());
    assert!(fs::create_dir_all(config.parent().map_or(Path::new("."), |value| value)).is_ok());
    assert!(fs::create_dir_all(&path_dir).is_ok());
    for path in [&cli, &env, &config, &path_dir.join("tailscale")] {
        assert!(fs::write(path, b"fixture").is_ok());
        make_executable(path);
    }
    let input = ExecutableResolution {
        cli_path: Some(cli.clone()),
        environment_path: Some(env.clone().into_os_string()),
        config_path: Some(config.clone()),
        path: Some(path_dir.clone().into_os_string()),
        socket_path: None,
        platform: HostPlatform::Unix,
    };
    let resolved = resolve_executable(&input);
    assert!(resolved.is_ok());
    if let Ok(resolved) = resolved {
        assert_eq!(resolved.path, cli);
        assert_eq!(resolved.source, ExecutableSource::Cli);
    }
    let mut input = input;
    input.cli_path = None;
    let resolved = resolve_executable(&input);
    assert!(resolved.is_ok());
    if let Ok(resolved) = resolved {
        assert_eq!(resolved.path, env);
        assert_eq!(resolved.source, ExecutableSource::Environment);
    }
    input.environment_path = None;
    let resolved = resolve_executable(&input);
    assert!(resolved.is_ok());
    if let Ok(resolved) = resolved {
        assert_eq!(resolved.path, config);
        assert_eq!(resolved.source, ExecutableSource::Config);
    }
    input.config_path = None;
    let resolved = resolve_executable(&input);
    assert!(resolved.is_ok());
    if let Ok(resolved) = resolved {
        assert_eq!(resolved.path, path_dir.join("tailscale"));
        assert_eq!(resolved.source, ExecutableSource::Path);
    }
    let windows_dir = root.join("windows");
    assert!(fs::create_dir_all(&windows_dir).is_ok());
    let windows_name = windows_dir.join("tailscale.exe");
    assert!(fs::write(&windows_name, b"fixture").is_ok());
    let windows = resolve_executable(&ExecutableResolution {
        cli_path: None,
        environment_path: None,
        config_path: None,
        socket_path: None,
        path: Some(windows_dir.clone().into_os_string()),
        platform: HostPlatform::Windows,
    });
    assert!(windows.is_ok());
    let missing = resolve_executable(&ExecutableResolution {
        cli_path: Some(root.join("missing")),
        environment_path: None,
        config_path: None,
        socket_path: None,
        path: None,
        platform: HostPlatform::Unix,
    });
    // The failure names the path it checked so the message can show it.
    assert_eq!(
        missing,
        Err(ExecutableError::NotFound {
            searched: vec![root.join("missing")],
        })
    );
    if let Err(error) = missing {
        assert_eq!(
            error.searched(),
            vec![root.join("missing").display().to_string()]
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn no_local_bootstrap_has_no_local_effect() {
    let root = temp_dir("no-local");
    assert!(fs::create_dir_all(&root).is_ok());
    let cli = Cli {
        command: None,
        profile: None,
        config: Some(root.join("missing.toml")),
        view: None,
        read_only: false,
        no_local: true,
        tailscale_path: Some(root.join("does not exist")),
        tailscale_socket: None,
        mock: false,
    };
    let environment = EnvironmentValues {
        config_file: None,
        tailscale_path: Some(root.join("environment").to_string_lossy().into_owned()),
        tailscale_socket: None,
        no_color: false,
    };
    let paths = PathEnvironment {
        platform: Platform::Unix,
        current_dir: root.clone(),
        xdg_config_home: Some(root.join("config")),
        home: Some(root.join("home")),
        xdg_state_home: Some(root.join("state")),
        xdg_cache_home: Some(root.join("cache")),
        appdata: None,
        localappdata: None,
    };
    let config = config::resolve(&cli, &environment, &paths);
    assert!(config.is_ok());
    if let Ok(config) = config {
        let mut app = App::new(config);
        assert_eq!(app.source_mode, SourceMode::Unavailable);
        assert!(app.bootstrap_effects().is_empty());
        let _ = app.update(Event::Tick(std::time::Instant::now()));
    }
    let mock_cli = Cli {
        command: None,
        profile: None,
        config: Some(root.join("missing.toml")),
        view: None,
        read_only: false,
        no_local: false,
        tailscale_path: Some(root.join("does not exist")),
        tailscale_socket: None,
        mock: true,
    };
    let mock_config = config::resolve(&mock_cli, &environment, &paths);
    assert!(mock_config.is_ok());
    if let Ok(mock_config) = mock_config {
        let mut app = App::new(mock_config);
        assert_eq!(app.source_mode, SourceMode::Mock);
        let effects = app.bootstrap_effects();
        assert!(
            effects
                .iter()
                .all(|effect| !matches!(effect, Effect::StartLocalDiscovery { .. }))
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn redacted_diagnostic_report_is_deterministic_and_secret_free() {
    let input = DiagnosticReportInput {
        tale_version: "0.1.0".to_owned(),
        tailscale_version: "1.98.9".to_owned(),
        platform: "linux".to_owned(),
        local_state: "running".to_owned(),
        health_categories: vec!["alice@example.com has DNS trouble at /Users/alice".to_owned()],
        peer_identity: Some("nodekey:secret".to_owned()),
        peer_os: Some("linux".to_owned()),
        peer_path: Some("direct".to_owned()),
        ping: None,
        netcheck: None,
        dns: None,
        observed_at: timestamp(),
        stale: false,
        names: vec!["Alice Example".to_owned(), "tailnet.example".to_owned()],
        addresses: vec!["100.64.0.11".to_owned(), "198.51.100.10:41641".to_owned()],
        paths: vec!["/Users/alice/project".to_owned()],
        public_endpoints: vec!["198.51.100.10:41641".to_owned()],
    };
    let first = redact_diagnostic_report(&input);
    let second = redact_diagnostic_report(&input);
    assert_eq!(first, second);
    assert!(!first.text.contains("Alice Example"));
    assert!(!first.text.contains("alice@example.com"));
    assert!(!first.text.contains("100.64.0.11"));
    assert!(!first.text.contains("/Users/alice"));
    assert!(first.text.contains("Peer: id-1"));
    assert!(first.text.contains("Health: dns"));
}

#[tokio::test(start_paused = true)]
async fn watcher_disconnect_marks_resources_stale_and_refresh_is_targeted() {
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    let app = local_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let _ = app.bootstrap_effects();
        let snapshot = decode_status(STATUS, "1.98.9".to_owned(), None, timestamp());
        assert!(snapshot.is_ok());
        if let Ok(snapshot) = snapshot {
            let generation = 1;
            let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
                generation,
                snapshot: Box::new(snapshot),
            })));
            assert_eq!(app.local_resource.status, LocalResourceStatus::Fresh);
            let _ = app.update(Event::Local(Box::new(LocalEvent::WatcherDisconnected {
                generation: 1,
                failure: LocalFailure::new(
                    LocalFailureKind::DaemonUnavailable,
                    "watch-ipn-bus",
                    "watcher disconnected",
                    "fixture disconnect",
                    true,
                ),
            })));
            assert_eq!(app.local_resource.status, LocalResourceStatus::Stale);
            assert!(app.local_resource.snapshot.is_some());
            app.local_executable = Some(LocalExecutable {
                path: PathBuf::from("tailscale"),
                socket_path: None,
                source: ExecutableSource::Path,
                version: "1.98.9".to_owned(),
                daemon_version: None,
                build: None,
                capabilities: LocalCapabilities::all_supported(),
            });
            let effects = app.dispatch_action(tale::action::ActionId::ViewRefresh);
            assert!(
                effects
                    .iter()
                    .any(|effect| matches!(effect, Effect::StartLocalSnapshotRefresh { .. }))
            );
        }
    }
}

fn local_app() -> Option<App> {
    let root = temp_dir("app");
    if fs::create_dir_all(&root).is_err() {
        return None;
    }
    let cli = Cli {
        command: None,
        profile: None,
        config: Some(root.join("missing.toml")),
        view: None,
        read_only: false,
        no_local: false,
        tailscale_path: None,
        tailscale_socket: None,
        mock: false,
    };
    let environment = EnvironmentValues {
        config_file: None,
        tailscale_path: None,
        tailscale_socket: None,
        no_color: false,
    };
    let paths = PathEnvironment {
        platform: Platform::Unix,
        current_dir: root.clone(),
        xdg_config_home: Some(root.join("config")),
        home: Some(root.join("home")),
        xdg_state_home: Some(root.join("state")),
        xdg_cache_home: Some(root.join("cache")),
        appdata: None,
        localappdata: None,
    };
    config::resolve(&cli, &environment, &paths)
        .ok()
        .map(App::new)
}

fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tale-local-observer-{}-{}",
        std::process::id(),
        label
    ))
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);
            let _ = fs::set_permissions(path, permissions);
        }
    }
}

#[tokio::test(start_paused = true)]
async fn a_newer_status_snapshot_cannot_discard_cli_discovery() {
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    let app = local_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let _ = app.bootstrap_effects();
        assert!(app.local_executable.is_none());

        // The daemon watcher reserves a fresh status generation on every read,
        // and it wins that race because discovery has to run subprocesses.
        let snapshot = decode_status(STATUS, "1.98.9".to_owned(), None, timestamp());
        assert!(snapshot.is_ok());
        if let Ok(snapshot) = snapshot {
            for generation in 1..=4 {
                let _ = app.update(Event::Local(Box::new(LocalEvent::StatusStarted {
                    generation,
                    attempted_at: timestamp(),
                })));
                let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
                    generation,
                    snapshot: Box::new(snapshot.clone()),
                })));
            }
        }
        assert!(app.local_resource.generation >= 4);

        // Discovery started before any of that and still counts when it lands.
        let _ = app.update(Event::Local(Box::new(LocalEvent::DiscoverySucceeded {
            generation: 1,
            executable: LocalExecutable {
                path: PathBuf::from("/usr/local/bin/tailscale"),
                socket_path: None,
                source: ExecutableSource::Path,
                version: "1.102.2".to_owned(),
                daemon_version: None,
                build: None,
                capabilities: LocalCapabilities::all_supported(),
            },
        })));
        assert!(
            app.local_executable.is_some(),
            "a later status read must not discard the discovered executable"
        );
        assert_eq!(app.local_cli_state, LocalCliState::Available);
    }
}

#[tokio::test(start_paused = true)]
async fn a_superseded_discovery_result_is_still_ignored() {
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    let app = local_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let _ = app.bootstrap_effects();
        // A second discovery run supersedes the first, so the stale answer loses.
        let _ = app.update(Event::Local(Box::new(LocalEvent::DiscoveryStarted {
            generation: 7,
        })));
        let _ = app.update(Event::Local(Box::new(LocalEvent::DiscoveryFailed {
            generation: 2,
            failure: LocalFailure::new(
                LocalFailureKind::ExecutableMissing,
                "executable discovery",
                "stale",
                "stale",
                false,
            ),
        })));
        assert_ne!(
            app.local_cli_state,
            LocalCliState::Missing {
                detail: "stale. stale".to_owned()
            }
        );
    }
}
