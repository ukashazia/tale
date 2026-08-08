use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::app::{App, DeviceViewSource, Focus, SourceMode};
use crate::domain::Timestamp;
use crate::domain::device::{AdminDevice, ConnectionPath, Device, DeviceCapabilities, LocalDevice};
use crate::ui::components::{grid, panel};
use crate::ui::{text, theme};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.focus == Focus::Inspector {
        render_full_detail(frame, app, area);
        return;
    }
    if app.source_mode == SourceMode::Mock {
        render_mock_summary(frame, app, area);
        return;
    }
    match app.device_view_source() {
        DeviceViewSource::Local => render_local_summary(frame, app, area),
        DeviceViewSource::Composed => render_composed_summary(frame, app, area),
        DeviceViewSource::Admin => render_admin_summary(frame, app, area),
    }
}

/// Used by the reducer to keep `j`/`k`, `g`/`G`, and page movement inside the
/// full-screen document. The line count is independent of terminal width: long
/// values are clipped, never wrapped into a second logical row.
pub fn device_detail_line_count(app: &App) -> usize {
    full_detail_lines(app, usize::MAX).len()
}

pub fn device_detail_max_scroll(app: &App, area_height: u16) -> usize {
    let visible = usize::from(area_height.saturating_sub(2)).max(1);
    device_detail_line_count(app).saturating_sub(visible)
}

pub fn device_detail_search_matches(app: &App, query: &str) -> Vec<usize> {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    full_detail_lines(app, usize::MAX)
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            line_text(line)
                .to_ascii_lowercase()
                .contains(&query)
                .then_some(index)
        })
        .collect()
}

fn render_full_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = full_detail_lines(app, usize::from(area.width.saturating_sub(4)));
    if lines.is_empty() {
        panel::render(frame, app, area, "device details", "No device selected");
        return;
    }
    let visible = usize::from(area.height.saturating_sub(2)).max(1);
    let max_scroll = device_detail_max_scroll(app, area.height);
    let scroll = app.views.devices.detail_scroll.min(max_scroll);
    let end = scroll.saturating_add(visible).min(lines.len());
    let matches = style_search_matches(app, &mut lines);
    let source = match app.source_mode {
        SourceMode::Mock => "mock",
        _ => app.device_view_source().label(),
    };
    let position = app.views.devices.detail_search_match.and_then(|line| {
        matches
            .iter()
            .position(|candidate| *candidate == line)
            .map(|position| position.saturating_add(1))
    });
    let search = if app.views.devices.detail_search.is_empty() {
        String::new()
    } else {
        format!(
            " · match {}/{} · /{}",
            position.map_or(0, |value| value),
            matches.len(),
            app.views.devices.detail_search
        )
    };
    let title = if max_scroll == 0 {
        format!("device details · {source}{search}")
    } else {
        format!(
            "device details · {source} · {}-{} of {}{search}",
            scroll.saturating_add(1),
            end,
            lines.len()
        )
    };
    let scroll = u16::try_from(scroll).map_or(u16::MAX, |value| value);
    panel::render_focusable_scrolled(frame, app, area, &title, lines, scroll);
}

