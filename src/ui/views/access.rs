use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::ui::components::panel;
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
                app.theme.style(theme::StyleRole::StateWarning)
            } else if line.contains('"') {
                app.theme.style(theme::StyleRole::Focus)
            } else {
                app.theme.style(theme::StyleRole::TextPrimary)
            };
            Line::from(Span::styled(line.to_owned(), style))
        }));
    } else {
        lines.push(Line::from(resource.error.as_deref().map_or_else(
            || "policy source not returned".to_owned(),
            str::to_owned,
        )));
    }
    lines.push(Line::from(""));
    lines.extend(
        crate::ui::views::access_explorer::summary(app)
            .lines()
            .map(|line| Line::from(line.to_owned())),
    );
    panel::render(
        frame,
        app,
        area,
        "access · preserved HuJSON source · read-only",
        lines,
    );
}
