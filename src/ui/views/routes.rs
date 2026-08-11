use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};

use crate::app::{App, Focus};
use crate::ui::components::{grid, panel};
use crate::ui::{text, theme};

const ADMIN_COLUMNS: &[(&str, grid::Width)] = &[
    ("S", grid::Width::Fixed(2)),
    ("DEVICE", grid::Width::Fill(16)),
    ("KIND", grid::Width::Fill(12)),
    ("ADVERTISED", grid::Width::Fill(18)),
    ("APPROVED", grid::Width::Fill(18)),
];

pub fn render_admin(frame: &mut Frame<'_>, app: &App, area: Rect, wide_inspector: Option<Rect>) {
    if app.focus == Focus::Inspector || wide_inspector.is_none() {
        if app.focus == Focus::Inspector {
            render_admin_inspector(frame, app, area);
        } else {
            render_admin_table(frame, app, area);
        }
        return;
    }
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    render_admin_table(frame, app, horizontal[0]);
    if let Some(inspector_area) = wide_inspector {
        render_admin_inspector(frame, app, inspector_area);
    } else {
        render_admin_inspector(frame, app, horizontal[1]);
    }
}

fn render_admin_table(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let resource = &app.admin.routes;
    let all = app.admin.route_observations();
    let observations = app.filtered_admin_routes();
    let lines = if observations.is_empty() {
        text::empty_state(
            app.theme,
            "routes",
            "routes",
            app.admin.profile.is_some(),
            resource.state,
            resource.error.as_deref(),
        )
    } else {
        let columns = ADMIN_COLUMNS
            .iter()
            .map(|(header, width)| grid::Column {
                header: (*header).to_owned(),
                width: *width,
            })
            .collect::<Vec<_>>();
        let viewport = usize::from(area.height.saturating_sub(3)).max(1);
        let start = app
            .admin_route_selected
            .saturating_add(1)
            .saturating_sub(viewport)
            .min(observations.len().saturating_sub(1));
        let rows = observations
            .iter()
            .enumerate()
            .skip(start)
            .take(viewport)
            .map(|(index, route)| {
                grid::Row::new([
                    completeness_cell(app, route.complete),
                    grid::Cell::new(route.device_id.clone()),
                    grid::Cell::new(route_role(route)),
                    grid::Cell::new(route_list(&route.advertised)),
                    grid::Cell::new(route_list(&route.enabled)),
                ])
                .selected(index == app.admin_route_selected)
            })
            .collect::<Vec<_>>();
        grid::lines(app, &columns, &rows, area.width.saturating_sub(4))
    };
    let mut detail = if observations.iter().any(|route| !route.complete) {
        vec!["some observations incomplete".to_owned()]
    } else {
        Vec::new()
    };
    if !app.views.routes.filter.is_empty() {
        detail.insert(
            0,
            format!("/{}", text::ellipsize(&app.views.routes.filter, 32)),
        );
    }
    panel::render_view(
        frame,
        app,
        area,
        text::view_title(app.theme, "routes", observations.len(), all.len(), &detail),
        lines,
    );
}

fn render_admin_inspector(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(route) = app.selected_admin_route_for_view() else {
        panel::render(frame, app, area, "inspector", "No route selected");
        return;
    };
    let pairs = [
        ("device", route.device_id.clone()),
        ("kind", route_role(&route).to_owned()),
        (
            "observation",
            if route.complete {
                "complete"
            } else {
                "incomplete"
            }
            .to_owned(),
        ),
        ("advertised", route_list(&route.advertised)),
        ("approved", route_list(&route.enabled)),
        ("observed", text::format_timestamp(route.observed_at)),
    ];
    let mut lines = vec![Line::from(Span::styled(
        route.device_id.clone(),
        app.theme.style(theme::StyleRole::TextPrimary),
    ))];
    lines.extend(grid::detail(app, &pairs));
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "Advertising and approval are separate server observations.",
        app.theme.style(theme::StyleRole::TextMuted),
    )));
    panel::render_focusable(
        frame,
        app,
        area,
        "inspector",
        lines,
        app.focus == Focus::Inspector,
    );
}

fn completeness_cell(app: &App, complete: bool) -> grid::Cell {
    let unicode = app.resolved_config.ui.symbols.unicode();
    if complete {
        grid::Cell::new(if unicode { "✓" } else { "+" }).with_role(theme::StyleRole::StateHealthy)
    } else {
        grid::Cell::new(if unicode { "▲" } else { "!" }).with_role(theme::StyleRole::StateWarning)
    }
}

fn route_list(routes: &[String]) -> String {
    if routes.is_empty() {
        "None".to_owned()
    } else {
        routes.join(", ")
    }
}

fn route_role(route: &crate::admin::routes::AdminRouteObservation) -> &'static str {
    if route.advertised_exit_node() {
        "exit advertisement"
    } else if !route.advertised.is_empty() {
        "subnet advertisement"
    } else if route.enabled_exit_node() {
        "exit approval"
    } else if !route.enabled.is_empty() {
        "subnet approval"
    } else if route.complete {
        "none"
    } else {
        "details incomplete"
    }
}
