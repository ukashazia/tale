use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::action::{self, ActionContext, ActionId};
use crate::app::{
    App, CopyField, FilterLineState, FilterSuggestion, FilterSuggestionKind,
    FilterSuggestionSection, InteractionMode, Route, TransientKind, TransientMenuState,
};
use crate::domain::filter;
use crate::ui::{layout, text, theme};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (lines, caret) = match &app.interaction {
        InteractionMode::Normal => (normal_lines(app, area.width), None),
        InteractionMode::CommandLine(state) => navigation_lines(app, state, area),
        InteractionMode::FilterLine(state) => filter_lines(app, state, area),
        InteractionMode::Transient(state) => (transient_lines(app, state, area), None),
        InteractionMode::HelpSheet => (help_lines(app, area), None),
    };
    frame.render_widget(
        Paragraph::new(lines).style(app.theme.style(theme::StyleRole::SurfaceRaised)),
        area,
    );
    place_caret(frame, area, caret);
}

pub fn render_minimum(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let prompt = match &app.interaction {
        InteractionMode::CommandLine(state) => Some(prompt_line(
            app,
            ':',
            &state.editor.input,
            state.editor.cursor,
            state.error.as_deref(),
            area.width,
        )),
        InteractionMode::FilterLine(state) => Some(filter_prompt_line(app, state, area.width)),
        InteractionMode::Transient(_) | InteractionMode::HelpSheet => {
            Some((Line::from("Esc cancel"), None))
        }
        InteractionMode::Normal => None,
    };
    if let Some((prompt, caret)) = prompt {
        let prompt_area = Rect {
            x: area.x,
            y: area.y.saturating_add(area.height.saturating_sub(1)),
            width: area.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(prompt).style(app.theme.style(theme::StyleRole::Prompt)),
            prompt_area,
        );
        place_caret(frame, prompt_area, caret.map(|column| (column, 0)));
    }
}

/// Put the real terminal cursor on the insertion point. Ratatui shows it only
/// for frames that ask for it, so leaving `caret` empty hides it again.
fn place_caret(frame: &mut Frame<'_>, area: Rect, caret: Option<(u16, u16)>) {
    let Some((column, row)) = caret else {
        return;
    };
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.set_cursor_position((
        area.x
            .saturating_add(column)
            .min(area.right().saturating_sub(1)),
        area.y
            .saturating_add(row)
            .min(area.bottom().saturating_sub(1)),
    ));
}

fn normal_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    if !app.resolved_config.ui.show_footer {
        return vec![Line::default()];
    }
    let context = context(app);
    let mut spans = Vec::new();
    for (index, hint) in action::footer_actions(context, app.current_route(), width)
        .into_iter()
        .enumerate()
    {
        if index > 0 {
            spans.push(Span::styled(
                "  ",
                app.theme.style(theme::StyleRole::SurfaceRaised),
            ));
        }
        spans.push(Span::styled(
            hint.key,
            app.theme.style(theme::StyleRole::KeyHint),
        ));
        spans.push(Span::styled(
            " ",
            app.theme.style(theme::StyleRole::SurfaceRaised),
        ));
        spans.push(Span::styled(
            hint.label,
            app.theme.style(theme::StyleRole::TextMuted),
        ));
    }
    vec![Line::from(spans)]
}

type ShellLines = (Vec<Line<'static>>, Option<(u16, u16)>);

/// The tallest the route grid gets: the longest section, plus its heading, in
/// each of the two bands, plus the blank between them. The palette reserves it
/// whether or not the current search fills it, so the prompt does not walk up
/// and down the screen as the user types.
pub const NAVIGATION_GRID_HEIGHT: usize = 10;

/// What the palette occupies: the grid, a title and a blank above it, and a
/// blank, the prompt, and the hint row below. Derived rather than written twice,
/// because the two numbers drifting apart is what moves the caret off the
/// prompt.
pub const fn navigation_height() -> u16 {
    NAVIGATION_GRID_HEIGHT as u16 + 5
}

fn navigation_lines(app: &App, state: &crate::app::CommandLineState, area: Rect) -> ShellLines {
    const GRID_HEIGHT: usize = NAVIGATION_GRID_HEIGHT;
    let sections = navigation_sections(&state.candidates);
    let columns = layout::navigation_columns(area.width).min(sections.len().max(1));
    let separator_width = columns.saturating_sub(1).saturating_mul(2);
    let available_width = usize::from(area.width).saturating_sub(separator_width);
    let cell_width = available_width / columns;
    let mut lines = vec![navigation_header(app), Line::default()];
    let grid_start = lines.len();
    for (band, section_row) in sections.chunks(columns).enumerate() {
        if band > 0 {
            lines.push(Line::default());
        }
        let height = section_row
            .iter()
            .map(|section| section.candidates.len().saturating_add(1))
            .max()
            .map_or(0, |value| value);
        for row in 0..height {
            let mut spans = Vec::new();
            for column in 0..columns {
                if column > 0 {
                    spans.push(Span::raw("  "));
                }
                if let Some(section) = section_row.get(column) {
                    spans.extend(navigation_section_line(app, section, row, cell_width));
                } else {
                    spans.push(Span::raw(" ".repeat(cell_width)));
                }
            }
            lines.push(Line::from(spans));
        }
    }
    while lines.len().saturating_sub(grid_start) < GRID_HEIGHT {
        lines.push(Line::default());
    }
    lines.push(Line::default());
    let (prompt, caret) = prompt_line(
        app,
        ':',
        &state.editor.input,
        state.editor.cursor,
        state.error.as_deref(),
        area.width,
    );
    let caret_row = u16::try_from(lines.len()).map_or(u16::MAX, |row| row);
    lines.push(prompt);
    lines.push(navigation_hints(app));
    (lines, caret.map(|column| (column, caret_row)))
}

fn navigation_header(app: &App) -> Line<'static> {
    Line::from(vec![
        Span::styled("Views", app.theme.style(theme::StyleRole::Focus)),
        Span::raw("   "),
        Span::styled("Esc", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(" close", app.theme.style(theme::StyleRole::TextMuted)),
    ])
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum NavigationSectionKind {
    Fleet,
    Local,
    Network,
    Operations,
}

impl NavigationSectionKind {
    const fn for_route(route: Route) -> Self {
        match route {
            Route::Overview | Route::Devices | Route::Users => Self::Fleet,
            Route::Local | Route::Services | Route::Diagnostics => Self::Local,
            Route::Routes | Route::Dns | Route::Access => Self::Network,
            Route::Credentials | Route::Tasks | Route::Audit | Route::Settings => Self::Operations,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Fleet => "Fleet",
            Self::Local => "Local",
            Self::Network => "Network",
            Self::Operations => "Operations",
        }
    }
}

struct NavigationSection<'a> {
    kind: NavigationSectionKind,
    candidates: Vec<&'a crate::app::NavigationCandidate>,
}

fn navigation_sections(
    candidates: &[crate::app::NavigationCandidate],
) -> Vec<NavigationSection<'_>> {
    let mut sections: Vec<NavigationSection<'_>> = Vec::new();
    for candidate in candidates {
        let kind = NavigationSectionKind::for_route(candidate.route);
        if let Some(section) = sections.iter_mut().find(|section| section.kind == kind) {
            section.candidates.push(candidate);
        } else {
            sections.push(NavigationSection {
                kind,
                candidates: vec![candidate],
            });
        }
    }
    sections
}

