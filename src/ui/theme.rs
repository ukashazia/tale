use ratatui::style::{Color, Modifier, Style};

use crate::app::App;
use crate::config::ColorMode;

pub fn normal(app: &App) -> Style {
    if app.resolved_config.ui.color == ColorMode::None {
        Style::default()
    } else {
        Style::default().fg(Color::Gray)
    }
}

pub fn title() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

pub fn focused() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn selected(app: &App) -> Style {
    let _ = app;
    Style::default().add_modifier(Modifier::REVERSED)
}

pub fn healthy(app: &App) -> Style {
    if app.resolved_config.ui.color == ColorMode::None {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    }
}

pub fn attention(app: &App) -> Style {
    if app.resolved_config.ui.color == ColorMode::None {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Yellow)
    }
}

pub fn error(app: &App) -> Style {
    if app.resolved_config.ui.color == ColorMode::None {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    }
}
