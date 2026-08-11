use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use tale::action::ActionId;
use tale::app::{App, FormState, Overlay, SourceMode};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::device::DeviceId;
use tale::domain::mutation::{LocalMutation, MutationResult};
use tale::domain::preference::{PreferenceEditability, PreferenceRequest};
use tale::domain::route::{
    AdvertisementRequest, ExitNodeRequest, ExitNodeSelection, parse_route_set,
    parse_static_endpoints,
};
use tale::domain::source::{
    ExecutableSource, LocalCapabilities, LocalDaemonState, LocalExecutable,
};
use tale::effect::Effect;
use tale::event::{Event, InputEvent, LocalEvent};
use tale::local::accounts::decode_accounts;
use tale::local::client::{
    advertisement_command, down_command, exit_node_command, set_command, up_command,
};
use tale::local::daemon::decode_preferences;
use tale::local::policy::{SystemPolicyEntry, decode_policy};
use tale::paths::{PathEnvironment, Platform};

const STATUS: &str = include_str!("fixtures/tailscale/1.98.9/linux/status.json");
const PREFS: &[u8] = include_bytes!("fixtures/tailscale/1.98.9/linux/prefs.json");
const ACCOUNTS: &str = include_str!("fixtures/tailscale/1.98.9/linux/accounts.json");
const SYSPOLICY: &str = include_str!("fixtures/tailscale/1.98.9/linux/syspolicy.json");

fn app(read_only: bool) -> Option<App> {
    let root = PathBuf::from("/fictional/tale-local-operator");
    let cli = Cli {
        command: None,
        profile: None,
        config: Some(root.join("missing.toml")),
        view: None,
        read_only,
        no_local: false,
        tailscale_path: None,
        tailscale_socket: None,
        mock: false,
    };
    let environment = EnvironmentValues {
        config_file: None,
        tailscale_path: None,
        tailscale_socket: None,
        no_color: false,
    };
    let paths = PathEnvironment {
        platform: Platform::Unix,
        current_dir: root.clone(),
        xdg_config_home: Some(root.join("config")),
        home: Some(root.join("home")),
        xdg_state_home: Some(root.join("state")),
        xdg_cache_home: Some(root.join("cache")),
        appdata: None,
        localappdata: None,
    };
    config::resolve(&cli, &environment, &paths)
        .ok()
        .map(App::new)
}

fn prepared_app() -> Option<App> {
    let mut app = app(false)?;
    app.source_mode = SourceMode::Local;
    let executable = LocalExecutable {
        path: PathBuf::from("/bin/tailscale"),
        socket_path: None,
        source: ExecutableSource::Cli,
        version: "1.98.9".to_owned(),
        daemon_version: Some("1.98.9".to_owned()),
        build: None,
        capabilities: LocalCapabilities::all_supported(),
    };
    app.local_executable = Some(executable);
    app.local_capabilities = LocalCapabilities::all_supported();
    let snapshot = tale::local::daemon::decode_status(
        STATUS,
        "1.98.9".to_owned(),
        Some("1.98.9".to_owned()),
        1_754_000_000,
    )
    .ok()?;
    let _ = app.update(Event::Local(Box::new(LocalEvent::StatusSucceeded {
        generation: 1,
        snapshot: Box::new(snapshot),
    })));
    app.local_daemon_state = LocalDaemonState::Live;
    app.local_preferences = decode_preferences(PREFS, 1_754_000_000).ok()?;
    Some(app)
}

