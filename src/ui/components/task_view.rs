use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};

use crate::app::App;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let items = app.tasks.filtered(&app.task_filter).map(|task| {
        let marker = match task.state {
            crate::task::TaskState::Succeeded => "+",
            crate::task::TaskState::Failed => "!",
            crate::task::TaskState::Cancelled => "-",
            crate::task::TaskState::Cancelling => "~",
            crate::task::TaskState::Queued | crate::task::TaskState::Running => "*",
        };
        let role = match task.state {
            crate::task::TaskState::Succeeded => theme::StyleRole::TaskSucceeded,
            crate::task::TaskState::Failed => theme::StyleRole::TaskFailed,
            crate::task::TaskState::Cancelled => theme::StyleRole::TaskCancelled,
            crate::task::TaskState::Cancelling => theme::StyleRole::TaskRunning,
            crate::task::TaskState::Queued => theme::StyleRole::TaskQueued,
            crate::task::TaskState::Running => theme::StyleRole::TaskRunning,
        };
        ListItem::new(Line::from(vec![
            Span::styled(
                format!("{marker} {}", task.state.label()),
                app.theme.style(role),
            ),
            Span::raw(format!("  {} {}", task.id, task.target_label)),
        ]))
    });
    let title = if app.task_filter.is_empty() {
        "task history".to_owned()
    } else {
        format!("task history · filter={}", app.task_filter)
    };
    frame.render_widget(
        List::new(items)
            .style(app.theme.style(theme::StyleRole::Surface))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(app.theme.style(theme::StyleRole::BorderNormal))
                    .title(title),
            ),
        area,
    );
}
