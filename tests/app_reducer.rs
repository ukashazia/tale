use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use tale::action::{self, ActionContext, Binding};
use tale::app::{
    App, Focus, InteractionMode, Route, ShutdownState, SourceMode, ViewFrame, ViewHistory,
};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::device::{SortDirection, SortField, SortSpec};
use tale::domain::service::{
    Backend, Exposure, FunnelStatus, Listener, PathMount, Port, ProxyProtocol, ServeStatus,
    ServiceActionRequest, ServiceFailure, ServiceFailureKind, ServiceMapping, ServiceTaskData,
};
use tale::domain::source::{ExecutableSource, LocalCapabilities, LocalExecutable};
use tale::event::{Event, InputEvent, LocalEvent, ServicesEvent, SourceEvent, TaskEvent};
use tale::mock::{self, MOCK_NOW};
use tale::paths::{PathEnvironment, Platform};
use tale::task::{Progress, TaskState};
use tale::ui::theme::ThemeId;

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

fn press(app: &mut App, code: KeyCode) {
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        code,
        KeyModifiers::NONE,
    ))));
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

#[test]
fn stale_watcher_generation_cannot_replace_current_connection_state() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.source_mode = SourceMode::Local;
        let effects = app.bootstrap_effects();
        assert!(effects.iter().any(|effect| matches!(
            effect,
            tale::effect::Effect::StartLocalObservation { generation: 1, .. }
        )));
        let _ = app.update(Event::Local(Box::new(LocalEvent::WatcherConnected {
            generation: 1,
        })));
        assert!(matches!(
            app.local_daemon_state,
            tale::domain::source::LocalDaemonState::Connecting
        ));

        let failure = tale::domain::source::LocalFailure::new(
            tale::domain::source::LocalFailureKind::DaemonUnavailable,
            "watch-ipn-bus",
            "fictional disconnect",
            "fictional old observer",
            true,
        );
        let _ = app.update(Event::Local(Box::new(LocalEvent::WatcherDisconnected {
            generation: 0,
            failure: failure.clone(),
        })));
        assert!(matches!(
            app.local_daemon_state,
            tale::domain::source::LocalDaemonState::Connecting
        ));
        let _ = app.update(Event::Local(Box::new(LocalEvent::WatcherDisconnected {
            generation: 1,
            failure,
        })));
        assert!(matches!(
            app.local_daemon_state,
            tale::domain::source::LocalDaemonState::Reconnecting
        ));
    }
}

#[test]
fn completion_generations_advance_and_resize_preserves_editor_state() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        press(&mut app, KeyCode::Char(':'));
        let initial_generation = match &app.interaction {
            InteractionMode::CommandLine(state) => state.generation,
            _ => 0,
        };
        let _ = app.update(Event::Input(InputEvent::Paste("de".to_owned())));
        let (edited_generation, input, cursor) = match &app.interaction {
            InteractionMode::CommandLine(state) => (
                state.generation,
                state.editor.input.clone(),
                state.editor.cursor,
            ),
            _ => (0, String::new(), 0),
        };
        assert!(edited_generation > initial_generation);
        let _ = app.update(Event::Input(InputEvent::Resize {
            width: 60,
            height: 18,
        }));
        assert!(matches!(
            &app.interaction,
            InteractionMode::CommandLine(state)
                if state.generation == edited_generation
                    && state.editor.input == input
                    && state.editor.cursor == cursor
        ));
    }
}

#[test]
fn navigation_palette_is_canonical_and_fuzzy() {
    assert_eq!(Route::parse("devices"), Some(Route::Devices));
    assert_eq!(Route::parse("dev"), None);
    assert_eq!(Route::parse("home"), None);

    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.terminal_width = 140;
        press(&mut app, KeyCode::Char(':'));
        assert!(matches!(
            &app.interaction,
            InteractionMode::CommandLine(state)
                if state.candidates.len() == 11
                    && state.candidates.first().map(|candidate| candidate.route)
                        == Some(Route::Devices)
        ));

        let _ = app.update(Event::Input(InputEvent::Paste("dvcs".to_owned())));
        assert!(matches!(
            &app.interaction,
            InteractionMode::CommandLine(state)
                if state.candidates.first().map(|candidate| candidate.route)
                    == Some(Route::Devices)
        ));

        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char(':'));
        let _ = app.update(Event::Input(InputEvent::Paste("audit".to_owned())));
        assert!(matches!(
            &app.interaction,
            InteractionMode::CommandLine(state)
                if state.candidates.first().map(|candidate| candidate.route)
                    == Some(Route::Activity)
        ));

        press(&mut app, KeyCode::Enter);
        assert_eq!(app.current_route(), Route::Activity);
    }
}

