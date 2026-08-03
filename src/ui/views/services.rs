use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, Focus};
use crate::domain::service::ServiceSection;
use crate::ui::theme;
use crate::ui::views::{diagnostics, transfers};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, wide_inspector: Option<Rect>) {
    if app.focus == Focus::Inspector || wide_inspector.is_none() {
        if app.focus == Focus::Inspector {
            render_inspector(frame, app, area);
        } else {
            render_collection(frame, app, area);
        }
        return;
    }
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    render_collection(frame, app, horizontal[0]);
    render_inspector(frame, app, wide_inspector.unwrap_or(horizontal[1]));
}

fn render_collection(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = vec![Line::from(
        ServiceSection::ALL
            .iter()
            .map(|section| {
                if *section == app.views.services.section {
                    format!("[{}]", section.label())
                } else {
                    section.label().to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("  "),
    )];
    lines.push(Line::from(format!(
        "section: {} · status: {}",
        app.views.services.section.label(),
        section_status(app),
    )));
    if app.resolved_config.read_only {
        lines.push(Line::from("mode: read-only · local mutations disabled"));
    }
    if let Some(failure) = section_failure(app) {
        lines.push(Line::from(format!(
            "error: {} · {}",
            failure.kind.label(),
            failure.detail
        )));
    }
    match app.views.services.section {
        ServiceSection::Serve => {
            if let Some(status) = app.services_snapshot.serve.value.as_ref() {
                lines.extend(status.mappings.iter().enumerate().map(|(index, mapping)| {
                    Line::from(format!(
                        "{} {}:{}{} → {} · {}",
                        marker(index, app.views.services.selected),
                        mapping.listener.label(),
                        mapping.listener.port(),
                        mapping.mount,
                        mapping.backend.argument(),
                        mapping.exposure.label()
                    ))
                }));
            }
        }
        ServiceSection::Funnel => {
            if let Some(status) = app.services_snapshot.funnel.value.as_ref() {
                lines.extend(status.mappings.iter().enumerate().map(|(index, mapping)| {
                    Line::from(format!(
                        "{} PUBLIC {}:{}{} → {}",
                        marker(index, app.views.services.selected),
                        mapping.listener.label(),
                        mapping.listener.port(),
                        mapping.mount,
                        mapping.backend.argument()
                    ))
                }));
            }
        }
        ServiceSection::Taildrop => {
            lines.extend(transfers::taildrop_lines(app));
        }
        ServiceSection::Taildrive => {
            lines.extend(transfers::taildrive_lines(app));
        }
        ServiceSection::Certificates => {
            if let Some(domains) = app.services_snapshot.certificate_domains.value.as_ref() {
                lines.extend(domains.iter().enumerate().map(|(index, domain)| {
                    Line::from(format!(
                        "{} eligible domain {domain}",
                        marker(index, app.views.services.selected)
                    ))
                }));
            }
        }
        ServiceSection::Metrics => lines.extend(diagnostics::metrics_lines(
            app,
            area.height.saturating_sub(6),
        )),
        ServiceSection::BugReport => lines.extend(diagnostics::bug_report_lines(app)),
    }
    if lines.len() == 2 {
        lines.push(Line::from(section_empty_message(app)));
    }
    lines.push(Line::from(
        "j/k select · [/] section · a actions · Enter inspector",
    ));
    frame.render_widget(
        Paragraph::new(lines).style(theme::normal(app)).block(
            Block::default()
                .borders(Borders::ALL)
                .title("local services"),
        ),
        area,
    );
}

fn render_inspector(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let section = app.views.services.section;
    let capability = capability_text(app, section);
    let mut lines = vec![
        Line::from(format!("section     {}", section.label())),
        Line::from(format!("capability  {capability}")),
        Line::from(format!("source      {}", app.source_mode.label())),
        Line::from(format!(
            "client      {}",
            app.services_snapshot
                .command_version
                .as_deref()
                .unwrap_or("not returned")
        )),
        Line::from(format!(
            "observed    {}",
            app.services_snapshot
                .observed_at
                .map_or_else(|| "not returned".to_owned(), |value| value.to_string())
        )),
    ];
    if let Some(failure) = section_failure(app) {
        lines.push(Line::from(format!(
            "error: {} · {}",
            failure.kind.label(),
            failure.detail
        )));
    }
    match section {
        ServiceSection::Serve => {
            if let Some(mapping) = app.selected_service_mapping() {
                lines.push(Line::from(format!(
                    "exposure    {}",
                    mapping.exposure.label()
                )));
                lines.push(Line::from(format!(
                    "listener    {}:{}",
                    mapping.listener.label(),
                    mapping.listener.port()
                )));
                lines.push(Line::from(format!("mount       {}", mapping.mount)));
                lines.push(Line::from(format!(
                    "backend     {}",
                    mapping.backend.argument()
                )));
                lines.push(Line::from(format!(
                    "proxy       {}",
                    mapping.proxy_protocol.cli_value().unwrap_or("none")
                )));
            }
        }
        ServiceSection::Funnel => {
            if let Some(mapping) = app.selected_service_mapping() {
                lines.push(Line::from("PUBLIC       yes"));
                lines.push(Line::from(format!(
                    "listener    {}:{}",
                    mapping.listener.label(),
                    mapping.listener.port()
                )));
                lines.push(Line::from(format!("mount       {}", mapping.mount)));
                lines.push(Line::from(format!(
                    "backend     {}",
                    mapping.backend.argument()
                )));
            }
        }
        ServiceSection::Taildrop => {
            if let Some(target) = app.selected_taildrop_target() {
                lines.push(Line::from(format!("target      {}", target.command_target)));
                lines.push(Line::from(format!("display     {}", target.display_name)));
                lines.push(Line::from(format!("device      {}", target.device_name)));
                lines.push(Line::from(format!(
                    "online      {}",
                    target
                        .online
                        .map_or("unknown", |value| if value { "yes" } else { "no" })
                )));
                if let Some(reason) = target.capability_reason.as_deref() {
                    lines.push(Line::from(format!("reason      {reason}")));
                }
            } else {
                lines.push(Line::from(
                    "No waiting-file inventory is provided by this contract.",
                ));
            }
            if app.resolved_config.read_only {
                lines.push(Line::from(
                    "mode        read-only · local mutations disabled",
                ));
            }
        }
        ServiceSection::Taildrive => {
            lines.push(Line::from("ALPHA        enabled only for this run"));
            if let Some(share) = app.selected_taildrive_share() {
                lines.push(Line::from(format!("name        {}", share.name)));
                lines.push(Line::from(format!("path        {}", share.path.display())));
                lines.push(Line::from(format!(
                    "as          {}",
                    share.as_user.as_deref().unwrap_or("not returned")
                )));
            }
        }
        ServiceSection::Certificates => {
            if let Some(domain) = app
                .services_snapshot
                .certificate_domains
                .value
                .as_ref()
                .and_then(|domains| domains.get(app.views.services.selected))
            {
                lines.push(Line::from(format!("domain      {domain}")));
            }
            lines.push(Line::from(
                "private-key contents are never rendered, copied, logged, or stored",
            ));
        }
        ServiceSection::Metrics => {
            lines.extend(diagnostics::metrics_lines(
                app,
                area.height.saturating_sub(8),
            ));
        }
        ServiceSection::BugReport => {
            lines.extend(diagnostics::bug_report_lines(app));
        }
    }
    lines.push(Line::from("a actions · Esc collection"));
    frame.render_widget(
        Paragraph::new(lines).style(theme::normal(app)).block(
            Block::default()
                .borders(Borders::ALL)
                .title("service inspector"),
        ),
        area,
    );
}

fn marker(index: usize, selected: usize) -> &'static str {
    if index == selected { ">" } else { " " }
}

fn section_status(app: &App) -> &'static str {
    match app.views.services.section {
        ServiceSection::Serve => app.services_snapshot.serve.status.label(),
        ServiceSection::Funnel => app.services_snapshot.funnel.status.label(),
        ServiceSection::Taildrop => app.services_snapshot.taildrop_targets.status.label(),
        ServiceSection::Taildrive => app.services_snapshot.taildrive.status.label(),
        ServiceSection::Certificates => app.services_snapshot.certificate_domains.status.label(),
        ServiceSection::Metrics => app.services_snapshot.metrics.status.label(),
        ServiceSection::BugReport => app.services_snapshot.bug_report.status.label(),
    }
}

fn section_failure(app: &App) -> Option<&crate::domain::service::ServiceFailure> {
    match app.views.services.section {
        ServiceSection::Serve => app.services_snapshot.serve.failure.as_ref(),
        ServiceSection::Funnel => app.services_snapshot.funnel.failure.as_ref(),
        ServiceSection::Taildrop => app.services_snapshot.taildrop_targets.failure.as_ref(),
        ServiceSection::Taildrive => app.services_snapshot.taildrive.failure.as_ref(),
        ServiceSection::Certificates => app.services_snapshot.certificate_domains.failure.as_ref(),
        ServiceSection::Metrics => app.services_snapshot.metrics.failure.as_ref(),
        ServiceSection::BugReport => app.services_snapshot.bug_report.failure.as_ref(),
    }
}

fn capability_text(app: &App, section: ServiceSection) -> String {
    let state = match section {
        ServiceSection::Serve => &app.services_snapshot.capabilities.serve,
        ServiceSection::Funnel => &app.services_snapshot.capabilities.funnel,
        ServiceSection::Taildrop => &app.services_snapshot.capabilities.taildrop,
        ServiceSection::Taildrive => &app.services_snapshot.capabilities.taildrive,
        ServiceSection::Certificates => &app.services_snapshot.capabilities.certificates,
        ServiceSection::Metrics => &app.services_snapshot.capabilities.metrics,
        ServiceSection::BugReport => &app.services_snapshot.capabilities.bug_report,
    };
    state
        .reason
        .as_deref()
        .unwrap_or(state.status.label())
        .to_owned()
}

fn section_empty_message(app: &App) -> String {
    match app.views.services.section {
        ServiceSection::Taildrop => {
            "No Taildrop targets returned; waiting files are not inventoried.".to_owned()
        }
        ServiceSection::Taildrive if !app.alpha_local_features => {
            "Taildrive is ALPHA and disabled. Use actions to enable it for this run.".to_owned()
        }
        _ => "No rows returned.".to_owned(),
    }
}
