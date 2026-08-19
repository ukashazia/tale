use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::action::{self, ActionId};
use crate::app::{App, PolicyWorkflowView};
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
    let visible = viewport_height(area.height);
    let visual_lines = panel::wrapped_line_count(lines.clone(), area.width.saturating_sub(4));
    let max_scroll = visual_lines.saturating_sub(visible);
    let scroll = app.detail_scroll.min(max_scroll);
    let end = scroll.saturating_add(visible).min(visual_lines);
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
            position.unwrap_or(0),
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
            visual_lines
        )
    };
    let scroll = u16::try_from(scroll).unwrap_or(u16::MAX);
    panel::render_scrolled(frame, app, area, &title, lines, scroll);
}

pub fn line_count(app: &App, area_width: u16) -> usize {
    let lines = app
        .admin
        .policy
        .snapshot
        .as_ref()
        .map_or_else(Vec::new, |policy| document_lines(app, policy));
    panel::wrapped_line_count(lines, area_width.saturating_sub(4))
}

pub fn max_scroll(app: &App, area_width: u16, area_height: u16) -> usize {
    line_count(app, area_width).saturating_sub(viewport_height(area_height))
}

fn viewport_height(area_height: u16) -> usize {
    usize::from(area_height.saturating_sub(2)).max(1)
}

pub fn search_matches(app: &App, query: &str) -> Vec<usize> {
    let Some(policy) = app.admin.policy.snapshot.as_ref() else {
        return Vec::new();
    };
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    if app
        .policy_workflow
        .as_ref()
        .is_some_and(crate::domain::policy_workflow::PolicyWorkflow::has_candidate_changes)
    {
        return workflow_lines(app)
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                line_text(line)
                    .to_ascii_lowercase()
                    .contains(&query)
                    .then_some(index)
            })
            .collect();
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
    if app
        .policy_workflow
        .as_ref()
        .is_some_and(crate::domain::policy_workflow::PolicyWorkflow::has_candidate_changes)
    {
        return workflow_lines(app);
    }
    let mut lines = document_prefix(app, policy);
    lines.extend(source_lines(app, policy));
    lines
}

fn workflow_lines(app: &App) -> Vec<Line<'static>> {
    let Some(workflow) = app.policy_workflow.as_ref() else {
        return Vec::new();
    };
    let summary = workflow.summary();
    let mut lines = grid::detail(
        app,
        &[
            ("state", summary.state.label().to_owned()),
            (
                "base",
                summary
                    .base_hash
                    .unwrap_or_else(|| "unavailable".to_owned()),
            ),
            (
                "candidate",
                summary
                    .candidate_hash
                    .unwrap_or_else(|| "unavailable".to_owned()),
            ),
            ("candidate bytes", summary.candidate_bytes.to_string()),
        ],
    );
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "Policy actions",
        app.theme.style(theme::StyleRole::SectionHeading),
    )));
    for action_id in policy_actions() {
        let Some(spec) = action::find_action(action_id) else {
            continue;
        };
        let key = action::transient_sequence(action_id).map_or("a", |sequence| sequence);
        let available = app.action_unavailable_reason(action_id).is_none();
        let role = if available {
            theme::StyleRole::TextPrimary
        } else {
            theme::StyleRole::TextMuted
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{key:<3}"),
                app.theme.style(theme::StyleRole::Focus),
            ),
            Span::styled(spec.label, app.theme.style(role)),
        ]));
    }
    lines.push(Line::default());
    if app.policy_workflow_view == PolicyWorkflowView::Diff
        && let Some(diff) = workflow.diff()
    {
        lines.push(Line::from(Span::styled(
            format!("Policy diff · +{} -{}", diff.additions, diff.removals),
            app.theme.style(theme::StyleRole::SectionHeading),
        )));
        lines.extend(diff.text.lines().map(|line| {
            let role = if line.starts_with('+') && !line.starts_with("+++") {
                theme::StyleRole::DiffAdded
            } else if line.starts_with('-') && !line.starts_with("---") {
                theme::StyleRole::StateDanger
            } else {
                theme::StyleRole::TextPrimary
            };
            Line::from(Span::styled(line.to_owned(), app.theme.style(role)))
        }));
    } else if app.policy_workflow_view == PolicyWorkflowView::Validation
        && let Some(validation) = workflow.validation()
    {
        lines.push(Line::from(Span::styled(
            if validation.valid {
                "Validation passed"
            } else {
                "Validation failed"
            },
            app.theme.style(if validation.valid {
                theme::StyleRole::StateHealthy
            } else {
                theme::StyleRole::StateDanger
            }),
        )));
        if let Some(message) = validation.message.as_ref() {
            lines.push(Line::from(message.to_owned()));
        }
        for diagnostic in &validation.diagnostics {
            lines.push(Line::from(diagnostic.message.clone()));
        }
    } else if app.policy_workflow_view == PolicyWorkflowView::Preview
        && let Some(preview) = workflow.preview()
    {
        lines.push(Line::from(Span::styled(
            format!(
                "Permission preview · {} {} · {} matches",
                preview.selector_type.api_value(),
                preview.selector,
                preview.matches.len()
            ),
            app.theme.style(theme::StyleRole::SectionHeading),
        )));
        for item in &preview.matches {
            lines.push(Line::from(format!(
                "users: {} · ports: {} · line: {}",
                item.users.join(", "),
                item.ports.join(", "),
                item.line_number
                    .map_or_else(|| "—".to_owned(), |line| line.to_string())
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "Choose an action above. The fetched policy source is hidden while this candidate is pending.",
            app.theme.style(theme::StyleRole::TextMuted),
        )));
    }
    lines
}

const fn policy_actions() -> [ActionId; 8] {
    [
        ActionId::AdminPolicyEditorReopen,
        ActionId::AdminPolicyRemoteRefresh,
        ActionId::AdminPolicyValidate,
        ActionId::AdminPolicyPreview,
        ActionId::AdminPolicyDiff,
        ActionId::AdminPolicyApply,
        ActionId::AdminPolicyCandidateDiscard,
        ActionId::AdminPolicyWorkflowClose,
    ]
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
        "Access Explorer",
        app.theme.style(theme::StyleRole::SectionHeading),
    )));
    lines.extend(
        crate::ui::views::access_explorer::summary(app)
            .lines()
            .map(|line| Line::from(line.to_owned())),
    );
    lines.push(Line::default());
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
    format_policy_source(source)
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

fn format_policy_source(source: &str) -> String {
    match fjson::to_jsonc(source) {
        Ok(formatted) => formatted,
        Err(_) => source.to_owned(),
    }
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::format_policy_source;

    #[test]
    fn formats_hujson_for_display_without_losing_comments() {
        let source = r#"// policy
{"groups":{"group:ops":["alice@example.com","bob@example.com"],},"acls":[{"action":"accept","src":["*"],"dst":["*:*"],},],}"#;

        let formatted = format_policy_source(source);

        assert_eq!(
            formatted,
            r#"// policy
{
  "groups": {
    "group:ops": ["alice@example.com", "bob@example.com"]
  },
  "acls": [
    {
      "action": "accept",
      "src": ["*"],
      "dst": ["*:*"]
    }
  ]
}
"#
        );
    }

    #[test]
    fn leaves_invalid_hujson_visible_verbatim() {
        let source = "{ not valid yet";

        assert_eq!(format_policy_source(source), source);
    }
}
