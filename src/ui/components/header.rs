use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::ui::text;
use crate::ui::theme;

/// The wordmark. Pure ASCII so it needs no symbol fallback.
const LOGO: [&str; 4] = [
    "   ______      __   ",
    "  /_  __/___ _/ /__ ",
    "   / / / __ `/ / _ \\",
    "  /_/  \\__,_/_/\\___/",
];

/// Rows the tall header occupies, including the blank above and below.
pub const TALL_ROWS: u16 = 6;
/// Below this the header collapses to a single line so the content keeps room.
const TALL_MINIMUM_HEIGHT: u16 = 26;

pub const fn rows(available: u16) -> u16 {
    if available >= TALL_MINIMUM_HEIGHT {
        TALL_ROWS
    } else {
        1
    }
}

/// A logo, then what the session is doing, then the versions — spaced apart
/// rather than packed together, and carrying only what is worth a permanent row.
pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.height < TALL_ROWS {
        frame.render_widget(
            Paragraph::new(compact_line(app)).style(app.theme.style(theme::StyleRole::Surface)),
            area,
        );
        return;
    }
    let status = status_block(app);
    let versions = version_block(app);
    const VERSION_LABEL: usize = 11;
    let version_width = versions
        .iter()
        .map(|(_, value)| VERSION_LABEL.saturating_add(value.chars().count()))
        .max()
        .map_or(0, |width| width);
    let logo_width = LOGO
        .iter()
        .map(|line| line.chars().count())
        .max()
        .map_or(0, |width| width);
    let gap = 6_usize;
    let mut lines = vec![Line::default()];
    for row in 0..4 {
        let mut spans = vec![Span::styled(
            LOGO.get(row).map_or("", |line| line).to_owned(),
            app.theme.style(theme::StyleRole::Focus),
        )];
        // The status block sits against the middle of the logo, not its top.
        let detail = row.checked_sub(1).and_then(|index| status.get(index));
        let version = row.checked_sub(1).and_then(|index| versions.get(index));
        let mut used = logo_width;
        if let Some(detail) = detail {
            spans.push(Span::styled(
                " ".repeat(gap),
                app.theme.style(theme::StyleRole::Surface),
            ));
            used = used.saturating_add(gap);
            for span in detail {
                used = used.saturating_add(span.content.chars().count());
                spans.push(span.clone());
            }
        }
        if let Some((name, value)) = version {
            let column = usize::from(area.width)
                .saturating_sub(version_width)
                .saturating_sub(2);
            spans.push(Span::styled(
                " ".repeat(column.saturating_sub(used).max(gap)),
                app.theme.style(theme::StyleRole::Surface),
            ));
            spans.push(Span::styled(
                format!("{name:<VERSION_LABEL$}"),
                app.theme.style(theme::StyleRole::TextMuted),
            ));
            spans.push(Span::styled(
                value.clone(),
                app.theme.style(theme::StyleRole::TextPrimary),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::default());
    frame.render_widget(
        Paragraph::new(lines).style(app.theme.style(theme::StyleRole::Surface)),
        area,
    );
}

/// Up to three rows: what the session is doing, who it is, and anything that
/// currently needs attention.
fn status_block(app: &App) -> Vec<Vec<Span<'static>>> {
    let (label, role, hint) = connection_state(app);
    let mut first = vec![
        Span::styled("Status:  ", app.theme.style(theme::StyleRole::TextMuted)),
        Span::styled(
            format!(" {label} "),
            app.theme.style(role).add_modifier(Modifier::REVERSED),
        ),
    ];
    if let Some(hint) = hint {
        first.push(Span::styled(
            format!("  ({hint})"),
            app.theme.style(theme::StyleRole::TextMuted),
        ));
    }
    let mut block = vec![first];
    let mut second = Vec::new();
    if let Some(identity) = tailnet_identity(app) {
        second.push(Span::styled(
            identity,
            app.theme.style(theme::StyleRole::TextPrimary),
        ));
    }
    // Freshness is silent while the data is current; it only speaks up when
    // the snapshot has fallen behind.
    if let Some((note, role)) = staleness(app) {
        if !second.is_empty() {
            second.push(Span::styled(
                "   ",
                app.theme.style(theme::StyleRole::Surface),
            ));
        }
        second.push(Span::styled(note, app.theme.style(role)));
    }
    if let Some((tasks, role)) = task_state(app) {
        if !second.is_empty() {
            second.push(Span::styled(
                "   ",
                app.theme.style(theme::StyleRole::Surface),
            ));
        }
        second.push(Span::styled(tasks, app.theme.style(role)));
    }
    if !second.is_empty() {
        block.push(second);
    }
    block
}

fn version_block(app: &App) -> Vec<(&'static str, String)> {
    let mut versions = vec![("tale:", env!("CARGO_PKG_VERSION").to_owned())];
    // Only claim a Tailscale version when one was actually read.
    if let Some(version) = app
        .local_executable
        .as_ref()
        .map(|executable| executable.version.clone())
        .or_else(|| {
            app.local_resource
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.client_version.clone())
        })
    {
        versions.push(("tailscale:", version));
    }
    versions
}

