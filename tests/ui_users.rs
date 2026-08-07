use std::fs;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use tale::app::{App, Focus, Route};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::user::AdminUser;
use tale::event::{Event, InputEvent};
use tale::paths::{PathEnvironment, Platform};

/// `:users` is a table like `:devices`, not a run of preformatted sentences:
/// headings, one row per user, and a state glyph rather than a `>` marker.
#[test]
fn users_render_as_a_table_with_headings_and_no_row_marker() {
    let app = users_app();
    // The shared fixture: if it stops building, every test here goes quiet.
    assert!(app.is_some());
    let Some(app) = app else {
        return;
    };
    let Some(lines) = render_lines(&app, 160, 30) else {
        return;
    };
    let heading = lines.iter().find(|line| line.contains(" NAME "));
    assert!(heading.is_some(), "the users table has no heading row");
    if let Some(heading) = heading {
        for column in ["S", "NAME", "LOGIN", "ROLE", "STATUS", "DEVICES", "SEEN"] {
            assert!(
                heading.contains(column),
                "the heading row is missing {column}: {heading}"
            );
        }
    }
    assert!(
        lines.iter().any(|line| line.contains("Fictional Person 1")),
        "no rows were drawn"
    );
    // The highlight is the selection; a marker column would be a second answer
    // to the same question.
    assert!(
        !lines.iter().any(|line| line.contains("> Fictional")),
        "the table still draws a row marker"
    );
    // Route context lives in the border, and a timestamp is an age.
    assert!(lines.iter().any(|line| line.contains("┌ users · 3 ")));
    assert!(
        lines
            .iter()
            .all(|line| !line.contains(&tale::mock::MOCK_NOW.to_string())),
        "a raw timestamp reached the screen"
    );
}

/// The narrow terminal keeps the columns that identify a row and drops the ones
/// that merely enrich it, rather than truncating every column to nothing.
#[test]
fn narrow_terminals_keep_the_identifying_columns() {
    let Some(app) = users_app() else {
        return;
    };
    let Some(lines) = render_lines(&app, 80, 24) else {
        return;
    };
    let heading = lines
        .iter()
        .find(|line| line.contains(" NAME "))
        .cloned()
        .unwrap_or_default();
    assert!(heading.contains("ROLE") && heading.contains("STATUS"));
    assert!(
        !heading.contains("LOGIN") && !heading.contains(" ID"),
        "a wide column survived a narrow terminal: {heading}"
    );
}

/// `i` brings the side pane in and takes it away again; `Enter` replaces the
/// table with the same detail at full width, and `Esc` gives the table back.
#[test]
fn i_toggles_the_inspector_and_enter_opens_it_full_width() {
    let Some(mut app) = users_app() else {
        return;
    };
    let pane_drawn = |lines: &[String]| lines.iter().any(|line| line.contains("┌ inspector "));
    let table_drawn = |lines: &[String]| lines.iter().any(|line| line.contains("┌ users · "));

    let hidden = render_lines(&app, 160, 30);
    assert!(
        hidden.as_deref().is_some_and(|lines| !pane_drawn(lines)),
        "the inspector pane opened uninvited"
    );

    press(&mut app, KeyCode::Char('i'));
    assert_eq!(app.focus, Focus::Collection, "the table lost the keys");
    let shown = render_lines(&app, 160, 30);
    assert!(shown.as_deref().is_some_and(pane_drawn));
    assert!(
        shown.as_deref().is_some_and(table_drawn),
        "the side pane replaced the table instead of sharing with it"
    );

    press(&mut app, KeyCode::Char('i'));
    assert!(
        render_lines(&app, 160, 30)
            .as_deref()
            .is_some_and(|lines| !pane_drawn(lines))
    );

    press(&mut app, KeyCode::Enter);
    assert_eq!(app.focus, Focus::Inspector);
    let opened = render_lines(&app, 160, 30);
    assert!(opened.as_deref().is_some_and(pane_drawn));
    assert!(
        opened.as_deref().is_some_and(|lines| !table_drawn(lines)),
        "Enter left the table on screen"
    );
    // What the detail pane says about the row, in words rather than in fields
    // the API happens to name.
    if let Some(opened) = opened {
        assert!(opened.iter().any(|line| line.contains("person0@example")));
        assert!(opened.iter().any(|line| line.contains("connected")));
    }

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.focus, Focus::Collection);
    assert!(
        render_lines(&app, 160, 30)
            .as_deref()
            .is_some_and(table_drawn)
    );
}

