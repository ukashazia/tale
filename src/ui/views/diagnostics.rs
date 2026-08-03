use ratatui::text::Line;

use crate::app::App;

pub fn metrics_lines(app: &App, viewport: u16) -> Vec<Line<'static>> {
    let Some(metrics) = app.services_snapshot.metrics.value.as_ref() else {
        return vec![Line::from("No metrics capture yet.")];
    };
    if metrics.text.is_empty() {
        return vec![Line::from("Metrics command returned empty output.")];
    }
    let source = metrics.text.lines().collect::<Vec<_>>();
    let start = app
        .views
        .services
        .scroll
        .min(source.len().saturating_sub(1));
    let notice_lines = usize::from(metrics.truncated);
    let body_limit = usize::from(viewport).saturating_sub(notice_lines).max(1);
    let mut lines = source
        .iter()
        .skip(start)
        .take(body_limit)
        .map(|line| Line::from((*line).to_owned()))
        .collect::<Vec<_>>();
    if metrics.truncated {
        lines.push(Line::from(
            "NOTICE: metrics output was truncated at the task cap.",
        ));
    }
    lines
}

pub fn bug_report_lines(app: &App) -> Vec<Line<'static>> {
    app.services_snapshot.bug_report.value.as_ref().map_or_else(
        || vec![Line::from("No bug report created yet.")],
        |report| {
            vec![
                Line::from("Tailscale diagnostic report identifier:"),
                Line::from(report.identifier.clone()),
                Line::from("Not copied, uploaded, or shared automatically."),
            ]
        },
    )
}
