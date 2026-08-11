mod common;

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use tale::action::ActionId;
use tale::action::validate_transient_sequences;
use tale::app::{App, Focus, InteractionMode, Route};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::device::{
    ConnectionPath, Device, DeviceCapabilities, DeviceId, Liveness, OperatingSystem,
};
use tale::domain::service::{
    Backend, CapabilityState, Exposure, FunnelStatus, Listener, MetricsOutput, PathMount, Port,
    ProxyProtocol, ServeStatus, ServiceActionRequest, ServiceCapabilities, ServiceFailure,
    ServiceFailureKind, ServiceMapping, ServiceResourceStatus, ServiceSection,
};
use tale::domain::source::{ExecutableSource, LocalCapabilities, LocalExecutable};
use tale::domain::transfer::{TaildriveShare, TaildropTarget};
use tale::event::{Event, InputEvent, TaskEvent};
use tale::paths::{PathEnvironment, Platform};

const OBSERVED_AT: u64 = 1_754_000_000;

#[test]
fn services_render_all_sections_at_required_widths() {
    let app = populated_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        for section in ServiceSection::ALL {
            app.views.services.section = section;
            app.views.services.selected = 0;
            for (width, height) in [(60, 18), (80, 24), (110, 30), (160, 45)] {
                let lines = render_lines(&app, width, height);
                assert!(lines.is_some(), "render failed at {width}x{height}");
                if let Some(lines) = lines {
                    assert!(lines.iter().all(|line| !line.contains('\n')));
                    assert!(lines.iter().any(|line| line.contains(section.label())));
                }
            }
        }

        // Serve and Funnel are one table, keyed by an exposure column.
        app.views.services.section = ServiceSection::Serve;
        let mappings = render_lines(&app, 160, 45);
        assert!(mappings.is_some());
        if let Some(mappings) = mappings {
            assert!(mappings.iter().any(|line| line.contains("EXPOSURE")));
            assert!(mappings.iter().any(|line| line.contains("public")));
            assert!(mappings.iter().any(|line| line.contains("tailnet")));
            assert!(mappings.iter().any(|line| line.contains("1 public")));
        }

        app.views.services.section = ServiceSection::Taildrive;
        let drive = render_lines(&app, 160, 45);
        assert!(drive.is_some());
        if let Some(drive) = drive {
            assert!(drive.iter().any(|line| line.contains("docs")));
        }

        // Metrics and the bug report are diagnostics, on their own route.
        app.set_route(Route::Diagnostics);
        let diagnostics = render_lines(&app, 160, 45);
        assert!(diagnostics.is_some());
        if let Some(diagnostics) = diagnostics {
            assert!(
                diagnostics
                    .iter()
                    .any(|line| line.contains("tale_requests"))
            );
            assert!(diagnostics.iter().any(|line| line.contains("cut off")));
            assert!(diagnostics.iter().any(|line| line.contains("BUG-")));
            assert!(
                diagnostics
                    .iter()
                    .any(|line| line.contains("Nothing was uploaded"))
            );
        }
    }
}

#[test]
fn services_inspector_is_opt_in_and_empty_sections_do_not_open_it() {
    let Some(mut app) = populated_app() else {
        return;
    };
    app.set_route(Route::Services);
    assert!(!app.inspector_pane_visible());
    let _ = app.dispatch_action(ActionId::CollectionInspect);
    assert!(app.inspector_pane_visible());
    let _ = app.dispatch_action(ActionId::CollectionInspect);
    assert!(!app.inspector_pane_visible());

    app.views.services.section = ServiceSection::Taildrive;
    app.alpha_local_features = false;
    let _ = app.dispatch_action(ActionId::CollectionOpen);
    assert_eq!(app.focus, Focus::Collection);
}