fn style_search_matches(app: &App, lines: &mut [Line<'static>]) -> Vec<usize> {
    let matches = device_detail_search_matches(app, &app.views.devices.detail_search);
    for index in &matches {
        let Some(line) = lines.get_mut(*index) else {
            continue;
        };
        let role = if app.views.devices.detail_search_match == Some(*index) {
            theme::StyleRole::Selection
        } else {
            theme::StyleRole::CompletionMatch
        };
        let style = app.theme.style(role);
        line.style = line.style.patch(style);
        for span in &mut line.spans {
            span.style = span.style.patch(style);
        }
    }
    matches
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn full_detail_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    if app.source_mode == SourceMode::Mock {
        return app
            .selected_device()
            .map_or_else(Vec::new, |device| mock_detail(app, device, width));
    }
    let local = app.selected_local_device();
    let admin = selected_admin_device(app);
    if local.is_none() && admin.is_none() {
        return Vec::new();
    }
    let name = local
        .map(|device| device.display_name.as_str())
        .or_else(|| admin.map(AdminDevice::display_name));
    let mut lines = vec![Line::from(Span::styled(
        text::ellipsize(name.unwrap_or("not returned"), width),
        app.theme.style(theme::StyleRole::TextPrimary),
    ))];
    if let Some(local) = local {
        append_local_sections(app, &mut lines, local, width);
    }
    if let Some(admin) = admin {
        append_admin_sections(app, &mut lines, admin, width);
    }
    append_unavailable_section(app, &mut lines);
    lines
}

fn mock_detail(app: &App, device: &Device, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        text::ellipsize(&device.display_name, width),
        app.theme.style(theme::StyleRole::TextPrimary),
    ))];
    append_section(
        app,
        &mut lines,
        "Identity",
        vec![
            ("stable ID", device.id.to_string()),
            ("machine name", device.display_name.clone()),
            ("OS hostname", device.hostname.clone()),
            (
                "owner",
                optional(device.owner.as_deref().or(device.owner_label.as_deref())),
            ),
            ("tags", list(&device.tags, "none")),
            (
                "OS / version",
                format!(
                    "{} / {}",
                    device.os.label(),
                    optional(device.version.as_deref())
                ),
            ),
            ("created", moment(app, device.created_at)),
        ],
    );
    append_section(
        app,
        &mut lines,
        "Network",
        vec![
            ("addresses", list(&device.addresses, "none")),
            (
                "connection",
                format!(
                    "{} / {}",
                    device.liveness.label(),
                    path_detail(&device.path)
                ),
            ),
            ("last seen", moment(app, device.last_seen)),
            (
                "traffic",
                format!(
                    "rx {} · tx {}",
                    text::format_bytes(device.rx_bytes),
                    text::format_bytes(device.tx_bytes)
                ),
            ),
        ],
    );
    append_section(
        app,
        &mut lines,
        "Routing and roles",
        vec![
            ("advertised", list(&device.advertised_routes, "none")),
            ("capabilities", capability_summary(&device.capabilities)),
            ("key", key_state(&device.capabilities).to_owned()),
        ],
    );
    append_section(
        app,
        &mut lines,
        "Source",
        vec![(
            "Status:",
            "Simulated data · deterministic fictional record".to_owned(),
        )],
    );
    lines
}

fn append_local_sections(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    device: &LocalDevice,
    width: usize,
) {
    append_section(
        app,
        lines,
        "Identity · local daemon",
        vec![
            ("stable ID", device.id.to_string()),
            ("node public key", optional(device.public_key.as_deref())),
            ("machine name", device.display_name.clone()),
            ("OS hostname", device.hostname.clone()),
            (
                "full domain",
                optional(device.dns_name.as_deref())
                    .trim_end_matches('.')
                    .to_owned(),
            ),
            ("owner", local_owner(device)),
            ("tags", list(&device.tags, "none")),
            (
                "OS / version",
                format!(
                    "{} / {}",
                    device.os.label(),
                    optional(device.version.as_deref())
                ),
            ),
            ("created", moment(app, device.created_at)),
        ],
    );
    append_section(
        app,
        lines,
        "Addresses · local daemon",
        vec![
            (
                "Tailscale IPs",
                list(&device.tailscale_ips, "none returned"),
            ),
            (
                "endpoint",
                format!(
                    "{} · potentially sensitive",
                    optional(device.current_endpoint.as_deref())
                ),
            ),
        ],
    );
    append_section(
        app,
        lines,
        "Connection · local daemon",
        vec![
            ("online", optional_bool(device.online, "yes", "no")),
            ("active", yes_no(device.active).to_owned()),
            ("path", path_detail(&device.path)),
            ("relay", optional(device.relay_region.as_deref())),
            ("last seen", moment(app, device.last_seen)),
            ("last handshake", moment(app, device.last_handshake)),
            (
                "traffic",
                format!(
                    "rx {} · tx {}",
                    text::format_bytes(device.rx_bytes),
                    text::format_bytes(device.tx_bytes)
                ),
            ),
        ],
    );
    append_section(
        app,
        lines,
        "Routing and roles · local daemon",
        vec![
            ("advertised routes", list(&device.advertised_routes, "none")),
            (
                "capabilities",
                text::capability_list(&[
                    ("Exit node", device.exit_node),
                    ("Exit node option", device.exit_node_option),
                    ("Subnet router", !device.advertised_routes.is_empty()),
                    ("SSH", device.ssh_host_keys_present),
                    ("Shared", device.shared),
                ]),
            ),
        ],
    );
    append_section(
        app,
        lines,
        "Reported capabilities · local daemon",
        reported_capability_rows(device, width),
    );
    append_section(
        app,
        lines,
        "Source · local daemon",
        vec![
            ("state", app.local_resource.status.label().to_owned()),
            ("observed", moment(app, app.local_resource.last_success_at)),
        ],
    );
}

