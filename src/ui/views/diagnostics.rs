use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::app::{App, DiagnosticsSection};
use crate::domain::service::ServiceResourceStatus;
use crate::ui::components::{panel, tabs};
use crate::ui::text;
use crate::ui::theme;

/// Metrics and the bug report identifier. These are diagnostics, not services:
/// nothing here is something the tailnet offers, it is evidence about this
/// machine to hand to someone else.
pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    match app.views.diagnostics.section {
        DiagnosticsSection::Client => render_client(frame, app, area),
        DiagnosticsSection::DnsStatus => render_dns_status(frame, app, area),
    }
}

fn render_client(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = vec![
        tab_line(app),
        Line::default(),
        Line::from(Span::styled(
            "Bug report",
            app.theme.style(theme::StyleRole::SectionHeading),
        )),
    ];
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

fn render_dns_status(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = vec![tab_line(app), Line::default()];
    if let Some(status) = super::dns::latest_local_status(app) {
        lines.extend(super::dns::local_status_lines(app, status));
    } else if app.dns_status_is_loading() {
        lines.push(Line::from(Span::styled(
            "Reading DNS status…",
            app.theme.style(theme::StyleRole::TextMuted),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Waiting for the local client to become ready.",
            app.theme.style(theme::StyleRole::TextMuted),
        )));
    }
    panel::render_wrapped(frame, app, area, " diagnostics ", lines);
}

fn tab_line(app: &App) -> Line<'static> {
    tabs::line(
        app,
        [
            (
                "Client",
                app.views.diagnostics.section == DiagnosticsSection::Client,
            ),
            (
                "DNS status",
                app.views.diagnostics.section == DiagnosticsSection::DnsStatus,
            ),
        ],
    )
}

pub fn metrics_lines(app: &App, viewport: u16) -> Vec<Line<'static>> {
    let Some(metrics) = app.services_snapshot.metrics.value.as_ref() else {
        return match app.services_snapshot.metrics.status {
            ServiceResourceStatus::Loading => vec![muted(app, "Reading metrics…")],
            ServiceResourceStatus::Failed => vec![
                muted(app, "Reading metrics failed"),
                text::action_hint(app.theme, "  retry                  ", "r"),
            ],
            _ => vec![muted(app, "Waiting for the local client to become ready.")],
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
                text::action_hint(app.theme, "  create one             ", "a c"),
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
