use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::domain::device::{AdminDevice, Device, Liveness};
use crate::ui::components::{grid, panel};
use crate::ui::{text, theme};

/// When a column is worth its width. The device table used to carry five
/// parallel header lists and five parallel width lists that had to be kept in
/// step by hand; it is one ordered list with a predicate instead.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Tier {
    Always,
    /// Shown once the user asks for wide columns.
    Wide,
    /// Needs the local client and the width to carry it.
    Local,
    /// Needs an admin profile.
    Admin,
    /// Either of the two above, sharing one column.
    LocalOrAdmin,
    /// Local, and wide enough for traffic counters.
    Traffic,
}

struct Layout {
    wide: bool,
    local: bool,
    admin: bool,
    traffic: bool,
}

impl Layout {
    fn shows(&self, tier: Tier) -> bool {
        match tier {
            Tier::Always => true,
            Tier::Wide => self.wide,
            Tier::Local => self.local,
            Tier::Admin => self.admin,
            Tier::LocalOrAdmin => self.local || self.admin,
            Tier::Traffic => self.traffic,
        }
    }
}

/// Header, width, and when it appears. The order here is the order on screen.
const COLUMNS: &[(&str, Tier, grid::Width)] = &[
    ("S", Tier::Always, grid::Width::Fixed(2)),
    ("NAME", Tier::Always, grid::Width::Fill(13)),
    ("OWNER", Tier::Always, grid::Width::Fill(14)),
    ("TAGS", Tier::Always, grid::Width::Fill(12)),
    ("OS", Tier::Always, grid::Width::Fill(7)),
    ("RELAY", Tier::Always, grid::Width::Fill(9)),
    ("SEEN", Tier::Always, grid::Width::Fill(6)),
    ("IP", Tier::Wide, grid::Width::Fill(14)),
    ("VER", Tier::Wide, grid::Width::Fill(9)),
    ("ROUTES", Tier::Local, grid::Width::Fill(14)),
    ("APPROVAL", Tier::Admin, grid::Width::Fill(12)),
    ("KEY", Tier::Admin, grid::Width::Fill(12)),
    ("ROLE", Tier::LocalOrAdmin, grid::Width::Fill(14)),
    ("POSTURE", Tier::Admin, grid::Width::Fill(10)),
    ("RX", Tier::Traffic, grid::Width::Fill(10)),
    ("TX", Tier::Traffic, grid::Width::Fill(10)),
];

pub fn render_devices(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let initial_loading = app.devices_resource.health == crate::domain::SourceHealth::Loading
        && app.devices_resource.snapshot.is_empty();
    let (columns, rows) = if app.views.devices.columns.is_empty() {
        default_table(app, area)
    } else {
        registered_table(app, area)
    };
    let lines = if initial_loading {
        vec![Line::from(Span::styled(
            "Loading devices…",
            app.theme.style(theme::StyleRole::StatePending),
        ))]
    } else {
        grid::lines(app, &columns, &rows, area.width.saturating_sub(4))
    };
    panel::render_view(frame, app, area, devices_title(app), lines);
}

fn layout_for(app: &App, area: Rect) -> Layout {
    // The columns follow the rows. Local columns are empty for a tailnet this
    // machine is not on, and admin columns are empty for a tailnet no profile
    // is reading, so neither is spent on a source that is not on screen.
    let source = app.device_view_source();
    let wide = app.views.devices.wide_columns && area.width >= 120;
    let local = source.is_locally_reachable() && app.source_mode == crate::app::SourceMode::Local;
    Layout {
        wide: app.views.devices.wide_columns,
        local: local && wide,
        admin: source != crate::app::DeviceViewSource::Local && wide,
        traffic: local && wide && area.width >= 150,
    }
}

fn default_table(app: &App, area: Rect) -> (Vec<grid::Column>, Vec<grid::Row>) {
    let layout = layout_for(app, area);
    let columns = COLUMNS
        .iter()
        .filter(|(_, tier, _)| layout.shows(*tier))
        .map(|(header, _, width)| grid::Column {
            header: (*header).to_owned(),
            width: *width,
        })
        .collect::<Vec<_>>();
    let rows = visible_devices(app, area)
        .map(|(device, selected)| {
            let cells = COLUMNS
                .iter()
                .filter(|(_, tier, _)| layout.shows(*tier))
                .map(|(header, _, _)| cell(app, device, header))
                .collect::<Vec<_>>();
            grid::Row::new(cells).selected(selected)
        })
        .collect();
    (columns, rows)
}

