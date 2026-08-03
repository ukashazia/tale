use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, List, ListItem};

use crate::action::{self, ActionContext};
use crate::app::{App, Overlay};
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, overlay: &Overlay) {
    let Overlay::Help(state) = overlay else {
        return;
    };
    let query = state.query.to_ascii_lowercase();
    let items = action::all_actions()
        .into_iter()
        .filter(|spec| {
            spec.contexts.contains(&ActionContext::Root)
                || spec.contexts.contains(&ActionContext::Collection)
                || spec.contexts.contains(&ActionContext::Activity)
        })
        .filter(|spec| {
            query.is_empty()
                || spec.label.to_ascii_lowercase().contains(&query)
                || spec.description.to_ascii_lowercase().contains(&query)
        })
        .map(|spec| {
            let binding = spec
                .default_bindings
                .first()
                .map_or("-", |binding| binding.label());
            let disabled = spec
                .capability
                .reason()
                .map_or_else(String::new, |reason| format!(" ({reason})"));
            ListItem::new(format!(
                "{binding:>8}  {:<24} {}{}",
                spec.id.as_str(),
                spec.label,
                disabled
            ))
        });
    let title = if state.searchable {
        "help · search"
    } else {
        "help · ? to search"
    };
    frame.render_widget(
        List::new(items)
            .style(theme::normal(app))
            .block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}