#[test]
fn services_render_loading_partial_stale_failed_unsupported_read_only_and_running_states() {
    let app = local_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.set_route(Route::Services);
        app.views.services.section = ServiceSection::Serve;
        // Nothing has been asked of the client yet, and the view says so
        // rather than claiming a request is in flight.
        let idle = render_lines(&app, 80, 24);
        assert!(idle.is_some());
        if let Some(idle) = idle {
            assert!(idle.iter().any(|line| line.contains("No mappings loaded")));
        }

        let Some(port_3000) = port(3000) else {
            return;
        };
        let Some(mapping) = mapping(443, "/", Backend::Port(port_3000)) else {
            return;
        };
        app.services_snapshot.serve.succeed(
            1,
            OBSERVED_AT,
            ServeStatus {
                mappings: vec![mapping.clone()],
            },
        );
        let mut public_mapping = mapping.clone();
        public_mapping.exposure = Exposure::Public;
        app.services_snapshot.funnel.succeed(
            1,
            OBSERVED_AT,
            FunnelStatus {
                mappings: vec![public_mapping],
            },
        );
        app.services_snapshot.funnel.fail(
            1,
            ServiceFailure::new(
                ServiceFailureKind::CommandFailed,
                "funnel status",
                "funnel status failed",
                "fictional funnel failure",
            ),
        );
        // One table, so a Funnel failure surfaces beside the Serve rows that
        // did load rather than hiding behind a tab.
        app.views.services.section = ServiceSection::Serve;
        let partial = render_lines(&app, 80, 24);
        assert!(partial.is_some());
        if let Some(partial) = partial {
            assert!(partial.iter().any(|line| line.contains("fictional funnel")));
            assert!(partial.iter().any(|line| line.contains("EXPOSURE")));
        }

        app.alpha_local_features = false;
        app.services_snapshot.taildrive.status = ServiceResourceStatus::Unsupported;
        app.services_snapshot.taildrive.failure = None;
        app.views.services.section = ServiceSection::Taildrive;
        let unsupported = render_lines(&app, 80, 24);
        assert!(unsupported.is_some());
        if let Some(unsupported) = unsupported {
            assert!(unsupported.iter().any(|line| line.contains("alpha")));
            assert!(
                unsupported
                    .iter()
                    .any(|line| line.contains("off for this run"))
            );
        }

        app.resolved_config.read_only = true;
        app.views.services.section = ServiceSection::Serve;
        let _ = app.dispatch_action(ActionId::ResourceActions);
        let read_only = render_lines(&app, 110, 30);
        assert!(read_only.is_some());
        if let Some(read_only) = read_only {
            assert!(
                read_only
                    .iter()
                    .any(|line| line.contains("read-only") || line.contains("Read-only"))
            );
        }

        app.resolved_config.read_only = false;
        let task_id = app.tasks.create(
            ActionId::ServicesMetricsRefresh,
            "local metrics",
            OBSERVED_AT,
            true,
        );
        let _ = app.update(Event::Task(Box::new(TaskEvent::Started { task_id })));
        let running = render_lines(&app, 80, 24);
        assert!(running.is_some());
        if let Some(running) = running {
            assert!(running.iter().any(|line| line.contains("1 task running")));
        }
    }
}

fn populated_app() -> Option<App> {
    let mut app = local_app()?;
    app.alpha_local_features = true;
    app.services_snapshot.capabilities = available_capabilities();
    let serve_mapping = mapping(
        443,
        "/",
        Backend::HttpUrl("http://127.0.0.1:3000".to_owned()),
    )?;
    let funnel_mapping = ServiceMapping {
        exposure: Exposure::Public,
        listener: Listener::Https(port(8443)?),
        mount: PathMount::Root,
        backend: Backend::Port(port(3001)?),
        proxy_protocol: ProxyProtocol::None,
        hostname: Some("public.example.ts.net".to_owned()),
    };
    app.services_snapshot.serve.succeed(
        1,
        OBSERVED_AT,
        ServeStatus {
            mappings: vec![serve_mapping],
        },
    );
    app.services_snapshot.funnel.succeed(
        1,
        OBSERVED_AT,
        FunnelStatus {
            mappings: vec![funnel_mapping],
        },
    );
    app.services_snapshot.taildrop_targets.succeed(
        1,
        OBSERVED_AT,
        vec![TaildropTarget {
            command_target: "100.64.0.2".to_owned(),
            display_name: "Office Laptop".to_owned(),
            device_name: Some("office-laptop".to_owned()),
            online: Some(true),
            capability_reason: None,
        }],
    );
    // Taildrop acts on the selected device, so the devices route needs the row
    // that target describes.
    app.devices_resource.snapshot = vec![office_laptop()];
    app.views.devices.selected_id = Some(DeviceId::new("office-laptop"));
    app.services_snapshot.taildrive.succeed(
        1,
        OBSERVED_AT,
        vec![TaildriveShare {
            name: "docs".to_owned(),
            path: "/srv/tale/docs".into(),
            as_user: Some("alice".to_owned()),
        }],
    );
    app.services_snapshot.certificate_domains.succeed(
        1,
        OBSERVED_AT,
        vec!["node.example.ts.net".to_owned()],
    );
    app.services_snapshot.metrics.succeed(
        1,
        OBSERVED_AT,
        MetricsOutput {
            text: "tale_requests 1\ntale_errors 0\ntale_transfers 2".to_owned(),
            captured_at: OBSERVED_AT,
            truncated: true,
        },
    );
    app.services_snapshot.bug_report.succeed(
        1,
        OBSERVED_AT,
        tale::domain::service::BugReportResult {
            identifier: "BUG-1234".to_owned(),
            observed_at: OBSERVED_AT,
        },
    );
    app.set_route(Route::Services);
    Some(app)
}

