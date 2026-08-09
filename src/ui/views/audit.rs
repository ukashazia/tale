use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};

use crate::app::{App, Focus};
use crate::domain::activity::AuditEvent;
use crate::ui::components::{grid, panel};
use crate::ui::{text, theme};

/// What the tailnet was told, as opposed to what this client did. The two used
/// to share one route and one pane, which meant neither had room to be a table.
const COLUMNS: &[(&str, grid::Width)] = &[
    ("TIME", grid::Width::Fill(20)),
    ("ACTOR", grid::Width::Fill(16)),
    ("ACTION", grid::Width::Fill(20)),
    ("TARGET", grid::Width::Fill(18)),
];

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, wide_inspector: Option<Rect>) {
    if app.focus != Focus::Inspector && wide_inspector.is_none() && area.width >= 110 {
        let regions = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);
        render_events(frame, app, regions[0]);
        render_delivery(frame, app, regions[1]);
        return;
    }
    if app.focus == Focus::Inspector || wide_inspector.is_none() {
        if app.focus == Focus::Inspector {
            render_inspector(frame, app, area);
        } else {
            render_events(frame, app, area);
        }
        return;
    }
    let regions = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    render_events(frame, app, regions[0]);
    if let Some(inspector_area) = wide_inspector {
        render_inspector(frame, app, inspector_area);
    } else {
        render_inspector(frame, app, regions[1]);
    }
}

fn render_events(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let resource = &app.admin.activity;
    let Some(snapshot) = resource.snapshot.as_ref() else {
        let lines = text::empty_state(
            "audit events",
            "audit",
            app.admin.profile.is_some(),
            resource.state,
            resource.error.as_deref(),
        )
        .into_iter()
        .map(|line| {
            Line::from(Span::styled(
                line,
                app.theme.style(theme::StyleRole::TextMuted),
            ))
        })
        .collect::<Vec<_>>();
        panel::render(frame, app, area, "audit", lines);
        return;
    };
    let events = snapshot.filtered_events(&app.audit_filters);
    let columns = COLUMNS
        .iter()
        .map(|(header, width)| grid::Column {
            header: (*header).to_owned(),
            width: *width,
        })
        .collect::<Vec<_>>();
    let viewport = usize::from(area.height.saturating_sub(3)).max(1);
    let start = app
        .admin_activity_selected
        .saturating_add(1)
        .saturating_sub(viewport)
        .min(events.len().saturating_sub(1));
    let rows = events
        .iter()
        .enumerate()
        .skip(start)
        .take(viewport)
        .map(|(index, event)| {
            let cells = COLUMNS
                .iter()
                .map(|(header, _)| grid::Cell::new(value(event, header)))
                .collect::<Vec<_>>();
            grid::Row::new(cells).selected(index == app.admin_activity_selected)
        })
        .collect::<Vec<_>>();
    let lines = grid::lines(app, &columns, &rows, area.width.saturating_sub(4));
    panel::render(
        frame,
        app,
        area,
        &title(app, events.len(), snapshot.events.len()),
        lines,
    );
}

fn value(event: &AuditEvent, header: &str) -> String {
    match header {
        "TIME" => text::format_timestamp(event.event_time),
        "ACTOR" => event
            .actor
            .as_ref()
            .and_then(|actor| actor.display.as_deref().or(actor.id.as_deref()))
            .map_or_else(|| "-".to_owned(), str::to_owned),
        "ACTION" => event
            .action
            .as_deref()
            .map_or_else(|| "-".to_owned(), str::to_owned),
        "TARGET" => event
            .target
            .as_ref()
            .and_then(|target| target.display.as_deref().or(target.id.as_deref()))
            .map_or_else(|| "-".to_owned(), str::to_owned),
        _ => "-".to_owned(),
    }
}

fn render_inspector(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(event) = app.selected_audit_event_for_view() else {
        panel::render(frame, app, area, "inspector", "No audit event selected");
        return;
    };
    let mut pairs = vec![("time", text::format_timestamp(event.event_time))];
    push_optional(&mut pairs, "event", event.event_type.as_deref());
    push_optional(&mut pairs, "action", event.action.as_deref());
    if let Some(actor) = event.actor.as_ref() {
        push_optional(
            &mut pairs,
            "actor",
            actor.display.as_deref().or(actor.id.as_deref()),
        );
        push_optional(&mut pairs, "actor type", actor.kind.as_deref());
    }
    if let Some(target) = event.target.as_ref() {
        push_optional(
            &mut pairs,
            "target",
            target.display.as_deref().or(target.id.as_deref()),
        );
        push_optional(&mut pairs, "target type", target.kind.as_deref());
    }
    push_optional(&mut pairs, "origin", event.origin.as_deref());
    push_optional(&mut pairs, "details", event.action_details.as_deref());
    push_optional(&mut pairs, "error", event.error.as_deref());
    if event.old.is_some() || event.new.is_some() {
        pairs.push((
            "change",
            "Open the investigation to review the diff".to_owned(),
        ));
    }
    let title = event
        .action
        .as_deref()
        .or(event.event_type.as_deref())
        .map_or("Audit event", |value| value);
    let mut lines = vec![Line::from(Span::styled(
        title.to_owned(),
        app.theme.style(theme::StyleRole::TextPrimary),
    ))];
    lines.extend(grid::detail(app, &pairs));
    lines.extend(delivery_summary(app));
    panel::render_focusable_wrapped(
        frame,
        app,
        area,
        "inspector",
        lines,
        app.focus == Focus::Inspector,
    );
}

fn delivery_summary(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::default(),
        Line::from(Span::styled(
            "Delivery",
            app.theme.style(theme::StyleRole::SectionHeading),
        )),
    ];
    for summary in [
        super::flows::summary(app),
        super::log_streams::summary(app),
        super::webhooks::summary(app),
    ] {
        lines.extend(summary.lines().map(|line| {
            Line::from(Span::styled(
                line.to_owned(),
                app.theme.style(theme::StyleRole::TextMuted),
            ))
        }));
    }
    lines
}

fn render_delivery(frame: &mut Frame<'_>, app: &App, area: Rect) {
    panel::render_wrapped(frame, app, area, "delivery", delivery_summary(app));
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

fn title(app: &App, shown: usize, total: usize) -> String {
    let snapshot = app.admin.activity.snapshot.as_ref();
    let mut detail = vec![
        if snapshot.is_some_and(|snapshot| snapshot.delayed) {
            "delivery may be delayed".to_owned()
        } else {
            "server order".to_owned()
        },
        // Worth repeating on the route itself: an empty audit is not evidence
        // that nothing happened, only that nothing was configured.
        "configuration changes only".to_owned(),
    ];
    if app.audit_filters != crate::domain::activity::AuditFilters::default() {
        detail.insert(0, "filtered".to_owned());
    }
    text::view_title("audit", shown, total, &detail)
}
