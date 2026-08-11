use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};

use crate::app::{App, Focus, ProfileRow};
use crate::domain::profile::{CredentialPresence, ProbeState};
use crate::ui::components::{grid, panel};
use crate::ui::{text, theme};

/// When a column is worth its width. The list is short, so the tiers read the
/// terminal rather than waiting for a `w columns` key this route does not offer.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Tier {
    Always,
    /// Wide enough for where the secret is kept.
    Wide,
    /// Wide enough for the store reference as well.
    Widest,
}

/// Header, width, and when it appears. The order here is the order on screen.
const COLUMNS: &[(&str, Tier, grid::Width)] = &[
    ("S", Tier::Always, grid::Width::Fixed(2)),
    ("PROFILE", Tier::Always, grid::Width::Fill(14)),
    ("TAILNET", Tier::Always, grid::Width::Fill(16)),
    ("STATE", Tier::Always, grid::Width::Fill(12)),
    ("ACCESS", Tier::Always, grid::Width::Fill(10)),
    ("BACKEND", Tier::Wide, grid::Width::Fill(10)),
    ("CREDENTIAL", Tier::Widest, grid::Width::Fill(14)),
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
    let rows = app.profile_rows();
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
    let table_rows = visible_rows(app, &rows, area)
        .map(|(row, selected)| {
            let cells = COLUMNS
                .iter()
                .filter(|(_, tier, _)| shows(*tier))
                .map(|(header, _, _)| cell(app, row, header))
                .collect::<Vec<_>>();
            grid::Row::new(cells).selected(selected)
        })
        .collect::<Vec<_>>();
    let lines = grid::lines(app, &columns, &table_rows, area.width.saturating_sub(4));
    panel::render_view(frame, app, area, title(app, &rows), lines);
}

/// The row again, one fact per line, plus the one thing the table has no room
/// for: why a profile is in the state it reports.
fn render_inspector(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(row) = app.selected_profile_row() else {
        panel::render(frame, app, area, "inspector", "No profile selected");
        return;
    };
    let mut pairs = vec![
        (
            "selection",
            if row.active() {
                "active".to_owned()
            } else {
                "not active".to_owned()
            },
        ),
        ("state", row.state_label().to_owned()),
    ];
    match row {
        ProfileRow::Local {
            tailnet, account, ..
        } => {
            pairs.push(("kind", "local client".to_owned()));
            pairs.push(("tailnet", optional(tailnet)));
            pairs.push(("account", optional(account)));
            pairs.push(("reads", "the tailscaled socket on this machine".to_owned()));
        }
        ProfileRow::Admin { config, status, .. } => {
            pairs.push(("kind", "control API credential".to_owned()));
            pairs.push(("tailnet", config.tailnet.clone()));
            pairs.push(("access", row.access_label().to_owned()));
            pairs.push(("credential", config.credential.clone()));
            pairs.push((
                "backend",
                format!(
                    "{} ({})",
                    config.credential_backend.label(),
                    config.credential_backend.location().display()
                ),
            ));
            if let Some(status) = status {
                pairs.push((
                    "stored",
                    match status.presence.as_ref() {
                        None => "not read yet".to_owned(),
                        Some(presence) => presence.label().to_owned(),
                    },
                ));
                if let Some(CredentialPresence::Stored { scopes, .. }) = status.presence.as_ref()
                    && !scopes.is_empty()
                {
                    pairs.push(("scopes", scopes.join(" ")));
                }
                pairs.push(("verified", probe_summary(app, &status.probe)));
            }
            // What this session actually asked for and got back, which only the
            // profile the app is reading from can answer.
            if row.active() {
                pairs.push((
                    "requested",
                    if app.admin.requested_scopes.is_empty() {
                        "not recorded".to_owned()
                    } else {
                        app.admin.requested_scopes.join(" ")
                    },
                ));
                if !app.admin.capabilities.is_empty() {
                    pairs.push((
                        "allowed",
                        app.admin
                            .capabilities
                            .iter()
                            .map(|(name, state)| format!("{name}={}", state.label()))
                            .collect::<Vec<_>>()
                            .join(", "),
                    ));
                }
                append_managed_tailnet(&mut pairs, app);
            }
        }
    }
    let mut lines = vec![Line::from(Span::styled(
        text::ellipsize(row.label(), usize::from(area.width.saturating_sub(4))),
        app.theme.style(theme::StyleRole::TextPrimary),
    ))];
    lines.extend(grid::detail(app, &pairs));
    if let Some(detail) = row.detail() {
        lines.push(Line::from(Span::styled(
            detail.to_owned(),
            app.theme.style(theme::StyleRole::StateDanger),
        )));
    }
    panel::render_focusable(
        frame,
        app,
        area,
        "inspector",
        lines,
        app.focus == Focus::Inspector,
    );
}

