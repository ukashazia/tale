use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tale::action::ActionId;

use tale::app::{App, Focus, Overlay, Route};
use tale::cli::Cli;
use tale::config::{self, ColorMode, EnvironmentValues, SymbolsMode};
use tale::event::{Event, InputEvent, SourceEvent};
use tale::mock;
use tale::paths::{PathEnvironment, Platform};
use tale::ui;

fn mock_app() -> Option<App> {
    let root = PathBuf::from("/fictional/tale-snapshots");
    let cli = Cli {
        command: None,
        profile: None,
        config: Some(root.join("missing.toml")),
        view: None,
        read_only: false,
        no_local: false,
        tailscale_path: None,
        tailscale_socket: None,
        mock: true,
    };
    let environment = EnvironmentValues {
        config_file: None,
        profile: None,
        access_token_present: false,
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

fn populated_app() -> Option<App> {
    let mut app = mock_app()?;
    app.route_stack = vec![Route::Devices];
    let _ = app.update(Event::Source(SourceEvent::LoadSucceeded {
        generation: 1,
        devices: mock::devices(),
        observed_at: mock::MOCK_NOW,
    }));
    Some(app)
}

fn lines_at(app: &App, width: u16, height: u16) -> Option<Vec<String>> {
    let backend = TestBackend::new(width, height);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(_) => return None,
    };
    if terminal.draw(|frame| ui::render(frame, app)).is_err() {
        return None;
    }
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

fn assert_frame_shape(lines: &[String], width: u16, height: u16) {
    assert_eq!(lines.len(), usize::from(height));
    assert!(lines.iter().all(|line| !line.contains('\n')));
    assert!(lines.iter().any(|line| line.contains("Tale")));
    assert!(
        lines
            .iter()
            .all(|line| line.chars().count() >= usize::from(width.saturating_sub(1)))
    );
}

#[test]
fn required_responsive_frames_render_without_wrapped_rows() {
    let empty = mock_app();
    assert!(empty.is_some());
    if let Some(mut empty) = empty {
        empty.resolved_config.ui.color = ColorMode::None;
        empty.resolved_config.ui.symbols = SymbolsMode::Ascii;
        let lines = lines_at(&empty, 60, 18);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert_frame_shape(&lines, 60, 18);
            assert!(lines.iter().any(|line| line.contains("devices")));
        }
    }

    let populated = populated_app();
    assert!(populated.is_some());
    if let Some(mut populated) = populated {
        populated.resolved_config.ui.color = ColorMode::None;
        populated.resolved_config.ui.symbols = SymbolsMode::Ascii;
        let lines = lines_at(&populated, 80, 24);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert_frame_shape(&lines, 80, 24);
            assert!(lines.iter().any(|line| line.contains("devices")));
            assert!(lines.iter().any(|line| line.contains("build-01")));
        }
    }

    let populated = populated_app();
    assert!(populated.is_some());
    if let Some(mut populated) = populated {
        populated.resolved_config.ui.color = ColorMode::Ansi256;
        populated.resolved_config.ui.symbols = SymbolsMode::Unicode;
        let lines = lines_at(&populated, 110, 30);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert_frame_shape(&lines, 110, 30);
            assert!(lines.iter().any(|line| line.contains('●')));
            assert!(lines.iter().any(|line| line.contains("inspector")));
        }
    }

    let populated = populated_app();
    assert!(populated.is_some());
    if let Some(mut populated) = populated {
        populated.resolved_config.ui.color = ColorMode::TrueColor;
        populated.resolved_config.ui.symbols = SymbolsMode::Unicode;
        let lines = lines_at(&populated, 160, 45);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert_frame_shape(&lines, 160, 45);
            assert!(lines.iter().any(|line| line.contains("inspector")));
        }
    }
}

