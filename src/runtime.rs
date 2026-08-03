use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::event;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinSet;

use crate::action::ActionId;
use crate::app::App;
use crate::domain::account::LocalAccount;
use crate::domain::mutation::{LocalMutation, MutationResult};
use crate::domain::preference::{LocalPreferences, PreferenceRequest};
use crate::domain::redaction::Redactor;
use crate::domain::route::{
    AdvertisementRequest, ExitNodeRequest, ExitNodeSelection, canonical_routes,
    format_static_endpoints, parse_route_set, parse_static_endpoints,
};
use crate::domain::source::{LocalFailure, LocalFailureKind, LocalSnapshot, LocalState};
use crate::effect::{Effect, Resource};
use crate::error::TaleError;
use crate::event::LocalEvent;
use crate::event::{self as app_event, Event, InputEvent, ShutdownReason, SourceEvent, TaskEvent};
use crate::local::client::{self, LocalClient};
use crate::local::diagnostics;
use crate::local::process::{self, Cancellation, LocalProcessError, OutputStream, ProcessLine};
use crate::local::{accounts, handoff, policy};
use crate::mock::{self, MOCK_NOW, MockTaskBehavior};
use crate::task::{Progress, TaskId, grace_duration};
use crate::terminal::RealTerminal;
use crate::ui;

const EVENT_CAPACITY: usize = 256;
const TICK_INTERVAL: Duration = Duration::from_millis(100);

pub trait TerminalDriver {
    fn draw(&mut self, app: &App) -> Result<(), TaleError>;
    fn restore(&mut self) -> Result<(), TaleError>;

    fn suspend_for_handoff(&mut self) -> Result<(), TaleError> {
        Ok(())
    }

    fn resume_after_handoff(&mut self) -> Result<(), TaleError> {
        Ok(())
    }
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

    fn suspend_for_handoff(&mut self) -> Result<(), TaleError> {
        RealTerminal::suspend_for_handoff(self)
    }

