use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::ui::theme;

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
    let lines = vec![
        Line::from("Local node · read-only"),
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
        Line::from("No local preference controls are available in Phase 2."),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::normal(app))
            .block(Block::default().borders(Borders::ALL).title("local")),
        area,
    );
}

fn local_display_state(app: &App) -> String {
    match app.local_resource.status {
        crate::domain::source::LocalResourceStatus::Loading => "discovering".to_owned(),
        crate::domain::source::LocalResourceStatus::Stale => "stale".to_owned(),
        _ => app.local_state.label().to_owned(),
    }
}
