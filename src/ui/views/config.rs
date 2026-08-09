use ratatui::Frame;
use ratatui::layout::Rect;

use crate::app::App;
use crate::config::{SettingDisplay, ValueSource};
use crate::ui::components::{grid, panel};
use crate::ui::{text, theme};

const COLUMNS: &[(&str, grid::Width)] = &[
    ("SETTING", grid::Width::Fill(20)),
    ("VALUE", grid::Width::Fill(30)),
    ("SOURCE", grid::Width::Fixed(13)),
];

/// Everything this client resolved for itself, and what decided each value.
/// Only Tale's own configuration lives here: anything a tailnet owns is read
/// through the profile that manages it.
pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let rows = app.config_rows();
    let columns = COLUMNS
        .iter()
        .map(|(header, width)| grid::Column {
            header: (*header).to_owned(),
            width: *width,
        })
        .collect::<Vec<_>>();
    let table_rows = visible_rows(app, &rows, area)
        .map(|(row, selected)| {
            grid::Row::new(vec![
                grid::Cell::new(row.name).with_role(theme::StyleRole::TextMuted),
                grid::Cell::new(row.value.as_str()),
                grid::Cell::new(row.source.label()).with_role(source_role(row.source)),
            ])
            .selected(selected)
        })
        .collect::<Vec<_>>();
    let lines = grid::lines(app, &columns, &table_rows, area.width.saturating_sub(4));
    panel::render(frame, app, area, &title(app, rows.len()), lines);
}

fn visible_rows<'a>(
    app: &App,
    rows: &'a [SettingDisplay],
    area: Rect,
) -> impl Iterator<Item = (&'a SettingDisplay, bool)> {
    let viewport = usize::from(area.height.saturating_sub(3)).max(1);
    let selected = app.views.config.selected;
    let start = selected
        .saturating_add(1)
        .saturating_sub(viewport)
        .min(rows.len().saturating_sub(1));
    rows.iter()
        .enumerate()
        .skip(start)
        .take(viewport)
        .map(move |(index, row)| (row, index == selected))
}

fn title(app: &App, shown: usize) -> String {
    let mut detail = Vec::new();
    if !app.views.config.filter.is_empty() {
        detail.push(format!(
            "/{}",
            text::ellipsize(&app.views.config.filter, 32)
        ));
    }
    detail.push(format!(
        "{} {}",
        app.views.config.sort.field.label(),
        if app.views.config.sort.direction.is_ascending() {
            "\u{2191}"
        } else {
            "\u{2193}"
        }
    ));
    let total = app.all_config_rows().len();
    if shown != total {
        detail.push(format!("{shown} of {total}"));
    }
    format!("config · read-only · {}", detail.join(" · "))
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