fn reported_capability_rows(device: &LocalDevice, width: usize) -> Vec<(&'static str, String)> {
    if device.capabilities.is_empty() {
        return vec![("capabilities", "none reported".to_owned())];
    }
    device
        .capabilities
        .iter()
        .map(|(name, enabled)| describe_reported_capability(name, *enabled, width))
        .collect()
}

fn describe_reported_capability(name: &str, enabled: bool, width: usize) -> (&'static str, String) {
    let available = if enabled { "available" } else { "unavailable" };
    let enabled_state = if enabled { "enabled" } else { "disabled" };
    let yes_no = if enabled { "yes" } else { "no" };
    let known = match name {
        "defaultAutoUpdate" => Some(("default auto-update", enabled_state.to_owned())),
        "funnel" => Some(("Funnel", available.to_owned())),
        "https" => Some(("HTTPS", available.to_owned())),
        "ssh" => Some(("Tailscale SSH", available.to_owned())),
        "approved" => Some(("device approved", yes_no.to_owned())),
        "expired" => Some(("node key expired", yes_no.to_owned())),
        "exit-node" => Some(("exit node", enabled_state.to_owned())),
        "exit-node-option" => Some(("exit node option", enabled_state.to_owned())),
        "shared" => Some(("shared node", yes_no.to_owned())),
        _ => describe_tailscale_capability_url(name, enabled),
    };
    let (label, value) =
        known.unwrap_or_else(|| ("other capability", format!("{enabled_state} · {name}")));
    (label, bounded_capability_value(&value, width))
}

fn describe_tailscale_capability_url(name: &str, enabled: bool) -> Option<(&'static str, String)> {
    let url = url::Url::parse(name).ok()?;
    if url.scheme() != "https" || url.host_str() != Some("tailscale.com") {
        return None;
    }
    let available = if enabled { "available" } else { "unavailable" };
    match url.path() {
        "/cap/file-sharing" => Some(("file sharing", available.to_owned())),
        "/cap/is-admin" => Some((
            "tailnet admin",
            if enabled { "yes" } else { "no" }.to_owned(),
        )),
        "/cap/funnel-ports" => {
            let ports = url
                .query_pairs()
                .find_map(|(key, value)| (key == "ports").then(|| value.into_owned()));
            Some((
                "Funnel ports",
                ports.map_or_else(
                    || available.to_owned(),
                    |ports| {
                        let ports = ports.split(',').collect::<Vec<_>>().join(", ");
                        if enabled {
                            ports
                        } else {
                            format!("unavailable · {ports}")
                        }
                    },
                ),
            ))
        }
        _ => None,
    }
}

fn bounded_capability_value(value: &str, width: usize) -> String {
    text::ellipsize(value, width.saturating_sub(21))
}

