use std::fs;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tale::app::{App, Route};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::access_explorer::{AccessDecision, AccessResult, PolicySource};
use tale::domain::flow::{FlowMessage, FlowMode, FlowSnapshot, FlowWindow};
use tale::domain::health::{Finding, ObservedFact, Severity};
use tale::domain::log_stream::{
    LogStreamConfiguration, LogStreamDestination, LogStreamStatus, LogType, SecretAction,
};
use tale::domain::policy::PolicySnapshot;
use tale::domain::webhook::{DestinationType, SubscriptionSet, WebhookEndpoint};
use tale::event::{Event, InputEvent};
use tale::paths::{PathEnvironment, Platform};

/// Each caller gets its own directory. Tests run in parallel, and two of them
/// writing one `config.toml` is a race that resolves a half-written file.
fn admin_app(name: &str) -> Option<App> {
    let root = std::env::temp_dir().join(format!("tale-admin-ui-{}-{name}", std::process::id()));
    let _ = fs::create_dir_all(&root);
    let config_path = root.join("config.toml");
    let write = fs::write(
        &config_path,
        "[profiles.audit]\ntailnet = \"example.test\"\ncredential = \"audit\"\ncredential_backend = \"file\"\ncredential_file = \"credentials.toml\"\n",
    );
    if write.is_err() {
        return None;
    }
    let cli = Cli {
        command: None,
        // A profile is active only when it is asked for; the fixture asks.
        profile: Some("audit".to_owned()),
        config: Some(config_path),
        view: None,
        read_only: true,
        no_local: true,
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
    let resolved = config::resolve(&cli, &environment, &paths).ok()?;
    let mut app = App::new(resolved);
    app.admin.devices.begin(1);
    app.admin.devices.succeed(1, Vec::new(), 1_785_751_200);
    app.admin.policy.begin(1);
    app.admin.policy.succeed(
        1,
        PolicySnapshot {
            source_bytes: b"{\n  // fictional policy\n}\n".to_vec(),
            content_type: "application/hujson".to_owned(),
            fetched_at: 1_785_751_200,
            content_hash: "fictional-hash".to_owned(),
            etag: None,
        },
        1_785_751_200,
    );
    Some(app)
}

fn render_lines(app: &App, width: u16, height: u16) -> Option<Vec<String>> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).ok()?;
    terminal.draw(|frame| tale::ui::render(frame, app)).ok()?;
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::with_capacity(usize::from(height));
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            if let Some(cell) = buffer.cell((x, y)) {
                line.push_str(cell.symbol());
            }
        }
        lines.push(line);
    }
    Some(lines)
}

fn press(app: &mut App, code: KeyCode) {
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        code,
        KeyModifiers::NONE,
    ))));
}

