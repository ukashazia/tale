use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let text = app.notifications.last().map_or_else(
        || {
            app.copied_value.as_ref().map_or_else(
                || option_string_or_empty(app.devices_resource.error.clone()),
                |value| format!("copied: {value}"),
            )
        },
        |notification| notification.message.clone(),
    );
    if text.is_empty() {
        return;
    }
    let style = if app.devices_resource.health == crate::domain::SourceHealth::Error {
        app.theme.style(theme::StyleRole::StateDanger)
    } else {
        app.theme.style(theme::StyleRole::StateWarning)
    };
    frame.render_widget(Paragraph::new(text).style(style), area);
}

fn option_string_or_empty(value: Option<String>) -> String {
    let mut result = String::new();
    if let Some(value) = value {
        result.push_str(&value);
    }
    result
}
