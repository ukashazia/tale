use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::ui::components::{grid, panel};
use crate::ui::{text, theme};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let resource = &app.admin.policy;
    let Some(policy) = resource.snapshot.as_ref() else {
        let lines = text::empty_state(
            "access policy",
            "access",
            app.admin.profile.is_some(),
            resource.state,
            resource.error.as_deref(),
        )
        .into_iter()
        .map(|line| {
            Line::from(Span::styled(
                line,
                app.theme.style(theme::StyleRole::TextMuted),
            ))
        })
        .collect::<Vec<_>>();
        panel::render(frame, app, area, "access · policy", lines);
        return;
    };
    let mut lines = grid::detail(
        app,
        &[
            ("format", policy.content_type.clone()),
            ("hash", policy.content_hash.clone()),
            ("fetched", text::format_timestamp(policy.fetched_at)),
        ],
    );
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "Policy source",
        app.theme.style(theme::StyleRole::SectionHeading),
    )));
    if let Some(source) = policy.as_str() {
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
        lines.push(Line::from(Span::styled(
            "The policy source is not valid UTF-8.",
            app.theme.style(theme::StyleRole::StateDanger),
        )));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "Access Explorer",
        app.theme.style(theme::StyleRole::SectionHeading),
    )));
    lines.extend(
        crate::ui::views::access_explorer::summary(app)
            .lines()
            .map(|line| Line::from(line.to_owned())),
    );
    panel::render(
        frame,
        app,
        area,
        "access · preserved source · read-only",
        lines,
    );
}
