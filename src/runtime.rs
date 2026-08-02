use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::event;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinSet;

use crate::app::App;
use crate::effect::{Effect, Resource};
use crate::error::TaleError;
use crate::event::{self as app_event, Event, InputEvent, ShutdownReason, SourceEvent, TaskEvent};
use crate::mock::{self, MOCK_NOW, MockTaskBehavior};
use crate::task::{Progress, TaskId, grace_duration};
use crate::terminal::RealTerminal;
use crate::ui;

const EVENT_CAPACITY: usize = 256;
const TICK_INTERVAL: Duration = Duration::from_millis(100);

pub trait TerminalDriver {
    fn draw(&mut self, app: &App) -> Result<(), TaleError>;
    fn restore(&mut self) -> Result<(), TaleError>;
}

#[derive(Clone)]
pub struct EventQueue {
    state: Arc<QueueState>,
}

struct QueueState {
    events: Mutex<std::collections::VecDeque<Event>>,
    notify: Notify,
}

impl EventQueue {
    pub fn new() -> Self {
        Self {
            state: Arc::new(QueueState {
                events: Mutex::new(std::collections::VecDeque::with_capacity(EVENT_CAPACITY)),
                notify: Notify::new(),
            }),
        }
    }

    pub async fn send(&self, event: Event) {
        let cosmetic = matches!(event, Event::Tick(_));
        loop {
            let notified = {
                let mut events = self.state.events.lock().await;
                if let Some(existing) = events.iter_mut().find(|existing| {
                    matches!(
                        (existing, &event),
                        (Event::Tick(_), Event::Tick(_))
                            | (
                                Event::Input(InputEvent::Resize { .. }),
                                Event::Input(InputEvent::Resize { .. })
                            )
                    )
                }) {
                    *existing = event;
                    self.state.notify.notify_one();
                    return;
                }
                if events.len() < EVENT_CAPACITY {
                    events.push_back(event);
                    self.state.notify.notify_one();
                    return;
                }
                if cosmetic {
                    return;
                }
                self.state.notify.notified()
            };
            notified.await;
        }
    }

    pub async fn recv(&self) -> Event {
        loop {
            let event = {
                let mut events = self.state.events.lock().await;
                events.pop_front()
            };
            if let Some(event) = event {
                self.state.notify.notify_waiters();
                return event;
            }
            self.state.notify.notified().await;
        }
    }
}

impl Default for EventQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct StopFlag(Arc<AtomicBool>);

impl StopFlag {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn is_stopped(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone)]
struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl TerminalDriver for RealTerminal {
    fn draw(&mut self, app: &App) -> Result<(), TaleError> {
        self.terminal
            .draw(|frame| ui::render(frame, app))
            .map(|_| ())
            .map_err(|error| TaleError::Terminal(error.to_string()))
    }

    fn restore(&mut self) -> Result<(), TaleError> {
        RealTerminal::restore(self)
    }
}

pub async fn run(app: &mut App, terminal: &mut RealTerminal) -> Result<(), TaleError> {
    run_with_driver(app, terminal).await
}

pub async fn run_with_driver<T: TerminalDriver>(
    app: &mut App,
    terminal: &mut T,
) -> Result<(), TaleError> {
    run_with_driver_and_queue(app, terminal, EventQueue::new()).await
}

