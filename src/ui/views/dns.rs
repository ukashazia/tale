use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::domain::diagnostic::DiagnosticResult;
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
    let lines = vec![
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
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::normal(app))
            .block(Block::default().borders(Borders::ALL).title("dns")),
        area,
    );
}
