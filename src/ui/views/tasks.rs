use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};

use crate::app::{App, Focus};
use crate::task::{Task, TaskState};
use crate::ui::components::{grid, panel};
use crate::ui::{text, theme};

/// When a column is worth its width. Tasks have no `w columns` key — that is
/// offered on `:devices` only — so the tiers read the terminal instead.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Tier {
    Always,
    /// Wide enough for how long the work took and how far it got.
    Wide,
    /// Wide enough to carry the one-line result as well.
    Widest,
}

/// Header, width, and when it appears. The order here is the order on screen.
const COLUMNS: &[(&str, Tier, grid::Width)] = &[
    ("S", Tier::Always, grid::Width::Fixed(2)),
    ("#", Tier::Always, grid::Width::Fixed(4)),
    ("ACTION", Tier::Always, grid::Width::Fill(18)),
    ("TARGET", Tier::Always, grid::Width::Fill(16)),
    ("STATE", Tier::Always, grid::Width::Fill(10)),
    ("PROGRESS", Tier::Wide, grid::Width::Fill(8)),
    ("STARTED", Tier::Always, grid::Width::Fill(7)),
    ("TOOK", Tier::Wide, grid::Width::Fill(7)),
    ("RESULT", Tier::Widest, grid::Width::Fill(22)),
];

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, wide_inspector: Option<Rect>) {
    if app.focus == Focus::Inspector || wide_inspector.is_none() {
        // `i` hides the side pane, so a narrow terminal is not the only reason
        // the table can have the whole width.
        if app.focus == Focus::Inspector {
            render_inspector(frame, app, area);
        } else {
            render_table(frame, app, area);
        }
        return;
    }
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    render_table(frame, app, horizontal[0]);
    if let Some(inspector_area) = wide_inspector {
        render_inspector(frame, app, inspector_area);
    } else {
        render_inspector(frame, app, horizontal[1]);
    }
}

fn render_table(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let tasks = app.tasks.filtered(&app.task_filter).collect::<Vec<_>>();
    let lines = if tasks.is_empty() {
        empty_state(app)
    } else {
        let shows = |tier: Tier| match tier {
            Tier::Always => true,
            Tier::Wide => area.width >= 100,
            Tier::Widest => area.width >= 140,
        };
        let columns = COLUMNS
            .iter()
            .filter(|(_, tier, _)| shows(*tier))
            .map(|(header, _, width)| grid::Column {
                header: (*header).to_owned(),
                width: *width,
            })
            .collect::<Vec<_>>();
        let rows = visible_tasks(app, &tasks, area)
            .map(|(task, selected)| {
                let cells = COLUMNS
                    .iter()
                    .filter(|(_, tier, _)| shows(*tier))
                    .map(|(header, _, _)| cell(app, task, header))
                    .collect::<Vec<_>>();
                grid::Row::new(cells).selected(selected)
            })
            .collect::<Vec<_>>();
        grid::lines(app, &columns, rows, area.width.saturating_sub(4))
    };
    panel::render_view(frame, app, area, title(app), lines);
}

/// Nothing here is fetched, so an empty page means either that this session has
/// not run anything yet or that the filter excluded what it did run.
fn empty_state(app: &App) -> Vec<Line<'static>> {
    if app.task_history_loading {
        return vec![text::muted_help(app.theme, "Loading task history…")];
    }
    if app.tasks.all().is_empty() {
        return vec![
            text::muted_help(app.theme, "No tasks yet"),
            Line::default(),
            text::muted_help(
                app.theme,
                "Every action that runs in the background records itself here:",
            ),
            text::muted_help(app.theme, "what ran, against what, and what came back."),
        ];
    }
    vec![
        text::muted_help(app.theme, "No tasks match the filter"),
        Line::default(),
        text::action_hint(
            app.theme,
            "  filter           ",
            format!("/{}", app.task_filter),
        ),
        text::action_hint(
            app.theme,
            "  clear it         ",
            "/ then Enter on an empty line",
        ),
    ]
}

/// The row again, one fact per line, followed by whatever the task wrote. Only
/// what the run actually reported: a missing exit status describes the task,
/// not the client.
fn render_inspector(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(task) = app.focused_task() else {
        panel::render(frame, app, area, "inspector", "No task selected");
        return;
    };
    let width = usize::from(area.width.saturating_sub(4));
    let action_label = crate::action::find_action(task.action_id)
        .map_or(task.action_id.as_str(), |action| action.label);
    let mut pairs = vec![
        ("id", task.id.to_string()),
        ("action", action_label.to_owned()),
        ("target", task.target_label.clone()),
        ("state", task.state.label().to_owned()),
        ("started", ago(app, task.started_at)),
    ];
    if let Some(finished) = task.finished_at {
        pairs.push(("finished", ago(app, finished)));
        pairs.push((
            "took",
            text::format_age(finished.saturating_sub(task.started_at)),
        ));
    } else {
        pairs.push((
            "running for",
            text::format_age(app.now.saturating_sub(task.started_at)),
        ));
    }
    if let Some(progress) = task.progress {
        pairs.push((
            "progress",
            format!("{} of {}", progress.completed, progress.total),
        ));
    }
    pairs.push((
        "cancellable",
        if task.cancellable { "yes" } else { "no" }.to_owned(),
    ));
    if let Some(status) = task.exit_status {
        pairs.push(("exit status", status.to_string()));
    }
    if let Some(verification) = task.verification.as_deref() {
        pairs.push(("confirmed", verification.to_owned()));
    }
    if !task.requested_fields.is_empty() {
        pairs.push(("fields", task.requested_fields.join(", ")));
    }
    if !task.redacted_argv.is_empty() {
        pairs.push(("command", task.redacted_argv.join(" ")));
    }
    pairs.push(("result", task.summary.clone()));

    let mut lines = vec![Line::from(Span::styled(
        text::ellipsize(&format!("{} · {}", action_label, task.target_label), width),
        app.theme.style(theme::StyleRole::TextPrimary),
    ))];
    lines.extend(grid::detail(app, &pairs));
    if !task.changes.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "changes",
            app.theme.style(theme::StyleRole::SectionHeading),
        )));
        for change in &task.changes {
            let before = change.before.as_deref().unwrap_or("—");
            let after = change.after.as_deref().unwrap_or("—");
            lines.push(Line::from(format!(
                "{}: {} → {}",
                change.field, before, after
            )));
        }
    }
    lines.extend(output_lines(app, task, area, lines.len()));
    panel::render_focusable(
        frame,
        app,
        area,
        "inspector",
        lines,
        app.focus == Focus::Inspector,
    );
}

