use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::App;
use crate::ui::components::{inspector, task_view};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let regions = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    task_view::render(frame, app, regions[0]);
    if let Some(task) = app.focused_task() {
        let detail = format!("{}\n{}\n{}", task.state.label(), task.summary, task.detail);
        frame.render_widget(
            ratatui::widgets::Paragraph::new(detail).block(
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title("task detail"),
            ),
            regions[1],
        );
    } else {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(
                "Select a task with j/k; x cancels a cancellable task",
            )
            .block(
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title("task detail"),
            ),
            regions[1],
        );
    }
    let _ = inspector::render;
}
