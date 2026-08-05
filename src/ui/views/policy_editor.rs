use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(workflow) = app.policy_workflow.as_ref() else {
        frame.render_widget(
            Paragraph::new("? no policy workflow is open")
                .style(app.theme.style(theme::StyleRole::StateUnknown)),
            area,
        );
        return;
    };
    let summary = workflow.summary();
    let mut lines = vec![
        format!("workflow: {}", summary.workflow_id),
        format!("state: {}", summary.state.label()),
        format!("profile: {}", summary.profile),
        format!("tailnet: {}", summary.tailnet),
        format!(
            "base hash: {}",
            summary
                .base_hash
                .as_deref()
                .map_or("not returned", |value| value)
        ),
        format!(
            "candidate hash: {}",
            summary
                .candidate_hash
                .as_deref()
                .map_or("not returned", |value| value)
        ),
        format!(
            "latest remote hash: {}",
            summary
                .latest_remote_hash
                .as_deref()
                .map_or("not returned", |value| value)
        ),
        format!("candidate bytes: {}", summary.candidate_bytes),
        format!(
            "temporary path: {}",
            summary
                .candidate_path
                .as_deref()
                .map_or("not returned".to_owned(), |value| value
                    .display()
                    .to_string())
        ),
        format!(
            "latest remote path: {}",
            summary
                .latest_remote_path
                .as_deref()
                .map_or("not retained".to_owned(), |value| value
                    .display()
                    .to_string())
        ),
        format!(
            "server validation bound: {}",
            summary.validation_bound_to_candidate
        ),
        format!(
            "server permission preview bound: {}",
            summary.preview_bound_to_candidate
        ),
        String::new(),
        "e reopen editor   v validate   p preview   d diff   a apply   x discard   Esc close"
            .to_owned(),
    ];
    if let Some(validation) = workflow.validation() {
        lines.push(format!(
            "server diagnostics: {}",
            validation.diagnostics.len()
        ));
        lines.push(format!(
            "server validation: {}",
            if validation.valid { "passed" } else { "failed" }
        ));
        lines.push(format!(
            "server tests: {}/{} passed",
            validation
                .server_tests
                .iter()
                .filter(|test| test.passed)
                .count(),
            validation.server_tests.len()
        ));
        if let Some(detail) = validation.bounded_safe_detail.as_deref() {
            lines.push(format!("server detail: {detail}"));
        }
        lines.extend(validation.diagnostics.iter().take(100).map(|diagnostic| {
            let location = match (diagnostic.line, diagnostic.column) {
                (Some(line), Some(column)) => format!(" at {line}:{column}"),
                (Some(line), None) => format!(" at line {line}"),
                _ => String::new(),
            };
            format!(
                "diagnostic{} {} {}",
                location,
                diagnostic.severity.as_deref().map_or("", |value| value),
                if diagnostic.message.is_empty() {
                    "server diagnostic"
                } else {
                    diagnostic.message.as_str()
                }
            )
        }));
        lines.extend(validation.server_tests.iter().take(100).map(|test| {
            format!(
                "test {}: {}{}",
                test.name,
                if test.passed { "passed" } else { "failed" },
                test.message
                    .as_deref()
                    .map_or(String::new(), |value| format!(" · {value}"))
            )
        }));
    }
    if let Some(preview) = workflow.preview() {
        lines.push(format!(
            "preview selector: {} {}",
            preview.selector_type.api_value(),
            preview.selector
        ));
        lines.push(format!("preview matches: {}", preview.matches.len()));
        lines.push(
            "preview shows only Tailscale-returned matches; runtime health and local reachability are not evaluated"
                .to_owned(),
        );
        if preview.matches.is_empty() {
            lines.push("no server match was returned; Tale does not infer a local deny".to_owned());
        } else {
            lines.extend(preview.matches.iter().take(100).map(|item| {
                format!(
                    "preview match: users={} ports={} line={}",
                    if item.users.is_empty() {
                        "not returned".to_owned()
                    } else {
                        item.users.join(",")
                    },
                    if item.ports.is_empty() {
                        "not returned".to_owned()
                    } else {
                        item.ports.join(",")
                    },
                    item.line_number
                        .map_or_else(|| "not returned".to_owned(), |value| value.to_string())
                )
            }));
        }
    }
    if let Some(diff) = workflow.diff() {
        lines.push(format!("diff: +{} -{}", diff.additions, diff.removals));
        lines.push(String::new());
        lines.extend(diff.text.lines().take(200).map(str::to_owned));
    }
    let lines = lines.into_iter().map(|line| {
        let role = if line.starts_with('+') {
            theme::StyleRole::DiffAdded
        } else if line.starts_with('-') {
            theme::StyleRole::DiffRemoved
        } else if line.starts_with("diff:") {
            theme::StyleRole::DiffChanged
        } else if line.contains("failed") || line.starts_with("diagnostic") {
            theme::StyleRole::StateDanger
        } else {
            theme::StyleRole::TextPrimary
        };
        Line::from(Span::styled(line, app.theme.style(role)))
    });
    frame.render_widget(
        Paragraph::new(lines.collect::<Vec<_>>())
            .style(app.theme.style(theme::StyleRole::SurfaceInset))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("policy workflow · server authoritative"),
            ),
        area,
    );
}