#[test]
fn operator_commands_are_exact_and_keep_complete_sets_typed() {
    let path = std::path::Path::new("tailscale");
    assert_eq!(up_command(path, Duration::from_secs(1)).args.len(), 1);
    assert_eq!(
        up_command(path, Duration::from_secs(1)).args[0],
        std::ffi::OsString::from("up")
    );
    assert_eq!(
        down_command(path, Duration::from_secs(1), true).args,
        vec![
            std::ffi::OsString::from("down"),
            std::ffi::OsString::from("--accept-risk=lose-ssh")
        ]
    );

    let request = PreferenceRequest {
        accept_dns: Some(false),
        hostname: Some("host with spaces".to_owned()),
        web_client: Some(true),
        ..PreferenceRequest::default()
    };
    let command = set_command(path, Duration::from_secs(1), &request);
    assert!(command.is_ok());
    if let Ok(command) = command {
        assert_eq!(
            command.args,
            vec![
                OsString::from("set"),
                OsString::from("--accept-dns=false"),
                OsString::from("--hostname=host with spaces"),
                OsString::from("--webclient=true"),
            ]
        );
    }

    let exit = exit_node_command(
        path,
        Duration::from_secs(1),
        &ExitNodeRequest {
            selection: ExitNodeSelection::AutoAny,
            allow_lan_access: true,
        },
    );
    assert_eq!(
        exit.args,
        vec![
            OsString::from("set"),
            OsString::from("--exit-node=auto:any"),
            OsString::from("--exit-node-allow-lan-access=true"),
        ]
    );

    let routes = parse_route_set("10.20.2.3/16,2001:db8::1/64");
    let endpoints = parse_static_endpoints("[2001:db8::1]:443,203.0.113.10:80");
    assert!(routes.is_ok());
    assert!(endpoints.is_ok());
    if let (Ok(routes), Ok(endpoints)) = (routes, endpoints) {
        let missing_risk = advertisement_command(
            path,
            Duration::from_secs(1),
            &AdvertisementRequest {
                advertise_connector: Some(true),
                ..AdvertisementRequest::default()
            },
        );
        assert!(missing_risk.is_err());
        let advertisement = advertisement_command(
            path,
            Duration::from_secs(1),
            &AdvertisementRequest {
                routes: Some(routes),
                advertise_connector: Some(true),
                relay_server_port: Some(Some(0)),
                relay_server_static_endpoints: Some(endpoints),
                accept_mac_app_connector_risk: true,
                ..AdvertisementRequest::default()
            },
        );
        assert!(advertisement.is_ok());
        if let Ok(advertisement) = advertisement {
            assert_eq!(
                advertisement.args,
                vec![
                    OsString::from("set"),
                    OsString::from("--advertise-routes=10.20.0.0/16,2001:db8::/64"),
                    OsString::from("--advertise-connector=true"),
                    OsString::from("--accept-risk=mac-app-connector"),
                    OsString::from("--relay-server-port=0"),
                    OsString::from(
                        "--relay-server-static-endpoints=203.0.113.10:80,[2001:db8::1]:443",
                    ),
                ]
            );
        }
    }
}

#[test]
fn versioned_account_and_policy_fixtures_decode_effective_state_and_errors() {
    let accounts = decode_accounts(ACCOUNTS);
    assert!(accounts.is_ok());
    if let Ok(accounts) = accounts {
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].profile_name.as_deref(), Some("work"));
        assert!(accounts[0].active);
    }

    let policy = decode_policy(SYSPOLICY);
    assert!(policy.is_ok());
    if let Ok(policy) = policy {
        assert_eq!(policy.len(), 3);
        assert!(policy.iter().any(|entry| {
            entry.name == "fixture-error"
                && entry.error.as_deref() == Some("unsupported policy value type")
        }));
    }
}

#[test]
fn read_only_mode_is_rechecked_when_confirmation_dispatches() {
    let prepared = prepared_app();
    assert!(prepared.is_some());
    if let Some(mut app) = prepared {
        let opened = app.dispatch_action(ActionId::LocalConnect);
        assert!(opened.is_empty());
        assert!(matches!(
            app.overlays.last(),
            Some(Overlay::Confirmation(_))
        ));
        app.resolved_config.read_only = true;
        let effects = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(effects.is_empty());
        assert_eq!(app.mutation_in_flight, None);
    }
}

#[test]
fn policy_managed_preferences_remain_visible_but_cannot_dispatch() {
    let prepared = prepared_app();
    assert!(prepared.is_some());
    if let Some(mut app) = prepared {
        app.views.devices.selected_id = Some(DeviceId::new("nodekey:selffixture"));
        let _ = app.update(Event::Local(Box::new(LocalEvent::PolicySucceeded {
            entries: vec![SystemPolicyEntry {
                name: "UseTailscaleDNSSettings".to_owned(),
                source: Some("mdm (Device)".to_owned()),
                value: Some("always".to_owned()),
                error: None,
            }],
        })));
        assert_eq!(
            app.local_preferences.accept_dns.editability,
            PreferenceEditability::PolicyManaged
        );
        let _ = app.dispatch_action(ActionId::LocalPreferencesEdit);
        // The field is on the form, holding what the daemon reports, but the
        // form says who decides it instead of opening an editor.
        let locked = match app.overlays.last() {
            Some(Overlay::Form(state)) => state
                .fields
                .iter()
                .find(|field| field.key == "accept-dns")
                .and_then(|field| field.locked.clone()),
            _ => None,
        };
        assert!(locked.is_some());
        let effects = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(effects.is_empty());
        let reported = match app.overlays.last() {
            Some(Overlay::Form(state)) => state.error.clone(),
            _ => None,
        };
        assert_eq!(reported, locked);
    }
}

#[test]
fn repeated_confirmation_dispatches_only_one_mutation() {
    let prepared = prepared_app();
    assert!(prepared.is_some());
    if let Some(mut app) = prepared {
        let _ = app.dispatch_action(ActionId::LocalConnect);
        let first = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert_eq!(
            first
                .iter()
                .filter(|effect| matches!(effect, Effect::StartLocalMutation { .. }))
                .count(),
            1
        );
        let second = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(second.is_empty());
    }
}

