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
            app.theme,
            "access policy",
            "access",
            app.admin.profile.is_some(),
            resource.state,
            resource.error.as_deref(),
        );
        panel::render(frame, app, area, "access · policy", lines);
        return;
    };
    let mut lines = document_lines(app, policy);
    let matches = search_matches(app, &app.detail_search);
    for index in &matches {
        let Some(line) = lines.get_mut(*index) else {
            continue;
        };
        let role = if app.detail_search_match == Some(*index) {
            theme::StyleRole::Selection
        } else {
            theme::StyleRole::CompletionMatch
        };
        let style = app.theme.style(role);
        line.style = line.style.patch(style);
        for span in &mut line.spans {
            span.style = span.style.patch(style);
        }
    }
    let visible = usize::from(area.height.saturating_sub(2)).max(1);
    let max_scroll = lines.len().saturating_sub(visible);
    let scroll = app.detail_scroll.min(max_scroll);
    let end = scroll.saturating_add(visible).min(lines.len());
    let position = app.detail_search_match.and_then(|line| {
        matches
            .iter()
            .position(|candidate| *candidate == line)
            .map(|position| position.saturating_add(1))
    });
    let search = if app.detail_search.is_empty() {
        String::new()
    } else {
        format!(
            " · match {}/{} · /{}",
            position.map_or(0, |value| value),
            matches.len(),
            app.detail_search
        )
    };
    let title = if max_scroll == 0 {
        format!("access · policy{search}")
    } else {
        format!(
            "access · policy · {}-{} of {}{search}",
            scroll.saturating_add(1),
            end,
            lines.len()
        )
    };
    let scroll = u16::try_from(scroll).map_or(u16::MAX, |value| value);
    panel::render_scrolled(frame, app, area, &title, lines, scroll);
}

pub fn line_count(app: &App) -> usize {
    app.admin
        .policy
        .snapshot
        .as_ref()
        .map_or(0, |policy| document_lines(app, policy).len())
}

pub fn search_matches(app: &App, query: &str) -> Vec<usize> {
    let Some(policy) = app.admin.policy.snapshot.as_ref() else {
        return Vec::new();
    };
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let offset = document_prefix(app, policy).len();
    source_lines(app, policy)
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            line_text(line)
                .to_ascii_lowercase()
                .contains(&query)
                .then_some(offset.saturating_add(index))
        })
        .collect()
}

fn document_lines(app: &App, policy: &crate::domain::policy::PolicySnapshot) -> Vec<Line<'static>> {
    let mut lines = document_prefix(app, policy);
    lines.extend(source_lines(app, policy));
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
    lines
}

fn document_prefix(
    app: &App,
    policy: &crate::domain::policy::PolicySnapshot,
) -> Vec<Line<'static>> {
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
    lines.push(Line::from(Span::styled(
        "Policy source · read-only",
        app.theme.style(theme::StyleRole::SectionHeading),
    )));
    lines
}

fn source_lines(app: &App, policy: &crate::domain::policy::PolicySnapshot) -> Vec<Line<'static>> {
    let Some(source) = policy.as_str() else {
        return vec![Line::from(Span::styled(
            "The policy source is not valid UTF-8.",
            app.theme.style(theme::StyleRole::StateDanger),
        ))];
    };
    source
        .lines()
        .map(|line| {
            let role = if line.trim_start().starts_with("//") {
                theme::StyleRole::StateWarning
            } else if line.contains('"') {
                theme::StyleRole::Focus
            } else {
                theme::StyleRole::TextPrimary
            };
            Line::from(Span::styled(line.to_owned(), app.theme.style(role)))
        })
        .collect()
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}
