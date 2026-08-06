use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Cell, Row, Table};

use crate::app::App;
use crate::domain::device::{Device, Liveness};
use crate::ui::{text, theme};

pub fn render_devices(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if !app.views.devices.columns.is_empty() {
        render_registered_columns(frame, app, area);
        return;
    }
    let local_extended = app.source_mode == crate::app::SourceMode::Local
        && app.views.devices.wide_columns
        && area.width >= 120;
    let admin_extended =
        app.admin.profile.is_some() && app.views.devices.wide_columns && area.width >= 120;
    let local_traffic = local_extended && area.width >= 150;
    let visible = app.visible_indices_arc();
    let start = app.views.devices.scroll.min(visible.len());
    let viewport = usize::from(area.height.saturating_sub(3));
    let rows = visible
        .iter()
        .skip(start)
        .take(viewport)
        .filter_map(|index| {
            let device = app.devices_resource.snapshot.get(*index)?;
            let selected = app
                .views
                .devices
                .selected_id
                .as_ref()
                .is_some_and(|id| id == &device.id);
            Some(device_row(
                app,
                device,
                selected,
                area.width,
                local_extended,
                local_traffic,
                admin_extended,
            ))
        });
    let header_values = if local_traffic {
        vec![
            "S",
            "NAME",
            "OWNER/TAGS",
            "OS",
            "PATH",
            "SEEN",
            "IP",
            "VER",
            "ROUTES",
            "ROLE",
            "RX",
            "TX",
        ]
    } else if local_extended {
        vec![
            "S",
            "NAME",
            "OWNER/TAGS",
            "OS",
            "PATH",
            "SEEN",
            "IP",
            "VER",
            "ROUTES",
            "ROLE",
        ]
    } else if admin_extended {
        vec![
            "S",
            "NAME",
            "OWNER/TAGS",
            "OS",
            "PATH",
            "SEEN",
            "IP",
            "VER",
            "APPROVAL",
            "KEY",
            "ROLE",
            "POSTURE",
        ]
    } else if app.views.devices.wide_columns {
        vec!["S", "NAME", "OWNER/TAGS", "OS", "PATH", "SEEN", "IP", "VER"]
    } else {
        vec!["S", "NAME", "OWNER/TAGS", "OS", "PATH", "SEEN"]
    };
    let header = Row::new(header_values).style(app.theme.style(theme::StyleRole::TextPrimary));
    let widths = if local_traffic {
        vec![
            ConstraintWidth::Fixed(2),
            ConstraintWidth::Fill(13),
            ConstraintWidth::Fill(16),
            ConstraintWidth::Fill(7),
            ConstraintWidth::Fill(9),
            ConstraintWidth::Fill(6),
            ConstraintWidth::Fill(14),
            ConstraintWidth::Fill(9),
            ConstraintWidth::Fill(14),
            ConstraintWidth::Fill(10),
            ConstraintWidth::Fill(10),
            ConstraintWidth::Fill(10),
        ]
    } else if local_extended {
        vec![
            ConstraintWidth::Fixed(2),
            ConstraintWidth::Fill(13),
            ConstraintWidth::Fill(16),
            ConstraintWidth::Fill(7),
            ConstraintWidth::Fill(9),
            ConstraintWidth::Fill(6),
            ConstraintWidth::Fill(14),
            ConstraintWidth::Fill(9),
            ConstraintWidth::Fill(14),
            ConstraintWidth::Fill(10),
        ]
    } else if admin_extended {
        vec![
            ConstraintWidth::Fixed(2),
            ConstraintWidth::Fill(13),
            ConstraintWidth::Fill(16),
            ConstraintWidth::Fill(7),
            ConstraintWidth::Fill(9),
            ConstraintWidth::Fill(6),
            ConstraintWidth::Fill(14),
            ConstraintWidth::Fill(9),
            ConstraintWidth::Fill(12),
            ConstraintWidth::Fill(12),
            ConstraintWidth::Fill(14),
            ConstraintWidth::Fill(10),
        ]
    } else if app.views.devices.wide_columns {
        vec![
            ConstraintWidth::Fixed(2),
            ConstraintWidth::Fill(13),
            ConstraintWidth::Fill(16),
            ConstraintWidth::Fill(7),
            ConstraintWidth::Fill(9),
            ConstraintWidth::Fill(6),
            ConstraintWidth::Fill(14),
            ConstraintWidth::Fill(9),
        ]
    } else {
        vec![
            ConstraintWidth::Fixed(2),
            ConstraintWidth::Fill(13),
            ConstraintWidth::Fill(16),
            ConstraintWidth::Fill(7),
            ConstraintWidth::Fill(9),
            ConstraintWidth::Fill(6),
        ]
    };
    let constraints = widths
        .into_iter()
        .map(ConstraintWidth::constraint)
        .collect::<Vec<_>>();
    frame.render_widget(
        Table::new(rows, constraints)
            .header(header)
            .column_spacing(1)
            .style(app.theme.style(theme::StyleRole::Surface))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(app.theme.style(theme::StyleRole::BorderNormal))
                    .title(devices_title(app)),
            ),
        area,
    );
}

