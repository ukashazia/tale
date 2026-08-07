use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::domain::service::ServiceResourceStatus;
use crate::ui::components::panel;
use crate::ui::text;
use crate::ui::theme;

/// Metrics and the bug report identifier. These are diagnostics, not services:
/// nothing here is something the tailnet offers, it is evidence about this
/// machine to hand to someone else.
pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = vec![Line::from(Span::styled(
        "Bug report",
        app.theme.style(theme::StyleRole::SectionHeading),
    ))];
    lines.extend(bug_report_lines(app));
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "Client metrics",
        app.theme.style(theme::StyleRole::SectionHeading),
    )));
    let used = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    lines.extend(metrics_lines(
        app,
        area.height.saturating_sub(used).saturating_sub(2),
    ));
    panel::render(frame, app, area, " diagnostics ", lines);
}

pub fn metrics_lines(app: &App, viewport: u16) -> Vec<Line<'static>> {
    let Some(metrics) = app.services_snapshot.metrics.value.as_ref() else {
        return match app.services_snapshot.metrics.status {
            ServiceResourceStatus::Loading => vec![muted(app, "Reading metrics…")],
            ServiceResourceStatus::Failed => vec![
                muted(app, "Reading metrics failed"),
                muted(app, "  retry                  r"),
            ],
            _ => vec![
                muted(app, "Metrics are read on request"),
                muted(app, "  read now               a m"),
            ],
        };
    };
    if metrics.text.is_empty() {
        return vec![muted(app, "The client returned no metrics.")];
    }
    let source = metrics.text.lines().collect::<Vec<_>>();
    let start = app
        .views
        .diagnostics
        .scroll
        .min(source.len().saturating_sub(1));
    let notice_lines = usize::from(metrics.truncated);
    let body_limit = usize::from(viewport).saturating_sub(notice_lines).max(1);
    let mut lines = source
        .iter()
        .skip(start)
        .take(body_limit)
        .map(|line| {
            Line::from(Span::styled(
                (*line).to_owned(),
                app.theme.style(theme::StyleRole::TextPrimary),
            ))
        })
        .collect::<Vec<_>>();
    if metrics.truncated {
        lines.push(Line::from(Span::styled(
            "Output was cut off at the capture limit.",
            app.theme.style(theme::StyleRole::StateStale),
        )));
    }
    lines
}

pub fn bug_report_lines(app: &App) -> Vec<Line<'static>> {
    app.services_snapshot.bug_report.value.as_ref().map_or_else(
        || {
            vec![
                muted(app, "No report created yet"),
                muted(app, "  create one             a c"),
            ]
        },
        |report| {
            vec![
                Line::from(vec![
                    Span::styled(
                        text::pad_or_trim("identifier", 15),
                        app.theme.style(theme::StyleRole::TextMuted),
                    ),
                    Span::styled(
                        report.identifier.clone(),
                        app.theme.style(theme::StyleRole::TextPrimary),
                    ),
                ]),
                muted(
                    app,
                    "Share this with Tailscale support. Nothing was uploaded.",
                ),
            ]
        },
    )
}

fn muted(app: &App, value: &str) -> Line<'static> {
    Line::from(Span::styled(
        value.to_owned(),
        app.theme.style(theme::StyleRole::TextMuted),
    ))
}
