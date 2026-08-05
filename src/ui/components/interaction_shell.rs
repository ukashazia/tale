use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::action::{self, ActionContext};
use crate::app::{
    App, CopyField, Focus, InteractionMode, Route, TransientKind, TransientMenuState,
};
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let lines = match &app.interaction {
        InteractionMode::Normal => normal_lines(app, area.width),
        InteractionMode::CommandLine(state) => {
            let mut lines = completion_lines(
                &state.candidates,
                state.selected_completion,
                area.height.saturating_sub(1),
            );
            lines.push(prompt_line(
                ':',
                &state.editor.input,
                state.editor.cursor,
                state.error.as_deref(),
                area.width,
            ));
            lines
        }
        InteractionMode::FilterLine(state) => {
            let mut lines = completion_lines(
                &state.candidates,
                state.selected_completion,
                area.height.saturating_sub(1),
            );
            lines.push(prompt_line(
                '/',
                &state.editor.input,
                state.editor.cursor,
                state.error.as_deref(),
                area.width,
            ));
            lines
        }
        InteractionMode::Transient(state) => transient_lines(app, state, area.width),
        InteractionMode::HelpSheet(state) => help_lines(app, &state.query, state.scroll, area),
    };
    frame.render_widget(Paragraph::new(lines).style(theme::normal(app)), area);
}

pub fn render_minimum(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let prompt = match &app.interaction {
        InteractionMode::CommandLine(state) => Some(prompt_line(
            ':',
            &state.editor.input,
            state.editor.cursor,
            state.error.as_deref(),
            area.width,
        )),
        InteractionMode::FilterLine(state) => Some(prompt_line(
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
            Paragraph::new(prompt).style(theme::normal(app)),
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

fn completion_lines(
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
            Line::from(format!(
                "{} {:<16} {}{}",
                if selected == Some(index) { ">" } else { " " },
                candidate.label,
                candidate.description,
                if candidate.alias { " · alias" } else { "" }
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
        Span::raw(format!(
            "{prefix} {}{before}",
            if clipped_left { "‹" } else { "" }
        )),
        Span::raw("█"),
        Span::raw(format!("{after}{}", if clipped_right { "›" } else { "" })),
    ];
    if let Some(error) = error {
        spans.push(Span::raw(format!("  {error}")));
    }
    Line::from(spans)
}

fn transient_lines(app: &App, state: &TransientMenuState, width: u16) -> Vec<Line<'static>> {
    let mut items = Vec::new();
    match state.kind {
        TransientKind::Action => {
            let breadcrumb = state
                .prefix
                .map_or("Actions".to_owned(), |prefix| format!("Actions › {prefix}"));
            items.push(breadcrumb);
            for id in &state.actions {
                let Some(sequence) = action::transient_sequence(*id) else {
                    continue;
                };
                if let Some(prefix) = state.prefix {
                    if !sequence.starts_with(prefix) || sequence.len() != 2 {
                        continue;
                    }
                } else if sequence.len() == 2 {
                    let group = sequence.chars().next().map_or(' ', |value| value);
                    let marker = format!("{group} …");
                    if !items.contains(&marker) {
                        items.push(marker);
                    }
                    continue;
                }
                let key = sequence.chars().last().map_or(' ', |value| value);
                let label = action::find_action(*id).map_or(id.as_str(), |spec| spec.label);
                let disabled = app
                    .action_unavailable_reason(*id)
                    .map_or_else(String::new, |reason| format!(" [disabled: {reason}]"));
                items.push(format!("{key} {label}{disabled}"));
            }
        }
        TransientKind::Copy => {
            items.push("Copy".to_owned());
            for field in &state.fields {
                items.push(format!("{} {}", copy_key(*field), field.label()));
            }
        }
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
