use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::domain::account::LocalSection;
use crate::domain::preference::ObservedPreference;
use crate::ui::components::{grid, panel, tabs};
use crate::ui::{text, theme};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    match app.views.local.section {
        LocalSection::Client => render_client(frame, app, area),
        LocalSection::Accounts => render_accounts(frame, app, area),
    }
}

fn render_client(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(snapshot) = app.local_resource.snapshot.as_ref() else {
        render_without_snapshot(frame, app, area);
        return;
    };
    let mut lines = vec![tab_line(app), Line::default()];
    section(app, &mut lines, "Client");
    let mut client = vec![
        ("daemon", local_daemon_label(app).to_owned()),
        ("command", local_command_label(app).to_owned()),
        ("version", snapshot.client_version.clone()),
    ];
    if let Some(version) = snapshot.daemon_version.as_deref() {
        client.push(("daemon version", version.to_owned()));
    }
    if let Some(executable) = app.local_executable.as_ref() {
        client.push(("executable", executable.path.display().to_string()));
    }
    client.push(("observed", text::format_timestamp(snapshot.observed_at)));
    lines.extend(grid::detail(app, &client));

    section(app, &mut lines, "Identity");
    let node = &snapshot.self_node;
    let mut identity = vec![("node", node.display_name.clone())];
    push_optional(&mut identity, "DNS name", node.dns_name.as_deref());
    if !node.tailscale_ips.is_empty() {
        identity.push(("addresses", node.tailscale_ips.join(" · ")));
    }
    push_optional(
        &mut identity,
        "tailnet",
        snapshot.current_tailnet.as_deref(),
    );
    push_optional(
        &mut identity,
        "MagicDNS suffix",
        snapshot.magic_dns_suffix.as_deref(),
    );
    if !snapshot.health_messages.is_empty() {
        identity.push(("attention", snapshot.health_messages.join(" · ")));
    }
    lines.extend(grid::detail(app, &identity));

    section(app, &mut lines, "Preferences");
    let mut preferences = Vec::new();
    push_toggle(
        &mut preferences,
        "accept DNS",
        &app.local_preferences.accept_dns,
    );
    push_toggle(
        &mut preferences,
        "accept routes",
        &app.local_preferences.accept_routes,
    );
    push_toggle(
        &mut preferences,
        "shields up",
        &app.local_preferences.shields_up,
    );
    push_toggle(
        &mut preferences,
        "Tailscale SSH",
        &app.local_preferences.ssh,
    );
    push_toggle(
        &mut preferences,
        "automatic update",
        &app.local_preferences.automatic_update,
    );
    push_toggle(
        &mut preferences,
        "posture reporting",
        &app.local_preferences.report_posture,
    );
    push_preference(
        &mut preferences,
        "hostname",
        &app.local_preferences.hostname,
    );
    push_preference(
        &mut preferences,
        "nickname",
        &app.local_preferences.nickname,
    );
    lines.extend(grid::detail(app, &preferences));

    if !app.system_policy.is_empty() || app.system_policy_failure.is_some() {
        section(app, &mut lines, "System policy");
        if let Some(failure) = app.system_policy_failure.as_ref() {
            lines.push(Line::from(Span::styled(
                failure.detail.clone(),
                app.theme.style(theme::StyleRole::StateDanger),
            )));
        } else {
            let policy = app
                .system_policy
                .iter()
                .map(|entry| {
                    let value = entry
                        .value
                        .as_deref()
                        .filter(|value| !value.is_empty())
                        .map_or("configured", |value| value);
                    (entry.name.as_str(), value.to_owned())
                })
                .collect::<Vec<_>>();
            lines.extend(grid::detail(app, &policy));
        }
    }
    let mut title_detail = vec![match app.source_mode {
        crate::app::SourceMode::Mock => "simulated".to_owned(),
        _ => freshness(app),
    }];
    if !app.local_accounts.is_empty() {
        title_detail.push(format!("{} accounts", app.local_accounts.len()));
    }
    panel::render(
        frame,
        app,
        area,
        &format!("local · read-only state · {}", title_detail.join(" · ")),
        lines,
    );
}

fn render_accounts(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = vec![tab_line(app), Line::default()];
    if app.resolved_config.read_only {
        lines.push(Line::from(Span::styled(
            "Read-only: accounts cannot be changed",
            app.theme.style(theme::StyleRole::StateDisabled),
        )));
    }
    if let Some(failure) = app.local_accounts_failure.as_ref() {
        lines.push(Line::from(Span::styled(
            format!("{} · {}", failure.summary, failure.detail),
            app.theme.style(theme::StyleRole::StateDanger),
        )));
    }
    let columns = vec![
        grid::Column::fixed("STATUS", 8),
        grid::Column::fill("PROFILE", 1),
        grid::Column::fill("ACCOUNT", 2),
        grid::Column::fill("TAILNET", 2),
    ];
    let mut rows = app
        .local_accounts
        .iter()
        .map(|account| {
            let status = grid::Cell::new(if account.active { "active" } else { "" });
            let status = if account.active {
                status.with_role(theme::StyleRole::StateHealthy)
            } else {
                status
            };
            grid::Row::new(vec![
                status,
                grid::Cell::new(account.display_label()),
                grid::Cell::new(account.account_name.as_deref().unwrap_or("not returned")),
                grid::Cell::new(account.tailnet_name.as_deref().unwrap_or("not returned")),
            ])
        })
        .collect::<Vec<_>>();
    if let Some(row) = rows.get_mut(app.views.local.selected) {
        row.selected = true;
    }
    if rows.is_empty() {
        lines.extend(accounts_empty_message(app));
    } else {
        lines.extend(grid::lines(
            app,
            &columns,
            rows,
            area.width.saturating_sub(4),
        ));
    }
    let active = app
        .local_accounts
        .iter()
        .filter(|account| account.active)
        .count();
    let detail = (active > 0)
        .then(|| format!("{active} active"))
        .into_iter()
        .collect::<Vec<_>>();
    panel::render_view(
        frame,
        app,
        area,
        text::view_title(
            app.theme,
            "accounts",
            app.local_accounts.len(),
            app.local_accounts.len(),
            &detail,
        ),
        lines,
    );
}

