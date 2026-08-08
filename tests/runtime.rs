use std::path::PathBuf;
use std::time::{Duration, Instant};

use tale::app::App;
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::error::TaleError;
use tale::event::{Event, InputEvent, ShutdownReason, SourceEvent, TaskEvent};
use tale::paths::{PathEnvironment, Platform};
use tale::runtime::{EventQueue, TerminalDriver};
use tale::task::TaskId;

fn mock_app() -> Option<App> {
    let root = PathBuf::from("/fictional/tale-runtime");
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

struct FakeDriver {
    fail_draw: bool,
    draws: usize,
    restores: usize,
}

impl TerminalDriver for FakeDriver {
    fn draw(&mut self, _app: &App) -> Result<(), TaleError> {
        self.draws = self.draws.saturating_add(1);
        if self.fail_draw {
            Err(TaleError::Terminal("fictional render failure".to_owned()))
        } else {
            Ok(())
        }
    }

    fn restore(&mut self) -> Result<(), TaleError> {
        self.restores = self.restores.saturating_add(1);
        Ok(())
    }
}

#[tokio::test]
async fn bounded_queue_applies_backpressure_without_dropping_completion() {
    let queue = EventQueue::new();
    for _ in 0..256 {
        queue.send(Event::Input(InputEvent::FocusGained)).await;
    }

    let sender = {
        let queue = queue.clone();
        tokio::spawn(async move {
            queue
                .send(Event::Task(Box::new(TaskEvent::Succeeded {
                    task_id: TaskId(91),
                    finished_at: 1_754_000_000,
                    summary: "complete".to_owned(),
                    detail: "fictional completion".to_owned(),
                })))
                .await;
        })
    };

    for _ in 0..256 {
        let _ = queue.recv().await;
    }
    let received = tokio::time::timeout(Duration::from_millis(100), queue.recv()).await;
    assert!(received.is_ok());
    if let Ok(event) = received {
        assert!(matches!(
            event,
            Event::Task(task)
                if matches!(
                    *task,
                    TaskEvent::Succeeded {
                        task_id: TaskId(91),
                        ..
                    }
                )
        ));
    }
    let joined = tokio::time::timeout(Duration::from_millis(100), sender).await;
    assert!(joined.is_ok());
    if let Ok(result) = joined {
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn cosmetic_ticks_and_resize_events_keep_only_the_latest_value() {
    let queue = EventQueue::new();
    let first_tick = Instant::now();
    let second_tick = first_tick + Duration::from_secs(1);
    queue.send(Event::Tick(first_tick)).await;
    queue.send(Event::Tick(second_tick)).await;
    let tick = queue.recv().await;
    assert!(matches!(tick, Event::Tick(value) if value == second_tick));

    queue
        .send(Event::Input(InputEvent::Resize {
            width: 80,
            height: 24,
        }))
        .await;
    queue
        .send(Event::Input(InputEvent::Resize {
            width: 160,
            height: 45,
        }))
        .await;
    let resize = queue.recv().await;
    assert!(matches!(
        resize,
        Event::Input(InputEvent::Resize {
            width: 160,
            height: 45
        })
    ));
}

#[test]
fn idle_ticks_do_not_invalidate_a_clean_frame_but_active_tasks_do() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.clear_render_invalidated();
        let _ = app.update(Event::Tick(Instant::now()));
        assert!(!app.render_invalidated());

        let task_id = app.tasks.create(
            tale::action::ActionId::MockCancellable,
            "fictional task",
            1_754_000_000,
            true,
        );
        let _ = app.update(Event::Task(Box::new(TaskEvent::Started { task_id })));
        app.clear_render_invalidated();
        let _ = app.update(Event::Tick(Instant::now()));
        assert!(app.render_invalidated());
    }
}

#[tokio::test]
async fn render_failure_restores_the_terminal_before_returning_the_error() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let mut driver = FakeDriver {
            fail_draw: true,
            draws: 0,
            restores: 0,
        };
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            tale::runtime::run_with_driver(&mut app, &mut driver),
        )
        .await;
        assert!(result.is_ok());
        if let Ok(result) = result {
            assert!(matches!(result, Err(TaleError::Terminal(_))));
        }
        assert_eq!(driver.draws, 1);
        assert_eq!(driver.restores, 1);
    }
}

#[tokio::test]
async fn event_source_failure_restores_the_terminal_before_returning_the_error() {
    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        let queue = EventQueue::new();
        queue
            .send(Event::Source(SourceEvent::InputFailed(
                "fictional input source failure".to_owned(),
            )))
            .await;
        let mut driver = FakeDriver {
            fail_draw: false,
            draws: 0,
            restores: 0,
        };
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            tale::runtime::run_with_driver_and_queue(&mut app, &mut driver, queue),
        )
        .await;
        assert!(result.is_ok());
        if let Ok(result) = result {
            assert!(matches!(
                result,
                Err(TaleError::Application(message)) if message == "fictional input source failure"
            ));
        }
        assert_eq!(driver.restores, 1);
    }
}

#[tokio::test]
async fn shutdown_restores_terminal_with_each_bottom_interaction_active() {
    for key in [None, Some(':'), Some('/'), Some('a'), Some('?')] {
        let app = mock_app();
        assert!(app.is_some());
        if let Some(mut app) = app {
            app.set_route(tale::app::Route::Devices);
            let _ = app.update(Event::Source(SourceEvent::LoadSucceeded {
                generation: 1,
                devices: tale::mock::devices(),
                observed_at: tale::mock::MOCK_NOW,
            }));
            if let Some(key) = key {
                let _ = app.update(Event::Input(InputEvent::Key(
                    crossterm::event::KeyEvent::new(
                        crossterm::event::KeyCode::Char(key),
                        crossterm::event::KeyModifiers::NONE,
                    ),
                )));
            }
            let queue = EventQueue::new();
            queue
                .send(Event::ShutdownRequested(ShutdownReason::Signal))
                .await;
            let mut driver = FakeDriver {
                fail_draw: false,
                draws: 0,
                restores: 0,
            };
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                tale::runtime::run_with_driver_and_queue(&mut app, &mut driver, queue),
            )
            .await;
            assert!(matches!(result, Ok(Ok(()))));
            assert_eq!(driver.restores, 1);
        }
    }
}