fn local_app() -> Option<App> {
    let root = PathBuf::from("/fictional/tale-services");
    let cli = Cli {
        command: None,
        profile: None,
        config: Some(root.join("missing.toml")),
        view: None,
        read_only: false,
        no_local: false,
        tailscale_path: Some("/fictional/tailscale".into()),
        tailscale_socket: None,
        mock: false,
    };
    let environment = EnvironmentValues {
        config_file: None,
        tailscale_path: None,
        tailscale_socket: None,
        no_color: true,
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
    let mut app = config::resolve(&cli, &environment, &paths)
        .ok()
        .map(App::new)?;
    let capabilities = LocalCapabilities::all_supported();
    app.local_capabilities = capabilities;
    app.local_executable = Some(LocalExecutable {
        path: "/fictional/tailscale".into(),
        socket_path: None,
        source: ExecutableSource::Cli,
        version: "1.98.9".to_owned(),
        daemon_version: Some("1.98.9".to_owned()),
        build: None,
        capabilities,
    });
    Some(app)
}

/// Taildrop's rows were the tailnet's devices listed a second time, so it moved
/// to `:devices`, where the selected row is the target. The services route no
/// longer offers it, and the menu that does has to open.
#[test]
fn taildrop_is_offered_on_the_devices_route_and_not_on_services() {
    let Some(mut app) = populated_app() else {
        return;
    };
    assert!(
        !app.contextual_actions()
            .contains(&ActionId::DevicesTaildropSend),
        "the services route still offers Taildrop"
    );

    app.set_route(Route::Devices);
    let actions = app.contextual_actions();
    assert!(actions.contains(&ActionId::DevicesTaildropSend));
    assert!(actions.contains(&ActionId::DevicesTaildropReceive));
    // A one-key leaf that also prefixes a two-key sequence makes the whole
    // menu refuse to open rather than shadowing one entry.
    assert!(validate_transient_sequences(&actions).is_ok());
    let _ = app.dispatch_action(ActionId::ResourceActions);
    assert!(matches!(app.interaction, InteractionMode::Transient(_)));
    assert!(app.runtime_error.is_none());

    // The same menu with a profile for the tailnet this machine is on, where the
    // admin device actions crowd in alongside the local ones. A profile for some
    // other tailnet is a different menu entirely — see `tests/device_source.rs`.
    app.interaction = InteractionMode::Normal;
    app.admin.profile = Some("fictional".to_owned());
    common::install_aligned_sources(&mut app, "fixture.ts.net", &["office-laptop"]);
    app.views.devices.selected_id = Some(DeviceId::new("office-laptop"));
    let with_admin = app.contextual_actions();
    assert!(with_admin.contains(&ActionId::DevicesTaildropSend));
    assert!(validate_transient_sequences(&with_admin).is_ok());
}

/// The form asks only for the files: the row it was opened on is the target,
/// and it is named above the fields rather than typed into one.
#[test]
fn the_send_form_takes_its_target_from_the_selected_device() {
    let Some(mut app) = populated_app() else {
        return;
    };
    app.set_route(Route::Devices);
    let _ = app.dispatch_action(ActionId::DevicesTaildropSend);
    assert_eq!(app.overlays.len(), 1);
    let lines = render_lines(&app, 120, 40);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        assert!(lines.iter().any(|line| line.contains("Office Laptop")));
        assert!(lines.iter().any(|line| line.contains("Files")));
        assert!(
            !lines.iter().any(|line| line.contains("target")),
            "the form asks for a target it already knows"
        );
    }

    // A device the client did not offer as a target says so instead of opening
    // a form that could only fail.
    app.overlays.clear();
    app.views.devices.selected_id = Some(DeviceId::new("unknown-device"));
    app.devices_resource.snapshot.push(Device {
        id: DeviceId::new("unknown-device"),
        display_name: "unknown-device".to_owned(),
        hostname: "unknown-device".to_owned(),
        addresses: vec!["100.64.0.9".to_owned()],
        ..office_laptop()
    });
    let _ = app.dispatch_action(ActionId::DevicesTaildropSend);
    assert!(app.overlays.is_empty());
    assert!(
        app.runtime_error
            .as_deref()
            .is_some_and(|error| error.contains("unknown-device"))
    );
}

#[test]
fn the_send_review_expands_home_paths_and_shows_the_resolved_file() {
    let Some(mut app) = populated_app() else {
        return;
    };
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let Ok(current_dir) = std::env::current_dir() else {
        return;
    };
    let Ok(relative_dir) = current_dir.strip_prefix(&home) else {
        return;
    };
    let file = current_dir.join("Cargo.toml");
    if !file.is_file() {
        return;
    }
    let shorthand = PathBuf::from("~").join(relative_dir).join("Cargo.toml");

    app.set_route(Route::Devices);
    let _ = app.dispatch_action(ActionId::DevicesTaildropSend);
    if let Some(tale::app::Overlay::Form(state)) = app.overlays.last_mut() {
        if let Some(field) = state.fields.iter_mut().find(|field| field.key == "files") {
            field.value = shorthand.display().to_string();
        }
        state.selected = state.fields.len();
    }
    press(&mut app, KeyCode::Enter);

    let lines = render_lines(&app, 140, 40);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        assert!(lines.iter().any(|line| line.contains("Resolved files:")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains(&file.display().to_string()))
        );
    }
}

