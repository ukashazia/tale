use std::fs;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tale::app::{App, Route};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::policy::PolicySnapshot;
use tale::paths::{PathEnvironment, Platform};

fn admin_app() -> Option<App> {
    let root = std::env::temp_dir().join(format!("tale-admin-ui-{}", std::process::id()));
    let _ = fs::create_dir_all(&root);
    let config_path = root.join("config.toml");
    let write = fs::write(
        &config_path,
        "default_profile = \"audit\"\n[profiles.audit]\ntailnet = \"example.test\"\ncredential = \"audit\"\n",
    );
    if write.is_err() {
        return None;
    }
    let cli = Cli {
        command: None,
        profile: None,
        config: Some(config_path),
        view: None,
        read_only: true,
        no_local: true,
        tailscale_path: None,
        mock: false,
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
    let resolved = config::resolve(&cli, &environment, &paths).ok()?;
    let mut app = App::new(resolved);
    app.admin.devices.begin(1);
    app.admin.devices.succeed(1, Vec::new(), 1_785_751_200);
    app.admin.policy.begin(1);
    app.admin.policy.succeed(
        1,
        PolicySnapshot {
            source_bytes: b"{\n  // fictional policy\n}\n".to_vec(),
            content_type: "application/hujson".to_owned(),
            fetched_at: 1_785_751_200,
            content_hash: "fictional-hash".to_owned(),
            etag: None,
        },
        1_785_751_200,
    );
    Some(app)
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

#[test]
fn admin_views_render_partial_and_read_only_states_at_required_sizes() {
    let app = admin_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        for route in [
            Route::Overview,
            Route::Users,
            Route::Routes,
            Route::Dns,
            Route::Access,
            Route::Credentials,
            Route::Activity,
            Route::Settings,
        ] {
            app.route_stack = vec![route];
            for (width, height) in [(60, 18), (80, 24), (110, 30), (160, 45)] {
                let lines = render_lines(&app, width, height);
                assert!(lines.is_some());
                if let Some(lines) = lines {
                    assert!(lines.iter().all(|line| !line.contains('\n')));
                    assert!(lines.iter().any(|line| line.contains("Tale")));
                }
            }
        }
    }
}
