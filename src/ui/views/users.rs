use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, List, ListItem};

use crate::app::App;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let resource = &app.admin.users;
    let mut items = Vec::new();
    if let Some(users) = resource.snapshot.as_ref() {
        for (index, user) in users.iter().enumerate() {
            items.push(ListItem::new(format!(
                "{} {:<24} {:<18} {:<12} devices:{} last:{}",
                if index == app.admin_user_selected {
                    ">"
                } else {
                    " "
                },
                user.label(),
                user.role.as_deref().map_or("unknown", |value| value),
                user.status.as_deref().map_or("unknown", |value| value),
                user.device_count
                    .map_or_else(|| "?".to_owned(), |count| count.to_string()),
                user.last_seen
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
            )));
        }
        if let Some(user) = app
            .admin
            .users
            .snapshot
            .as_ref()
            .and_then(|users| users.get(app.admin_user_selected))
        {
            items.push(ListItem::new(format!(
                "selected: {} · id:{} · currently_connected:{}",
                user.label(),
                user.id,
                user.currently_connected
                    .map_or("unknown", |value| if value { "yes" } else { "no" })
            )));
        }
    }
    if items.is_empty() {
        items.push(ListItem::new(format!(
            "{} · {}",
            resource.state.label(),
            resource
                .error
                .as_deref()
                .map_or("no users observed", |value| value)
        )));
    }
    frame.render_widget(
        List::new(items)
            .style(app.theme.style(theme::StyleRole::Surface))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("users · admin"),
            ),
        area,
    );
}
