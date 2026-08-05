use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Paragraph};

use crate::app::{App, Overlay};
use crate::ui::components::{batch_result, confirm, form};
use crate::ui::theme::{StyleRole, ThemeId};
use crate::ui::views::{audit, policy_editor, secret_result};

pub fn render(frame: &mut Frame<'_>, app: &App, overlay: &Overlay) {
    let screen = frame.area();
    frame.render_widget(
        ratatui::widgets::Block::default().style(app.theme.style(StyleRole::Backdrop)),
        screen,
    );
    let area = overlay_area(frame.area(), overlay);
    frame.render_widget(Clear, area);
    match overlay {
        Overlay::QuitConfirmation => frame.render_widget(
            Paragraph::new(
                "Active tasks are still running.\nEnter/y: quit and cancel tasks   n/Esc: continue",
            )
            .style(app.theme.style(StyleRole::RiskDestructive)),
            area,
        ),
        Overlay::TaskInspector(task_id) => {
            if let Some(batch) = app.admin_batch_results.get(task_id) {
                batch_result::render(frame, app, area, batch);
                return;
            }
            let detail = app.tasks.get(*task_id).map_or_else(
                || "task no longer available".to_owned(),
                |task| format!("{}\n{}\n{}", task.state.label(), task.summary, task.detail),
            );
            frame.render_widget(
                Paragraph::new(detail)
                    .style(app.theme.style(StyleRole::SurfaceRaised))
                    .block(
                        ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL)
                            .title("task inspector"),
                    ),
                area,
            );
        }
        Overlay::SortPicker { selected } => {
            let fields = [
                "name asc",
                "name desc",
                "state asc",
                "state desc",
                "owner asc",
                "owner desc",
                "os asc",
                "os desc",
                "path asc",
                "path desc",
                "lastSeen asc",
                "lastSeen desc",
                "rx asc",
                "rx desc",
                "tx asc",
                "tx desc",
                "id asc",
                "id desc",
                "version asc",
                "version desc",
            ];
            let lines = fields
                .iter()
                .enumerate()
                .map(|(index, field)| {
                    format!("{} {field}", if index == *selected { ">" } else { " " })
                })
                .collect::<Vec<_>>()
                .join("\n");
            frame.render_widget(
                Paragraph::new(lines)
                    .style(app.theme.style(StyleRole::SurfaceRaised))
                    .block(
                        ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL)
                            .title("sort"),
                    ),
                area,
            );
        }
        Overlay::DiagnosticInput(state) => {
            let label = match &state.kind {
                crate::app::DiagnosticInputKind::DnsQuery => "DNS name and optional type",
                crate::app::DiagnosticInputKind::Whois => {
                    "IP address or IP:port and optional tcp/udp"
                }
            };
            let error = state
                .error
                .as_deref()
                .map_or(String::new(), |value| format!("\nerror: {value}"));
            frame.render_widget(
                Paragraph::new(format!("{label}\n> {}{}", state.input, error))
                    .style(app.theme.style(StyleRole::SurfaceRaised))
                    .block(
                        ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL)
                            .title("diagnostic input"),
                    ),
                area,
            );
        }
        Overlay::Confirmation(state) => confirm::render(frame, app, area, state),
        Overlay::OperatorForm(state) => form::render_operator(frame, app, area, state),
        Overlay::ServiceForm(state) => form::render_service(frame, app, area, state),
        Overlay::ServiceSectionPicker(state) => {
            let lines = crate::domain::service::ServiceSection::ALL
                .iter()
                .enumerate()
                .map(|(index, section)| {
                    format!(
                        "{} {}",
                        if index == state.selected { ">" } else { " " },
                        section.label()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            frame.render_widget(
                Paragraph::new(format!("{lines}\n\nj/k select   Enter apply   Esc cancels"))
                    .style(app.theme.style(StyleRole::SurfaceRaised))
                    .block(
                        ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL)
                            .title("service section"),
                    ),
                area,
            );
        }
        Overlay::Appearance(state) => {
            let lines = ThemeId::ALL
                .iter()
                .map(|id| {
                    let marker = if *id == state.selected { ">" } else { " " };
                    ratatui::text::Line::from(vec![
                        ratatui::text::Span::styled(marker, app.theme.style(StyleRole::Selection)),
                        ratatui::text::Span::raw(" "),
                        ratatui::text::Span::styled(
                            id.as_str(),
                            app.theme.style(StyleRole::TextPrimary),
                        ),
                    ])
                })
                .chain([
                    ratatui::text::Line::from(""),
                    ratatui::text::Line::from(vec![
                        ratatui::text::Span::styled(
                            "✓ healthy",
                            app.theme.style(StyleRole::StateHealthy),
                        ),
                        ratatui::text::Span::raw("  "),
                        ratatui::text::Span::styled(
                            "! warning",
                            app.theme.style(StyleRole::StateWarning),
                        ),
                        ratatui::text::Span::raw("  "),
                        ratatui::text::Span::styled(
                            "X danger/public",
                            app.theme.style(StyleRole::StatePublic),
                        ),
                    ]),
                    ratatui::text::Line::from(vec![
                        ratatui::text::Span::styled(
                            "local",
                            app.theme.style(StyleRole::SourceLocal),
                        ),
                        ratatui::text::Span::raw("  "),
                        ratatui::text::Span::styled(
                            "admin",
                            app.theme.style(StyleRole::SourceAdmin),
                        ),
                        ratatui::text::Span::raw("  "),
                        ratatui::text::Span::styled(
                            "local+admin",
                            app.theme.style(StyleRole::SourceCombined),
                        ),
                    ]),
                    ratatui::text::Line::from(
                        "Enter apply for session · Esc cancel · persist with ui.theme",
                    ),
                ])
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(lines)
                    .style(app.theme.style(StyleRole::SurfaceRaised))
                    .block(
                        ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL)
                            .border_style(app.theme.style(StyleRole::BorderFocused))
                            .title("appearance"),
                    ),
                area,
            );
        }
        Overlay::AccountPicker(state) => form::render_accounts(frame, app, area, state),
        Overlay::HandoffInput(state) => form::render_handoff(frame, app, area, state),
        Overlay::PolicyEditor => policy_editor::render(frame, app, area),
        Overlay::SecretResult => secret_result::render(frame, app, area),
        Overlay::AuditInvestigation => audit::render(frame, app, area),
    }
}

fn overlay_area(area: Rect, overlay: &Overlay) -> Rect {
    match overlay {
        Overlay::QuitConfirmation | Overlay::Confirmation(_) => {
            let width = area.width.saturating_mul(2) / 3;
            let height = area.height.saturating_mul(2) / 3;
            Rect {
                x: area.x + area.width.saturating_sub(width) / 2,
                y: area.y + area.height.saturating_sub(height) / 2,
                width,
                height,
            }
        }
        Overlay::TaskInspector(_)
        | Overlay::PolicyEditor
        | Overlay::SecretResult
        | Overlay::AuditInvestigation => area,
        Overlay::SortPicker { .. }
        | Overlay::DiagnosticInput(_)
        | Overlay::OperatorForm(_)
        | Overlay::ServiceForm(_)
        | Overlay::ServiceSectionPicker(_)
        | Overlay::Appearance(_)
        | Overlay::AccountPicker(_)
        | Overlay::HandoffInput(_) => {
            let height = area.height.saturating_mul(2) / 5;
            Rect {
                x: area.x,
                y: area.y.saturating_add(area.height.saturating_sub(height)),
                width: area.width,
                height,
            }
        }
    }
}
