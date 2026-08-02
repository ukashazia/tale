use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

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
        mock: true,
    };
    let environment = EnvironmentValues {
        config_file: None,
        profile: None,
        access_token_present: false,
        tailscale_path: None,
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
