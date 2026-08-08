pub mod components;
pub mod layout;
pub mod text;
pub mod theme;
pub mod views;

use ratatui::Frame;
use ratatui::widgets::Block;

use crate::app::{App, Route};

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(app.theme.style(theme::StyleRole::Canvas)),
        area,
    );
    let layout = layout::compute(area, app);
    if layout.minimum {
        let message = format!(
            "Tale needs at least 60 columns and 18 rows. Current terminal: {}x{}\nResize the terminal or press q to quit.",
            area.width, area.height
        );
        components::panel::render(frame, app, area, "minimum size", message);
        components::interaction_shell::render_minimum(frame, app, area);
        return;
    }

    components::header::render(frame, app, layout.header);
    match app.current_route() {
        Route::Overview => views::overview::render(frame, app, layout.content),
        Route::Local => views::local::render(frame, app, layout.content),
        Route::Devices => views::devices::render(frame, app, layout.content, layout.inspector),
        Route::Users => views::users::render(frame, app, layout.content, layout.inspector),
        Route::Routes => views::routes::render_admin(frame, app, layout.content),
        Route::Dns => views::dns::render(frame, app, layout.content),
        Route::Access => views::access::render(frame, app, layout.content),
        Route::Credentials => views::credentials::render(frame, app, layout.content),
        Route::Tasks => views::tasks::render(frame, app, layout.content, layout.inspector),
        Route::Audit => views::audit::render(frame, app, layout.content),
        Route::Settings => views::settings::render(frame, app, layout.content),
        Route::Services => views::services::render(frame, app, layout.content, layout.inspector),
        Route::Diagnostics => views::diagnostics::render(frame, app, layout.content),
    }
    components::notification::render(frame, app, layout.notification);
    components::interaction_shell::render(frame, app, layout.footer);
    if let Some(overlay) = app.overlays.last() {
        components::overlay::render(frame, app, overlay);
    }
}