    fn resume_after_handoff(&mut self) -> Result<(), TaleError> {
        RealTerminal::resume_after_handoff(self)
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
    let mut cancellations: HashMap<TaskId, Cancellation> = HashMap::new();
    let mut local_status_cancellation: Option<Cancellation> = None;
    let mut local_discovery_cancellation: Option<Cancellation> = None;
    let mut mutation_cancellations: HashMap<u64, Cancellation> = HashMap::new();
    let handoff_input_gate = Arc::new(AtomicBool::new(true));
    let mut terminal_suspended = false;

    spawn_input_source(
        &mut tasks,
        queue.clone(),
        stop.clone(),
        handoff_input_gate.clone(),
    );
    spawn_tick_source(&mut tasks, queue.clone(), stop.clone());
    spawn_signal_source(&mut tasks, queue.clone(), stop.clone());

    let mut shutdown_requested = false;
    let mut final_error = None;
    {
        let mut dispatch_context = DispatchContext {
            queue: &queue,
            tasks: &mut tasks,
            cancellations: &mut cancellations,
            local_status_cancellation: &mut local_status_cancellation,
            local_discovery_cancellation: &mut local_discovery_cancellation,
            mutation_cancellations: &mut mutation_cancellations,
            terminal,
            handoff_input_gate: &handoff_input_gate,
            terminal_suspended: &mut terminal_suspended,
        };

        for effect in app.bootstrap_effects() {
            dispatch_effect(effect, &mut dispatch_context);
        }

        loop {
            if app.render_invalidated()
                && !shutdown_requested
                && !*dispatch_context.terminal_suspended
            {
                let render_result = dispatch_context.terminal.draw(app);
                if let Err(error) = render_result {
                    final_error = Some(error);
                    let effects =
                        app.update(Event::ShutdownRequested(ShutdownReason::RenderFailure));
                    for effect in effects {
                        if matches!(effect, Effect::RequestShutdown) {
                            shutdown_requested = true;
                        }
                        dispatch_effect(effect, &mut dispatch_context);
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
                dispatch_effect(effect, &mut dispatch_context);
            }
        }
    }
    stop.stop();
    for cancellation in cancellations.values() {
        cancellation.cancel();
    }
    if let Some(cancellation) = local_status_cancellation {
        cancellation.cancel();
    }
    if let Some(cancellation) = local_discovery_cancellation {
        cancellation.cancel();
    }
    for cancellation in mutation_cancellations.values() {
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

struct DispatchContext<'a, T: TerminalDriver> {
    queue: &'a EventQueue,
    tasks: &'a mut JoinSet<()>,
    cancellations: &'a mut HashMap<TaskId, Cancellation>,
    local_status_cancellation: &'a mut Option<Cancellation>,
    local_discovery_cancellation: &'a mut Option<Cancellation>,
    mutation_cancellations: &'a mut HashMap<u64, Cancellation>,
    terminal: &'a mut T,
    handoff_input_gate: &'a Arc<AtomicBool>,
    terminal_suspended: &'a mut bool,
}

fn dispatch_effect<T: TerminalDriver>(effect: Effect, context: &mut DispatchContext<'_, T>) {
    let queue = context.queue;
    let tasks = &mut *context.tasks;
    let cancellations = &mut *context.cancellations;
    let local_status_cancellation = &mut *context.local_status_cancellation;
    let local_discovery_cancellation = &mut *context.local_discovery_cancellation;
    let mutation_cancellations = &mut *context.mutation_cancellations;
    let terminal = &mut *context.terminal;
    let handoff_input_gate = context.handoff_input_gate;
    let terminal_suspended = &mut *context.terminal_suspended;
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
            let cancellation = Cancellation::new();
            cancellations.insert(task_id, cancellation.clone());
            tasks.spawn(async move {
                queue
                    .send(Event::Task(Box::new(TaskEvent::Started { task_id })))
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
                                .send(Event::Task(Box::new(TaskEvent::Progress {
                                    task_id,
                                    progress: Progress {
                                        completed,
                                        total: 3,
                                    },
                                    detail: format!("simulation step {completed}/3"),
                                })))
                                .await;
                        }
                        queue
                            .send(Event::Task(Box::new(TaskEvent::Succeeded {
                                task_id,
                                finished_at: MOCK_NOW,
                                summary: "mock refresh completed".to_owned(),
                                detail: "fictional task completed successfully".to_owned(),
                            })))
                            .await;
                    }
                    MockTaskBehavior::DelayedFailure => {
                        tokio::time::sleep(Duration::from_millis(60)).await;
                        if cancellation.is_cancelled() {
                            queue.send(cancelled_event(task_id)).await;
                            return;
                        }
                        queue
                            .send(Event::Task(Box::new(TaskEvent::Failed {
                                task_id,
                                finished_at: MOCK_NOW,
                                summary: "mock operation failed".to_owned(),
                                detail: "fictional failure detail: simulated timeout".to_owned(),
                            })))
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
                                .send(Event::Task(Box::new(TaskEvent::Progress {
                                    task_id,
                                    progress: Progress {
                                        completed,
                                        total: 20,
                                    },
                                    detail: format!("long simulation step {completed}/20"),
                                })))
                                .await;
                        }
                        queue
                            .send(Event::Task(Box::new(TaskEvent::Succeeded {
                                task_id,
                                finished_at: MOCK_NOW,
                                summary: "long mock operation completed".to_owned(),
                                detail: "fictional cancellable task completed".to_owned(),
                            })))
                            .await;
                    }
                    MockTaskBehavior::NonCancellable => {
                        tokio::time::sleep(Duration::from_millis(120)).await;
                        queue
                            .send(Event::Task(Box::new(TaskEvent::Succeeded {
                                task_id,
                                finished_at: MOCK_NOW,
                                summary: "non-cancellable simulation completed".to_owned(),
                                detail: "fictional non-cancellable task completed".to_owned(),
                            })))
                            .await;
                    }
                }
            });
        }
        Effect::StartLocalDiscovery {
            generation,
            resolution,
            timeout,
        } => {
            let queue = queue.clone();
            let cancellation = Cancellation::new();
            *local_discovery_cancellation = Some(cancellation.clone());
            tasks.spawn(async move {
                queue
                    .send(local_event(LocalEvent::DiscoveryStarted { generation }))
                    .await;
                match client::resolve_executable(&resolution) {
                    Ok(resolved) => {
                        match LocalClient::discover(resolved, timeout, &cancellation).await {
                            Ok(executable) => {
                                queue
                                    .send(local_event(LocalEvent::DiscoverySucceeded {
                                        generation,
                                        executable,
                                    }))
                                    .await;
                            }
                            Err(error) => {
                                queue
                                    .send(local_event(LocalEvent::DiscoveryFailed {
                                        generation,
                                        failure: error.failure(),
                                    }))
                                    .await;
                            }
                        }
                    }
                    Err(error) => {
                        let failure = match error {
                            client::ExecutableError::NotFound => LocalFailure::new(
                                LocalFailureKind::ExecutableMissing,
                                "executable discovery",
                                "tailscale executable missing",
                                "tailscale was not found on the configured path",
                                false,
                            ),
                            client::ExecutableError::PermissionDenied => LocalFailure::new(
                                LocalFailureKind::ExecutableDenied,
                                "executable discovery",
                                "tailscale executable permission denied",
                                "check the executable permissions outside Tale",
                                false,
                            ),
                            client::ExecutableError::InvalidPath => LocalFailure::new(
                                LocalFailureKind::Transport,
                                "executable discovery",
                                "tailscale executable path is invalid",
                                "the configured executable path is empty",
                                false,
                            ),
                        };
                        queue
                            .send(local_event(LocalEvent::DiscoveryFailed {
                                generation,
                                failure,
                            }))
                            .await;
                    }
                }
            });
        }
        Effect::StartLocalStatus {
            generation,
            executable,
            timeout,
        } => {
            if let Some(previous) = local_status_cancellation.take() {
                previous.cancel();
            }
            let queue = queue.clone();
            let cancellation = Cancellation::new();
            *local_status_cancellation = Some(cancellation.clone());
            tasks.spawn(async move {
                let attempted_at = crate::local::now();
                queue
                    .send(local_event(LocalEvent::StatusStarted {
                        generation,
                        attempted_at,
                    }))
                    .await;
                let client = LocalClient::new(executable.clone(), timeout);
                match client.status(crate::local::now(), &cancellation).await {
                    Ok(snapshot) => {
                        queue
                            .send(local_event(LocalEvent::StatusSucceeded {
                                generation,
                                snapshot: Box::new(snapshot),
                            }))
                            .await;
                    }
                    Err(error) => {
                        queue
                            .send(local_event(LocalEvent::StatusFailed {
                                generation,
                                failure: error.failure(),
                            }))
                            .await;
                    }
                }
            });
        }
        Effect::StartLocalPreferences {
            executable,
            timeout,
        } => {
            let queue = queue.clone();
            tasks.spawn(async move {
                let client = LocalClient::new(executable, timeout);
                match client
                    .preferences(crate::local::now(), &Cancellation::new())
                    .await
                {
                    Ok(preferences) => {
                        queue
                            .send(local_event(LocalEvent::PreferencesSucceeded {
                                preferences: Box::new(preferences),
                            }))
                            .await;
                    }
                    Err(error) => {
                        queue
                            .send(local_event(LocalEvent::PreferencesFailed {
                                failure: error.failure(),
                            }))
                            .await;
                    }
                }
            });
        }
        Effect::StartLocalAccounts {
            executable,
            timeout,
        } => {
            let queue = queue.clone();
            tasks.spawn(async move {
                match accounts::list(&executable.path, timeout, &Cancellation::new()).await {
                    Ok(accounts) => {
                        queue
                            .send(local_event(LocalEvent::AccountsSucceeded { accounts }))
                            .await;
                    }
                    Err(error) => {
                        queue
                            .send(local_event(LocalEvent::AccountsFailed {
                                failure: LocalFailure::new(
                                    error.failure_kind(),
                                    "accounts",
                                    "local accounts unavailable",
                                    safe_operator_detail(&error.to_string()),
                                    error.retryable(),
                                ),
                            }))
                            .await;
                    }
                }
            });
        }
        Effect::StartLocalPolicy {
            executable,
            timeout,
        } => {
            let queue = queue.clone();
            tasks.spawn(async move {
                match policy::list(&executable.path, timeout, &Cancellation::new()).await {
                    Ok(entries) => {
                        queue
                            .send(local_event(LocalEvent::PolicySucceeded { entries }))
                            .await;
                    }
                    Err(error) => {
                        queue
                            .send(local_event(LocalEvent::PolicyFailed {
                                failure: LocalFailure::new(
                                    error.failure_kind(),
                                    "system policy",
                                    "system policy unavailable",
                                    safe_operator_detail(&error.to_string()),
                                    error.retryable(),
                                ),
                            }))
                            .await;
                    }
                }
            });
        }
        Effect::StartLocalMutation {
            mutation_id,
            task_id,
            executable,
            timeout,
            mutation,
        } => {
            let queue = queue.clone();
            let cancellation = Cancellation::new();
            cancellations.insert(task_id, cancellation.clone());
            mutation_cancellations.insert(mutation_id, cancellation.clone());
            tasks.spawn(async move {
                run_local_mutation(
                    queue,
                    task_id,
                    mutation_id,
                    executable,
                    timeout,
                    mutation,
                    cancellation,
                )
                .await;
            });
        }
        Effect::StartTerminalHandoff { task_id, command } => {
            if *terminal_suspended {
                let queue = queue.clone();
                tasks.spawn(async move {
                    queue
                        .send(local_event(LocalEvent::HandoffFinished {
                            task_id,
                            result: Err("another interactive child owns the terminal".to_owned()),
                        }))
                        .await;
                });
                return;
            }
            if let Err(error) = terminal.suspend_for_handoff() {
                let queue = queue.clone();
                let detail = error.to_string();
                tasks.spawn(async move {
                    queue
                        .send(local_event(LocalEvent::HandoffFinished {
                            task_id,
                            result: Err(detail),
                        }))
                        .await;
                });
                return;
            }
            *terminal_suspended = true;
            handoff_input_gate.store(false, Ordering::Release);
            let queue = queue.clone();
            tasks.spawn(async move {
                let result = handoff::run(command)
                    .await
                    .map_err(|error| error.to_string());
                queue
                    .send(local_event(LocalEvent::HandoffFinished { task_id, result }))
                    .await;
            });
        }
        Effect::ResumeTerminal => {
            if !*terminal_suspended {
                return;
            }
            match terminal.resume_after_handoff() {
                Ok(()) => {
                    *terminal_suspended = false;
                    handoff_input_gate.store(true, Ordering::Release);
                }
                Err(error) => {
                    handoff_input_gate.store(true, Ordering::Release);
                    let queue = queue.clone();
                    let detail = error.to_string();
                    tasks.spawn(async move {
                        queue
                            .send(local_event(LocalEvent::TerminalResumeFailed { detail }))
                            .await;
                    });
                }
            }
        }
        Effect::StartLocalDiagnostic {
            task_id,
            executable,
            timeout,
            request,
        } => {
            let queue = queue.clone();
            let cancellation = Cancellation::new();
            cancellations.insert(task_id, cancellation.clone());
            tasks.spawn(async move {
                queue
                    .send(Event::Task(Box::new(TaskEvent::Started { task_id })))
                    .await;
                run_local_diagnostic(queue, task_id, executable, timeout, request, cancellation)
                    .await;
            });
        }
        Effect::CancelLocalDiscovery => {
            if let Some(cancellation) = local_discovery_cancellation.as_ref() {
                cancellation.cancel();
            }
        }
        Effect::CancelLocalStatus => {
            if let Some(cancellation) = local_status_cancellation.as_ref() {
                cancellation.cancel();
            }
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
    Event::Task(Box::new(TaskEvent::Cancelled {
        task_id,
        finished_at: MOCK_NOW,
        detail: "fictional task cancelled".to_owned(),
    }))
}