#[test]
fn account_change_clears_old_selection_and_needs_login_opens_login_choice() {
    let prepared = prepared_app();
    assert!(prepared.is_some());
    if let Some(mut app) = prepared {
        app.views.devices.selected_id = Some(DeviceId::new("nodekey:direct"));
        assert!(app.mutation_lock.hold(21));
        app.mutation_in_flight = Some(21);
        let snapshot = app.local_resource.snapshot.clone();
        let _ = app.update(Event::Local(Box::new(LocalEvent::MutationFinished {
            mutation_id: 21,
            task_id: tale::task::TaskId(999),
            action_id: ActionId::LocalAccountSwitch,
            mutation: LocalMutation::AccountSwitch {
                account_id: "profile-personal".to_owned(),
            },
            result: MutationResult::Verified {
                summary: "verified".to_owned(),
                detail: "fixture".to_owned(),
                exit_status: Some(0),
            },
            snapshot: snapshot.map(Box::new),
            preferences: None,
            accounts: None,
            policy: None,
        })));
        assert_ne!(
            app.views.devices.selected_id,
            Some(DeviceId::new("nodekey:direct"))
        );

        assert!(app.mutation_lock.hold(22));
        app.mutation_in_flight = Some(22);
        let needs_login = tale::local::daemon::decode_status(
            r#"{"BackendState":"NeedsLogin","Self":{"ID":"nodekey:self"}}"#,
            "1.98.9".to_owned(),
            None,
            1_754_000_001,
        );
        assert!(needs_login.is_ok());
        if let Ok(needs_login) = needs_login {
            let _ = app.update(Event::Local(Box::new(LocalEvent::MutationFinished {
                mutation_id: 22,
                task_id: tale::task::TaskId(1000),
                action_id: ActionId::LocalConnect,
                mutation: LocalMutation::Connect,
                result: MutationResult::VerificationMismatch {
                    summary: "login required".to_owned(),
                    detail: "fresh state requires login".to_owned(),
                    exit_status: Some(0),
                },
                snapshot: Some(Box::new(needs_login)),
                preferences: None,
                accounts: None,
                policy: None,
            })));
            assert!(matches!(
                app.overlays.last(),
                Some(Overlay::Confirmation(state))
                    if state.action_id == ActionId::LocalAccountLogin
            ));
        }
    }
}

fn press_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        code, modifiers,
    ))));
}

fn render_form(app: &App) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let Ok(mut terminal) = Terminal::new(TestBackend::new(100, 30));
    if terminal.draw(|frame| tale::ui::render(frame, app)).is_err() {
        return String::new();
    }
    let buffer = terminal.backend().buffer();
    (0..30)
        .map(|y| {
            (0..100)
                .filter_map(|x| buffer.cell((x, y)).map(|cell| cell.symbol().to_owned()))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn form(app: &App) -> Option<&tale::app::FormState> {
    match app.overlays.last() {
        Some(Overlay::Form(state)) => Some(state),
        _ => None,
    }
}

/// Local operator actions used to be one typed line of `field=value` pairs with
/// the grammar printed above it. Each one now names its fields and holds what
/// the daemon reports, so a change is a change to something already on screen.
#[test]
fn local_operator_forms_ask_field_by_field() {
    let prepared = prepared_app();
    assert!(prepared.is_some());
    if let Some(mut app) = prepared {
        let _ = app.dispatch_action(ActionId::LocalPreferencesEdit);
        let keys = form(&app).map(|state| {
            state
                .fields
                .iter()
                .map(|field| field.key)
                .collect::<Vec<_>>()
        });
        assert_eq!(
            keys,
            Some(vec![
                "accept-dns",
                "accept-routes",
                "shields-up",
                "ssh",
                "auto-update",
                "update-check",
                "report-posture",
                "webclient",
                "hostname",
                "nickname",
            ])
        );
        // Nothing is changed until a field is answered, and the form says so
        // rather than sending a mutation that changes nothing.
        let rows = form(&app).map_or(0, |state| state.fields.len());
        for _ in 0..rows {
            press_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
        }
        assert!(form(&app).is_some_and(|state| state.on_submit_row()));
        let effects = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))));
        assert!(effects.is_empty());
        assert_eq!(
            form(&app).and_then(|state| state.error.clone()),
            Some("no preference was changed".to_owned())
        );
    }
}

