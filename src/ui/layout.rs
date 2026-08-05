use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::{App, Focus, InteractionMode, TransientKind};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LayoutMode {
    Minimum,
    Compact,
    Collection,
    Wide,
}

#[derive(Debug, Clone, Copy)]
pub struct FrameLayout {
    pub minimum: bool,
    pub mode: LayoutMode,
    pub header: Rect,
    pub title: Rect,
    pub content: Rect,
    pub inspector: Option<Rect>,
    pub notification: Rect,
    pub footer: Rect,
}

pub fn compute(area: Rect, app: &App) -> FrameLayout {
    let minimum = area.width < 60 || area.height < 18;
    if minimum {
        return FrameLayout {
            minimum: true,
            mode: LayoutMode::Minimum,
            header: area,
            title: area,
            content: area,
            inspector: None,
            notification: area,
            footer: area,
        };
    }
    let mode = if area.width >= 110 {
        LayoutMode::Wide
    } else if area.width >= 80 {
        LayoutMode::Collection
    } else {
        LayoutMode::Compact
    };
    let interaction_height = match &app.interaction {
        InteractionMode::Normal => 1,
        InteractionMode::CommandLine(_) => navigation_palette_height(),
        InteractionMode::FilterLine(state) => {
            u16::try_from(state.candidates.len().min(6).saturating_add(1)).map_or(7, |value| value)
        }
        InteractionMode::Transient(state) => match state.kind {
            TransientKind::Action => {
                crate::ui::components::interaction_shell::action_menu_height(state, area.width)
            }
            TransientKind::Copy => 1,
        },
        InteractionMode::HelpSheet(_) => area.height.saturating_mul(3) / 5,
    }
    .max(1)
    .min(area.height.saturating_sub(4));
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(interaction_height),
        ])
        .split(area);
    let content = vertical[2];
    let inspector = if mode == LayoutMode::Wide && app.focus != Focus::Inspector {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(content);
        Some(horizontal[1])
    } else {
        None
    };
    FrameLayout {
        minimum: false,
        mode,
        header: vertical[0],
        title: vertical[1],
        content,
        inspector,
        notification: vertical[3],
        footer: vertical[4],
    }
}

pub const fn navigation_columns(width: u16) -> usize {
    if width >= 120 {
        3
    } else if width >= 60 {
        2
    } else {
        1
    }
}

pub const fn navigation_palette_height() -> u16 {
    14
}

pub const fn action_menu_columns(width: u16) -> usize {
    if width >= 160 {
        5
    } else if width >= 80 {
        4
    } else {
        3
    }
}
