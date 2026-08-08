use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use crate::app::App;
use crate::domain::device::ConnectionPath;
use crate::ui::components::panel;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.admin.profile.is_some() {
        render_combined(frame, app, area);
        return;
    }
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
    let unknown = app
        .devices_resource
        .snapshot
        .len()
        .saturating_sub(online)
        .saturating_sub(offline);
    // Connection state, not source plumbing: the header already says where the
    // data came from and how current it is.
    let mut lines = vec![Line::from(format!(
        "devices      {} total · {online} online · {offline} offline{}",
        app.devices_resource.snapshot.len(),
        if unknown > 0 {
            format!(" · {unknown} unknown")
        } else {
            String::new()
        }
    ))];
    let running = app.tasks.active().count();
    if running > 0 {
        lines.push(Line::from(format!("tasks        {running} running")));
    }
    append_health(&mut lines, app);
    panel::render(frame, app, area, "overview", lines);
}

fn render_combined(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let local_devices = app
        .local_resource
        .snapshot
        .as_ref()
        .map_or(0, |snapshot| snapshot.peers.len().saturating_add(1));
    let admin_devices = app.admin.devices.snapshot.as_ref().map_or(0, Vec::len);
    let queues = app.admin.overview_queues(app.now);
    let queue_lines = [
        (
            "awaiting device approval",
            queues.devices_awaiting_approval.len(),
        ),
        (
            "awaiting user approval",
            queues.users_awaiting_approval.len(),
        ),
        ("expired device keys", queues.expired_device_keys.len()),
        ("soon-expiring keys", queues.soon_expiring_device_keys.len()),
        (
            "advertised routes not approved",
            queues.unapproved_routes.len(),
        ),
    ];
    let mut lines = vec![
        Line::from("Overview · Local + Admin"),
        Line::from(format!(
            "profile      {} · tailnet {} · {}",
            app.admin.profile.as_deref().map_or("none", |value| value),
            app.admin
                .tailnet
                .as_deref()
                .map_or("unknown", |value| value),
            if app.admin.profile_read_only {
                "read-only"
            } else {
                "profile lock open"
            }
        )),
        Line::from(format!(
            "local        daemon {} · CLI {} · {} devices · {}",
            app.local_daemon_state.label(),
            app.local_cli_state.label(),
            local_devices,
            app.local_resource.status.label()
        )),
        Line::from(format!(
            "admin        {} · {} devices · {}",
            app.admin.devices.state.label(),
            admin_devices,
            app.admin.devices.observed_at.map_or_else(
                || "not observed".to_owned(),
                |value| format!("observed {value}")
            )
        )),
        Line::from("Admin queues · derived from observed snapshots"),
    ];
    for (label, count) in queue_lines {
        lines.push(Line::from(format!("  {label:<31} {count}")));
    }
    if !queues.resource_problems.is_empty() {
        lines.push(Line::from(format!(
            "resource states  {}",
            queues.resource_problems.join(" · ")
        )));
    }
    if !queues.client_versions.is_empty() {
        let versions = queues
            .client_versions
            .iter()
            .map(|(version, count)| format!("{version} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(Line::from(format!("client versions  {versions}")));
    }
    append_health(&mut lines, app);
    lines.push(Line::from(
        "Use :devices, :users, :routes, :dns, :access, or :credentials for read-only detail.",
    ));
    panel::render(frame, app, area, "overview", lines);
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
    let state = app.local_daemon_state.label().to_owned();
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
    let mut lines = vec![
        Line::from("Overview · local source"),
        Line::from(format!(
            "local       daemon {} · CLI {} · {}",
            state,
            app.local_cli_state.label(),
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
    append_health(&mut lines, app);
    panel::render(frame, app, area, "overview", lines);
}

fn append_health(lines: &mut Vec<Line<'static>>, app: &App) {
    lines.push(Line::from(""));
    lines.extend(
        crate::ui::views::health::summary(app)
            .lines()
            .map(|line| Line::from(line.to_owned())),
    );
}
