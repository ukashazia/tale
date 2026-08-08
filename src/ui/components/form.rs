use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, FieldKind, FormState, OperatorFormState};
use crate::ui::{text, theme};

pub fn render_operator(frame: &mut Frame<'_>, app: &App, area: Rect, state: &OperatorFormState) {
    let hint = match state.action_id {
        crate::action::ActionId::LocalPreferencesEdit => {
            "field=value pairs: accept-dns, accept-routes, shields-up, ssh, auto-update, update-check, report-posture, hostname, nickname, webclient"
        }
        crate::action::ActionId::LocalExitNodeSelect => {
            "target: none, auto:any, or current candidate ID/DNS/IP; optional lan=true; run Phase-2 ping before selecting if latency is unknown"
        }
        crate::action::ActionId::LocalRoutesEditAdvertisements => {
            "routes=10.0.0.0/8,fd00::/8;exit=true;connector=false;relay-port=0;relay-endpoints=203.0.113.1:443"
        }
        crate::action::ActionId::AdminDeviceRename => "new machine name",
        crate::action::ActionId::AdminDeviceTagsReplace => {
            "complete tag set: tag:team-a,tag:prod (empty clears tags)"
        }
        crate::action::ActionId::AdminDeviceKeyExpiryConfigure => "key expiry: on/off",
        crate::action::ActionId::AdminRoutesReplaceApprovals => {
            "complete approved CIDR set: 10.0.0.0/8,2001:db8::/32"
        }
        crate::action::ActionId::AdminDnsPreferencesEdit => "MagicDNS: on/off",
        crate::action::ActionId::AdminDnsNameserversReplace => {
            "complete ordered IP list: 1.1.1.1,9.9.9.9 (empty clears)"
        }
        crate::action::ActionId::AdminDnsSearchPathsReplace => {
            "complete ordered suffix list: example.com,corp.example.com (empty clears)"
        }
        crate::action::ActionId::AdminDnsSplitCreate
        | crate::action::ActionId::AdminDnsSplitEdit => "suffix=resolver[,resolver...]",
        crate::action::ActionId::AdminDnsSplitRemove => "suffix to remove",
        crate::action::ActionId::AdminUserRoleChange => {
            "documented role: owner/member/admin/it-admin/network-admin/billing-admin/auditor"
        }
        crate::action::ActionId::AdminCredentialAuthKeyCreate => {
            "description=text;expiry=7d;reusable=false;ephemeral=true;preauthorized=false;tags=tag:team-a,tag:prod"
        }
        crate::action::ActionId::AdminWebhookCreate => {
            "url=https://host.example/path;provider=slack;categories=nodeCreated;events=nodeNeedsApproval"
        }
        crate::action::ActionId::AdminWebhookEdit => {
            "categories=nodeCreated,nodeNeedsApproval;events=userCreated (unknown events are preserved)"
        }
        crate::action::ActionId::AdminLogStreamReplace => {
            "type=configuration|network;destination=splunk|elastic|panther|cribl|datadog|axiom|s3|gcs;typed fields only;secret=replace · Ctrl+S edits secret; Azure/private/Vector forms are unavailable"
        }
        crate::action::ActionId::AdminNetworkLogsSettings => "on or off",
        crate::action::ActionId::ActivityFlowsSelectWindow => {
            "start=<RFC3339 UTC>;end=<RFC3339 UTC> · IDs/labels, addresses, protocol, class, ports, min-bytes · inclusive, max 24h, within 30-day retention"
        }
        crate::action::ActionId::AccessExplorerAsk => {
            "source=<selector>;destination=<selector>;port=<number or protocol>;policy=current|candidate"
        }
        crate::action::ActionId::AdminPolicyPreview => {
            "type=user|ipport;previewFor=<server-supported user or ip:port selector>"
        }
        crate::action::ActionId::AuditFilterTime => {
            "inclusive UTC RFC3339 values: start=2026-08-03T00:00:00Z;end=2026-08-04T00:00:00Z (empty clears each bound)"
        }
        crate::action::ActionId::AuditFilterActor => {
            "exact fields: id=user-or-principal-id;display=resolved display value"
        }
        crate::action::ActionId::AuditFilterAction => "exact action value, such as device.view",
        crate::action::ActionId::AuditFilterTarget => {
            "exact fields: type=device|user|route|dns|credential|policy;id=stable-id;text=summary search"
        }
        _ => "enter a typed local operator request",
    };
    let preference_status = if state.action_id == crate::action::ActionId::LocalPreferencesEdit {
        format!(
            "\n\nfields:\n{}",
            [
                preference_status("accept-dns", &app.local_preferences.accept_dns),
                preference_status("accept-routes", &app.local_preferences.accept_routes),
                preference_status("shields-up", &app.local_preferences.shields_up),
                preference_status("ssh", &app.local_preferences.ssh),
                preference_status("auto-update", &app.local_preferences.automatic_update),
                preference_status("update-check", &app.local_preferences.update_check),
                preference_status("report-posture", &app.local_preferences.report_posture),
                preference_status("hostname", &app.local_preferences.hostname),
                preference_status("nickname", &app.local_preferences.nickname),
                preference_status("webclient", &app.local_preferences.web_client),
            ]
            .join("\n")
        )
    } else {
        String::new()
    };
    let candidates = if state.action_id == crate::action::ActionId::LocalExitNodeSelect {
        let values = app
            .exit_node_candidates()
            .iter()
            .map(|candidate| {
                format!(
                    "{} {} · {} · {} · {}",
                    if candidate.selected { "*" } else { " " },
                    candidate.display_name,
                    candidate.device_id,
                    if candidate.online == Some(true) {
                        "online"
                    } else if candidate.online == Some(false) {
                        "offline"
                    } else {
                        "unknown"
                    },
                    candidate
                        .last_probe_ms
                        .map_or("not probed".to_owned(), |value| format!("{value}ms"))
                )
            })
            .collect::<Vec<_>>();
        if values.is_empty() {
            "\n\ncandidates: not returned".to_owned()
        } else {
            format!("\n\ncandidates:\n{}", values.join("\n"))
        }
    } else {
        String::new()
    };
    let ordered = state.ordered_items.as_ref().map_or_else(
        String::new,
        |items| {
            let entries = if items.is_empty() {
                "  (empty)".to_owned()
            } else {
                items
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        format!(
                            "{} {}",
                            if index == state.ordered_selected { ">" } else { " " },
                            if value.is_empty() { "(empty)" } else { value }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            format!(
                "\n\nordered entries:\n{entries}\nitem editor: {}\nUp/Down select · Ctrl+Up/Ctrl+Down move · Ctrl+i insert · Ctrl+x remove",
                state.ordered_editor
            )
        },
    );
    let input = if state.secret_editing {
        state
            .secret_input
            .as_ref()
            .map_or_else(String::new, |value| {
                "•".repeat(value.as_str().chars().count())
            })
    } else {
        state.input.clone()
    };
    let secret_hint = state.secret_input.as_ref().map_or(String::new(), |_| {
        format!(
            "\nwrite-only secret: {} · Ctrl+S toggles secret input",
            if state.secret_editing {
                "editing"
            } else {
                "unchanged"
            }
        )
    });
    let error = state
        .error
        .as_deref()
        .map_or(String::new(), |value| format!("\nerror: {value}"));
    let title = if state.ordered_items.is_some() {
        "admin ordered form"
    } else {
        "operator form"
    };
    frame.render_widget(
        Paragraph::new(format!(
            "{hint}{preference_status}{candidates}{ordered}{secret_hint}\n\nreplacement: {}{}\nEnter previews   Esc cancels",
            input, error
        ))
        .style(app.theme.style(theme::StyleRole::SurfaceRaised))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(app.theme.style(theme::StyleRole::BorderFocused))
                .title(title),
        ),
        area,
    );
}

fn preference_status<T: std::fmt::Display>(
    name: &str,
    preference: &crate::domain::preference::ObservedPreference<T>,
) -> String {
    format!(
        "  {name}: {} · {}",
        preference
            .value
            .as_ref()
            .map_or_else(|| "not returned".to_owned(), ToString::to_string),
        preference.editability.label()
    )
}

/// A field-by-field form. Every row states what it wants in words, the selected
/// row explains itself, and nothing asks for something already on screen.
pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect, state: &FormState) {
    let label_width = state
        .fields
        .iter()
        .map(|field| field.label.chars().count())
        .chain(state.subject.iter().map(|(label, _)| label.chars().count()))
        .max()
        .unwrap_or(0)
        .max(10);
    let mut lines = Vec::new();
    // What the form acts on, stated rather than asked for.
    for (label, value) in &state.subject {
        lines.push(Line::from(vec![
            Span::styled(
                text::pad_or_trim(label, label_width.saturating_add(2)),
                app.theme.style(theme::StyleRole::TextMuted),
            ),
            Span::styled(
                value.clone(),
                app.theme.style(theme::StyleRole::TextPrimary),
            ),
        ]));
    }
    if !state.subject.is_empty() {
        lines.push(Line::default());
    }
    for (index, field) in state.fields.iter().enumerate() {
        let selected = index == state.selected;
        let editing = selected && state.is_editing();
        let (value, value_role) = match (&field.kind, field.value.as_str()) {
            (FieldKind::Text { hint }, "") => ((*hint).to_owned(), theme::StyleRole::TextDisabled),
            (_, value) => (value.to_owned(), theme::StyleRole::TextPrimary),
        };
        let mut spans = vec![
            Span::styled(
                if selected { "> " } else { "  " }.to_owned(),
                app.theme.style(theme::StyleRole::KeyHint),
            ),
            Span::styled(
                text::pad_or_trim(field.label, label_width),
                app.theme.style(if selected {
                    theme::StyleRole::Focus
                } else {
                    theme::StyleRole::TextMuted
                }),
            ),
            Span::styled("  ", app.theme.style(theme::StyleRole::SurfaceRaised)),
            Span::styled(
                value,
                if editing {
                    app.theme.style(theme::StyleRole::Focus)
                } else {
                    app.theme.style(value_role)
                },
            ),
        ];
        // A caret only where typing does something: inside an open text field.
        if editing && matches!(field.kind, FieldKind::Text { .. }) {
            spans.push(Span::styled(
                "\u{2588}",
                app.theme.style(theme::StyleRole::Focus),
            ));
        }
        if editing && !matches!(field.kind, FieldKind::Text { .. }) {
            spans.push(Span::styled(
                "  \u{2039} \u{203a}",
                app.theme.style(theme::StyleRole::KeyHint),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled(
            if state.on_submit_row() { "> " } else { "  " }.to_owned(),
            app.theme.style(theme::StyleRole::KeyHint),
        ),
        Span::styled(
            "Continue",
            app.theme.style(if state.on_submit_row() {
                theme::StyleRole::Focus
            } else {
                theme::StyleRole::TextMuted
            }),
        ),
    ]));
    lines.push(Line::default());
    let help = state
        .fields
        .get(state.selected)
        .map_or("Review the change before anything happens", |field| {
            field.help
        });
    lines.push(Line::from(Span::styled(
        help.to_owned(),
        app.theme.style(theme::StyleRole::TextMuted),
    )));
    if let Some(error) = state.error.as_deref() {
        lines.push(Line::from(Span::styled(
            error.to_owned(),
            app.theme.style(theme::StyleRole::StateDanger),
        )));
    }
    lines.push(Line::default());
    lines.push(hints(app, state));
    frame.render_widget(
        Paragraph::new(lines)
            .style(app.theme.style(theme::StyleRole::SurfaceRaised))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(app.theme.style(theme::StyleRole::BorderFocused))
                    .title(format!(" {} ", state.title)),
            ),
        area,
    );
}

/// Only the keys that do something on the field the user is standing on.
fn hints(app: &App, state: &FormState) -> Line<'static> {
    let pairs = if state.is_editing() {
        match state.fields.get(state.selected).map(|field| &field.kind) {
            Some(FieldKind::Text { .. }) => vec![("Enter", "keep"), ("Esc", "discard")],
            _ => vec![("←/→", "change"), ("Enter", "keep"), ("Esc", "discard")],
        }
    } else if state.on_submit_row() {
        vec![("j/k", "move"), ("Enter", "review"), ("Esc", "cancel")]
    } else {
        vec![("j/k", "move"), ("Enter", "edit"), ("Esc", "cancel")]
    };
    let mut spans = Vec::new();
    for (index, (key, label)) in pairs.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                "   ",
                app.theme.style(theme::StyleRole::SurfaceRaised),
            ));
        }
        spans.push(Span::styled(
            key,
            app.theme.style(theme::StyleRole::KeyHint),
        ));
        spans.push(Span::styled(
            " ",
            app.theme.style(theme::StyleRole::SurfaceRaised),
        ));
        spans.push(Span::styled(
            label,
            app.theme.style(theme::StyleRole::TextMuted),
        ));
    }
    Line::from(spans)
}
