use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use tale::app::{App, Overlay, Route, ShutdownState, SourceMode};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::device::{SortDirection, SortField, SortSpec};
use tale::domain::service::{
    Backend, Exposure, FunnelStatus, Listener, PathMount, Port, ProxyProtocol, ServeStatus,
    ServiceActionRequest, ServiceFailure, ServiceFailureKind, ServiceMapping, ServiceTaskData,
};
use tale::domain::source::{ExecutableSource, LocalCapabilities, LocalExecutable};
use tale::event::{Event, InputEvent, ServicesEvent, SourceEvent, TaskEvent};
use tale::mock::{self, MOCK_NOW};
use tale::paths::{PathEnvironment, Platform};
use tale::task::{Progress, TaskState};

fn mock_app() -> Option<App> {
    let root = PathBuf::from("/fictional/tale-reducer");
    let cli = Cli {
        command: None,
        profile: None,
        config: Some(root.join("missing.toml")),
        view: None,
        read_only: false,
        no_local: false,
        tailscale_path: None,
        mock: true,
    };
    let environment = EnvironmentValues {
        config_file: None,
        profile: None,
        access_token_present: false,
        tailscale_path: None,
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

fn load_app(app: &mut App) {
    let update = app.update(Event::Source(SourceEvent::LoadSucceeded {
        generation: 1,
        devices: mock::devices(),
        observed_at: MOCK_NOW,
    }));
    assert!(update.is_empty());
}

#[test]
fn bootstrap_and_source_updates_are_typed_and_deterministic() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let effects = app.bootstrap_effects();
        assert_eq!(effects.len(), 1);
        assert_eq!(app.devices_resource.generation, 1);
        load_app(&mut app);
        assert_eq!(app.source_mode, SourceMode::Mock);
        assert_eq!(app.devices_resource.snapshot.len(), 14);
        assert!(app.views.devices.selected_id.is_some());
        assert_eq!(app.devices_resource.health.label(), "healthy");
    }
}

#[test]
fn stale_generation_cannot_replace_newer_snapshot_or_metadata() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.devices_resource.generation = 2;
        load_app_generation(&mut app, 2, MOCK_NOW);
        let before = app.devices_resource.snapshot[0].id.clone();
        let before_health = app.devices_resource.health;
        let update = app.update(Event::Source(SourceEvent::LoadSucceeded {
            generation: 1,
            devices: Vec::new(),
            observed_at: MOCK_NOW.saturating_sub(500),
        }));
        assert!(update.is_empty());
        assert_eq!(app.devices_resource.snapshot[0].id, before);
        assert_eq!(app.devices_resource.health, before_health);
    }
}

fn load_app_generation(app: &mut App, generation: u64, observed_at: u64) {
    let _ = app.update(Event::Source(SourceEvent::LoadSucceeded {
        generation,
        devices: mock::devices(),
        observed_at,
    }));
}

#[test]
fn selection_is_by_device_id_across_sort_and_filter() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        load_app(&mut app);
        let selected = app.devices_resource.snapshot[4].id.clone();
        app.views.devices.selected_id = Some(selected.clone());
        app.views.devices.sort = SortSpec {
            field: SortField::Name,
            direction: SortDirection::Ascending,
        };
        let _ = app.update(Event::Tick(Instant::now()));
        assert_eq!(app.views.devices.selected_id, Some(selected.clone()));

        let parsed_filter = tale::domain::filter::parse("os:android");
        assert!(parsed_filter.is_ok());
        if let Ok(parsed_filter) = parsed_filter {
            app.views.devices.applied_filter = parsed_filter;
        }
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('r'),
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.views.devices.selected_id, Some(selected));
    }
}

#[test]
fn overlay_stack_restores_action_picker_after_help() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        load_app(&mut app);
        app.route_stack = vec![Route::Devices];
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            app.overlays.last(),
            Some(Overlay::ActionPicker(_))
        ));
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('?'),
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            app.overlays.as_slice(),
            [Overlay::ActionPicker(_), Overlay::Help(_)]
        ));
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        ))));
        assert!(matches!(
            app.overlays.as_slice(),
            [Overlay::ActionPicker(_)]
        ));
    }
}