#[test]
fn every_tui_local_path_expands_home_before_review() {
    let Some(mut app) = populated_app() else {
        return;
    };
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let Ok(current_dir) = std::env::current_dir() else {
        return;
    };
    let Ok(relative_dir) = current_dir.strip_prefix(&home) else {
        return;
    };
    let shorthand_dir = PathBuf::from("~").join(relative_dir);
    let shorthand = |name: &str| shorthand_dir.join(name).display().to_string();

    app.set_route(Route::Devices);
    let _ = app.dispatch_action(ActionId::DevicesTaildropReceive);
    submit_form(
        &mut app,
        &[("directory", &shorthand_dir.display().to_string())],
    );
    assert!(matches!(
        confirmed_service_request(&app),
        Some(ServiceActionRequest::TaildropReceive(request))
            if request.directory == current_dir
    ));
    app.overlays.clear();

    app.set_route(Route::Services);
    app.views.services.section = ServiceSection::Taildrive;
    let _ = app.dispatch_action(ActionId::ServicesDriveShare);
    submit_form(
        &mut app,
        &[
            ("name", "home_docs"),
            ("path", &shorthand_dir.display().to_string()),
        ],
    );
    assert!(matches!(
        confirmed_service_request(&app),
        Some(ServiceActionRequest::TaildriveShare { path, .. }) if path == &current_dir
    ));
    app.overlays.clear();

    app.views.services.section = ServiceSection::Certificates;
    let _ = app.dispatch_action(ActionId::ServicesCertificateObtain);
    submit_form(
        &mut app,
        &[
            ("cert", &shorthand("tale-home-test.crt")),
            ("key", &shorthand("tale-home-test.key")),
        ],
    );
    assert!(
        matches!(
            confirmed_service_request(&app),
            Some(ServiceActionRequest::Certificate(request))
                if request.certificate_path == current_dir.join("tale-home-test.crt")
                    && request.key_path == current_dir.join("tale-home-test.key")
        ),
        "certificate form did not confirm: {:?}",
        app.overlays.last()
    );
    app.overlays.clear();

    app.views.services.section = ServiceSection::Serve;
    let _ = app.dispatch_action(ActionId::ServicesServeCreate);
    submit_form(
        &mut app,
        &[
            ("port", "444"),
            ("backend", &shorthand_dir.display().to_string()),
        ],
    );
    assert!(matches!(
        confirmed_service_request(&app),
        Some(ServiceActionRequest::Serve { mapping, .. })
            if mapping.backend == Backend::FileSystemPath(current_dir.clone())
    ));
    app.overlays.clear();

    let _ = app.dispatch_action(ActionId::ServicesServeCreate);
    submit_form(
        &mut app,
        &[
            ("port", "445"),
            ("backend", &format!("unix:{}", shorthand("tale.sock"))),
        ],
    );
    assert!(matches!(
        confirmed_service_request(&app),
        Some(ServiceActionRequest::Serve { mapping, .. })
            if mapping.backend == Backend::UnixSocket(current_dir.join("tale.sock"))
    ));
    app.overlays.clear();

    app.set_route(Route::Devices);
    let _ = app.dispatch_action(ActionId::CollectionExport);
    submit_form(&mut app, &[("path", &shorthand("tale-home-test.json"))]);
    assert!(matches!(
        app.overlays.last(),
        Some(tale::app::Overlay::Confirmation(state))
            if matches!(
                state.operational_mutation.as_ref(),
                Some(tale::domain::operational::OperationalMutation::Export(request))
                    if request.path == current_dir.join("tale-home-test.json")
            )
    ));
}

fn submit_form(app: &mut App, values: &[(&str, &str)]) {
    if let Some(tale::app::Overlay::Form(state)) = app.overlays.last_mut() {
        for (key, value) in values {
            if let Some(field) = state.fields.iter_mut().find(|field| field.key == *key) {
                field.value = (*value).to_owned();
            }
        }
        state.selected = state.fields.len();
    }
    press(app, KeyCode::Enter);
}

fn confirmed_service_request(app: &App) -> Option<&ServiceActionRequest> {
    match app.overlays.last() {
        Some(tale::app::Overlay::Confirmation(state)) => state.service_request.as_ref(),
        _ => None,
    }
}

fn office_laptop() -> Device {
    Device {
        id: DeviceId::new("office-laptop"),
        display_name: "office-laptop".to_owned(),
        hostname: "office-laptop".to_owned(),
        owner: None,
        owner_label: None,
        os: OperatingSystem::Linux,
        version: Some("1.98.9".to_owned()),
        liveness: Liveness::Online,
        path: ConnectionPath::Direct { latency_ms: None },
        addresses: vec!["100.64.0.2".to_owned()],
        advertised_routes: Vec::new(),
        tags: Vec::new(),
        last_seen: Some(OBSERVED_AT),
        created_at: None,
        rx_bytes: None,
        tx_bytes: None,
        capabilities: DeviceCapabilities {
            exit_node: false,
            exit_node_option: false,
            subnet_router: false,
            ssh: false,
            funnel: false,
            shared: false,
            expired: false,
            approved: true,
        },
    }
}

