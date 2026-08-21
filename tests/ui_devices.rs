use std::fs;
use std::path::PathBuf;

use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use tale::action::ActionId;
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
fn initial_device_load_is_not_rendered_as_an_empty_fleet() {
    let Some(mut app) = local_app() else {
        return;
    };
    app.set_route(Route::Devices);
    let Some(lines) = render_lines(&app, 120, 30) else {
        return;
    };
    assert!(lines.iter().any(|line| line.contains("Loading devices…")));
    assert!(lines.iter().any(|line| line.contains("devices · loading")));
    assert!(!lines.iter().any(|line| line.contains("devices · 0")));
}

#[test]
fn device_cursor_scrolls_to_stay_visible_at_bottom_edge() {
    let Some(mut app) = local_app() else {
        return;
    };
    let Ok(snapshot) = decode_status(STATUS, "1.98.9".to_owned(), None, 1_754_000_000) else {
        return;
    };
    app.local_resource.generation = 1;
    let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
        generation: 1,
        snapshot: Box::new(snapshot),
    })));
    let Some(seed) = app.devices_resource.snapshot.first().cloned() else {
        return;
    };
    app.devices_resource.snapshot = (0..20)
        .map(|index| {
            let mut device = seed.clone();
            device.id = DeviceId::new(format!("device-{index:02}"));
            device.display_name = format!("device-{index:02}");
            device
        })
        .collect();
    app.devices_resource.generation = app.devices_resource.generation.saturating_add(1);
    app.set_route(Route::Devices);
    app.set_terminal_size(80, 24);
    app.views.devices.selected_id = Some(DeviceId::new("device-00"));

    for _ in 0..19 {
        press(&mut app, 'j');
    }

    assert!(app.views.devices.scroll > 0);
    let Some(lines) = render_lines(&app, 80, 24) else {
        return;
    };
    assert!(lines.iter().any(|line| line.contains("device-19")));
}

#[test]
fn local_devices_render_wide_fields_and_supported_filters() {
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
        for (device, age) in
            app.devices_resource
                .snapshot
                .iter_mut()
                .zip([Some(1), Some(60), Some(3_600), None])
        {
            device.last_seen = age.map(|age| app.now.saturating_sub(age));
        }
        app.devices_resource.generation = app.devices_resource.generation.saturating_add(1);
        let ages = app
            .visible_indices()
            .iter()
            .filter_map(|index| app.devices_resource.snapshot.get(*index))
            .filter_map(|device| device.age_at(app.now))
            .collect::<Vec<_>>();
        assert_eq!(ages, vec![3_600, 60, 1]);
        assert!(app
            .visible_indices()
            .last()
            .and_then(|index| app.devices_resource.snapshot.get(*index))
            .is_some_and(|device| device.last_seen.is_none()));
        app.views.devices.sort = SortSpec {
            field: SortField::Rx,
            direction: SortDirection::Descending,
        };
        assert_eq!(app.visible_indices().len(), 4);
    }
}

#[test]
fn column_mode_is_visible_in_the_title_and_reports_changes() {
    let Some(mut app) = local_app() else {
        return;
    };
    let snapshot = decode_status(STATUS, "1.98.9".to_owned(), None, 1_754_000_000);
    let Ok(snapshot) = snapshot else {
        return;
    };
    app.local_resource.generation = 1;
    let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
        generation: 1,
        snapshot: Box::new(snapshot),
    })));
    app.set_route(Route::Devices);

    let Some(standard) = render_lines(&app, 140, 30) else {
        return;
    };
    assert!(standard
        .iter()
        .any(|line| line.contains("columns: standard")));

    let _ = app.dispatch_action(ActionId::CollectionWideColumns);
    assert_eq!(
        app.status_notice.as_deref(),
        Some("device columns: extended")
    );
    assert!(app.runtime_error.is_none());
    let Some(extended) = render_lines(&app, 140, 30) else {
        return;
    };
    assert!(extended
        .iter()
        .any(|line| line.contains("columns: extended")));
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
    assert!(hidden_again
        .as_deref()
        .is_some_and(|lines| !pane_drawn(lines)));
}

