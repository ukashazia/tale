use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::app::{App, Focus};
use crate::domain::service::{ServiceMapping, ServiceResourceStatus, ServiceSection};
use crate::ui::components::{grid, panel};
use crate::ui::text;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, wide_inspector: Option<Rect>) {
    if app.focus == Focus::Inspector || wide_inspector.is_none() {
        if app.focus == Focus::Inspector {
            render_inspector(frame, app, area);
        } else {
            render_collection(frame, app, area);
        }
        return;
    }
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    if let Some(&collection) = horizontal.first() {
        render_collection(frame, app, collection);
    }
    let inspector = wide_inspector.or_else(|| horizontal.get(1).copied());
    if let Some(inspector) = inspector {
        render_inspector(frame, app, inspector);
    }
}

/// The sections as a tab strip. This belongs to the collection pane, not to
/// the app, so it is drawn inside that pane's border.
fn tab_line(app: &App) -> Line<'static> {
    let current = app.views.services.section;
    let mut spans = Vec::new();
    for section in ServiceSection::ALL {
        let selected = section == current;
        spans.push(Span::styled(
            format!(" {} ", section.label()),
            if selected {
                app.theme
                    .style(theme::StyleRole::Focus)
                    .add_modifier(Modifier::REVERSED)
            } else {
                app.theme.style(theme::StyleRole::TextMuted)
            },
        ));
        spans.push(Span::styled(
            " ",
            app.theme.style(theme::StyleRole::Surface),
        ));
    }
    Line::from(spans)
}

/// The route's own context, in the border where every other view keeps it.
fn collection_title(app: &App) -> String {
    let section = app.views.services.section;
    let shown = section_row_count(app);
    let total = match section {
        ServiceSection::Serve => app.service_mapping_total(),
        _ => shown,
    };
    let mut detail = Vec::new();
    if section == ServiceSection::Serve {
        let public = app
            .visible_service_mappings()
            .iter()
            .filter(|mapping| mapping.exposure.is_public())
            .count();
        if public > 0 {
            detail.push(format!("{public} public"));
        }
        let filter = app.views.services.filter_draft.trim();
        if !filter.is_empty() {
            detail.push(format!("/{filter}"));
        }
        let sort = app.views.services.sort;
        detail.push(format!(
            "{} {}",
            sort.field.label(),
            if sort.direction.is_ascending() {
                "\u{2191}"
            } else {
                "\u{2193}"
            }
        ));
    }
    text::view_title(section.noun(), shown, total, &detail)
}

fn section_row_count(app: &App) -> usize {
    match app.views.services.section {
        ServiceSection::Serve => app.visible_service_mappings().len(),
        ServiceSection::Taildrive if !app.alpha_local_features => 0,
        ServiceSection::Taildrive => app
            .services_snapshot
            .taildrive
            .value
            .as_ref()
            .map_or(0, Vec::len),
        ServiceSection::Certificates => app
            .services_snapshot
            .certificate_domains
            .value
            .as_ref()
            .map_or(0, Vec::len),
    }
}

fn render_collection(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = vec![tab_line(app), Line::default()];
    if app.resolved_config.read_only {
        lines.push(Line::from(Span::styled(
            "Read-only: nothing here can be changed",
            app.theme.style(theme::StyleRole::StateDisabled),
        )));
    }
    if let Some(failure) = section_failure(app) {
        lines.push(Line::from(Span::styled(
            format!("{} · {}", failure.kind.label(), failure.detail),
            app.theme.style(theme::StyleRole::StateDanger),
        )));
    }
    let (columns, mut rows) = section_rows(app);
    if let Some(row) = rows.get_mut(app.views.services.selected) {
        *row = row.clone().selected(true);
    }
    let body = if rows.is_empty() {
        Vec::new()
    } else {
        grid::lines(app, &columns, &rows, area.width.saturating_sub(4))
    };
    if body.is_empty() {
        lines.extend(section_empty_message(app));
    } else {
        lines.extend(body);
    }
    panel::render(frame, app, area, &collection_title(app), lines);
}

/// Every section is the same table, differing only in its columns. Exposure is
/// the one genuinely dangerous state on this screen, so it is the one styled.
fn section_rows(app: &App) -> (Vec<grid::Column>, Vec<grid::Row>) {
    match app.views.services.section {
        ServiceSection::Serve => (
            vec![
                grid::Column::fixed("EXPOSURE", 8),
                grid::Column::fixed("LISTENER", 22),
                grid::Column::fill("PATH", 1),
                grid::Column::fill("BACKEND", 2),
            ],
            app.visible_service_mappings()
                .into_iter()
                .map(|mapping| {
                    grid::Row::new(vec![
                        mapping.exposure.label().to_owned(),
                        listener_label(mapping),
                        mapping.mount.as_path().to_owned(),
                        mapping.backend.argument(),
                    ])
                    .with_role(if mapping.exposure.is_public() {
                        theme::StyleRole::StatePublic
                    } else {
                        theme::StyleRole::TextPrimary
                    })
                })
                .collect(),
        ),
        ServiceSection::Taildrive => (
            vec![
                grid::Column::fixed("NAME", 20),
                grid::Column::fill("FOLDER", 1),
            ],
            if app.alpha_local_features {
                app.services_snapshot
                    .taildrive
                    .value
                    .iter()
                    .flat_map(|shares| shares.iter())
                    .map(|share| {
                        grid::Row::new(vec![share.name.clone(), share.path.display().to_string()])
                    })
                    .collect()
            } else {
                Vec::new()
            },
        ),
        ServiceSection::Certificates => (
            vec![grid::Column::fill("DOMAIN", 1)],
            app.services_snapshot
                .certificate_domains
                .value
                .iter()
                .flat_map(|domains| domains.iter())
                .map(|domain| grid::Row::new(vec![domain.clone()]))
                .collect(),
        ),
    }
}