#[test]
fn refresh_removal_repairs_selection_without_discarding_active_input() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        load_app(&mut app);
        app.set_route(Route::Devices);
        let selected = app
            .devices_resource
            .snapshot
            .get(3)
            .map(|device| device.id.clone());
        assert!(selected.is_some());
        app.views.devices.selected_id = selected.clone();
        press(&mut app, KeyCode::Char(':'));
        let _ = app.update(Event::Input(InputEvent::Paste("devices".to_owned())));
        press(&mut app, KeyCode::Left);
        let before = match &app.interaction {
            InteractionMode::CommandLine(state) => (
                state.editor.input.clone(),
                state.editor.cursor,
                state.generation,
            ),
            _ => (String::new(), 0, 0),
        };
        let devices = mock::devices()
            .into_iter()
            .filter(|device| Some(&device.id) != selected.as_ref())
            .collect();
        let _ = app.update(Event::Source(SourceEvent::LoadSucceeded {
            generation: 2,
            devices,
            observed_at: MOCK_NOW.saturating_add(1),
        }));
        assert!(matches!(
            &app.interaction,
            InteractionMode::CommandLine(state)
                if state.editor.input == before.0
                    && state.editor.cursor == before.1
                    && state.generation == before.2
        ));
        assert_ne!(app.views.devices.selected_id, selected);
        assert_eq!(
            app.runtime_error.as_deref(),
            Some("selected resource no longer exists; selection was repaired")
        );
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

        let parsed_filter =
            tale::domain::filter::parse("os:android", &tale::domain::filter::device_schema());
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
fn shell_modes_are_mutually_exclusive() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        load_app(&mut app);
        app.set_route(Route::Devices);
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        ))));
        assert!(matches!(app.interaction, InteractionMode::Transient(_)));
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('?'),
            KeyModifiers::NONE,
        ))));
        assert!(matches!(app.interaction, InteractionMode::HelpSheet));
        press(&mut app, KeyCode::Char(':'));
        assert!(matches!(app.interaction, InteractionMode::CommandLine(_)));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Char('/'));
        assert!(matches!(app.interaction, InteractionMode::FilterLine(_)));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Char('a'));
        assert!(matches!(app.interaction, InteractionMode::Transient(_)));
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
        app.set_route(Route::Devices);
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('/'),
            KeyModifiers::NONE,
        ))));
        let _ = app.update(Event::Input(InputEvent::Paste("online:true q".to_owned())));
        assert!(matches!(app.interaction, InteractionMode::FilterLine(_)));
        if let InteractionMode::FilterLine(state) = &app.interaction {
            assert_eq!(state.editor.input, "online:true q");
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
fn fuzzy_navigation_filter_and_browser_history_restore_and_branch() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        load_app(&mut app);
        // Tale opens on Devices, so history needs a different frame first.
        press(&mut app, KeyCode::Char(':'));
        let _ = app.update(Event::Input(InputEvent::Paste("overview".to_owned())));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.current_route(), Route::Overview);
        press(&mut app, KeyCode::Char(':'));
        let _ = app.update(Event::Input(InputEvent::Paste("dvcs".to_owned())));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.current_route(), Route::Devices);
        press(&mut app, KeyCode::Char('/'));
        let _ = app.update(Event::Input(InputEvent::Paste(
            "owner:alice online:true".to_owned(),
        )));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.views.devices.filter_draft, "owner:alice online:true");

        press(&mut app, KeyCode::Char('['));
        assert_eq!(app.current_route(), Route::Overview);
        press(&mut app, KeyCode::Char(']'));
        assert_eq!(app.current_route(), Route::Devices);
        assert_eq!(app.views.devices.filter_draft, "owner:alice online:true");

        press(&mut app, KeyCode::Char('['));
        press(&mut app, KeyCode::Char(':'));
        let _ = app.update(Event::Input(InputEvent::Paste("services".to_owned())));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.current_route(), Route::Services);
        let length = app.view_history.frames.len();
        press(&mut app, KeyCode::Char(']'));
        assert_eq!(app.current_route(), Route::Services);
        assert_eq!(app.view_history.frames.len(), length);
    }
}

#[test]
fn filter_invalid_last_good_and_escape_restore_the_full_point() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        load_app(&mut app);
        app.set_route(Route::Devices);
        let original = app.views.devices.selected_id.clone();
        press(&mut app, KeyCode::Char('/'));
        let _ = app.update(Event::Input(InputEvent::Paste("online:true".to_owned())));
        assert_eq!(app.views.devices.filter_draft, "online:true");
        let _ = app.update(Event::Input(InputEvent::Paste(" owner:\"".to_owned())));
        assert!(matches!(
            app.interaction,
            InteractionMode::FilterLine(ref state) if state.error.is_some()
        ));
        assert_eq!(app.views.devices.filter_draft, "online:true");
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.interaction, InteractionMode::Normal));
        assert!(app.views.devices.filter_draft.is_empty());
        assert_eq!(app.views.devices.selected_id, original);
    }
}

