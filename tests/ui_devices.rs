use std::fs;
use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use tale::app::{App, Focus, Route, SourceMode};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::device::{DeviceId, SortDirection, SortField, SortSpec};
use tale::domain::filter;
use tale::effect::Effect;
use tale::event::{Event, InputEvent, LocalEvent, SourceEvent};
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
            assert!(lines.iter().any(|line| line.contains("OWNER")));
            assert!(lines.iter().any(|line| line.contains("TAGS")));
            assert!(lines.iter().any(|line| line.contains("ROUTES")));
            assert!(lines.iter().any(|line| line.contains("RX")));
            assert!(lines.iter().all(|line| !line.contains('\n')));
        }

        let dns_filter = filter::parse("tail.example.ts.net", &filter::device_schema());
        assert!(dns_filter.is_ok());
        if let Ok(dns_filter) = dns_filter {
            app.views.devices.applied_filter = dns_filter;
            assert_eq!(app.visible_indices().len(), 2);
        }
        let property_filter = filter::parse("property:exit-node", &filter::device_schema());
        assert!(property_filter.is_ok());
        if let Ok(property_filter) = property_filter {
            app.views.devices.applied_filter = property_filter;
            assert_eq!(app.visible_indices().len(), 1);
        }
        let property_filter = filter::parse("property:exit-node-option", &filter::device_schema());
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

/// The table starts with the whole width and `i` brings the side pane in and
/// out. It is not `Enter`: the table keeps the keys and keeps its own shape,
/// rather than being replaced by a full-width detail view.
#[test]
fn pressing_i_shows_and_hides_the_inspector_beside_the_table() {
    let app = local_app();
    assert!(app.is_some());
    let Some(mut app) = app else {
        return;
    };
    let snapshot = decode_status(STATUS, "1.98.9".to_owned(), None, 1_754_000_000);
    assert!(snapshot.is_ok());
    if let Ok(snapshot) = snapshot {
        app.local_resource.generation = 1;
        let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
            generation: 1,
            snapshot: Box::new(snapshot),
        })));
    }
    app.set_route(Route::Devices);

    // The pane's border title, not the footer hint that also says "inspector".
    let pane_drawn = |lines: &[String]| lines.iter().any(|line| line.contains("┌ inspector "));

    let hidden = render_lines(&app, 160, 30);
    assert!(hidden.is_some());
    if let Some(hidden) = hidden {
        assert!(!pane_drawn(&hidden), "the inspector pane opened uninvited");
        // The table has the width the pane would have taken.
        assert!(
            hidden
                .iter()
                .any(|line| line.contains("┌ devices") && line.trim_end().chars().count() > 120),
            "the table is not using the full width"
        );
    }

    press(&mut app, 'i');
    assert_eq!(app.focus, Focus::Collection, "the table lost the keys");
    let shown = render_lines(&app, 160, 30);
    assert!(shown.as_deref().is_some_and(pane_drawn));

    press(&mut app, 'i');
    let hidden_again = render_lines(&app, 160, 30);
    assert!(
        hidden_again
            .as_deref()
            .is_some_and(|lines| !pane_drawn(lines))
    );
}

/// The bar names what landed on the clipboard, not what it was called. A field
/// label only repeats the key that was just pressed; the value is the thing
/// worth checking without pasting somewhere to see it.
#[test]
fn copying_a_field_reports_the_text_that_was_copied() {
    let Some(mut app) = mock_app() else {
        return;
    };
    let _ = app.update(Event::Source(SourceEvent::LoadSucceeded {
        generation: 1,
        devices: tale::mock::devices(),
        observed_at: tale::mock::MOCK_NOW,
    }));
    app.set_route(Route::Devices);
    // A device with two addresses, so the multi-address path is exercised too.
    app.views.devices.selected_id = Some(DeviceId::new("dev-a01"));
    let Some(device) = app.selected_device().cloned() else {
        return;
    };
    assert_eq!(device.addresses.len(), 2);

    press(&mut app, 'y');
    press(&mut app, 'n');
    assert_eq!(
        app.copied_value.as_deref(),
        Some(device.display_name.as_str())
    );
    let lines = render_lines(&app, 120, 24);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        assert!(
            lines
                .iter()
                .any(|line| line.contains(&format!("copied: {}", device.display_name))),
            "the status bar does not report the copied text"
        );
    }

    // Several addresses are one per line on the clipboard and one line in the
    // bar: the bar confirms what was copied, it does not reproduce it.
    press(&mut app, 'y');
    press(&mut app, 'a');
    press(&mut app, 'a');
    assert_eq!(
        app.copied_value.as_deref(),
        Some(device.addresses.join("\n").as_str())
    );
    let lines = render_lines(&app, 120, 24);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        let bar = lines
            .iter()
            .find(|line| line.contains("copied: "))
            .cloned()
            .unwrap_or_default();
        assert!(
            bar.contains(&device.addresses.join(" · ")),
            "addresses were not joined onto one line: {bar}"
        );
    }

    // A value too long for the bar is cut, not wrapped into the view.
    app.copied_value = Some("x".repeat(400));
    let lines = render_lines(&app, 120, 24);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        assert!(
            lines
                .iter()
                .any(|line| line.contains("copied: ") && line.contains('…'))
        );
    }
}

