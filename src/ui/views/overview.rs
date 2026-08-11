use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};

use crate::admin::AdminResourceState;
use crate::app::{App, Focus, SourceMode};
use crate::domain::device::{ConnectionPath, Liveness};
use crate::domain::health::{Finding, Severity};
use crate::domain::source::LocalResourceStatus;
use crate::task::TaskState;
use crate::ui::components::{grid, panel};
use crate::ui::{text, theme};

/// Overview is an operational inbox. The source band answers whether its facts
/// can be trusted; the collection answers what needs attention; the inspector
/// carries the evidence that used to make the page an unreadable text dump.
pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.focus == Focus::Inspector {
        render_finding(frame, app, area);
        return;
    }

    let source_height = if area.width >= 110 { 5 } else { 6 };
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(source_height.min(area.height.saturating_sub(3))),
            Constraint::Min(3),
        ])
        .split(area);
    let Some(source_area) = vertical.first().copied() else {
        return;
    };
    let Some(attention_area) = vertical.get(1).copied() else {
        return;
    };
    render_sources(frame, app, source_area);

    if area.width >= 110 {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(attention_area);
        if let Some(collection) = horizontal.first().copied() {
            render_attention(frame, app, collection);
        }
        if let Some(inspector) = horizontal.get(1).copied() {
            render_finding(frame, app, inspector);
        }
    } else {
        render_attention(frame, app, attention_area);
    }
}

fn render_sources(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.width < 110 {
        let mut lines = local_summary(app, true);
        lines.extend(admin_summary(app, true));
        panel::render(frame, app, area, "sources", lines);
        return;
    }
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    if let Some(local) = horizontal.first().copied() {
        panel::render(frame, app, local, "local", local_summary(app, false));
    }
    if let Some(admin) = horizontal.get(1).copied() {
        panel::render(frame, app, admin, "admin", admin_summary(app, false));
    }
}

fn local_summary(app: &App, compact: bool) -> Vec<Line<'static>> {
    let status = app.local_resource.status;
    let status_label = if app.source_mode == SourceMode::Mock {
        "simulated"
    } else {
        status.label()
    };
    let role = local_status_role(status, app.source_mode);
    let state = semantic_line(
        app,
        role,
        format!(
            "{} · daemon {} · CLI {} · {}",
            if compact { "Local" } else { "Client" },
            app.local_daemon_state.label(),
            app.local_cli_state.label(),
            status_label
        ),
    );
    let running = app.tasks.active().count();
    let failed = app
        .tasks
        .all()
        .iter()
        .filter(|task| task.state == TaskState::Failed)
        .count();
    let task_detail = match (running, failed) {
        (0, 0) => "no active or failed tasks".to_owned(),
        _ => format!("{running} active tasks · {failed} failed"),
    };
    let Some(snapshot) = app.local_resource.snapshot.as_ref() else {
        let simulated = (app.source_mode == SourceMode::Mock).then(|| {
            let online = app
                .devices_resource
                .snapshot
                .iter()
                .filter(|device| device.liveness == Liveness::Online)
                .count();
            format!(
                "{} simulated devices · {online} online · {task_detail}",
                app.devices_resource.snapshot.len()
            )
        });
        return vec![
            state,
            Line::from(Span::styled(
                simulated.unwrap_or_else(|| format!("{} · {task_detail}", status.label())),
                app.theme.style(theme::StyleRole::TextMuted),
            )),
        ];
    };
    let direct = snapshot
        .peers
        .iter()
        .filter(|device| matches!(device.path, ConnectionPath::Direct { .. }))
        .count();
    let relayed = snapshot
        .peers
        .iter()
        .filter(|device| {
            matches!(
                device.path,
                ConnectionPath::Derp { .. } | ConnectionPath::PeerRelay { .. }
            )
        })
        .count();
    let node = if compact {
        format!(
            "{} · {} peers · {direct} direct · {relayed} relayed",
            snapshot.self_node.display_name,
            snapshot.peers.len()
        )
    } else {
        format!(
            "{} · {} peers · {direct} direct · {relayed} relayed · {task_detail}",
            snapshot.self_node.display_name,
            snapshot.peers.len()
        )
    };
    vec![
        state,
        Line::from(Span::styled(
            node,
            app.theme.style(theme::StyleRole::TextMuted),
        )),
    ]
}