fn append_admin_sections(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    device: &AdminDevice,
    width: usize,
) {
    append_section(
        app,
        lines,
        "Identity · admin",
        vec![
            ("stable ID", device.stable_id.clone()),
            ("node ID", optional(device.node_id.as_deref())),
            ("legacy ID", optional(device.legacy_id.as_deref())),
            (
                "machine name",
                optional(device.name.as_deref().map(short_name)),
            ),
            ("OS hostname", optional(device.hostname.as_deref())),
            ("full domain", optional(device.name.as_deref())),
            ("owner / user", admin_owner(app, device)),
            ("ACL tags", list(&device.tags, "none")),
            (
                "OS / version",
                format!(
                    "{} / {}",
                    device.os.as_ref().map_or("not returned", |os| os.label()),
                    optional(device.client_version.as_deref())
                ),
            ),
            ("created", moment(app, device.created_at)),
        ],
    );
    append_section(
        app,
        lines,
        "Status and key · admin",
        vec![
            (
                "control connection",
                optional_bool(device.connected_to_control, "connected", "not connected"),
            ),
            (
                "approval",
                optional_bool(device.authorized, "approved", "awaiting approval"),
            ),
            ("last seen", moment(app, device.last_seen)),
            ("key expiry", key_expiry(app, device)),
            (
                "update",
                optional_bool(device.update_available, "available", "current"),
            ),
            ("ephemeral", optional_bool(device.is_ephemeral, "yes", "no")),
            (
                "external / shared",
                optional_bool(device.is_external, "yes", "no"),
            ),
            (
                "multiple connections",
                optional_bool(device.multiple_connections, "yes", "no"),
            ),
            (
                "Tailscale SSH",
                optional_bool(device.ssh_enabled, "enabled", "disabled"),
            ),
        ],
    );
    append_section(
        app,
        lines,
        "Addresses and routing · admin",
        vec![
            ("Tailscale IPs", list(&device.addresses, "none returned")),
            (
                "advertised routes",
                returned_list(device.advertised_routes_returned, &device.advertised_routes),
            ),
            (
                "enabled routes",
                returned_list(device.enabled_routes_returned, &device.enabled_routes),
            ),
            (
                "exit node advertised",
                yes_no(
                    device
                        .advertised_routes
                        .iter()
                        .any(|route| is_exit_route(route)),
                )
                .to_owned(),
            ),
            (
                "exit node approved",
                yes_no(
                    device
                        .enabled_routes
                        .iter()
                        .any(|route| is_exit_route(route)),
                )
                .to_owned(),
            ),
        ],
    );
    let posture_state = match device.posture_present {
        Some(true) => "attributes returned",
        Some(false) => "no attributes returned",
        None => "not loaded",
    };
    let mut posture = vec![("state", posture_state.to_owned())];
    for (name, value) in &device.posture_attributes {
        posture.push((
            "attribute",
            format!("{name} = {}", json_value(value, width)),
        ));
    }
    for (name, value) in &device.posture_expiries {
        posture.push(("expiry", format!("{name} = {}", json_value(value, width))));
    }
    append_section(app, lines, "Posture · admin", posture);
    let enrichment = if app.admin_device_enrichment_in_flight(&device.stable_id) {
        "loading device, routes, and posture"
    } else if device.advertised_routes_returned
        || device.enabled_routes_returned
        || device.posture_present.is_some()
    {
        "detail fetch complete"
    } else {
        "detail fields not loaded"
    };
    append_section(
        app,
        lines,
        "Source · admin",
        vec![
            ("inventory", app.admin.devices.state.label().to_owned()),
            ("detail", enrichment.to_owned()),
            ("observed", moment(app, Some(device.source_observed_at))),
            ("routes", app.admin.routes.state.label().to_owned()),
            ("posture", app.admin.posture.state.label().to_owned()),
        ],
    );
}

fn append_unavailable_section(app: &App, lines: &mut Vec<Line<'static>>) {
    append_section(
        app,
        lines,
        "Not observable from adopted APIs",
        vec![
            (
                "relay latency",
                "the web console's fleet DERP latency matrix is not exposed".to_owned(),
            ),
            (
                "client connectivity",
                "remote IPv6, UDP, UPnP, PCP, and NAT-PMP flags are not exposed".to_owned(),
            ),
            (
                "TLS certificate",
                "remote certificate state is not exposed".to_owned(),
            ),
        ],
    );
}

fn render_local_summary(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(device) = app.selected_local_device() else {
        panel::render(frame, app, area, "inspector", "No device selected");
        return;
    };
    let pairs = vec![
        ("id", device.id.to_string()),
        ("hostname", device.hostname.clone()),
        ("DNS name", optional(device.dns_name.as_deref())),
        (
            "OS/version",
            format!(
                "{} / {}",
                device.os.label(),
                device.version.as_deref().map_or("-", |value| value)
            ),
        ),
        (
            "state",
            format!(
                "{} / {}",
                device.liveness().label(),
                path_detail(&device.path)
            ),
        ),
        ("addresses", list(&device.tailscale_ips, "none returned")),
        (
            "owner / tags",
            format!("{} / {}", local_owner(device), list(&device.tags, "none")),
        ),
        ("routes", list(&device.advertised_routes, "none")),
        (
            "traffic",
            format!(
                "rx {} · tx {}",
                text::format_bytes(device.rx_bytes),
                text::format_bytes(device.tx_bytes)
            ),
        ),
    ];
    render_summary(frame, app, area, &device.display_name, pairs, "local");
}

