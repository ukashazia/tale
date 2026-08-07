use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use crate::app::App;
use crate::ui::components::panel;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let resource = &app.admin.credentials;
    let mut items = Vec::new();
    if let Some(snapshot) = resource.snapshot.as_ref() {
        for credential in &snapshot.records {
            items.push(Line::from(format!(
                "{:<18} {:<16} owner:{} scopes:{} tags:{} created:{} expires:{}{}",
                credential.id,
                credential.key_type,
                credential
                    .user_id
                    .as_deref()
                    .map_or("unknown", |value| value),
                credential.scopes.len(),
                credential.tags.len(),
                credential
                    .created_at
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                credential
                    .expires_at
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                if credential.revoked_at.is_some() {
                    " · revoked"
                } else {
                    ""
                }
            )));
            if let Some(description) = credential.description.as_deref() {
                items.push(Line::from(format!("  description: {description}")));
            }
        }
        if snapshot.partial {
            items.push(Line::from(format!(
                "partial inventory: {}",
                snapshot
                    .partial_reason
                    .as_deref()
                    .map_or("narrow scopes", |value| value)
            )));
        }
    } else if matches!(
        resource.state,
        crate::admin::AdminResourceState::Forbidden | crate::admin::AdminResourceState::Unsupported
    ) {
        items.push(Line::from(
            "partial inventory: credential-specific read scope was not observed",
        ));
    }
    if items.is_empty() {
        for line in crate::ui::text::empty_state(
            "credentials",
            "credentials",
            app.admin.profile.is_some(),
            resource.state,
            resource.error.as_deref(),
        ) {
            items.push(Line::from(line));
        }
    }
    panel::render(frame, app, area, "credentials · metadata only", items);
}
