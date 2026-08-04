use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, Overlay};
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, overlay: &Overlay) {
    let Overlay::CommandPalette(state) = overlay else {
        return;
    };
    let candidates = state
        .candidates
        .iter()
        .map(|route| route.label())
        .map(str::to_owned)
        .chain(state.saved_views.iter().map(|name| format!("view:{name}")))
        .collect::<Vec<_>>()
        .join("  ");
    let error = state.error.as_deref().map_or("", |value| value);
    let text = format!(":{}\n{}\n{}", state.input, candidates, error);
    frame.render_widget(
        Paragraph::new(text).style(theme::normal(app)).block(
            Block::default()
                .borders(Borders::ALL)
                .title("command palette"),
        ),
        area,
    );
}
