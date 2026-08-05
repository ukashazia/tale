use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::ui::theme;
use crate::ui::views::routes;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let executable = app.local_executable.as_ref();
    let snapshot = app.local_resource.snapshot.as_ref();
    let self_node = snapshot.map(|snapshot| &snapshot.self_node);
    let client_version = executable
        .map(|value| value.version.as_str())
        .or_else(|| snapshot.map(|value| value.client_version.as_str()))
        .map_or("not returned", |value| value);
    let daemon_version = executable
        .and_then(|value| value.daemon_version.as_deref())
        .or_else(|| snapshot.and_then(|value| value.daemon_version.as_deref()))
        .map_or("not returned", |value| value);
    let upper_height = area.height.saturating_mul(3) / 5;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(upper_height.max(12)), Constraint::Min(7)])
        .split(area);
    let mut lines = vec![
        Line::from("Local node · operator (read-only state display)"),
        Line::from(format!("state       {}", local_display_state(app))),
        Line::from(format!(
            "executable  {}",
            match executable {
                Some(value) => value.path.display().to_string(),
                None => "not returned".to_owned(),
            }
        )),
        Line::from(format!(
            "source      {}",
            executable
                .map(|value| value.source.label())
                .map_or("not returned", |value| value)
        )),
        Line::from(format!(
            "version     {} / daemon {}",
            client_version, daemon_version
        )),
        Line::from(format!(
            "node        {}",
            self_node
                .map(|value| value.display_name.as_str())
                .map_or("not returned", |value| value)
        )),
        Line::from(format!(
            "DNS name    {}",
            self_node
                .and_then(|value| value.dns_name.as_deref())
                .map_or("not returned", |value| value)
        )),
        Line::from(format!(
            "addresses   {}",
            match self_node {
                Some(value) => value.tailscale_ips.join(", "),
                None => "not returned".to_owned(),
            }
        )),
        Line::from(format!(
            "tailnet     {}",
            snapshot
                .and_then(|value| value.current_tailnet.as_deref())
                .map_or("not returned", |value| value)
        )),
        Line::from(format!(
            "observed    {} · {}",
            match snapshot {
                Some(value) => value.observed_at.to_string(),
                None => "not returned".to_owned(),
            },
            app.local_resource.status.label()
        )),
        Line::from(format!(
            "health      {}",
            snapshot
                .map(|value| value.health_messages.join("; "))
                .filter(|value| !value.is_empty())
                .map_or("not returned".to_owned(), |value| value)
        )),
        Line::from(if app.preferences_ready() {
            "Preference controls: available through preview and confirmation."
        } else {
            "No local preference controls are available until a verified preference read."
        }),
        Line::from(format!(
            "accounts   {}{}",
            app.local_accounts.len(),
            if app.local_accounts.iter().any(|account| account.active) {
                " · active profile returned"
            } else {
                " · active profile not returned"
            }
        )),
        Line::from(format!(
            "policy     {}",
            if app.system_policy_failure.is_some() {
                "error"
            } else if app.system_policy.is_empty() {
                "not returned"
            } else {
                "loaded"
            }
        )),
        Line::from("System Policy · effective local settings"),
        Line::from("Local system/MDM policy; not tailnet access policy."),
    ];
    if let Some(failure) = app.system_policy_failure.as_ref() {
        lines.push(Line::from(format!("policy error: {}", failure.detail)));
    } else if app.system_policy.is_empty() {
        lines.push(Line::from("policy entries: not returned"));
    } else {
        lines.extend(app.system_policy.iter().take(4).map(|entry| {
            Line::from(format!(
                "  {} · source={} · value={}{}",
                entry.name,
                entry
                    .source
                    .as_deref()
                    .map_or("not returned", |value| value),
                entry.value.as_deref().map_or("not returned", |value| value),
                entry
                    .error
                    .as_deref()
                    .map_or(String::new(), |value| format!(" · error={value}"))
            ))
        }));
    }
    lines.push(Line::from("Preferences · verified current values"));
    lines.extend([
        preference_line("accept DNS", &app.local_preferences.accept_dns),
        preference_line("accept routes", &app.local_preferences.accept_routes),
        preference_line("shields up", &app.local_preferences.shields_up),
        preference_line("Tailscale SSH", &app.local_preferences.ssh),
        preference_line("automatic update", &app.local_preferences.automatic_update),
        preference_line("update check", &app.local_preferences.update_check),
        preference_line("posture reporting", &app.local_preferences.report_posture),
        preference_line("hostname", &app.local_preferences.hostname),
        preference_line("nickname", &app.local_preferences.nickname),
        preference_line("web client", &app.local_preferences.web_client),
    ]);
    frame.render_widget(
        Paragraph::new(lines)
            .style(app.theme.style(theme::StyleRole::Surface))
            .block(Block::default().borders(Borders::ALL).title("local")),
        chunks[0],
    );
    routes::render(frame, app, chunks[1]);
}

fn local_display_state(app: &App) -> String {
    let freshness = match app.local_resource.status {
        crate::domain::source::LocalResourceStatus::Stale => " · stale",
        _ => "",
    };
    format!(
        "daemon {} · CLI {}{}",
        app.local_daemon_state.label(),
        app.local_cli_state.label(),
        freshness
    )
}

fn preference_line<T: std::fmt::Display>(
    label: &str,
    preference: &crate::domain::preference::ObservedPreference<T>,
) -> Line<'static> {
    let value = preference
        .value
        .as_ref()
        .map_or_else(|| "not returned".to_owned(), ToString::to_string);
    Line::from(format!(
        "  {label}: {value} · {}",
        preference.editability.label()
    ))
}
