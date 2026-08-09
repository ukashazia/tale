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
    // The wordmark is drawn as art, so look for the status block instead.
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Status:") || line.contains("simulated data"))
    );
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
                // No cell is spent on a drawn caret.
                assert!(!lines.iter().any(|line| line.contains('▏')));
            }

            press(&mut app, KeyCode::Esc);
            press(&mut app, KeyCode::Char('/'));
            let _ = app.update(Event::Input(InputEvent::Paste("owner:\"".to_owned())));
            let lines = lines_at(&app, width, height);
            assert!(lines.is_some());
            if let Some(lines) = lines {
                let prompt = lines.len().saturating_sub(2);
                assert!(
                    lines
                        .get(prompt)
                        .is_some_and(|line| line.contains("/ owner"))
                );
                // The error explains the syntax on its own row, under the prompt.
                assert!(lines.last().is_some_and(|line| line.contains("column")));
                if width >= 80 {
                    assert!(lines.last().is_some_and(|line| line.contains("expected")));
                }
                if width >= 110 {
                    assert!(
                        lines
                            .last()
                            .is_some_and(|line| line.contains("showing last valid result"))
                    );
                }
                assert!(lines.iter().any(|line| line.contains("Filter")));
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
                assert!(lines.iter().any(|line| line.contains("Esc back")));
            }

            press(&mut app, KeyCode::Esc);
            let lines = lines_at(&app, width, height);
            assert!(lines.is_some());
            if let Some(lines) = lines {
                assert!(lines.iter().any(|line| line.contains("Actions")));
                assert!(lines.iter().any(|line| line.contains("Esc close")));
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
                // The yank menu is a grouped grid like the action and help menus.
                assert!(lines.iter().any(|line| line.contains("Copy")));
                assert!(lines.iter().any(|line| line.contains("Esc close")));
                assert!(lines.iter().any(|line| line.contains("Identity")));
                assert!(
                    lines
                        .last()
                        .is_some_and(|line| line.contains("copy immediately"))
                );
            }

            press(&mut app, KeyCode::Esc);
            press(&mut app, KeyCode::Char('?'));
            let lines = lines_at(&app, width, height);
            assert!(lines.is_some());
            if let Some(lines) = lines {
                let help_row = lines
                    .iter()
                    .position(|line| line.contains("Help") && line.contains("Esc close"));
                assert!(help_row.is_some());
                if let Some(help_row) = help_row {
                    assert!(help_row >= 2);
                }
                assert!(lines.iter().any(|line| line.contains("Navigation")));
                assert!(lines.iter().any(|line| line.contains("Current view")));
                if width >= 80 {
                    assert!(lines.iter().any(|line| line.contains(": command")));
                }
                assert!(!lines.iter().any(|line| line.contains("[disabled:")));
                assert!(!lines.iter().any(|line| line.contains("Simulation")));
                assert!(!lines.iter().any(|line| line.contains("service section")));
            }

            press(&mut app, KeyCode::Char('a'));
            let lines = lines_at(&app, width, height);
            assert!(lines.is_some());
            if let Some(lines) = lines {
                assert!(lines.iter().any(|line| line.contains("Actions")));
                assert!(lines.iter().any(|line| line.contains("Esc close")));
            }

            press(&mut app, KeyCode::Esc);
            press(&mut app, KeyCode::Char('?'));
            press(&mut app, KeyCode::Char('/'));
            let lines = lines_at(&app, width, height);
            assert!(lines.is_some());
            if let Some(lines) = lines {
                let prompt = lines.len().saturating_sub(2);
                assert!(lines.get(prompt).is_some_and(|line| line.contains("/ ")));
                assert!(
                    lines
                        .last()
                        .is_some_and(|line| line.contains("Enter apply"))
                );
            }
        }
    }
}