fn admin_summary(app: &App, compact: bool) -> Vec<Line<'static>> {
    let Some(profile) = app.admin.profile.as_deref() else {
        return vec![
            semantic_line(
                app,
                theme::StyleRole::StateDisabled,
                format!(
                    "{} · not configured",
                    if compact { "Admin" } else { "Profile" }
                ),
            ),
            text::inline_action(app.theme, "Open ", ":profiles", " to activate a credential"),
        ];
    };
    let role = admin_status_role(app.admin.devices.state);
    let access = if app.admin.profile_read_only {
        "read-only"
    } else {
        "read-write"
    };
    let state = semantic_line(
        app,
        role,
        format!(
            "{} · {profile} · {} · {access}",
            if compact { "Admin" } else { "Profile" },
            app.admin.devices.state.label()
        ),
    );
    let devices = app.admin.devices.snapshot.as_ref().map_or(0, Vec::len);
    let users = app.admin.users.snapshot.as_ref().map_or(0, Vec::len);
    let queues = app.admin.overview_queues(app.now);
    let approvals = queues
        .devices_awaiting_approval
        .len()
        .saturating_add(queues.users_awaiting_approval.len());
    let keys = queues
        .expired_device_keys
        .len()
        .saturating_add(queues.soon_expiring_device_keys.len());
    let routes = queues.unapproved_routes.len();
    let freshness = app.admin.devices.observed_at.map_or_else(
        || "not observed".to_owned(),
        |observed| format!("{} ago", text::format_age(app.now.saturating_sub(observed))),
    );
    let detail = if compact {
        format!("{devices} devices · {approvals} approvals · {routes} routes · {keys} keys")
    } else {
        format!(
            "{devices} devices · {users} users · {freshness} · {approvals} approvals · {routes} routes · {keys} keys"
        )
    };
    vec![
        state,
        Line::from(Span::styled(
            detail,
            app.theme.style(theme::StyleRole::TextMuted),
        )),
    ]
}

fn render_attention(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let findings = &app.health_findings;
    let selected_id = app
        .selected_overview_finding()
        .map(|finding| finding.id.as_str());
    let mut detail = Vec::new();
    let critical = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Critical)
        .count();
    let warning = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Warning)
        .count();
    if critical > 0 {
        detail.push(format!("{critical} critical"));
    }
    if warning > 0 {
        detail.push(format!("{warning} warning"));
    }
    let title = text::view_title(
        app.theme,
        "needs attention",
        findings.len(),
        findings.len(),
        &detail,
    );
    if findings.is_empty() {
        let message = if app.admin.profile.is_some() {
            vec![
                semantic_line(
                    app,
                    theme::StyleRole::StateHealthy,
                    "No derived findings from current authoritative snapshots".to_owned(),
                ),
                Line::default(),
                Line::from(Span::styled(
                    "Offline age alone is informational and does not create a finding.",
                    app.theme.style(theme::StyleRole::TextMuted),
                )),
            ]
        } else {
            vec![
                Line::from(Span::styled(
                    "No admin health findings to show",
                    app.theme.style(theme::StyleRole::TextPrimary),
                )),
                Line::default(),
                text::inline_action(
                    app.theme,
                    "Activate a credential in ",
                    ":profiles",
                    " to evaluate tailnet health.",
                ),
            ]
        };
        panel::render_view(frame, app, area, title, message);
        return;
    }

    let columns = if area.width >= 80 {
        vec![
            grid::Column::fixed("SEVERITY", 10),
            grid::Column::fill("RESOURCE", 2),
            grid::Column::fill("FINDING", 3),
            grid::Column::fixed("DETAIL", 16),
        ]
    } else {
        vec![
            grid::Column::fixed("STATE", 9),
            grid::Column::fill("RESOURCE", 2),
            grid::Column::fill("FINDING", 3),
        ]
    };
    let viewport = usize::from(area.height.saturating_sub(3)).max(1);
    let selected = selected_id
        .and_then(|id| findings.iter().position(|finding| finding.id == id))
        .map_or(0, |position| position);
    let start = selected
        .saturating_add(1)
        .saturating_sub(viewport)
        .min(findings.len().saturating_sub(1));
    let rows = findings
        .iter()
        .skip(start)
        .take(viewport)
        .map(|finding| {
            let severity = severity_label(app, finding.severity);
            let mut cells = vec![
                grid::Cell::new(severity).with_role(severity_role(finding.severity)),
                grid::Cell::new(finding_resource_label(app, finding)),
                grid::Cell::new(finding.title.clone()),
            ];
            if area.width >= 80 {
                cells.push(grid::Cell::new(finding_detail(app, finding)));
            }
            grid::Row::new(cells).selected(selected_id == Some(finding.id.as_str()))
        })
        .collect::<Vec<_>>();
    let lines = grid::lines(app, &columns, &rows, area.width.saturating_sub(4));
    panel::render_view(frame, app, area, title, lines);
}

