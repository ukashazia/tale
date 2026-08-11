use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::ui::theme;

/// A route-local tab strip. The caller owns the tab names and selection; this
/// component owns the shared spacing and selected-state treatment.
pub fn line<'a>(app: &App, tabs: impl IntoIterator<Item = (&'a str, bool)>) -> Line<'static> {
    let mut spans = Vec::new();
    for (label, selected) in tabs {
        spans.push(Span::styled(
            format!(" {label} "),
            if selected {
                app.theme
                    .style(theme::StyleRole::Focus)
                    .add_modifier(Modifier::REVERSED)
            } else {
                app.theme.style(theme::StyleRole::TextMuted)
            },
        ));
        spans.push(Span::styled(
            " ",
            app.theme.style(theme::StyleRole::Surface),
        ));
    }
    Line::from(spans)
}
