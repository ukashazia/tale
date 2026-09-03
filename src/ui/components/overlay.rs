use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Paragraph};

use crate::app::{App, Overlay};
use crate::ui::components::{batch_result, confirm, form};
use crate::ui::theme::StyleRole;
use crate::ui::views::{audit_investigation, secret_result};

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
        Overlay::Task(state) => {
            let title = app.tasks.get(state.task_id).map_or_else(
                || "task · Esc close · @ tasks".to_owned(),
                |task| {
                    let action = crate::action::find_action(task.action_id)
                        .map_or(task.action_id.as_str(), |spec| spec.label);
                    let cancel = if task.cancellable { " · c cancel" } else { "" };
                    format!(
                        "{action} · {} · Esc close · @ tasks{cancel}",
                        task.target_label
                    )
                },
            );
            if let Some(batch) = app.admin_batch(state.task_id) {
                batch_result::render(frame, app, area, batch, state.scroll, &title);
                return;
            }
            crate::ui::views::tasks::render_task_detail(
                frame,
                app,
                area,
                state.task_id,
                state.scroll,
                &title,
                StyleRole::SurfaceRaised,
            );
        }
        Overlay::Confirmation(state) => confirm::render(frame, app, area, state),
        Overlay::Form(state) => form::render(frame, app, area, state),
        Overlay::SecretResult => secret_result::render(frame, app, area),
        Overlay::AuditInvestigation => audit_investigation::render(frame, app, area),
    }
}

pub fn overlay_area(area: Rect, overlay: &Overlay) -> Rect {
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
        Overlay::Task(_) => task_area(area),
        Overlay::SecretResult | Overlay::AuditInvestigation => area,
        // A form is as tall as the questions it asks, so no field is asked for
        // off the bottom of the screen. It never grows past the screen itself.
        Overlay::Form(state) => {
            let rows = u16::try_from(form_rows(state)).unwrap_or(u16::MAX);
            let height = rows.max(area.height.saturating_mul(2) / 5).min(area.height);
            Rect {
                x: area.x,
                y: area.y.saturating_add(area.height.saturating_sub(height)),
                width: area.width,
                height,
            }
        }
    }
}

pub fn task_area(area: Rect) -> Rect {
    let width = area.width.saturating_mul(4) / 5;
    let height = area.height.saturating_mul(2) / 3;
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

/// How many rows the form needs: its subject, its fields, any open list, the
/// submit row, the help line, an error, and the key hints, inside a border.
fn form_rows(state: &crate::app::FormState) -> usize {
    let subject = if state.subject.is_empty() {
        0
    } else {
        state.subject.len().saturating_add(1)
    };
    let list = state
        .list
        .as_ref()
        .map_or(0, |list| list.entries.len().max(1));
    let error = usize::from(state.error.is_some());
    subject
        .saturating_add(state.fields.len())
        .saturating_add(list)
        .saturating_add(error)
        // blank, Continue, blank, help, blank, hints, and two border rows
        .saturating_add(8)
}