#[test]
fn admin_views_render_partial_and_read_only_states_at_required_sizes() {
    let app = admin_app("render");
    assert!(app.is_some());
    if let Some(mut app) = app {
        for route in [
            Route::Overview,
            Route::Users,
            Route::Routes,
            Route::Dns,
            Route::Access,
            Route::Credentials,
            Route::Tasks,
            Route::Audit,
        ] {
            app.set_route(route);
            for (width, height) in [(60, 18), (80, 24), (110, 30), (160, 45)] {
                let lines = render_lines(&app, width, height);
                assert!(lines.is_some());
                if let Some(lines) = lines {
                    assert!(lines.iter().all(|line| !line.contains('\n')));
                    // The header always states the connection, tall or compact.
                    assert!(
                        lines.iter().take(6).any(|line| line.contains("connection")
                            || line.contains("Connected")
                            || line.contains("Simulated")
                            || line.contains("Local")),
                        "no connection state at {width}x{height} on {route:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn phase_eight_sections_render_derived_and_authoritative_states() {
    let app = admin_app("sections");
    assert!(app.is_some());
    let Some(mut app) = app else {
        return;
    };
    app.health_findings = vec![Finding {
        id: "finding-ui".to_owned(),
        rule_id: "device-approval-pending".to_owned(),
        severity: Severity::Warning,
        title: "Device approval is pending".to_owned(),
        observed_facts: vec![ObservedFact::from_source(
            "approval",
            "pending",
            "fixture",
            1_785_751_200,
        )],
        observed_at: 1_785_751_200,
        affected_resource_ids: vec!["device-ui".to_owned()],
        truncated_affected_resource_count: 0,
        source_ids: vec!["fixture".to_owned()],
        explanation: "authoritative fixture observation".to_owned(),
        suggested_action_ids: Vec::new(),
        derived: true,
    }];
    let now = time::OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1_785_751_200);
    let window = FlowWindow::new(now - time::Duration::hours(1), now, now);
    assert!(window.is_ok());
    if let Ok(window) = window {
        let flow = FlowSnapshot::from_messages(
            window,
            vec![FlowMessage {
                node_id: "raw-node-id".to_owned(),
                reporting_node_name: None,
                logged: "2026-08-04T00:02:00Z".to_owned(),
                start: "2026-08-04T00:00:00Z".to_owned(),
                end: "2026-08-04T00:01:00Z".to_owned(),
                source_node: None,
                destination_nodes: Vec::new(),
                virtual_traffic: Vec::new(),
                subnet_traffic: Vec::new(),
                exit_traffic: Vec::new(),
                physical_traffic: Vec::new(),
            }],
            FlowMode::Raw,
            1_785_751_200,
        );
        assert!(flow.is_ok());
        if let Ok(flow) = flow {
            app.flow_snapshot = Some(flow);
        }
    }
    let subscriptions = SubscriptionSet::from_wire(
        vec!["device".to_owned()],
        vec!["futureEventFromServer".to_owned()],
    );
    assert!(subscriptions.is_ok());
    if let Ok(subscriptions) = subscriptions {
        app.webhooks = vec![WebhookEndpoint {
            stable_id: "webhook-ui".to_owned(),
            endpoint_url: "https://hooks.example.test/ui".to_owned(),
            destination_type: DestinationType::Slack,
            subscriptions,
            creator_login_name: None,
            created_at: None,
            last_modified_at: None,
            status: "observed".to_owned(),
            last_result: None,
            observed_at: 1_785_751_200,
            source_id: "fixture".to_owned(),
        }];
    }
    app.log_stream_configurations.insert(
        LogType::Network,
        LogStreamConfiguration {
            log_type: LogType::Network,
            enabled: true,
            destination: LogStreamDestination {
                kind: "splunk".to_owned(),
                identity: "https://logs.example.test".to_owned(),
            },
            secret_action: SecretAction::KeepExisting,
            observed_at: 1_785_751_200,
            source_id: "fixture".to_owned(),
        },
    );
    app.log_stream_statuses.insert(
        LogType::Network,
        LogStreamStatus {
            log_type: LogType::Network,
            configured: true,
            healthy: Some(true),
            status: "publishing observed".to_owned(),
            last_observation: Some(1_785_751_200),
            source_id: "fixture".to_owned(),
        },
    );
    app.access_explorer_result = Some(AccessResult {
        decision: AccessDecision::Indeterminate,
        policy_hash: "fixture-hash".to_owned(),
        input: "alice@example.test".to_owned(),
        requested_at: 1_785_751_200,
        limitations: vec!["empty preview envelope".to_owned()],
        matched_users: Vec::new(),
        matched_ports: Vec::new(),
        rule_locations: Vec::new(),
        source: PolicySource::CurrentRemote,
    });
    for (route, expected) in [
        (Route::Overview, "needs attention"),
        (Route::Audit, "Flow Logs"),
        (Route::Audit, "Log streams"),
        (Route::Audit, "Webhooks"),
        (Route::Access, "Access Explorer"),
    ] {
        app.set_route(route);
        let lines = render_lines(&app, 160, 45);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert!(
                lines.iter().any(|line| line.contains(expected)),
                "missing {expected}"
            );
        }
    }
}

#[test]
fn overview_is_a_responsive_operational_inbox_with_selected_evidence() {
    let Some(mut app) = admin_app("overview-inbox") else {
        return;
    };
    let Ok(dto) = serde_json::from_str::<tale::admin::dto::DevicesResponse>(include_str!(
        "fixtures/admin/devices.json"
    )) else {
        return;
    };
    let Ok(devices) = tale::admin::devices::decode_devices(dto.devices, app.now) else {
        return;
    };
    let Some(device) = devices.first() else {
        return;
    };
    let device_id = device.stable_id.clone();
    let device_name = device.display_name().to_owned();
    app.admin.devices.snapshot = Some(devices);
    app.refresh_device_view();
    let expired_at = app.now.saturating_sub(86_400);
    app.health_findings = vec![
        Finding {
            id: "first-finding".to_owned(),
            rule_id: "device-key-expired".to_owned(),
            severity: Severity::Critical,
            title: "Device key is expired".to_owned(),
            observed_facts: vec![ObservedFact::from_source(
                "expires_at",
                expired_at.to_string(),
                "devices",
                app.now,
            )],
            observed_at: app.now,
            affected_resource_ids: vec![device_id],
            truncated_affected_resource_count: 0,
            source_ids: vec!["devices".to_owned()],
            explanation: "The authoritative device observation is expired.".to_owned(),
            suggested_action_ids: Vec::new(),
            derived: true,
        },
        Finding {
            id: "second-finding".to_owned(),
            rule_id: "user-approval-pending".to_owned(),
            severity: Severity::Warning,
            title: "User approval is pending".to_owned(),
            observed_facts: vec![ObservedFact::from_source(
                "approval",
                "second-evidence",
                "users",
                1_785_751_200,
            )],
            observed_at: 1_785_751_200,
            affected_resource_ids: vec!["user-second".to_owned()],
            truncated_affected_resource_count: 0,
            source_ids: vec!["users".to_owned()],
            explanation: "The authoritative user observation is pending approval.".to_owned(),
            suggested_action_ids: vec!["admin.user.approve".to_owned()],
            derived: true,
        },
    ];
    app.views.overview.selected_id = Some("second-finding".to_owned());
    app.set_route(Route::Overview);
    app.views.overview.selected_id = Some("second-finding".to_owned());

    let Some(wide) = render_lines(&app, 160, 45) else {
        return;
    };
    let wide = wide.join("\n");
    for wanted in [
        "local",
        "admin",
        "needs attention · 2",
        "1 critical",
        "1 warning",
        &device_name,
        "expired 1d ago",
        "Observed facts",
        "second-evidence",
    ] {
        assert!(wide.contains(wanted), "wide overview is missing {wanted}");
    }

    let Some(collection) = render_lines(&app, 80, 24) else {
        return;
    };
    let collection = collection.join("\n");
    assert!(collection.contains("Device key is expired"));
    assert!(collection.contains("User approval is pending"));
    assert!(!collection.contains("Observed facts"));

    app.focus = tale::app::Focus::Inspector;
    let Some(detail) = render_lines(&app, 80, 24) else {
        return;
    };
    let detail = detail.join("\n");
    assert!(detail.contains("Observed facts"));
    assert!(detail.contains("second-evidence"));
    assert!(!detail.contains("first-evidence"));

    app.focus = tale::app::Focus::Collection;
    press(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Char('h'));
    press(&mut app, KeyCode::Char('o'));
    assert_eq!(app.current_route(), Route::Users);

    app.set_route(Route::Overview);
    app.views.overview.selected_id = Some("first-finding".to_owned());
    press(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Char('h'));
    press(&mut app, KeyCode::Char('o'));
    assert_eq!(app.current_route(), Route::Devices);
    assert_eq!(
        app.selected_device()
            .map(|device| device.display_name.as_str()),
        Some(device_name.as_str())
    );
}

/// The old settings page mixed two unrelated things. How the managed tailnet is
/// configured is only knowable through a credential, so it hangs off the
/// profile that holds one; how this client is set up is its own page.
#[test]
fn the_managed_tailnet_is_read_from_the_profile_that_manages_it() {
    let Some(mut app) = admin_app("acme") else {
        return;
    };
    app.set_route(Route::Profiles);
    app.focus = tale::app::Focus::Inspector;
    app.views.profiles.selected = 1;
    app.admin.settings.snapshot = Some(tale::admin::AdminSettings {
        acls_externally_managed_on: Some(false),
        acls_external_link: None,
        devices_approval_on: Some(true),
        devices_auto_updates_on: Some(true),
        devices_key_duration_days: Some(180),
        users_approval_on: Some(true),
        network_flow_logging_on: Some(false),
        regional_routing_on: Some(false),
        posture_identity_collection_on: Some(false),
        https_enabled: Some(true),
    });
    app.admin.contacts.snapshot = Some(tale::admin::AdminContacts {
        // An address the control plane returned empty, which used to render as
        // a blank cell rather than saying nothing came back.
        account: Some(tale::admin::AdminContact {
            email: Some(String::new()),
            fallback_email: None,
            needs_verification: None,
        }),
        support: Some(tale::admin::AdminContact {
            email: Some("ops@example.test".to_owned()),
            fallback_email: None,
            needs_verification: Some(true),
        }),
        security: None,
    });
    let Some(lines) = render_lines(&app, 120, 40) else {
        return;
    };
    let rendered = lines.join("\n");
    for wanted in [
        "device approval   required",
        "key lifetime      180 days",
        "flow logging      off",
        "HTTPS certs       on",
        "account contact   not returned",
        "support contact   ops@example.test · needs verification",
        "security contact  not returned",
    ] {
        assert!(rendered.contains(wanted), "inspector is missing {wanted}");
    }

    // The client's own configuration is a page of its own, and every row lines
    // its source up with the rest however long the name is.
    app.set_route(Route::Config);
    let Some(lines) = render_lines(&app, 120, 40) else {
        return;
    };
    // Column positions are counted in characters: a truncated path ends in an
    // ellipsis, which is one column but three bytes.
    let sources = lines
        .iter()
        .filter_map(|line| {
            line.find(" default")
                .or_else(|| line.find(" cli"))
                .map(|index| line[..index].chars().count())
        })
        .collect::<Vec<_>>();
    assert!(sources.len() > 5);
    assert!(
        sources.windows(2).all(|pair| pair[0] == pair[1]),
        "the source column does not line up: {sources:?}"
    );
    let rendered = lines.join("\n");
    assert!(rendered.contains("config · read-only"));
    assert!(rendered.contains("ui.color.resolved"));
    // Nothing a tailnet owns belongs on this page.
    assert!(!rendered.contains("tailnet.https_enabled"));
}
