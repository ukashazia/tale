use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, List, ListItem};

use crate::action;
use crate::app::{App, Overlay};
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, overlay: &Overlay) {
    let Overlay::ActionPicker(state) = overlay else {
        return;
    };
    let items = state.actions.iter().enumerate().map(|(index, id)| {
        let spec = action::find_action(*id);
        let label = spec.as_ref().map_or(id.as_str(), |value| value.label);
        let description = spec.as_ref().map_or("", |value| value.description);
        let prefix = if index == state.selected { ">" } else { " " };
        ListItem::new(format!("{prefix} {label} - {description}"))
    });
    frame.render_widget(
        List::new(items).style(theme::normal(app)).block(
            Block::default()
                .borders(Borders::ALL)
                .title("actions · simulations"),
        ),
        area,
    );
}