fn render_mock_summary(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(device) = app.selected_device() else {
        panel::render(frame, app, area, "inspector", "No device selected");
        return;
    };
    let pairs = vec![
        ("id", device.id.to_string()),
        ("hostname", device.hostname.clone()),
        (
            "state",
            format!(
                "{} / {}",
                device.liveness.label(),
                path_detail(&device.path)
            ),
        ),
        ("addresses", list(&device.addresses, "none returned")),
        ("tags", list(&device.tags, "none")),
        ("capabilities", capability_summary(&device.capabilities)),
        ("Status:", "Simulated data".to_owned()),
    ];
    render_summary(frame, app, area, &device.display_name, pairs, "mock");
}

fn render_composed_summary(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(device) = app.selected_device() else {
        panel::render(frame, app, area, "inspector", "No device selected");
        return;
    };
    let admin = selected_admin_device(app);
    let pairs = vec![
        ("id", device.id.to_string()),
        (
            "local state",
            format!(
                "{} / {}",
                device.liveness.label(),
                path_detail(&device.path)
            ),
        ),
        (
            "admin state",
            admin.map_or_else(
                || "not matched".to_owned(),
                |value| optional_bool(value.connected_to_control, "connected", "not connected"),
            ),
        ),
        (
            "approval",
            admin.map_or_else(
                || "not matched".to_owned(),
                |value| optional_bool(value.authorized, "approved", "awaiting approval"),
            ),
        ),
        ("addresses", list(&device.addresses, "none returned")),
        ("tags", list(&device.tags, "none")),
        ("capabilities", capability_summary(&device.capabilities)),
    ];
    render_summary(
        frame,
        app,
        area,
        &device.display_name,
        pairs,
        "local + admin",
    );
}

fn render_admin_summary(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(device) = selected_admin_device(app) else {
        panel::render(frame, app, area, "inspector", "No device selected");
        return;
    };
    let pairs = vec![
        ("stable ID", device.stable_id.clone()),
        ("hostname", optional(device.hostname.as_deref())),
        ("owner / user", admin_owner(app, device)),
        (
            "state",
            optional_bool(device.connected_to_control, "connected", "not connected"),
        ),
        (
            "approval",
            optional_bool(device.authorized, "approved", "awaiting approval"),
        ),
        ("addresses", list(&device.addresses, "none returned")),
        ("ACL tags", list(&device.tags, "none")),
        ("key expiry", key_expiry(app, device)),
    ];
    render_summary(frame, app, area, device.display_name(), pairs, "admin");
}

fn render_summary(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    name: &str,
    pairs: Vec<(&str, String)>,
    source: &str,
) {
    let mut lines = vec![Line::from(Span::styled(
        text::ellipsize(name, usize::from(area.width.saturating_sub(4))),
        app.theme.style(theme::StyleRole::TextPrimary),
    ))];
    lines.extend(grid::detail(app, &pairs));
    lines.push(Line::from(Span::styled(
        format!("Enter opens all available {source} details"),
        app.theme.style(theme::StyleRole::TextMuted),
    )));
    panel::render_focusable(frame, app, area, "inspector", lines, false);
}

fn append_section(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    title: &str,
    pairs: Vec<(&str, String)>,
) {
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        title.to_owned(),
        app.theme.style(theme::StyleRole::SectionHeading),
    )));
    lines.extend(grid::detail(app, &pairs));
}

fn selected_admin_device(app: &App) -> Option<&AdminDevice> {
    let selected = app.views.devices.selected_id.as_ref()?.0.as_str();
    app.admin
        .devices
        .snapshot
        .as_ref()?
        .iter()
        .find(|device| device.stable_id == selected || device.exact_node_id() == Some(selected))
}

