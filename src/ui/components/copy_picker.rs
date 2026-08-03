use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, CopyPickerState};
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, state: &CopyPickerState) {
    let fields = state
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let prefix = if index == state.selected { ">" } else { " " };
            format!("{prefix} {}", field.label())
        })
        .collect::<Vec<_>>()
        .join("\n");
    let value = app
        .copied_value
        .as_deref()
        .map_or_else(String::new, |value| format!("\n\nselected value:\n{value}"));
    frame.render_widget(
        Paragraph::new(format!("{fields}{value}"))
            .style(theme::normal(app))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("copy field · selectable render"),
            ),
        area,
    );
}
