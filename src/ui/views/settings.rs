use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};

use crate::app::App;
use crate::ui::theme;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut values = app.resolved_config.settings();
    values.push(crate::config::SettingDisplay {
        name: "ui.theme.session",
        value: app.theme.id().as_str().to_owned(),
        source: crate::config::ValueSource::Default,
    });
    values.push(crate::config::SettingDisplay {
        name: "ui.color.resolved",
        value: format!(
            "{} ({})",
            app.theme.capability().as_str(),
            if app.resolved_config.ui.color == crate::config::ColorMode::Auto {
                "auto policy"
            } else if app.resolved_config.ui.color == crate::config::ColorMode::None {
                "NO_COLOR or configured"
            } else {
                "configured"
            }
        ),
        source: app.resolved_config.ui.color_source,
    });
    if let Some(profile) = app.admin.profile.as_deref() {
        values.push(crate::config::SettingDisplay {
            name: "profile.selected",
            value: profile.to_owned(),
            source: crate::config::ValueSource::Cli,
        });
        values.push(crate::config::SettingDisplay {
            name: "profile.tailnet",
            value: app
                .admin
                .tailnet
                .clone()
                .unwrap_or_else(|| "unknown".to_owned()),
            source: crate::config::ValueSource::File,
        });
        values.push(crate::config::SettingDisplay {
            name: "profile.read_only",
            value: app.admin.profile_read_only.to_string(),
            source: crate::config::ValueSource::File,
        });
        values.push(crate::config::SettingDisplay {
            name: "admin.requested_scopes",
            value: if app.admin.requested_scopes.is_empty() {
                "not recorded".to_owned()
            } else {
                app.admin.requested_scopes.join(" ")
            },
            source: crate::config::ValueSource::Default,
        });
        values.push(crate::config::SettingDisplay {
            name: "admin.capabilities",
            value: app
                .admin
                .capabilities
                .iter()
                .map(|(name, state)| format!("{name}={}", state.label()))
                .collect::<Vec<_>>()
                .join(", "),
            source: crate::config::ValueSource::Default,
        });
        if let Some(settings) = app.admin.settings.snapshot.as_ref() {
            values.push(crate::config::SettingDisplay {
                name: "tailnet.devices_approval",
                value: settings
                    .devices_approval_on
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                source: crate::config::ValueSource::File,
            });
            values.push(crate::config::SettingDisplay {
                name: "tailnet.users_approval",
                value: settings
                    .users_approval_on
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                source: crate::config::ValueSource::File,
            });
            values.push(crate::config::SettingDisplay {
                name: "tailnet.acls_externally_managed",
                value: settings
                    .acls_externally_managed_on
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                source: crate::config::ValueSource::Default,
            });
            values.push(crate::config::SettingDisplay {
                name: "tailnet.devices_auto_updates",
                value: settings
                    .devices_auto_updates_on
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                source: crate::config::ValueSource::Default,
            });
            values.push(crate::config::SettingDisplay {
                name: "tailnet.devices_key_duration_days",
                value: settings
                    .devices_key_duration_days
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                source: crate::config::ValueSource::Default,
            });
            values.push(crate::config::SettingDisplay {
                name: "tailnet.network_flow_logging",
                value: settings
                    .network_flow_logging_on
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                source: crate::config::ValueSource::Default,
            });
            values.push(crate::config::SettingDisplay {
                name: "tailnet.regional_routing",
                value: settings
                    .regional_routing_on
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                source: crate::config::ValueSource::Default,
            });
            values.push(crate::config::SettingDisplay {
                name: "tailnet.posture_identity_collection",
                value: settings
                    .posture_identity_collection_on
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                source: crate::config::ValueSource::Default,
            });
            values.push(crate::config::SettingDisplay {
                name: "tailnet.https_enabled",
                value: settings
                    .https_enabled
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
                source: crate::config::ValueSource::Default,
            });
        }
        values.push(crate::config::SettingDisplay {
            name: "admin.contacts",
            value: app.admin.contacts.state.label().to_owned(),
            source: crate::config::ValueSource::Default,
        });
        if let Some(contacts) = app.admin.contacts.snapshot.as_ref() {
            for (name, contact) in [
                ("contact.account", contacts.account.as_ref()),
                ("contact.support", contacts.support.as_ref()),
                ("contact.security", contacts.security.as_ref()),
            ] {
                let value = contact.map_or_else(
                    || "not returned".to_owned(),
                    |contact| {
                        let email = contact
                            .email
                            .as_deref()
                            .or(contact.fallback_email.as_deref())
                            .map_or("not returned", |value| value);
                        format!(
                            "{}{}",
                            email,
                            if contact.needs_verification == Some(true) {
                                " · needs verification"
                            } else {
                                ""
                            }
                        )
                    },
                );
                values.push(crate::config::SettingDisplay {
                    name,
                    value,
                    source: crate::config::ValueSource::Default,
                });
            }
        }
    }
    // The palette lives on the page rather than behind a modal: picking a theme
    // applies at once, so this row is the preview. It goes first because the
    // settings list below it is long.
    let mut items = vec![
        ListItem::new(Line::from(vec![Span::styled(
            format!("{:<25} {}", "PALETTE", "a appearance changes the theme"),
            app.theme.style(theme::StyleRole::SectionHeading),
        )])),
        ListItem::new(Line::from(vec![
            Span::raw(" ".repeat(26)),
            Span::styled("✓ healthy", app.theme.style(theme::StyleRole::StateHealthy)),
            Span::raw("  "),
            Span::styled("! warning", app.theme.style(theme::StyleRole::StateWarning)),
            Span::raw("  "),
            Span::styled(
                "X danger/public",
                app.theme.style(theme::StyleRole::StatePublic),
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw(" ".repeat(26)),
            Span::styled("local", app.theme.style(theme::StyleRole::SourceLocal)),
            Span::raw("  "),
            Span::styled("admin", app.theme.style(theme::StyleRole::SourceAdmin)),
            Span::raw("  "),
            Span::styled(
                "local+admin",
                app.theme.style(theme::StyleRole::SourceCombined),
            ),
        ])),
        ListItem::new(Line::default()),
        ListItem::new(Line::from(vec![Span::styled(
            format!("{:<25} {:<32} [{}]", "SETTING", "VALUE", "SOURCE"),
            app.theme.style(theme::StyleRole::SectionHeading),
        )])),
    ];
    items.extend(values.into_iter().map(|setting| {
        ListItem::new(format!(
            "{:<25} {:<32} [{}]",
            setting.name,
            setting.value,
            setting.source.label()
        ))
    }));
    frame.render_widget(
        List::new(items)
            .style(app.theme.style(theme::StyleRole::Surface))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("settings · read-only"),
            ),
        area,
    );
}