fn navigation_section_line(
    app: &App,
    section: &NavigationSection<'_>,
    row: usize,
    width: usize,
) -> Vec<Span<'static>> {
    if row == 0 {
        let heading = format!(" {} ", section.kind.label());
        let padding = width.saturating_sub(heading.chars().count());
        return vec![
            Span::styled(heading, app.theme.style(theme::StyleRole::SectionHeading)),
            Span::styled(
                " ".repeat(padding),
                app.theme.style(theme::StyleRole::SurfaceRaised),
            ),
        ];
    }
    let command_width = section
        .candidates
        .iter()
        .map(|candidate| candidate.label.chars().count())
        .max()
        .map_or(0, |value| value);
    section.candidates.get(row.saturating_sub(1)).map_or_else(
        || vec![Span::raw(" ".repeat(width))],
        |candidate| navigation_cell(app, candidate, command_width, width),
    )
}

fn navigation_cell(
    app: &App,
    candidate: &crate::app::NavigationCandidate,
    command_width: usize,
    width: usize,
) -> Vec<Span<'static>> {
    let label_length = candidate.label.chars().count();
    let prefix_width = command_width.saturating_add(1);
    let description_budget = width.saturating_sub(prefix_width);
    let description = text::ellipsize(&candidate.description, description_budget);
    let used = prefix_width.saturating_add(description.chars().count());
    let mut spans = Vec::new();
    for (index, character) in candidate.label.chars().enumerate() {
        let style = if candidate
            .label_matches
            .contains(&u32::try_from(index).map_or(u32::MAX, |i| i))
        {
            app.theme.style(theme::StyleRole::KeyHint)
        } else {
            app.theme.style(theme::StyleRole::TextPrimary)
        };
        spans.push(Span::styled(character.to_string(), style));
    }
    spans.push(Span::styled(
        " ".repeat(command_width.saturating_sub(label_length)),
        app.theme.style(theme::StyleRole::TextPrimary),
    ));
    spans.push(Span::styled(
        " ",
        app.theme.style(theme::StyleRole::TextMuted),
    ));
    for (index, character) in description.chars().enumerate() {
        let style = if candidate
            .description_matches
            .contains(&u32::try_from(index).map_or(u32::MAX, |i| i))
        {
            app.theme.style(theme::StyleRole::Focus)
        } else {
            app.theme.style(theme::StyleRole::TextMuted)
        };
        spans.push(Span::styled(character.to_string(), style));
    }
    spans.push(Span::styled(
        " ".repeat(width.saturating_sub(used)),
        app.theme.style(theme::StyleRole::SurfaceRaised),
    ));
    spans
}

pub fn navigation_route_at(
    state: &crate::app::CommandLineState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<Route> {
    let sections = navigation_sections(&state.candidates);
    let columns = layout::navigation_columns(area.width).min(sections.len().max(1));
    let separator_width = columns.saturating_sub(1).saturating_mul(2);
    let cell_width = usize::from(area.width).saturating_sub(separator_width) / columns;
    let stride = cell_width.saturating_add(2);
    let relative_x = usize::from(column.saturating_sub(area.x));
    let section_column = relative_x / stride.max(1);
    if section_column >= columns || relative_x % stride.max(1) >= cell_width {
        return None;
    }
    let relative_y = usize::from(row.saturating_sub(area.y));
    let mut band_y = 2_usize;
    for section_row in sections.chunks(columns) {
        let height = section_row
            .iter()
            .map(|section| section.candidates.len().saturating_add(1))
            .max()?;
        if relative_y >= band_y && relative_y < band_y.saturating_add(height) {
            let candidate_row = relative_y.saturating_sub(band_y).checked_sub(1)?;
            return section_row
                .get(section_column)?
                .candidates
                .get(candidate_row)
                .map(|candidate| candidate.route);
        }
        band_y = band_y.saturating_add(height).saturating_add(1);
    }
    None
}

fn navigation_hints(app: &App) -> Line<'static> {
    let key = app.theme.style(theme::StyleRole::KeyHint);
    let label = app.theme.style(theme::StyleRole::TextMuted);
    Line::from(vec![
        Span::styled("Enter", key),
        Span::styled(" open best match", label),
    ])
}

/// Rows above the grid (title, blank) and below it (blank, prompt, hints).
const FILTER_CHROME: usize = 5;

/// One grid cell: a heading plus the suggestions underneath it. `base` is the
/// flat tray index of the first suggestion, which is the order `Tab` walks.
struct FilterCell<'a> {
    label: &'a str,
    base: usize,
    suggestions: &'a [FilterSuggestion],
}

fn filter_cells(sections: &[FilterSuggestionSection]) -> Vec<FilterCell<'_>> {
    let mut base = 0;
    sections
        .iter()
        .map(|section| {
            let cell = FilterCell {
                label: &section.label,
                base,
                suggestions: &section.suggestions,
            };
            base = base.saturating_add(section.suggestions.len());
            cell
        })
        .collect()
}

