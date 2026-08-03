use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{AccountPickerState, App, HandoffInputState, OperatorFormState, ServiceFormState};
use crate::ui::theme;

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
            "{hint}{preference_status}{candidates}{ordered}\n\nreplacement: {}{}\nEnter previews   Esc cancels",
            state.input, error
        ))
        .style(theme::normal(app))
        .block(Block::default().borders(Borders::ALL).title(title)),
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

pub fn render_accounts(frame: &mut Frame<'_>, app: &App, area: Rect, state: &AccountPickerState) {
    let lines = state
        .accounts
        .iter()
        .enumerate()
        .map(|(index, account)| {
            format!(
                "{} {}{} · {}",
                if index == state.selected { ">" } else { " " },
                account.display_label(),
                if account.active { " (active)" } else { "" },
                account.id
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    frame.render_widget(
        Paragraph::new(format!(
            "{lines}\n\nj/k select   Enter preview   Esc cancels"
        ))
        .style(theme::normal(app))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("local accounts"),
        ),
        area,
    );
}

pub fn render_handoff(frame: &mut Frame<'_>, app: &App, area: Rect, state: &HandoffInputState) {
    let label = match state.kind {
        crate::app::HandoffInputKind::Ssh => "optional SSH username",
        crate::app::HandoffInputKind::Nc => "netcat port 1-65535",
    };
    let error = state
        .error
        .as_deref()
        .map_or(String::new(), |value| format!("\nerror: {value}"));
    frame.render_widget(
        Paragraph::new(format!(
            "host: {}\n{}\n> {}{}\nEnter previews   Esc cancels",
            state.host, label, state.input, error
        ))
        .style(theme::normal(app))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("terminal handoff"),
        ),
        area,
    );
}

pub fn render_service(frame: &mut Frame<'_>, app: &App, area: Rect, state: &ServiceFormState) {
    let hint = match state.action_id {
        crate::action::ActionId::ServicesServeCreate
        | crate::action::ActionId::ServicesServeEdit
        | crate::action::ActionId::ServicesFunnelCreate
        | crate::action::ActionId::ServicesFunnelEdit => {
            "listener=https|http|tcp|tls-terminated-tcp;port=443;path=/;backend=3000|http://...|/absolute/path;proxy=none|1|2"
        }
        crate::action::ActionId::ServicesTaildropSend => {
            "target=<exact discovered target>;files=/path/one|/path with spaces/two"
        }
        crate::action::ActionId::ServicesTaildropReceive => {
            "directory=/existing/dir;conflict=skip|overwrite|rename;wait=true|false"
        }
        crate::action::ActionId::ServicesDriveShare => "name=<name>;path=/existing/directory",
        crate::action::ActionId::ServicesDriveRename => "old=<current name>;new=<new name>",
        crate::action::ActionId::ServicesCertificateObtain => {
            "domain=<eligible local domain>;cert=/explicit/cert.pem;key=/explicit/key.pem;min-validity=30d"
        }
        crate::action::ActionId::ServicesBugReportCreate => "diagnose=true|false;note=<plain text>",
        _ => "enter a typed local service request",
    };
    let error = state
        .error
        .as_deref()
        .map_or(String::new(), |value| format!("\nerror: {value}"));
    frame.render_widget(
        Paragraph::new(format!(
            "{hint}\n\n> {}{}\nEnter previews   Esc cancels",
            state.input, error
        ))
        .style(theme::normal(app))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("local service form"),
        ),
        area,
    );
}