fn render_registered_columns(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let visible = app.visible_indices_arc();
    let start = app.views.devices.scroll.min(visible.len());
    let viewport = usize::from(area.height.saturating_sub(3));
    let rows = visible
        .iter()
        .skip(start)
        .take(viewport)
        .filter_map(|index| {
            let device = app.devices_resource.snapshot.get(*index)?;
            let selected = app
                .views
                .devices
                .selected_id
                .as_ref()
                .is_some_and(|id| id == &device.id);
            Some(registered_device_row(app, device, selected, area.width))
        });
    let mut headers = vec!["S".to_owned()];
    headers.extend(app.views.devices.columns.iter().cloned());
    let header = Row::new(headers).style(app.theme.style(theme::StyleRole::TextPrimary));
    let mut widths = vec![ConstraintWidth::Fixed(2)];
    widths.extend(
        app.views
            .devices
            .columns
            .iter()
            .map(|_| ConstraintWidth::Fill(12)),
    );
    let constraints = widths
        .into_iter()
        .map(ConstraintWidth::constraint)
        .collect::<Vec<_>>();
    frame.render_widget(
        Table::new(rows, constraints)
            .header(header)
            .column_spacing(1)
            .style(app.theme.style(theme::StyleRole::Surface))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(app.theme.style(theme::StyleRole::BorderNormal))
                    .title("devices · saved columns"),
            ),
        area,
    );
}

fn registered_device_row(app: &App, device: &Device, selected: bool, width: u16) -> Row<'static> {
    let marker = if app.resolved_config.ui.symbols.unicode() {
        match device.liveness {
            Liveness::Online => "●",
            Liveness::Offline => "○",
            Liveness::Unknown => "?",
        }
    } else {
        device.liveness.marker()
    };
    let cells = app
        .views
        .devices
        .columns
        .iter()
        .map(|column| {
            let value = match column.as_str() {
                "id" => device.id.0.clone(),
                "name" => device.display_name.clone(),
                "owner" => device
                    .owner
                    .as_deref()
                    .or(device.owner_label.as_deref())
                    .map_or_else(|| "-".to_owned(), str::to_owned),
                "version" => device.version.clone(),
                "last_seen" => device
                    .age_at(app.now)
                    .map_or_else(|| "-".to_owned(), format_age),
                "os" => device.os.label().to_owned(),
                "path" => device.path.label().to_owned(),
                "tags" => {
                    if device.tags.is_empty() {
                        "-".to_owned()
                    } else {
                        device.tags.join(",")
                    }
                }
                "online" | "state" => device.liveness.label().to_owned(),
                "source" => app.source_mode.label().to_owned(),
                _ => "not returned".to_owned(),
            };
            Cell::from(text::ellipsize(&value, usize::from(width.max(12))))
        })
        .collect::<Vec<_>>();
    let marker_role = match device.liveness {
        Liveness::Online => theme::StyleRole::StateHealthy,
        Liveness::Offline => theme::StyleRole::StateOffline,
        Liveness::Unknown => theme::StyleRole::StateUnknown,
    };
    let mut values = vec![Cell::from(Span::styled(
        marker.to_owned(),
        app.theme.style(marker_role),
    ))];
    values.extend(cells);
    let row = Row::new(values);
    if selected {
        row.style(app.theme.style(theme::StyleRole::Selection))
    } else {
        row
    }
}

#[derive(Clone, Copy)]
enum ConstraintWidth {
    Fixed(u16),
    Fill(u16),
}

impl ConstraintWidth {
    const fn constraint(self) -> ratatui::layout::Constraint {
        match self {
            Self::Fixed(value) => ratatui::layout::Constraint::Length(value),
            Self::Fill(value) => ratatui::layout::Constraint::Min(value),
        }
    }
}