/// A hint says what a field would hold. Once the field is open it is showing
/// what the user typed, and an empty one shows nothing rather than words they
/// did not write and would have to delete first.
#[test]
fn opening_a_text_field_clears_its_placeholder() {
    let prepared = prepared_app();
    assert!(prepared.is_some());
    if let Some(mut app) = prepared {
        let _ = app.dispatch_action(ActionId::LocalSshOpen);
        let hint = match form(&app).and_then(FormState::selected_field) {
            Some(field) => match &field.kind {
                tale::app::FieldKind::Text { hint } => Some((*hint).to_owned()),
                _ => None,
            },
            None => None,
        };
        assert_eq!(hint.as_deref(), Some("remote default"));
        let rendered = render_form(&app);
        assert!(rendered.contains("remote default"), "hint is not shown");

        press_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let rendered = render_form(&app);
        assert!(
            !rendered.contains("remote default"),
            "the placeholder survived the field being opened"
        );
        // The field is still empty: the hint went, the value did not arrive.
        assert_eq!(form(&app).map(|state| state.value("user")), Some(""));
    }
}

/// A set that has an order is edited entry by entry, so it can be reordered
/// without spelling the whole set out again. Every binding is one the terminal
/// actually sends: Ctrl+I and Tab are the same byte, and Ctrl with an arrow is
/// not encoded at all under the escape sequences this app asks for.
#[test]
fn list_fields_are_edited_and_reordered_in_place() {
    let prepared = prepared_app();
    assert!(prepared.is_some());
    if let Some(mut app) = prepared {
        let _ = app.dispatch_action(ActionId::LocalRoutesEditAdvertisements);
        // Open the routes field and start from an empty set.
        press_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        for _ in 0..8 {
            press_key(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        }
        assert!(
            form(&app)
                .and_then(|state| state.list.as_ref())
                .is_some_and(|list| list.entries.is_empty())
        );
        press_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        let _ = app.update(Event::Input(InputEvent::Paste("10.0.0.0/8".to_owned())));
        press_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        let _ = app.update(Event::Input(InputEvent::Paste("192.168.0.0/16".to_owned())));
        let before = form(&app)
            .and_then(|state| state.list.as_ref())
            .map(|list| list.entries.clone())
            .unwrap_or_default();
        assert_eq!(
            before,
            vec!["10.0.0.0/8".to_owned(), "192.168.0.0/16".to_owned()]
        );
        // The last entry moves above the one before it, in place.
        press_key(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
        press_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let after = form(&app)
            .map(|state| state.entries("routes"))
            .unwrap_or_default();
        assert_eq!(
            after,
            vec!["192.168.0.0/16".to_owned(), "10.0.0.0/8".to_owned()]
        );
    }
}

/// A write-only secret is held once, zeroized, and never reaches the field the
/// screen can draw. The row still shows that something was entered, because a
/// field that looks empty after typing reads as a field that did not take it.
#[test]
fn secret_fields_show_their_length_and_never_hold_their_value() {
    let prepared = prepared_app();
    assert!(prepared.is_some());
    if let Some(mut app) = prepared {
        app.overlays.push(Overlay::Form(FormState {
            action_id: ActionId::AdminLogStreamReplace,
            title: "Replace a log stream",
            subject: Vec::new(),
            fields: vec![tale::app::FormField::secret(
                "secret",
                "Secret",
                "The write-only token",
            )],
            selected: 0,
            cursor: 0,
            draft: None,
            list: None,
            secret: None,
            error: None,
        }));
        press_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let _ = app.update(Event::Input(InputEvent::Paste("hunter2".to_owned())));
        assert_eq!(form(&app).map(FormState::secret_length), Some(7));
        // The field the renderer reads never held it, so it cannot be drawn.
        assert_eq!(form(&app).map(|state| state.value("secret")), Some(""));
        let rendered = render_form(&app);
        assert!(
            !rendered.contains("hunter2"),
            "the secret reached the screen"
        );
        assert!(
            rendered.contains(&"\u{2022}".repeat(7)),
            "the row does not show that a secret was entered"
        );
    }
}

#[test]
fn text_fields_edit_at_the_cursor_and_jump_between_words() {
    let prepared = prepared_app();
    assert!(prepared.is_some());
    if let Some(mut app) = prepared {
        app.overlays.push(Overlay::Form(FormState {
            action_id: ActionId::AdminLogStreamReplace,
            title: "Edit text",
            subject: Vec::new(),
            fields: vec![tale::app::FormField::text(
                "text",
                "Text",
                "Editable text",
                "",
                "alpha gamma",
            )],
            selected: 0,
            cursor: 0,
            draft: None,
            list: None,
            secret: None,
            error: None,
        }));

        press_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press_key(&mut app, KeyCode::Left, KeyModifiers::ALT);
        press_key(&mut app, KeyCode::Char('b'), KeyModifiers::NONE);

        assert_eq!(
            form(&app).map(|state| state.value("text")),
            Some("alpha bgamma")
        );
    }
}
