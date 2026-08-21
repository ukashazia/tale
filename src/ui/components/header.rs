use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;
use crate::ui::text;
use crate::ui::theme;

/// The wordmark. Pure ASCII so it needs no symbol fallback.
const LOGO: &str = include_str!("../tale-header-title.txt");

/// Rows the tall header occupies, including the blank above and below.
/// Below this the header collapses to a single line so the content keeps room.
const TALL_MINIMUM_HEIGHT: u16 = 26;

pub fn rows(available: u16) -> u16 {
    if available >= TALL_MINIMUM_HEIGHT {
        tall_rows()
    } else {
        1
    }
}

fn tall_rows() -> u16 {
    u16::try_from(LOGO.trim_end().lines().count()).map_or(u16::MAX, |rows| rows.saturating_add(2))
}

/// A logo, then what the session is doing, then the versions — spaced apart
/// rather than packed together, and carrying only what is worth a permanent row.
pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.height < tall_rows() {
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
        .unwrap_or(0);
    let logo = LOGO.trim_end().lines().collect::<Vec<_>>();
    let logo_width = logo
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let detail_start = logo.len().saturating_sub(status.len()) / 2;
    let gap = 6_usize;
    let mut lines = vec![Line::default()];
    for (row, logo_line) in logo.iter().enumerate() {
        let mut spans = vec![Span::styled(
            *logo_line,
            app.theme.style(theme::StyleRole::Focus),
        )];
        // The status block sits against the middle of the logo, not its top.
        let detail = row
            .checked_sub(detail_start)
            .and_then(|index| status.get(index));
        let version = row
            .checked_sub(detail_start)
            .and_then(|index| versions.get(index));
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

/// The width the three labels share, so their values start in one column.
const STATUS_LABEL: usize = 10;

/// Three rows, one fact each: how the session is doing, what this machine is
/// connected to, and what it is administering.
///
/// These used to be the state, then an unlabelled name, then an unlabelled
/// domain. Two tailnets on screen at once — the client's and the profile's —
/// made that unreadable: three bare names with nothing saying which was which,
/// and no way to tell a tailnet from a MagicDNS suffix. Every row is named now,
/// so the two identities can differ without the header becoming a guess.
fn status_block(app: &App) -> Vec<Vec<Span<'static>>> {
    let (label, role, hint) = connection_state(app);
    let mut first = vec![
        // One short of the shared width: the chip carries its own leading space
        // so the highlight has a margin, and that space is what lands the three
        // values in one column.
        Span::styled(
            format!(
                "{:<width$}",
                "Status:",
                width = STATUS_LABEL.saturating_sub(1)
            ),
            app.theme.style(theme::StyleRole::TextMuted),
        ),
        Span::styled(
            format!(" {label} "),
            app.theme.style(role).add_modifier(Modifier::REVERSED),
        ),
    ];
    if let Some((before, action, after)) = hint {
        first.push(Span::styled(
            format!("  ({before}"),
            app.theme.style(theme::StyleRole::TextMuted),
        ));
        first.push(Span::styled(
            action,
            app.theme.style(theme::StyleRole::KeyHint),
        ));
        first.push(Span::styled(
            format!("{after})"),
            app.theme.style(theme::StyleRole::TextMuted),
        ));
    }
    // Freshness and running work belong with the state they qualify, not on a
    // row of their own below two identities they say nothing about. Both stay
    // silent while there is nothing wrong.
    for (note, role) in [staleness(app), profile_loading(app), task_state(app)]
        .into_iter()
        .flatten()
    {
        first.push(Span::styled(
            "   ",
            app.theme.style(theme::StyleRole::Surface),
        ));
        first.push(Span::styled(note, app.theme.style(role)));
    }
    vec![
        first,
        labelled_row(app, "Local:", local_identity(app)),
        labelled_row(app, "Profile:", profile_identity(app)),
    ]
}

fn labelled_row(app: &App, label: &str, value: IdentityRow) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        format!("{label:<STATUS_LABEL$}"),
        app.theme.style(theme::StyleRole::TextMuted),
    )];
    spans.push(Span::styled(
        value.primary,
        app.theme.style(if value.present {
            theme::StyleRole::TextPrimary
        } else {
            theme::StyleRole::TextMuted
        }),
    ));
    if let Some(action) = value.action {
        spans.push(Span::styled(
            action,
            app.theme.style(theme::StyleRole::KeyHint),
        ));
        spans.push(Span::styled(
            value.action_suffix,
            app.theme.style(theme::StyleRole::TextMuted),
        ));
    }
    if let Some(secondary) = value.secondary {
        spans.push(Span::styled(
            format!(" · {secondary}"),
            app.theme.style(theme::StyleRole::TextMuted),
        ));
    }
    spans
}

