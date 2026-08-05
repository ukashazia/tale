use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use tale::action::{self, ActionContext, ActionId, Binding};
use tale::app::{App, Route};
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
        access_token_present: false,
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

#[test]
fn every_required_action_is_registered() {
    let registered: Vec<_> = action::phase_one_actions()
        .into_iter()
        .map(|spec| spec.id)
        .collect();
    for id in [
        ActionId::AppQuit,
        ActionId::ViewCommandPalette,
        ActionId::ViewFilter,
        ActionId::ViewRefresh,
        ActionId::ViewRefreshAll,
        ActionId::ViewHelp,
        ActionId::ViewTasks,
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
fn every_phase_four_action_is_registered_with_required_risk_metadata() {
    let registered: Vec<_> = action::phase_four_actions()
        .into_iter()
        .map(|spec| (spec.id, spec.risk))
        .collect();
    for id in [
        ActionId::ViewServices,
        ActionId::ServicesSectionNext,
        ActionId::ServicesSectionPrevious,
        ActionId::ServicesServeRefresh,
        ActionId::ServicesServeCreate,
        ActionId::ServicesServeEdit,
        ActionId::ServicesServeReset,
        ActionId::ServicesFunnelRefresh,
        ActionId::ServicesFunnelCreate,
        ActionId::ServicesFunnelEdit,
        ActionId::ServicesFunnelReset,
        ActionId::ServicesTaildropSend,
        ActionId::ServicesTaildropReceive,
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
        ActionContext::Activity,
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
        Some(ActionId::ViewCommandPalette)
    );
    let footer = action::footer_hints(ActionContext::Collection, 20);
    assert!(footer.last().is_some_and(|hint| hint == "? more"));

    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.route_stack = vec![Route::Devices];
        let _ = app.dispatch_action(ActionId::ResourceActions);
        assert!(app.tasks.all().is_empty());
        assert!(app.runtime_error.is_some());
        let _ = app.update(Event::Source(SourceEvent::LoadSucceeded {
            generation: 1,
            devices: mock::devices(),
            observed_at: mock::MOCK_NOW,
        }));
        let _ = app.dispatch_action(ActionId::ResourceActions);
        assert!(matches!(
            app.overlays.last(),
            Some(tale::app::Overlay::ActionPicker(_))
        ));
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
    assert_eq!(Binding::Ctrl('d').label(), "Ctrl+d");
    assert_eq!(Binding::Enter.label(), "Enter");
}