#[test]
fn pasted_text_is_editor_only_and_never_dispatches_global_keys() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let _ = app.update(Event::Input(InputEvent::Paste("q:devices".to_owned())));
        assert!(app.overlays.is_empty());
        assert!(matches!(app.shutdown_state, ShutdownState::Running));
        app.route_stack = vec![Route::Devices];
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('/'),
            KeyModifiers::NONE,
        ))));
        let _ = app.update(Event::Input(InputEvent::Paste("online:true q".to_owned())));
        assert!(matches!(
            app.overlays.last(),
            Some(Overlay::FilterEditor(_))
        ));
        if let Some(Overlay::FilterEditor(state)) = app.overlays.last() {
            assert_eq!(state.input, "online:true q");
        }
        assert!(matches!(app.shutdown_state, ShutdownState::Running));
    }
}

#[test]
fn quit_and_ctrl_c_follow_task_rules() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        ))));
        assert!(matches!(app.shutdown_state, ShutdownState::Requested(_)));

        let app = mock_app();
        assert!(app.is_some());
        if let Some(mut app) = app {
            let task_id = app.tasks.create(
                tale::action::ActionId::MockCancellable,
                "simulation",
                MOCK_NOW,
                true,
            );
            let _ = app.update(Event::Task(Box::new(TaskEvent::Started { task_id })));
            let effects = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
            ))));
            assert_eq!(effects.len(), 1);
            assert_eq!(
                app.tasks.get(task_id).map(|task| task.state),
                Some(TaskState::Cancelling)
            );
            assert!(matches!(app.shutdown_state, ShutdownState::Running));
        }
    }
}

#[test]
fn task_progress_and_terminal_events_update_only_through_reducer() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let task_id = app.tasks.create(
            tale::action::ActionId::MockSuccess,
            "simulation",
            MOCK_NOW,
            true,
        );
        let _ = app.update(Event::Task(Box::new(TaskEvent::Started { task_id })));
        let _ = app.update(Event::Task(Box::new(TaskEvent::Progress {
            task_id,
            progress: Progress {
                completed: 1,
                total: 2,
            },
            detail: "step one".to_owned(),
        })));
        assert_eq!(
            app.tasks.get(task_id).map(|task| task.progress),
            Some(Some(Progress {
                completed: 1,
                total: 2
            }))
        );
        let _ = app.update(Event::Task(Box::new(TaskEvent::Succeeded {
            task_id,
            finished_at: MOCK_NOW,
            summary: "done".to_owned(),
            detail: "complete".to_owned(),
        })));
        assert_eq!(
            app.tasks.get(task_id).map(|task| task.state),
            Some(TaskState::Succeeded)
        );
        let _ = app.update(Event::Task(Box::new(TaskEvent::Started { task_id })));
        assert_eq!(
            app.tasks.get(task_id).map(|task| task.state),
            Some(TaskState::Succeeded)
        );
    }
}

