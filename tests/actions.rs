use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use tale::action::{self, ActionContext, ActionId, Binding};
use tale::app::{App, InteractionMode, Route};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::event::{Event, InputEvent, SourceEvent};
use tale::mock;
use tale::paths::{PathEnvironment, Platform};

fn mock_app() -> Option<App> {
    let root = std::path::PathBuf::from("/fictional/tale-actions");
    let cli = Cli {
        command: None,
        profile: None,
        config: Some(root.join("missing.toml")),
        view: None,
        read_only: false,
        no_local: false,
        tailscale_path: None,
        tailscale_socket: None,
        mock: true,
    };
    let environment = EnvironmentValues {
        config_file: None,
        profile: None,
        tailscale_path: None,
        tailscale_socket: None,
        no_color: false,
    };
    let path_environment = PathEnvironment {
        platform: Platform::Unix,
        current_dir: root.clone(),
        xdg_config_home: Some(root.join("config")),
        home: Some(root.join("home")),
        xdg_state_home: Some(root.join("state")),
        xdg_cache_home: Some(root.join("cache")),
        appdata: None,
        localappdata: None,
    };
    config::resolve(&cli, &environment, &path_environment)
        .ok()
        .map(App::new)
}

/// The same app the screenshots come from: a local client, and an admin
/// profile alongside it.
fn local_app(with_admin_profile: bool) -> Option<App> {
    let root = std::env::temp_dir().join(format!(
        "tale-actions-{}-{with_admin_profile}",
        std::process::id()
    ));
    if std::fs::create_dir_all(&root).is_err() {
        return None;
    }
    let config_path = root.join("config.toml");
    if with_admin_profile
        && std::fs::write(
            &config_path,
            "default_profile = \"audit\"\n[profiles.audit]\ntailnet = \"example.test\"\ncredential = \"audit\"\n",
        )
        .is_err()
    {
        return None;
    }
    let cli = Cli {
        command: None,
        profile: None,
        config: Some(if with_admin_profile {
            config_path
        } else {
            root.join("missing.toml")
        }),
        view: None,
        read_only: false,
        no_local: false,
        tailscale_path: Some(std::path::PathBuf::from("tailscale")),
        tailscale_socket: None,
        mock: false,
    };
    let environment = EnvironmentValues {
        config_file: None,
        profile: None,
        tailscale_path: None,
        tailscale_socket: None,
        no_color: true,
    };
    let path_environment = PathEnvironment {
        platform: Platform::Unix,
        current_dir: root.clone(),
        xdg_config_home: Some(root.join("config")),
        home: Some(root.join("home")),
        xdg_state_home: Some(root.join("state")),
        xdg_cache_home: Some(root.join("cache")),
        appdata: None,
        localappdata: None,
    };
    config::resolve(&cli, &environment, &path_environment)
        .ok()
        .map(App::new)
}

/// An action is offered where its subject is on screen. The local client's
/// verbs were one list handed to every route that had none of its own, so
/// `:credentials` offered `remove local account` — a key acting on something
/// the route does not show.
#[test]
fn local_actions_are_offered_only_where_their_subject_is() {
    let app = local_app(false);
    assert!(app.is_some());
    let Some(mut app) = app else {
        return;
    };

    // A route about neither the machine nor a device gets neither set.
    for route in [Route::Credentials, Route::Access, Route::Settings] {
        app.set_route(route);
        let actions = app.contextual_actions();
        for id in [
            ActionId::LocalConnect,
            ActionId::LocalAccountRemove,
            ActionId::LocalSshOpen,
            ActionId::LocalProbeConnection,
            ActionId::DiagnosticCopy,
        ] {
            assert!(
                !actions.contains(&id),
                "{route:?} still offers {id:?}, which acts on something it does not show"
            );
        }
    }

    // The machine's own route keeps the machine's verbs.
    app.set_route(Route::Local);
    let local = app.contextual_actions();
    for id in [
        ActionId::LocalConnect,
        ActionId::LocalDisconnect,
        ActionId::LocalPreferencesEdit,
        ActionId::LocalAccountSwitch,
        ActionId::LocalAccountRemove,
        ActionId::LocalSyspolicyReload,
    ] {
        assert!(local.contains(&id), "the local route lost {id:?}");
    }
    assert!(
        !local.contains(&ActionId::LocalSshOpen),
        "a session to the selected device is not an action on this machine"
    );

    // Everything that acts on a selected row lives where the rows are.
    app.set_route(Route::Devices);
    let devices = app.contextual_actions();
    for id in [
        ActionId::LocalProbeConnection,
        ActionId::LocalWhois,
        ActionId::LocalSshOpen,
        ActionId::LocalNcOpen,
        ActionId::DevicesTaildropSend,
        ActionId::DevicesTaildropReceive,
    ] {
        assert!(devices.contains(&id), "the devices route lost {id:?}");
    }
    assert!(!devices.contains(&ActionId::LocalConnect));

    app.set_route(Route::Diagnostics);
    assert!(app.contextual_actions().contains(&ActionId::DiagnosticCopy));
}