fn band_heights(cells: &[FilterCell<'_>], columns: usize) -> Vec<usize> {
    cells
        .chunks(columns)
        .map(|band| {
            band.iter()
                .map(|cell| cell.suggestions.len().saturating_add(1))
                .max()
                .map_or(0, |height| height)
        })
        .collect()
}

fn grid_height(heights: &[usize]) -> usize {
    heights
        .iter()
        .sum::<usize>()
        .saturating_add(heights.len().saturating_sub(1))
}

/// Height of the route's whole field catalogue laid out in `columns`.
fn catalog_height(app: &App, columns: usize) -> usize {
    let schema = app.filter_schema();
    if schema.is_empty() {
        return 1;
    }
    let heights = schema
        .groups
        .chunks(columns.max(1))
        .map(|band| {
            band.iter()
                .map(|group| group.fields.len().saturating_add(1))
                .max()
                .map_or(0, |height| height)
        })
        .collect::<Vec<_>>();
    grid_height(&heights)
}

/// Wide cells show a name, a description, and an example. When the catalogue
/// would not fit the height on offer, the grid folds into more, narrower columns
/// and drops examples rather than hiding fields.
fn filter_columns(app: &App, width: u16, budget: usize) -> usize {
    let groups = app.filter_schema().groups.len().max(1);
    let limit = layout::filter_menu_column_limit(width).min(groups);
    let mut columns = layout::filter_menu_columns(width).min(groups);
    while columns < limit && catalog_height(app, columns).saturating_add(FILTER_CHROME) > budget {
        columns = columns.saturating_add(1);
    }
    columns
}

/// Rows the collection keeps even when the catalogue would rather have them.
const FILTER_CONTENT_RESERVE: usize = 6;

/// The tray reserves room for the whole catalogue, so the prompt and the rows
/// behind it never shift while suggestions narrow down.
pub fn filter_menu_height(app: &App, width: u16, available: u16) -> u16 {
    let budget = usize::from(available)
        .saturating_sub(FILTER_CONTENT_RESERVE)
        .max(FILTER_CHROME.saturating_add(1));
    let columns = filter_columns(app, width, budget);
    let natural = catalog_height(app, columns).saturating_add(FILTER_CHROME);
    u16::try_from(natural.min(budget)).map_or(u16::MAX, |height| height)
}

/// Recovers the column count the height was chosen for, so drawing and mouse
/// hit-testing agree on the same grid.
fn rendered_columns(app: &App, area: Rect) -> usize {
    filter_columns(app, area.width, usize::from(area.height))
}

fn filter_lines(app: &App, state: &FilterLineState, area: Rect) -> ShellLines {
    let content_budget = usize::from(area.height).saturating_sub(FILTER_CHROME);
    let mut content = filter_content(app, state, area);
    let overflow = content.len().saturating_sub(content_budget);
    content.truncate(content_budget);
    if overflow > 0
        && let Some(last) = content.last_mut()
    {
        *last = Line::styled(
            format!("… +{overflow} more need a taller terminal"),
            app.theme.style(theme::StyleRole::StateWarning),
        );
    }
    let mut lines = vec![filter_header(app, area.width), Line::default()];
    lines.extend(content);
    while lines.len().saturating_sub(2) < content_budget {
        lines.push(Line::default());
    }
    lines.push(Line::default());
    let (prompt, caret) = filter_prompt_line(app, state, area.width);
    let caret_row = u16::try_from(lines.len()).map_or(u16::MAX, |row| row);
    lines.push(prompt);
    lines.push(filter_status(app, state, area.width));
    (lines, caret.map(|column| (column, caret_row)))
}

fn filter_header(app: &App, width: u16) -> Line<'static> {
    let note = text::ellipsize(
        app.filter_schema().free_text,
        usize::from(width).saturating_sub(22),
    );
    Line::from(vec![
        Span::styled("Filter", app.theme.style(theme::StyleRole::Focus)),
        Span::raw("   "),
        Span::styled(note, app.theme.style(theme::StyleRole::TextMuted)),
        Span::raw("   "),
        Span::styled("Esc", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(" cancel", app.theme.style(theme::StyleRole::TextMuted)),
    ])
}

fn filter_content(app: &App, state: &FilterLineState, area: Rect) -> Vec<Line<'static>> {
    let cells = filter_cells(&state.sections);
    if cells.is_empty() {
        let note = if app.filter_schema().is_empty() {
            "this view has no filter fields"
        } else {
            "no field matches this text"
        };
        return vec![Line::styled(
            note,
            app.theme.style(theme::StyleRole::TextMuted),
        )];
    }
    let columns = rendered_columns(app, area).min(cells.len());
    let separator_width = columns.saturating_sub(1).saturating_mul(3);
    let cell_width = usize::from(area.width).saturating_sub(separator_width) / columns.max(1);
    let mut lines = Vec::new();
    for (band_index, band) in cells.chunks(columns).enumerate() {
        if band_index > 0 {
            lines.push(Line::default());
        }
        let height = band
            .iter()
            .map(|cell| cell.suggestions.len().saturating_add(1))
            .max()
            .map_or(0, |height| height);
        for row in 0..height {
            let mut spans = Vec::new();
            for column in 0..columns {
                if column > 0 {
                    spans.push(Span::raw("   "));
                }
                match band.get(column) {
                    Some(cell) => spans.extend(filter_cell_line(app, state, cell, row, cell_width)),
                    None => spans.push(Span::raw(" ".repeat(cell_width))),
                }
            }
            lines.push(Line::from(spans));
        }
    }
    lines
}

fn filter_cell_line(
    app: &App,
    state: &FilterLineState,
    cell: &FilterCell<'_>,
    row: usize,
    width: usize,
) -> Vec<Span<'static>> {
    if row == 0 {
        let heading = format!(" {} ", cell.label);
        let heading_width = heading.chars().count();
        return vec![
            Span::styled(heading, app.theme.style(theme::StyleRole::SectionHeading)),
            Span::styled(
                " ".repeat(width.saturating_sub(heading_width)),
                app.theme.style(theme::StyleRole::SurfaceRaised),
            ),
        ];
    }
    let index = row.saturating_sub(1);
    let longest = |select: fn(&FilterSuggestion) -> usize| {
        cell.suggestions
            .iter()
            .map(select)
            .max()
            .map_or(0, |value| value)
    };
    let columns = ColumnWidths {
        name: longest(|suggestion| suggestion.text.chars().count()),
    };
    cell.suggestions.get(index).map_or_else(
        || vec![Span::raw(" ".repeat(width))],
        |suggestion| {
            let selected = state.selected_completion == Some(cell.base.saturating_add(index));
            filter_suggestion_spans(app, suggestion, selected, columns, width)
        },
    )
}

/// Shared column stop so names and descriptions line up within a cell.
#[derive(Clone, Copy)]
struct ColumnWidths {
    name: usize,
}

fn filter_suggestion_spans(
    app: &App,
    suggestion: &FilterSuggestion,
    selected: bool,
    columns: ColumnWidths,
    width: usize,
) -> Vec<Span<'static>> {
    let base = if selected {
        theme::StyleRole::CompletionSelected
    } else {
        match suggestion.kind {
            FilterSuggestionKind::Field => theme::StyleRole::SyntaxField,
            FilterSuggestionKind::Operator => theme::StyleRole::SyntaxOperator,
            FilterSuggestionKind::Value => theme::StyleRole::SyntaxValue,
        }
    };
    let mut spans = Vec::new();
    let mut used = 0_usize;
    for (index, character) in suggestion.text.chars().enumerate() {
        let matched = suggestion
            .matches
            .contains(&u32::try_from(index).map_or(u32::MAX, |value| value));
        let role = if character == ':' && !selected {
            theme::StyleRole::SyntaxOperator
        } else if matched && !selected {
            theme::StyleRole::CompletionMatch
        } else {
            base
        };
        spans.push(Span::styled(character.to_string(), app.theme.style(role)));
        used = used.saturating_add(1);
    }
    let raised = app.theme.style(theme::StyleRole::SurfaceRaised);
    spans.push(Span::styled(
        " ".repeat(columns.name.saturating_sub(used).saturating_add(1)),
        raised,
    ));
    used = columns.name.saturating_add(1);
    let note = text::ellipsize(&suggestion.note, width.saturating_sub(used));
    let note_length = note.chars().count();
    spans.push(Span::styled(
        note,
        app.theme.style(theme::StyleRole::TextMuted),
    ));
    used = used.saturating_add(note_length);
    spans.push(Span::styled(
        " ".repeat(width.saturating_sub(used)),
        app.theme.style(theme::StyleRole::SurfaceRaised),
    ));
    spans
}

