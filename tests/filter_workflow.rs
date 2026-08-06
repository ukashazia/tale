use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use tale::app::{App, FilterSuggestionKind, InteractionMode, Route};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::event::{Event, InputEvent, SourceEvent};
use tale::mock;
use tale::paths::{PathEnvironment, Platform};
use tale::ui;
use tale::ui::theme::StyleRole;

fn app() -> Option<App> {
    let root = PathBuf::from("/fictional/tale-filter");
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
    let mut app = config::resolve(&cli, &environment, &paths)
        .ok()
        .map(App::new)?;
    app.set_route(Route::Devices);
    let _ = app.update(Event::Source(SourceEvent::LoadSucceeded {
        generation: 1,
        devices: mock::devices(),
        observed_at: mock::MOCK_NOW,
    }));
    Some(app)
}

fn press(app: &mut App, code: KeyCode) {
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        code,
        KeyModifiers::NONE,
    ))));
}

fn back_tab(app: &mut App) {
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::BackTab,
        KeyModifiers::SHIFT,
    ))));
}

fn type_text(app: &mut App, text: &str) {
    let _ = app.update(Event::Input(InputEvent::Paste(text.to_owned())));
}

fn input(app: &App) -> String {
    match &app.interaction {
        InteractionMode::FilterLine(state) => state.editor.input.clone(),
        _ => String::new(),
    }
}

fn labels(app: &App) -> Vec<String> {
    match &app.interaction {
        InteractionMode::FilterLine(state) => state
            .sections
            .iter()
            .map(|section| section.label.clone())
            .collect(),
        _ => Vec::new(),
    }
}

fn offered(app: &App) -> Vec<String> {
    match &app.interaction {
        InteractionMode::FilterLine(state) => state
            .suggestions()
            .map(|suggestion| suggestion.text.clone())
            .collect(),
        _ => Vec::new(),
    }
}

#[test]
fn opening_the_prompt_offers_every_field_grouped_with_guidance() {
    let app = app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        press(&mut app, KeyCode::Char('/'));
        assert_eq!(
            labels(&app),
            vec!["Machine", "Connection", "Administration"]
        );
        let names = offered(&app);
        assert_eq!(names.len(), 15);
        for expected in ["owner:", "online:", "last-seen:", "route-role:"] {
            assert!(names.contains(&expected.to_owned()), "missing {expected}");
        }
        if let InteractionMode::FilterLine(state) = &app.interaction {
            // Every offer names a field and explains it in a few words.
            for suggestion in state.suggestions() {
                assert_eq!(suggestion.kind, FilterSuggestionKind::Field);
                assert!(suggestion.text.ends_with(':'));
                assert!(!suggestion.note.is_empty());
            }
        }
    }
}

#[test]
fn matching_is_token_aware_and_fuzzy_over_fields_then_values() {
    let app = app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        press(&mut app, KeyCode::Char('/'));
        // A typo still finds the field it was reaching for.
        type_text(&mut app, "ownr");
        assert_eq!(labels(&app), vec!["Matches"]);
        assert_eq!(offered(&app), vec!["owner:".to_owned()]);

        // Past the separator the same token switches to snapshot values.
        type_text(&mut app, "");
        press(&mut app, KeyCode::Tab);
        assert_eq!(input(&app), "owner:");
        assert_eq!(labels(&app), vec!["owner values", "owner operators"]);
        assert!(offered(&app).contains(&"alice@example.com".to_owned()));

        // A fresh token goes back to completing fields, not values.
        type_text(&mut app, "alice@example.com onl");
        assert_eq!(labels(&app), vec!["Matches"]);
        assert!(offered(&app).contains(&"online:".to_owned()));
    }
}

#[test]
fn enumerations_and_durations_offer_their_own_vocabulary() {
    let app = app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "online:");
        assert_eq!(
            offered(&app),
            vec!["true".to_owned(), "false".to_owned(), "unknown".to_owned()]
        );

        type_text(&mut app, "");
        if let InteractionMode::FilterLine(state) = &mut app.interaction {
            state.editor.input.clear();
            state.editor.cursor = 0;
        }
        type_text(&mut app, "last-seen:");
        assert!(offered(&app).contains(&"<7d".to_owned()));

        // Free-text fields also expose their match modes.
        if let InteractionMode::FilterLine(state) = &mut app.interaction {
            state.editor.input.clear();
            state.editor.cursor = 0;
        }
        type_text(&mut app, "name:");
        assert_eq!(labels(&app), vec!["name values", "name operators"]);
        assert!(offered(&app).contains(&"starts_with=".to_owned()));
    }
}

