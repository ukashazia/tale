use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, List, ListItem};

use crate::app::App;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let resource = &app.admin.credentials;
    let mut items = Vec::new();
    if let Some(snapshot) = resource.snapshot.as_ref() {
        for credential in &snapshot.records {
            items.push(ListItem::new(format!(
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
                items.push(ListItem::new(format!("  description: {description}")));
            }
        }
        if snapshot.partial {
            items.push(ListItem::new(format!(
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
        items.push(ListItem::new(
            "partial inventory: credential-specific read scope was not observed",
        ));
    }
    if items.is_empty() {
        items.push(ListItem::new(format!(
            "{} · {}",
            resource.state.label(),
            resource
                .error
                .as_deref()
                .map_or("no credential metadata observed", |value| value)
        )));
    }
    frame.render_widget(
        List::new(items).style(theme::normal(app)).block(
            Block::default()
                .borders(Borders::ALL)
                .title("credentials · metadata only"),
        ),
        area,
    );
}
