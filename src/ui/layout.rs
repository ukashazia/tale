use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::{App, Focus};

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
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
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
