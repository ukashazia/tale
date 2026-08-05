use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, ConfirmationState};
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, state: &ConfirmationState) {
    let risk = state
        .mutation
        .as_ref()
        .map(crate::domain::mutation::LocalMutation::risk)
        .or_else(|| state.admin_mutation.as_ref().map(|mutation| mutation.risk))
        .or_else(|| crate::action::find_action(state.action_id).map(|spec| spec.risk))
        .map_or("unknown", crate::action::Risk::label);
    let phrase = state
        .required_phrase
        .as_deref()
        .map_or("Enter confirms", |value| value);
    let checkbox = if state.mutation.as_ref().is_some_and(|mutation| {
        matches!(
            mutation,
            crate::domain::mutation::LocalMutation::Disconnect { .. }
        )
    }) {
        format!(
            "\n[{}] accept possible loss of the current SSH connection (Tab)",
            if state.lose_ssh_checked { "x" } else { " " }
        )
    } else {
        String::new()
    };
    let error = state
        .error
        .as_deref()
        .map_or(String::new(), |value| format!("\nerror: {value}"));
    let command = if state.redacted_argv.is_empty() {
        String::new()
    } else {
        format!("\nargv: {}", state.redacted_argv.join(" "))
    };
    let text = format!(
        "{}{}\n\npreview:\n{}{}\n\nconfirmation: {}\n> {}{}\nEsc cancels",
        state.prompt,
        checkbox,
        state.preview_lines.join("\n"),
        command,
        phrase,
        state.input,
        error
    );
    let text = format!("risk: {risk}\n{text}");
    frame.render_widget(
        Paragraph::new(text)
            .style(app.theme.style(theme::StyleRole::TextPrimary))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(app.theme.style(theme::StyleRole::BorderDanger))
                    .title("confirm"),
            ),
        area,
    );
}