/// How the tailnet this credential manages is configured, as its control plane
/// reports it. It hangs off the profile because it is only ever knowable
/// through one: change the active profile and this is a different tailnet.
fn append_managed_tailnet(pairs: &mut Vec<(&'static str, String)>, app: &App) {
    if let Some(settings) = app.admin.settings.snapshot.as_ref() {
        pairs.extend([
            ("device approval", required(settings.devices_approval_on)),
            ("user approval", required(settings.users_approval_on)),
            ("ACLs managed", acls(settings.acls_externally_managed_on)),
            ("auto-updates", flag(settings.devices_auto_updates_on)),
            (
                "key lifetime",
                settings
                    .devices_key_duration_days
                    .map_or_else(|| "not returned".to_owned(), |days| format!("{days} days")),
            ),
            ("flow logging", flag(settings.network_flow_logging_on)),
            ("regional routing", flag(settings.regional_routing_on)),
            (
                "posture identity",
                flag(settings.posture_identity_collection_on),
            ),
            ("HTTPS certs", flag(settings.https_enabled)),
        ]);
    }
    let Some(contacts) = app.admin.contacts.snapshot.as_ref() else {
        return;
    };
    pairs.extend([
        ("account contact", contact_email(contacts.account.as_ref())),
        ("support contact", contact_email(contacts.support.as_ref())),
        (
            "security contact",
            contact_email(contacts.security.as_ref()),
        ),
    ]);
}

/// Whether a tailnet-wide switch is on. Only the two approval gates read as a
/// requirement; the rest are simply on or off, and saying otherwise would
/// describe a setting that does not exist.
fn flag(value: Option<bool>) -> String {
    value.map_or_else(
        || "not returned".to_owned(),
        |value| if value { "on" } else { "off" }.to_owned(),
    )
}

fn required(value: Option<bool>) -> String {
    value.map_or_else(
        || "not returned".to_owned(),
        |value| if value { "required" } else { "not required" }.to_owned(),
    )
}

fn acls(value: Option<bool>) -> String {
    value.map_or_else(
        || "not returned".to_owned(),
        |value| if value { "elsewhere" } else { "here" }.to_owned(),
    )
}

/// An address the control plane returned empty is as absent as one it omitted,
/// so both read the same rather than leaving a blank where a value belongs.
fn contact_email(contact: Option<&crate::admin::AdminContact>) -> String {
    let Some(contact) = contact else {
        return "not returned".to_owned();
    };
    let email = contact
        .email
        .as_deref()
        .or(contact.fallback_email.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match email {
        None => "not returned".to_owned(),
        Some(email) if contact.needs_verification == Some(true) => {
            format!("{email} · needs verification")
        }
        Some(email) => email.to_owned(),
    }
}

/// A profile is only ever verified because someone tried to activate it, so the
/// summary says when that was rather than implying a background check.
fn probe_summary(app: &App, probe: &ProbeState) -> String {
    match probe {
        ProbeState::NotProbed => "not attempted".to_owned(),
        ProbeState::InFlight => "checking now".to_owned(),
        ProbeState::Reachable { kind, at } => format!(
            "{} reached the control plane {} ago",
            kind.label(),
            text::format_age(app.now.saturating_sub(*at))
        ),
        ProbeState::Rejected { at, .. } => format!(
            "rejected {} ago",
            text::format_age(app.now.saturating_sub(*at))
        ),
    }
}

/// The window that fits, kept over the selection. The list carries no scroll
/// offset of its own, so the cursor position decides which slice is on screen.
fn visible_rows<'a>(
    app: &App,
    rows: &'a [ProfileRow<'a>],
    area: Rect,
) -> impl Iterator<Item = (&'a ProfileRow<'a>, bool)> {
    let viewport = usize::from(area.height.saturating_sub(3)).max(1);
    let selected = app.views.profiles.selected;
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

/// The one cell that means something the row does not: whether this is the
/// source the rest of Tale is reading from.
fn selection_cell(app: &App, row: &ProfileRow<'_>) -> grid::Cell {
    let unicode = app.resolved_config.ui.symbols.unicode();
    if row.active() {
        return grid::Cell::new(if unicode { "●" } else { "*" })
            .with_role(theme::StyleRole::StateHealthy);
    }
    let (marker, role) = match row {
        ProfileRow::Local { .. } => ("○", theme::StyleRole::StateUnknown),
        ProfileRow::Admin { status, .. } => match status {
            None => ("?", theme::StyleRole::StateUnknown),
            Some(status) => match (&status.presence, &status.probe) {
                (_, ProbeState::InFlight) => ("~", theme::StyleRole::StateUnknown),
                (Some(CredentialPresence::Stored { .. }), ProbeState::Rejected { .. }) => {
                    ("!", theme::StyleRole::StateDanger)
                }
                (Some(CredentialPresence::Stored { .. }), _) => {
                    ("○", theme::StyleRole::StateUnknown)
                }
                (Some(_), _) => ("!", theme::StyleRole::StateDanger),
                (None, _) => ("?", theme::StyleRole::StateUnknown),
            },
        },
    };
    let marker = if unicode || marker != "○" {
        marker
    } else {
        "o"
    };
    grid::Cell::new(marker).with_role(role)
}

fn cell(app: &App, row: &ProfileRow<'_>, header: &str) -> grid::Cell {
    match header {
        "S" => selection_cell(app, row),
        "PROFILE" => grid::Cell::new(row.label()),
        "TAILNET" => grid::Cell::new(optional(row.tailnet())),
        "STATE" => grid::Cell::new(row.state_label()),
        "ACCESS" => grid::Cell::new(row.access_label()),
        "BACKEND" => grid::Cell::new(match row {
            ProfileRow::Local { .. } => "socket",
            ProfileRow::Admin { config, .. } => config.credential_backend.label(),
        }),
        "CREDENTIAL" => grid::Cell::new(match row {
            ProfileRow::Local { .. } => "-",
            ProfileRow::Admin { config, .. } => config.credential.as_str(),
        }),
        _ => grid::Cell::new("-"),
    }
}

fn optional(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .map_or_else(|| "-".to_owned(), str::to_owned)
}

/// Route context lives in the border, the way it does on every other route.
fn title(app: &App, rows: &[ProfileRow<'_>]) -> ratatui::text::Line<'static> {
    let mut detail = Vec::new();
    if !app.views.profiles.filter.is_empty() {
        detail.push(format!(
            "/{}",
            text::ellipsize(&app.views.profiles.filter, 32)
        ));
    }
    detail.push(format!(
        "{} {}",
        app.views.profiles.sort.field.label(),
        if app.views.profiles.sort.direction.is_ascending() {
            "\u{2191}"
        } else {
            "\u{2193}"
        }
    ));
    // The active source is a fact about the session, not about the rows, so it
    // is named even when the filter has hidden the row it names.
    detail.push(format!(
        "active: {}",
        app.all_profile_rows()
            .iter()
            .find(|row| row.active())
            .map_or("none", ProfileRow::label)
    ));
    text::view_title(
        app.theme,
        "profiles",
        rows.len(),
        app.all_profile_rows().len(),
        &detail,
    )
}
