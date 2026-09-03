use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use crate::app::App;
use crate::domain::admin_mutation::{BatchChildOutcome, BatchMutation};
use crate::ui::components::panel;

pub fn render(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    batch: &BatchMutation,
    scroll: usize,
    title: &str,
) {
    let lines = lines(batch);
    let max_scroll = max_scroll(batch, area.width, area.height);
    panel::render_scrolled_styled(
        frame,
        app,
        area,
        title,
        lines,
        scroll.min(max_scroll) as u16,
        crate::ui::theme::StyleRole::SurfaceRaised,
    );
}

fn lines(batch: &BatchMutation) -> Vec<Line<'static>> {
    let outcomes = batch
        .targets
        .iter()
        .map(|target| {
            let outcome = batch
                .child_outcomes
                .get(&target.target_id)
                .copied()
                .map_or("pending", BatchChildOutcome::label);
            Line::from(format!(
                "{outcome} · {} · {}",
                target.target_label, target.requested_change
            ))
        })
        .collect::<Vec<_>>();
    let summary = if batch.child_outcomes.len() < batch.targets.len() {
        "Updating the remaining devices"
    } else if batch.has_partial_failure() {
        "Some devices could not be updated; completed changes were kept"
    } else if batch.has_failures() {
        "failed targets require review before any new preview"
    } else {
        "All devices updated"
    };
    let summary = Line::from(format!(
        "{summary} · {}/{} updated",
        batch.verified_count(),
        batch.targets.len()
    ));
    std::iter::once(summary)
        .chain(std::iter::once(Line::default()))
        .chain(outcomes)
        .collect()
}

pub fn max_scroll(batch: &BatchMutation, area_width: u16, area_height: u16) -> usize {
    let visual_lines = panel::wrapped_line_count(lines(batch), area_width.saturating_sub(4));
    visual_lines.saturating_sub(usize::from(area_height.saturating_sub(2)))
}