async fn run_local_mutation(
    queue: EventQueue,
    task_id: TaskId,
    mutation_id: u64,
    executable: crate::domain::source::LocalExecutable,
    timeout: Duration,
    mutation: LocalMutation,
    cancellation: Cancellation,
) {
    let action_id = mutation.action_id();
    let client = LocalClient::new(executable.clone(), timeout);
    queue
        .send(Event::Task(Box::new(TaskEvent::Started { task_id })))
        .await;
    if cancellation.is_cancelled() {
        send_cancelled_before_dispatch(queue, task_id, mutation_id, action_id, mutation).await;
        return;
    }
    let command = match &mutation {
        LocalMutation::Connect => client::up_command(&executable.path, timeout),
        LocalMutation::Disconnect { accept_lose_ssh } => {
            client::down_command(&executable.path, timeout, *accept_lose_ssh)
        }
        LocalMutation::Preferences(request) => {
            match crate::local::preferences::set_command(&executable.path, timeout, request) {
                Ok(command) => command,
                Err(error) => {
                    send_mutation_result(
                        queue,
                        MutationCompletion {
                            task_id,
                            mutation_id,
                            action_id,
                            mutation,
                            result: MutationResult::CommandFailed {
                                summary: "preference request rejected".to_owned(),
                                detail: safe_operator_detail(&error.to_string()),
                                exit_status: None,
                            },
                            snapshot: None,
                            preferences: None,
                            accounts: None,
                            policy: None,
                        },
                    )
                    .await;
                    return;
                }
            }
        }
        LocalMutation::ExitNode(request) => {
            crate::local::preferences::exit_node_command(&executable.path, timeout, request)
        }
        LocalMutation::Advertisements(request) => {
            match crate::local::preferences::advertisement_command(
                &executable.path,
                timeout,
                request,
            ) {
                Ok(command) => command,
                Err(error) => {
                    send_mutation_result(
                        queue,
                        MutationCompletion {
                            task_id,
                            mutation_id,
                            action_id,
                            mutation,
                            result: MutationResult::CommandFailed {
                                summary: "advertisement request rejected".to_owned(),
                                detail: safe_operator_detail(&error.to_string()),
                                exit_status: None,
                            },
                            snapshot: None,
                            preferences: None,
                            accounts: None,
                            policy: None,
                        },
                    )
                    .await;
                    return;
                }
            }
        }
        LocalMutation::AccountSwitch { account_id } => {
            match accounts::switch_command(&executable.path, timeout, account_id) {
                Ok(command) => command,
                Err(error) => {
                    send_mutation_result(
                        queue,
                        MutationCompletion {
                            task_id,
                            mutation_id,
                            action_id,
                            mutation,
                            result: MutationResult::CommandFailed {
                                summary: "account switch request rejected".to_owned(),
                                detail: safe_operator_detail(&error.to_string()),
                                exit_status: None,
                            },
                            snapshot: None,
                            preferences: None,
                            accounts: None,
                            policy: None,
                        },
                    )
                    .await;
                    return;
                }
            }
        }
        LocalMutation::AccountRemove { account_id } => {
            match accounts::remove_command(&executable.path, timeout, account_id) {
                Ok(command) => command,
                Err(error) => {
                    send_mutation_result(
                        queue,
                        MutationCompletion {
                            task_id,
                            mutation_id,
                            action_id,
                            mutation,
                            result: MutationResult::CommandFailed {
                                summary: "account removal request rejected".to_owned(),
                                detail: safe_operator_detail(&error.to_string()),
                                exit_status: None,
                            },
                            snapshot: None,
                            preferences: None,
                            accounts: None,
                            policy: None,
                        },
                    )
                    .await;
                    return;
                }
            }
        }
        LocalMutation::SyspolicyReload => policy::reload_command(&executable.path, timeout),
    };
    if cancellation.is_cancelled() {
        send_cancelled_before_dispatch(queue, task_id, mutation_id, action_id, mutation).await;
        return;
    }
    let command_result = client.run_command(command, &cancellation).await;
    let command_status = match &command_result {
        Ok(result) => result.exit_status,
        Err(crate::local::client::ClientError::NonZero { status, .. }) => *status,
        Err(_) => None,
    };
    let successful_stderr = command_result.as_ref().ok().and_then(|result| {
        let stderr = String::from_utf8_lossy(&result.stderr);
        if stderr.trim().is_empty() {
            None
        } else {
            Some(safe_operator_detail(&stderr))
        }
    });
    let command_error = command_result.as_ref().err().map(mutation_error_detail);
    let uncertain_after_dispatch = matches!(
        command_result.as_ref(),
        Err(crate::local::client::ClientError::Process(
            crate::local::process::LocalProcessError::Cancelled
                | crate::local::process::LocalProcessError::TimedOut
        ))
    );

    let mut snapshot = None;
    let mut preferences = None;
    let mut account_values = None;
    let mut policy_values = None;
    let mut read_error = None;
    match &mutation {
        LocalMutation::Connect | LocalMutation::Disconnect { .. } => {
            match client
                .status(crate::local::now(), &Cancellation::new())
                .await
            {
                Ok(value) => snapshot = Some(Box::new(value)),
                Err(error) => read_error = Some(error.failure().detail),
            }
        }
        LocalMutation::Preferences(_)
        | LocalMutation::ExitNode(_)
        | LocalMutation::Advertisements(_) => {
            match client
                .preferences(crate::local::now(), &Cancellation::new())
                .await
            {
                Ok(value) => preferences = Some(Box::new(value)),
                Err(error) => read_error = Some(error.failure().detail),
            }
        }
        LocalMutation::AccountSwitch { .. } | LocalMutation::AccountRemove { .. } => {
            match accounts::list(&executable.path, timeout, &Cancellation::new()).await {
                Ok(value) => account_values = Some(value),
                Err(error) => read_error = Some(safe_operator_detail(&error.to_string())),
            }
            if read_error.is_none() {
                match client
                    .status(crate::local::now(), &Cancellation::new())
                    .await
                {
                    Ok(value) => snapshot = Some(Box::new(value)),
                    Err(error) => read_error = Some(error.failure().detail),
                }
            }
            if read_error.is_none() {
                match client
                    .preferences(crate::local::now(), &Cancellation::new())
                    .await
                {
                    Ok(value) => preferences = Some(Box::new(value)),
                    Err(error) => read_error = Some(error.failure().detail),
                }
            }
        }
        LocalMutation::SyspolicyReload => {
            match policy::list(&executable.path, timeout, &Cancellation::new()).await {
                Ok(value) => policy_values = Some(value),
                Err(error) => read_error = Some(safe_operator_detail(&error.to_string())),
            }
        }
    }

    let result = if let Some(command_detail) = command_error {
        if uncertain_after_dispatch {
            match read_error {
                Some(detail) => MutationResult::OutcomeUnknown {
                    summary: "command outcome unknown after interruption".to_owned(),
                    detail,
                    exit_status: command_status,
                },
                None => match verify_mutation(
                    &mutation,
                    snapshot.as_deref(),
                    preferences.as_deref(),
                    account_values.as_deref(),
                    policy_values.as_deref(),
                ) {
                    Ok(true) => MutationResult::Verified {
                        summary: "interrupted wait, fresh state verified".to_owned(),
                        detail: mutation_detail(
                            "the command wait ended, but fresh authoritative state matches",
                            successful_stderr.as_deref(),
                        ),
                        exit_status: command_status,
                    },
                    Ok(false) => MutationResult::VerificationMismatch {
                        summary: "fresh state did not match after interruption".to_owned(),
                        detail:
                            "fresh authoritative state determined that the request was not applied"
                                .to_owned(),
                        exit_status: command_status,
                    },
                    Err(detail) => MutationResult::OutcomeUnknown {
                        summary: "command outcome unknown after interruption".to_owned(),
                        detail,
                        exit_status: command_status,
                    },
                },
            }
        } else {
            MutationResult::CommandFailed {
                summary: "local command failed".to_owned(),
                detail: command_detail,
                exit_status: command_status,
            }
        }
    } else if let Some(detail) = read_error {
        MutationResult::ReadFailed {
            summary: "command ran but fresh verification failed".to_owned(),
            detail: mutation_detail(&detail, successful_stderr.as_deref()),
            exit_status: command_status,
        }
    } else {
        match verify_mutation(
            &mutation,
            snapshot.as_deref(),
            preferences.as_deref(),
            account_values.as_deref(),
            policy_values.as_deref(),
        ) {
            Ok(true) => MutationResult::Verified {
                summary: "command completed and state verified".to_owned(),
                detail: mutation_detail(
                    "fresh authoritative local state matches the submitted fields",
                    successful_stderr.as_deref(),
                ),
                exit_status: command_status,
            },
            Ok(false) => MutationResult::VerificationMismatch {
                summary: "fresh state did not match the request".to_owned(),
                detail: mutation_detail(
                    "the daemon or policy returned a different value; no retry was attempted",
                    successful_stderr.as_deref(),
                ),
                exit_status: command_status,
            },
            Err(detail) => MutationResult::ReadFailed {
                summary: "verification could not compare the request".to_owned(),
                detail: mutation_detail(&detail, successful_stderr.as_deref()),
                exit_status: command_status,
            },
        }
    };
    send_mutation_result(
        queue,
        MutationCompletion {
            task_id,
            mutation_id,
            action_id,
            mutation,
            result,
            snapshot,
            preferences,
            accounts: account_values,
            policy: policy_values,
        },
    )
    .await;
}