#[test]
fn quick_footer_separates_accent_keys_from_muted_help() {
    let app = populated_app();
    assert!(app.is_some());
    if let Some(app) = app {
        let backend = TestBackend::new(100, 30);
        let terminal = Terminal::new(backend).ok();
        assert!(terminal.is_some());
        if let Some(mut terminal) = terminal {
            let drawn = terminal.draw(|frame| ui::render(frame, &app));
            assert!(drawn.is_ok());
            let buffer = terminal.backend().buffer();
            let key = buffer.cell((0, 29));
            let label = buffer.cell((2, 29));
            assert!(key.is_some());
            assert!(label.is_some());
            if let (Some(key), Some(label)) = (key, label) {
                assert_eq!(key.symbol(), "k");
                assert_eq!(label.symbol(), "u");
                if let Some(expected) = app.theme.style(StyleRole::KeyHint).fg {
                    assert_eq!(key.fg, expected);
                }
                if let Some(expected) = app.theme.style(StyleRole::TextMuted).fg {
                    assert_eq!(label.fg, expected);
                }
                assert_ne!(key.modifier, label.modifier);
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
            // Freshness is stated once, in the header, and only when it slips.
            assert!(lines.iter().any(|line| line.contains("data stale")));
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
            // The route line describes the snapshot, not the fleet, and offers
            // the key that fixes it.
            assert!(lines.iter().any(|line| line.contains("data unavailable")));
            assert!(lines.iter().any(|line| line.contains("r to retry")));
            assert!(!lines.iter().any(|line| line.contains("source:")));
            assert!(
                lines
                    .iter()
                    .any(|line| line.contains("fictional source failure"))
            );
        }
    }

    let loading_profile = populated_app();
    assert!(loading_profile.is_some());
    if let Some(mut loading_profile) = loading_profile {
        loading_profile.admin.profile = Some("fictional".to_owned());
        loading_profile.admin.devices.begin(1);
        let lines = lines_at(&loading_profile, 80, 24);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert!(
                lines
                    .iter()
                    .any(|line| line.contains("loading profile data"))
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
        assert!(matches!(overlay.interaction, InteractionMode::HelpSheet));
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
        let lines = lines_at(&minimum, 55, 17);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert!(lines.iter().any(|line| line.contains("at least 60")));
            assert!(
                lines
                    .iter()
                    .any(|line| line.contains("Current terminal: 55x17"))
            );
        }
    }
}

#[test]
fn the_prompt_caret_is_the_real_terminal_cursor() {
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
            // The insertion point is the terminal's own cursor, so it blinks and
            // costs no cell. It sits just past the typed text.
            assert_eq!(
                terminal.get_cursor_position().ok(),
                Some((3, prompt_y).into())
            );
            let input_cell = terminal.backend().buffer().cell((2, prompt_y));
            let caret_cell = terminal.backend().buffer().cell((3, prompt_y));
            assert!(input_cell.is_some());
            assert!(caret_cell.is_some());
            if let (Some(input_cell), Some(caret_cell)) = (input_cell, caret_cell) {
                assert_eq!(input_cell.symbol(), "e");
                assert_eq!(caret_cell.symbol(), " ");
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
    activity.set_route(Route::Tasks);
    let first = activity
        .tasks
        .create(ActionId::MockSuccess, "first task", 1, false);
    let second = activity
        .tasks
        .create(ActionId::MockFailure, "second task", 1, false);
    activity.tasks.selected = Some(first);
    // The app header, then the panel border, then the table's own heading row,
    // and only then the first task.
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
                    let header_height =
                        ui::layout::compute(ratatui::layout::Rect::new(0, 0, width, height), &app)
                            .header
                            .height;
                    // The wordmark is art now, so check the header drew
                    // something rather than looking for a letter.
                    let mut has_header = false;
                    let mut has_non_reset = false;
                    for y in 0..height {
                        for x in 0..width {
                            if let Some(cell) = buffer.cell((x, y)) {
                                has_header |= y < header_height && cell.symbol().trim() != "";
                                has_non_reset |= cell.fg != ratatui::style::Color::Reset
                                    || cell.bg != ratatui::style::Color::Reset;
                                if capability == ColorCapability::None {
                                    assert_eq!(cell.fg, ratatui::style::Color::Reset);
                                    assert_eq!(cell.bg, ratatui::style::Color::Reset);
                                }
                            }
                        }
                    }
                    assert!(has_header);
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

/// The palette moved off a page of its own: appearance is reachable from
/// wherever the user already is, and shows its swatches in the menu.
#[test]
fn appearance_is_a_choice_menu_reachable_from_any_page() {
    let app = populated_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.set_route(Route::Devices);
        // Appearance is a bottom choice menu, not a centered overlay.
        let effects = app.dispatch_action(ActionId::SettingsAppearance);
        assert!(effects.is_empty());
        assert!(app.overlays.is_empty());
        let lines = lines_at(&app, 100, 32);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            assert!(lines.iter().any(|line| line.contains("Appearance")));
            assert!(lines.iter().any(|line| line.contains("Esc close")));
            assert!(lines.iter().any(|line| line.contains("terminal")));
        }
    }
}

#[test]
fn a_long_remedy_wraps_instead_of_being_cut_off() {
    let app = populated_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.runtime_error = Some(
            "the tailscale command was not found. Looked in /usr/bin/tailscale, \
             /nonexistent/tailscale and 1 more. Install Tailscale or pass --tailscale-path."
                .to_owned(),
        );
        let area = ratatui::layout::Rect::new(0, 0, 100, 30);
        let layout = tale::ui::layout::compute(area, &app);
        assert_eq!(
            layout.notification.height, 2,
            "a long message needs two rows"
        );

        let lines = lines_at(&app, area.width, area.height);
        assert!(lines.is_some());
        if let Some(lines) = lines {
            let notification = lines
                .iter()
                .skip(usize::from(layout.notification.y))
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            // The whole remedy survives, including the flag that fixes it.
            assert!(notification.contains("was not found"));
            assert!(notification.contains("/usr/bin/tailscale"));
            assert!(notification.contains("--tailscale-path"));
        }

        // A short message still costs a single row.
        app.runtime_error = Some("no device selected".to_owned());
        let layout = tale::ui::layout::compute(area, &app);
        assert_eq!(layout.notification.height, 1);
    }
}

