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
        InteractionMode::Normal => {
            crate::ui::components::interaction_shell::normal_height(app, area.width)
        }
        InteractionMode::CommandLine(_) => navigation_palette_height(),
        InteractionMode::FilterLine(_) => {
            crate::ui::components::interaction_shell::filter_menu_height(
                app,
                area.width,
                area.height,
            )
        }
        InteractionMode::Transient(state) => match state.kind {
            TransientKind::Action => {
                crate::ui::components::interaction_shell::action_menu_height(app, state, area.width)
            }
            TransientKind::Copy => {
                crate::ui::components::interaction_shell::copy_menu_height(state, area.width)
            }
            TransientKind::Choice => {
                crate::ui::components::interaction_shell::choice_menu_height(state, area.width)
            }
        },
        InteractionMode::HelpSheet => {
            crate::ui::components::interaction_shell::help_menu_height(app, area.width)
        }
    }
    .max(1)
    .min(area.height.saturating_sub(4));
    let notification_height = crate::ui::components::notification::rows(app, area.width)
        .min(area.height / 4)
        .max(1);
    // The route name, its counts, and its filter live in the view's own border
    // title, so no separate row repeats them under the header.
    let header_height = crate::ui::components::header::rows(area.height);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(1),
            Constraint::Length(notification_height),
            Constraint::Length(interaction_height),
        ])
        .split(area);
    let content = vertical[1];
    let inspector = if mode == LayoutMode::Wide
        && app.focus != Focus::Inspector
        && app.inspector_pane_visible()
    {
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
        content,
        inspector,
        notification: vertical[2],
        footer: vertical[3],
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
    crate::ui::components::interaction_shell::navigation_height()
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

/// Filter cells carry a field name and a short description.
pub const fn filter_menu_columns(width: u16) -> usize {
    if width >= 80 {
        3
    } else if width >= 54 {
        2
    } else {
        1
    }
}

/// Narrowest cell that still shows a field name beside a readable description.
const FILTER_CELL_MINIMUM: usize = 20;

/// How far the grid may fold when a short terminal cannot show the tall shape.
pub const fn filter_menu_column_limit(width: u16) -> usize {
    let width = width as usize;
    let mut columns = 1;
    while columns < 4 {
        let separators = columns * 3;
        if width < separators || (width - separators) / (columns + 1) < FILTER_CELL_MINIMUM {
            return columns;
        }
        columns += 1;
    }
    columns
}

/// Choice menus can carry a group per subject, so they use the action menu's
/// denser column count.
pub const fn choice_menu_columns(width: u16) -> usize {
    action_menu_columns(width)
}

/// Copy labels are short, so the grid can be dense.
pub const fn copy_menu_columns(width: u16) -> usize {
    if width >= 110 {
        3
    } else if width >= 70 {
        2
    } else {
        1
    }
}

pub const fn help_menu_columns(width: u16) -> usize {
    if width >= 120 {
        4
    } else if width >= 80 {
        3
    } else {
        2
    }
}
