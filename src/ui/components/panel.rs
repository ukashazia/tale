use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::{Block, Borders, Padding, Paragraph};

use crate::app::App;
use crate::ui::theme;

/// Every bordered box in Tale. One place decides that titles are padded, that
/// the border takes the normal role, and that content sits on the surface — so
/// no view can drift into `┌inspector─` while its neighbour has `┌ devices ─`.
pub fn render(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    title: &str,
    content: impl Into<Text<'static>>,
) {
    render_styled(
        frame,
        app,
        area,
        title,
        content,
        theme::StyleRole::Surface,
        theme::StyleRole::BorderNormal,
    );
}

/// A pane whose whole surface carries a meaning — a revealed secret, say. Rare
/// by design: if every pane is special, none of them reads as special.
pub fn render_styled(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    title: &str,
    content: impl Into<Text<'static>>,
    surface: theme::StyleRole,
    border: theme::StyleRole,
) {
    frame.render_widget(
        Paragraph::new(content.into())
            .style(app.theme.style(surface))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(app.theme.style(border))
                    // Content never touches the border; one rule, every box.
                    .padding(Padding::horizontal(1))
                    .title(pad(title)),
            ),
        area,
    );
}

/// The same box, but the border says whether keys land here. Only panes that
/// can take focus should use this; a static panel claiming focus is noise.
pub fn render_focusable(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    title: &str,
    content: impl Into<Text<'static>>,
    focused: bool,
) {
    render_styled(
        frame,
        app,
        area,
        title,
        content,
        theme::StyleRole::Surface,
        if focused {
            theme::StyleRole::BorderFocused
        } else {
            theme::StyleRole::BorderNormal
        },
    );
}

pub fn block<'a>(app: &App, title: &str, content: impl Into<Text<'static>>) -> Paragraph<'a> {
    Paragraph::new(content.into())
        .style(app.theme.style(theme::StyleRole::Surface))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(app.theme.style(theme::StyleRole::BorderNormal))
                // Content never touches the border; one rule, every box.
                .padding(Padding::horizontal(1))
                .title(pad(title)),
        )
}

/// A title touching its border is the difference between a label and a smudge.
pub fn pad(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!(" {trimmed} ")
    }
}
