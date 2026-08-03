use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let resource = &app.admin.policy;
    let mut lines = vec![
        Line::from(format!("state: {}", resource.state.label())),
        Line::from(format!(
            "content-type: {}",
            resource
                .snapshot
                .as_ref()
                .map_or("unknown", |policy| policy.content_type.as_str())
        )),
        Line::from(format!(
            "hash: {}",
            resource
                .snapshot
                .as_ref()
                .map_or("unknown", |policy| policy.content_hash.as_str())
        )),
        Line::from(""),
    ];
    if let Some(source) = resource
        .snapshot
        .as_ref()
        .and_then(|policy| policy.as_str())
    {
        let source = source.chars().take(8_000).collect::<String>();
        lines.extend(source.lines().map(|line| {
            let style = if line.trim_start().starts_with("//") {
                theme::attention(app)
            } else if line.contains('"') {
                theme::focused()
            } else {
                theme::normal(app)
            };
            Line::from(Span::styled(line.to_owned(), style))
        }));
    } else {
        lines.push(Line::from(
            resource
                .error
                .as_deref()
                .map_or("policy source not returned", |value| value),
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).style(theme::normal(app)).block(
            Block::default()
                .borders(Borders::ALL)
                .title("access · preserved HuJSON source · read-only"),
        ),
        area,
    );
}
