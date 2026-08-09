use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::domain::diagnostic::DiagnosticResult;
use crate::ui::components::{grid, panel};
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let status =
        app.local_diagnostics
            .values()
            .rev()
            .find_map(|state| match state.result.as_ref() {
                Some(DiagnosticResult::DnsStatus(value)) => Some(value),
                _ => None,
            });
    let query =
        app.local_diagnostics
            .values()
            .rev()
            .find_map(|state| match state.result.as_ref() {
                Some(DiagnosticResult::DnsQuery(value)) => Some(value),
                _ => None,
            });
    let has_admin = app.admin.nameservers.snapshot.is_some()
        || app.admin.dns_preferences.snapshot.is_some()
        || app.admin.search_paths.snapshot.is_some()
        || app.admin.split_dns.snapshot.is_some();
    if status.is_none() && query.is_none() && !has_admin {
        render_empty(frame, app, area);
        return;
    }

    let mut lines = Vec::new();
    if let Some(status) = status {
        section(app, &mut lines, "This machine");
        let mut local = Vec::new();
        push_toggle(&mut local, "forwarder", status.forwarder_enabled);
        push_toggle(&mut local, "MagicDNS", status.magic_dns_enabled);
        push_optional(&mut local, "DNS suffix", status.magic_dns_suffix.as_deref());
        push_optional(
            &mut local,
            "node name",
            status.current_node_dns_name.as_deref(),
        );
        if !status.resolvers.is_empty() {
            local.push(("resolvers", status.resolvers.join(" · ")));
        }
        if !status.cert_domains.is_empty() {
            local.push(("certificate domains", status.cert_domains.join(" · ")));
        }
        local.push((
            "observed",
            crate::ui::text::format_timestamp(status.observed_at),
        ));
        lines.extend(grid::detail(app, &local));
        for (domain, resolvers) in &status.split_routes {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {domain}"),
                    app.theme.style(theme::StyleRole::TextMuted),
                ),
                Span::styled(
                    format!("  {}", resolvers.join(" · ")),
                    app.theme.style(theme::StyleRole::TextPrimary),
                ),
            ]));
        }
    }

    if let Some(query) = query {
        section(app, &mut lines, "Last query");
        let mut query_pairs = vec![
            ("name", query.name.clone()),
            ("type", query.record_type.clone()),
            ("result", query.result_class.clone()),
        ];
        if let Some(latency) = query.latency_ms {
            query_pairs.push(("latency", format!("{latency} ms")));
        }
        if !query.answers.is_empty() {
            query_pairs.push((
                "answers",
                query
                    .answers
                    .iter()
                    .map(|answer| answer.value.as_str())
                    .collect::<Vec<_>>()
                    .join(" · "),
            ));
        }
        lines.extend(grid::detail(app, &query_pairs));
    }

    if has_admin {
        section(app, &mut lines, "Tailnet");
        let mut admin = Vec::new();
        if let Some(nameservers) = app.admin.nameservers.snapshot.as_ref()
            && !nameservers.values.is_empty()
        {
            admin.push(("nameservers", nameservers.values.join(" · ")));
        }
        if let Some(preferences) = app.admin.dns_preferences.snapshot.as_ref() {
            push_toggle(&mut admin, "MagicDNS", preferences.magic_dns);
        }
        if let Some(paths) = app.admin.search_paths.snapshot.as_ref()
            && !paths.values.is_empty()
        {
            admin.push(("search paths", paths.values.join(" · ")));
        }
        if let Some(split) = app.admin.split_dns.snapshot.as_ref() {
            admin.push(("split mappings", split.entries.len().to_string()));
        }
        lines.extend(grid::detail(app, &admin));
        if let Some(split) = app.admin.split_dns.snapshot.as_ref() {
            for (domain, resolvers) in split.iter() {
                let value = resolvers.map_or_else(
                    || "use the tailnet default".to_owned(),
                    |values| values.join(" · "),
                );
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {domain}"),
                        app.theme.style(theme::StyleRole::TextMuted),
                    ),
                    Span::styled(
                        format!("  {value}"),
                        app.theme.style(theme::StyleRole::TextPrimary),
                    ),
                ]));
            }
        }
    }
    let mut sources = Vec::new();
    if status.is_some() || query.is_some() {
        sources.push("local");
    }
    if has_admin {
        sources.push("admin");
    }
    panel::render(
        frame,
        app,
        area,
        &format!("dns · {}", sources.join(" + ")),
        lines,
    );
}

fn render_empty(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let lines = if app.admin.profile.is_some() {
        vec![
            Line::from("No DNS configuration to show"),
            Line::default(),
            Line::from("Neither the local diagnostics nor the admin API returned DNS data."),
        ]
    } else {
        vec![
            Line::from("No DNS configuration to show"),
            Line::default(),
            Line::from("Local DNS diagnostics have not been run."),
            Line::from("An admin profile adds the tailnet-wide DNS configuration."),
        ]
    };
    panel::render(frame, app, area, "dns", lines);
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

fn push_toggle(pairs: &mut Vec<(&'static str, String)>, label: &'static str, value: Option<bool>) {
    if let Some(value) = value {
        pairs.push((label, if value { "on" } else { "off" }.to_owned()));
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