#[test]
fn transient_leaf_dispatches_without_list_navigation() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        load_app(&mut app);
        app.set_route(Route::Devices);
        press(&mut app, KeyCode::Char('a'));
        assert!(matches!(app.interaction, InteractionMode::Transient(_)));
        press(&mut app, KeyCode::Char('s'));
        assert!(matches!(app.interaction, InteractionMode::Normal));
        assert_eq!(app.tasks.all().len(), 1);
    }
}

#[test]
fn transient_prefix_keeps_catalog_and_disabled_leaf_reports_in_place() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        load_app(&mut app);
        app.set_route(Route::Overview);
        press(&mut app, KeyCode::Char('a'));
        let action_count = match &app.interaction {
            InteractionMode::Transient(state) => state.actions.len(),
            _ => 0,
        };
        press(&mut app, KeyCode::Char('v'));
        assert!(matches!(
            &app.interaction,
            InteractionMode::Transient(state)
                if state.prefix == Some('v') && state.actions.len() == action_count
        ));
        press(&mut app, KeyCode::Esc);
        assert!(matches!(
            &app.interaction,
            InteractionMode::Transient(state)
                if state.prefix.is_none() && state.actions.len() == action_count
        ));
        press(&mut app, KeyCode::Char('v'));
        press(&mut app, KeyCode::Char('z'));
        assert!(matches!(
            &app.interaction,
            InteractionMode::Transient(state)
                if state.prefix.is_none()
                    && state.message.as_deref() == Some("unknown key: vz")
        ));

        press(&mut app, KeyCode::Char('h'));
        press(&mut app, KeyCode::Char('o'));
        assert!(matches!(
            &app.interaction,
            InteractionMode::Transient(state)
                if state.message.is_some() && state.prefix == Some('h')
        ));
    }
}

#[test]
fn view_history_is_non_empty_and_bounded_to_one_hundred() {
    let mut history = ViewHistory::new(Route::Overview);
    for index in 0..250 {
        let route = if index % 2 == 0 {
            Route::Devices
        } else {
            Route::Users
        };
        let _ = history.append(ViewFrame::new(route));
        assert!(!history.frames.is_empty());
        assert!(history.cursor < history.frames.len());
        assert!(history.frames.len() <= 100);
    }
    assert_eq!(history.frames.len(), 100);
    let before = history.current().cloned();
    let _ = history.backward();
    let _ = history.forward();
    assert_eq!(history.current(), before.as_ref());
}

#[test]
fn missing_history_identity_restores_deterministically_with_notice() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        load_app(&mut app);
        app.set_route(Route::Devices);
        let selected = app
            .devices_resource
            .snapshot
            .first()
            .map(|device| device.id.clone());
        assert!(selected.is_some());
        app.views.devices.selected_id = selected.clone();

        press(&mut app, KeyCode::Char(':'));
        let _ = app.update(Event::Input(InputEvent::Paste("services".to_owned())));
        press(&mut app, KeyCode::Enter);
        let mut replacement = mock::devices();
        replacement.retain(|device| Some(&device.id) != selected.as_ref());
        let _ = app.update(Event::Source(SourceEvent::LoadSucceeded {
            generation: 2,
            devices: replacement,
            observed_at: MOCK_NOW.saturating_add(1),
        }));
        press(&mut app, KeyCode::Char('['));
        assert_eq!(app.current_route(), Route::Devices);
        assert!(app.views.devices.selected_id.is_some());
        assert_eq!(
            app.runtime_error.as_deref(),
            Some("previous selection no longer exists")
        );
    }
}

#[test]
fn q_with_active_tasks_uses_confirmation_boundary() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let task_id = app.tasks.create(
            tale::action::ActionId::MockNonCancellable,
            "active simulation",
            MOCK_NOW,
            false,
        );
        let _ = app.update(Event::Task(Box::new(TaskEvent::Started { task_id })));
        press(&mut app, KeyCode::Char('q'));
        assert_eq!(app.overlay_title(), Some("quit"));
        assert!(matches!(app.shutdown_state, ShutdownState::Running));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.shutdown_state, ShutdownState::Requested(_)));
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
            socket_path: None,
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
            socket_path: None,
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
        app.set_route(Route::Services);
        let _ = app.dispatch_action(tale::action::ActionId::ServicesFunnelReset);
        assert!(app.runtime_error.is_some());
        assert!(app.tasks.all().is_empty());
    }
}

