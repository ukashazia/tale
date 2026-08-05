use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::action::{self, ActionContext};
use crate::app::{
    App, CopyField, Focus, InteractionMode, Route, TransientKind, TransientMenuState,
};
use crate::ui::{layout, text, theme};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let lines = match &app.interaction {
        InteractionMode::Normal => normal_lines(app, area.width),
        InteractionMode::CommandLine(state) => navigation_lines(app, state, area),
        InteractionMode::FilterLine(state) => {
            let mut lines = completion_lines(
                app,
                &state.candidates,
                state.selected_completion,
                area.height.saturating_sub(1),
            );
            lines.push(prompt_line(
                app,
                '/',
                &state.editor.input,
                state.editor.cursor,
                state.error.as_deref(),
                area.width,
            ));
            lines
        }
        InteractionMode::Transient(state) => transient_lines(app, state, area),
        InteractionMode::HelpSheet(state) => help_lines(app, &state.query, state.scroll, area),
    };
    frame.render_widget(
        Paragraph::new(lines).style(app.theme.style(theme::StyleRole::SurfaceRaised)),
        area,
    );
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
        InteractionMode::FilterLine(state) => Some(prompt_line(
            app,
            '/',
            &state.editor.input,
            state.editor.cursor,
            state.error.as_deref(),
            area.width,
        )),
        InteractionMode::Transient(_) | InteractionMode::HelpSheet(_) => {
            Some(Line::from("Esc cancel"))
        }
        InteractionMode::Normal => None,
    };
    if let Some(prompt) = prompt {
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
    }
}

fn normal_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    if !app.resolved_config.ui.show_footer {
        return vec![Line::default()];
    }
    let context = context(app);
    vec![Line::from(action::footer_hints(context, width).join("  "))]
}

fn navigation_lines(
    app: &App,
    state: &crate::app::CommandLineState,
    area: Rect,
) -> Vec<Line<'static>> {
    const GRID_HEIGHT: usize = 9;
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
    lines.push(prompt_line(
        app,
        ':',
        &state.editor.input,
        state.editor.cursor,
        state.error.as_deref(),
        area.width,
    ));
    lines.push(navigation_hints(app));
    lines
}

fn navigation_header(app: &App) -> Line<'static> {
    Line::from(vec![
        Span::styled("Views", app.theme.style(theme::StyleRole::Focus)),
        Span::raw("   "),
        Span::styled("Esc", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(" Close", app.theme.style(theme::StyleRole::TextMuted)),
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
            Route::Local | Route::Services => Self::Local,
            Route::Routes | Route::Dns | Route::Access => Self::Network,
            Route::Credentials | Route::Activity | Route::Settings => Self::Operations,
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
        Span::styled(" Open best match", label),
    ])
}

fn completion_lines(
    app: &App,
    candidates: &[crate::app::CompletionCandidate],
    selected: Option<usize>,
    available: u16,
) -> Vec<Line<'static>> {
    let limit = usize::from(available).min(6);
    let mut lines = candidates
        .iter()
        .take(limit)
        .enumerate()
        .map(|(index, candidate)| {
            let role = if selected == Some(index) {
                theme::StyleRole::CompletionSelected
            } else {
                theme::StyleRole::CompletionMatch
            };
            Line::from(Span::styled(
                format!(
                    "{} {:<16} {}{}",
                    if selected == Some(index) { ">" } else { " " },
                    candidate.label,
                    candidate.description,
                    if candidate.alias { " · alias" } else { "" }
                ),
                app.theme.style(role),
            ))
        })
        .collect::<Vec<_>>();
    if candidates.len() > limit && !lines.is_empty() {
        let overflow = candidates.len().saturating_sub(limit);
        if let Some(last) = lines.last_mut() {
            *last = Line::from(format!("… +{overflow} more"));
        }
    }
    lines
}

fn prompt_line(
    app: &App,
    prefix: char,
    input: &str,
    cursor: usize,
    error: Option<&str>,
    width: u16,
) -> Line<'static> {
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
            "▏",
            app.theme
                .style(theme::StyleRole::SurfaceRaised)
                .patch(app.theme.style(theme::StyleRole::PromptCursor)),
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
    Line::from(spans)
}

