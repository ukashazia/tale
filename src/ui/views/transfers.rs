use ratatui::text::Line;

use crate::app::App;

pub fn taildrop_lines(app: &App) -> Vec<Line<'static>> {
    app.services_snapshot
        .taildrop_targets
        .value
        .as_ref()
        .map(|targets| {
            targets
                .iter()
                .enumerate()
                .map(|(index, target)| {
                    Line::from(format!(
                        "{} {} · {} · {}",
                        if index == app.views.services.selected {
                            ">"
                        } else {
                            " "
                        },
                        target.command_target,
                        target.display_name,
                        if target.available() {
                            "available"
                        } else {
                            target.capability_reason.as_deref().unwrap_or("offline")
                        }
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn taildrive_lines(app: &App) -> Vec<Line<'static>> {
    if !app.alpha_local_features {
        return vec![Line::from("ALPHA · disabled until enabled for this run")];
    }
    app.services_snapshot
        .taildrive
        .value
        .as_ref()
        .map(|shares| {
            shares
                .iter()
                .enumerate()
                .map(|(index, share)| {
                    Line::from(format!(
                        "{} {} · {}",
                        if index == app.views.services.selected {
                            ">"
                        } else {
                            " "
                        },
                        share.name,
                        share.path.display()
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}