/// Output can run to a quarter of a megabyte, and the end is the part anyone
/// reads. The heading says how much was left above so the tail does not pass
/// itself off as the whole of it.
fn output_lines(app: &App, task: &Task, area: Rect, used: usize) -> Vec<Line<'static>> {
    if task.detail.is_empty() {
        return Vec::new();
    }
    let body = task.detail.lines().collect::<Vec<_>>();
    let room = usize::from(area.height.saturating_sub(2))
        .saturating_sub(used)
        .saturating_sub(2);
    if room == 0 {
        return Vec::new();
    }
    let hidden = body.len().saturating_sub(room);
    let heading = if hidden == 0 {
        format!("output · {} lines", body.len())
    } else {
        format!("output · last {room} of {} lines", body.len())
    };
    let mut lines = vec![
        Line::from(String::new()),
        Line::from(Span::styled(
            heading,
            app.theme.style(theme::StyleRole::SectionHeading),
        )),
    ];
    lines.extend(body.into_iter().skip(hidden).map(|line| {
        Line::from(Span::styled(
            line.to_owned(),
            app.theme.style(theme::StyleRole::TextCode),
        ))
    }));
    lines
}

fn ago(app: &App, moment: crate::domain::Timestamp) -> String {
    format!("{} ago", text::format_age(app.now.saturating_sub(moment)))
}

/// Where the window over the history starts. The table and the mouse both call
/// this, so a click lands on the row the pointer is over.
pub fn window_start(selected: usize, len: usize, viewport: usize) -> usize {
    selected
        .saturating_add(1)
        .saturating_sub(viewport)
        .min(len.saturating_sub(1))
}

fn visible_tasks<'a>(
    app: &App,
    tasks: &'a [&'a Task],
    area: Rect,
) -> impl Iterator<Item = (&'a Task, bool)> {
    let viewport = usize::from(area.height.saturating_sub(3)).max(1);
    let selected = app.task_cursor();
    let start = window_start(selected, tasks.len(), viewport);
    tasks
        .iter()
        .enumerate()
        .skip(start)
        .take(viewport)
        .map(move |(index, task)| (*task, index == selected && app.tasks.selected.is_some()))
}

const fn state_role(state: TaskState) -> theme::StyleRole {
    match state {
        TaskState::Queued => theme::StyleRole::TaskQueued,
        TaskState::Running | TaskState::Cancelling => theme::StyleRole::TaskRunning,
        TaskState::Succeeded => theme::StyleRole::TaskSucceeded,
        TaskState::Failed => theme::StyleRole::TaskFailed,
        TaskState::Cancelled | TaskState::Interrupted => theme::StyleRole::TaskCancelled,
    }
}

/// The one cell that means something the row does not: how the run ended, or
/// that it has not.
fn state_cell(app: &App, task: &Task) -> grid::Cell {
    let role = state_role(task.state);
    let signal = role.signal();
    let marker = if app.resolved_config.ui.symbols.unicode() {
        signal.unicode
    } else {
        signal.ascii
    };
    grid::Cell::new(marker).with_role(role)
}

fn cell(app: &App, task: &Task, header: &str) -> grid::Cell {
    match header {
        "S" => state_cell(app, task),
        "#" => grid::Cell::new(task.id.0.to_string()),
        "ACTION" => grid::Cell::new(task.action_id.as_str()),
        "TARGET" => grid::Cell::new(task.target_label.clone()),
        "STATE" => grid::Cell::new(task.state.label()),
        "PROGRESS" => grid::Cell::new(task.progress.map_or_else(
            || "-".to_owned(),
            |progress| format!("{}/{}", progress.completed, progress.total),
        )),
        "STARTED" => grid::Cell::new(text::format_age(app.now.saturating_sub(task.started_at))),
        "TOOK" => grid::Cell::new(task.finished_at.map_or_else(
            || "-".to_owned(),
            |finished| text::format_age(finished.saturating_sub(task.started_at)),
        )),
        "RESULT" => grid::Cell::new(task.summary.clone()),
        _ => grid::Cell::new("-"),
    }
}

/// Route context lives in the border, the way it does on every other route.
fn title(app: &App) -> ratatui::text::Line<'static> {
    let mut detail = Vec::new();
    if !app.task_filter.is_empty() {
        detail.push(format!("/{}", text::ellipsize(&app.task_filter, 32)));
    }
    let active = app.tasks.active().count();
    if active > 0 {
        detail.push(format!("{active} running"));
    }
    text::view_title(
        app.theme,
        "tasks",
        app.filtered_task_count(),
        app.tasks.all().len(),
        &detail,
    )
}