#[test]
fn appearance_is_a_direct_key_choice_and_cancelling_changes_nothing() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.set_route(Route::Settings);
        let original = app.theme;
        let route = app.current_route();
        let history_len = app.view_history.frames.len();
        let source_mode = app.source_mode;

        // Leaving the menu alone leaves the theme alone.
        let effects = app.dispatch_action(tale::action::ActionId::SettingsAppearance);
        assert!(effects.is_empty());
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.theme, original);
        assert!(matches!(app.interaction, InteractionMode::Normal));

        // One key applies and closes, like every other transient menu.
        let effects = app.dispatch_action(tale::action::ActionId::SettingsAppearance);
        assert!(effects.is_empty());
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.theme.id(), ThemeId::TailscaleLight);
        assert!(matches!(app.interaction, InteractionMode::Normal));
        assert!(app.overlays.is_empty());

        // And it is reversible by picking the other one.
        let _ = app.dispatch_action(tale::action::ActionId::SettingsAppearance);
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.theme.id(), ThemeId::TailscaleDark);
        assert_eq!(app.current_route(), route);
        assert_eq!(app.view_history.frames.len(), history_len);
        assert_eq!(app.source_mode, source_mode);
        assert!(app.tasks.all().is_empty());
    }
}

#[test]
fn sort_is_a_two_key_mnemonic_naming_the_column_then_the_order() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        load_app(&mut app);
        app.set_route(Route::Devices);

        // The first key names the column and waits, like an action prefix.
        press(&mut app, KeyCode::Char('s'));
        assert!(matches!(app.interaction, InteractionMode::Transient(_)));
        press(&mut app, KeyCode::Char('n'));
        if let InteractionMode::Transient(state) = &app.interaction {
            assert_eq!(state.prefix, Some('n'));
        } else {
            panic!("the menu should stay open while a prefix is pending");
        }

        // The second key names the order, applies, and closes.
        press(&mut app, KeyCode::Char('d'));
        assert!(matches!(app.interaction, InteractionMode::Normal));
        assert_eq!(
            app.views.devices.sort,
            SortSpec {
                field: SortField::Name,
                direction: SortDirection::Descending,
            }
        );

        // A different column keeps its own order key.
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::Char('a'));
        assert_eq!(
            app.views.devices.sort,
            SortSpec {
                field: SortField::LastSeen,
                direction: SortDirection::Ascending,
            }
        );

        // Esc backs out of a pending prefix before it closes the menu.
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('w'));
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.interaction, InteractionMode::Transient(_)));
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.interaction, InteractionMode::Normal));
        assert_eq!(
            app.views.devices.sort,
            SortSpec {
                field: SortField::LastSeen,
                direction: SortDirection::Ascending,
            }
        );
    }
}

#[test]
fn opening_a_detail_can_always_be_left_again() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        load_app(&mut app);
        app.set_route(Route::Devices);

        // `h` returns, as the documented binding says it does.
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.focus, Focus::Inspector);
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(app.focus, Focus::Collection);

        // So does Esc, because the detail pane is a state the user opened.
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.focus, Focus::Inspector);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.focus, Focus::Collection);

        // And the way back is advertised while it applies.
        press(&mut app, KeyCode::Enter);
        let detail = action::footer_hints(ActionContext::Detail, 200);
        assert!(detail.iter().any(|hint| hint == "h back"));
        let collection = action::footer_hints(ActionContext::Collection, 200);
        assert!(!collection.iter().any(|hint| hint == "h back"));
    }
}

#[test]
fn shifted_bindings_reach_their_actions() {
    // Uppercase keys arrive with SHIFT set; a binding for `G` must still match.
    for character in ['G', 'R', 'H', 'L'] {
        assert!(
            Binding::Char(character)
                .matches(KeyEvent::new(KeyCode::Char(character), KeyModifiers::SHIFT)),
            "{character} should match when the terminal reports shift"
        );
        assert!(
            Binding::Char(character)
                .matches(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)),
            "{character} should match when the terminal reports no modifier"
        );
    }
    // A real modifier still has to be respected.
    assert!(!Binding::Char('G').matches(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::CONTROL)));
}

#[test]
fn shift_g_jumps_to_the_last_row() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        load_app(&mut app);
        app.set_route(Route::Devices);
        let last = app
            .visible_indices()
            .last()
            .and_then(|index| app.devices_resource.snapshot.get(*index))
            .map(|device| device.id.clone());
        assert!(last.is_some());

        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('G'),
            KeyModifiers::SHIFT,
        ))));
        assert_eq!(app.views.devices.selected_id, last);

        let first = app
            .visible_indices()
            .first()
            .and_then(|index| app.devices_resource.snapshot.get(*index))
            .map(|device| device.id.clone());
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.views.devices.selected_id, first);
    }
}