/// A saved view names its own columns, so the header is whatever it asked for.
fn registered_table(app: &App, area: Rect) -> (Vec<grid::Column>, Vec<grid::Row>) {
    let mut columns = vec![grid::Column::fixed("S", 2)];
    columns.extend(
        app.views
            .devices
            .columns
            .iter()
            .map(|name| grid::Column::fill(name.to_uppercase(), 10)),
    );
    let rows = visible_devices(app, area)
        .map(|(device, selected)| {
            let mut cells = vec![liveness_cell(app, device)];
            cells.extend(
                app.views
                    .devices
                    .columns
                    .iter()
                    .map(|name| grid::Cell::new(registered_value(app, device, name))),
            );
            grid::Row::new(cells).selected(selected)
        })
        .collect();
    (columns, rows)
}

fn visible_devices<'a>(app: &'a App, area: Rect) -> impl Iterator<Item = (&'a Device, bool)> + 'a {
    let visible = app.visible_indices_arc();
    let start = app.views.devices.scroll.min(visible.len());
    let viewport = usize::from(area.height.saturating_sub(3));
    visible
        .iter()
        .skip(start)
        .take(viewport)
        .filter_map(|index| app.devices_resource.snapshot.get(*index))
        .map(|device| {
            let selected = app
                .views
                .devices
                .selected_id
                .as_ref()
                .is_some_and(|id| id == &device.id);
            (device, selected)
        })
        .collect::<Vec<_>>()
        .into_iter()
}

/// The one cell that means something the row does not: whether the device is
/// reachable at all.
fn liveness_cell(app: &App, device: &Device) -> grid::Cell {
    let marker = if app.resolved_config.ui.symbols.unicode() {
        match device.liveness {
            Liveness::Online => "●",
            Liveness::Offline => "○",
            Liveness::Unknown => "?",
        }
    } else {
        device.liveness.marker()
    };
    grid::Cell::new(marker).with_role(match device.liveness {
        Liveness::Online => theme::StyleRole::StateHealthy,
        Liveness::Offline => theme::StyleRole::StateOffline,
        Liveness::Unknown => theme::StyleRole::StateUnknown,
    })
}

fn cell(app: &App, device: &Device, header: &str) -> grid::Cell {
    match header {
        "S" => liveness_cell(app, device),
        "NAME" => grid::Cell::new(device.display_name.clone()),
        "OWNER" => grid::Cell::new(
            device
                .owner
                .as_deref()
                .or(device.owner_label.as_deref())
                .unwrap_or("-"),
        ),
        "TAGS" => grid::Cell::new(text::tag_list(&device.tags)),
        "OS" => grid::Cell::new(device.os.label()),
        "RELAY" => grid::Cell::new(device.path.relay_label()),
        "SEEN" => grid::Cell::new(
            device
                .age_at(app.now)
                .map_or_else(|| "-".to_owned(), text::format_age),
        ),
        "IP" => grid::Cell::new(device.addresses.join(", ")),
        "VER" => grid::Cell::new(device.version.as_deref().unwrap_or("-")),
        "ROUTES" => grid::Cell::new(device.advertised_routes.join(", ")),
        "RX" => grid::Cell::new(optional_bytes(device.rx_bytes)),
        "TX" => grid::Cell::new(optional_bytes(device.tx_bytes)),
        "ROLE" if app.source_mode == crate::app::SourceMode::Local => {
            grid::Cell::new(text::capability_list(&[
                ("exit", device.capabilities.exit_node),
                ("option", device.capabilities.exit_node_option),
                ("router", device.capabilities.subnet_router),
                ("ssh", device.capabilities.ssh),
                ("shared", device.capabilities.shared),
            ]))
        }
        _ => admin_cell(app, device, header),
    }
}

fn admin_cell(app: &App, device: &Device, header: &str) -> grid::Cell {
    let admin = app.admin.devices.snapshot.as_ref().and_then(|devices| {
        devices.iter().find(|candidate| {
            candidate.stable_id == device.id.0
                || candidate.exact_node_id() == Some(device.id.0.as_str())
        })
    });
    let Some(admin) = admin else {
        return grid::Cell::new("unknown");
    };
    grid::Cell::new(match header {
        "APPROVAL" => match admin.authorized {
            Some(true) => "approved".to_owned(),
            Some(false) => "pending".to_owned(),
            None => "unknown".to_owned(),
        },
        "KEY" => key_expiry(app, admin),
        "ROLE" => route_role(admin),
        "POSTURE" => match admin.posture_present {
            Some(true) => "present".to_owned(),
            Some(false) => "empty".to_owned(),
            None => "not loaded".to_owned(),
        },
        _ => "unknown".to_owned(),
    })
}

fn key_expiry(app: &App, admin: &AdminDevice) -> String {
    if admin.key_expiry_disabled == Some(true) {
        return "disabled".to_owned();
    }
    admin.expires_at.map_or_else(
        || "unknown".to_owned(),
        |expires| {
            if expires <= app.now {
                "expired".to_owned()
            } else {
                text::format_age(expires.saturating_sub(app.now))
            }
        },
    )
}

fn route_role(admin: &AdminDevice) -> String {
    if !admin.advertised_routes_returned && !admin.enabled_routes_returned {
        return "unknown".to_owned();
    }
    let exit = |routes: &[String]| {
        routes
            .iter()
            .any(|route| route == "0.0.0.0/0" || route == "::/0")
    };
    text::capability_list(&[
        ("exit-advert", exit(&admin.advertised_routes)),
        ("exit-enabled", exit(&admin.enabled_routes)),
        ("subnet-advert", !admin.advertised_routes.is_empty()),
    ])
}

fn registered_value(app: &App, device: &Device, name: &str) -> String {
    match name {
        "id" => device.id.0.clone(),
        "name" => device.display_name.clone(),
        "owner" => device
            .owner
            .as_deref()
            .or(device.owner_label.as_deref())
            .map_or_else(|| "-".to_owned(), str::to_owned),
        "version" => device.version.clone().unwrap_or_else(|| "-".to_owned()),
        "last_seen" => device
            .age_at(app.now)
            .map_or_else(|| "-".to_owned(), text::format_age),
        "os" => device.os.label().to_owned(),
        "path" => device.path.label().to_owned(),
        "relay" => device.path.relay_label().to_owned(),
        "tags" => text::tag_list(&device.tags),
        "online" | "state" => device.liveness.label().to_owned(),
        "source" => app.source_mode.label().to_owned(),
        _ => "-".to_owned(),
    }
}

fn optional_bytes(value: Option<u64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| text::format_bytes(Some(value)))
}

