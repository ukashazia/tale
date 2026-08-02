use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
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