pub async fn run_with_driver_and_queue<T: TerminalDriver>(
    app: &mut App,
    terminal: &mut T,
    queue: EventQueue,
) -> Result<(), TaleError> {
    let stop = StopFlag::new();
    let mut tasks: JoinSet<()> = JoinSet::new();
    let mut cancellations: HashMap<TaskId, CancelFlag> = HashMap::new();

    spawn_input_source(&mut tasks, queue.clone(), stop.clone());
    spawn_tick_source(&mut tasks, queue.clone(), stop.clone());
    spawn_signal_source(&mut tasks, queue.clone(), stop.clone());

    for effect in app.bootstrap_effects() {
        dispatch_effect(effect, &queue, &mut tasks, &mut cancellations);
    }

    let mut shutdown_requested = false;
    let mut final_error = None;
    loop {
        if app.render_invalidated() && !shutdown_requested {
            let render_result = terminal.draw(app);
            if let Err(error) = render_result {
                final_error = Some(error);
                let effects = app.update(Event::ShutdownRequested(ShutdownReason::RenderFailure));
                for effect in effects {
                    if matches!(effect, Effect::RequestShutdown) {
                        shutdown_requested = true;
                    }
                    dispatch_effect(effect, &queue, &mut tasks, &mut cancellations);
                }
            } else {
                app.clear_render_invalidated();
            }
        }

        if shutdown_requested {
            break;
        }

        let event = queue.recv().await;
        let effects = app.update(event);
        for effect in effects {
            if matches!(effect, Effect::RequestShutdown) {
                shutdown_requested = true;
            }
            dispatch_effect(effect, &queue, &mut tasks, &mut cancellations);
        }
    }

    stop.stop();
    for cancellation in cancellations.values() {
        cancellation.cancel();
    }
    let _ = tokio::time::timeout(grace_duration(), async {
        while tasks.join_next().await.is_some() {}
    })
    .await;
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}

    let restore_result = terminal.restore();
    if let Some(error) = final_error {
        return Err(error);
    }
    if let Some(error) = app.runtime_error.clone() {
        return Err(TaleError::Application(error));
    }
    restore_result
}

fn dispatch_effect(
    effect: Effect,
    queue: &EventQueue,
    tasks: &mut JoinSet<()>,
    cancellations: &mut HashMap<TaskId, CancelFlag>,
) {
    match effect {
        Effect::StartMockLoad {
            resource,
            generation,
            scenario,
        } => {
            if resource != Resource::Devices {
                return;
            }
            let queue = queue.clone();
            tasks.spawn(async move {
                queue
                    .send(Event::Source(SourceEvent::LoadStarted {
                        generation,
                        scenario,
                    }))
                    .await;
                tokio::time::sleep(Duration::from_millis(25)).await;
                match mock::load_devices(scenario) {
                    Ok((devices, observed_at)) => {
                        queue
                            .send(Event::Source(SourceEvent::LoadSucceeded {
                                generation,
                                devices,
                                observed_at,
                            }))
                            .await;
                    }
                    Err(detail) => {
                        queue
                            .send(Event::Source(SourceEvent::LoadFailed {
                                generation,
                                detail,
                            }))
                            .await;
                    }
                }
            });
        }
        Effect::StartMockTask { task_id, behavior } => {
            let queue = queue.clone();
            let cancellation = CancelFlag::new();
            cancellations.insert(task_id, cancellation.clone());
            tasks.spawn(async move {
                queue
                    .send(Event::Task(TaskEvent::Started { task_id }))
                    .await;
                match behavior {
                    MockTaskBehavior::DelayedSuccess => {
                        for completed in 1..=3 {
                            tokio::time::sleep(Duration::from_millis(30)).await;
                            if cancellation.is_cancelled() {
                                queue.send(cancelled_event(task_id)).await;
                                return;
                            }
                            queue
                                .send(Event::Task(TaskEvent::Progress {
                                    task_id,
                                    progress: Progress {
                                        completed,
                                        total: 3,
                                    },
                                    detail: format!("simulation step {completed}/3"),
                                }))
                                .await;
                        }
                        queue
                            .send(Event::Task(TaskEvent::Succeeded {
                                task_id,
                                finished_at: MOCK_NOW,
                                summary: "mock refresh completed".to_owned(),
                                detail: "fictional task completed successfully".to_owned(),
                            }))
                            .await;
                    }
                    MockTaskBehavior::DelayedFailure => {
                        tokio::time::sleep(Duration::from_millis(60)).await;
                        if cancellation.is_cancelled() {
                            queue.send(cancelled_event(task_id)).await;
                            return;
                        }
                        queue
                            .send(Event::Task(TaskEvent::Failed {
                                task_id,
                                finished_at: MOCK_NOW,
                                summary: "mock operation failed".to_owned(),
                                detail: "fictional failure detail: simulated timeout".to_owned(),
                            }))
                            .await;
                    }
                    MockTaskBehavior::CancellableLong => {
                        for completed in 1..=20 {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            if cancellation.is_cancelled() {
                                queue.send(cancelled_event(task_id)).await;
                                return;
                            }
                            queue
                                .send(Event::Task(TaskEvent::Progress {
                                    task_id,
                                    progress: Progress {
                                        completed,
                                        total: 20,
                                    },
                                    detail: format!("long simulation step {completed}/20"),
                                }))
                                .await;
                        }
                        queue
                            .send(Event::Task(TaskEvent::Succeeded {
                                task_id,
                                finished_at: MOCK_NOW,
                                summary: "long mock operation completed".to_owned(),
                                detail: "fictional cancellable task completed".to_owned(),
                            }))
                            .await;
                    }
                    MockTaskBehavior::NonCancellable => {
                        tokio::time::sleep(Duration::from_millis(120)).await;
                        queue
                            .send(Event::Task(TaskEvent::Succeeded {
                                task_id,
                                finished_at: MOCK_NOW,
                                summary: "non-cancellable simulation completed".to_owned(),
                                detail: "fictional non-cancellable task completed".to_owned(),
                            }))
                            .await;
                    }
                }
            });
        }
        Effect::CancelTask { task_id } => {
            if let Some(cancellation) = cancellations.get(&task_id) {
                cancellation.cancel();
            }
        }
        Effect::WriteConfigCandidate { .. } => {}
        Effect::RequestShutdown => {}
    }
}

