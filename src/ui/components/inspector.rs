use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::ui::components::panel;
use crate::ui::{text, theme};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.admin.profile.is_some() && app.local_resource.snapshot.is_some() {
        render_combined(frame, app, area);
        return;
    }
    if app.source_mode == crate::app::SourceMode::Local {
        render_local(frame, app, area);
        return;
    }
    let Some(device) = app.selected_device() else {
        panel::render(frame, app, area, "inspector", "No device selected");
        return;
    };
    let owner = device
        .owner
        .as_deref()
        .or(device.owner_label.as_deref())
        .map_or("not returned", |value| value);
    let addresses = if device.addresses.is_empty() {
        "not returned".to_owned()
    } else {
        device.addresses.join(", ")
    };
    let tags = text::tag_list(&device.tags);
    let lines = vec![
        Line::from(Span::styled(
            text::ellipsize(
                &device.display_name,
                usize::from(area.width.saturating_sub(4)),
            ),
            app.theme.style(theme::StyleRole::TextPrimary),
        )),
        Line::from(format!("id           {}", device.id)),
        Line::from(format!("hostname     {}", device.hostname)),
        Line::from(format!("owner        {owner}")),
        Line::from(format!(
            "os           {} {}",
            device.os.label(),
            device.version
        )),
        Line::from(format!(
            "state        {} / {}",
            device.liveness.label(),
            device.path.label()
        )),
        Line::from(format!("address      {addresses}")),
        Line::from(format!("tags         {tags}")),
        Line::from(format!(
            "capabilities {}",
            capability_summary(&device.capabilities)
        )),
        Line::from(format!("key          {}", key_state(&device.capabilities))),
        Line::from(format!(
            "seen         {}",
            device
                .last_seen
                .map_or_else(|| "not reported".to_owned(), |value| value.to_string())
        )),
        Line::from("source       mock · deterministic fictional data"),
    ];
    panel::render_focusable(
        frame,
        app,
        area,
        "inspector",
        lines,
        app.focus == crate::app::Focus::Inspector,
    );
}

fn render_combined(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(device) = app.selected_device() else {
        panel::render(frame, app, area, "inspector", "No device selected");
        return;
    };
    let admin = app.admin.devices.snapshot.as_ref().and_then(|devices| {
        devices.iter().find(|candidate| {
            candidate.stable_id == device.id.0
                || candidate.exact_node_id() == Some(device.id.0.as_str())
        })
    });
    let lines = vec![
        Line::from(Span::styled(
            text::ellipsize(
                &device.display_name,
                usize::from(area.width.saturating_sub(4)),
            ),
            app.theme.style(theme::StyleRole::TextPrimary),
        )),
        Line::from(format!("id          {}", device.id)),
        Line::from(format!(
            "local state {} / {}",
            device.liveness.label(),
            device.path.label()
        )),
        Line::from(format!(
            "admin state {}",
            admin.map_or("not matched", |value| {
                if value.connected_to_control == Some(true) {
                    "online"
                } else if value.connected_to_control == Some(false) {
                    "offline"
                } else {
                    "unknown"
                }
            })
        )),
        Line::from(format!(
            "approval    {}",
            admin.map_or("unknown", |value| match value.authorized {
                Some(true) => "approved",
                Some(false) => "awaiting approval",
                None => "unknown",
            })
        )),
        Line::from(format!(
            "key expiry  {}",
            admin.map_or("unknown".to_owned(), |value| value
                .expires_at
                .map_or_else(|| "unknown".to_owned(), |expiry| expiry.to_string(),))
        )),
        Line::from(format!(
            "posture     {}",
            admin.map_or("unknown", |value| match value.posture_present {
                Some(true) => "present",
                Some(false) => "empty",
                None => "not loaded",
            })
        )),
        Line::from(format!("admin source {}", app.admin.devices.state.label())),
        Line::from("identity composition uses the exact stable node ID only"),
    ];
    panel::render(frame, app, area, "inspector · combined", lines);
}

fn render_local(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(device) = app.selected_local_device() else {
        panel::render(frame, app, area, "inspector", "No device selected");
        return;
    };
    let addresses = if device.tailscale_ips.is_empty() {
        "not returned".to_owned()
    } else {
        device.tailscale_ips.join(", ")
    };
    let routes = if device.advertised_routes.is_empty() {
        "not returned".to_owned()
    } else {
        device.advertised_routes.join(", ")
    };
    let tags = if device.tags.is_empty() {
        "not returned".to_owned()
    } else {
        device.tags.join(", ")
    };
    let lines = vec![
        Line::from(Span::styled(
            text::ellipsize(
                &device.display_name,
                usize::from(area.width.saturating_sub(4)),
            ),
            app.theme.style(theme::StyleRole::TextPrimary),
        )),
        Line::from(format!("id          {}", device.id)),
        Line::from(format!("hostname    {}", device.hostname)),
        Line::from(format!(
            "DNS name    {}",
            device
                .dns_name
                .as_deref()
                .map_or("not returned", |value| value)
        )),
        Line::from(format!(
            "OS/version  {} / {}",
            device.os.label(),
            device
                .version
                .as_deref()
                .map_or("not returned", |value| value)
        )),
        Line::from(format!(
            "state       {} / {} / active={}",
            device.liveness().label(),
            device.path.label(),
            device.active
        )),
        Line::from(format!("addresses   {addresses}")),
        Line::from(format!(
            "endpoint    {}",
            device
                .current_endpoint
                .as_deref()
                .map_or("not returned", |value| value)
        )),
        Line::from(format!(
            "relay       {}",
            device
                .relay_region
                .as_deref()
                .map_or("not returned", |value| value)
        )),
        Line::from(format!(
            "owner/tags  {} / {tags}",
            device
                .owner_label
                .as_deref()
                .map_or("not returned", |value| value)
        )),
        Line::from(format!("routes      {routes}")),
        Line::from(format!(
            "capabilities {}",
            text::capability_list(&[
                ("Exit node", device.exit_node),
                ("Exit node option", device.exit_node_option),
                ("SSH", device.ssh_host_keys_present),
                ("Shared", device.shared),
            ])
        )),
        Line::from(format!(
            "traffic     rx {} · tx {}",
            text::format_bytes(device.rx_bytes),
            text::format_bytes(device.tx_bytes)
        )),
        Line::from(format!(
            "seen        {} / handshake {}",
            device
                .last_seen
                .map_or_else(|| "not returned".to_owned(), |value| value.to_string()),
            device
                .last_handshake
                .map_or_else(|| "not returned".to_owned(), |value| value.to_string())
        )),
        Line::from(format!(
            "source      local · {}",
            app.local_resource.status.label()
        )),
    ];
    panel::render_focusable(
        frame,
        app,
        area,
        "inspector",
        lines,
        app.focus == crate::app::Focus::Inspector,
    );
}

/// Names only the capabilities a device has. `false` is not information a
/// reader needs six times in a row.
fn capability_summary(capabilities: &crate::domain::device::DeviceCapabilities) -> String {
    text::capability_list(&[
        ("Exit node", capabilities.exit_node),
        ("Exit node option", capabilities.exit_node_option),
        ("Subnet router", capabilities.subnet_router),
        ("SSH", capabilities.ssh),
        ("Funnel", capabilities.funnel),
        ("Shared", capabilities.shared),
    ])
}

fn key_state(capabilities: &crate::domain::device::DeviceCapabilities) -> &'static str {
    match (capabilities.expired, capabilities.approved) {
        (true, true) => "expired",
        (true, false) => "expired · awaiting approval",
        (false, true) => "valid",
        (false, false) => "awaiting approval",
    }
}
