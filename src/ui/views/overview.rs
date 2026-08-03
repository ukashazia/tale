use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::domain::device::ConnectionPath;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.source_mode == crate::app::SourceMode::Local {
        render_local(frame, app, area);
        return;
    }
    let online = app
        .devices_resource
        .snapshot
        .iter()
        .filter(|device| device.liveness == crate::domain::device::Liveness::Online)
        .count();
    let offline = app
        .devices_resource
        .snapshot
        .iter()
        .filter(|device| device.liveness == crate::domain::device::Liveness::Offline)
        .count();
    let lines = vec![
        Line::from("Overview"),
        Line::from(format!("source       {}", app.source_mode.label())),
        Line::from(format!(
            "devices      {} total · {} online · {} offline",
            app.devices_resource.snapshot.len(),
            online,
            offline
        )),
        Line::from(format!(
            "source state  {}",
            app.devices_resource.health.label()
        )),
        Line::from(format!(
            "tasks        {} active · {} total",
            app.tasks.active().count(),
            app.tasks.all().len()
        )),
        Line::from(if app.source_mode == crate::app::SourceMode::Mock {
            "mock data is deterministic and offline"
        } else {
            "local integration is unavailable in this build"
        }),
        Line::from("Use : to navigate, / to filter Devices, ? for help."),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::normal(app))
            .block(Block::default().borders(Borders::ALL).title("overview")),
        area,
    );
}

fn render_local(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let snapshot = app.local_resource.snapshot.as_ref();
    let peers = snapshot.map_or(&[][..], |value| value.peers.as_slice());
    let online = peers
        .iter()
        .filter(|device| device.online == Some(true))
        .count();
    let offline = peers
        .iter()
        .filter(|device| device.online == Some(false))
        .count();
    let unknown = peers
        .iter()
        .filter(|device| device.online.is_none())
        .count();
    let active = peers.iter().filter(|device| device.active).count();
    let direct = peers
        .iter()
        .filter(|device| matches!(device.path, ConnectionPath::Direct { .. }))
        .count();
    let derp = peers
        .iter()
        .filter(|device| matches!(device.path, ConnectionPath::Derp { .. }))
        .count();
    let relay = peers
        .iter()
        .filter(|device| matches!(device.path, ConnectionPath::PeerRelay { .. }))
        .count();
    let state = match app.local_resource.status {
        crate::domain::source::LocalResourceStatus::Loading => "discovering".to_owned(),
        crate::domain::source::LocalResourceStatus::Stale => "stale".to_owned(),
        _ => app.local_state.label().to_owned(),
    };
    let version_mismatch = app
        .local_executable
        .as_ref()
        .and_then(|value| value.daemon_version.as_deref())
        .is_some_and(|daemon| {
            app.local_executable
                .as_ref()
                .is_some_and(|value| value.version != daemon)
        });
    let self_name = snapshot
        .map(|value| value.self_node.display_name.as_str())
        .map_or("not returned", |value| value);
    let tailnet = snapshot
        .and_then(|value| value.current_tailnet.as_deref())
        .map_or("not returned", |value| value);
    let addresses = match snapshot {
        Some(value) => value.self_node.tailscale_ips.join(", "),
        None => "not returned".to_owned(),
    };
    let health = snapshot
        .map(|value| value.health_messages.join("; "))
        .filter(|value| !value.is_empty())
        .map_or("none".to_owned(), |value| value);
    let freshness =
        if app.local_resource.status == crate::domain::source::LocalResourceStatus::Stale {
            " (stale)"
        } else {
            ""
        };
    let last_good = app.local_resource.last_success_at.map_or_else(
        || "not returned".to_owned(),
        |value| format!("{}s ago", app.now.saturating_sub(value)),
    );
    let lines = vec![
        Line::from("Overview · local source"),
        Line::from(format!(
            "local       {} · {}",
            state,
            app.local_resource.status.label()
        )),
        Line::from(format!("node        {} · {}", self_name, tailnet)),
        Line::from(format!("addresses   {addresses}")),
        Line::from(format!(
            "version     {}",
            if version_mismatch {
                "CLI/daemon mismatch"
            } else {
                "matched or not returned"
            }
        )),
        Line::from(format!(
            "peers       {} total · {} online · {} offline · {} unknown{}",
            peers.len(),
            online,
            offline,
            unknown,
            freshness
        )),
        Line::from(format!(
            "paths       {} active · {} direct · {} DERP · {} peer relay{}",
            active, direct, derp, relay, freshness
        )),
        Line::from(format!("health      {health}")),
        Line::from(format!("last good   {last_good}")),
        Line::from(format!("tasks       {} active", app.tasks.active().count())),
        Line::from("Use :local, :devices, :dns, or a → local diagnostics."),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::normal(app))
            .block(Block::default().borders(Borders::ALL).title("overview")),
        area,
    );
}
