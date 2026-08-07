use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::{App, Focus};
use crate::ui::components::{inspector, table};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, wide_inspector: Option<Rect>) {
    if app.focus == Focus::Inspector || wide_inspector.is_none() {
        // `i` hides the side pane, so a narrow terminal is not the only reason
        // the table can have the whole width.
        if app.focus == Focus::Inspector {
            inspector::render(frame, app, area);
        } else {
            table::render_devices(frame, app, area);
        }
        return;
    }
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    table::render_devices(frame, app, horizontal[0]);
    if let Some(inspector_area) = wide_inspector {
        inspector::render(frame, app, inspector_area);
    } else {
        inspector::render(frame, app, horizontal[1]);
    }
}