fn available_capabilities() -> ServiceCapabilities {
    ServiceCapabilities {
        serve: CapabilityState::available(),
        funnel: CapabilityState::available(),
        taildrop: CapabilityState::available(),
        taildrive: CapabilityState::available(),
        certificates: CapabilityState::available(),
        metrics: CapabilityState::available(),
        bug_report: CapabilityState::available(),
    }
}

fn mapping(port_number: u16, path: &str, backend: Backend) -> Option<ServiceMapping> {
    Some(ServiceMapping {
        exposure: Exposure::Tailnet,
        listener: Listener::Https(port(port_number)?),
        mount: PathMount::parse(path).ok()?,
        backend,
        proxy_protocol: ProxyProtocol::None,
        hostname: Some("node.example.ts.net".to_owned()),
    })
}

fn port(value: u16) -> Option<Port> {
    Port::new(value).ok()
}

/// `/`, `s` and `y` were dead or wrong on this route because there was no
/// collection for them to act on. One mapping table gives all three a target.
#[test]
fn the_mapping_table_answers_filter_sort_and_copy() {
    let Some(mut app) = populated_app() else {
        return;
    };
    app.views.services.section = ServiceSection::Serve;

    assert!(
        !app.filter_schema().is_empty(),
        "serve has no filter schema"
    );
    for name in ["exposure", "listener", "port", "path", "backend"] {
        assert!(app.filter_schema().field(name).is_some(), "missing {name}");
    }

    // Filtering narrows the table, and the border says so.
    let _ = app.dispatch_action(ActionId::ViewFilter);
    for character in "exposure:public".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.visible_service_mappings().len(), 1);
    assert_eq!(app.service_mapping_total(), 2);
    let filtered = render_lines(&app, 160, 45);
    assert!(filtered.is_some());
    if let Some(filtered) = filtered {
        assert!(filtered.iter().any(|line| line.contains("1 of 2")));
        assert!(!filtered.iter().any(|line| line.contains("  tailnet ")));
    }

    // Sorting offers the table's own columns, not the device fields.
    let _ = app.dispatch_action(ActionId::CollectionSort);
    let sort = render_lines(&app, 160, 45);
    assert!(sort.is_some());
    if let Some(sort) = sort {
        assert!(sort.iter().any(|line| line.contains("exposure")));
        assert!(sort.iter().any(|line| line.contains("backend")));
        assert!(
            !sort.iter().any(|line| line.contains("last seen")),
            "device sort fields leaked onto services"
        );
    }
    press(&mut app, KeyCode::Esc);

    // Copying offers the selected mapping, starting with a pasteable URL.
    let fields = app.contextual_copy_fields();
    assert!(fields.contains(&tale::app::CopyField::ServiceUrl));
    let _ = app.dispatch_action(ActionId::ResourceCopy);
    let copy = render_lines(&app, 160, 45);
    assert!(copy.is_some());
    if let Some(copy) = copy {
        assert!(copy.iter().any(|line| line.contains("u url")));
    }
}

/// The route's own key was sorted past `? more`, and the hint printed inside
/// the box named keys that did something else entirely.
#[test]
fn the_footer_offers_the_keys_this_route_actually_has() {
    let Some(mut app) = populated_app() else {
        return;
    };
    app.views.services.section = ServiceSection::Serve;
    let services = render_lines(&app, 160, 45);
    assert!(services.is_some());
    if let Some(services) = services {
        let footer = services.last().cloned().unwrap_or_default();
        assert!(footer.contains("Tab next tab"), "no tab key: {footer}");
        assert!(!footer.contains("columns"), "columns has no meaning here");
        assert!(
            !services.iter().any(|line| line.contains("[/] section")),
            "the box still names keys that do something else"
        );
    }

    app.set_route(Route::Devices);
    let devices = render_lines(&app, 160, 45);
    assert!(devices.is_some());
    if let Some(devices) = devices {
        let footer = devices.last().cloned().unwrap_or_default();
        assert!(!footer.contains("tab"), "the tab key leaked onto devices");
        assert!(footer.contains("columns"));
    }
}