fn transient_lines(app: &App, state: &TransientMenuState, area: Rect) -> Vec<Line<'static>> {
    match state.kind {
        TransientKind::Action => action_menu_lines(app, state, area),
        TransientKind::Copy => copy_menu_lines(state, area.width),
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

pub fn action_menu_height(state: &TransientMenuState, width: u16) -> u16 {
    let sections = action_menu_sections(state);
    let columns = layout::action_menu_columns(width).min(sections.len().max(1));
    let bands = sections.len().div_ceil(columns);
    let content_height = sections
        .chunks(columns)
        .map(|band| {
            band.iter()
                .map(|section| section.items.len().saturating_add(1))
                .max()
                .map_or(0, |height| height)
        })
        .sum::<usize>()
        .saturating_add(bands.saturating_sub(1));
    u16::try_from(content_height.saturating_add(4))
        .map_or(u16::MAX, |height| height)
        .max(14)
}

pub fn action_menu_action_at(
    state: &TransientMenuState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<action::ActionId> {
    let sections = action_menu_sections(state);
    let columns = layout::action_menu_columns(area.width).min(sections.len().max(1));
    let separator_width = columns.saturating_sub(1).saturating_mul(3);
    let cell_width = usize::from(area.width).saturating_sub(separator_width) / columns;
    let stride = cell_width.saturating_add(3);
    let relative_x = usize::from(column.saturating_sub(area.x));
    let selected_column = relative_x / stride.max(1);
    if selected_column >= columns || relative_x % stride.max(1) >= cell_width {
        return None;
    }
    let content_row = usize::from(row.saturating_sub(area.y)).checked_sub(2)?;
    let mut band_y = 0_usize;
    for band in sections.chunks(columns) {
        let height = band
            .iter()
            .map(|section| section.items.len().saturating_add(1))
            .max()?;
        if content_row >= band_y && content_row < band_y.saturating_add(height) {
            let item_row = content_row.saturating_sub(band_y).checked_sub(1)?;
            return band
                .get(selected_column)?
                .items
                .get(item_row)
                .map(|item| item.id);
        }
        band_y = band_y.saturating_add(height).saturating_add(1);
    }
    None
}

fn action_menu_lines(app: &App, state: &TransientMenuState, area: Rect) -> Vec<Line<'static>> {
    let sections = action_menu_sections(state);
    let columns = layout::action_menu_columns(area.width).min(sections.len().max(1));
    let separator_width = columns.saturating_sub(1).saturating_mul(3);
    let available_width = usize::from(area.width).saturating_sub(separator_width);
    let cell_width = available_width / columns;
    let mut content = Vec::new();
    for (band_index, band) in sections.chunks(columns).enumerate() {
        if band_index > 0 {
            content.push(Line::default());
        }
        let height = band
            .iter()
            .map(|section| section.items.len().saturating_add(1))
            .max()
            .map_or(0, |height| height);
        for row in 0..height {
            let mut spans = Vec::new();
            for column in 0..columns {
                if column > 0 {
                    spans.push(Span::raw("   "));
                }
                if let Some(section) = band.get(column) {
                    spans.extend(action_menu_section_line(
                        app, state, section, row, cell_width,
                    ));
                } else {
                    spans.push(Span::raw(" ".repeat(cell_width)));
                }
            }
            content.push(Line::from(spans));
        }
    }
    let mut lines = vec![action_menu_header(app, state), Line::default()];
    let content_budget = usize::from(area.height.saturating_sub(4));
    let content_height = content.len();
    lines.extend(content.into_iter().take(content_budget));
    if content_height > content_budget
        && let Some(line) = lines.last_mut()
    {
        *line = Line::styled(
            "… more actions need a taller terminal",
            app.theme.style(theme::StyleRole::StateWarning),
        );
    }
    while lines.len().saturating_sub(2) < content_budget {
        lines.push(Line::default());
    }
    lines.push(Line::default());
    lines.push(action_menu_status(app, state));
    lines
}

fn action_menu_header(app: &App, state: &TransientMenuState) -> Line<'static> {
    let escape_label = if state.prefix.is_some() {
        " Back"
    } else {
        " Close"
    };
    Line::from(vec![
        Span::styled("Actions", app.theme.style(theme::StyleRole::Focus)),
        Span::raw("   "),
        Span::styled("?", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(" Help", app.theme.style(theme::StyleRole::TextMuted)),
        Span::raw("   "),
        Span::styled("Esc", app.theme.style(theme::StyleRole::KeyHint)),
        Span::styled(escape_label, app.theme.style(theme::StyleRole::TextMuted)),
    ])
}

fn action_menu_section_line(
    app: &App,
    state: &TransientMenuState,
    section: &ActionMenuSection,
    row: usize,
    width: usize,
) -> Vec<Span<'static>> {
    if row == 0 {
        return action_group_heading(app, state, section, width);
    }
    section.items.get(row.saturating_sub(1)).map_or_else(
        || vec![Span::raw(" ".repeat(width))],
        |item| action_menu_item(app, state, item, width),
    )
}

