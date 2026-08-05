use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tale::action::ActionId;

use tale::app::{App, Focus, InteractionMode, Route};
use tale::cli::Cli;
use tale::config::{self, ColorMode, EnvironmentValues, SymbolsMode};
use tale::event::{Event, InputEvent, SourceEvent};
use tale::mock;
use tale::paths::{PathEnvironment, Platform};
use tale::ui;
use tale::ui::theme::{ColorCapability, StyleRole, Theme, ThemeId};

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
    app.set_route(Route::Devices);
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

fn press(app: &mut App, code: KeyCode) {
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        code,
        KeyModifiers::NONE,
    ))));
}

#[test]
fn interaction_surfaces_are_bottom_anchored_at_all_required_viewports() {
    for (width, height) in [(160, 45), (110, 30), (80, 24), (60, 18)] {
        let app = populated_app();
        assert!(app.is_some());
        if let Some(mut app) = app {
            press(&mut app, KeyCode::Char(':'));
            let _ = app.update(Event::Input(InputEvent::Paste("dev".to_owned())));
            let lines = lines_at(&app, width, height);
            assert!(lines.is_some());
            if let Some(lines) = lines {
                assert!(lines.iter().any(|line| line.contains(": dev")));
                assert!(lines.iter().any(|line| line.contains('▏')));
            }

            press(&mut app, KeyCode::Esc);
            press(&mut app, KeyCode::Char('/'));
            let _ = app.update(Event::Input(InputEvent::Paste("owner:\"".to_owned())));
            let lines = lines_at(&app, width, height);
            assert!(lines.is_some());
            if let Some(lines) = lines {
                assert!(lines.last().is_some_and(|line| line.contains("/ owner")));
                assert!(lines.last().is_some_and(|line| line.contains("column")));
            }

            press(&mut app, KeyCode::Esc);
            press(&mut app, KeyCode::Char('a'));
            let lines = lines_at(&app, width, height);
            assert!(lines.is_some());
            if let Some(lines) = lines {
                assert!(lines.iter().any(|line| line.contains("Actions")));
                assert!(lines.iter().any(|line| line.contains("Simulation")));
                assert!(lines.iter().any(|line| line.contains("Views")));
                assert!(!lines.iter().any(|line| line.contains("[disabled:")));
                assert!(
                    !lines.iter().take(usize::from(height / 2)).any(|line| {
                        line.contains("Actions ›") || line.contains("Esc cancel")
                    })
                );
            }

            press(&mut app, KeyCode::Char('v'));
            let lines = lines_at(&app, width, height);
            assert!(lines.is_some());
            if let Some(lines) = lines {
                assert!(
                    lines
                        .last()
                        .is_some_and(|line| line.contains("v …  waiting for next key"))
                );
                assert!(lines.iter().any(|line| line.contains("Simulation")));
                assert!(lines.iter().any(|line| line.contains("Views")));
                assert!(lines.iter().any(|line| line.contains("Esc Back")));
            }

            press(&mut app, KeyCode::Esc);
            let lines = lines_at(&app, width, height);
            assert!(lines.is_some());
            if let Some(lines) = lines {
                assert!(lines.iter().any(|line| line.contains("Actions")));
                assert!(lines.iter().any(|line| line.contains("Esc Close")));
                assert!(
                    !lines
                        .iter()
                        .any(|line| line.contains("waiting for next key"))
                );
            }

            press(&mut app, KeyCode::Esc);
            press(&mut app, KeyCode::Char('y'));
            let lines = lines_at(&app, width, height);
            assert!(lines.is_some());
            if let Some(lines) = lines {
                assert!(lines.last().is_some_and(|line| line.contains("Copy")));
            }

            press(&mut app, KeyCode::Esc);
            press(&mut app, KeyCode::Char('?'));
            let lines = lines_at(&app, width, height);
            assert!(lines.is_some());
            if let Some(lines) = lines {
                let help_row = lines.iter().position(|line| line.contains("help ·"));
                assert!(help_row.is_some());
                if let Some(help_row) = help_row {
                    assert!(help_row >= usize::from(height.saturating_mul(2) / 5));
                }
            }
        }
    }
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
        error.set_route(Route::Devices);
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

    let reducer_notice = populated_app();
    assert!(reducer_notice.is_some());
    if let Some(mut reducer_notice) = reducer_notice {
        reducer_notice.runtime_error =
            Some("selected resource no longer exists; selection was repaired".to_owned());
        let lines = lines_at(&reducer_notice, 80, 24);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert!(
                lines
                    .iter()
                    .any(|line| line.contains("selection was repaired"))
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
        assert!(matches!(overlay.interaction, InteractionMode::HelpSheet(_)));
        let lines = lines_at(&overlay, 80, 24);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert!(lines.iter().any(|line| line.contains("help")));
            assert!(lines.iter().any(|line| line.contains("Navigation")));
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
fn command_caret_keeps_the_prompt_surface_background() {
    let app = populated_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        press(&mut app, KeyCode::Char(':'));
        let _ = app.update(Event::Input(InputEvent::Paste("e".to_owned())));
        let area = ratatui::layout::Rect::new(0, 0, 80, 24);
        let layout = tale::ui::layout::compute(area, &app);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).ok();
        assert!(terminal.is_some());
        if let Some(terminal) = terminal.as_mut() {
            assert!(terminal.draw(|frame| ui::render(frame, &app)).is_ok());
            let prompt_y = layout
                .footer
                .y
                .saturating_add(layout.footer.height.saturating_sub(2));
            let input_cell = terminal.backend().buffer().cell((2, prompt_y));
            let caret_cell = terminal.backend().buffer().cell((3, prompt_y));
            assert!(input_cell.is_some());
            assert!(caret_cell.is_some());
            if let (Some(input_cell), Some(caret_cell)) = (input_cell, caret_cell) {
                assert_eq!(caret_cell.symbol(), "▏");
                assert_eq!(caret_cell.bg, input_cell.bg);
                assert_eq!(
                    Some(caret_cell.bg),
                    app.theme.style(StyleRole::SurfaceRaised).bg
                );
            }
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
    activity.set_route(Route::Activity);
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
fn mouse_footer_completion_transient_and_outside_cancel_match_keys() {
    let app = populated_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.resolved_config.ui.mouse = true;
        app.terminal_width = 80;
        app.terminal_height = 24;

        press(&mut app, KeyCode::Char(':'));
        let _ = app.update(Event::Input(InputEvent::Paste("dev".to_owned())));
        let layout = tale::ui::layout::compute(ratatui::layout::Rect::new(0, 0, 80, 24), &app);
        let _ = app.update(Event::Input(InputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: layout.footer.y,
            modifiers: KeyModifiers::NONE,
        })));
        assert!(matches!(
            app.interaction,
            InteractionMode::CommandLine(ref state) if !state.editor.input.is_empty()
        ));

        let _ = app.update(Event::Input(InputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 0,
            modifiers: KeyModifiers::NONE,
        })));
        assert!(matches!(app.interaction, InteractionMode::Normal));

        press(&mut app, KeyCode::Char(':'));
        let _ = app.update(Event::Input(InputEvent::Paste("dev".to_owned())));
        let layout = tale::ui::layout::compute(ratatui::layout::Rect::new(0, 0, 80, 24), &app);
        let _ = app.update(Event::Input(InputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: layout.footer.y.saturating_add(3),
            modifiers: KeyModifiers::NONE,
        })));
        assert_eq!(app.current_route(), Route::Devices);
        assert!(matches!(app.interaction, InteractionMode::Normal));

        app.set_route(Route::Overview);
        press(&mut app, KeyCode::Char('a'));
        let layout = tale::ui::layout::compute(ratatui::layout::Rect::new(0, 0, 80, 24), &app);
        let _ = app.update(Event::Input(InputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: layout.footer.y.saturating_add(3),
            modifiers: KeyModifiers::NONE,
        })));
        assert_eq!(app.tasks.all().len(), 1);
        assert!(matches!(app.interaction, InteractionMode::Normal));
    }
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