#[test]
fn tab_takes_the_best_offer_and_shift_tab_walks_back() {
    let app = app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Tab);
        assert_eq!(input(&app), "id:");
        press(&mut app, KeyCode::Tab);
        assert_eq!(input(&app), "name:");
        back_tab(&mut app);
        assert_eq!(input(&app), "id:");
        // Walking back past the first offer wraps to the last one.
        back_tab(&mut app);
        assert_eq!(input(&app), "route-role:");

        // A single match is accepted outright and the tray moves on to values.
        if let InteractionMode::FilterLine(state) = &mut app.interaction {
            state.editor.input.clear();
            state.editor.cursor = 0;
            state.selected_completion = None;
        }
        type_text(&mut app, "ownr");
        press(&mut app, KeyCode::Tab);
        assert_eq!(input(&app), "owner:");
        press(&mut app, KeyCode::Tab);
        assert_eq!(input(&app), "owner:alice@example.com");
    }
}

#[test]
fn enter_applies_a_valid_query_and_an_invalid_one_keeps_the_last_result() {
    let app = app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "online:true");
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.interaction, InteractionMode::Normal));
        assert_eq!(app.views.devices.filter_draft, "online:true");
        let applied = app.views.devices.applied_filter.clone();

        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, " online:yes");
        // The error explains the syntax instead of dropping the previous result.
        if let InteractionMode::FilterLine(state) = &app.interaction {
            let error = state.error.as_ref();
            assert!(error.is_some());
            if let Some(error) = error {
                assert!(error.message.contains("yes is not a value of online"));
                assert_eq!(error.expected, "online:true|false|unknown");
            }
        }
        assert_eq!(app.views.devices.applied_filter, applied);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.interaction, InteractionMode::FilterLine(_)));
        assert_eq!(app.views.devices.applied_filter, applied);

        // Cancelling returns to the committed query, not to an empty one.
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.interaction, InteractionMode::Normal));
        assert_eq!(app.views.devices.filter_draft, "online:true");
        assert_eq!(app.views.devices.applied_filter, applied);
    }
}

#[test]
fn suggestions_follow_the_route_and_never_offer_another_views_fields() {
    let app = app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.set_route(Route::Activity);
        press(&mut app, KeyCode::Char('/'));
        assert!(matches!(app.interaction, InteractionMode::FilterLine(_)));
        assert!(
            offered(&app).is_empty(),
            "activity must not offer device fields"
        );

        app.set_route(Route::Devices);
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('/'));
        assert!(offered(&app).contains(&"path:".to_owned()));
    }
}

#[test]
fn the_prompt_colours_fields_operators_and_values_apart() {
    let app = app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "owner:alice");
        let backend = TestBackend::new(110, 30);
        let terminal = Terminal::new(backend).ok();
        assert!(terminal.is_some());
        if let Some(mut terminal) = terminal {
            let drawn = terminal.draw(|frame| ui::render(frame, &app));
            assert!(drawn.is_ok());
            let buffer = terminal.backend().buffer();
            let prompt_row = 28;
            // "/ owner:alice" — field name, separator, then the value.
            let field = buffer.cell((2, prompt_row));
            let operator = buffer.cell((7, prompt_row));
            let value = buffer.cell((8, prompt_row));
            assert!(field.is_some());
            assert!(operator.is_some());
            assert!(value.is_some());
            if let (Some(field), Some(operator), Some(value)) = (field, operator, value) {
                assert_eq!(field.symbol(), "o");
                assert_eq!(operator.symbol(), ":");
                assert_eq!(value.symbol(), "a");
                assert_eq!(Some(field.fg), app.theme.style(StyleRole::SyntaxField).fg);
                assert_eq!(
                    Some(operator.fg),
                    app.theme.style(StyleRole::SyntaxOperator).fg
                );
                assert_eq!(Some(value.fg), app.theme.style(StyleRole::SyntaxValue).fg);
                assert_ne!(field.fg, operator.fg);
                assert_ne!(operator.fg, value.fg);
                assert_ne!(field.fg, value.fg);
            }
        }
    }
}

