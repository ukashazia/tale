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
    if let Some(workflow) = app.policy_workflow.as_ref() {
        let summary = workflow.summary();
        lines.push(Line::from(Span::styled(
            "Pending change",
            app.theme.style(theme::StyleRole::SectionHeading),
        )));
        lines.extend(grid::detail(
            app,
            &[
                ("state", summary.state.label().to_owned()),
                (
                    "candidate",
                    summary
                        .candidate_hash
                        .unwrap_or_else(|| "not ready".to_owned()),
                ),
                (
                    "validation",
                    workflow.validation().map_or_else(
                        || "not run".to_owned(),
                        |validation| {
                            if validation.valid {
                                "passed".to_owned()
                            } else {
                                "failed".to_owned()
                            }
                        },
                    ),
                ),
                (
                    "preview",
                    workflow.preview().map_or_else(
                        || "not run".to_owned(),
                        |preview| format!("{} matches", preview.matches.len()),
                    ),
                ),
                (
                    "diff",
                    workflow.diff().map_or_else(
                        || "not generated".to_owned(),
                        |diff| format!("+{} -{}", diff.additions, diff.removals),
                    ),
                ),
            ],
        ));
        lines.push(Line::default());
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
    panel::render(frame, app, area, "access · policy", lines);
}