fn device_row(
    app: &App,
    device: &Device,
    selected: bool,
    width: u16,
    local_extended: bool,
    local_traffic: bool,
    admin_extended: bool,
) -> Row<'static> {
    let owner = device
        .owner
        .as_deref()
        .or(device.owner_label.as_deref())
        .map_or("-", |value| value);
    let owner_tags = if device.tags.is_empty() {
        owner.to_owned()
    } else {
        format!("{} · {}", owner, device.tags.join(","))
    };
    let seen = device
        .age_at(app.now)
        .map_or_else(|| "-".to_owned(), format_age);
    let marker = if app.resolved_config.ui.symbols.unicode() {
        match device.liveness {
            Liveness::Online => "●",
            Liveness::Offline => "○",
            Liveness::Unknown => "?",
        }
    } else {
        match device.liveness {
            Liveness::Online => "*",
            Liveness::Offline => "o",
            Liveness::Unknown => "?",
        }
    };
    let name_width = usize::from(width.saturating_sub(45));
    let marker_role = match device.liveness {
        Liveness::Online => theme::StyleRole::StateHealthy,
        Liveness::Offline => theme::StyleRole::StateOffline,
        Liveness::Unknown => theme::StyleRole::StateUnknown,
    };
    let mut cells = vec![
        Cell::from(Span::styled(
            marker.to_owned(),
            app.theme.style(marker_role),
        )),
        Cell::from(text::ellipsize(&device.display_name, name_width.max(8))),
        Cell::from(text::ellipsize(&owner_tags, 22)),
        Cell::from(text::ellipsize(device.os.label(), 9)),
        Cell::from(text::ellipsize(device.path.label(), 11)),
        Cell::from(seen),
    ];
    if app.views.devices.wide_columns {
        cells.push(Cell::from(text::ellipsize(
            &device.addresses.join(", "),
            20,
        )));
        cells.push(Cell::from(text::ellipsize(&device.version, 12)));
    }
    if local_extended {
        cells.push(Cell::from(text::ellipsize(
            &device.advertised_routes.join(", "),
            20,
        )));
        let mut roles = Vec::new();
        if device.capabilities.exit_node {
            roles.push("exit");
        }
        if device.capabilities.exit_node_option {
            roles.push("option");
        }
        if device.capabilities.subnet_router {
            roles.push("router");
        }
        if device.capabilities.ssh {
            roles.push("ssh");
        }
        if device.capabilities.shared {
            roles.push("shared");
        }
        cells.push(Cell::from(text::ellipsize(&roles.join(","), 14)));
    }
    if admin_extended {
        let admin = app.admin.devices.snapshot.as_ref().and_then(|devices| {
            devices.iter().find(|candidate| {
                candidate.stable_id == device.id.0
                    || candidate.exact_node_id() == Some(device.id.0.as_str())
            })
        });
        let approval = admin.map_or_else(
            || "unknown".to_owned(),
            |value| match value.authorized {
                Some(true) => "approved".to_owned(),
                Some(false) => "pending".to_owned(),
                None => "unknown".to_owned(),
            },
        );
        let key = admin.map_or_else(
            || "unknown".to_owned(),
            |value| {
                if value.key_expiry_disabled == Some(true) {
                    "disabled".to_owned()
                } else {
                    value.expires_at.map_or_else(
                        || "unknown".to_owned(),
                        |expires| {
                            if expires <= app.now {
                                "expired".to_owned()
                            } else {
                                expires.to_string()
                            }
                        },
                    )
                }
            },
        );
        let role = admin.map_or_else(
            || "unknown".to_owned(),
            |value| {
                if !value.advertised_routes_returned && !value.enabled_routes_returned {
                    "unknown".to_owned()
                } else {
                    let mut roles = Vec::new();
                    if value
                        .advertised_routes
                        .iter()
                        .any(|route| route == "0.0.0.0/0" || route == "::/0")
                    {
                        roles.push("exit-advert");
                    }
                    if value
                        .enabled_routes
                        .iter()
                        .any(|route| route == "0.0.0.0/0" || route == "::/0")
                    {
                        roles.push("exit-enabled");
                    }
                    if !value.advertised_routes.is_empty() {
                        roles.push("subnet-advert");
                    }
                    if roles.is_empty() {
                        "none".to_owned()
                    } else {
                        roles.join(",")
                    }
                }
            },
        );
        let posture = admin.map_or("unknown", |value| match value.posture_present {
            Some(true) => "present",
            Some(false) => "empty",
            None => "not loaded",
        });
        cells.push(Cell::from(text::ellipsize(&approval, 12)));
        cells.push(Cell::from(text::ellipsize(&key, 12)));
        cells.push(Cell::from(text::ellipsize(&role, 14)));
        cells.push(Cell::from(posture));
    }
    if local_traffic {
        cells.push(Cell::from(format_optional_bytes(device.rx_bytes)));
        cells.push(Cell::from(format_optional_bytes(device.tx_bytes)));
    }
    let row = Row::new(cells);
    if selected {
        row.style(app.theme.style(theme::StyleRole::Selection))
    } else {
        row
    }
}

fn format_age(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3_599 => format!("{}m", seconds / 60),
        3_600..=86_399 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

fn format_optional_bytes(value: Option<u64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
}

/// Route context now lives in the border: what this is, how much of it is
/// showing, and the terms that narrowed it.
fn devices_title(app: &App) -> String {
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
        match app.views.devices.sort.direction {
            crate::domain::device::SortDirection::Ascending => "↑",
            crate::domain::device::SortDirection::Descending => "↓",
        }
    ));
    if app.admin.profile.is_some() {
        detail.push("local + admin".to_owned());
    }
    text::view_title(
        "devices",
        app.visible_indices().len(),
        app.devices_resource.snapshot.len(),
        &detail,
    )
}