/// `y d` copies the name the device answers to across the tailnet. The client
/// reports it with a trailing dot, which belongs in a zone file and nowhere
/// this value gets pasted.
#[test]
fn copying_the_dns_name_gives_the_full_magicdns_name_without_its_trailing_dot() {
    let Some(mut app) = local_app() else {
        return;
    };
    let snapshot = decode_status(STATUS, "1.98.9".to_owned(), None, 1_754_000_000);
    assert!(snapshot.is_ok());
    if let Ok(snapshot) = snapshot {
        app.local_resource.generation = 1;
        let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
            generation: 1,
            snapshot: Box::new(snapshot),
        })));
    }
    app.set_route(Route::Devices);
    app.views.devices.selected_id = Some(DeviceId::new("nodekey:direct"));

    let name = app.selected_dns_name();
    assert!(
        name.as_deref()
            .is_some_and(|name| name.ends_with(".ts.net")),
        "no MagicDNS name for the selected device: {name:?}"
    );
    assert!(name.as_deref().is_some_and(|name| !name.ends_with('.')));

    // The menu offers it, and its key copies it.
    press(&mut app, 'y');
    let lines = render_lines(&app, 120, 30);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        assert!(
            lines.iter().any(|line| line.contains("d DNS name")),
            "the copy menu does not offer the DNS name"
        );
    }
    // Against a real client the clipboard is asynchronous, so the key produces
    // the effect that carries the text rather than setting it here.
    let effects = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('d'),
        KeyModifiers::NONE,
    ))));
    assert_eq!(
        effects.iter().find_map(|effect| match effect {
            Effect::CopyText { text } => Some(text.clone()),
            _ => None,
        }),
        name
    );
}

/// The account says who is signed in; the domain says which tailnet the devices
/// are on. Two facts, two rows.
#[test]
fn the_header_shows_the_tailnet_domain_below_the_account() {
    let Some(mut app) = local_app() else {
        return;
    };
    let snapshot = decode_status(STATUS, "1.98.9".to_owned(), None, 1_754_000_000);
    assert!(snapshot.is_ok());
    if let Ok(snapshot) = snapshot {
        app.local_resource.generation = 1;
        let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
            generation: 1,
            snapshot: Box::new(snapshot),
        })));
    }
    app.set_route(Route::Devices);

    let lines = render_lines(&app, 120, 30);
    assert!(lines.is_some());
    if let Some(lines) = lines {
        let account = lines
            .iter()
            .position(|line| line.contains("Example Tailnet"));
        let domain = lines
            .iter()
            .position(|line| line.contains("tail.example.ts.net"));
        assert!(account.is_some(), "the header lost the account");
        assert!(domain.is_some(), "the header does not show the tailnet");
        assert_eq!(
            domain,
            account.map(|row| row.saturating_add(1)),
            "the tailnet is not on the row below the account"
        );
    }

    // Below 26 rows the header is one line, so the domain follows the account
    // rather than disappearing.
    let short = render_lines(&app, 120, 24);
    assert!(short.is_some());
    if let Some(short) = short {
        assert!(
            short.iter().any(
                |line| line.contains("Example Tailnet") && line.contains("tail.example.ts.net")
            )
        );
    }
}

fn press(app: &mut App, character: char) {
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char(character),
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

/// The same app with simulated devices, where a copy lands in state instead of
/// on the system clipboard.
fn mock_app() -> Option<App> {
    build_app(true)
}

fn local_app() -> Option<App> {
    build_app(false)
}

fn build_app(mock: bool) -> Option<App> {
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
        mock,
    };
    let environment = EnvironmentValues {
        config_file: None,
        profile: None,
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