/// A peer that never told us its client version gets the same dash every other
/// empty column gets. `not returned` reads like a value, sorts like one, and
/// copies like one.
#[test]
fn a_device_without_a_reported_version_shows_a_dash() {
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
    app.views.devices.selected_id = Some(DeviceId::new("nodekey:derp"));
    assert_eq!(
        app.selected_device()
            .and_then(|device| device.version.clone()),
        None,
        "the fixture peer is expected to omit its version"
    );

    app.views.devices.wide_columns = true;
    let Some(table) = render_lines(&app, 280, 35) else {
        return;
    };
    // The border glyphs are multi-byte, so the cell starts at a character
    // offset rather than the byte offset `find` reports.
    let version_column = table
        .iter()
        .find(|line| line.contains(" VER "))
        .and_then(|header| header.find(" VER ").map(|byte| header[..byte].chars()))
        .map(Iterator::count);
    assert!(version_column.is_some(), "the VER column is not on screen");
    if let Some(column) = version_column {
        let row = table.iter().find(|line| line.contains("relay fixture"));
        assert!(row.is_some());
        if let Some(row) = row {
            let cell = row.chars().skip(column + 1).take(3).collect::<String>();
            assert_eq!(cell.trim(), "-", "the VER cell reads {cell:?} in {row:?}");
        }
    }

    press(&mut app, 'i');
    let Some(lines) = render_lines(&app, 160, 30) else {
        return;
    };
    assert!(
        lines
            .iter()
            .any(|line| line.contains("OS/version") && line.contains("/ -")),
        "the inspector did not dash the missing version: {lines:?}"
    );
}

#[test]
fn enter_opens_a_scrollable_full_device_record_without_moving_selection() {
    let Some(mut app) = local_app() else {
        return;
    };
    let Ok(snapshot) = decode_status(STATUS, "1.98.9".to_owned(), None, 1_754_000_000) else {
        return;
    };
    app.local_resource.generation = 1;
    let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
        generation: 1,
        snapshot: Box::new(snapshot),
    })));
    app.set_route(Route::Devices);
    app.views.devices.selected_id = Some(DeviceId::new("nodekey:direct"));
    app.set_terminal_size(80, 24);

    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    ))));
    assert_eq!(app.focus, Focus::Inspector);
    let selected = app.views.devices.selected_id.clone();
    let Some(top) = render_lines(&app, 80, 24) else {
        return;
    };
    let top = top.join("\n");
    for wanted in [
        "device details · local",
        "Identity · local daemon",
        "node public key",
        "full domain",
        "build.tail.example.ts.net",
        "/ search",
    ] {
        assert!(top.contains(wanted), "device details are missing {wanted}");
    }

    press(&mut app, 'G');
    assert_eq!(app.views.devices.selected_id, selected);
    assert!(app.views.devices.detail_scroll > 0);
    let Some(bottom) = render_lines(&app, 80, 24) else {
        return;
    };
    let bottom = bottom.join("\n");
    for wanted in [
        "Source · local daemon",
        "Not observable from adopted APIs",
        "relay latency",
        "TLS certificate",
    ] {
        assert!(
            bottom.contains(wanted),
            "scrolled details are missing {wanted}"
        );
    }

    // A taller resize reduces the maximum offset immediately. The old state
    // kept the now-invisible extra offset and made `k` pay it back first.
    app.set_terminal_size(160, 45);
    press(&mut app, 'G');
    let bottom_scroll = app.views.devices.detail_scroll;
    for _ in 0..100 {
        press(&mut app, 'j');
    }
    for _ in 0..20 {
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
        ))));
    }
    assert_eq!(app.views.devices.detail_scroll, bottom_scroll);

    press(&mut app, 'k');
    assert_eq!(app.views.devices.selected_id, selected);
    assert_eq!(
        app.views.devices.detail_scroll,
        bottom_scroll.saturating_sub(1)
    );
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    ))));
    assert_eq!(
        app.views.devices.detail_scroll,
        bottom_scroll.saturating_sub(6)
    );
}

