use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, FieldKind, FormState, ListEditor};
use crate::ui::{text, theme};

/// A field-by-field form. Every row states what it wants in words, the selected
/// row explains itself, and nothing asks for something already on screen.
pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, state: &FormState) {
    let label_width = state
        .fields
        .iter()
        .map(|field| field.label.chars().count())
        .chain(state.subject.iter().map(|(label, _)| label.chars().count()))
        .max()
        .unwrap_or(0)
        .max(10);
    let mut lines = Vec::new();
    // What the form acts on, stated rather than asked for.
    for (label, value) in &state.subject {
        lines.push(Line::from(vec![
            Span::styled(
                text::pad_or_trim(label, label_width.saturating_add(2)),
                app.theme.style(theme::StyleRole::TextMuted),
            ),
            Span::styled(
                value.clone(),
                app.theme.style(theme::StyleRole::TextPrimary),
            ),
        ]));
    }
    if !state.subject.is_empty() {
        lines.push(Line::default());
    }
    for (index, field) in state.fields.iter().enumerate() {
        let selected = index == state.selected;
        let editing = selected && state.is_editing();
        let (value, value_role) = match (&field.kind, field.display()) {
            (FieldKind::Text { hint }, "") => ((*hint).to_owned(), theme::StyleRole::TextDisabled),
            (FieldKind::List { hint }, "") => ((*hint).to_owned(), theme::StyleRole::TextDisabled),
            (FieldKind::List { .. }, value) => {
                (value.replace(',', ", "), theme::StyleRole::TextPrimary)
            }
            (_, value) => (
                value.to_owned(),
                if field.locked.is_some() {
                    theme::StyleRole::TextDisabled
                } else {
                    theme::StyleRole::TextPrimary
                },
            ),
        };
        let mut spans = vec![
            Span::styled(
                if selected { "> " } else { "  " }.to_owned(),
                app.theme.style(theme::StyleRole::KeyHint),
            ),
            Span::styled(
                text::pad_or_trim(field.label, label_width),
                app.theme.style(if selected {
                    theme::StyleRole::Focus
                } else {
                    theme::StyleRole::TextMuted
                }),
            ),
            Span::styled("  ", app.theme.style(theme::StyleRole::SurfaceRaised)),
            Span::styled(
                value,
                if editing {
                    app.theme.style(theme::StyleRole::Focus)
                } else {
                    app.theme.style(value_role)
                },
            ),
        ];
        // A caret only where typing does something: inside an open text field.
        if editing && field.is_text() {
            spans.push(Span::styled(
                "\u{2588}",
                app.theme.style(theme::StyleRole::Focus),
            ));
        }
        if editing && matches!(field.kind, FieldKind::Choice { .. } | FieldKind::Toggle) {
            spans.push(Span::styled(
                "  \u{2039} \u{203a}",
                app.theme.style(theme::StyleRole::KeyHint),
            ));
        }
        lines.push(Line::from(spans));
        // An open list shows its entries under the field, so the order the
        // user is arranging is the order they can see.
        if editing && let Some(list) = state.list.as_ref() {
            lines.extend(list_entries(app, list, label_width));
        }
    }
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled(
            if state.on_submit_row() { "> " } else { "  " }.to_owned(),
            app.theme.style(theme::StyleRole::KeyHint),
        ),
        Span::styled(
            "Continue",
            app.theme.style(if state.on_submit_row() {
                theme::StyleRole::Focus
            } else {
                theme::StyleRole::TextMuted
            }),
        ),
    ]));
    lines.push(Line::default());
    // The selected row explains itself, and a row the form cannot answer says
    // what decides it instead.
    let help = state.selected_field().map_or_else(
        || "Review the change before anything happens".to_owned(),
        |field| {
            field.locked.as_ref().map_or_else(
                || field.help.to_owned(),
                |reason| format!("{}: {reason}", field.help),
            )
        },
    );
    lines.push(Line::from(Span::styled(
        help,
        app.theme.style(theme::StyleRole::TextMuted),
    )));
    if let Some(error) = state.error.as_deref() {
        lines.push(Line::from(Span::styled(
            error.to_owned(),
            app.theme.style(theme::StyleRole::StateDanger),
        )));
    }
    lines.push(Line::default());
    lines.push(hints(app, state));
    frame.render_widget(
        Paragraph::new(lines)
            .style(app.theme.style(theme::StyleRole::SurfaceRaised))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(app.theme.style(theme::StyleRole::BorderFocused))
                    .title(format!(" {} ", state.title)),
            ),
        area,
    );
}

/// The entries of the open list, one per row, under the field they belong to.
fn list_entries(app: &App, list: &ListEditor, label_width: usize) -> Vec<Line<'static>> {
    if list.entries.is_empty() {
        return vec![Line::from(Span::styled(
            format!("{}(empty)", " ".repeat(label_width.saturating_add(4))),
            app.theme.style(theme::StyleRole::TextDisabled),
        ))];
    }
    list.entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let selected = index == list.selected;
            let mut spans = vec![
                Span::styled(
                    format!(
                        "{}{} ",
                        " ".repeat(label_width.saturating_add(2)),
                        if selected { "\u{2022}" } else { " " }
                    ),
                    app.theme.style(theme::StyleRole::KeyHint),
                ),
                Span::styled(
                    entry.clone(),
                    app.theme.style(if selected {
                        theme::StyleRole::Focus
                    } else {
                        theme::StyleRole::TextPrimary
                    }),
                ),
            ];
            if selected {
                spans.push(Span::styled(
                    "\u{2588}",
                    app.theme.style(theme::StyleRole::Focus),
                ));
            }
            Line::from(spans)
        })
        .collect()
}

/// Only the keys that do something on the field the user is standing on.
fn hints(app: &App, state: &FormState) -> Line<'static> {
    let pairs = if state.is_editing() {
        match state.selected_field().map(|field| &field.kind) {
            Some(FieldKind::Text { .. }) => vec![("Enter", "keep"), ("Esc", "discard")],
            Some(FieldKind::List { .. }) => vec![
                ("↑/↓", "entry"),
                ("Ctrl+↑/↓", "move"),
                ("Ctrl+i", "add"),
                ("Ctrl+x", "drop"),
                ("Enter", "keep"),
            ],
            _ => vec![("←/→", "change"), ("Enter", "keep"), ("Esc", "discard")],
        }
    } else if state.on_submit_row() {
        vec![("j/k", "move"), ("Enter", "review"), ("Esc", "cancel")]
    } else {
        vec![("j/k", "move"), ("Enter", "edit"), ("Esc", "cancel")]
    };
    let mut spans = Vec::new();
    for (index, (key, label)) in pairs.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                "   ",
                app.theme.style(theme::StyleRole::SurfaceRaised),
            ));
        }
        spans.push(Span::styled(
            key,
            app.theme.style(theme::StyleRole::KeyHint),
        ));
        spans.push(Span::styled(
            " ",
            app.theme.style(theme::StyleRole::SurfaceRaised),
        ));
        spans.push(Span::styled(
            label,
            app.theme.style(theme::StyleRole::TextMuted),
        ));
    }
    Line::from(spans)
}
