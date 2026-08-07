use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use crate::app::App;
use crate::domain::diagnostic::DiagnosticResult;
use crate::ui::components::panel;

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
    let mut lines = vec![
        Line::from("DNS · local diagnostics"),
        Line::from(format!(
            "source      local · {}",
            app.local_resource.status.label()
        )),
        Line::from(format!(
            "forwarder   {}",
            status
                .and_then(|value| value.forwarder_enabled)
                .map_or("not returned", |value| if value {
                    "enabled"
                } else {
                    "disabled"
                })
        )),
        Line::from(format!(
            "MagicDNS     {} · {}",
            status
                .and_then(|value| value.magic_dns_enabled)
                .map_or("not returned", |value| if value {
                    "enabled"
                } else {
                    "disabled"
                }),
            status
                .and_then(|value| value.magic_dns_suffix.as_deref())
                .map_or("not returned", |value| value)
        )),
        Line::from(format!(
            "resolvers    {}",
            match status {
                Some(value) => value.resolvers.join(", "),
                None => "not returned".to_owned(),
            }
        )),
        Line::from(format!(
            "split DNS    {} routes",
            status.map_or(0, |value| value.split_routes.len())
        )),
        Line::from(format!(
            "last query   {}",
            query.map_or_else(
                || "not run".to_owned(),
                |value| format!(
                    "{} {} · {}",
                    value.name, value.record_type, value.result_class
                ),
            )
        )),
        Line::from("Use a → actions → DNS query to run a read-only query."),
    ];
    if app.admin.profile.is_some() {
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "admin DNS   nameservers:{} · preferences:{} · search paths:{} · split:{}",
            app.admin.nameservers.state.label(),
            app.admin.dns_preferences.state.label(),
            app.admin.search_paths.state.label(),
            app.admin.split_dns.state.label()
        )));
        if let Some(nameservers) = app.admin.nameservers.snapshot.as_ref() {
            lines.push(Line::from(format!(
                "nameservers  {}",
                nameservers.values.join(", ")
            )));
        }
        if let Some(preferences) = app.admin.dns_preferences.snapshot.as_ref() {
            lines.push(Line::from(format!(
                "MagicDNS     {}",
                preferences
                    .magic_dns
                    .map_or("not returned", |value| if value {
                        "enabled"
                    } else {
                        "disabled"
                    })
            )));
        }
        if let Some(paths) = app.admin.search_paths.snapshot.as_ref() {
            lines.push(Line::from(format!(
                "search paths  {}",
                paths.values.join(", ")
            )));
        }
        if let Some(split) = app.admin.split_dns.snapshot.as_ref() {
            lines.push(Line::from(format!(
                "split DNS     {} mappings",
                split.entries.len()
            )));
        }
        lines.push(Line::from(
            "Admin DNS edits replace only the selected server subresource; local DNS remains separate.",
        ));
    }
    panel::render(frame, app, area, "dns", lines);
}