fn local_owner(device: &LocalDevice) -> String {
    match (device.owner_label.as_deref(), device.user_id.as_deref()) {
        (Some(label), Some(id)) => format!("{label} ({id})"),
        (Some(label), None) => label.to_owned(),
        (None, Some(id)) => id.to_owned(),
        (None, None) => "not returned".to_owned(),
    }
}

fn admin_owner(app: &App, device: &AdminDevice) -> String {
    let Some(id) = device.user_id.as_deref() else {
        return "not returned".to_owned();
    };
    app.admin
        .users
        .snapshot
        .as_ref()
        .and_then(|users| users.iter().find(|user| user.id == id))
        .map_or_else(|| id.to_owned(), |user| format!("{} ({id})", user.label()))
}

fn short_name(name: &str) -> &str {
    name.split_once('.').map_or(name, |(short, _)| short)
}

fn optional(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .map_or_else(|| "not returned".to_owned(), str::to_owned)
}

fn optional_bool(value: Option<bool>, yes: &str, no: &str) -> String {
    value.map_or_else(
        || "not returned".to_owned(),
        |value| if value { yes } else { no }.to_owned(),
    )
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn list(values: &[String], empty: &str) -> String {
    if values.is_empty() {
        empty.to_owned()
    } else {
        values.join(", ")
    }
}

fn returned_list(returned: bool, values: &[String]) -> String {
    if !returned {
        "not loaded".to_owned()
    } else {
        list(values, "none")
    }
}

fn path_detail(path: &ConnectionPath) -> String {
    match path {
        ConnectionPath::Direct {
            latency_ms: Some(latency),
        } => {
            format!("direct · {latency} ms")
        }
        ConnectionPath::Direct { latency_ms: None } => "direct".to_owned(),
        ConnectionPath::Derp { region } => format!("DERP · {region}"),
        ConnectionPath::PeerRelay { peer } => format!("peer relay · {peer}"),
        ConnectionPath::Idle => "idle".to_owned(),
        ConnectionPath::Unknown(detail) if !detail.is_empty() => format!("unknown · {detail}"),
        ConnectionPath::Unknown(_) => "unknown".to_owned(),
        ConnectionPath::NoPath => "no path".to_owned(),
    }
}

fn capability_summary(capabilities: &DeviceCapabilities) -> String {
    text::capability_list(&[
        ("Exit node", capabilities.exit_node),
        ("Exit node option", capabilities.exit_node_option),
        ("Subnet router", capabilities.subnet_router),
        ("SSH", capabilities.ssh),
        ("Funnel", capabilities.funnel),
        ("Shared", capabilities.shared),
    ])
}

fn key_state(capabilities: &DeviceCapabilities) -> &'static str {
    match (capabilities.expired, capabilities.approved) {
        (true, true) => "expired",
        (true, false) => "expired · awaiting approval",
        (false, true) => "valid",
        (false, false) => "awaiting approval",
    }
}

fn key_expiry(app: &App, device: &AdminDevice) -> String {
    match device.key_expiry_disabled {
        Some(true) => "disabled · no expiry".to_owned(),
        Some(false) => moment(app, device.expires_at),
        None if device.expires_at.is_some() => moment(app, device.expires_at),
        None => "not returned".to_owned(),
    }
}

fn moment(app: &App, value: Option<Timestamp>) -> String {
    let Some(value) = value else {
        return "not returned".to_owned();
    };
    let age = text::format_age(app.now.saturating_sub(value));
    let absolute = i64::try_from(value)
        .ok()
        .and_then(|value| OffsetDateTime::from_unix_timestamp(value).ok())
        .and_then(|value| value.format(&Rfc3339).ok());
    absolute.map_or_else(
        || format!("{value} · {age} ago"),
        |value| format!("{value} · {age} ago"),
    )
}

fn json_value(value: &Value, width: usize) -> String {
    let value = match value {
        Value::String(value) => value.clone(),
        value => value.to_string(),
    };
    text::ellipsize(&value, width.saturating_sub(20).max(16))
}

fn is_exit_route(route: &str) -> bool {
    route == "0.0.0.0/0" || route == "::/0"
}
