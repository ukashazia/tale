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
        "Policy editor",
        app.theme.style(theme::StyleRole::SectionHeading),
    )));
    lines.push(Line::from(vec![
        Span::styled("e", app.theme.style(theme::StyleRole::Focus)),
        Span::styled(
            "  Open the exact fetched HuJSON in $EDITOR",
            app.theme.style(theme::StyleRole::TextPrimary),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "Tale suspends this screen while the editor owns the terminal. Search, scroll, and edit there.",
        app.theme.style(theme::StyleRole::TextMuted),
    )));
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
    panel::render(frame, app, area, "access · policy", lines);
}