/// Tab and Shift-Tab move between the four services and wrap, and the strip
/// says which one is showing.
#[test]
fn tab_moves_between_the_service_tabs_and_wraps() {
    let Some(mut app) = populated_app() else {
        return;
    };
    let lines = render_lines(&app, 160, 45);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        let strip = lines
            .iter()
            .find(|line| line.contains(ServiceSection::Certificates.label()))
            .cloned()
            .unwrap_or_default();
        for section in ServiceSection::ALL {
            assert!(strip.contains(section.label()), "{section:?} has no tab");
        }
    }

    for expected in [
        ServiceSection::Taildrive,
        ServiceSection::Certificates,
        ServiceSection::Serve,
    ] {
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.views.services.section, expected);
    }

    press(&mut app, KeyCode::BackTab);
    assert_eq!(app.views.services.section, ServiceSection::Certificates);
}

fn press(app: &mut App, code: KeyCode) {
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        code,
        KeyModifiers::NONE,
    ))));
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

/// Every service form used to be one line of `key=value;key=value` with the
/// grammar spelled out above it. Each field now names itself, and anything the
/// selection already knows is stated rather than asked for.
#[test]
fn service_forms_ask_field_by_field_and_never_show_their_serialization() {
    let Some(mut app) = populated_app() else {
        return;
    };
    for (route, section, action, expected) in [
        (
            Route::Services,
            ServiceSection::Serve,
            ActionId::ServicesServeCreate,
            vec![
                "New tailnet mapping",
                "this tailnet only",
                "Protocol",
                "Port",
            ],
        ),
        (
            Route::Services,
            ServiceSection::Serve,
            ActionId::ServicesServeEdit,
            vec!["Edit mapping", "reachable by", "listener", "Serve"],
        ),
        (
            Route::Devices,
            ServiceSection::Serve,
            ActionId::DevicesTaildropSend,
            vec!["Send files", "Office Laptop", "Files"],
        ),
        (
            Route::Devices,
            ServiceSection::Serve,
            ActionId::DevicesTaildropReceive,
            vec![
                "Receive files",
                "Save to",
                "If a name is taken",
                "Keep waiting",
            ],
        ),
        (
            Route::Services,
            ServiceSection::Certificates,
            ActionId::ServicesCertificateObtain,
            vec![
                "Get a certificate",
                "node.example.ts.net",
                "Certificate file",
            ],
        ),
    ] {
        app.overlays.clear();
        app.set_route(route);
        app.views.services.section = section;
        app.views.services.selected = 0;
        let _ = app.dispatch_action(action);
        let lines = render_lines(&app, 120, 40);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            for wanted in expected {
                assert!(
                    lines.iter().any(|line| line.contains(wanted)),
                    "{action:?} is missing {wanted}"
                );
            }
            assert!(
                !lines.iter().any(|line| line.contains(";files=")
                    || line.contains("listener=https")
                    || line.contains("local service form")),
                "{action:?} still shows its serialization"
            );
        }
    }
}

/// Listener support belongs to the Serve command itself. Human-readable help
/// output is not a capability contract, so a standard HTTPS mapping must reach
/// confirmation whenever Serve is available.
#[test]
fn https_mapping_is_not_gated_by_listener_help_text() {
    let Some(mut app) = populated_app() else {
        return;
    };
    let _ = app.dispatch_action(ActionId::ServicesServeCreate);

    for _ in 0..3 {
        press(&mut app, KeyCode::Char('j'));
    }
    press(&mut app, KeyCode::Enter);
    for character in "4321".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Enter);

    assert!(matches!(
        app.overlays.last(),
        Some(tale::app::Overlay::Confirmation(_))
    ));
}

/// Editing acts on whichever row is selected, so it must not fail merely
/// because the top of the table happens to be a public mapping.
#[test]
fn editing_follows_the_selected_row_rather_than_a_fixed_exposure() {
    let Some(mut app) = populated_app() else {
        return;
    };
    app.views.services.section = ServiceSection::Serve;
    app.views.services.selected = 0;
    let selected = app.selected_service_mapping();
    assert!(selected.is_some_and(|mapping| mapping.exposure == Exposure::Public));

    app.overlays.clear();
    let _ = app.dispatch_action(ActionId::ServicesServeEdit);
    assert_eq!(app.overlays.len(), 1, "edit refused a public row");
    let lines = render_lines(&app, 120, 40);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        assert!(
            lines
                .iter()
                .any(|line| line.contains("anyone on the internet"))
        );
    }
}

/// The confirmation used to print `argv[0] = "file"` above the same command it
/// then showed again. It now states the effect in words, takes its risk from
/// the request rather than the action, and warns only when there is a warning.
#[test]
fn the_confirmation_explains_the_change_rather_than_dumping_its_arguments() {
    let Some(mut app) = populated_app() else {
        return;
    };
    app.set_route(Route::Devices);
    let _ = app.dispatch_action(ActionId::DevicesTaildropReceive);
    // Enter opens the field, Enter keeps it, j moves on.
    press(&mut app, KeyCode::Enter);
    for character in "/tmp".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Enter);
    // rename -> skip -> overwrite
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Enter);

    let lines = render_lines(&app, 110, 40);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        assert!(lines.iter().any(|line| line.contains("What will happen")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Save incoming files into /tmp"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("cannot be recovered")),
            "an overwrite gave no warning"
        );
        // Overwriting is disruptive even though receiving files is not.
        assert!(lines.iter().any(|line| line.contains("Disruptive")));
        assert!(
            !lines.iter().any(|line| line.contains("argv[")),
            "the confirmation still dumps its arguments"
        );
    }
}

