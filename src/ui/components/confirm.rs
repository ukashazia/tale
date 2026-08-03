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
    let text = format!(
        "{}{}\n\npreview:\n{}\nargv: {}\n\nconfirmation: {}\n> {}{}\nEsc cancels",
        state.prompt,
        checkbox,
        state.preview_lines.join("\n"),
        state.redacted_argv.join(" "),
        phrase,
        state.input,
        error
    );
    let text = format!("risk: {risk}\n{text}");
    frame.render_widget(
        Paragraph::new(text)
            .style(theme::normal(app))
            .block(Block::default().borders(Borders::ALL).title("confirm")),
        area,
    );
}
