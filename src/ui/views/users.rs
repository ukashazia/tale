use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};

use crate::app::{App, Focus};
use crate::domain::user::AdminUser;
use crate::ui::components::{grid, panel};
use crate::ui::{text, theme};

/// When a column is worth its width. Users have no `w columns` key — the key is
/// offered on `:devices` only — so the tiers read the terminal instead.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Tier {
    Always,
    /// Wide enough for the login and the account's age.
    Wide,
    /// Wide enough for the opaque identifier as well.
    Widest,
}

/// Header, width, and when it appears. The order here is the order on screen.
const COLUMNS: &[(&str, Tier, grid::Width)] = &[
    ("S", Tier::Always, grid::Width::Fixed(2)),
    ("NAME", Tier::Always, grid::Width::Fill(14)),
    ("LOGIN", Tier::Wide, grid::Width::Fill(16)),
    ("ROLE", Tier::Always, grid::Width::Fill(10)),
    ("STATUS", Tier::Always, grid::Width::Fill(10)),
    ("DEVICES", Tier::Always, grid::Width::Fixed(7)),
    ("SEEN", Tier::Always, grid::Width::Fill(6)),
    ("CREATED", Tier::Wide, grid::Width::Fill(7)),
    ("ID", Tier::Widest, grid::Width::Fill(18)),
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
    let resource = &app.admin.users;
    let users = resource.snapshot.as_deref().unwrap_or_default();
    let filtered = app.filtered_admin_users();
    let lines = if users.is_empty() {
        text::empty_state(
            app.theme,
            "users",
            "users",
            app.admin.profile.is_some(),
            resource.state,
            resource.error.as_deref(),
        )
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
        let rows = visible_users(app, &filtered, area)
            .map(|(user, selected)| {
                let cells = COLUMNS
                    .iter()
                    .filter(|(_, tier, _)| shows(*tier))
                    .map(|(header, _, _)| cell(app, user, header))
                    .collect::<Vec<_>>();
                grid::Row::new(cells).selected(selected)
            })
            .collect::<Vec<_>>();
        grid::lines(app, &columns, &rows, area.width.saturating_sub(4))
    };
    panel::render(frame, app, area, &title(app, &filtered, users.len()), lines);
}

/// The row again, one fact per line. Only what the API reported: a row of
/// `not returned` describes the client, not the person.
fn render_inspector(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(user) = app.selected_admin_user() else {
        panel::render(frame, app, area, "inspector", "No user selected");
        return;
    };
    let mut pairs = Vec::new();
    if let Some(name) = user.display_name.as_deref() {
        pairs.push(("name", name.to_owned()));
    }
    if let Some(login) = user.login_name.as_deref() {
        pairs.push(("login", login.to_owned()));
    }
    pairs.push(("id", user.id.clone()));
    if let Some(role) = user.role.as_deref() {
        pairs.push(("role", role.to_owned()));
    }
    if let Some(status) = user.status.as_deref() {
        pairs.push(("status", status.to_owned()));
    }
    if let Some(relation) = user.relation_type.as_deref() {
        pairs.push(("relation", relation.to_owned()));
    }
    if let Some(connected) = user.currently_connected {
        pairs.push((
            "connection",
            if connected {
                "connected"
            } else {
                "not connected"
            }
            .to_owned(),
        ));
    }
    if let Some(count) = user.device_count {
        pairs.push(("devices", count.to_string()));
    }
    if let Some(seen) = user.last_seen {
        pairs.push(("last seen", ago(app, seen)));
    }
    if let Some(created) = user.created_at {
        pairs.push(("created", ago(app, created)));
    }
    let mut lines = vec![Line::from(Span::styled(
        text::ellipsize(user.label(), usize::from(area.width.saturating_sub(4))),
        app.theme.style(theme::StyleRole::TextPrimary),
    ))];
    lines.extend(grid::detail(app, &pairs));
    panel::render_focusable(
        frame,
        app,
        area,
        "inspector",
        lines,
        app.focus == Focus::Inspector,
    );
}

fn ago(app: &App, moment: crate::domain::Timestamp) -> String {
    format!("{} ago", text::format_age(app.now.saturating_sub(moment)))
}

/// The window that fits, kept over the selection. Users carry no scroll offset
/// of their own, so the cursor position decides which slice is on screen.
fn visible_users<'a>(
    app: &App,
    users: &'a [&'a AdminUser],
    area: Rect,
) -> impl Iterator<Item = (&'a AdminUser, bool)> {
    let viewport = usize::from(area.height.saturating_sub(3)).max(1);
    let selected = app.admin_user_selected;
    let start = selected
        .saturating_add(1)
        .saturating_sub(viewport)
        .min(users.len().saturating_sub(1));
    users
        .iter()
        .enumerate()
        .skip(start)
        .take(viewport)
        .map(move |(index, user)| (*user, index == selected))
}

/// The one cell that means something the row does not: whether the user is on
/// the tailnet right now.
fn connection_cell(app: &App, user: &AdminUser) -> grid::Cell {
    let unicode = app.resolved_config.ui.symbols.unicode();
    let (marker, role) = match user.currently_connected {
        Some(true) => (
            if unicode { "●" } else { "*" },
            theme::StyleRole::StateHealthy,
        ),
        Some(false) => (
            if unicode { "○" } else { "o" },
            theme::StyleRole::StateOffline,
        ),
        None => ("?", theme::StyleRole::StateUnknown),
    };
    grid::Cell::new(marker).with_role(role)
}

fn cell(app: &App, user: &AdminUser, header: &str) -> grid::Cell {
    match header {
        "S" => connection_cell(app, user),
        "NAME" => grid::Cell::new(user.label()),
        "LOGIN" => grid::Cell::new(optional(user.login_name.as_deref())),
        "ROLE" => grid::Cell::new(optional(user.role.as_deref())),
        "STATUS" => grid::Cell::new(optional(user.status.as_deref())),
        "DEVICES" => grid::Cell::new(
            user.device_count
                .map_or_else(|| "-".to_owned(), |count| count.to_string()),
        ),
        "SEEN" => grid::Cell::new(age(app, user.last_seen)),
        "CREATED" => grid::Cell::new(age(app, user.created_at)),
        "ID" => grid::Cell::new(user.id.clone()),
        _ => grid::Cell::new("-"),
    }
}

fn optional(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .map_or_else(|| "-".to_owned(), str::to_owned)
}

fn age(app: &App, moment: Option<crate::domain::Timestamp>) -> String {
    moment.map_or_else(
        || "-".to_owned(),
        |moment| text::format_age(app.now.saturating_sub(moment)),
    )
}

/// Route context lives in the border, the way it does on every other route.
fn title(app: &App, users: &[&AdminUser], total: usize) -> String {
    let mut detail = Vec::new();
    let connected = users
        .iter()
        .filter(|user| user.currently_connected == Some(true))
        .count();
    if connected > 0 {
        detail.push(format!("{connected} connected"));
    }
    if !app.views.users.filter.is_empty() {
        detail.insert(
            0,
            format!("/{}", text::ellipsize(&app.views.users.filter, 32)),
        );
    }
    text::view_title("users", users.len(), total, &detail)
}
