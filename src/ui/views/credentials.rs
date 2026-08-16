use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};

use crate::app::{App, Focus};
use crate::domain::credential::CredentialMetadata;
use crate::ui::components::{grid, panel};
use crate::ui::{text, theme};

const COLUMNS: &[(&str, grid::Width)] = &[
    ("S", grid::Width::Fixed(2)),
    ("TYPE", grid::Width::Fill(10)),
    ("DESCRIPTION", grid::Width::Fill(18)),
    ("OWNER", grid::Width::Fill(14)),
    ("SCOPES", grid::Width::Fixed(6)),
    ("EXPIRES", grid::Width::Fill(8)),
];

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, wide_inspector: Option<Rect>) {
    if app.focus == Focus::Inspector || wide_inspector.is_none() {
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
    let resource = &app.admin.credentials;
    let credentials = resource
        .snapshot
        .as_ref()
        .map_or(&[][..], |snapshot| snapshot.records.as_slice());
    let filtered = app.filtered_admin_credentials();
    let lines = if credentials.is_empty() {
        text::empty_state(
            app.theme,
            "credentials",
            "credentials",
            app.admin.profile.is_some(),
            resource.state,
            resource.error.as_deref(),
        )
    } else {
        let columns = COLUMNS
            .iter()
            .map(|(header, width)| grid::Column {
                header: (*header).to_owned(),
                width: *width,
            })
            .collect::<Vec<_>>();
        let viewport = usize::from(area.height.saturating_sub(3)).max(1);
        let start = app
            .admin_credential_selected
            .saturating_add(1)
            .saturating_sub(viewport)
            .min(filtered.len().saturating_sub(1));
        let rows = filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(viewport)
            .map(|(index, credential)| {
                grid::Row::new([
                    state_cell(app, credential),
                    grid::Cell::new(credential.key_type.clone()),
                    grid::Cell::new(optional(credential.description.as_deref())),
                    grid::Cell::new(optional(credential.user_id.as_deref())),
                    grid::Cell::new(credential.scopes.len().to_string()),
                    grid::Cell::new(expiry(app, credential)),
                ])
                .selected(index == app.admin_credential_selected)
            })
            .collect::<Vec<_>>();
        grid::lines(app, &columns, rows, area.width.saturating_sub(4))
    };
    let mut detail = vec!["metadata only".to_owned()];
    if !app.views.credentials.filter.is_empty() {
        detail.insert(
            0,
            format!("/{}", text::ellipsize(&app.views.credentials.filter, 32)),
        );
    }
    if resource
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.partial)
    {
        detail.push("partial inventory".to_owned());
    }
    panel::render_view(
        frame,
        app,
        area,
        text::view_title(
            app.theme,
            "credentials",
            filtered.len(),
            credentials.len(),
            &detail,
        ),
        lines,
    );
}

fn render_inspector(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let credential = app.selected_admin_credential_for_view();
    let Some(credential) = credential else {
        panel::render(frame, app, area, "inspector", "No credential selected");
        return;
    };
    let mut pairs = vec![
        ("id", credential.id.clone()),
        ("type", credential.key_type.clone()),
        ("state", state_label(app, credential).to_owned()),
    ];
    push_optional(&mut pairs, "description", credential.description.as_deref());
    push_optional(&mut pairs, "owner", credential.user_id.as_deref());
    if !credential.scopes.is_empty() {
        pairs.push(("scopes", credential.scopes.join(" · ")));
    }
    if !credential.tags.is_empty() {
        pairs.push(("tags", text::tag_list(&credential.tags)));
    }
    push_time(&mut pairs, "created", credential.created_at);
    push_time(&mut pairs, "updated", credential.updated_at);
    push_time(&mut pairs, "expires", credential.expires_at);
    push_time(&mut pairs, "last used", credential.last_used_at);
    push_time(&mut pairs, "revoked", credential.revoked_at);
    if !credential.capability_summary.is_empty() {
        pairs.push(("capabilities", credential.capability_summary.join(" · ")));
    }
    if !credential.known_dependents.is_empty() {
        pairs.push(("used by", credential.known_dependents.join(" · ")));
    }
    let mut lines = vec![Line::from(Span::styled(
        credential
            .description
            .as_deref()
            .filter(|value| !value.is_empty())
            .map_or(credential.key_type.as_str(), |value| value)
            .to_owned(),
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

fn state_cell(app: &App, credential: &CredentialMetadata) -> grid::Cell {
    let unicode = app.resolved_config.ui.symbols.unicode();
    let (symbol, role) = if credential.revoked_at.is_some() || credential.invalid == Some(true) {
        (
            if unicode { "◆" } else { "X" },
            theme::StyleRole::StateDanger,
        )
    } else if credential
        .expires_at
        .is_some_and(|expiry| expiry <= app.now)
    {
        (
            if unicode { "▲" } else { "!" },
            theme::StyleRole::StateWarning,
        )
    } else {
        (
            if unicode { "✓" } else { "+" },
            theme::StyleRole::StateHealthy,
        )
    };
    grid::Cell::new(symbol).with_role(role)
}

fn state_label(app: &App, credential: &CredentialMetadata) -> &'static str {
    if credential.revoked_at.is_some() {
        "revoked"
    } else if credential.invalid == Some(true) {
        "invalid"
    } else if credential
        .expires_at
        .is_some_and(|expiry| expiry <= app.now)
    {
        "expired"
    } else {
        "active"
    }
}

fn expiry(app: &App, credential: &CredentialMetadata) -> String {
    if credential.revoked_at.is_some() {
        return "revoked".to_owned();
    }
    credential.expires_at.map_or_else(
        || "never".to_owned(),
        |expiry| {
            if expiry <= app.now {
                "expired".to_owned()
            } else {
                format!("in {}", text::format_age(expiry.saturating_sub(app.now)))
            }
        },
    )
}

fn optional(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .map_or_else(|| "-".to_owned(), str::to_owned)
}

fn push_optional(
    pairs: &mut Vec<(&'static str, String)>,
    label: &'static str,
    value: Option<&str>,
) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        pairs.push((label, value.to_owned()));
    }
}

fn push_time(
    pairs: &mut Vec<(&'static str, String)>,
    label: &'static str,
    value: Option<crate::domain::Timestamp>,
) {
    if let Some(value) = value {
        pairs.push((label, text::format_timestamp(value)));
    }
}