#[test]
fn an_unknown_field_is_marked_in_the_prompt_before_it_is_committed() {
    let app = app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "nope:1");
        let backend = TestBackend::new(110, 30);
        let terminal = Terminal::new(backend).ok();
        assert!(terminal.is_some());
        if let Some(mut terminal) = terminal {
            let drawn = terminal.draw(|frame| ui::render(frame, &app));
            assert!(drawn.is_ok());
            let buffer = terminal.backend().buffer();
            let cell = buffer.cell((2, 28));
            assert!(cell.is_some());
            if let Some(cell) = cell {
                assert_eq!(cell.symbol(), "n");
                assert_eq!(Some(cell.fg), app.theme.style(StyleRole::StateDanger).fg);
            }
        }
    }
}

#[test]
fn negated_quoted_and_compared_terms_all_complete() {
    let app = app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        // Negation is punctuation on the token, not part of the field name.
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "online:true !tag:serv");
        assert!(offered(&app).contains(&"server".to_owned()));
        press(&mut app, KeyCode::Tab);
        assert_eq!(input(&app), "online:true !tag:server");

        // A half-typed quote still matches, and the completion re-quotes.
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "owner:\"ali");
        assert!(offered(&app).contains(&"alice@example.com".to_owned()));
        press(&mut app, KeyCode::Tab);
        assert_eq!(input(&app), "owner:alice@example.com");

        // Comparisons complete as whole operands.
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "last-seen:<");
        assert_eq!(labels(&app), vec!["last-seen values"]);
        press(&mut app, KeyCode::Tab);
        assert_eq!(input(&app), "last-seen:<1h");
        if let InteractionMode::FilterLine(state) = &app.interaction {
            assert!(state.error.is_none());
        }
    }
}

#[test]
fn text_with_no_match_reports_it_without_offering_a_wrong_field() {
    let app = app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "zzzz");
        assert!(offered(&app).is_empty());
        // Tab has nothing to take, so the text is left exactly as typed.
        press(&mut app, KeyCode::Tab);
        assert_eq!(input(&app), "zzzz");
    }
}

#[test]
fn the_insertion_point_is_a_real_cursor_that_tracks_the_prompt() {
    let app = app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let area = ratatui::layout::Rect::new(0, 0, 110, 30);
        let backend = TestBackend::new(area.width, area.height);
        let terminal = Terminal::new(backend).ok();
        assert!(terminal.is_some());
        if let Some(mut terminal) = terminal {
            // Nothing is being edited, so no frame asks for a cursor and the
            // backend is left at its starting position.
            assert!(terminal.draw(|frame| ui::render(frame, &app)).is_ok());
            assert_eq!(terminal.get_cursor_position().ok(), Some((0, 0).into()));

            press(&mut app, KeyCode::Char('/'));
            type_text(&mut app, "os:linux");
            let layout = tale::ui::layout::compute(area, &app);
            let prompt_y = layout
                .footer
                .y
                .saturating_add(layout.footer.height.saturating_sub(2));
            assert!(terminal.draw(|frame| ui::render(frame, &app)).is_ok());
            // "/ os:linux" is 10 cells, so the cursor rests on the eleventh.
            assert_eq!(
                terminal.get_cursor_position().ok(),
                Some((10, prompt_y).into())
            );
            assert!(
                terminal
                    .backend()
                    .buffer()
                    .cell((10, prompt_y))
                    .is_some_and(|cell| cell.symbol() == " ")
            );

            // Moving left inside the text moves the cursor with it.
            press(&mut app, KeyCode::Left);
            press(&mut app, KeyCode::Left);
            assert!(terminal.draw(|frame| ui::render(frame, &app)).is_ok());
            assert_eq!(
                terminal.get_cursor_position().ok(),
                Some((8, prompt_y).into())
            );

            // Leaving the editor stops asking for a cursor, which is what makes
            // ratatui hide it; the position is simply never moved again.
            press(&mut app, KeyCode::Esc);
            assert!(terminal.draw(|frame| ui::render(frame, &app)).is_ok());
            assert_eq!(
                terminal.get_cursor_position().ok(),
                Some((8, prompt_y).into())
            );
        }
    }
}