/// Forms have two modes and one rule: Enter acts on what is selected. Browsing,
/// j and k move and Enter opens a field or submits; editing, Enter keeps the
/// value and Esc puts back what was there before.
#[test]
fn forms_browse_with_jk_and_edit_one_field_at_a_time() {
    let Some(mut app) = populated_app() else {
        return;
    };
    app.set_route(Route::Devices);
    let _ = app.dispatch_action(ActionId::DevicesTaildropReceive);

    // Browsing: typing does not reach the field.
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('k'));
    assert!(form_value(&app, "directory").is_some_and(str::is_empty));

    // Enter opens the field, Enter keeps what was typed.
    press(&mut app, KeyCode::Enter);
    for character in "/tmp".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Enter);
    assert_eq!(form_value(&app, "directory"), Some("/tmp"));

    // Esc puts back the value the field had before editing began.
    press(&mut app, KeyCode::Enter);
    for character in "/nope".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Esc);
    assert_eq!(form_value(&app, "directory"), Some("/tmp"));
    assert_eq!(app.overlays.len(), 1, "Esc left the field and the form");

    // A choice only changes while it is open.
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Right);
    assert_eq!(form_value(&app, "conflict"), Some("rename"));
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Enter);
    assert_eq!(form_value(&app, "conflict"), Some("skip"));

    // Past the last field is the submit row.
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    let lines = render_lines(&app, 100, 34);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        assert!(lines.iter().any(|line| line.contains("> Continue")));
    }
    press(&mut app, KeyCode::Enter);
    assert!(
        render_lines(&app, 100, 34)
            .is_some_and(|lines| lines.iter().any(|line| line.contains("Confirm")))
    );
}

fn form_value<'a>(app: &'a App, key: &str) -> Option<&'a str> {
    app.overlays.iter().find_map(|overlay| match overlay {
        tale::app::Overlay::Form(state) => Some(state.value(key)),
        _ => None,
    })
}

/// Every list is the same table, so a row reads the same way whichever service
/// it belongs to.
#[test]
fn every_service_list_is_a_table_with_column_headings() {
    let Some(mut app) = populated_app() else {
        return;
    };
    for (section, headings) in [
        (
            ServiceSection::Serve,
            vec!["EXPOSURE", "LISTENER", "BACKEND"],
        ),
        (ServiceSection::Taildrive, vec!["NAME", "FOLDER"]),
        (ServiceSection::Certificates, vec!["DOMAIN"]),
    ] {
        app.views.services.section = section;
        app.views.services.selected = 0;
        let lines = render_lines(&app, 140, 40);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            for heading in headings {
                assert!(
                    lines.iter().any(|line| line.contains(heading)),
                    "{section:?} has no {heading} column"
                );
            }
        }
    }
}

/// The only way out of a public mapping used to be removing every public
/// mapping. A row can now be unpublished, which keeps it serving the tailnet,
/// or removed outright, which leaves its neighbours alone.
#[test]
fn one_mapping_can_stop_being_public_or_be_removed_on_its_own() {
    let Some(mut app) = populated_app() else {
        return;
    };
    app.views.services.section = ServiceSection::Serve;
    app.views.services.selected = 0;
    assert!(
        app.selected_service_mapping()
            .is_some_and(|mapping| mapping.exposure == Exposure::Public),
        "the fixture no longer selects a public row"
    );

    let actions = app.contextual_actions();
    assert!(actions.contains(&ActionId::ServicesServeRemove));
    assert!(actions.contains(&ActionId::ServicesFunnelUnpublish));
    // `d` and `u` are leaves next to the `x` reset prefix; a collision would
    // make the whole mappings menu refuse to open.
    assert!(validate_transient_sequences(&actions).is_ok());

    app.overlays.clear();
    let _ = app.dispatch_action(ActionId::ServicesFunnelUnpublish);
    assert_eq!(app.overlays.len(), 1, "unpublish refused the public row");
    let lines = render_lines(&app, 120, 44);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        assert!(
            lines
                .iter()
                .any(|line| line.contains("tailnet only") || line.contains("tailnet-only")),
            "unpublish never says the mapping survives"
        );
        // Funnel is held per listener, so the blast radius is the whole port.
        assert!(
            lines.iter().any(|line| line.contains("set per listener")),
            "unpublish hides that the whole listener stops being public"
        );
        assert!(lines.iter().any(|line| line.contains("UNPUBLISH")));
        // Nothing here removes anything, so nothing here mentions a reset.
        assert!(!lines.iter().any(|line| line.contains("RESET")));
    }

    app.overlays.clear();
    let _ = app.dispatch_action(ActionId::ServicesServeRemove);
    assert_eq!(app.overlays.len(), 1, "remove refused the public row");
    let lines = render_lines(&app, 120, 44);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        assert!(lines.iter().any(|line| line.contains("REMOVE-PUBLIC")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("https:8443") || line.contains("8443")),
            "removal never names the row it acts on"
        );
        assert!(
            lines.iter().any(|line| line.contains("--set-path=/")),
            "the preview omits the mount that scopes the removal"
        );
        assert!(
            lines.iter().any(|line| line.contains("off")),
            "the preview does not show the off target"
        );
    }

    // A tailnet row is removable too, and asks for a quieter phrase.
    app.overlays.clear();
    app.views.services.selected = 1;
    assert!(
        app.selected_service_mapping()
            .is_some_and(|mapping| mapping.exposure == Exposure::Tailnet)
    );
    let _ = app.dispatch_action(ActionId::ServicesServeRemove);
    assert_eq!(app.overlays.len(), 1);
    let lines = render_lines(&app, 120, 44);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        assert!(lines.iter().any(|line| line.contains("REMOVE")));
        assert!(!lines.iter().any(|line| line.contains("REMOVE-PUBLIC")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("left alone") || line.contains("Other mappings"))
        );
    }

    // There is nothing to unpublish on a tailnet row, and saying so beats
    // sending a command that would silently do nothing.
    app.overlays.clear();
    let _ = app.dispatch_action(ActionId::ServicesFunnelUnpublish);
    assert!(
        app.overlays.is_empty(),
        "unpublish opened a confirmation for a tailnet row"
    );
    assert!(
        app.runtime_error
            .as_deref()
            .is_some_and(|error| error.contains("already tailnet-only"))
    );
}