#[test]
fn service_task_success_verifies_and_failure_preserves_snapshot() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.source_mode = SourceMode::Local;
        let capabilities = LocalCapabilities::all_supported();
        app.local_capabilities = capabilities;
        app.local_executable = Some(LocalExecutable {
            path: PathBuf::from("/fictional/tailscale"),
            source: ExecutableSource::Cli,
            version: "1.98.9".to_owned(),
            daemon_version: None,
            build: None,
            capabilities,
        });
        app.services_snapshot.generation = 1;
        let Some(port_443) = Port::new(443).ok() else {
            return;
        };
        let Some(port_3000) = Port::new(3000).ok() else {
            return;
        };
        let mapping = ServiceMapping {
            exposure: Exposure::Tailnet,
            listener: Listener::Https(port_443),
            mount: PathMount::Root,
            backend: Backend::Port(port_3000),
            proxy_protocol: ProxyProtocol::None,
            hostname: None,
        };
        let request = ServiceActionRequest::Serve {
            mapping: mapping.clone(),
            edit: false,
        };
        let task_id = app
            .tasks
            .create(request.action_id(), request.target_label(), MOCK_NOW, true);
        let _ = app.update(Event::Task(Box::new(TaskEvent::Started { task_id })));
        let effects = app.update(Event::Services(Box::new(ServicesEvent::TaskFinished {
            task_id,
            request: request.clone(),
            result: Ok(ServiceTaskData::Serve {
                status: ServeStatus {
                    mappings: vec![mapping.clone()],
                },
                verified: true,
                summary: "verified Serve write".to_owned(),
            }),
            exit_status: Some(0),
            stdout_truncated: false,
            stderr_truncated: false,
        })));
        assert!(effects.iter().any(|effect| matches!(
            effect,
            tale::effect::Effect::StartLocalServicesRefresh { .. }
        )));
        assert_eq!(
            app.tasks.get(task_id).map(|task| task.state),
            Some(TaskState::Succeeded)
        );
        assert_eq!(
            app.tasks.get(task_id).and_then(|task| task.exit_status),
            Some(0)
        );
        assert_eq!(
            app.services_snapshot
                .serve
                .value
                .as_ref()
                .map(|status| status.mappings.as_slice()),
            Some([mapping.clone()].as_slice())
        );

        let failed_request = ServiceActionRequest::ServeReset;
        let failed_task = app.tasks.create(
            failed_request.action_id(),
            failed_request.target_label(),
            MOCK_NOW,
            true,
        );
        let _ = app.update(Event::Task(Box::new(TaskEvent::Started {
            task_id: failed_task,
        })));
        let failure = ServiceFailure {
            kind: ServiceFailureKind::CommandFailed,
            operation: "serve reset".to_owned(),
            summary: "local service command returned an error".to_owned(),
            detail: "fictional failure".to_owned(),
            exit_status: Some(7),
            stdout_truncated: false,
            stderr_truncated: true,
        };
        let _ = app.update(Event::Services(Box::new(ServicesEvent::TaskFinished {
            task_id: failed_task,
            request: failed_request,
            result: Err(failure),
            exit_status: Some(7),
            stdout_truncated: false,
            stderr_truncated: true,
        })));
        assert_eq!(
            app.tasks.get(failed_task).map(|task| task.state),
            Some(TaskState::Failed)
        );
        assert_eq!(
            app.tasks.get(failed_task).and_then(|task| task.exit_status),
            Some(7)
        );
        assert!(
            app.tasks
                .get(failed_task)
                .is_some_and(|task| task.detail.contains("truncated"))
        );
        assert_eq!(
            app.services_snapshot
                .serve
                .value
                .as_ref()
                .map(|status| status.mappings.as_slice()),
            Some([mapping].as_slice())
        );
    }
}

#[test]
fn stale_service_refresh_cannot_replace_newer_data_and_read_only_blocks_dispatch() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.source_mode = SourceMode::Local;
        let capabilities = LocalCapabilities::all_supported();
        app.local_capabilities = capabilities;
        app.local_executable = Some(LocalExecutable {
            path: PathBuf::from("/fictional/tailscale"),
            source: ExecutableSource::Cli,
            version: "1.98.9".to_owned(),
            daemon_version: None,
            build: None,
            capabilities,
        });
        app.services_snapshot.generation = 2;
        let Some(port) = Port::new(443).ok() else {
            return;
        };
        let Some(backend_port) = Port::new(3000).ok() else {
            return;
        };
        app.services_snapshot.funnel.succeed(
            2,
            MOCK_NOW,
            FunnelStatus {
                mappings: vec![ServiceMapping {
                    exposure: Exposure::Public,
                    listener: Listener::Https(port),
                    mount: PathMount::Root,
                    backend: Backend::Port(backend_port),
                    proxy_protocol: ProxyProtocol::None,
                    hostname: None,
                }],
            },
        );
        let before = app.services_snapshot.funnel.value.clone();
        let _ = app.update(Event::Services(Box::new(ServicesEvent::RefreshFinished {
            generation: 1,
            observed_at: MOCK_NOW.saturating_sub(10),
            command_version: "1.0.0".to_owned(),
            serve: Ok(ServeStatus::default()),
            funnel: Ok(FunnelStatus::default()),
            taildrop_targets: Ok(Vec::new()),
            taildrive: Ok(Vec::new()),
        })));
        assert_eq!(app.services_snapshot.funnel.value, before);

        app.resolved_config.read_only = true;
        app.route_stack = vec![Route::Services];
        let _ = app.dispatch_action(tale::action::ActionId::ServicesFunnelReset);
        assert!(app.runtime_error.is_some());
        assert!(app.tasks.all().is_empty());
    }
}
