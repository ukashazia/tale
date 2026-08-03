use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::domain::route::{overlapping_routes, parse_route_set};
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let preferences = &app.local_preferences;
    let routes = preferences.advertised_routes.value.as_ref().map_or_else(
        || "not returned".to_owned(),
        |value| {
            if value.is_empty() {
                "none".to_owned()
            } else {
                value.join(", ")
            }
        },
    );
    let relay_port = match preferences.relay_server_port.value {
        Some(value) => value.to_string(),
        None if preferences.relay_server_port_disabled.value == Some(true) => "disabled".to_owned(),
        None => "not returned".to_owned(),
    };
    let relay_endpoints = preferences
        .relay_server_static_endpoints
        .value
        .as_ref()
        .map_or_else(
            || "not returned".to_owned(),
            |value| {
                if value.is_empty() {
                    "none".to_owned()
                } else {
                    value.join(", ")
                }
            },
        );
    let mut lines = vec![
        Line::from(format!("routes       {routes}")),
        Line::from(format!(
            "exit advert  {}",
            boolean(preferences.advertised_exit_node.value)
        )),
        Line::from(format!(
            "app connector {}",
            boolean(preferences.app_connector.value)
        )),
        Line::from(format!("relay port   {relay_port}")),
        Line::from(format!("relay peers  {relay_endpoints}")),
        Line::from(
            "This device will advertise; a tailnet administrator may still need to approve the route.",
        ),
    ];
    if let Some(value) = preferences.advertised_routes.value.as_ref()
        && let Ok(parsed) = parse_route_set(&value.join(","))
    {
        for (left, right) in overlapping_routes(&parsed) {
            lines.push(Line::from(format!(
                "warning: overlapping routes {left} and {right}"
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(lines).style(theme::normal(app)).block(
            Block::default()
                .borders(Borders::ALL)
                .title("advertisements"),
        ),
        area,
    );
}

fn boolean(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "on",
        Some(false) => "off",
        None => "not returned",
    }
}