#[test]
fn the_header_is_a_spaced_block_that_hides_what_does_not_matter() {
    let app = populated_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let area = ratatui::layout::Rect::new(0, 0, 120, 32);
        let layout = tale::ui::layout::compute(area, &app);
        let title = include_str!("../src/ui/tale-header-title.txt").trim_end();
        assert_eq!(
            usize::from(layout.header.height),
            title.lines().count().saturating_add(2),
            "a tall terminal fits the file-backed title and its outer spacing"
        );

        let backend = TestBackend::new(area.width, area.height);
        let terminal = Terminal::new(backend).ok();
        assert!(terminal.is_some());
        if let Some(mut terminal) = terminal {
            assert!(terminal.draw(|frame| ui::render(frame, &app)).is_ok());
            let buffer = terminal.backend().buffer();
            let mut rendered = Vec::new();
            for y in 0..area.height {
                let mut line = String::new();
                for x in 0..area.width {
                    if let Some(cell) = buffer.cell((x, y)) {
                        line.push_str(cell.symbol());
                    }
                }
                rendered.push(line);
            }
            let text = rendered.join("\n");
            for (row, title_line) in title.lines().enumerate() {
                assert!(
                    rendered
                        .get(row.saturating_add(1))
                        .is_some_and(|line| line.starts_with(title_line)),
                    "title row {} did not come from tale-header-title.txt",
                    row.saturating_add(1)
                );
            }
            // The status chip is a reversed run, not plain text.
            let chip = rendered
                .iter()
                .position(|line| line.contains("Status:"))
                .and_then(|y| {
                    let column = rendered.get(y)?.find("Status:")? + 9;
                    buffer.cell((u16::try_from(column).ok()?, u16::try_from(y).ok()?))
                });
            assert!(
                chip.is_some_and(|cell| cell.modifier.contains(ratatui::style::Modifier::REVERSED))
            );

            // Nothing repeats the route name or the freshness any more.
            assert!(!text.contains("refreshed"));
            assert!(!text.contains("updated"));
            assert!(text.contains("devices · 14"));
            assert_eq!(text.matches("devices").count(), 1);
        }

        // A short terminal collapses the header instead of eating the content.
        let short = ratatui::layout::Rect::new(0, 0, 120, 24);
        assert_eq!(tale::ui::layout::compute(short, &app).header.height, 1);

        // Task state stays hidden until a task needs attention.
        let _ = app.update(Event::Source(SourceEvent::LoadFailed {
            generation: 2,
            detail: "fictional".to_owned(),
        }));
        if let Some(lines) = lines_at(&app, 120, 32) {
            assert!(lines.iter().any(|line| line.contains("data unavailable")));
        }
    }
}