struct MutationCompletion {
    task_id: TaskId,
    mutation_id: u64,
    action_id: ActionId,
    mutation: LocalMutation,
    result: MutationResult,
    snapshot: Option<Box<LocalSnapshot>>,
    preferences: Option<Box<LocalPreferences>>,
    accounts: Option<Vec<LocalAccount>>,
    policy: Option<Vec<policy::SystemPolicyEntry>>,
}

async fn send_mutation_result(queue: EventQueue, completion: MutationCompletion) {
    queue
        .send(local_event(LocalEvent::MutationFinished {
            mutation_id: completion.mutation_id,
            task_id: completion.task_id,
            action_id: completion.action_id,
            mutation: completion.mutation,
            result: completion.result,
            snapshot: completion.snapshot,
            preferences: completion.preferences,
            accounts: completion.accounts,
            policy: completion.policy,
        }))
        .await;
}

async fn send_cancelled_before_dispatch(
    queue: EventQueue,
    task_id: TaskId,
    mutation_id: u64,
    action_id: ActionId,
    mutation: LocalMutation,
) {
    send_mutation_result(
        queue,
        MutationCompletion {
            task_id,
            mutation_id,
            action_id,
            mutation,
            result: MutationResult::CancelledBeforeDispatch {
                summary: "mutation cancelled before dispatch".to_owned(),
                detail: "no local command was started".to_owned(),
                exit_status: None,
            },
            snapshot: None,
            preferences: None,
            accounts: None,
            policy: None,
        },
    )
    .await;
}

