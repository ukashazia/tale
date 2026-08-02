use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Cell, Row, Table};

use crate::app::App;
use crate::config::SymbolsMode;
use crate::domain::device::{Device, Liveness};
use crate::ui::{text, theme};

pub fn render_devices(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let visible = app.visible_indices();
    let rows = visible.into_iter().filter_map(|index| {
        let device = app.devices_resource.snapshot.get(index)?;
        let selected = app
            .views
            .devices
            .selected_id
            .as_ref()
            .is_some_and(|id| id == &device.id);
        Some(device_row(app, device, selected, area.width))
    });
    let header = Row::new(["S", "NAME", "OWNER", "OS", "PATH", "SEEN"]).style(theme::title());
    let widths = if app.views.devices.wide_columns {
        [
            ConstraintWidth::Fixed(2),
            ConstraintWidth::Fill(15),
            ConstraintWidth::Fill(10),
            ConstraintWidth::Fill(7),
            ConstraintWidth::Fill(9),
            ConstraintWidth::Fill(6),
        ]
    } else {
        [
            ConstraintWidth::Fixed(2),
            ConstraintWidth::Fill(13),
            ConstraintWidth::Fill(10),
            ConstraintWidth::Fill(7),
            ConstraintWidth::Fill(9),
            ConstraintWidth::Fill(6),
        ]
    };
    let constraints = widths.map(|width| width.constraint());
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

fn device_row(app: &App, device: &Device, selected: bool, width: u16) -> Row<'static> {
    let owner = device
        .owner
        .as_deref()
        .or(device.owner_label.as_deref())
        .map_or("-", |value| value);
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
    let row = Row::new([
        Cell::from(marker.to_owned()),
        Cell::from(text::ellipsize(&device.display_name, name_width.max(8))),
        Cell::from(text::ellipsize(owner, 17)),
        Cell::from(text::ellipsize(device.os.label(), 9)),
        Cell::from(text::ellipsize(device.path.label(), 11)),
        Cell::from(seen),
    ]);
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