#[test]
fn stale_error_overlay_long_text_and_minimum_states_are_visible() {
    let stale = populated_app();
    assert!(stale.is_some());
    if let Some(mut stale) = stale {
        stale.devices_resource.observed_at = Some(mock::MOCK_NOW.saturating_sub(240));
        stale.devices_resource.health = tale::domain::SourceHealth::Stale;
        let lines = lines_at(&stale, 80, 24);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert!(lines.iter().any(|line| line.contains("stale")));
        }
    }

    let error = mock_app();
    assert!(error.is_some());
    if let Some(mut error) = error {
        error.route_stack = vec![Route::Devices];
        let _ = error.update(Event::Source(SourceEvent::LoadFailed {
            generation: 1,
            detail: "fictional source failure".to_owned(),
        }));
        let lines = lines_at(&error, 80, 24);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert!(lines.iter().any(|line| line.contains("error")));
            assert!(
                lines
                    .iter()
                    .any(|line| line.contains("fictional source failure"))
            );
        }
    }

    let overlay = populated_app();
    assert!(overlay.is_some());
    if let Some(mut overlay) = overlay {
        let _ = overlay.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('?'),
            KeyModifiers::NONE,
        ))));
        assert!(matches!(overlay.overlays.last(), Some(Overlay::Help(_))));
        let lines = lines_at(&overlay, 80, 24);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert!(lines.iter().any(|line| line.contains("help")));
            assert!(lines.iter().any(|line| line.contains("collection")));
        }
    }

    let long = populated_app();
    assert!(long.is_some());
    if let Some(mut long) = long {
        long.views.devices.selected_id = Some(tale::domain::device::DeviceId::new("dev-g07"));
        long.focus = Focus::Inspector;
        let lines = lines_at(&long, 80, 24);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert!(lines.iter().any(|line| line.contains("archive-node")));
            assert!(lines.iter().any(|line| line.contains("address")));
        }
    }

    let minimum = populated_app();
    assert!(minimum.is_some());
    if let Some(minimum) = minimum {
        let lines = lines_at(&minimum, 59, 18);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert!(lines.iter().any(|line| line.contains("at least 60")));
        }
    }
}

#[test]
fn mouse_is_opt_in_and_dispatches_the_same_collection_actions() {
    let keyboard = populated_app();
    assert!(keyboard.is_some());
    let mut keyboard = match keyboard {
        Some(value) => value,
        None => return,
    };
    keyboard.dispatch_action(ActionId::CollectionFirst);
    let _ = keyboard.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('j'),
        KeyModifiers::NONE,
    ))));
    let keyboard_selected = keyboard.views.devices.selected_id.clone();

    let mouse_disabled = populated_app();
    assert!(mouse_disabled.is_some());
    let mut mouse_disabled = match mouse_disabled {
        Some(value) => value,
        None => return,
    };
    mouse_disabled.dispatch_action(ActionId::CollectionFirst);
    let before_disabled = mouse_disabled.views.devices.selected_id.clone();
    let _ = mouse_disabled.update(Event::Input(InputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 2,
        row: 4,
        modifiers: KeyModifiers::NONE,
    })));
    assert_eq!(mouse_disabled.views.devices.selected_id, before_disabled);

    let enabled = populated_app();
    assert!(enabled.is_some());
    let mut enabled = match enabled {
        Some(value) => value,
        None => return,
    };
    enabled.resolved_config.ui.mouse = true;
    enabled.dispatch_action(ActionId::CollectionFirst);
    let _ = enabled.update(Event::Input(InputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 2,
        row: 4,
        modifiers: KeyModifiers::NONE,
    })));
    assert_eq!(enabled.views.devices.selected_id, keyboard_selected);

    let activity = populated_app();
    assert!(activity.is_some());
    let mut activity = match activity {
        Some(value) => value,
        None => return,
    };
    activity.resolved_config.ui.mouse = true;
    activity.route_stack = vec![Route::Activity];
    let first = activity
        .tasks
        .create(ActionId::MockSuccess, "first task", 1, false);
    let second = activity
        .tasks
        .create(ActionId::MockFailure, "second task", 1, false);
    activity.tasks.selected = Some(first);
    let _ = activity.update(Event::Input(InputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 4,
        modifiers: KeyModifiers::NONE,
    })));
    assert_eq!(activity.tasks.selected, Some(second));
}

#[test]
fn all_color_and_symbol_modes_render_compact_keyboard_surfaces() {
    for color in [
        ColorMode::None,
        ColorMode::Ansi16,
        ColorMode::Ansi256,
        ColorMode::TrueColor,
    ] {
        for symbols in [SymbolsMode::Ascii, SymbolsMode::Unicode] {
            let app = populated_app();
            assert!(app.is_some());
            if let Some(mut app) = app {
                app.resolved_config.ui.color = color;
                app.resolved_config.ui.symbols = symbols;
                let lines = lines_at(&app, 60, 18);
                assert!(lines.is_some());
                if let Some(lines) = lines {
                    assert_frame_shape(&lines, 60, 18);
                    assert!(lines.iter().any(|line| line.contains("devices")));
                    if symbols == SymbolsMode::Ascii {
                        assert!(!lines.iter().any(|line| line.contains('●')));
                    }
                }
            }
        }
    }
}