fn verify_mutation(
    mutation: &LocalMutation,
    snapshot: Option<&LocalSnapshot>,
    preferences: Option<&LocalPreferences>,
    accounts: Option<&[LocalAccount]>,
    policy: Option<&[policy::SystemPolicyEntry]>,
) -> Result<bool, String> {
    match mutation {
        LocalMutation::Connect => Ok(snapshot.is_some_and(|snapshot| {
            matches!(
                snapshot.backend_state,
                LocalState::Running | LocalState::Degraded { .. }
            )
        })),
        LocalMutation::Disconnect { .. } => {
            Ok(snapshot
                .is_some_and(|snapshot| matches!(snapshot.backend_state, LocalState::Stopped)))
        }
        LocalMutation::Preferences(request) => {
            let preferences =
                preferences.ok_or_else(|| "preferences were not returned".to_owned())?;
            verify_preference_request(preferences, request)
        }
        LocalMutation::ExitNode(request) => {
            let preferences =
                preferences.ok_or_else(|| "preferences were not returned".to_owned())?;
            verify_exit_node(preferences, request)
        }
        LocalMutation::Advertisements(request) => {
            let preferences =
                preferences.ok_or_else(|| "preferences were not returned".to_owned())?;
            verify_advertisements(preferences, request)
        }
        LocalMutation::AccountSwitch { account_id } => {
            let accounts = accounts.ok_or_else(|| "accounts were not returned".to_owned())?;
            Ok(accounts
                .iter()
                .any(|account| account.active && account.id == *account_id))
        }
        LocalMutation::AccountRemove { account_id } => {
            let accounts = accounts.ok_or_else(|| "accounts were not returned".to_owned())?;
            Ok(!accounts.iter().any(|account| account.id == *account_id))
        }
        LocalMutation::SyspolicyReload => Ok(policy.is_some()),
    }
}