fn compact_line(app: &App) -> Line<'static> {
    let (label, role, _) = connection_state(app);
    let mut spans = vec![Span::styled(label, app.theme.style(role))];
    if let Some(identity) = tailnet_identity(app) {
        spans.push(Span::styled(
            "   ",
            app.theme.style(theme::StyleRole::Surface),
        ));
        spans.push(Span::styled(
            identity,
            app.theme.style(theme::StyleRole::TextPrimary),
        ));
    }
    if let Some((note, role)) = staleness(app) {
        spans.push(Span::styled(
            "   ",
            app.theme.style(theme::StyleRole::Surface),
        ));
        spans.push(Span::styled(note, app.theme.style(role)));
    }
    if let Some((tasks, role)) = task_state(app) {
        spans.push(Span::styled(
            "   ",
            app.theme.style(theme::StyleRole::Surface),
        ));
        spans.push(Span::styled(tasks, app.theme.style(role)));
    }
    Line::from(spans)
}

/// Nothing is said while the snapshot is current; a stale or unreachable source
/// is worth a permanent row, a fresh one is not.
fn staleness(app: &App) -> Option<(String, theme::StyleRole)> {
    match app.devices_resource.health {
        crate::domain::SourceHealth::Stale => Some((
            app.devices_resource.observed_at.map_or_else(
                || "data stale".to_owned(),
                |observed| {
                    format!(
                        "data stale · last updated {} ago",
                        text::format_age(app.now.saturating_sub(observed))
                    )
                },
            ),
            theme::StyleRole::StateStale,
        )),
        crate::domain::SourceHealth::Error | crate::domain::SourceHealth::Unavailable => Some((
            "data unavailable · r to retry".to_owned(),
            theme::StyleRole::StateDanger,
        )),
        crate::domain::SourceHealth::Loading | crate::domain::SourceHealth::Healthy => None,
    }
}

/// The tailnet the user is looking at, falling back to the configured profile.
fn tailnet_identity(app: &App) -> Option<String> {
    app.admin
        .tailnet
        .as_deref()
        .or_else(|| {
            app.local_resource
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.current_tailnet.as_deref())
        })
        .or(app.resolved_config.profile.as_deref())
        .map(str::to_owned)
}

/// The label, its meaning, and the key that acts on it when there is one.
fn connection_state(app: &App) -> (String, theme::StyleRole, Option<&'static str>) {
    use crate::domain::source::LocalDaemonState;
    match app.source_mode {
        crate::app::SourceMode::Mock => (
            "Simulated data".to_owned(),
            theme::StyleRole::StateUnknown,
            None,
        ),
        crate::app::SourceMode::Unavailable => (
            "No local connection".to_owned(),
            theme::StyleRole::StateDanger,
            None,
        ),
        crate::app::SourceMode::Local => match &app.local_daemon_state {
            LocalDaemonState::Live => (
                "Connected locally".to_owned(),
                theme::StyleRole::StateHealthy,
                None,
            ),
            LocalDaemonState::Connecting => (
                "Connecting".to_owned(),
                theme::StyleRole::StatePending,
                None,
            ),
            LocalDaemonState::Reconnecting => (
                "Reconnecting".to_owned(),
                theme::StyleRole::StateWarning,
                None,
            ),
            LocalDaemonState::Disabled => (
                "Local access off".to_owned(),
                theme::StyleRole::TextMuted,
                None,
            ),
            LocalDaemonState::Mock => (
                "Simulated data".to_owned(),
                theme::StyleRole::StateUnknown,
                None,
            ),
            LocalDaemonState::PermissionDenied { .. } => (
                "Local access denied".to_owned(),
                theme::StyleRole::StateDanger,
                Some("press a d for diagnostics"),
            ),
            LocalDaemonState::Unsupported { .. } => (
                "Local client unsupported".to_owned(),
                theme::StyleRole::StateDanger,
                None,
            ),
            LocalDaemonState::Unavailable { .. } => (
                "Local daemon unreachable".to_owned(),
                theme::StyleRole::StateDanger,
                Some("press r to retry"),
            ),
        },
    }
}

fn task_state(app: &App) -> Option<(String, theme::StyleRole)> {
    let running = app.tasks.active().count();
    if running > 0 {
        let plural = if running == 1 { "task" } else { "tasks" };
        return Some((
            format!("{running} {plural} running"),
            theme::StyleRole::TaskRunning,
        ));
    }
    let failed = app
        .tasks
        .all()
        .iter()
        .filter(|task| task.state == crate::task::TaskState::Failed)
        .count();
    if failed > 0 {
        let plural = if failed == 1 { "task" } else { "tasks" };
        return Some((
            format!("{failed} {plural} failed"),
            theme::StyleRole::TaskFailed,
        ));
    }
    None
}
