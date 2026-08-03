use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Cell, Row, Table};

use crate::app::App;
use crate::config::SymbolsMode;
use crate::domain::device::{Device, Liveness};
use crate::ui::{text, theme};

pub fn render_devices(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let local_extended = app.source_mode == crate::app::SourceMode::Local
        && app.views.devices.wide_columns
        && area.width >= 120;
    let local_traffic = local_extended && area.width >= 150;
    let visible = app.visible_indices();
    let rows = visible.into_iter().filter_map(|index| {
        let device = app.devices_resource.snapshot.get(index)?;
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
    } else if app.views.devices.wide_columns {
        vec!["S", "NAME", "OWNER/TAGS", "OS", "PATH", "SEEN", "IP", "VER"]
    } else {
        vec!["S", "NAME", "OWNER/TAGS", "OS", "PATH", "SEEN"]
    };
    let header = Row::new(header_values).style(theme::title());
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
            .style(theme::normal(app))
            .block(Block::default().borders(Borders::ALL).title("devices")),
        area,
    );
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
    let marker = if app.resolved_config.ui.symbols == SymbolsMode::Unicode {
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
    let mut cells = vec![
        Cell::from(marker.to_owned()),
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
    if local_traffic {
        cells.push(Cell::from(format_optional_bytes(device.rx_bytes)));
        cells.push(Cell::from(format_optional_bytes(device.tx_bytes)));
    }
    let row = Row::new(cells);
    if selected {
        row.style(theme::selected(app))
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