fn verify_preference_request(
    current: &LocalPreferences,
    request: &PreferenceRequest,
) -> Result<bool, String> {
    let checks = [
        (
            "accept DNS",
            request.accept_dns,
            current.accept_dns.value,
            current.accept_dns.can_edit(),
        ),
        (
            "accept routes",
            request.accept_routes,
            current.accept_routes.value,
            current.accept_routes.can_edit(),
        ),
        (
            "shields up",
            request.shields_up,
            current.shields_up.value,
            current.shields_up.can_edit(),
        ),
        (
            "Tailscale SSH",
            request.ssh,
            current.ssh.value,
            current.ssh.can_edit(),
        ),
        (
            "automatic update",
            request.automatic_update,
            current.automatic_update.value,
            current.automatic_update.can_edit(),
        ),
        (
            "update check",
            request.update_check,
            current.update_check.value,
            current.update_check.can_edit(),
        ),
        (
            "posture reporting",
            request.report_posture,
            current.report_posture.value,
            current.report_posture.can_edit(),
        ),
        (
            "web client",
            request.web_client,
            current.web_client.value,
            current.web_client.can_edit(),
        ),
    ];
    for (name, requested, actual, editable) in checks {
        if requested.is_some() && !editable {
            return Err(format!("{name} is unknown or not editable"));
        }
        if requested.is_some() && requested != actual {
            return Ok(false);
        }
    }
    if let Some(value) = request.hostname.as_ref() {
        if !current.hostname.can_edit() {
            return Err("hostname is unknown or not editable".to_owned());
        }
        if current.hostname.value.as_ref() != Some(value) {
            return Ok(false);
        }
    }
    if let Some(value) = request.nickname.as_ref() {
        if !current.nickname.can_edit() {
            return Err("nickname is unknown or not editable".to_owned());
        }
        if current.nickname.value.as_ref() != Some(value) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn verify_exit_node(current: &LocalPreferences, request: &ExitNodeRequest) -> Result<bool, String> {
    if current.auto_exit_node.value.is_none()
        || current.exit_node_id.value.is_none()
        || current.exit_node_ip.value.is_none()
        || current.exit_node_allow_lan_access.value.is_none()
    {
        return Err("exit-node preferences were not fully returned".to_owned());
    }
    let selected = match &request.selection {
        ExitNodeSelection::None => {
            current.auto_exit_node.value == Some(false)
                && current
                    .exit_node_id
                    .value
                    .as_deref()
                    .is_none_or(str::is_empty)
                && current
                    .exit_node_ip
                    .value
                    .as_deref()
                    .is_none_or(str::is_empty)
        }
        ExitNodeSelection::AutoAny => current.auto_exit_node.value == Some(true),
        ExitNodeSelection::Device { device_id, target } => {
            if let Some(current_id) = current.exit_node_id.value.as_deref()
                && !current_id.is_empty()
            {
                current_id == device_id.0.as_str()
            } else if let Some(current_ip) = current.exit_node_ip.value.as_deref()
                && !current_ip.is_empty()
            {
                current_ip == target.as_str()
            } else {
                false
            }
        }
    };
    Ok(selected
        && current.exit_node_allow_lan_access.value
            == Some(
                request.allow_lan_access && !matches!(&request.selection, ExitNodeSelection::None),
            ))
}

fn verify_advertisements(
    current: &LocalPreferences,
    request: &AdvertisementRequest,
) -> Result<bool, String> {
    if let Some(routes) = request.canonical_routes() {
        let actual = current
            .advertised_routes
            .value
            .as_ref()
            .ok_or_else(|| "advertised routes were not returned".to_owned())?;
        let actual = parse_route_set(&actual.join(","))
            .map_err(|error| format!("advertised routes were invalid: {error}"))?;
        if canonical_routes(&actual) != canonical_routes(&routes) {
            return Ok(false);
        }
    }
    if let Some(value) = request.advertise_exit_node
        && current.advertised_exit_node.value != Some(value)
    {
        return Ok(false);
    }
    if let Some(value) = request.advertise_connector
        && current.app_connector.value != Some(value)
    {
        return Ok(false);
    }
    if let Some(port) = request.relay_server_port {
        match port {
            Some(value) if current.relay_server_port.value != Some(value) => return Ok(false),
            None if current.relay_server_port_disabled.value != Some(true) => return Ok(false),
            _ => {}
        }
    }
    if let Some(endpoints) = request.relay_server_static_endpoints.as_deref() {
        let actual = current
            .relay_server_static_endpoints
            .value
            .as_ref()
            .ok_or_else(|| "relay endpoints were not returned".to_owned())?;
        let actual = parse_static_endpoints(&actual.join(","))
            .map_err(|error| format!("relay endpoints were invalid: {error}"))?;
        if format_static_endpoints(&actual) != format_static_endpoints(endpoints) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn local_event(event: LocalEvent) -> Event {
    Event::Local(Box::new(event))
}

async fn run_local_diagnostic(
    queue: EventQueue,
    task_id: TaskId,
    executable: crate::domain::source::LocalExecutable,
    timeout: Duration,
    request: diagnostics::DiagnosticRequest,
    cancellation: Cancellation,
) {
    match request {
        diagnostics::DiagnosticRequest::Ping { target } => {
            let command = diagnostics::ping_command(&executable.path, timeout, &target);
            run_streaming_diagnostic(
                queue,
                task_id,
                diagnostics::DiagnosticRequest::Ping { target },
                command,
                cancellation,
            )
            .await;
        }
        diagnostics::DiagnosticRequest::Netcheck { live } => {
            let command = diagnostics::netcheck_command(
                &executable.path,
                if live { None } else { Some(timeout) },
                live,
            );
            run_streaming_diagnostic(
                queue,
                task_id,
                diagnostics::DiagnosticRequest::Netcheck { live },
                command,
                cancellation,
            )
            .await;
        }
        diagnostics::DiagnosticRequest::DnsStatus => {
            let command = diagnostics::dns_status_command(&executable.path, timeout);
            run_collected_diagnostic(
                queue,
                task_id,
                diagnostics::DiagnosticRequest::DnsStatus,
                command,
                cancellation,
            )
            .await;
        }
        diagnostics::DiagnosticRequest::DnsQuery { name, record_type } => {
            let command =
                diagnostics::dns_query_command(&executable.path, timeout, &name, record_type);
            run_collected_diagnostic(
                queue,
                task_id,
                diagnostics::DiagnosticRequest::DnsQuery { name, record_type },
                command,
                cancellation,
            )
            .await;
        }
        diagnostics::DiagnosticRequest::Whois { target, protocol } => {
            let command = diagnostics::whois_command(&executable.path, timeout, &target, protocol);
            run_collected_diagnostic(
                queue,
                task_id,
                diagnostics::DiagnosticRequest::Whois { target, protocol },
                command,
                cancellation,
            )
            .await;
        }
    }
}

async fn run_streaming_diagnostic(
    queue: EventQueue,
    task_id: TaskId,
    request: diagnostics::DiagnosticRequest,
    command: process::LocalCommand,
    cancellation: Cancellation,
) {
    let (sender, receiver) = mpsc::channel::<ProcessLine>(128);
    let process = process::run_lines(command, &cancellation, sender);
    tokio::pin!(process);
    let mut receiver = Some(receiver);
    let mut stream = StreamAccumulator::new();
    let process_result = loop {
        if let Some(receiver_ref) = receiver.as_mut() {
            tokio::select! {
                line = receiver_ref.recv() => {
                    match line {
                        Some(line) => handle_stream_line(
                            &queue,
                            task_id,
                            &request,
                            line,
                            &mut stream,
                        ).await,
                        None => receiver = None,
                    }
                }
                result = &mut process => break result,
            }
        } else {
            break process.await;
        }
    };
    let process_result = match process_result {
        Ok(result) => result,
        Err(error) => {
            finish_diagnostic_error(queue, task_id, error).await;
            return;
        }
    };
    if process_result.result.exit_status != Some(0) {
        let detail = append_bounded(
            &stream.detail,
            &bounded_process_output(&process_result.result.stderr),
        );
        finish_diagnostic_failure(queue, task_id, "diagnostic command failed", &detail).await;
        return;
    }
    if process_result.invalid_utf8 {
        finish_diagnostic_failure(
            queue,
            task_id,
            "diagnostic output was not UTF-8",
            &stream.detail,
        )
        .await;
        return;
    }
    match request {
        diagnostics::DiagnosticRequest::Ping { .. } => {
            let summary = diagnostics::summarize_ping(Some(10), &stream.ping_samples);
            let summary_text = if stream.ping_samples.is_empty() {
                "succeeded with unparsed output".to_owned()
            } else {
                format_ping_summary(&summary)
            };
            queue
                .send(local_event(LocalEvent::DiagnosticResult {
                    task_id,
                    result: crate::domain::diagnostic::DiagnosticResult::Ping(summary),
                }))
                .await;
            finish_diagnostic_success(queue, task_id, &summary_text, &stream.detail).await;
        }
        diagnostics::DiagnosticRequest::Netcheck { .. } => {
            let (observation, errors) =
                diagnostics::parse_netcheck_lines(&stream.netcheck_lines, crate::local::now());
            for error in errors {
                stream.detail = append_bounded(&stream.detail, &error);
            }
            let Some(observation) = observation else {
                finish_diagnostic_failure(
                    queue,
                    task_id,
                    "netcheck returned no decodable observations",
                    &stream.detail,
                )
                .await;
                return;
            };
            let summary = format_netcheck_summary(&observation);
            queue
                .send(local_event(LocalEvent::DiagnosticResult {
                    task_id,
                    result: crate::domain::diagnostic::DiagnosticResult::Netcheck(observation),
                }))
                .await;
            finish_diagnostic_success(queue, task_id, &summary, &stream.detail).await;
        }
        _ => {}
    }
}

struct StreamAccumulator {
    ping_samples: Vec<crate::domain::diagnostic::PingSample>,
    netcheck_lines: Vec<String>,
    detail: String,
    sequence: u64,
}

impl StreamAccumulator {
    const fn new() -> Self {
        Self {
            ping_samples: Vec::new(),
            netcheck_lines: Vec::new(),
            detail: String::new(),
            sequence: 0,
        }
    }
}

async fn handle_stream_line(
    queue: &EventQueue,
    task_id: TaskId,
    request: &diagnostics::DiagnosticRequest,
    line: ProcessLine,
    stream: &mut StreamAccumulator,
) {
    let text = match std::str::from_utf8(&line.bytes) {
        Ok(value) => value.trim_end_matches(['\r', '\n']).to_owned(),
        Err(_) => {
            stream.detail = append_bounded(
                &stream.detail,
                &format!("{}: non-UTF-8 output", stream_label(line.stream)),
            );
            return;
        }
    };
    if line.stream == OutputStream::Stderr {
        stream.detail = append_bounded(&stream.detail, &format!("stderr: {text}"));
        return;
    }
    match request {
        diagnostics::DiagnosticRequest::Ping { .. } => {
            stream.sequence = stream.sequence.saturating_add(1);
            let sample = diagnostics::parse_ping_line(&text, stream.sequence, crate::local::now());
            if let Some(sample) = sample.as_ref() {
                stream.ping_samples.push(sample.clone());
            } else {
                stream.detail = append_bounded(&stream.detail, &text);
            }
            let completed =
                u16::try_from(stream.ping_samples.len()).map_or(u16::MAX, |value| value.min(10));
            queue
                .send(local_event(LocalEvent::DiagnosticProgress {
                    task_id,
                    progress: Progress {
                        completed,
                        total: 10,
                    },
                    detail: text,
                    sample,
                    netcheck: None,
                }))
                .await;
        }
        diagnostics::DiagnosticRequest::Netcheck { .. } => {
            stream.netcheck_lines.push(text.clone());
            let observation =
                diagnostics::parse_netcheck_lines(std::slice::from_ref(&text), crate::local::now())
                    .0;
            if observation.is_none() {
                stream.detail = append_bounded(&stream.detail, &text);
            }
            let completed =
                u16::try_from(stream.netcheck_lines.len()).map_or(u16::MAX, |value| value);
            queue
                .send(local_event(LocalEvent::DiagnosticProgress {
                    task_id,
                    progress: Progress {
                        completed,
                        total: 0,
                    },
                    detail: text,
                    sample: None,
                    netcheck: observation,
                }))
                .await;
        }
        _ => {}
    }
}

async fn run_collected_diagnostic(
    queue: EventQueue,
    task_id: TaskId,
    request: diagnostics::DiagnosticRequest,
    command: process::LocalCommand,
    cancellation: Cancellation,
) {
    let result = match process::run(command, &cancellation).await {
        Ok(result) => result,
        Err(error) => {
            finish_diagnostic_error(queue, task_id, error).await;
            return;
        }
    };
    let raw_detail = bounded_process_output(&result.stdout);
    if result.exit_status != Some(0) {
        finish_diagnostic_failure(
            queue,
            task_id,
            "diagnostic command failed",
            &append_bounded(&raw_detail, &bounded_process_output(&result.stderr)),
        )
        .await;
        return;
    }
    let input = match process::decode_utf8(&result.stdout) {
        Ok(input) => input,
        Err(error) => {
            finish_diagnostic_failure(
                queue,
                task_id,
                "diagnostic output was not UTF-8",
                &error.to_string(),
            )
            .await;
            return;
        }
    };
    let observed_at = crate::local::now();
    let parsed = match request {
        diagnostics::DiagnosticRequest::DnsStatus => {
            diagnostics::parse_dns_status(input, observed_at)
                .map(crate::domain::diagnostic::DiagnosticResult::DnsStatus)
        }
        diagnostics::DiagnosticRequest::DnsQuery { name, record_type } => {
            diagnostics::parse_dns_query(input, name, record_type, observed_at)
                .map(crate::domain::diagnostic::DiagnosticResult::DnsQuery)
        }
        diagnostics::DiagnosticRequest::Whois { target, .. } => {
            diagnostics::parse_whois(input, target, observed_at)
                .map(crate::domain::diagnostic::DiagnosticResult::Whois)
        }
        _ => Err("unexpected streaming diagnostic".to_owned()),
    };
    match parsed {
        Ok(result) => {
            let summary = diagnostic_result_summary(&result);
            queue
                .send(local_event(LocalEvent::DiagnosticResult {
                    task_id,
                    result,
                }))
                .await;
            finish_diagnostic_success(queue, task_id, &summary, &raw_detail).await;
        }
        Err(error) => {
            finish_diagnostic_failure(queue, task_id, "unsupported diagnostic output", &error).await
        }
    }
}

async fn finish_diagnostic_error(queue: EventQueue, task_id: TaskId, error: LocalProcessError) {
    if error == LocalProcessError::Cancelled {
        queue
            .send(Event::Task(Box::new(TaskEvent::Cancelled {
                task_id,
                finished_at: crate::local::now(),
                detail: "diagnostic cancelled".to_owned(),
            })))
            .await;
    } else {
        finish_diagnostic_failure(
            queue,
            task_id,
            "diagnostic process failed",
            &error.to_string(),
        )
        .await;
    }
}

async fn finish_diagnostic_success(
    queue: EventQueue,
    task_id: TaskId,
    summary: &str,
    detail: &str,
) {
    queue
        .send(Event::Task(Box::new(TaskEvent::Succeeded {
            task_id,
            finished_at: crate::local::now(),
            summary: summary.to_owned(),
            detail: bounded_diagnostic_detail(detail),
        })))
        .await;
}

async fn finish_diagnostic_failure(
    queue: EventQueue,
    task_id: TaskId,
    summary: &str,
    detail: &str,
) {
    queue
        .send(Event::Task(Box::new(TaskEvent::Failed {
            task_id,
            finished_at: crate::local::now(),
            summary: summary.to_owned(),
            detail: bounded_diagnostic_detail(detail),
        })))
        .await;
}

fn diagnostic_result_summary(result: &crate::domain::diagnostic::DiagnosticResult) -> String {
    match result {
        crate::domain::diagnostic::DiagnosticResult::DnsStatus(value) => {
            format!("DNS status: {} resolvers", value.resolvers.len())
        }
        crate::domain::diagnostic::DiagnosticResult::DnsQuery(value) => {
            format!("DNS {}: {} answers", value.record_type, value.answers.len())
        }
        crate::domain::diagnostic::DiagnosticResult::Whois(value) => {
            format!(
                "whois: id={} name={} addresses={} tags={} user={} capabilities={}",
                value.machine_id.as_deref().map_or("unknown", |id| id),
                value.machine_name.as_deref().map_or("unknown", |name| name),
                value.addresses.len(),
                value.tags.len(),
                value
                    .user_identity
                    .as_deref()
                    .map_or("unknown", |user| user),
                value.capabilities.len(),
            )
        }
        crate::domain::diagnostic::DiagnosticResult::Ping(value) => format_ping_summary(value),
        crate::domain::diagnostic::DiagnosticResult::Netcheck(value) => {
            format_netcheck_summary(value)
        }
    }
}

fn format_ping_summary(value: &crate::domain::diagnostic::PingSummary) -> String {
    format!(
        "ping: received={} loss={} min={}ms avg={}ms max={}ms path={} direct={}",
        value.received,
        value
            .loss_percent
            .map_or_else(|| "not returned".to_owned(), |value| format!("{value}%")),
        value
            .minimum_ms
            .map_or_else(|| "not returned".to_owned(), |value| value.to_string()),
        value
            .average_ms
            .map_or_else(|| "not returned".to_owned(), |value| value.to_string()),
        value
            .maximum_ms
            .map_or_else(|| "not returned".to_owned(), |value| value.to_string()),
        value
            .last_path
            .as_ref()
            .map_or("unknown", |path| path.label()),
        value.reached_direct,
    )
}

fn format_netcheck_summary(value: &crate::domain::diagnostic::NetcheckObservation) -> String {
    format!(
        "netcheck: udp={} ipv4={} ipv6={} nearest-derp={} regions={}",
        value
            .udp
            .map_or("not returned", |value| if value { "true" } else { "false" }),
        value
            .ipv4
            .map_or("not returned", |value| if value { "true" } else { "false" }),
        value
            .ipv6
            .map_or("not returned", |value| if value { "true" } else { "false" }),
        value
            .nearest_derp
            .as_deref()
            .map_or("not returned", |value| value),
        value.derp_latency.len(),
    )
}

fn bounded_process_output(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut redactor = Redactor::new();
    let redacted = redactor.text(&text);
    bounded_task_detail(&redacted)
}

fn safe_operator_detail(value: &str) -> String {
    let mut redactor = Redactor::new();
    let redacted = redactor
        .text(value)
        .split_whitespace()
        .map(|token| {
            if token.starts_with("http://") || token.starts_with("https://") {
                "[url redacted]"
            } else {
                token
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    crate::task::bounded_detail(&redacted, 4096)
}

fn mutation_error_detail(error: &crate::local::client::ClientError) -> String {
    let failure = error.failure();
    match error {
        crate::local::client::ClientError::NonZero { detail, .. } if !detail.trim().is_empty() => {
            format!(
                "{}; bounded stderr: {}",
                failure.detail,
                safe_operator_detail(detail)
            )
        }
        _ => failure.detail,
    }
}

fn mutation_detail(base: &str, stderr: Option<&str>) -> String {
    stderr.map_or_else(
        || base.to_owned(),
        |stderr| format!("{base}; bounded stderr: {stderr}"),
    )
}

fn bounded_diagnostic_detail(value: &str) -> String {
    let mut redactor = Redactor::new();
    let redacted = redactor.text(value);
    bounded_task_detail(&redacted)
}

fn bounded_task_detail(value: &str) -> String {
    const CAP: usize = 256 * 1024;
    crate::task::bounded_detail(value, CAP)
}

fn append_bounded(existing: &str, value: &str) -> String {
    if existing.is_empty() {
        return bounded_task_detail(value);
    }
    bounded_task_detail(&format!("{existing}\n{value}"))
}

fn stream_label(stream: OutputStream) -> &'static str {
    match stream {
        OutputStream::Stdout => "stdout",
        OutputStream::Stderr => "stderr",
    }
}

fn spawn_input_source(
    tasks: &mut JoinSet<()>,
    queue: EventQueue,
    stop: StopFlag,
    handoff_input_gate: Arc<AtomicBool>,
) {
    tasks.spawn(async move {
        while !stop.is_stopped() {
            if !handoff_input_gate.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(25)).await;
                continue;
            }
            let read =
                tokio::task::spawn_blocking(|| match event::poll(Duration::from_millis(100)) {
                    Ok(true) => event::read().map(Some).map_err(|error| error.to_string()),
                    Ok(false) => Ok(None),
                    Err(error) => Err(error.to_string()),
                })
                .await;
            match read {
                Ok(Ok(Some(value))) => {
                    if !handoff_input_gate.load(Ordering::Acquire) {
                        continue;
                    }
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
