use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Paragraph};

use crate::app::{App, Overlay};
use crate::ui::components::{action_picker, command_palette, filter, help};
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, overlay: &Overlay) {
    let area = centered(frame.area(), overlay);
    frame.render_widget(Clear, area);
    match overlay {
        Overlay::CommandPalette(_) => command_palette::render(frame, app, area, overlay),
        Overlay::FilterEditor(_) => filter::render(frame, app, area, overlay),
        Overlay::Help(_) => help::render(frame, app, area, overlay),
        Overlay::ActionPicker(_) => action_picker::render(frame, app, area, overlay),
        Overlay::CopyPicker(state) => {
            let lines = state
                .fields
                .iter()
                .enumerate()
                .map(|(index, field)| {
                    let prefix = if index == state.selected { ">" } else { " " };
                    format!("{prefix} {}", field.label())
                })
                .collect::<Vec<_>>()
                .join("\n");
            frame.render_widget(
                Paragraph::new(lines).style(theme::normal(app)).block(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL)
                        .title("copy field"),
                ),
                area,
            );
        }
        Overlay::QuitConfirmation => frame.render_widget(
            Paragraph::new(
                "Active tasks are still running.\nEnter/y: quit and cancel tasks   n/Esc: continue",
            ),
            area,
        ),
        Overlay::TaskInspector(task_id) => {
            let detail = app.tasks.get(*task_id).map_or_else(
                || "task no longer available".to_owned(),
                |task| format!("{}\n{}\n{}", task.state.label(), task.summary, task.detail),
            );
            frame.render_widget(
                Paragraph::new(detail).style(theme::normal(app)).block(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL)
                        .title("task inspector"),
                ),
                area,
            );
        }
        Overlay::SortPicker { selected } => {
            let fields = [
                "name asc",
                "name desc",
                "state asc",
                "state desc",
                "owner asc",
                "owner desc",
                "os asc",
                "os desc",
                "path asc",
                "path desc",
                "lastSeen asc",
                "lastSeen desc",
                "id asc",
                "id desc",
            ];
            let lines = fields
                .iter()
                .enumerate()
                .map(|(index, field)| {
                    format!("{} {field}", if index == *selected { ">" } else { " " })
                })
                .collect::<Vec<_>>()
                .join("\n");
            frame.render_widget(
                Paragraph::new(lines).style(theme::normal(app)).block(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL)
                        .title("sort"),
                ),
                area,
            );
        }
    }
}

fn centered(area: Rect, overlay: &Overlay) -> Rect {
    let width = match overlay {
        Overlay::Help(_) => area.width.saturating_mul(3) / 4,
        Overlay::CommandPalette(_) | Overlay::FilterEditor(_) => area.width.saturating_mul(2) / 3,
        _ => area.width.saturating_mul(3) / 5,
    }
    .max(20)
    .min(area.width);
    let height = match overlay {
        Overlay::Help(_) => area.height.saturating_mul(3) / 4,
        _ => area.height.saturating_mul(2) / 3,
    }
    .max(5)
    .min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}