fn cancelled_event(task_id: TaskId) -> Event {
    Event::Task(TaskEvent::Cancelled {
        task_id,
        finished_at: MOCK_NOW,
        detail: "fictional task cancelled".to_owned(),
    })
}

fn spawn_input_source(tasks: &mut JoinSet<()>, queue: EventQueue, stop: StopFlag) {
    tasks.spawn(async move {
        while !stop.is_stopped() {
            let read =
                tokio::task::spawn_blocking(|| match event::poll(Duration::from_millis(100)) {
                    Ok(true) => event::read().map(Some).map_err(|error| error.to_string()),
                    Ok(false) => Ok(None),
                    Err(error) => Err(error.to_string()),
                })
                .await;
            match read {
                Ok(Ok(Some(value))) => {
                    if let Some(input) = app_event::from_terminal_event(value) {
                        queue.send(Event::Input(input)).await;
                    }
                }
                Ok(Ok(None)) => {}
                Ok(Err(detail)) => {
                    queue
                        .send(Event::Source(SourceEvent::InputFailed(detail)))
                        .await;
                    return;
                }
                Err(error) => {
                    queue
                        .send(Event::Source(SourceEvent::InputFailed(error.to_string())))
                        .await;
                    return;
                }
            }
        }
    });
}

fn spawn_tick_source(tasks: &mut JoinSet<()>, queue: EventQueue, stop: StopFlag) {
    tasks.spawn(async move {
        let mut interval = tokio::time::interval(TICK_INTERVAL);
        while !stop.is_stopped() {
            interval.tick().await;
            queue.send(Event::Tick(Instant::now())).await;
        }
    });
}

fn spawn_signal_source(tasks: &mut JoinSet<()>, queue: EventQueue, stop: StopFlag) {
    tasks.spawn(async move {
        #[cfg(unix)]
        {
            let mut terminate = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(error) => {
                    queue.send(Event::Source(SourceEvent::InputFailed(error.to_string()))).await;
                    return;
                }
            };
            let mut interrupt = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()) {
                Ok(signal) => signal,
                Err(error) => {
                    queue.send(Event::Source(SourceEvent::InputFailed(error.to_string()))).await;
                    return;
                }
            };
            tokio::select! {
                _ = terminate.recv() => {},
                _ = interrupt.recv() => {},
                _ = async { while !stop.is_stopped() { tokio::time::sleep(Duration::from_millis(100)).await; } } => { return; }
            }
            queue.send(Event::ShutdownRequested(ShutdownReason::Signal)).await;
        }
        #[cfg(not(unix))]
        {
            let signal = tokio::signal::ctrl_c().await;
            if signal.is_ok() && !stop.is_stopped() {
                queue.send(Event::ShutdownRequested(ShutdownReason::Signal)).await;
            }
        }
    });
}