#[test]
fn complete_theme_capability_viewport_matrix_has_semantic_cells() {
    for theme_id in ThemeId::ALL {
        for capability in ColorCapability::ALL {
            for scene in 0..5 {
                for (width, height) in [(160, 45), (110, 30), (80, 24), (60, 18), (59, 17)] {
                    let app = populated_app();
                    assert!(app.is_some());
                    let Some(mut app) = app else {
                        return;
                    };
                    app.theme = Theme::new(theme_id, capability);
                    match scene {
                        1 => {
                            press(&mut app, KeyCode::Char(':'));
                            let _ = app.update(Event::Input(InputEvent::Paste(
                                "unknown-route".to_owned(),
                            )));
                            press(&mut app, KeyCode::Enter);
                        }
                        2 => press(&mut app, KeyCode::Char('a')),
                        3 => press(&mut app, KeyCode::Char('?')),
                        4 => {
                            app.set_route(Route::Settings);
                            let effects = app.dispatch_action(ActionId::SettingsAppearance);
                            assert!(effects.is_empty());
                        }
                        _ => {}
                    }
                    let backend = TestBackend::new(width, height);
                    let mut terminal = match Terminal::new(backend) {
                        Ok(terminal) => terminal,
                        Err(_) => return,
                    };
                    assert!(terminal.draw(|frame| ui::render(frame, &app)).is_ok());
                    let buffer = terminal.backend().buffer();
                    let mut has_tale = false;
                    let mut has_non_reset = false;
                    for y in 0..height {
                        for x in 0..width {
                            if let Some(cell) = buffer.cell((x, y)) {
                                has_tale |= cell.symbol() == "T";
                                has_non_reset |= cell.fg != ratatui::style::Color::Reset
                                    || cell.bg != ratatui::style::Color::Reset;
                                if capability == ColorCapability::None {
                                    assert_eq!(cell.fg, ratatui::style::Color::Reset);
                                    assert_eq!(cell.bg, ratatui::style::Color::Reset);
                                }
                            }
                        }
                    }
                    assert!(has_tale);
                    if capability != ColorCapability::None
                        && !(theme_id == ThemeId::Terminal && (width < 60 || height < 18))
                    {
                        assert!(has_non_reset);
                    }
                }
            }
        }
    }
}

#[test]
fn settings_appearance_preview_renders_state_source_and_risk_labels() {
    let app = populated_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.set_route(Route::Settings);
        let effects = app.dispatch_action(ActionId::SettingsAppearance);
        assert!(effects.is_empty());
        let lines = lines_at(&app, 80, 24);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            let rendered = lines.join("\n");
            assert!(rendered.contains("tailscale-dark"));
            assert!(rendered.contains("healthy"));
            assert!(rendered.contains("danger/public"));
            assert!(rendered.contains("local+admin"));
            assert!(rendered.contains("ui.theme"));
        }
    }
}