fn listener_label(mapping: &ServiceMapping) -> String {
    format!("{}:{}", mapping.listener.label(), mapping.listener.port())
}

fn render_inspector(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let section = app.views.services.section;
    let mut lines = Vec::new();
    match section {
        ServiceSection::Serve => {
            if let Some(mapping) = app.selected_service_mapping() {
                if mapping.exposure.is_public() {
                    lines.push(Line::from(Span::styled(
                        "Reachable from the public internet",
                        app.theme.style(theme::StyleRole::StatePublic),
                    )));
                    lines.push(Line::default());
                }
                lines.push(field(app, "exposure", mapping.exposure.label()));
                lines.push(field(app, "listener", &listener_label(&mapping)));
                lines.push(field(app, "path", mapping.mount.as_path()));
                lines.push(field(app, "backend", &mapping.backend.argument()));
                lines.push(field(app, "backend kind", mapping.backend.label()));
                if let Some(proxy) = mapping.proxy_protocol.cli_value() {
                    lines.push(field(app, "proxy protocol", proxy));
                }
            }
        }
        ServiceSection::Taildrive => {
            if !app.alpha_local_features {
                lines.push(Line::from(Span::styled(
                    "Taildrive is alpha and off for this run",
                    app.theme.style(theme::StyleRole::StateWarning),
                )));
                lines.push(Line::default());
            }
            if let Some(share) = app
                .alpha_local_features
                .then(|| app.selected_taildrive_share())
                .flatten()
            {
                lines.push(field(app, "name", &share.name));
                lines.push(field(app, "path", &share.path.display().to_string()));
                if let Some(user) = share.as_user.as_deref() {
                    lines.push(field(app, "as user", user));
                }
            }
        }
        ServiceSection::Certificates => {
            if let Some(domain) = app
                .services_snapshot
                .certificate_domains
                .value
                .as_ref()
                .and_then(|domains| domains.get(app.views.services.selected))
            {
                lines.push(field(app, "domain", domain));
                lines.push(Line::default());
            }
            lines.push(Line::from(Span::styled(
                "Private keys are never shown, copied, logged, or stored.",
                app.theme.style(theme::StyleRole::TextMuted),
            )));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Nothing selected",
            app.theme.style(theme::StyleRole::TextMuted),
        )));
    }
    panel::render_wrapped(frame, app, area, "inspector", lines);
}

fn field(app: &App, name: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            text::pad_or_trim(name, 15),
            app.theme.style(theme::StyleRole::TextMuted),
        ),
        Span::styled(
            value.to_owned(),
            app.theme.style(theme::StyleRole::TextPrimary),
        ),
    ])
}

fn section_status(app: &App) -> ServiceResourceStatus {
    match app.views.services.section {
        ServiceSection::Serve => app.services_snapshot.mapping_status(),
        ServiceSection::Taildrive => app.services_snapshot.taildrive.status,
        ServiceSection::Certificates => app.services_snapshot.certificate_domains.status,
    }
}

fn section_failure(app: &App) -> Option<&crate::domain::service::ServiceFailure> {
    match app.views.services.section {
        ServiceSection::Serve => app.services_snapshot.mapping_failure(),
        ServiceSection::Taildrive => app.services_snapshot.taildrive.failure.as_ref(),
        ServiceSection::Certificates => app.services_snapshot.certificate_domains.failure.as_ref(),
    }
}

/// An empty box is a dead end. Name the reason and the next step.
fn section_empty_message(app: &App) -> Vec<Line<'static>> {
    let section = app.views.services.section;
    let noun = section.noun();
    if section == ServiceSection::Taildrive && !app.alpha_local_features {
        return vec![
            text::muted_help(app.theme, "Taildrive is alpha and off for this run"),
            Line::default(),
            text::action_hint(app.theme, "  enable for this run    ", "a e"),
        ];
    }
    if section == ServiceSection::Serve && !app.views.services.filter_draft.trim().is_empty() {
        return vec![
            text::muted_help(app.theme, format!("No {noun} match this filter")),
            Line::default(),
            text::action_hint(app.theme, "  clear the filter       ", "/ then Esc"),
        ];
    }
    match section_status(app) {
        ServiceResourceStatus::Idle => vec![
            text::muted_help(app.theme, format!("No {noun} loaded yet")),
            Line::default(),
            text::action_hint(app.theme, "  load                   ", "r"),
        ],
        ServiceResourceStatus::Loading => {
            vec![text::muted_help(app.theme, format!("Loading {noun}…"))]
        }
        ServiceResourceStatus::Unsupported => vec![text::muted_help(
            app.theme,
            format!("This version of the Tailscale client does not report {noun}."),
        )],
        ServiceResourceStatus::Failed => vec![
            text::muted_help(app.theme, format!("Reading {noun} failed")),
            Line::default(),
            text::action_hint(app.theme, "  retry                  ", "r"),
        ],
        ServiceResourceStatus::Ready | ServiceResourceStatus::Stale => {
            vec![text::muted_help(
                app.theme,
                format!("This machine has no {noun}"),
            )]
        }
    }
}
