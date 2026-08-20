use std::fs;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use tale::action::ActionId;
use tale::app::{App, Focus, Route};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::event::{Event, InputEvent};
use tale::paths::{PathEnvironment, Platform};
use tale::task::{Progress, TaskChange};

/// `:tasks` is a table like `:devices`, not a run of preformatted sentences:
/// headings, one row per task, and a state glyph rather than a `*` marker.
#[test]
fn tasks_render_as_a_table_with_headings_and_no_row_marker() {
    let app = tasks_app();
    // The shared fixture: if it stops building, every test here goes quiet.
    assert!(app.is_some());
    let Some(app) = app else {
        return;
    };
    let Some(lines) = render_lines(&app, 160, 30) else {
        return;
    };
    let heading = lines.iter().find(|line| line.contains(" ACTION "));
    assert!(heading.is_some(), "the tasks table has no heading row");
    if let Some(heading) = heading {
        for column in ["S", "#", "ACTION", "TARGET", "STATE", "STARTED", "RESULT"] {
            assert!(
                heading.contains(column),
                "the heading row is missing {column}: {heading}"
            );
        }
    }
    assert!(
        lines
            .iter()
            .any(|line| line.contains("admin.device.rename")),
        "no rows were drawn"
    );
    // The highlight is the selection; a marker column would be a second answer
    // to the same question.
    assert!(
        !lines.iter().any(|line| line.contains("* succeeded")),
        "the table still draws the old marker-and-label row"
    );
    // Route context lives in the border, and a timestamp is an age.
    assert!(
        lines.iter().any(|line| line.contains("┌ tasks · 3 ")),
        "the border does not carry the route and its counts"
    );
}

/// The narrow terminal keeps the columns that identify a row and drops the ones
/// that merely enrich it, rather than truncating every column to nothing.
#[test]
fn narrow_terminals_keep_the_identifying_columns() {
    let Some(app) = tasks_app() else {
        return;
    };
    let Some(lines) = render_lines(&app, 80, 24) else {
        return;
    };
    let heading = lines
        .iter()
        .find(|line| line.contains(" ACTION "))
        .cloned()
        .unwrap_or_default();
    assert!(heading.contains("TARGET") && heading.contains("STATE"));
    assert!(
        !heading.contains("PROGRESS") && !heading.contains("RESULT"),
        "a wide column survived a narrow terminal: {heading}"
    );
}

/// `i` brings the side pane in and takes it away again; `Enter` replaces the
/// table with the same detail at full width, and `h` gives the table back.
#[test]
fn i_toggles_the_inspector_and_enter_opens_it_full_width() {
    let Some(mut app) = tasks_app() else {
        return;
    };
    let pane_drawn = |lines: &[String]| lines.iter().any(|line| line.contains("┌ inspector "));
    let table_drawn = |lines: &[String]| lines.iter().any(|line| line.contains("┌ tasks · "));

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

    press(&mut app, KeyCode::Char('h'));
    assert_eq!(app.focus, Focus::Collection);
    assert!(
        render_lines(&app, 160, 30)
            .as_deref()
            .is_some_and(table_drawn)
    );
}

/// The inspector is what the old right pane never was: this task and nothing
/// else, ending in the output the run actually produced.
#[test]
fn the_inspector_describes_one_task_and_shows_its_output() {
    let Some(mut app) = tasks_app() else {
        return;
    };
    press(&mut app, KeyCode::Char('G'));
    press(&mut app, KeyCode::Enter);
    // The responsive footer uses its second row at this width, so keep the
    // inspector's existing content budget while checking its final output row.
    let Some(lines) = render_lines(&app, 160, 31) else {
        return;
    };
    for expected in [
        "task-3",
        "Netcheck",
        "failed",
        "exit status",
        "command",
        "output",
        "could not reach the control plane",
    ] {
        assert!(
            lines.iter().any(|line| line.contains(expected)),
            "the inspector never says {expected}"
        );
    }
    // The audit half of the old page went to `:audit`; nothing here reports on
    // the tailnet's own log.
    assert!(
        lines.iter().all(|line| !line.contains("Webhooks")),
        "webhook inventory is still riding along with the task detail"
    );
}

/// A field the run never reported gets no row. `not returned` six times over
/// describes the client, not the task.
#[test]
fn the_inspector_omits_what_the_run_did_not_report() {
    let Some(mut app) = tasks_app() else {
        return;
    };
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Enter);
    let Some(lines) = render_lines(&app, 160, 30) else {
        return;
    };
    assert!(lines.iter().any(|line| line.contains("task-1")));
    assert!(
        lines.iter().all(|line| !line.contains("exit status")),
        "an admin mutation was given a process exit status"
    );
    assert!(
        lines.iter().all(|line| !line.contains("not returned")),
        "the inspector invented a value the run never produced"
    );
}

#[test]
fn the_inspector_shows_structured_before_and_after_values() {
    let Some(mut app) = tasks_app() else {
        return;
    };
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Enter);
    let Some(lines) = render_lines(&app, 100, 30) else {
        return;
    };
    assert!(lines.iter().any(|line| line.contains("changes")));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("machine name: alpha → laptop-0"))
    );
}

