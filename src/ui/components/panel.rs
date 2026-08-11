use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Wrap};

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

/// A collection panel whose title contains independently styled status data.
pub fn render_view(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    title: Line<'static>,
    content: impl Into<Text<'static>>,
) {
    frame.render_widget(
        Paragraph::new(content)
            .style(app.theme.style(theme::StyleRole::Surface))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(app.theme.style(theme::StyleRole::BorderNormal))
                    .padding(Padding::horizontal(1))
                    .title(title),
            ),
        area,
    );
}

/// A semantic detail pane whose values may be longer than the available
/// column. Tables stay single-line; prose and inspector values wrap.
pub fn render_wrapped(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    title: &str,
    content: impl Into<Text<'static>>,
) {
    let (title, content) = searchable_content(app, title, content.into());
    frame.render_widget(block(app, &title, content).wrap(Wrap { trim: false }), area);
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
    let (title, content) = searchable_content(app, title, content.into());
    frame.render_widget(
        Paragraph::new(content)
            .style(app.theme.style(surface))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(app.theme.style(border))
                    // A title names the page; it is text, not another border
                    // glyph. Keeping its ink independent prevents a subtle or
                    // unfocused boundary from making the route name illegible.
                    .title_style(app.theme.style(theme::StyleRole::TextPrimary))
                    // Content never touches the border; one rule, every box.
                    .padding(Padding::horizontal(1))
                    .title(pad(&title)),
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

pub fn render_focusable_wrapped(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    title: &str,
    content: impl Into<Text<'static>>,
    focused: bool,
) {
    let (title, content) = searchable_content(app, title, content.into());
    frame.render_widget(
        Paragraph::new(content)
            .style(app.theme.style(theme::StyleRole::Surface))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(app.theme.style(if focused {
                        theme::StyleRole::BorderFocused
                    } else {
                        theme::StyleRole::BorderNormal
                    }))
                    .title_style(app.theme.style(theme::StyleRole::TextPrimary))
                    .padding(Padding::horizontal(1))
                    .title(pad(&title)),
            ),
        area,
    );
}

/// A full-screen document whose body is taller than the terminal. Its input
/// target is unambiguous, so it keeps the normal boundary instead of claiming
/// split-pane focus with a blue border.
pub fn render_scrolled(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    title: &str,
    content: impl Into<Text<'static>>,
    scroll: u16,
) {
    let (title, content) = searchable_content(app, title, content.into());
    frame.render_widget(
        Paragraph::new(content)
            .style(app.theme.style(theme::StyleRole::Surface))
            .scroll((scroll, 0))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(app.theme.style(theme::StyleRole::BorderNormal))
                    .title_style(app.theme.style(theme::StyleRole::TextPrimary))
                    .padding(Padding::horizontal(1))
                    .title(pad(&title)),
            ),
        area,
    );
}

pub fn block<'a>(app: &App, title: &str, content: impl Into<Text<'static>>) -> Paragraph<'a> {
    Paragraph::new(content.into())
        .style(app.theme.style(theme::StyleRole::Surface))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(app.theme.style(theme::StyleRole::BorderNormal))
                .title_style(app.theme.style(theme::StyleRole::TextPrimary))
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

fn searchable_content(
    app: &App,
    title: &str,
    mut content: Text<'static>,
) -> (String, Text<'static>) {
    if app.action_context() != crate::action::ActionContext::Detail
        || app.current_route() == crate::app::Route::Devices
    {
        return (title.to_owned(), content);
    }
    let query = app.active_detail_search().trim();
    if query.is_empty() {
        return (title.to_owned(), content);
    }
    let query = query.to_ascii_lowercase();
    for line in &mut content.lines {
        if contains_match(&line_text(line), &query) {
            let style = app.theme.style(theme::StyleRole::CompletionMatch);
            line.style = line.style.patch(style);
            for span in &mut line.spans {
                span.style = span.style.patch(style);
            }
        }
    }
    (
        format!("{title} · /{}", app.active_detail_search()),
        content,
    )
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn contains_match(candidate: &str, query: &str) -> bool {
    candidate.to_ascii_lowercase().contains(query)
}

#[cfg(test)]
mod tests {
    use super::contains_match;

    #[test]
    fn detail_search_is_case_insensitive_contains_not_fuzzy() {
        assert!(contains_match("Client preferences", "lient"));
        assert!(contains_match("Client preferences", "client"));
        assert!(!contains_match("Client preferences", "clnt"));
    }
}
