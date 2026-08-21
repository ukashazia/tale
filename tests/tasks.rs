use tale::action::ActionId;
use tale::app::App;
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::event::{Event, TaskEvent};
use tale::mock::MOCK_NOW;
use tale::paths::{PathEnvironment, Platform};
use tale::task::{DETAIL_CAP, Progress, TaskChange, TaskState, TaskStore, bounded_detail};

fn app() -> Option<App> {
    let root = std::path::PathBuf::from("/fictional/tale-tasks");
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
fn only_valid_state_transitions_are_accepted_and_cancellation_is_idempotent() {
    let mut store = TaskStore::new();
    let task = store.create(ActionId::MockCancellable, "fictional task", MOCK_NOW, true);
    assert!(store.request_cancel(task));
    assert_eq!(
        store.get(task).map(|value| value.state),
        Some(TaskState::Cancelling)
    );
    assert!(store.cancel(task, MOCK_NOW, "cancelled before dispatch"));
    let task = store.create(ActionId::MockCancellable, "fictional task", MOCK_NOW, true);
    assert!(!store.succeed(task, MOCK_NOW, "bad", "bad"));
    assert!(store.start(task));
    assert!(store.request_cancel(task));
    assert!(store.request_cancel(task));
    assert!(store.cancel(task, MOCK_NOW, "cancelled"));
    assert!(!store.start(task));
    assert!(!store.request_cancel(task));
}

#[test]
fn active_tasks_survive_completed_history_eviction() {
    let mut store = TaskStore::new();
    let active = store.create(ActionId::MockCancellable, "active", MOCK_NOW, true);
    assert!(store.start(active));
    for index in 0..3 {
        let task = store.create(
            ActionId::MockSuccess,
            format!("completed-{index}"),
            MOCK_NOW,
            true,
        );
        assert!(store.start(task));
        assert!(store.succeed(task, MOCK_NOW, "done", "detail"));
    }
    store.evict_completed(1);
    assert!(store.get(active).is_some());
    assert_eq!(
        store
            .all()
            .iter()
            .filter(|task| task.state.is_terminal())
            .count(),
        1
    );
}

#[test]
fn restored_history_gets_fresh_session_ids_without_moving_live_tasks() {
    let mut previous_session = TaskStore::new();
    let historical = previous_session.create(ActionId::MockSuccess, "historical", 10, true);
    assert!(previous_session.start(historical));
    assert!(previous_session.succeed(historical, 11, "done", "saved"));
    let restored = previous_session.take_dirty();

    let mut current_session = TaskStore::new();
    let live = current_session.create(ActionId::MockCancellable, "live", 20, true);
    let live_record = current_session.get(live).map(|task| task.record_id);
    current_session.merge_restored(restored);

    assert_eq!(
        current_session.get(live).map(|task| task.record_id),
        live_record
    );
    let ids = current_session
        .all()
        .iter()
        .map(|task| task.id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 2);
    assert_eq!(
        current_session
            .session()
            .map(|task| task.target_label.as_str())
            .collect::<Vec<_>>(),
        vec!["live"]
    );
}

#[test]
fn structured_changes_are_included_in_dirty_updates() {
    let mut store = TaskStore::new();
    let task = store.create(ActionId::AdminDeviceRename, "machine alpha", 10, true);
    let _ = store.take_dirty();
    let changes = vec![TaskChange {
        field: "machine name".to_owned(),
        before: Some("alpha".to_owned()),
        after: Some("beta".to_owned()),
    }];
    assert!(store.set_changes(task, changes.clone()));
    let dirty = store.take_dirty();
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0].changes, changes);
}

#[test]
fn output_cap_retains_both_ends_with_a_visible_marker() {
    let value = format!("{}END", "A".repeat(DETAIL_CAP + 100));
    let bounded = bounded_detail(&value, DETAIL_CAP);
    assert!(bounded.len() <= DETAIL_CAP);
    assert!(bounded.contains("output truncated"));
    assert!(bounded.starts_with("AAA"));
    assert!(bounded.ends_with("END"));
    let unicode = bounded_detail(&"界".repeat(DETAIL_CAP), DETAIL_CAP);
    assert!(unicode.len() <= DETAIL_CAP);
    assert!(std::str::from_utf8(unicode.as_bytes()).is_ok());
}

#[test]
fn task_filtering_keeps_selection_inside_the_filtered_projection() {
    let mut store = TaskStore::new();
    let first = store.create(ActionId::MockSuccess, "network flow", MOCK_NOW, false);
    let second = store.create(ActionId::MockFailure, "webhook delivery", MOCK_NOW, false);
    let third = store.create(ActionId::MockSuccess, "network export", MOCK_NOW, false);
    assert!(store.start(first));
    assert!(store.succeed(first, MOCK_NOW, "flow complete", "bounded flow"));
    assert!(store.start(second));
    assert!(store.fail(second, MOCK_NOW, "webhook failed", "server rejected test"));
    assert!(store.start(third));
    assert!(store.succeed(third, MOCK_NOW, "export complete", "deterministic output"));

    let filtered = store
        .filtered("network")
        .map(|task| task.id)
        .collect::<Vec<_>>();
    assert_eq!(filtered, vec![first, third]);
    store.select_filtered_first("network");
    assert_eq!(store.selected, Some(first));
    store.select_next_filtered("network", 1);
    assert_eq!(store.selected, Some(third));
    store.select_filtered_position("webhook", 0);
    assert_eq!(store.selected, Some(second));
    store.select_next_filtered("missing", 1);
    assert_eq!(store.selected, None);
}

#[test]
fn notifications_expire_without_removing_task_results() {
    let application = app();
    assert!(application.is_some());
    if let Some(mut application) = application {
        let task = application
            .tasks
            .create(ActionId::MockSuccess, "fictional", MOCK_NOW, true);
        let _ = application.update(Event::Task(Box::new(TaskEvent::Started { task_id: task })));
        let _ = application.update(Event::Task(Box::new(TaskEvent::Progress {
            task_id: task,
            progress: Progress {
                completed: 1,
                total: 1,
            },
            detail: "step".to_owned(),
        })));
        let _ = application.update(Event::Task(Box::new(TaskEvent::Succeeded {
            task_id: task,
            finished_at: MOCK_NOW,
            summary: "done".to_owned(),
            detail: "finished".to_owned(),
        })));
        assert_eq!(application.notifications.len(), 1);
        let started = std::time::Instant::now();
        for seconds in 0..=6 {
            let _ = application.update(Event::Tick(
                started + std::time::Duration::from_secs(seconds),
            ));
        }
        assert!(application.notifications.is_empty());
        assert_eq!(
            application.tasks.get(task).map(|value| value.state),
            Some(TaskState::Succeeded)
        );
    }
}