/// An admin profile adds the tailnet's verbs to the devices menu; it does not
/// take away the ones the local client offers on the same row. Both sets share
/// one menu, so no sequence may shadow another.
#[test]
fn the_devices_menu_carries_admin_and_local_actions_together() {
    let Some(mut app) = local_app(true) else {
        return;
    };
    // Without both halves the test proves nothing.
    assert!(app.admin.profile.is_some(), "the fixture has no profile");
    app.set_route(Route::Devices);
    let actions = app.contextual_actions();
    assert!(actions.contains(&ActionId::AdminDeviceRename));
    assert!(actions.contains(&ActionId::LocalSshOpen));
    assert!(actions.contains(&ActionId::DevicesTaildropSend));
    assert!(action::validate_transient_sequences(&actions).is_ok());
}

#[test]
fn every_required_action_is_registered() {
    let registered: Vec<_> = action::phase_one_actions()
        .into_iter()
        .map(|spec| spec.id)
        .collect();
    for id in [
        ActionId::AppQuit,
        ActionId::ViewCommandLine,
        ActionId::ViewFilter,
        ActionId::ViewRefresh,
        ActionId::ViewRefreshAll,
        ActionId::ViewHelp,
        ActionId::ViewTasks,
        ActionId::ViewHistoryBack,
        ActionId::ViewHistoryForward,
        ActionId::CollectionMoveUp,
        ActionId::CollectionMoveDown,
        ActionId::CollectionFirst,
        ActionId::CollectionLast,
        ActionId::CollectionPageUp,
        ActionId::CollectionPageDown,
        ActionId::CollectionOpen,
        ActionId::CollectionSort,
        ActionId::CollectionWideColumns,
        ActionId::ResourceActions,
        ActionId::ResourceCopy,
        ActionId::TaskCancel,
    ] {
        assert!(registered.contains(&id), "missing {}", id.as_str());
    }
}

#[test]
fn every_action_risk_maps_to_an_explicit_semantic_role() {
    for spec in action::all_actions() {
        let role = spec.risk.style_role();
        assert!(matches!(
            role,
            tale::ui::theme::StyleRole::RiskObserve
                | tale::ui::theme::StyleRole::RiskReversible
                | tale::ui::theme::StyleRole::RiskDisruptive
                | tale::ui::theme::StyleRole::RiskDestructive
        ));
        assert!(!role.signal().label.is_empty());
    }
}

#[test]
fn every_phase_four_action_is_registered_with_required_risk_metadata() {
    let registered: Vec<_> = action::phase_four_actions()
        .into_iter()
        .map(|spec| (spec.id, spec.risk))
        .collect();
    for id in [
        ActionId::ViewServices,
        ActionId::ViewDiagnostics,
        ActionId::ServicesSectionNext,
        ActionId::ServicesSectionPrevious,
        ActionId::ServicesServeRefresh,
        ActionId::ServicesServeCreate,
        ActionId::ServicesServeEdit,
        ActionId::ServicesServeRemove,
        ActionId::ServicesServeReset,
        ActionId::ServicesFunnelCreate,
        ActionId::ServicesFunnelEdit,
        ActionId::ServicesFunnelUnpublish,
        ActionId::ServicesFunnelReset,
        ActionId::DevicesTaildropSend,
        ActionId::DevicesTaildropReceive,
        ActionId::ServicesDriveRefresh,
        ActionId::ServicesDriveShare,
        ActionId::ServicesDriveRename,
        ActionId::ServicesDriveUnshare,
        ActionId::ServicesCertificateObtain,
        ActionId::ServicesMetricsRefresh,
        ActionId::ServicesBugReportCreate,
    ] {
        assert!(
            registered
                .iter()
                .any(|(registered_id, _)| *registered_id == id),
            "missing {}",
            id.as_str()
        );
    }
    assert_eq!(
        registered
            .iter()
            .find(|(id, _)| *id == ActionId::ServicesFunnelCreate)
            .map(|(_, risk)| *risk),
        Some(tale::action::Risk::Disruptive)
    );
    assert_eq!(
        registered
            .iter()
            .find(|(id, _)| *id == ActionId::ServicesServeReset)
            .map(|(_, risk)| *risk),
        Some(tale::action::Risk::Disruptive)
    );
    // Taking one mapping down is narrower than a reset but no less disruptive
    // to whoever is using it.
    for id in [
        ActionId::ServicesServeRemove,
        ActionId::ServicesFunnelUnpublish,
    ] {
        assert_eq!(
            registered
                .iter()
                .find(|(registered_id, _)| *registered_id == id)
                .map(|(_, risk)| *risk),
            Some(tale::action::Risk::Disruptive),
            "{} is not disruptive",
            id.as_str()
        );
    }
    assert_eq!(
        registered
            .iter()
            .find(|(id, _)| *id == ActionId::ServicesDriveUnshare)
            .map(|(_, risk)| *risk),
        Some(tale::action::Risk::Disruptive)
    );
}