fn render_finding(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(finding) = app.selected_overview_finding() else {
        panel::render(frame, app, area, "finding", "No finding selected");
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            text::ellipsize(&finding.title, usize::from(area.width.saturating_sub(4))),
            app.theme.style(theme::StyleRole::TextPrimary),
        )),
        semantic_line(
            app,
            severity_role(finding.severity),
            format!("{} · Derived by Tale", finding.severity.label()),
        ),
        Line::default(),
    ];
    lines.extend(wrapped_lines(
        app,
        &finding.explanation,
        usize::from(area.width.saturating_sub(4)),
        theme::StyleRole::TextMuted,
    ));
    let affected_total = finding
        .affected_resource_ids
        .len()
        .saturating_add(finding.truncated_affected_resource_count);
    let source = if finding.source_ids.is_empty() {
        "not returned".to_owned()
    } else {
        finding.source_ids.join(", ")
    };
    let mut pairs = vec![
        ("rule", finding.rule_id.clone()),
        ("resource", finding_resource_label(app, finding)),
        (
            "resource id",
            finding
                .affected_resource_ids
                .first()
                .cloned()
                .unwrap_or_else(|| "not returned".to_owned()),
        ),
        ("affected", affected_total.to_string()),
        (
            "observed",
            format!(
                "{} ago",
                text::format_age(app.now.saturating_sub(finding.observed_at))
            ),
        ),
        ("source", source),
    ];
    if let Some(action) = finding.suggested_action_ids.first() {
        pairs.push(("suggested", action.clone()));
    }
    if let Some(expiry) = finding_fact(finding, "expires_at") {
        pairs.push(("key expiry", expiry_detail(app, expiry)));
    }
    lines.push(Line::default());
    lines.extend(grid::detail(app, &pairs));
    if !finding.observed_facts.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "Observed facts",
            app.theme.style(theme::StyleRole::SectionHeading),
        )));
        lines.extend(finding.observed_facts.iter().take(6).map(|fact| {
            Line::from(vec![
                Span::styled(
                    format!("{}  ", fact.label),
                    app.theme.style(theme::StyleRole::TextMuted),
                ),
                Span::styled(
                    fact.value.clone(),
                    app.theme.style(theme::StyleRole::TextPrimary),
                ),
            ])
        }));
    }
    lines.push(Line::default());
    lines.push(if finding.suggested_action_ids.is_empty() {
        text::inline_action(
            app.theme,
            "",
            "Enter",
            " opens details · no suggested action",
        )
    } else {
        Line::from(vec![
            Span::styled("Enter", app.theme.style(theme::StyleRole::KeyHint)),
            Span::styled(
                " opens details · ",
                app.theme.style(theme::StyleRole::TextMuted),
            ),
            Span::styled("a", app.theme.style(theme::StyleRole::KeyHint)),
            Span::styled(
                " for suggested action",
                app.theme.style(theme::StyleRole::TextMuted),
            ),
        ])
    });
    panel::render_focusable(
        frame,
        app,
        area,
        "finding",
        lines,
        app.focus == Focus::Inspector,
    );
}

fn finding_resource_label(app: &App, finding: &Finding) -> String {
    let affected = finding
        .affected_resource_ids
        .len()
        .saturating_add(finding.truncated_affected_resource_count);
    if affected != 1 {
        let noun = match finding.rule_id.as_str() {
            "client-version-skew" => "devices",
            "route-overlap-review" => "routes",
            _ => "resources",
        };
        return format!("{affected} {noun}");
    }
    let Some(id) = finding.affected_resource_ids.first() else {
        return "not returned".to_owned();
    };
    if let Some(device) = app
        .admin
        .devices
        .snapshot
        .as_ref()
        .and_then(|devices| devices.iter().find(|device| device.stable_id == *id))
    {
        return device.display_name().to_owned();
    }
    if let Some(user) = app
        .admin
        .users
        .snapshot
        .as_ref()
        .and_then(|users| users.iter().find(|user| user.id == *id))
    {
        return user.label().to_owned();
    }
    id.clone()
}

