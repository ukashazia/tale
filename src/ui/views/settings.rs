use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, List, ListItem};

use crate::app::App;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let items = app.resolved_config.settings().into_iter().map(|setting| {
        ListItem::new(format!(
            "{:<25} {:<32} [{}]",
            setting.name,
            setting.value,
            setting.source.label()
        ))
    });
    frame.render_widget(
        List::new(items).style(theme::normal(app)).block(
            Block::default()
                .borders(Borders::ALL)
                .title("settings · read-only"),
        ),
        area,
    );
}