pub fn filter_suggestion_at(
    app: &App,
    state: &FilterLineState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<usize> {
    let cells = filter_cells(&state.sections);
    if cells.is_empty() {
        return None;
    }
    let columns = rendered_columns(app, area).min(cells.len());
    let separator_width = columns.saturating_sub(1).saturating_mul(3);
    let cell_width = usize::from(area.width).saturating_sub(separator_width) / columns.max(1);
    let stride = cell_width.saturating_add(3);
    let relative_x = usize::from(column.saturating_sub(area.x));
    let selected_column = relative_x / stride.max(1);
    if selected_column >= columns || relative_x % stride.max(1) >= cell_width {
        return None;
    }
    let content_row = usize::from(row.saturating_sub(area.y)).checked_sub(2)?;
    let heights = band_heights(&cells, columns);
    let mut band_y = 0_usize;
    for (band, height) in cells.chunks(columns).zip(heights) {
        if content_row >= band_y && content_row < band_y.saturating_add(height) {
            let item_row = content_row.saturating_sub(band_y).checked_sub(1)?;
            let cell = band.get(selected_column)?;
            let _ = cell.suggestions.get(item_row)?;
            return Some(cell.base.saturating_add(item_row));
        }
        band_y = band_y.saturating_add(height).saturating_add(1);
    }
    None
}

/// Per-character semantic roles for filter text: field names, operators, and
/// values each read in their own colour.
fn filter_syntax_roles(app: &App, input: &str) -> Vec<theme::StyleRole> {
    let schema = app.filter_schema();
    let length = input.chars().count();
    let mut roles = vec![theme::StyleRole::Prompt; length];
    let char_index = input
        .char_indices()
        .enumerate()
        .map(|(index, (byte, _))| (byte, index))
        .collect::<Vec<_>>();
    let position = |byte: usize| {
        char_index
            .iter()
            .find(|(candidate, _)| *candidate == byte)
            .map_or(length, |(_, index)| *index)
    };
    for (start, end) in filter::token_spans(input) {
        let token = input.get(start..end).map_or("", |value| value);
        let mut offset = position(start);
        let limit = position(end).min(roles.len());
        let body = match token.strip_prefix('!') {
            Some(body) => {
                if let Some(role) = roles.get_mut(offset) {
                    *role = theme::StyleRole::SyntaxOperator;
                }
                offset = offset.saturating_add(1);
                body
            }
            None => token,
        };
        let Some(colon) = body.find(':') else {
            for role in roles.iter_mut().take(limit).skip(offset) {
                *role = theme::StyleRole::SyntaxValue;
            }
            continue;
        };
        let name = body.get(..colon).map_or("", |value| value);
        let name_role = if schema.field(name).is_some() {
            theme::StyleRole::SyntaxField
        } else {
            theme::StyleRole::StateDanger
        };
        let name_end = offset.saturating_add(name.chars().count());
        for role in roles.iter_mut().take(name_end.min(limit)).skip(offset) {
            *role = name_role;
        }
        for (index, character) in body
            .get(colon..)
            .map_or("", |value| value)
            .chars()
            .enumerate()
        {
            let Some(role) = roles.get_mut(name_end.saturating_add(index)) else {
                break;
            };
            *role = if matches!(character, ':' | '<' | '>' | '=' | ',') {
                theme::StyleRole::SyntaxOperator
            } else {
                theme::StyleRole::SyntaxValue
            };
        }
    }
    roles
}

/// Merge neighbouring characters that share a role into one span.
fn collapse_spans(
    app: &App,
    characters: &[char],
    roles: &[theme::StyleRole],
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current = String::new();
    let mut current_role = None;
    for (index, character) in characters.iter().enumerate() {
        let role = roles
            .get(index)
            .copied()
            .map_or(theme::StyleRole::Prompt, |role| role);
        if current_role != Some(role) && !current.is_empty() {
            let previous = current_role.map_or(theme::StyleRole::Prompt, |role| role);
            spans.push(Span::styled(
                std::mem::take(&mut current),
                app.theme.style(previous),
            ));
        }
        current_role = Some(role);
        current.push(*character);
    }
    if !current.is_empty() {
        let previous = current_role.map_or(theme::StyleRole::Prompt, |role| role);
        spans.push(Span::styled(current, app.theme.style(previous)));
    }
    spans
}

fn filter_prompt_line(
    app: &App,
    state: &FilterLineState,
    width: u16,
) -> (Line<'static>, Option<u16>) {
    let input = &state.editor.input;
    let characters = input.chars().collect::<Vec<_>>();
    let roles = filter_syntax_roles(app, input);
    let cursor = input
        .get(..state.editor.cursor)
        .map_or(characters.len(), |value| value.chars().count());
    let budget = usize::from(width.saturating_sub(4)).max(1);
    let keep_before = cursor.min(budget.saturating_sub(1));
    let start = cursor.saturating_sub(keep_before);
    let remaining = budget.saturating_sub(keep_before).saturating_sub(1);
    let end = characters.len().min(cursor.saturating_add(remaining));
    let prompt = app.theme.style(theme::StyleRole::Prompt);
    let mut spans = vec![Span::styled(
        format!("/ {}", if start > 0 { "‹" } else { "" }),
        prompt,
    )];
    spans.extend(collapse_spans(
        app,
        characters.get(start..cursor).map_or(&[], |value| value),
        roles.get(start..cursor).map_or(&[], |value| value),
    ));
    spans.extend(collapse_spans(
        app,
        characters.get(cursor..end).map_or(&[], |value| value),
        roles.get(cursor..end).map_or(&[], |value| value),
    ));
    if end < characters.len() {
        spans.push(Span::styled("›", prompt));
    }
    (
        Line::from(spans),
        Some(caret_column(start > 0, keep_before)),
    )
}

fn filter_status(app: &App, state: &FilterLineState, width: u16) -> Line<'static> {
    let key = app.theme.style(theme::StyleRole::KeyHint);
    let label = app.theme.style(theme::StyleRole::TextMuted);
    let Some(error) = &state.error else {
        return Line::from(vec![
            Span::styled("Enter", key),
            Span::styled(" apply", label),
            Span::raw("   "),
            Span::styled("Tab", key),
            Span::styled(" complete", label),
            Span::raw("   "),
            Span::styled("S-Tab", key),
            Span::styled(" back", label),
        ]);
    };
    // The rows behind the prompt still show the last expression that parsed, so
    // the error explains the syntax instead of blanking the view. Narrow rows
    // give up the reassurance first, then the guidance, never the diagnosis.
    let mut remaining = usize::from(width);
    let message = text::ellipsize(&error.message, remaining);
    remaining = remaining.saturating_sub(message.chars().count());
    let mut spans = vec![Span::styled(
        message,
        app.theme.style(theme::StyleRole::StateDanger),
    )];
    let expected = format!("expected {}", error.expected);
    if expected.chars().count().saturating_add(3) <= remaining {
        remaining = remaining
            .saturating_sub(expected.chars().count())
            .saturating_sub(3);
        spans.push(Span::raw("   "));
        spans.push(Span::styled(expected, label));
    }
    const RETAINED: &str = "showing last valid result";
    if RETAINED.chars().count().saturating_add(3) <= remaining {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(RETAINED, label));
    }
    Line::from(spans)
}