fn finding_detail(app: &App, finding: &Finding) -> String {
    if let Some(expiry) = finding_fact(finding, "expires_at") {
        return expiry_detail(app, expiry);
    }
    finding
        .observed_facts
        .first()
        .map_or_else(|| "derived".to_owned(), |fact| fact.value.clone())
}

fn finding_fact<'a>(finding: &'a Finding, label: &str) -> Option<&'a str> {
    finding
        .observed_facts
        .iter()
        .find(|fact| fact.label == label)
        .map(|fact| fact.value.as_str())
}

fn expiry_detail(app: &App, value: &str) -> String {
    let Ok(expiry) = value.parse::<u64>() else {
        return value.to_owned();
    };
    if expiry <= app.now {
        format!(
            "expired {} ago",
            text::format_age(app.now.saturating_sub(expiry))
        )
    } else {
        format!(
            "expires in {}",
            text::format_age(expiry.saturating_sub(app.now))
        )
    }
}

fn semantic_line(app: &App, role: theme::StyleRole, label: String) -> Line<'static> {
    let signal = role.signal();
    let marker = if app.resolved_config.ui.symbols.unicode() {
        signal.unicode
    } else {
        signal.ascii
    };
    Line::from(vec![
        Span::styled(format!("{marker} "), app.theme.style(role)),
        Span::styled(label, app.theme.style(role)),
    ])
}

fn severity_label(app: &App, severity: Severity) -> String {
    let role = severity_role(severity);
    let signal = role.signal();
    let marker = if app.resolved_config.ui.symbols.unicode() {
        signal.unicode
    } else {
        signal.ascii
    };
    format!("{marker} {}", severity.label())
}

const fn severity_role(severity: Severity) -> theme::StyleRole {
    match severity {
        Severity::Critical => theme::StyleRole::StateDanger,
        Severity::Warning => theme::StyleRole::StateWarning,
        Severity::Info => theme::StyleRole::StateInfo,
    }
}

fn local_status_role(status: LocalResourceStatus, source: SourceMode) -> theme::StyleRole {
    match source {
        SourceMode::Mock => return theme::StyleRole::StateInfo,
        SourceMode::Unavailable => return theme::StyleRole::StateDisabled,
        SourceMode::Local => {}
    }
    match status {
        LocalResourceStatus::Fresh => theme::StyleRole::StateHealthy,
        LocalResourceStatus::Loading | LocalResourceStatus::NeverLoaded => {
            theme::StyleRole::StatePending
        }
        LocalResourceStatus::Stale => theme::StyleRole::StateStale,
        LocalResourceStatus::Failed => theme::StyleRole::StateDanger,
    }
}

const fn admin_status_role(status: AdminResourceState) -> theme::StyleRole {
    match status {
        AdminResourceState::Ready => theme::StyleRole::StateHealthy,
        AdminResourceState::Loading | AdminResourceState::Idle => theme::StyleRole::StatePending,
        AdminResourceState::Stale => theme::StyleRole::StateStale,
        AdminResourceState::Forbidden
        | AdminResourceState::PlanRestricted
        | AdminResourceState::Unsupported
        | AdminResourceState::Unauthenticated
        | AdminResourceState::Failed => theme::StyleRole::StateDanger,
    }
}

fn wrapped_lines(
    app: &App,
    value: &str,
    width: usize,
    role: theme::StyleRole,
) -> Vec<Line<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in value.split_whitespace() {
        let extra = usize::from(!current.is_empty());
        if !current.is_empty()
            && current
                .chars()
                .count()
                .saturating_add(extra)
                .saturating_add(word.chars().count())
                > width
        {
            lines.push(Line::from(Span::styled(current, app.theme.style(role))));
            current = String::new();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(Line::from(Span::styled(current, app.theme.style(role))));
    }
    lines
}