/// Route context lives in the border: what this is, how much of it is showing,
/// and the terms that narrowed it.
fn devices_title(app: &App) -> ratatui::text::Line<'static> {
    let mut detail = Vec::new();
    if !app.views.devices.filter_draft.is_empty() {
        detail.push(format!(
            "/{}",
            text::ellipsize(&app.views.devices.filter_draft, 32)
        ));
    }
    detail.push(format!(
        "{} {}",
        app.views.devices.sort.field.display_label(),
        if app.views.devices.sort.direction.is_ascending() {
            "\u{2191}"
        } else {
            "\u{2193}"
        }
    ));
    detail.push(
        if app.views.devices.columns.is_empty() {
            if app.views.devices.wide_columns {
                "columns: extended"
            } else {
                "columns: standard"
            }
        } else {
            "columns: saved view"
        }
        .to_owned(),
    );
    detail.push(app.device_view_source().label().to_owned());
    if let crate::app::SourceAlignment::Divergent { local, .. } = app.source_alignment() {
        // The rows are the profile's tailnet; this machine is on another one.
        // Saying so is what stops the list from reading as one fleet.
        detail.push(format!("local client on {local}"));
    }
    if app.devices_resource.health == crate::domain::SourceHealth::Loading
        && app.devices_resource.snapshot.is_empty()
    {
        detail.insert(0, "loading".to_owned());
        return text::status_title(app.theme, "devices", &detail);
    }
    text::view_title(
        app.theme,
        "devices",
        app.visible_indices().len(),
        app.devices_resource.snapshot.len(),
        &detail,
    )
}