/// One identity: the name that matters, and the qualifier that would be
/// mistaken for a second name if it were shown on its own.
struct IdentityRow {
    primary: String,
    action: Option<String>,
    action_suffix: String,
    secondary: Option<String>,
    present: bool,
}

impl IdentityRow {
    fn absent(reason: &str) -> Self {
        Self {
            primary: reason.to_owned(),
            action: None,
            action_suffix: String::new(),
            secondary: None,
            present: false,
        }
    }

    fn absent_with_action(before: &str, action: &str, after: &str) -> Self {
        Self {
            primary: before.to_owned(),
            action: Some(action.to_owned()),
            action_suffix: after.to_owned(),
            secondary: None,
            present: false,
        }
    }
}

/// The tailnet this machine's client is on, and the MagicDNS suffix its devices
/// answer to. The suffix used to sit unlabelled on its own row, where it read as
/// a third tailnet rather than as a property of this one.
fn local_identity(app: &App) -> IdentityRow {
    if app.source_mode == crate::app::SourceMode::Mock {
        return IdentityRow::absent("simulated");
    }
    if app.source_mode == crate::app::SourceMode::Unavailable {
        return IdentityRow::absent("local access off");
    }
    let Some(snapshot) = app.local_resource.snapshot.as_ref() else {
        return IdentityRow::absent("not connected");
    };
    let suffix = snapshot
        .magic_dns_suffix
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    match snapshot
        .current_tailnet
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        Some(tailnet) => IdentityRow {
            primary: tailnet.to_owned(),
            action: None,
            action_suffix: String::new(),
            // Some tailnets are named after their suffix. Printing it twice
            // reads as two facts when there is one.
            secondary: suffix.filter(|suffix| !suffix.eq_ignore_ascii_case(tailnet)),
            present: true,
        },
        // No tailnet name, but the suffix still names the tailnet, so it is
        // promoted rather than dropped alongside an "unknown".
        None => match suffix {
            Some(suffix) => IdentityRow {
                primary: suffix,
                action: None,
                action_suffix: String::new(),
                secondary: None,
                present: true,
            },
            None => IdentityRow::absent("tailnet not reported"),
        },
    }
}