fn tab_line(app: &App) -> Line<'static> {
    let current = app.views.local.section;
    tabs::line(
        app,
        LocalSection::ALL.map(|section| (section.label(), section == current)),
    )
}

fn accounts_empty_message(app: &App) -> Vec<Line<'static>> {
    if app.local_accounts_failure.is_some() {
        return vec![
            text::muted_help(app.theme, "Account profiles could not be loaded"),
            Line::default(),
            text::action_hint(app.theme, "  retry                   ", "r"),
        ];
    }
    let message = if app.local_capabilities.accounts {
        "This machine has no saved account profiles"
    } else {
        "This Tailscale client does not report account profiles"
    };
    let mut lines = vec![text::muted_help(app.theme, message)];
    if app.local_capabilities.account_login && !app.resolved_config.read_only {
        lines.push(Line::default());
        lines.push(text::action_hint(
            app.theme,
            "  add an account          ",
            "a al",
        ));
    }
    lines
}

fn section(app: &App, lines: &mut Vec<Line<'static>>, title: &str) {
    if !lines.is_empty() {
        lines.push(Line::default());
    }
    lines.push(Line::from(Span::styled(
        title.to_owned(),
        app.theme.style(theme::StyleRole::SectionHeading),
    )));
}

fn freshness(app: &App) -> String {
    let freshness = match app.local_resource.status {
        crate::domain::source::LocalResourceStatus::Fresh => text::Freshness::Current,
        crate::domain::source::LocalResourceStatus::Loading => text::Freshness::Loading,
        crate::domain::source::LocalResourceStatus::Stale => text::Freshness::Stale,
        crate::domain::source::LocalResourceStatus::NeverLoaded
        | crate::domain::source::LocalResourceStatus::Failed => text::Freshness::Unavailable,
    };
    freshness.phrase(
        app.local_resource
            .last_success_at
            .map(|observed| app.now.saturating_sub(observed)),
    )
}

fn local_daemon_label(app: &App) -> &'static str {
    if matches!(
        app.local_daemon_state,
        crate::domain::source::LocalDaemonState::Mock
    ) {
        "simulated"
    } else {
        app.local_daemon_state.label()
    }
}

fn local_command_label(app: &App) -> &'static str {
    if matches!(
        app.local_cli_state,
        crate::domain::source::LocalCliState::Mock
    ) {
        "simulated"
    } else {
        app.local_cli_state.label()
    }
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

fn push_preference<T: std::fmt::Display>(
    pairs: &mut Vec<(&'static str, String)>,
    label: &'static str,
    preference: &ObservedPreference<T>,
) {
    let Some(value) = preference.value.as_ref() else {
        return;
    };
    let mut display = value.to_string();
    if !preference.editability.can_edit() {
        display.push_str(" · ");
        display.push_str(preference.editability.label());
    }
    pairs.push((label, display));
}

fn push_toggle(
    pairs: &mut Vec<(&'static str, String)>,
    label: &'static str,
    preference: &ObservedPreference<bool>,
) {
    let Some(value) = preference.value else {
        return;
    };
    let mut display = if value { "on" } else { "off" }.to_owned();
    if !preference.editability.can_edit() {
        display.push_str(" · ");
        display.push_str(preference.editability.label());
    }
    pairs.push((label, display));
}

/// With no snapshot at all, every field would claim the daemon answered and
/// omitted it. The empty state says what Tale actually knows and what to do.
fn render_without_snapshot(frame: &mut Frame<'_>, app: &App, area: Rect) {
    use crate::domain::source::LocalDaemonState;
    let mut lines = vec![
        tab_line(app),
        Line::default(),
        Line::from("No local node details to show"),
        Line::default(),
    ];
    match &app.local_daemon_state {
        LocalDaemonState::Mock => lines.push(Line::from(
            "The simulated local snapshot is unavailable. Restart mock mode to reload it.",
        )),
        LocalDaemonState::Disabled => lines.push(Line::from(
            "Local access is off for this run. Restart without --no-local.",
        )),
        LocalDaemonState::Connecting | LocalDaemonState::Reconnecting => {
            lines.push(Line::from("Connecting to the local Tailscale daemon…"));
        }
        LocalDaemonState::Live => {
            lines.push(Line::from(
                "The daemon is connected but has not answered yet.",
            ));
            lines.push(Line::default());
            lines.push(text::action_hint(app.theme, "  retry   ", "r"));
        }
        LocalDaemonState::PermissionDenied { detail }
        | LocalDaemonState::Unsupported { detail }
        | LocalDaemonState::Unavailable { detail } => {
            lines.push(Line::from(detail.clone()));
            lines.push(Line::default());
            lines.push(text::action_hint(app.theme, "  retry   ", "r"));
        }
    }
    panel::render(frame, app, area, "local", lines);
}
