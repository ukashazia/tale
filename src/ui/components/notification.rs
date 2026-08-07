use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::App;
use crate::ui::theme;

/// A remedy that gets cut off is not a remedy, so a long message is given a
/// second row rather than truncated.
pub const MAXIMUM_ROWS: u16 = 2;

pub fn rows(app: &App, width: u16) -> u16 {
    let text = message(app);
    if text.is_empty() || width == 0 {
        return 1;
    }
    let needed = text
        .chars()
        .count()
        .div_ceil(usize::from(width).max(1))
        .max(1);
    u16::try_from(needed).map_or(MAXIMUM_ROWS, |rows| rows.min(MAXIMUM_ROWS))
}

fn message(app: &App) -> String {
    app.runtime_error.as_ref().map_or_else(
        || {
            app.notifications.last().map_or_else(
                || {
                    app.copied_value.as_ref().map_or_else(
                        || option_string_or_empty(app.devices_resource.error.clone()),
                        |value| format!("copied: {}", one_line(value)),
                    )
                },
                |notification| notification.message.clone(),
            )
        },
        Clone::clone,
    )
}

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let text = message(app);
    if text.is_empty() {
        return;
    }
    let style = if app.devices_resource.health == crate::domain::SourceHealth::Error {
        app.theme.style(theme::StyleRole::StateDanger)
    } else {
        app.theme.style(theme::StyleRole::StateWarning)
    };
    frame.render_widget(
        Paragraph::new(text).style(style).wrap(Wrap { trim: true }),
        area,
    );
}

/// What went to the clipboard, said back in one line. A copied field can be
/// several addresses or a whole policy; the bar confirms what landed there, so
/// it shows the beginning rather than growing to fit or wrapping into the view.
fn one_line(value: &str) -> String {
    let joined = value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    crate::ui::text::ellipsize(&joined, COPIED_WIDTH)
}

/// Long enough for an address list or a URL, short enough to stay on one row
/// beside the widths Tale supports.
const COPIED_WIDTH: usize = 72;

fn option_string_or_empty(value: Option<String>) -> String {
    let mut result = String::new();
    if let Some(value) = value {
        result.push_str(&value);
    }
    result
}
