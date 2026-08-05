use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(result) = app.secret_result.as_ref() else {
        frame.render_widget(
            Paragraph::new("### redacted · the one-time result is closed")
                .style(app.theme.style(theme::StyleRole::Redacted)),
            area,
        );
        return;
    };
    let metadata = result.metadata();
    let secret = result
        .secret_handle()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "<closed>".to_owned());
    let text = format!(
        "type: {}\nid: {}\ndescription: {}\ncreated: {}\nexpires: {}\n\nsecret (view once):\n{}\n\n{}\ny/c: copy explicitly   Esc: close and destroy",
        metadata.credential_type,
        metadata
            .credential_id
            .as_deref()
            .map_or("not returned", |value| value),
        metadata
            .description
            .as_deref()
            .map_or("not returned", |value| value),
        metadata.created_at,
        metadata
            .expires_at
            .map_or_else(|| "not returned".to_owned(), |value| value.to_string()),
        secret,
        metadata.warning,
    );
    frame.render_widget(
        Paragraph::new(text)
            .style(app.theme.style(theme::StyleRole::Secret))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("secret result · closes permanently"),
            ),
        area,
    );
}