/// The profile being administered, and the tailnet it reads. Named separately
/// from the local client because they are two different tailnets as often as
/// they are one, and the header has no business deciding which one to show.
fn profile_identity(app: &App) -> IdentityRow {
    let Some(profile) = app.admin.profile.as_deref() else {
        return IdentityRow::absent_with_action("none · ", ":profiles", " to choose one");
    };
    // `tailnet = "-"` is a request parameter meaning "this credential's own
    // tailnet", so it identifies nothing; what the API returned does.
    let configured = app
        .admin
        .tailnet
        .as_deref()
        .filter(|value| !value.is_empty() && *value != "-");
    let tailnet = configured
        .map(str::to_owned)
        .or_else(|| app.admin_tailnet_suffix().map(str::to_owned));
    IdentityRow {
        primary: profile.to_owned(),
        action: None,
        action_suffix: String::new(),
        secondary: Some(tailnet.unwrap_or_else(|| "tailnet not read yet".to_owned())),
        present: true,
    }
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

/// The same three facts on one line. One row has no columns to align, so each
/// identity carries the word that says what it is rather than a padded label —
/// two bare tailnet names in a row is exactly the ambiguity the tall header
/// stopped producing.
fn compact_line(app: &App) -> Line<'static> {
    let (label, role, _) = connection_state(app);
    let mut spans = vec![Span::styled(label, app.theme.style(role))];
    let push = |spans: &mut Vec<Span<'static>>, prefix: &str, value: IdentityRow| {
        spans.push(Span::styled(
            "   ",
            app.theme.style(theme::StyleRole::Surface),
        ));
        spans.push(Span::styled(
            format!("{prefix} "),
            app.theme.style(theme::StyleRole::TextMuted),
        ));
        spans.push(Span::styled(
            value.primary,
            app.theme.style(if value.present {
                theme::StyleRole::TextPrimary
            } else {
                theme::StyleRole::TextMuted
            }),
        ));
    };
    push(&mut spans, "local", local_identity(app));
    // The profile is named only when there is one; on a single line a permanent
    // "none" would cost more than it says.
    if app.admin.profile.is_some() {
        push(&mut spans, "profile", profile_identity(app));
    }
    for (note, role) in [staleness(app), profile_loading(app), task_state(app)]
        .into_iter()
        .flatten()
    {
        spans.push(Span::styled(
            "   ",
            app.theme.style(theme::StyleRole::Surface),
        ));
        spans.push(Span::styled(note, app.theme.style(role)));
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

/// The label, its meaning, and the key that acts on it when there is one.
fn connection_state(
    app: &App,
) -> (
    String,
    theme::StyleRole,
    Option<(&'static str, &'static str, &'static str)>,
) {
    use crate::domain::source::LocalDaemonState;
    match app.source_mode {
        crate::app::SourceMode::Mock => (
            "simulated data".to_owned(),
            theme::StyleRole::StateUnknown,
            None,
        ),
        crate::app::SourceMode::Unavailable => (
            "no local connection".to_owned(),
            theme::StyleRole::StateDanger,
            None,
        ),
        crate::app::SourceMode::Local => match &app.local_daemon_state {
            LocalDaemonState::Live => (
                "connected locally".to_owned(),
                theme::StyleRole::StateHealthy,
                None,
            ),
            LocalDaemonState::Connecting => (
                "connecting".to_owned(),
                theme::StyleRole::StatePending,
                None,
            ),
            LocalDaemonState::Reconnecting => (
                "reconnecting".to_owned(),
                theme::StyleRole::StateWarning,
                None,
            ),
            LocalDaemonState::Disabled => (
                "local access off".to_owned(),
                theme::StyleRole::TextMuted,
                None,
            ),
            LocalDaemonState::Mock => (
                "simulated data".to_owned(),
                theme::StyleRole::StateUnknown,
                None,
            ),
            LocalDaemonState::PermissionDenied { .. } => (
                "local access denied".to_owned(),
                theme::StyleRole::StateDanger,
                Some(("press ", "a d", " for diagnostics")),
            ),
            LocalDaemonState::Unsupported { .. } => (
                "local client unsupported".to_owned(),
                theme::StyleRole::StateDanger,
                None,
            ),
            LocalDaemonState::Unavailable { .. } => (
                "local daemon unreachable".to_owned(),
                theme::StyleRole::StateDanger,
                Some(("press ", "r", " to retry")),
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
    let failed = app.current_session_failed_task_count();
    if failed > 0 {
        let plural = if failed == 1 { "task" } else { "tasks" };
        return Some((
            format!("{failed} {plural} failed"),
            theme::StyleRole::TaskFailed,
        ));
    }
    None
}

fn profile_loading(app: &App) -> Option<(String, theme::StyleRole)> {
    (app.admin.profile.is_some() && app.admin.is_loading()).then(|| {
        (
            "loading profile data".to_owned(),
            theme::StyleRole::StatePending,
        )
    })
}
