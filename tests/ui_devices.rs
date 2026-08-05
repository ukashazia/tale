use std::fs;
use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use tale::app::{App, Route, SourceMode};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::device::{SortDirection, SortField, SortSpec};
use tale::domain::filter;
use tale::event::{Event, LocalEvent};
use tale::local::daemon::decode_status;
use tale::paths::{PathEnvironment, Platform};

const STATUS: &str = include_str!("fixtures/tailscale/1.98.9/linux/status.json");

#[test]
fn local_devices_render_wide_fields_and_support_phase_two_filters() {
    let app = local_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let snapshot = decode_status(STATUS, "1.98.9".to_owned(), None, 1_754_000_000);
        assert!(snapshot.is_ok());
        if let Ok(snapshot) = snapshot {
            app.local_resource.generation = 1;
            let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
                generation: 1,
                snapshot: Box::new(snapshot),
            })));
        }
        assert_eq!(app.source_mode, SourceMode::Local);
        app.set_route(Route::Devices);
        app.views.devices.wide_columns = true;
        let lines = render_lines(&app, 280, 35);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert!(lines.iter().any(|line| line.contains("OWNER/TAGS")));
            assert!(lines.iter().any(|line| line.contains("ROUTES")));
            assert!(lines.iter().any(|line| line.contains("RX")));
            assert!(lines.iter().all(|line| !line.contains('\n')));
        }

        let dns_filter = filter::parse("tail.example.ts.net");
        assert!(dns_filter.is_ok());
        if let Ok(dns_filter) = dns_filter {
            app.views.devices.applied_filter = dns_filter;
            assert_eq!(app.visible_indices().len(), 2);
        }
        let property_filter = filter::parse("property:exit-node");
        assert!(property_filter.is_ok());
        if let Ok(property_filter) = property_filter {
            app.views.devices.applied_filter = property_filter;
            assert_eq!(app.visible_indices().len(), 1);
        }
        let property_filter = filter::parse("property:exit-node-option");
        assert!(property_filter.is_ok());
        if let Ok(property_filter) = property_filter {
            app.views.devices.applied_filter = property_filter;
            assert_eq!(app.visible_indices().len(), 1);
        }
        app.views.devices.applied_filter = filter::FilterExpression::empty();
        assert_eq!(app.visible_indices().len(), 4);
        assert_eq!(
            app.visible_indices()
                .first()
                .and_then(|index| app.devices_resource.snapshot.get(*index))
                .map(|device| device.display_name.as_str()),
            Some("observer")
        );
        app.views.devices.sort = SortSpec {
            field: SortField::Rx,
            direction: SortDirection::Descending,
        };
        assert_eq!(app.visible_indices().len(), 4);
    }
}

#[test]
fn selected_local_device_id_survives_same_id_refresh() {
    let app = local_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let snapshot = decode_status(STATUS, "1.98.9".to_owned(), None, 1_754_000_000);
        assert!(snapshot.is_ok());
        if let Ok(snapshot) = snapshot {
            app.local_resource.generation = 1;
            let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
                generation: 1,
                snapshot: Box::new(snapshot),
            })));
        }
        app.views.devices.selected_id = Some(tale::domain::device::DeviceId::new("nodekey:direct"));
        let snapshot = decode_status(STATUS, "1.98.9".to_owned(), None, 1_754_000_010);
        assert!(snapshot.is_ok());
        if let Ok(snapshot) = snapshot {
            let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
                generation: 2,
                snapshot: Box::new(snapshot),
            })));
        }
        assert_eq!(
            app.views.devices.selected_id.map(|id| id.0),
            Some("nodekey:direct".to_owned())
        );
    }
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

fn local_app() -> Option<App> {
    let root = std::env::temp_dir().join(format!("tale-ui-devices-{}", std::process::id()));
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
        tailscale_path: Some(PathBuf::from("tailscale")),
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
    config::resolve(&cli, &environment, &paths)
        .ok()
        .map(App::new)
}