#[test]
fn no_duplicate_active_binding_exists_in_one_context() {
    for context in [
        ActionContext::Root,
        ActionContext::Collection,
        ActionContext::Detail,
        ActionContext::Audit,
    ] {
        let actions = action::phase_one_actions();
        for (index, left) in actions.iter().enumerate() {
            if !left.contexts.contains(&context) {
                continue;
            }
            for right in actions.iter().skip(index + 1) {
                if !right.contexts.contains(&context) {
                    continue;
                }
                for left_binding in left.default_bindings {
                    for right_binding in right.default_bindings {
                        assert_ne!(
                            left_binding, right_binding,
                            "duplicate binding in {context:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn dispatch_uses_registered_bindings_and_footer_reports_more() {
    let key = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE);
    assert_eq!(
        action::action_for_key(key, ActionContext::Collection),
        Some(ActionId::ViewRefresh)
    );
    assert_eq!(
        action::action_for_key(
            KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
            ActionContext::Root
        ),
        Some(ActionId::ViewCommandLine)
    );
    let footer = action::footer_hints(ActionContext::Collection, Route::Devices, 20);
    assert!(footer.last().is_some_and(|hint| hint == "? more"));
    let footer = action::footer_hints(ActionContext::Collection, Route::Devices, 120);
    assert!(footer.first().is_some_and(|hint| hint == "k up"));
    assert!(footer.iter().any(|hint| hint == ": command"));
    assert!(footer.iter().any(|hint| hint == "i inspector"));
    assert_eq!(
        footer.iter().filter(|hint| hint.starts_with("? ")).count(),
        1
    );

    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.set_route(Route::Devices);
        let _ = app.dispatch_action(ActionId::ResourceActions);
        assert!(app.tasks.all().is_empty());
        assert!(app.runtime_error.is_some());
        let _ = app.update(Event::Source(SourceEvent::LoadSucceeded {
            generation: 1,
            devices: mock::devices(),
            observed_at: mock::MOCK_NOW,
        }));
        let _ = app.dispatch_action(ActionId::ResourceActions);
        assert!(matches!(app.interaction, InteractionMode::Transient(_)));
        let _ = app.dispatch_action(ActionId::MockSuccess);
        assert_eq!(app.tasks.all().len(), 1);
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        ))));
    }
}

#[test]
fn binding_type_labels_are_stable() {
    assert_eq!(Binding::Char(':').label(), ":");
    assert_eq!(Binding::Ctrl('d').label(), "C-d");
    assert_eq!(Binding::Enter.label(), "Enter");
}

#[test]
fn transient_sequences_and_reserved_history_bindings_are_stable() {
    let actions = [
        ActionId::LocalConnect,
        ActionId::LocalDisconnect,
        ActionId::LocalAccountSwitch,
        ActionId::DiagnosticCopy,
        ActionId::SavedViewCreate,
        ActionId::CollectionExport,
    ];
    assert!(action::validate_transient_sequences(&actions).is_ok());
    assert!(
        action::validate_transient_sequences(&[ActionId::LocalConnect, ActionId::MockCancellable])
            .is_err()
    );
    assert_eq!(
        action::transient_sequence(ActionId::LocalAccountSwitch),
        Some("as")
    );
    assert_eq!(
        action::action_for_key(
            KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE),
            ActionContext::Collection,
        ),
        Some(ActionId::ViewHistoryBack)
    );
    assert_eq!(
        action::action_for_key(
            KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE),
            ActionContext::Collection,
        ),
        Some(ActionId::ViewHistoryForward)
    );
}

#[test]
fn every_transient_action_has_an_explicit_menu_group() {
    for id in ActionId::all() {
        if action::transient_sequence(*id).is_some() {
            assert!(
                action::transient_group(*id).is_some(),
                "missing group for {}",
                id.as_str()
            );
        }
    }
}

/// Binding labels used to come from a hand-listed match that fell through to
/// the placeholder "key". A newly bound character has to name itself.
#[test]
fn every_registered_binding_names_its_own_key() {
    for spec in action::all_actions() {
        for binding in spec.default_bindings {
            let label = binding.label();
            assert_ne!(
                label, "key",
                "{:?} renders its binding as a placeholder",
                spec.id
            );
            assert_ne!(
                label, "C-key",
                "{:?} renders its binding as a placeholder",
                spec.id
            );
        }
    }
}
