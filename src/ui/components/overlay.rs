use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Paragraph};

use crate::app::{App, Overlay};
use crate::ui::components::{batch_result, confirm, form};
use crate::ui::theme::StyleRole;
use crate::ui::views::{audit_investigation, policy_editor, secret_result};

pub fn render(frame: &mut Frame<'_>, app: &App, overlay: &Overlay) {
    let screen = frame.area();
    frame.render_widget(
        ratatui::widgets::Block::default().style(app.theme.style(StyleRole::Backdrop)),
        screen,
    );
    let area = overlay_area(frame.area(), overlay);
    // `Clear` only resets cells to the terminal default, so an overlay whose own
    // base style carries a foreground and no background reads as a hole in the
    // screen rather than a panel above it. The surface is painted here, once,
    // so no overlay can forget it; a renderer that sets its own raised surface
    // paints the same colour over the top and nothing changes.
    frame.render_widget(Clear, area);
    frame.render_widget(
        ratatui::widgets::Block::default().style(app.theme.style(StyleRole::SurfaceRaised)),
        area,
    );
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
        Overlay::Form(state) => form::render(frame, app, area, state),
        Overlay::HandoffInput(state) => form::render_handoff(frame, app, area, state),
        Overlay::PolicyEditor => policy_editor::render(frame, app, area),
        Overlay::SecretResult => secret_result::render(frame, app, area),
        Overlay::AuditInvestigation => audit_investigation::render(frame, app, area),
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
        Overlay::DiagnosticInput(_)
        | Overlay::OperatorForm(_)
        | Overlay::Form(_)
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