/// A node that has lost Funnel can no longer create public mappings, but it
/// must still be able to take the ones it has down. Both exits run
/// `tailscale serve`, so both stay available.
#[test]
fn taking_a_public_mapping_down_does_not_need_the_funnel_capability() {
    let Some(mut app) = populated_app() else {
        return;
    };
    app.views.services.section = ServiceSection::Serve;
    app.views.services.selected = 0;
    let mut capabilities = available_capabilities();
    capabilities.funnel = CapabilityState::unsupported("this CLI no longer advertises Funnel");
    app.services_snapshot.capabilities = capabilities;
    app.local_capabilities.funnel = false;

    for action in [
        ActionId::ServicesServeRemove,
        ActionId::ServicesFunnelUnpublish,
    ] {
        app.overlays.clear();
        app.runtime_error = None;
        let _ = app.dispatch_action(action);
        assert_eq!(app.overlays.len(), 1, "{action:?} was gated on Funnel");
    }

    app.overlays.clear();
    app.runtime_error = None;
    let _ = app.dispatch_action(ActionId::ServicesFunnelReset);
    assert!(
        app.overlays.is_empty(),
        "resetting Funnel survived losing the Funnel capability"
    );
}

/// A terminal reports a capital as Shift plus the character, so a text input
/// that insists on no modifiers at all silently drops every uppercase key --
/// which made the phrase a Tier 2 confirmation demands impossible to type.
#[test]
fn typing_a_capital_letter_reaches_every_text_input() {
    fn type_text(app: &mut App, text: &str) {
        for character in text.chars() {
            let modifiers = if character.is_uppercase() {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            };
            let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
                KeyCode::Char(character),
                modifiers,
            ))));
        }
    }

    let Some(mut app) = populated_app() else {
        return;
    };
    app.views.services.section = ServiceSection::Serve;
    app.views.services.selected = 0;
    let _ = app.dispatch_action(ActionId::ServicesFunnelUnpublish);
    type_text(&mut app, "UNPUBLISH");
    let typed = app.overlays.last().and_then(|overlay| match overlay {
        tale::app::Overlay::Confirmation(state) => {
            Some((state.input.clone(), state.required_phrase.clone()))
        }
        _ => None,
    });
    assert!(typed.is_some(), "the unpublish confirmation is gone");
    if let Some((input, phrase)) = typed {
        assert_eq!(input, "UNPUBLISH");
        assert_eq!(phrase.as_deref(), Some("UNPUBLISH"));
    }

    // The same key path feeds the typed forms, where a capital is ordinary
    // rather than ceremonial: a path or a share name may contain one.
    app.overlays.clear();
    app.set_route(Route::Services);
    app.views.services.section = ServiceSection::Taildrive;
    app.views.services.selected = 0;
    let _ = app.dispatch_action(ActionId::ServicesDriveShare);
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "Quarterly Reports");
    assert_eq!(form_value(&app, "name"), Some("Quarterly Reports"));

    // Control still means a command rather than a letter.
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    ))));
    assert_eq!(
        form_value(&app, "name"),
        Some("Quarterly Reports"),
        "a control chord was typed into the field"
    );
}
