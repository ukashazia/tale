use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use tale::action::ActionId;
use tale::app::{App, Route};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::service::{
    Backend, CapabilityState, Exposure, FunnelStatus, Listener, MetricsOutput, PathMount, Port,
    ProxyProtocol, ServeStatus, ServiceCapabilities, ServiceFailure, ServiceFailureKind,
    ServiceMapping, ServiceResourceStatus, ServiceSection,
};
use tale::domain::source::{ExecutableSource, LocalCapabilities, LocalExecutable};
use tale::domain::transfer::{TaildriveShare, TaildropTarget};
use tale::event::{Event, TaskEvent};
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
                    assert!(lines.iter().any(|line| line.contains("services")));
                }
            }
        }

        app.views.services.section = ServiceSection::Funnel;
        let funnel = render_lines(&app, 160, 45);
        assert!(funnel.is_some());
        if let Some(funnel) = funnel {
            assert!(funnel.iter().any(|line| line.contains("PUBLIC")));
        }

        app.views.services.section = ServiceSection::Taildrive;
        let drive = render_lines(&app, 160, 45);
        assert!(drive.is_some());
        if let Some(drive) = drive {
            assert!(drive.iter().any(|line| line.contains("ALPHA")));
        }

        app.views.services.section = ServiceSection::Metrics;
        let metrics = render_lines(&app, 160, 45);
        assert!(metrics.is_some());
        if let Some(metrics) = metrics {
            assert!(metrics.iter().any(|line| line.contains("NOTICE")));
            assert!(metrics.iter().any(|line| line.contains("tale_requests")));
        }

        app.views.services.section = ServiceSection::BugReport;
        let bug_report = render_lines(&app, 160, 45);
        assert!(bug_report.is_some());
        if let Some(bug_report) = bug_report {
            assert!(bug_report.iter().any(|line| line.contains("BUG-")));
            assert!(
                bug_report
                    .iter()
                    .any(|line| line.contains("Not copied, uploaded, or shared"))
            );
        }
    }
}

#[test]
fn services_render_loading_partial_stale_failed_unsupported_read_only_and_running_states() {
    let app = local_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.route_stack = vec![Route::Services];
        app.views.services.section = ServiceSection::Serve;
        let loading = render_lines(&app, 80, 24);
        assert!(loading.is_some());
        if let Some(loading) = loading {
            assert!(loading.iter().any(|line| line.contains("loading")));
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
        app.views.services.section = ServiceSection::Funnel;
        let partial = render_lines(&app, 80, 24);
        assert!(partial.is_some());
        if let Some(partial) = partial {
            assert!(partial.iter().any(|line| line.contains("stale")));
            assert!(partial.iter().any(|line| line.contains("fictional funnel")));
        }

        app.services_snapshot.taildrop_targets.fail(
            2,
            ServiceFailure::new(
                ServiceFailureKind::DecodeFailed,
                "file cp --targets",
                "target decode failed",
                "fictional target decode failure",
            ),
        );
        app.views.services.section = ServiceSection::Taildrop;
        let failed = render_lines(&app, 80, 24);
        assert!(failed.is_some());
        if let Some(failed) = failed {
            assert!(failed.iter().any(|line| line.contains("failed")));
        }

        app.alpha_local_features = false;
        app.services_snapshot.taildrive.status = ServiceResourceStatus::Unsupported;
        app.services_snapshot.taildrive.failure = None;
        app.views.services.section = ServiceSection::Taildrive;
        let unsupported = render_lines(&app, 80, 24);
        assert!(unsupported.is_some());
        if let Some(unsupported) = unsupported {
            assert!(unsupported.iter().any(|line| line.contains("ALPHA")));
            assert!(unsupported.iter().any(|line| line.contains("disabled")));
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
            assert!(running.iter().any(|line| line.contains("tasks: 1")));
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
            device_name: "office-laptop".to_owned(),
            online: Some(true),
            capability_reason: None,
        }],
    );
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
    app.route_stack = vec![Route::Services];
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
        profile: None,
        access_token_present: false,
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
