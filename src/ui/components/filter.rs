use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, Overlay};
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, overlay: &Overlay) {
    let Overlay::FilterEditor(state) = overlay else {
        return;
    };
    let error = state.error.as_deref().map_or("", |value| value);
    frame.render_widget(
        Paragraph::new(format!("/{}\n{}", state.input, error))
            .style(theme::normal(app))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("filter devices"),
            ),
        area,
    );
}
