use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::App;
use crate::ui::components::{inspector, task_view};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let regions = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    task_view::render(frame, app, regions[0]);
    if let Some(task) = app.focused_task() {
        let audit = admin_audit_summary(app);
        let detail = format!(
            "{}\n{}\n{}\n\nAdmin audit\n{}\n{}\n\n{}\n\n{}\n\n{}",
            task.state.label(),
            task.summary,
            task.detail,
            audit,
            admin_audit_events(app),
            crate::ui::views::flows::summary(app),
            crate::ui::views::log_streams::summary(app),
            crate::ui::views::webhooks::summary(app)
        );
        frame.render_widget(
            ratatui::widgets::Paragraph::new(detail).block(
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title("task detail"),
            ),
            regions[1],
        );
    } else {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(format!(
                "Select a task with j/k; x cancels a cancellable task\n\nAdmin audit\n{}\n{}\n\n{}\n\n{}\n\n{}",
                admin_audit_summary(app),
                admin_audit_events(app),
                crate::ui::views::flows::summary(app),
                crate::ui::views::log_streams::summary(app),
                crate::ui::views::webhooks::summary(app)
            ))
            .block(
                ratatui::widgets::Block::default()
                    .borders(ratatui::widgets::Borders::ALL)
                    .title("task detail"),
            ),
            regions[1],
        );
    }
    let _ = inspector::render;
}

fn admin_audit_summary(app: &App) -> String {
    let resource = &app.admin.activity;
    let Some(snapshot) = resource.snapshot.as_ref() else {
        return format!(
            "{} · {}",
            resource.state.label(),
            resource
                .error
                .as_deref()
                .map_or("not observed", |value| value)
        );
    };
    let filtered = snapshot.filtered_events(&app.audit_filters);
    let first = filtered.first().copied();
    format!(
        "{} of {} events · {} · {} · configuration audit only; read-only activity is absent by server design",
        filtered.len(),
        snapshot.events.len(),
        if snapshot.delayed {
            "delivery may be delayed"
        } else {
            "server order"
        },
        first.map_or("no events", |event| {
            event
                .action
                .as_deref()
                .map_or("action unknown", |value| value)
        })
    )
}

fn admin_audit_events(app: &App) -> String {
    let Some(snapshot) = app.admin.activity.snapshot.as_ref() else {
        return "no audit events".to_owned();
    };
    let filtered = snapshot.filtered_events(&app.audit_filters);
    if filtered.is_empty() {
        return "no events in selected window".to_owned();
    }
    filtered
        .into_iter()
        .take(8)
        .enumerate()
        .map(|(index, event)| {
            let marker = if index == app.admin_activity_selected {
                ">"
            } else {
                " "
            };
            let action = event
                .action
                .as_deref()
                .map_or("action unknown", |value| value);
            let target = event
                .target
                .as_ref()
                .and_then(|target| target.id.as_deref())
                .map_or("target unknown", |value| value);
            format!("{marker} {} · {action} · {target}", event.event_time_text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
