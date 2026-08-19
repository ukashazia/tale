use ratatui::Frame;
use ratatui::layout::Rect;

use crate::app::App;
use crate::domain::admin_mutation::{BatchChildOutcome, BatchMutation};
use crate::ui::components::panel;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, batch: &BatchMutation) {
    let lines = batch
        .targets
        .iter()
        .map(|target| {
            let outcome = batch
                .child_outcomes
                .get(&target.target_id)
                .copied()
                .map_or("pending", BatchChildOutcome::label);
            format!(
                "{outcome} · {} · {}",
                target.target_label, target.requested_change
            )
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
    let summary = format!(
        "{summary} · {}/{} updated",
        batch.verified_count(),
        batch.targets.len()
    );
    panel::render_styled(
        frame,
        app,
        area,
        "batch outcomes",
        format!("{summary}\n\n{}", lines.join("\n")),
        theme::StyleRole::SurfaceRaised,
        theme::StyleRole::BorderNormal,
    );
}
