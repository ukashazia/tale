use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let lines = app.admin.activity.snapshot.as_ref().map_or_else(
        || vec!["audit investigation: no server events returned".to_owned()],
        |snapshot| {
            let filtered = snapshot.filtered_events(&app.audit_filters);
            let mut lines = vec![
                format!("UTC window: {} -> {}", snapshot.start, snapshot.end),
                format!("events: {} of {}", filtered.len(), snapshot.events.len()),
                format!(
                    "filters: time={:?} actor={:?}/{:?} action={:?} target={:?}/{:?} text={:?}",
                    app.audit_filters.start,
                    app.audit_filters.actor_id,
                    app.audit_filters.actor_display,
                    app.audit_filters.action,
                    app.audit_filters.target_type,
                    app.audit_filters.target_id,
                    app.audit_filters.text
                ),
                "filters use already-decoded server fields; values are redacted".to_owned(),
                String::new(),
            ];
            lines.extend(filtered.into_iter().take(200).map(|event| {
                format!(
                    "{} {} {} -> {}",
                    event.event_time_text,
                    event.action.as_deref().map_or("unknown", |value| value),
                    event
                        .actor
                        .as_ref()
                        .and_then(|value| value.id.as_deref())
                        .map_or("unknown", |value| value),
                    event
                        .target
                        .as_ref()
                        .and_then(|value| value.id.as_deref())
                        .map_or("unknown", |value| value),
                )
            }));
            if let Some(event) = app.selected_audit_event_for_view() {
                lines.push(String::new());
                lines.push(format!(
                    "selected: {} {}",
                    event.event_time_text,
                    event.action.as_deref().map_or("action unknown", |value| value)
                ));
                if let Some(old) = event.old.as_ref() {
                    lines.push(format!("server old: {}", safe_value_text(old)));
                }
                if let Some(new) = event.new.as_ref() {
                    lines.push(format!("server new: {}", safe_value_text(new)));
                }
                lines.push(
                    "policy old/new values are server-provided; no local policy evaluation is performed"
                        .to_owned(),
                );
            }
            lines
        },
    );
    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .style(theme::normal(app))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("audit investigation · redacted"),
            ),
        area,
    );
}

fn safe_value_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "<unavailable>".to_owned()),
    }
}
