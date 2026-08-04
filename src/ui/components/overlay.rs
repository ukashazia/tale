use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Paragraph};

use crate::app::{App, Overlay};
use crate::ui::components::{
    action_picker, batch_result, command_palette, confirm, copy_picker, filter, form, help,
};
use crate::ui::theme;
use crate::ui::views::{audit, policy_editor, secret_result};

pub fn render(frame: &mut Frame<'_>, app: &App, overlay: &Overlay) {
    let area = centered(frame.area(), overlay);
    frame.render_widget(Clear, area);
    match overlay {
        Overlay::CommandPalette(_) => command_palette::render(frame, app, area, overlay),
        Overlay::FilterEditor(_) => filter::render(frame, app, area, overlay),
        Overlay::Help(_) => help::render(frame, app, area, overlay),
        Overlay::ActionPicker(_) => action_picker::render(frame, app, area, overlay),
        Overlay::CopyPicker(state) => copy_picker::render(frame, app, area, state),
        Overlay::QuitConfirmation => frame.render_widget(
            Paragraph::new(
                "Active tasks are still running.\nEnter/y: quit and cancel tasks   n/Esc: continue",
            ),
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
                Paragraph::new(detail).style(theme::normal(app)).block(
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
                Paragraph::new(lines).style(theme::normal(app)).block(
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
                    .style(theme::normal(app))
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
                    .style(theme::normal(app))
                    .block(
                        ratatui::widgets::Block::default()
                            .borders(ratatui::widgets::Borders::ALL)
                            .title("service section"),
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

fn centered(area: Rect, overlay: &Overlay) -> Rect {
    let width = match overlay {
        Overlay::Help(_) => area.width.saturating_mul(3) / 4,
        Overlay::CommandPalette(_)
        | Overlay::FilterEditor(_)
        | Overlay::DiagnosticInput(_)
        | Overlay::OperatorForm(_)
        | Overlay::ServiceForm(_)
        | Overlay::ServiceSectionPicker(_)
        | Overlay::HandoffInput(_)
        | Overlay::Confirmation(_)
        | Overlay::PolicyEditor
        | Overlay::SecretResult
        | Overlay::AuditInvestigation => area.width.saturating_mul(2) / 3,
        _ => area.width.saturating_mul(3) / 5,
    }
    .max(20)
    .min(area.width);
    let height = match overlay {
        Overlay::Help(_) => area.height.saturating_mul(3) / 4,
        _ => area.height.saturating_mul(2) / 3,
    }
    .max(5)
    .min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}
