use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::action::{self, ActionContext};
use crate::app::{App, Focus, Route};
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if !app.resolved_config.ui.show_footer {
        return;
    }
    let context = match app.current_route() {
        Route::Activity => ActionContext::Activity,
        Route::Devices if app.focus == Focus::Inspector => ActionContext::Detail,
        Route::Devices => ActionContext::Collection,
        Route::Users | Route::Routes | Route::Credentials => ActionContext::Collection,
        Route::Access | Route::Dns | Route::Settings => ActionContext::Root,
        Route::Services if app.focus == Focus::Inspector => ActionContext::Detail,
        Route::Services => ActionContext::Collection,
        _ => ActionContext::Root,
    };
    let hints = action::footer_hints(context, area.width);
    frame.render_widget(
        Paragraph::new(Line::from(hints.join("  "))).style(theme::normal(app)),
        area,
    );
}