/// A user the API did not describe fully is described by what it did send. A
/// row of `not returned` is a fact about the client, not about the person.
#[test]
fn the_inspector_omits_fields_the_api_did_not_send() {
    let Some(mut app) = users_app() else {
        return;
    };
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Enter);
    let Some(lines) = render_lines(&app, 160, 30) else {
        return;
    };
    assert!(lines.iter().any(|line| line.contains("┌ inspector ")));
    assert!(lines.iter().any(|line| line.contains("user-fictional-002")));
    assert!(
        lines.iter().all(|line| !line.contains("not returned")),
        "the inspector invented a value for a field the API did not send"
    );
    assert!(
        lines.iter().all(|line| !line.contains("last seen")),
        "a user with no last-seen time still got a row for it"
    );
}

/// `y` offers the identifiers worth pasting, and only the ones this user has.
#[test]
fn the_copy_menu_offers_the_selected_users_identifiers() {
    let Some(mut app) = users_app() else {
        return;
    };
    press(&mut app, KeyCode::Char('y'));
    let Some(lines) = render_lines(&app, 120, 30) else {
        return;
    };
    assert!(lines.iter().any(|line| line.contains("i id")));
    assert!(lines.iter().any(|line| line.contains("n name")));
    assert!(lines.iter().any(|line| line.contains("l login")));

    press(&mut app, KeyCode::Char('i'));
    assert_eq!(app.copied_value.as_deref(), Some("user-fictional-000"));

    press(&mut app, KeyCode::Char('y'));
    press(&mut app, KeyCode::Char('l'));
    assert_eq!(app.copied_value.as_deref(), Some("person0@example.test"));

    // The third user has neither a display name nor a login, so the menu says
    // so by not offering them.
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('y'));
    let Some(lines) = render_lines(&app, 120, 30) else {
        return;
    };
    assert!(lines.iter().any(|line| line.contains("i id")));
    assert!(lines.iter().all(|line| !line.contains("l login")));
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

/// Three fictional users: one fully described, one partly, one that is little
/// more than an identifier.
fn users() -> Vec<AdminUser> {
    vec![
        AdminUser {
            id: "user-fictional-000".to_owned(),
            display_name: Some("Fictional Person 0".to_owned()),
            login_name: Some("person0@example.test".to_owned()),
            tailnet_id: Some("example.test".to_owned()),
            created_at: Some(tale::mock::MOCK_NOW - 259_200),
            relation_type: Some("member".to_owned()),
            role: Some("owner".to_owned()),
            status: Some("active".to_owned()),
            device_count: Some(3),
            last_seen: Some(tale::mock::MOCK_NOW - 600),
            currently_connected: Some(true),
        },
        AdminUser {
            id: "user-fictional-001".to_owned(),
            display_name: Some("Fictional Person 1".to_owned()),
            login_name: Some("person1@example.test".to_owned()),
            tailnet_id: Some("example.test".to_owned()),
            created_at: Some(tale::mock::MOCK_NOW - 345_600),
            relation_type: Some("member".to_owned()),
            role: Some("member".to_owned()),
            status: Some("suspended".to_owned()),
            device_count: Some(0),
            last_seen: Some(tale::mock::MOCK_NOW - 86_400),
            currently_connected: Some(false),
        },
        AdminUser {
            id: "user-fictional-002".to_owned(),
            display_name: None,
            login_name: None,
            tailnet_id: None,
            created_at: None,
            relation_type: None,
            role: None,
            status: None,
            device_count: None,
            last_seen: None,
            currently_connected: None,
        },
    ]
}

fn users_app() -> Option<App> {
    let mut app = build_app()?;
    app.set_route(Route::Users);
    app.admin.users.begin(1);
    app.admin.users.succeed(1, users(), tale::mock::MOCK_NOW);
    app.now = tale::mock::MOCK_NOW;
    Some(app)
}

fn build_app() -> Option<App> {
    let root = std::env::temp_dir().join(format!("tale-ui-users-{}", std::process::id()));
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
        tailscale_path: None,
        tailscale_socket: None,
        // A copy lands in state instead of on the system clipboard.
        mock: true,
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