#[test]
fn reported_capabilities_are_readable_bounded_rows() {
    let Some(mut app) = local_app() else {
        return;
    };
    let Ok(mut snapshot) = decode_status(STATUS, "1.98.9".to_owned(), None, 1_754_000_000) else {
        return;
    };
    let Some(device) = snapshot
        .peers
        .iter_mut()
        .find(|device| device.id.0 == "nodekey:direct")
    else {
        return;
    };
    for capability in [
        "defaultAutoUpdate",
        "funnel",
        "https",
        "https://tailscale.com/cap/file-sharing",
        "https://tailscale.com/cap/funnel-ports?ports=443,8443,10000",
        "https://tailscale.com/cap/is-admin",
        "https://example.com/cap/a-deliberately-long-future-capability-name",
    ] {
        device.capabilities.insert(capability.to_owned(), true);
    }
    app.local_resource.generation = 1;
    let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
        generation: 1,
        snapshot: Box::new(snapshot),
    })));
    app.set_route(Route::Devices);
    app.views.devices.selected_id = Some(DeviceId::new("nodekey:direct"));
    app.set_terminal_size(68, 60);
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    ))));

    let Some(lines) = render_lines(&app, 68, 60) else {
        return;
    };
    let detail = lines.join("\n");
    for wanted in [
        "Reported capabilities · local daemon",
        "default auto-update",
        "Funnel",
        "HTTPS",
        "file sharing",
        "Funnel ports",
        "443, 8443, 10000",
        "tailnet admin",
        "other capability",
        "https://example.com/cap/",
    ] {
        assert!(
            detail.contains(wanted),
            "device details are missing {wanted}"
        );
    }
    assert!(!detail.contains("httpstailscalecomcap"));
    assert!(lines.iter().all(|line| line.chars().count() <= 68));
    let funnel_ports = lines.iter().find(|line| line.contains("Funnel ports"));
    assert!(funnel_ports.is_some());
    if let Some(funnel_ports) = funnel_ports {
        assert!(!funnel_ports.contains("file sharing"));
        assert!(!funnel_ports.contains("tailnet admin"));
    }
}

#[test]
fn slash_searches_inside_device_details_and_n_walks_matches() {
    let Some(mut app) = local_app() else {
        return;
    };
    let Ok(snapshot) = decode_status(STATUS, "1.98.9".to_owned(), None, 1_754_000_000) else {
        return;
    };
    app.local_resource.generation = 1;
    let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
        generation: 1,
        snapshot: Box::new(snapshot),
    })));
    app.set_route(Route::Devices);
    app.views.devices.selected_id = Some(DeviceId::new("nodekey:direct"));
    app.set_terminal_size(80, 24);
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    ))));

    press(&mut app, '/');
    assert!(matches!(
        app.interaction,
        tale::app::InteractionMode::FilterLine(tale::app::FilterLineState {
            purpose: tale::app::FilterLinePurpose::DetailSearch { .. },
            ..
        })
    ));
    let _ = app.update(Event::Input(InputEvent::Paste("local daemon".to_owned())));
    assert_eq!(app.views.devices.detail_search, "local daemon");
    let first_match = app.views.devices.detail_search_match;
    assert!(first_match.is_some());
    let Some(prompt) = render_lines(&app, 80, 24) else {
        return;
    };
    assert!(prompt
        .iter()
        .any(|line| line.contains("Search device details")));

    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    ))));
    assert!(matches!(
        app.interaction,
        tale::app::InteractionMode::Normal
    ));
    app.set_terminal_size(160, 45);
    let Some(detail) = render_lines(&app, 160, 45) else {
        return;
    };
    assert!(detail.iter().any(|line| line.contains("match 1/")));

    press(&mut app, 'n');
    assert_ne!(app.views.devices.detail_search_match, first_match);
    press(&mut app, 'N');
    assert_eq!(app.views.devices.detail_search_match, first_match);
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
        assert!(lines
            .iter()
            .any(|line| line.contains("copied: ") && line.contains('…')));
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

/// The tailnet says who this machine is signed in to; the MagicDNS suffix says
/// what its devices answer to. One row names both, because on its own the suffix
/// reads as a second tailnet — which is exactly what it looked like once a
/// profile put a real second tailnet on the screen beside it.
#[test]
fn the_header_names_the_local_tailnet_and_its_domain_together() {
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
        let local = lines.iter().find(|line| line.contains("Local:"));
        assert!(local.is_some(), "the header has no local row");
        assert!(
            local.is_some_and(
                |row| row.contains("Example Tailnet") && row.contains("tail.example.ts.net")
            ),
            "the local row does not carry both the tailnet and its domain: {local:?}"
        );
        // The row is named, so neither value can be mistaken for the other or
        // for the profile's.
        assert!(lines.iter().any(|line| line.contains("Profile:")));
    }

    // Below 26 rows the header is one line, so the tailnet keeps the word that
    // says which of the two identities it is.
    let short = render_lines(&app, 120, 24);
    assert!(short.is_some());
    if let Some(short) = short {
        assert!(
            short
                .iter()
                .any(|line| line.contains("local") && line.contains("Example Tailnet")),
            "the compact header dropped the local tailnet or its label"
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
