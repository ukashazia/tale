use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use tale::app::{App, Route, SourceMode};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::event::{Event, InputEvent, LocalEvent};
use tale::local::daemon::decode_status;
use tale::paths::{PathEnvironment, Platform};

const STATUS: &str = include_str!("fixtures/tailscale/1.98.9/linux/status.json");

#[test]
fn local_overview_and_local_view_show_snapshot_without_blocking_navigation() {
    let app = local_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let snapshot = decode_status(
            STATUS,
            "1.98.9".to_owned(),
            Some("1.98.9".to_owned()),
            1_754_000_000,
        );
        assert!(snapshot.is_ok());
        if let Ok(snapshot) = snapshot {
            app.local_resource.generation = 1;
            let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
                generation: 1,
                snapshot: Box::new(snapshot),
            })));
        }
        assert_eq!(app.source_mode, SourceMode::Local);
        app.set_route(Route::Overview);
        let overview = render_lines(&app, 120, 30);
        assert!(overview.is_some());
        if let Some(overview) = overview {
            assert!(overview.iter().any(|line| line.contains("observer")));
            assert!(overview.iter().any(|line| line.contains("Example Tailnet")));
            assert!(overview.iter().any(|line| line.contains("direct")));
            assert!(overview.iter().any(|line| line.contains("health")));
        }
        app.set_route(Route::Local);
        let local = render_lines(&app, 100, 30);
        assert!(local.is_some());
        if let Some(local) = local {
            assert!(local.iter().any(|line| line.contains("read-only")));
            assert!(local.iter().any(|line| line.contains("1.98.9")));
            // The tall header costs this dense view its last row at 30 lines,
            // so assert the data rather than the trailing hint.
            assert!(local.iter().any(|line| line.contains("tailnet")));
            assert!(local.iter().any(|line| line.contains("addresses")));
        }
        app.local_resource.mark_stale();
        app.set_route(Route::Overview);
        let stale = render_lines(&app, 120, 30);
        assert!(stale.is_some());
        if let Some(stale) = stale {
            assert!(stale.iter().any(|line| line.contains("stale")));
            assert!(stale.iter().any(|line| line.contains("observer")));
        }
    }
}

#[test]
fn slash_searches_the_local_detail_document() {
    let Some(mut app) = local_app() else {
        return;
    };
    app.set_route(Route::Local);
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('/'),
        KeyModifiers::NONE,
    ))));
    assert!(matches!(
        app.interaction,
        tale::app::InteractionMode::FilterLine(tale::app::FilterLineState {
            purpose: tale::app::FilterLinePurpose::DetailSearch {
                route: Route::Local,
                ..
            },
            ..
        })
    ));
    let _ = app.update(Event::Input(InputEvent::Paste("Identity".to_owned())));
    assert_eq!(app.detail_search, "Identity");
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
    let root = std::env::temp_dir().join(format!("tale-ui-local-{}", std::process::id()));
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
