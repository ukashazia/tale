use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::action::Risk;
use crate::app::{App, ConfirmationState};
use crate::ui::theme;

/// The last screen before something changes. It says how risky the change is,
/// what will happen, and the exact command — in that order, in words.
pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, state: &ConfirmationState) {
    // The request knows more than the action does: receiving files is
    // reversible until the conflict rule says overwrite.
    let risk = state
        .mutation
        .as_ref()
        .map(crate::domain::mutation::LocalMutation::risk)
        .or_else(|| state.admin_mutation.as_ref().map(|mutation| mutation.risk))
        .or_else(|| state.service_request.as_ref().map(|request| request.risk()))
        .or_else(|| crate::action::find_action(state.action_id).map(|spec| spec.risk));
    let mut lines = Vec::new();
    if let Some(risk) = risk {
        lines.push(Line::from(Span::styled(
            format!(" {} ", risk_label(risk)),
            app.theme
                .style(risk.style_role())
                .add_modifier(Modifier::REVERSED),
        )));
        lines.push(Line::default());
    }
    if !state.prompt.is_empty() {
        lines.push(Line::from(Span::styled(
            state.prompt.clone(),
            app.theme.style(theme::StyleRole::TextPrimary),
        )));
    }
    if state.mutation.as_ref().is_some_and(|mutation| {
        matches!(
            mutation,
            crate::domain::mutation::LocalMutation::Disconnect { .. }
        )
    }) {
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled(
                if state.lose_ssh_checked {
                    "[x] "
                } else {
                    "[ ] "
                },
                app.theme.style(theme::StyleRole::Focus),
            ),
            Span::styled(
                "I accept losing the current SSH connection",
                app.theme.style(theme::StyleRole::TextPrimary),
            ),
            Span::styled(
                "  (Tab)",
                app.theme.style(theme::StyleRole::KeyHintDisabled),
            ),
        ]));
    }
    lines.extend(section(app, "What will happen", &state.preview_lines));
    if !state.redacted_argv.is_empty() {
        lines.extend(section(
            app,
            "Command",
            std::slice::from_ref(&state.redacted_argv.join(" ")),
        ));
    }
    lines.push(Line::default());
    match state.required_phrase.as_deref() {
        Some(phrase) => {
            lines.push(Line::from(vec![
                Span::styled("Type ", app.theme.style(theme::StyleRole::TextMuted)),
                Span::styled(
                    phrase.to_owned(),
                    app.theme.style(theme::StyleRole::StateDanger),
                ),
                Span::styled(" to confirm", app.theme.style(theme::StyleRole::TextMuted)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("> ", app.theme.style(theme::StyleRole::KeyHint)),
                Span::styled(
                    state.input.clone(),
                    app.theme.style(theme::StyleRole::TextPrimary),
                ),
                Span::styled("\u{2588}", app.theme.style(theme::StyleRole::Focus)),
            ]));
        }
        None => lines.push(Line::from(Span::styled(
            "Nothing else is needed to confirm.",
            app.theme.style(theme::StyleRole::TextMuted),
        ))),
    }
    if let Some(error) = state.error.as_deref() {
        lines.push(Line::from(Span::styled(
            error.to_owned(),
            app.theme.style(theme::StyleRole::StateDanger),
        )));
    }
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled("Enter", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(" run", app.theme.style(theme::StyleRole::TextMuted)),
        Span::styled("   ", app.theme.style(theme::StyleRole::TextMuted)),
        Span::styled("Esc", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(" cancel", app.theme.style(theme::StyleRole::TextMuted)),
    ]));
    frame.render_widget(
        Paragraph::new(lines)
            // Sentences here explain a change that cannot be undone; clipping
            // one at the border is the wrong place to save a row.
            .wrap(Wrap { trim: false })
            .style(app.theme.style(theme::StyleRole::TextPrimary))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(app.theme.style(theme::StyleRole::BorderDanger))
                    .title(" Confirm "),
            ),
        area,
    );
}

fn section(app: &App, heading: &'static str, body: &[String]) -> Vec<Line<'static>> {
    if body.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![
        Line::default(),
        Line::from(Span::styled(
            heading,
            app.theme.style(theme::StyleRole::SectionHeading),
        )),
    ];
    lines.extend(body.iter().map(|entry| {
        Line::from(Span::styled(
            format!("  {entry}"),
            app.theme.style(theme::StyleRole::TextPrimary),
        ))
    }));
    lines
}

/// Risk in the reader's terms rather than the enum's.
const fn risk_label(risk: Risk) -> &'static str {
    match risk {
        Risk::Observe => "Reads only",
        Risk::Reversible => "Reversible",
        Risk::Disruptive => "Disruptive",
        Risk::DestructiveOrSecret => "Destructive",
    }
}