/// An overlay is a panel above the screen, not a hole through it. `Clear` only
/// resets cells to the terminal default, so a renderer whose base style carries
/// a foreground and no background used to leave the view showing through.
#[test]
fn every_overlay_paints_a_surface_rather_than_showing_the_view_through_it() {
    let Some(mut app) = populated_app() else {
        return;
    };
    let Some(raised) = app.theme.style(StyleRole::SurfaceRaised).bg else {
        return;
    };
    let Some(backdrop) = app.theme.style(StyleRole::Backdrop).bg else {
        return;
    };
    assert_ne!(
        raised, backdrop,
        "a panel cannot be told from its backdrop in this theme"
    );

    let confirmation = tale::app::Overlay::Confirmation(Box::new(tale::app::ConfirmationState {
        action_id: ActionId::ServicesFunnelUnpublish,
        mutation: None,
        admin_mutation: None,
        admin_batch: None,
        service_request: None,
        operational_mutation: None,
        handoff: None,
        prompt: "This mapping stops being reachable from the public internet.".to_owned(),
        required_phrase: Some("UNPUBLISH".to_owned()),
        input: String::new(),
        lose_ssh_checked: false,
        preview_lines: vec!["Keep https:8443/ serving 3001.".to_owned()],
        redacted_argv: vec!["serve".to_owned(), "--bg".to_owned()],
        error: None,
    }));

    for overlay in [confirmation, tale::app::Overlay::QuitConfirmation] {
        app.overlays.clear();
        app.overlays.push(overlay);
        let (width, height) = (80_u16, 24_u16);
        let backend = TestBackend::new(width, height);
        let mut terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(_) => return,
        };
        assert!(terminal.draw(|frame| ui::render(frame, &app)).is_ok());
        let buffer = terminal.backend().buffer();

        // The middle of the screen is inside a two-thirds panel; the top-left
        // corner is outside every one of them.
        let inside = buffer.cell((width / 2, height / 2));
        let outside = buffer.cell((0, 0));
        assert!(inside.is_some());
        assert!(outside.is_some());
        if let (Some(inside), Some(outside)) = (inside, outside) {
            assert_eq!(
                inside.bg, raised,
                "the panel interior is not painted with the raised surface"
            );
            assert_eq!(outside.bg, backdrop, "the backdrop stopped being painted");
            assert_ne!(
                inside.bg, outside.bg,
                "the panel is indistinguishable from what is behind it"
            );
        }
    }
}

#[test]
fn mock_resource_routes_use_the_shared_visual_grammar() {
    let Some(mut app) = mock_app() else {
        return;
    };
    for (route, expected) in [
        (Route::Local, ["Client", "Identity", "Preferences"]),
        (Route::Routes, ["DEVICE", "subnet-gateway", "APPROVED"]),
        (Route::Dns, ["This machine", "Tailnet", "100.100.100.100"]),
        (
            Route::Access,
            ["Policy source", "Fictional mock policy", "hash"],
        ),
        (
            Route::Credentials,
            ["DESCRIPTION", "CI deployment", "EXPIRES"],
        ),
        (Route::Audit, ["ACTION", "Approved route", "Flow Logs"]),
    ] {
        app.set_route(route);
        let Some(lines) = lines_at(&app, 120, 32) else {
            return;
        };
        let rendered = lines.join("\n");
        for marker in expected {
            assert!(
                rendered.contains(marker),
                "{route:?} did not render {marker} in mock mode"
            );
        }
        assert!(!rendered.contains("state: idle"), "{route:?} leaked state");
        assert!(
            !rendered.contains("not returned"),
            "{route:?} rendered sentinel data"
        );
    }

    for route in [Route::Routes, Route::Credentials, Route::Audit] {
        app.set_route(route);
        press(&mut app, KeyCode::Enter);
        let Some(lines) = lines_at(&app, 100, 30) else {
            return;
        };
        assert!(
            lines.iter().any(|line| line.contains("inspector")),
            "{route:?} did not open its row detail"
        );
        press(&mut app, KeyCode::Char('h'));
    }
}