fn action_group_heading(
    app: &App,
    state: &TransientMenuState,
    section: &ActionMenuSection,
    width: usize,
) -> Vec<Span<'static>> {
    let active = state.prefix.is_none_or(|prefix| {
        section
            .items
            .iter()
            .any(|item| item.sequence.starts_with(prefix))
    });
    let heading = format!(" {} ", section.group.label());
    let heading_width = heading.chars().count();
    let style = if active && section.group == action::TransientGroup::Danger {
        app.theme.style(theme::StyleRole::RiskDestructive)
    } else if active {
        app.theme.style(theme::StyleRole::SectionHeading)
    } else {
        app.theme.style(theme::StyleRole::TextDisabled)
    };
    vec![
        Span::styled(heading, style),
        Span::styled(
            " ".repeat(width.saturating_sub(heading_width)),
            app.theme.style(theme::StyleRole::SurfaceRaised),
        ),
    ]
}

fn action_menu_item(
    app: &App,
    state: &TransientMenuState,
    item: &ActionMenuItem,
    width: usize,
) -> Vec<Span<'static>> {
    let unavailable = app.action_unavailable_reason(item.id).is_some();
    let prefix_mismatch = state
        .prefix
        .is_some_and(|prefix| !item.sequence.starts_with(prefix));
    let disabled = unavailable || prefix_mismatch;
    let key_style = if disabled {
        app.theme.style(theme::StyleRole::KeyHintDisabled)
    } else {
        match item.risk {
            action::Risk::Observe => app.theme.style(theme::StyleRole::KeyHint),
            risk => app.theme.style(risk.style_role()),
        }
    };
    let label_style = if disabled {
        app.theme.style(theme::StyleRole::TextDisabled)
    } else if item.risk == action::Risk::DestructiveOrSecret {
        app.theme.style(theme::StyleRole::RiskDestructive)
    } else {
        app.theme.style(theme::StyleRole::TextMuted)
    };
    let key_width = item.sequence.chars().count();
    let label_budget = width.saturating_sub(key_width.saturating_add(1));
    let label = text::ellipsize(&item.label.to_ascii_lowercase(), label_budget);
    let used = key_width
        .saturating_add(1)
        .saturating_add(label.chars().count());
    vec![
        Span::styled(item.sequence, key_style),
        Span::styled(" ", app.theme.style(theme::StyleRole::SurfaceRaised)),
        Span::styled(label, label_style),
        Span::styled(
            " ".repeat(width.saturating_sub(used)),
            app.theme.style(theme::StyleRole::SurfaceRaised),
        ),
    ]
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
    Line::styled(
        "Keys activate immediately",
        app.theme.style(theme::StyleRole::TextMuted),
    )
}

