use ratatui::style::Style;
use ratatui::text::Text;
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn paragraph<'a>(content: impl Into<Text<'a>>, title: &'a str, style: Style) -> Paragraph<'a> {
    Paragraph::new(content)
        .style(style)
        .block(Block::default().borders(Borders::ALL).title(title))
}
