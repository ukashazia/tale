use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::ui::text::ellipsize;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let profile = app
        .resolved_config
        .profile
        .as_deref()
        .map_or("offline", |value| value);
    let source = if app.source_mode == crate::app::SourceMode::Local {
        match app.local_resource.status {
            crate::domain::source::LocalResourceStatus::Loading => "local: discovering".to_owned(),
            crate::domain::source::LocalResourceStatus::Stale => "local: stale".to_owned(),
            _ => format!("local: {}", app.local_state.label()),
        }
    } else {
        format!("source: {}", app.source_mode.label())
    };
    let line = Line::from(vec![
        Span::styled("Tale", theme::title()),
        Span::raw(format!(" · {profile} · {source}")),
        Span::raw(format!(" · tasks: {}", app.tasks.all().len())),
    ]);
    frame.render_widget(Paragraph::new(line).style(theme::normal(app)), area);
}

pub fn render_route_line(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let route = app.current_route().label();
    if app.current_route() == crate::app::Route::Services {
        let section = app.views.services.section.label();
        let count = match app.views.services.section {
            crate::domain::service::ServiceSection::Serve => app
                .services_snapshot
                .serve
                .value
                .as_ref()
                .map_or(0, |value| value.mappings.len()),
            crate::domain::service::ServiceSection::Funnel => app
                .services_snapshot
                .funnel
                .value
                .as_ref()
                .map_or(0, |value| value.mappings.len()),
            crate::domain::service::ServiceSection::Taildrop => app
                .services_snapshot
                .taildrop_targets
                .value
                .as_ref()
                .map_or(0, Vec::len),
            crate::domain::service::ServiceSection::Taildrive => app
                .services_snapshot
                .taildrive
                .value
                .as_ref()
                .map_or(0, Vec::len),
            crate::domain::service::ServiceSection::Certificates => app
                .services_snapshot
                .certificate_domains
                .value
                .as_ref()
                .map_or(0, Vec::len),
            crate::domain::service::ServiceSection::Metrics => {
                usize::from(app.services_snapshot.metrics.value.is_some())
            }
            crate::domain::service::ServiceSection::BugReport => {
                usize::from(app.services_snapshot.bug_report.value.is_some())
            }
        };
        let line = format!(
            "{route} · {section}  {count}  source:{}",
            app.services_snapshot
                .observed_at
                .map_or("loading", |_| "local")
        );
        frame.render_widget(Paragraph::new(line).style(theme::title()), area);
        return;
    }
    let filter = if app.views.devices.filter_draft.is_empty() {
        "".to_owned()
    } else {
        format!(" / {}", ellipsize(&app.views.devices.filter_draft, 28))
    };
    let count = app.visible_indices().len();
    let total = app.devices_resource.snapshot.len();
    let line = format!(
        "{route}{filter}  {count}/{total}  source:{}",
        app.devices_resource.health.label()
    );
    frame.render_widget(Paragraph::new(line).style(theme::title()), area);
}