fn copy_menu_lines(state: &TransientMenuState, width: u16) -> Vec<Line<'static>> {
    let mut items = vec!["Copy".to_owned()];
    for field in &state.fields {
        items.push(format!("{} {}", copy_key(*field), field.label()));
    }
    if let Some(message) = &state.message {
        items.push(message.clone());
    }
    items.push("? help".to_owned());
    items.push("Esc cancel".to_owned());
    let capacity = usize::from(width);
    let mut visible = Vec::new();
    let mut used = 0_usize;
    for (index, item) in items.iter().enumerate() {
        let separator = usize::from(!visible.is_empty()) * 2;
        let reserve = if index.saturating_add(1) < items.len() {
            "  … +99  ? help  Esc cancel".len()
        } else {
            0
        };
        if used
            .saturating_add(separator)
            .saturating_add(item.len())
            .saturating_add(reserve)
            > capacity
        {
            let hidden = items.len().saturating_sub(index);
            visible.push(format!("… +{hidden}"));
            visible.push("? help".to_owned());
            visible.push("Esc cancel".to_owned());
            break;
        }
        used = used.saturating_add(separator).saturating_add(item.len());
        visible.push(item.clone());
    }
    vec![Line::from(visible.join("  "))]
}

fn help_lines(app: &App, query: &str, scroll: usize, area: Rect) -> Vec<Line<'static>> {
    let query = query.to_ascii_lowercase();
    let context = context(app);
    let mut lines = vec![
        Line::from("help · / filter · ? or Esc close"),
        Line::from("Navigation · collection"),
    ];
    for spec in action::all_actions() {
        if !spec.contexts.contains(&context) || spec.default_bindings.is_empty() {
            continue;
        }
        if !query.is_empty()
            && !spec.label.to_ascii_lowercase().contains(&query)
            && !spec.default_bindings[0]
                .label()
                .to_ascii_lowercase()
                .contains(&query)
        {
            continue;
        }
        let disabled = app
            .action_unavailable_reason(spec.id)
            .map_or_else(String::new, |reason| format!(" [disabled: {reason}]"));
        lines.push(Line::from(format!(
            "{:>8}  {}{}",
            spec.default_bindings[0].label(),
            spec.label,
            disabled
        )));
    }
    lines.push(Line::from("Actions"));
    for id in app.contextual_actions() {
        let Some(sequence) = action::transient_sequence(id) else {
            continue;
        };
        let Some(spec) = action::find_action(id) else {
            continue;
        };
        if !query.is_empty()
            && !spec.label.to_ascii_lowercase().contains(&query)
            && !sequence.contains(&query)
        {
            continue;
        }
        let disabled = app
            .action_unavailable_reason(id)
            .map_or_else(String::new, |reason| format!(" [disabled: {reason}]"));
        lines.push(Line::from(format!(
            "a {sequence:>2}  {}{disabled}",
            spec.label
        )));
    }
    lines.push(Line::from("Copy"));
    for field in app.contextual_copy_fields() {
        let key = copy_key(field);
        if query.is_empty()
            || field.label().to_ascii_lowercase().contains(&query)
            || key.to_string().contains(&query)
        {
            lines.push(Line::from(format!("y {key:>2}  {}", field.label())));
        }
    }
    lines.push(Line::from("Tasks and refresh"));
    lines.push(Line::from("r refresh   R refresh all   @ tasks"));
    lines.push(Line::from("Global and exit"));
    lines.push(Line::from("[ back   ] forward   q quit   Esc cancel"));
    lines.push(Line::from(
        "Legend: ! destructive   … prefix   [disabled: reason]",
    ));
    let height = usize::from(area.height);
    lines.into_iter().skip(scroll).take(height).collect()
}

fn context(app: &App) -> ActionContext {
    match app.current_route() {
        Route::Activity => ActionContext::Activity,
        Route::Devices | Route::Services if app.focus == Focus::Inspector => ActionContext::Detail,
        Route::Devices | Route::Users | Route::Routes | Route::Credentials | Route::Services => {
            ActionContext::Collection
        }
        _ => ActionContext::Root,
    }
}

const fn copy_key(field: CopyField) -> char {
    match field {
        CopyField::DeviceId => 'i',
        CopyField::DisplayName => 'n',
        CopyField::Hostname => 'h',
        CopyField::Owner => 'o',
        CopyField::Addresses => 'a',
        CopyField::Tags => 't',
        CopyField::PublicKey => 'p',
        CopyField::Endpoint => 'e',
        CopyField::DiagnosticSummary => 'd',
        CopyField::Metrics => 'm',
    }
}
