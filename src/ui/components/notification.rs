use std::borrow::Cow;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::App;
use crate::task::TaskResultKind;
use crate::ui::theme;

/// A remedy that gets cut off is not a remedy, so a long message is given a
/// second row rather than truncated.
pub const MAXIMUM_ROWS: u16 = 2;

pub fn rows(app: &App, width: u16) -> u16 {
    let Some(hint) = status_hint(app) else {
        return 1;
    };
    if hint.text.is_empty() || width == 0 {
        return 1;
    }
    let needed = hint
        .text
        .chars()
        .count()
        .div_ceil(usize::from(width).max(1))
        .max(1);
    u16::try_from(needed).map_or(MAXIMUM_ROWS, |rows| rows.min(MAXIMUM_ROWS))
}

struct StatusHint<'a> {
    text: Cow<'a, str>,
    role: theme::StyleRole,
}

fn status_hint(app: &App) -> Option<StatusHint<'_>> {
    if let Some(error) = &app.runtime_error {
        return Some(StatusHint {
            text: Cow::Borrowed(error),
            role: theme::StyleRole::StateDanger,
        });
    }
    if let Some(notification) = app.notifications.last() {
        let role = match notification.kind {
            TaskResultKind::Success => theme::StyleRole::TaskSucceeded,
            TaskResultKind::Failure => theme::StyleRole::TaskFailed,
            TaskResultKind::Cancelled => theme::StyleRole::TaskCancelled,
        };
        return Some(StatusHint {
            text: Cow::Borrowed(&notification.message),
            role,
        });
    }
    if let Some(value) = &app.copied_value {
        return Some(StatusHint {
            text: Cow::Owned(format!("copied: {}", one_line(value))),
            role: theme::StyleRole::StateInfo,
        });
    }
    app.devices_resource.error.as_ref().map(|error| StatusHint {
        text: Cow::Borrowed(error),
        role: theme::StyleRole::StateDanger,
    })
}

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(hint) = status_hint(app) else {
        return;
    };
    frame.render_widget(
        Paragraph::new(hint.text.as_ref())
            .style(app.theme.style(hint.role))
            .wrap(Wrap { trim: true }),
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
