use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, List, ListItem};

use crate::app::App;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let items = app.tasks.all().iter().map(|task| {
        let marker = match task.state {
            crate::task::TaskState::Succeeded => "+",
            crate::task::TaskState::Failed => "!",
            crate::task::TaskState::Cancelled => "-",
            crate::task::TaskState::Cancelling => "~",
            crate::task::TaskState::Queued | crate::task::TaskState::Running => "*",
        };
        ListItem::new(format!(
            "{marker} {} {} [{}]",
            task.id,
            task.target_label,
            task.state.label()
        ))
    });
    frame.render_widget(
        List::new(items)
            .style(theme::normal(app))
            .block(Block::default().borders(Borders::ALL).title("task history")),
        area,
    );
}
