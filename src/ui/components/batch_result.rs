use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::domain::admin_mutation::{BatchChildOutcome, BatchMutation};
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
        "batch in progress: undispatched targets remain pending"
    } else if batch.has_partial_failure() {
        "partial failure: verified successes are preserved"
    } else if batch.has_failures() {
        "failed targets require review before any new preview"
    } else {
        "all targets verified"
    };
    let summary = format!(
        "{summary} · {}/{} verified",
        batch.verified_count(),
        batch.targets.len()
    );
    frame.render_widget(
        Paragraph::new(format!("{summary}\n\n{}", lines.join("\n")))
            .style(theme::normal(app))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("batch outcomes"),
            ),
        area,
    );
}