fn prompt_line(
    app: &App,
    prefix: char,
    input: &str,
    cursor: usize,
    error: Option<&str>,
    width: u16,
) -> (Line<'static>, Option<u16>) {
    let before = input.get(..cursor).map_or("", |value| value);
    let after = input.get(cursor..).map_or("", |value| value);
    let budget = usize::from(width.saturating_sub(4)).max(1);
    let before_chars = before.chars().collect::<Vec<_>>();
    let keep_before = before_chars.len().min(budget.saturating_sub(1));
    let clipped_left = keep_before < before_chars.len();
    let before = before_chars[before_chars.len().saturating_sub(keep_before)..]
        .iter()
        .collect::<String>();
    let remaining = budget.saturating_sub(keep_before).saturating_sub(1);
    let after_chars = after.chars().collect::<Vec<_>>();
    let keep_after = after_chars.len().min(remaining);
    let clipped_right = keep_after < after_chars.len();
    let after = after_chars[..keep_after].iter().collect::<String>();
    let mut spans = vec![
        Span::styled(
            format!("{prefix} {}{before}", if clipped_left { "‹" } else { "" }),
            app.theme.style(theme::StyleRole::Prompt),
        ),
        Span::styled(
            format!("{after}{}", if clipped_right { "›" } else { "" }),
            app.theme.style(theme::StyleRole::Prompt),
        ),
    ];
    if let Some(error) = error {
        spans.push(Span::styled(
            format!("  {error}"),
            app.theme.style(theme::StyleRole::StateDanger),
        ));
    }
    (
        Line::from(spans),
        Some(caret_column(clipped_left, keep_before)),
    )
}

/// Column the insertion point sits on: the prefix, the clipping marker, and the
/// text kept to the left of the cursor.
fn caret_column(clipped_left: bool, keep_before: usize) -> u16 {
    let column = PROMPT_PREFIX_WIDTH
        .saturating_add(usize::from(clipped_left))
        .saturating_add(keep_before);
    u16::try_from(column).map_or(u16::MAX, |column| column)
}

/// `"/ "` and `": "` are both two cells wide.
const PROMPT_PREFIX_WIDTH: usize = 2;

fn transient_lines(app: &App, state: &TransientMenuState, area: Rect) -> Vec<Line<'static>> {
    match state.kind {
        TransientKind::Action => action_menu_lines(app, state, area),
        TransientKind::Copy => copy_menu_lines(app, state, area),
        TransientKind::Choice => choice_menu_lines(app, state, area),
    }
}

struct ActionMenuItem {
    id: action::ActionId,
    sequence: &'static str,
    label: &'static str,
    risk: action::Risk,
}

struct ActionMenuSection {
    group: action::TransientGroup,
    items: Vec<ActionMenuItem>,
}

/// One key-and-label entry in a grouped menu grid.
struct MenuEntry {
    key: String,
    label: String,
    key_role: theme::StyleRole,
    label_role: theme::StyleRole,
}

/// A titled band of entries. Empty sections are never emitted.
struct MenuSection {
    label: String,
    heading_role: theme::StyleRole,
    entries: Vec<MenuEntry>,
}

/// The shape shared by `a`, `y`, and `?`: a title row, a grouped grid of keys,
/// and a status row. Keeping one implementation is what keeps them consistent.
fn menu_grid_lines(
    app: &App,
    header: Line<'static>,
    status: Line<'static>,
    sections: &[MenuSection],
    columns: usize,
    area: Rect,
    overflow: &str,
) -> Vec<Line<'static>> {
    let columns = columns.min(sections.len().max(1));
    let separator_width = columns.saturating_sub(1).saturating_mul(3);
    let cell_width = usize::from(area.width).saturating_sub(separator_width) / columns.max(1);
    let mut content = Vec::new();
    for (band_index, band) in sections.chunks(columns).enumerate() {
        if band_index > 0 {
            content.push(Line::default());
        }
        for row in 0..band_height(band) {
            let mut spans = Vec::new();
            for column in 0..columns {
                if column > 0 {
                    spans.push(Span::raw("   "));
                }
                match band.get(column) {
                    Some(section) => spans.extend(menu_cell(app, section, row, cell_width)),
                    None => spans.push(Span::raw(" ".repeat(cell_width))),
                }
            }
            content.push(Line::from(spans));
        }
    }
    let budget = usize::from(area.height.saturating_sub(4));
    let hidden = content.len() > budget;
    let mut lines = vec![header, Line::default()];
    lines.extend(content.into_iter().take(budget));
    if hidden && let Some(line) = lines.last_mut() {
        *line = Line::styled(
            overflow.to_owned(),
            app.theme.style(theme::StyleRole::StateWarning),
        );
    }
    while lines.len().saturating_sub(2) < budget {
        lines.push(Line::default());
    }
    lines.push(Line::default());
    lines.push(status);
    lines
}

fn band_height(band: &[MenuSection]) -> usize {
    band.iter()
        .map(|section| section.entries.len().saturating_add(1))
        .max()
        .map_or(0, |height| height)
}

fn menu_cell(app: &App, section: &MenuSection, row: usize, width: usize) -> Vec<Span<'static>> {
    if row == 0 {
        let heading = format!(" {} ", section.label);
        let used = heading.chars().count();
        return vec![
            Span::styled(heading, app.theme.style(section.heading_role)),
            Span::styled(
                " ".repeat(width.saturating_sub(used)),
                app.theme.style(theme::StyleRole::SurfaceRaised),
            ),
        ];
    }
    let key_width = section
        .entries
        .iter()
        .map(|entry| entry.key.chars().count())
        .max()
        .map_or(0, |value| value);
    section.entries.get(row.saturating_sub(1)).map_or_else(
        || vec![Span::raw(" ".repeat(width))],
        |entry| {
            let padding = key_width
                .saturating_sub(entry.key.chars().count())
                .saturating_add(1);
            let label_budget = width.saturating_sub(key_width.saturating_add(1));
            let label = text::ellipsize(&entry.label, label_budget);
            let used = key_width
                .saturating_add(1)
                .saturating_add(label.chars().count());
            vec![
                Span::styled(entry.key.clone(), app.theme.style(entry.key_role)),
                Span::styled(
                    " ".repeat(padding),
                    app.theme.style(theme::StyleRole::SurfaceRaised),
                ),
                Span::styled(label, app.theme.style(entry.label_role)),
                Span::styled(
                    " ".repeat(width.saturating_sub(used)),
                    app.theme.style(theme::StyleRole::SurfaceRaised),
                ),
            ]
        },
    )
}

