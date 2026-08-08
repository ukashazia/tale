use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::config::{SettingDisplay, ValueSource};
use crate::ui::components::panel;
use crate::ui::{text, theme};

/// Everything this client resolved for itself, and what decided each value.
/// Only Tale's own configuration lives here: anything a tailnet owns is read
/// through the profile that manages it.
pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut values = app.resolved_config.settings();
    // Two values the file cannot state, because they are only settled once a
    // terminal has answered for itself.
    values.push(SettingDisplay {
        name: "ui.theme.session",
        value: app.theme.id().as_str().to_owned(),
        source: ValueSource::Default,
    });
    values.push(SettingDisplay {
        name: "ui.color.resolved",
        value: format!(
            "{} ({})",
            app.theme.capability().as_str(),
            match app.resolved_config.ui.color {
                crate::config::ColorMode::Auto => "auto policy",
                crate::config::ColorMode::None => "NO_COLOR or configured",
                _ => "configured",
            }
        ),
        source: app.resolved_config.ui.color_source,
    });
    // The name column fits the longest name there is, so no row pushes its own
    // source out of line with the rest.
    let name_width = values
        .iter()
        .map(|setting| setting.name.chars().count())
        .max()
        .unwrap_or(0)
        .max("SETTING".len())
        .saturating_add(2);
    let source_width = "environment".len().saturating_add(2);
    let value_width = usize::from(area.width.saturating_sub(4))
        .saturating_sub(name_width)
        .saturating_sub(source_width)
        .max(8);
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{}{}{}",
            text::pad_or_trim("SETTING", name_width),
            text::pad_or_trim("VALUE", value_width),
            "SOURCE"
        ),
        app.theme.style(theme::StyleRole::SectionHeading),
    ))];
    lines.extend(values.into_iter().map(|setting| {
        Line::from(vec![
            Span::styled(
                text::pad_or_trim(setting.name, name_width),
                app.theme.style(theme::StyleRole::TextMuted),
            ),
            // Two columns of gap the value can never eat, so a truncated path
            // does not run into the source that explains it.
            Span::styled(
                text::pad_or_trim(&setting.value, value_width.saturating_sub(2)),
                app.theme.style(theme::StyleRole::TextPrimary),
            ),
            Span::raw("  "),
            Span::styled(
                setting.source.label().to_owned(),
                app.theme.style(source_role(setting.source)),
            ),
        ])
    }));
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "Read-only here. Edit the config file at the path above, or pass a flag.",
        app.theme.style(theme::StyleRole::TextMuted),
    )));
    panel::render(frame, app, area, "config · read-only", lines);
}

/// A value someone chose reads differently from one nobody did, so the source
/// says at a glance which rows are this machine's own decisions.
const fn source_role(source: ValueSource) -> theme::StyleRole {
    match source {
        ValueSource::Default => theme::StyleRole::TextDisabled,
        ValueSource::Cli | ValueSource::Environment | ValueSource::File => {
            theme::StyleRole::TextMuted
        }
    }
}