/// `/` narrows the history, and the border says so rather than leaving the
/// reader to wonder where the other rows went.
#[test]
fn the_filter_narrows_the_table_and_the_border_says_so() {
    let Some(mut app) = tasks_app() else {
        return;
    };
    press(&mut app, KeyCode::Char('/'));
    let _ = app.update(Event::Input(InputEvent::Paste("netcheck".to_owned())));
    press(&mut app, KeyCode::Enter);
    let Some(lines) = render_lines(&app, 160, 30) else {
        return;
    };
    assert!(
        lines.iter().any(|line| line.contains("tasks · 1 of 3 ")),
        "the border does not say how much of the history is showing"
    );
    assert!(
        lines
            .iter()
            .all(|line| !line.contains("admin.device.rename")),
        "a filtered-out task is still on screen"
    );
}

#[test]
fn task_inspector_wraps_long_output_instead_of_clipping_its_end() {
    let Some(mut app) = tasks_app() else {
        return;
    };
    let task_id = app.tasks.create(
        ActionId::LocalNetcheck,
        "long-output-device",
        app.now.saturating_sub(5),
        false,
    );
    let _ = app.tasks.start(task_id);
    let _ = app.tasks.fail(
        task_id,
        app.now,
        "long output",
        "beginning of a deliberately long diagnostic line with enough words to cross the pane boundary and preserve the final-remedy",
    );
    app.tasks.select_filtered_last("");
    app.focus = Focus::Inspector;

    let Some(lines) = render_lines(&app, 70, 30) else {
        return;
    };
    let beginning = lines
        .iter()
        .position(|line| line.contains("beginning of a deliberately long"));
    let ending = lines.iter().position(|line| line.contains("final-remedy"));
    assert!(
        beginning.is_some(),
        "the start of the long line was not rendered"
    );
    assert!(
        ending.is_some(),
        "the wrapped end of the long line was clipped"
    );
    assert_ne!(beginning, ending, "the regression value did not wrap");
}

/// `y` offers what anyone actually pastes into a bug report, and only what this
/// run produced.
#[test]
fn the_copy_menu_offers_the_selected_tasks_command_and_output() {
    let Some(mut app) = tasks_app() else {
        return;
    };
    press(&mut app, KeyCode::Char('G'));
    press(&mut app, KeyCode::Char('y'));
    let Some(lines) = render_lines(&app, 120, 30) else {
        return;
    };
    for entry in ["i id", "r result", "c command", "o output"] {
        assert!(
            lines.iter().any(|line| line.contains(entry)),
            "the copy menu is missing {entry}"
        );
    }

    press(&mut app, KeyCode::Char('c'));
    assert_eq!(app.copied_value.as_deref(), Some("tailscale netcheck"));

    // The admin mutation ran no command and printed nothing, so the menu says
    // so by not offering either.
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('y'));
    let Some(lines) = render_lines(&app, 120, 30) else {
        return;
    };
    assert!(lines.iter().any(|line| line.contains("i id")));
    assert!(
        lines
            .iter()
            .all(|line| !line.contains("c command") && !line.contains("o output"))
    );
}

/// An empty page is a dead end unless it says what would fill it.
#[test]
fn an_empty_history_explains_itself() {
    let Some(mut app) = build_app() else {
        return;
    };
    app.set_route(Route::Tasks);
    let Some(lines) = render_lines(&app, 160, 30) else {
        return;
    };
    assert!(lines.iter().any(|line| line.contains("No tasks yet")));
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

/// Three fictional tasks: one that finished cleanly, one still going, and one
/// that failed with output worth reading.
fn tasks_app() -> Option<App> {
    let mut app = build_app()?;
    app.set_route(Route::Tasks);
    let now = tale::mock::MOCK_NOW;
    app.now = now;

    let renamed = app
        .tasks
        .create(ActionId::AdminDeviceRename, "laptop-0", now - 900, false);
    app.tasks.start(renamed);
    app.tasks
        .succeed(renamed, now - 890, "renamed to laptop-0", "");
    app.tasks.set_changes(
        renamed,
        vec![TaskChange {
            field: "machine name".to_owned(),
            before: Some("alpha".to_owned()),
            after: Some("laptop-0".to_owned()),
        }],
    );

    let running = app
        .tasks
        .create(ActionId::AdminDeviceTagsReplace, "phone-1", now - 30, true);
    app.tasks.start(running);
    app.tasks.progress(
        running,
        Progress {
            completed: 2,
            total: 5,
        },
        "tagging phone-1",
    );

    let netcheck = app
        .tasks
        .create(ActionId::LocalNetcheck, "this machine", now - 120, true);
    app.tasks.start(netcheck);
    app.tasks.set_local_metadata(
        netcheck,
        vec!["derp".to_owned(), "latency".to_owned()],
        vec!["tailscale".to_owned(), "netcheck".to_owned()],
    );
    app.tasks.set_exit_status(netcheck, Some(1));
    app.tasks.fail(
        netcheck,
        now - 100,
        "netcheck failed",
        "could not reach the control plane\nno DERP region answered",
    );

    app.tasks.select_filtered_first("");
    Some(app)
}

fn build_app() -> Option<App> {
    let root = std::env::temp_dir().join(format!("tale-ui-tasks-{}", std::process::id()));
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
