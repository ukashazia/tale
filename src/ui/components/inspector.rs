use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::ui::{text, theme};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.source_mode == crate::app::SourceMode::Local {
        render_local(frame, app, area);
        return;
    }
    let Some(device) = app.selected_device() else {
        frame.render_widget(
            Paragraph::new("No device selected")
                .block(Block::default().borders(Borders::ALL).title("inspector")),
            area,
        );
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
    let tags = if device.tags.is_empty() {
        "-".to_owned()
    } else {
        device.tags.join(", ")
    };
    let lines = vec![
        Line::from(Span::styled(
            text::ellipsize(
                &device.display_name,
                usize::from(area.width.saturating_sub(4)),
            ),
            theme::title(),
        )),
        Line::from(format!("id       {}", device.id)),
        Line::from(format!("hostname {}", device.hostname)),
        Line::from(format!("owner    {owner}")),
        Line::from(format!("os       {} {}", device.os.label(), device.version)),
        Line::from(format!(
            "state    {} / {}",
            device.liveness.label(),
            device.path.label()
        )),
        Line::from(format!("address  {addresses}")),
        Line::from(format!("tags     {tags}")),
        Line::from(format!(
            "features exit={} option={} router={} ssh={} funnel={} shared={}",
            device.capabilities.exit_node,
            device.capabilities.exit_node_option,
            device.capabilities.subnet_router,
            device.capabilities.ssh,
            device.capabilities.funnel,
            device.capabilities.shared
        )),
        Line::from(format!(
            "key      expired={} approved={}",
            device.capabilities.expired, device.capabilities.approved
        )),
        Line::from(format!(
            "seen     {}",
            device
                .last_seen
                .map_or_else(|| "not returned".to_owned(), |value| value.to_string())
        )),
        Line::from("source   mock · deterministic fictional data"),
    ];
    let style = if app.focus == crate::app::Focus::Inspector {
        theme::focused()
    } else {
        theme::normal(app)
    };
    frame.render_widget(
        Paragraph::new(lines)
            .style(style)
            .block(Block::default().borders(Borders::ALL).title("inspector")),
        area,
    );
}

fn render_local(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(device) = app.selected_local_device() else {
        frame.render_widget(
            Paragraph::new("No device selected")
                .block(Block::default().borders(Borders::ALL).title("inspector")),
            area,
        );
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
            theme::title(),
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
            "roles       exit={} option={} ssh={} shared={}",
            device.exit_node, device.exit_node_option, device.ssh_host_keys_present, device.shared
        )),
        Line::from(format!(
            "traffic     rx={} tx={}",
            device
                .rx_bytes
                .map_or_else(|| "not returned".to_owned(), |value| value.to_string()),
            device
                .tx_bytes
                .map_or_else(|| "not returned".to_owned(), |value| value.to_string())
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
    let style = if app.focus == crate::app::Focus::Inspector {
        theme::focused()
    } else {
        theme::normal(app)
    };
    frame.render_widget(
        Paragraph::new(lines)
            .style(style)
            .block(Block::default().borders(Borders::ALL).title("inspector")),
        area,
    );
}