/// Rows a grouped grid needs, including its chrome, with the same minimum the
/// other bottom surfaces reserve.
fn menu_grid_height(sections: &[MenuSection], columns: usize) -> u16 {
    let columns = columns.min(sections.len().max(1));
    let bands = sections.len().div_ceil(columns.max(1));
    let content = sections
        .chunks(columns.max(1))
        .map(band_height)
        .sum::<usize>()
        .saturating_add(bands.saturating_sub(1));
    u16::try_from(content.saturating_add(4))
        .map_or(u16::MAX, |height| height)
        .max(14)
}

/// Which entry the pointer is over, in the same geometry the grid draws.
fn menu_grid_entry_at(
    sections: &[MenuSection],
    columns: usize,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<(usize, usize)> {
    let columns = columns.min(sections.len().max(1));
    let separator_width = columns.saturating_sub(1).saturating_mul(3);
    let cell_width = usize::from(area.width).saturating_sub(separator_width) / columns.max(1);
    let stride = cell_width.saturating_add(3);
    let relative_x = usize::from(column.saturating_sub(area.x));
    let selected_column = relative_x / stride.max(1);
    if selected_column >= columns || relative_x % stride.max(1) >= cell_width {
        return None;
    }
    let content_row = usize::from(row.saturating_sub(area.y)).checked_sub(2)?;
    let mut band_y = 0_usize;
    for (band_index, band) in sections.chunks(columns.max(1)).enumerate() {
        let height = band_height(band);
        if content_row >= band_y && content_row < band_y.saturating_add(height) {
            let entry_row = content_row.saturating_sub(band_y).checked_sub(1)?;
            let section = band_index
                .saturating_mul(columns)
                .saturating_add(selected_column);
            let _ = sections.get(section)?.entries.get(entry_row)?;
            return Some((section, entry_row));
        }
        band_y = band_y.saturating_add(height).saturating_add(1);
    }
    None
}

fn action_menu_sections(state: &TransientMenuState) -> Vec<ActionMenuSection> {
    let mut sections: Vec<ActionMenuSection> = Vec::new();
    for id in &state.actions {
        let Some(sequence) = action::transient_sequence(*id) else {
            continue;
        };
        let Some(group) = action::transient_group(*id) else {
            continue;
        };
        let Some(spec) = action::find_action(*id) else {
            continue;
        };
        let item = ActionMenuItem {
            id: *id,
            sequence,
            label: spec.label,
            risk: spec.risk,
        };
        if let Some(section) = sections.iter_mut().find(|section| section.group == group) {
            section.items.push(item);
        } else {
            sections.push(ActionMenuSection {
                group,
                items: vec![item],
            });
        }
    }
    sections
}

/// Action entries as shared grid sections. Disabled and prefix-filtered items
/// stay visible but dimmed, so the map of what exists never changes shape.
fn action_grid_sections(app: &App, state: &TransientMenuState) -> Vec<MenuSection> {
    action_menu_sections(state)
        .into_iter()
        .map(|section| {
            let active = state.prefix.is_none_or(|prefix| {
                section
                    .items
                    .iter()
                    .any(|item| item.sequence.starts_with(prefix))
            });
            let heading_role = if active && section.group == action::TransientGroup::Danger {
                theme::StyleRole::RiskDestructive
            } else if active {
                theme::StyleRole::SectionHeading
            } else {
                theme::StyleRole::TextDisabled
            };
            MenuSection {
                label: section.group.label().to_owned(),
                heading_role,
                entries: section
                    .items
                    .iter()
                    .map(|item| {
                        let disabled = app.action_unavailable_reason(item.id).is_some()
                            || state
                                .prefix
                                .is_some_and(|prefix| !item.sequence.starts_with(prefix));
                        MenuEntry {
                            key: item.sequence.to_owned(),
                            label: item.label.to_ascii_lowercase(),
                            key_role: if disabled {
                                theme::StyleRole::KeyHintDisabled
                            } else if item.risk == action::Risk::Observe {
                                theme::StyleRole::KeyHint
                            } else {
                                item.risk.style_role()
                            },
                            label_role: if disabled {
                                theme::StyleRole::TextDisabled
                            } else if item.risk == action::Risk::DestructiveOrSecret {
                                theme::StyleRole::RiskDestructive
                            } else {
                                theme::StyleRole::TextMuted
                            },
                        }
                    })
                    .collect(),
            }
        })
        .collect()
}

pub fn action_menu_height(app: &App, state: &TransientMenuState, width: u16) -> u16 {
    let sections = action_grid_sections(app, state);
    menu_grid_height(&sections, layout::action_menu_columns(width))
}

pub fn action_menu_action_at(
    app: &App,
    state: &TransientMenuState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<action::ActionId> {
    let sections = action_grid_sections(app, state);
    let (section, entry) = menu_grid_entry_at(
        &sections,
        layout::action_menu_columns(area.width),
        area,
        column,
        row,
    )?;
    action_menu_sections(state)
        .get(section)?
        .items
        .get(entry)
        .map(|item| item.id)
}

fn action_menu_lines(app: &App, state: &TransientMenuState, area: Rect) -> Vec<Line<'static>> {
    menu_grid_lines(
        app,
        action_menu_header(app, state),
        action_menu_status(app, state),
        &action_grid_sections(app, state),
        layout::action_menu_columns(area.width),
        area,
        "… more actions need a taller terminal",
    )
}

fn action_menu_header(app: &App, state: &TransientMenuState) -> Line<'static> {
    let escape_label = if state.prefix.is_some() {
        " back"
    } else {
        " close"
    };
    Line::from(vec![
        Span::styled("Actions", app.theme.style(theme::StyleRole::Focus)),
        Span::raw("   "),
        Span::styled("?", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(" help", app.theme.style(theme::StyleRole::TextMuted)),
        Span::raw("   "),
        Span::styled("Esc", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(escape_label, app.theme.style(theme::StyleRole::TextMuted)),
    ])
}

fn action_menu_status(app: &App, state: &TransientMenuState) -> Line<'static> {
    if let Some(message) = &state.message {
        return Line::styled(
            message.clone(),
            app.theme.style(theme::StyleRole::StateWarning),
        );
    }
    if let Some(prefix) = state.prefix {
        return Line::from(vec![
            Span::styled(
                prefix.to_string(),
                app.theme.style(theme::StyleRole::KeyHint),
            ),
            Span::styled(
                " …  waiting for next key",
                app.theme.style(theme::StyleRole::TextMuted),
            ),
        ]);
    }
    Line::from(vec![
        Span::styled("Keys", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(
            " activate immediately",
            app.theme.style(theme::StyleRole::TextMuted),
        ),
    ])
}

/// Height the copy menu needs: one row normally, a listed choice per address
/// once the address level is open.
/// Every "pick one value" menu — sort, section, appearance, account — uses the
/// same grid as the action menu, so learning one teaches the rest.
/// The top level names subjects; drilling in replaces the menu with just that
/// subject's variants. A pending key never leaves the other subjects on screen
/// greyed out — it swaps the menu for the next question.
fn choice_grid_sections(state: &TransientMenuState) -> Vec<MenuSection> {
    let mut sections: Vec<MenuSection> = Vec::new();
    let mut push = |group: &str, entry: MenuEntry| {
        if let Some(section) = sections.iter_mut().find(|section| section.label == group) {
            section.entries.push(entry);
        } else {
            sections.push(MenuSection {
                label: group.to_owned(),
                heading_role: theme::StyleRole::SectionHeading,
                entries: vec![entry],
            });
        }
    };
    if let Some(prefix) = state.prefix {
        for choice in state
            .choices
            .iter()
            .filter(|choice| choice.sequence.starts_with(prefix))
        {
            let key = choice
                .sequence
                .get(prefix.len_utf8()..)
                .map_or_else(String::new, str::to_owned);
            push(
                &choice.subject,
                MenuEntry {
                    key,
                    label: variant_label(choice),
                    key_role: theme::StyleRole::KeyHint,
                    label_role: if choice.active {
                        theme::StyleRole::TextPrimary
                    } else {
                        theme::StyleRole::TextMuted
                    },
                },
            );
        }
        return sections;
    }
    // One entry per distinct first key, so a subject appears once.
    let mut seen: Vec<char> = Vec::new();
    for choice in &state.choices {
        let Some(key) = choice.sequence.chars().next() else {
            continue;
        };
        if choice.subject.is_empty() {
            push(
                &choice.group,
                MenuEntry {
                    key: choice.sequence.clone(),
                    label: variant_label(choice),
                    key_role: theme::StyleRole::KeyHint,
                    label_role: if choice.active {
                        theme::StyleRole::TextPrimary
                    } else {
                        theme::StyleRole::TextMuted
                    },
                },
            );
            continue;
        }
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        // The subject is marked when any of its variants is the current value.
        let active = state
            .choices
            .iter()
            .any(|other| other.sequence.starts_with(key) && other.active);
        push(
            &choice.group,
            MenuEntry {
                key: key.to_string(),
                label: if active {
                    format!("{} ·", choice.subject)
                } else {
                    choice.subject.clone()
                },
                key_role: theme::StyleRole::KeyHint,
                label_role: if active {
                    theme::StyleRole::TextPrimary
                } else {
                    theme::StyleRole::TextMuted
                },
            },
        );
    }
    sections
}

fn variant_label(choice: &crate::app::MenuChoice) -> String {
    if choice.active {
        format!("{} ·", choice.label)
    } else {
        choice.label.clone()
    }
}

pub fn choice_menu_height(state: &TransientMenuState, width: u16) -> u16 {
    menu_grid_height(
        &choice_grid_sections(state),
        layout::choice_menu_columns(width),
    )
}

pub fn choice_menu_key_at(
    state: &TransientMenuState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<char> {
    let sections = choice_grid_sections(state);
    let (section, entry) = menu_grid_entry_at(
        &sections,
        layout::choice_menu_columns(area.width),
        area,
        column,
        row,
    )?;
    sections
        .get(section)?
        .entries
        .get(entry)?
        .key
        .chars()
        .next()
}

fn choice_menu_status(app: &App, state: &TransientMenuState) -> Line<'static> {
    if let Some(message) = &state.message {
        return Line::styled(
            message.clone(),
            app.theme.style(theme::StyleRole::StateWarning),
        );
    }
    Line::from(vec![
        Span::styled("Keys", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(
            if state.prefix.is_some() {
                " apply immediately · Esc goes back"
            } else {
                " apply immediately · · marks the current value"
            },
            app.theme.style(theme::StyleRole::TextMuted),
        ),
    ])
}

fn choice_menu_lines(app: &App, state: &TransientMenuState, area: Rect) -> Vec<Line<'static>> {
    // Drilling in retitles the menu, so the question on screen is the one being
    // answered.
    let title = state.prefix.and_then(|prefix| {
        state
            .choices
            .iter()
            .find(|choice| choice.sequence.starts_with(prefix) && !choice.subject.is_empty())
            .map(|choice| format!("{} · {}", state.title, choice.subject))
    });
    let header = Line::from(vec![
        Span::styled(
            title.unwrap_or_else(|| state.title.to_owned()),
            app.theme.style(theme::StyleRole::Focus),
        ),
        Span::raw("   "),
        Span::styled("Esc", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(
            if state.prefix.is_some() {
                " back"
            } else {
                " close"
            },
            app.theme.style(theme::StyleRole::TextMuted),
        ),
    ]);
    let status = choice_menu_status(app, state);
    menu_grid_lines(
        app,
        header,
        status,
        &choice_grid_sections(state),
        layout::choice_menu_columns(area.width),
        area,
        "… more choices need a taller terminal",
    )
}

/// Copy targets grouped the way a reader looks for them, rather than as one
/// undifferentiated row of keys.
fn copy_grid_sections(state: &TransientMenuState) -> Vec<MenuSection> {
    if state.prefix == Some(crate::app::ADDRESS_PREFIX) {
        return address_grid_sections(state);
    }
    // Each field names its own heading, so the route decides which fields
    // exist and in what order, and no field can be offered without appearing.
    crate::app::CopyGroup::ALL
        .into_iter()
        .filter_map(|group| {
            let entries = state
                .fields
                .iter()
                .filter(|field| field.group() == group)
                .map(|field| MenuEntry {
                    key: crate::app::copy_field_key(*field).to_string(),
                    label: copy_entry_label(state, *field),
                    key_role: theme::StyleRole::KeyHint,
                    label_role: theme::StyleRole::TextMuted,
                })
                .collect::<Vec<_>>();
            (!entries.is_empty()).then_some(MenuSection {
                label: group.label().to_owned(),
                heading_role: theme::StyleRole::SectionHeading,
                entries,
            })
        })
        .collect()
}

/// A field holding several values says so, so the extra keystroke is expected.
fn copy_entry_label(state: &TransientMenuState, field: CopyField) -> String {
    if field == CopyField::Addresses && state.addresses.len() > 1 {
        format!("{} ({})", field.label(), state.addresses.len())
    } else {
        field.label().to_owned()
    }
}

fn address_grid_sections(state: &TransientMenuState) -> Vec<MenuSection> {
    let mut entries = state
        .addresses
        .iter()
        .enumerate()
        .map(|(index, address)| {
            let number = index.saturating_add(1);
            MenuEntry {
                key: if number <= 9 {
                    number.to_string()
                } else {
                    " ".to_owned()
                },
                label: address.clone(),
                key_role: theme::StyleRole::KeyHint,
                label_role: theme::StyleRole::SyntaxValue,
            }
        })
        .collect::<Vec<_>>();
    entries.push(MenuEntry {
        key: crate::app::ADDRESS_PREFIX.to_string(),
        label: "all addresses".to_owned(),
        key_role: theme::StyleRole::KeyHint,
        label_role: theme::StyleRole::TextMuted,
    });
    vec![MenuSection {
        label: "Addresses".to_owned(),
        heading_role: theme::StyleRole::SectionHeading,
        entries,
    }]
}

pub fn copy_menu_height(state: &TransientMenuState, width: u16) -> u16 {
    menu_grid_height(&copy_grid_sections(state), layout::copy_menu_columns(width))
}

pub fn copy_menu_field_at(
    state: &TransientMenuState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<char> {
    let sections = copy_grid_sections(state);
    let (section, entry) = menu_grid_entry_at(
        &sections,
        layout::copy_menu_columns(area.width),
        area,
        column,
        row,
    )?;
    sections
        .get(section)?
        .entries
        .get(entry)?
        .key
        .chars()
        .next()
}

fn copy_menu_lines(app: &App, state: &TransientMenuState, area: Rect) -> Vec<Line<'static>> {
    let nested = state.prefix == Some(crate::app::ADDRESS_PREFIX);
    let header = Line::from(vec![
        Span::styled(
            if nested { "Copy address" } else { "Copy" },
            app.theme.style(theme::StyleRole::Focus),
        ),
        Span::raw("   "),
        Span::styled("Esc", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(
            if nested { " back" } else { " close" },
            app.theme.style(theme::StyleRole::TextMuted),
        ),
    ]);
    let status = state.message.as_ref().map_or_else(
        || {
            Line::from(vec![
                Span::styled("Keys", app.theme.style(theme::StyleRole::KeyHint)),
                Span::styled(
                    " copy immediately",
                    app.theme.style(theme::StyleRole::TextMuted),
                ),
            ])
        },
        |message| {
            Line::styled(
                message.clone(),
                app.theme.style(theme::StyleRole::StateWarning),
            )
        },
    );
    menu_grid_lines(
        app,
        header,
        status,
        &copy_grid_sections(state),
        layout::copy_menu_columns(area.width),
        area,
        "… more fields need a taller terminal",
    )
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum HelpGroup {
    Navigation,
    CurrentView,
    SearchAndCommands,
    Data,
    Global,
}

impl HelpGroup {
    const ORDER: [Self; 5] = [
        Self::Navigation,
        Self::CurrentView,
        Self::SearchAndCommands,
        Self::Data,
        Self::Global,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Navigation => "Navigation",
            Self::CurrentView => "Current view",
            Self::SearchAndCommands => "Search & commands",
            Self::Data => "Data",
            Self::Global => "Global",
        }
    }
}

struct HelpItem {
    key: &'static str,
    label: &'static str,
}

struct HelpSection {
    group: HelpGroup,
    items: Vec<HelpItem>,
}

fn help_group(id: ActionId) -> Option<HelpGroup> {
    match id {
        ActionId::CollectionMoveUp
        | ActionId::CollectionMoveDown
        | ActionId::CollectionFirst
        | ActionId::CollectionLast
        | ActionId::CollectionPageUp
        | ActionId::CollectionPageDown
        | ActionId::CollectionOpen
        | ActionId::ServicesSectionNext
        | ActionId::ServicesSectionPrevious => Some(HelpGroup::Navigation),
        ActionId::CollectionSort | ActionId::CollectionWideColumns | ActionId::TaskCancel => {
            Some(HelpGroup::CurrentView)
        }
        ActionId::ViewCommandLine
        | ActionId::ViewFilter
        | ActionId::ResourceActions
        | ActionId::ResourceCopy => Some(HelpGroup::SearchAndCommands),
        ActionId::ViewRefresh
        | ActionId::ViewRefreshAll
        | ActionId::ViewTasks
        | ActionId::ViewHistoryBack
        | ActionId::ViewHistoryForward => Some(HelpGroup::Data),
        ActionId::ViewHelp | ActionId::AppQuit => Some(HelpGroup::Global),
        _ => None,
    }
}

fn help_sections(app: &App) -> Vec<HelpSection> {
    let context = context(app);
    let actions = action::all_actions();
    HelpGroup::ORDER
        .into_iter()
        .filter_map(|group| {
            let items = actions
                .iter()
                .filter(|spec| {
                    help_group(spec.id) == Some(group)
                        && (spec.id == ActionId::AppQuit || spec.contexts.contains(&context))
                        && help_action_is_relevant(app, spec.id)
                        && !spec.default_bindings.is_empty()
                        && app.action_unavailable_reason(spec.id).is_none()
                })
                .filter_map(|spec| {
                    let binding = spec.default_bindings.first()?;
                    let label = action::compact_help_label(spec.id)?;
                    Some(HelpItem {
                        key: binding.label(),
                        label,
                    })
                })
                .collect::<Vec<_>>();
            (!items.is_empty()).then_some(HelpSection { group, items })
        })
        .collect()
}

fn help_action_is_relevant(app: &App, id: ActionId) -> bool {
    action::applies_to_route(id, app.current_route())
}

fn help_grid_sections(app: &App) -> Vec<MenuSection> {
    help_sections(app)
        .into_iter()
        .map(|section| MenuSection {
            label: section.group.label().to_owned(),
            heading_role: theme::StyleRole::SectionHeading,
            entries: section
                .items
                .iter()
                .map(|item| MenuEntry {
                    key: item.key.to_owned(),
                    label: item.label.to_ascii_lowercase(),
                    key_role: theme::StyleRole::KeyHint,
                    label_role: theme::StyleRole::TextMuted,
                })
                .collect(),
        })
        .collect()
}

pub fn help_menu_height(app: &App, width: u16) -> u16 {
    menu_grid_height(&help_grid_sections(app), layout::help_menu_columns(width))
}

fn help_lines(app: &App, area: Rect) -> Vec<Line<'static>> {
    menu_grid_lines(
        app,
        help_header(app),
        help_status(app),
        &help_grid_sections(app),
        layout::help_menu_columns(area.width),
        area,
        "… more keys need a taller terminal",
    )
}

fn help_header(app: &App) -> Line<'static> {
    Line::from(vec![
        Span::styled("Help", app.theme.style(theme::StyleRole::Focus)),
        Span::raw("   "),
        Span::styled("Esc", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(" close", app.theme.style(theme::StyleRole::TextMuted)),
    ])
}

fn help_status(app: &App) -> Line<'static> {
    Line::from(vec![
        Span::styled("Keys", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(
            " activate immediately",
            app.theme.style(theme::StyleRole::TextMuted),
        ),
    ])
}

fn context(app: &App) -> ActionContext {
    app.action_context()
}
