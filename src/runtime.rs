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
use crate::domain::service::{
    FunnelStatus, ServeStatus, ServiceActionRequest, ServiceFailure, ServiceFailureKind,
    ServiceTaskData,
};
use crate::domain::source::{LocalFailure, LocalFailureKind, LocalSnapshot, LocalState};
use crate::domain::transfer::{TaildriveShare, TaildropTarget, validate_receive_directory};
use crate::effect::{Effect, Resource};
use crate::error::TaleError;
use crate::event::LocalEvent;
use crate::event::{self as app_event, Event, InputEvent, ShutdownReason, SourceEvent, TaskEvent};
use crate::local::client::{self, LocalClient};
use crate::local::diagnostics;
use crate::local::process::{
    self, Cancellation, LocalOperation, LocalProcessError, OutputStream, ProcessLine,
};
use crate::local::{accounts, handoff, policy};
use crate::local::{certificates, services, transfers};
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
    let mut local_services_refresh_cancellation: Option<Cancellation> = None;
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
            local_services_refresh_cancellation: &mut local_services_refresh_cancellation,
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
    if let Some(cancellation) = local_services_refresh_cancellation {
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
    local_services_refresh_cancellation: &'a mut Option<Cancellation>,
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
    let local_services_refresh_cancellation = &mut *context.local_services_refresh_cancellation;
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
        Effect::StartLocalServicesRefresh {
            generation,
            executable,
            timeout,
            alpha_enabled,
        } => {
            if let Some(previous) = local_services_refresh_cancellation.take() {
                previous.cancel();
            }
            let queue = queue.clone();
            let cancellation = Cancellation::new();
            *local_services_refresh_cancellation = Some(cancellation.clone());
            tasks.spawn(async move {
                run_services_refresh(
                    queue,
                    generation,
                    executable,
                    timeout,
                    alpha_enabled,
                    cancellation,
                )
                .await;
            });
        }
        Effect::StartServiceTask {
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
                let outcome = run_service_task(
                    executable,
                    timeout,
                    request,
                    cancellation,
                    queue.clone(),
                    task_id,
                )
                .await;
                queue
                    .send(Event::Services(Box::new(
                        app_event::ServicesEvent::TaskFinished {
                            task_id,
                            request: outcome.request,
                            result: outcome.result,
                            exit_status: outcome.exit_status,
                            stdout_truncated: outcome.stdout_truncated,
                            stderr_truncated: outcome.stderr_truncated,
                        },
                    )))
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

struct ServiceTaskOutcome {
    request: ServiceActionRequest,
    result: Result<ServiceTaskData, ServiceFailure>,
    exit_status: Option<i32>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

struct ServiceRunError {
    failure: ServiceFailure,
}

async fn run_services_refresh(
    queue: EventQueue,
    generation: u64,
    executable: crate::domain::source::LocalExecutable,
    timeout: Duration,
    alpha_enabled: bool,
    cancellation: Cancellation,
) {
    let serve_future = read_serve_status(&executable, timeout, &cancellation);
    let funnel_future = read_funnel_status(&executable, timeout, &cancellation);
    let targets_future = read_taildrop_targets(&executable, timeout, &cancellation);
    let drive_future = read_taildrive_shares(&executable, timeout, &cancellation, alpha_enabled);
    let (serve, funnel, taildrop_targets, taildrive) =
        tokio::join!(serve_future, funnel_future, targets_future, drive_future);
    queue
        .send(Event::Services(Box::new(
            app_event::ServicesEvent::RefreshFinished {
                generation,
                observed_at: crate::local::now(),
                command_version: executable.version,
                serve,
                funnel,
                taildrop_targets,
                taildrive,
            },
        )))
        .await;
}

async fn read_serve_status(
    executable: &crate::domain::source::LocalExecutable,
    timeout: Duration,
    cancellation: &Cancellation,
) -> Result<ServeStatus, ServiceFailure> {
    if !executable.capabilities.serve {
        return Err(unsupported_service(
            "serve status",
            "Serve is not advertised by this CLI",
        ));
    }
    let result = run_service_command(
        services::serve_status_command(&executable.path, timeout),
        cancellation,
    )
    .await
    .map_err(|error| error.failure)?;
    let output = process::decode_utf8(&result.stdout).map_err(|error| {
        annotate_service_failure(service_failure_from_process("serve status", error), &result)
    })?;
    services::parse_serve_status(output).map_err(|error| {
        annotate_service_failure(
            ServiceFailure::new(
                ServiceFailureKind::DecodeFailed,
                "serve status",
                "Serve status could not be decoded",
                format!(
                    "{} (CLI {})",
                    bounded_task_detail(&error.to_string()),
                    executable.version
                ),
            ),
            &result,
        )
    })
}

async fn read_funnel_status(
    executable: &crate::domain::source::LocalExecutable,
    timeout: Duration,
    cancellation: &Cancellation,
) -> Result<FunnelStatus, ServiceFailure> {
    if !executable.capabilities.funnel {
        return Err(unsupported_service(
            "funnel status",
            "Funnel is not advertised by this CLI",
        ));
    }
    let result = run_service_command(
        services::funnel_status_command(&executable.path, timeout),
        cancellation,
    )
    .await
    .map_err(|error| error.failure)?;
    let output = process::decode_utf8(&result.stdout).map_err(|error| {
        annotate_service_failure(
            service_failure_from_process("funnel status", error),
            &result,
        )
    })?;
    services::parse_funnel_status(output).map_err(|error| {
        annotate_service_failure(
            ServiceFailure::new(
                ServiceFailureKind::DecodeFailed,
                "funnel status",
                "Funnel status could not be decoded",
                format!(
                    "{} (CLI {})",
                    bounded_task_detail(&error.to_string()),
                    executable.version
                ),
            ),
            &result,
        )
    })
}

async fn read_taildrop_targets(
    executable: &crate::domain::source::LocalExecutable,
    timeout: Duration,
    cancellation: &Cancellation,
) -> Result<Vec<TaildropTarget>, ServiceFailure> {
    if !executable.capabilities.taildrop {
        return Err(unsupported_service(
            "file cp --targets",
            "Taildrop target discovery is not advertised by this CLI",
        ));
    }
    let result = run_service_command(
        transfers::taildrop_targets_command(&executable.path, timeout),
        cancellation,
    )
    .await
    .map_err(|error| error.failure)?;
    let output = process::decode_utf8(&result.stdout).map_err(|error| {
        annotate_service_failure(
            service_failure_from_process("file cp --targets", error),
            &result,
        )
    })?;
    transfers::parse_taildrop_targets(output).map_err(|error| {
        annotate_service_failure(
            ServiceFailure::new(
                ServiceFailureKind::DecodeFailed,
                "file cp --targets",
                "Taildrop targets could not be decoded",
                format!(
                    "{} (CLI {})",
                    bounded_task_detail(&error.to_string()),
                    executable.version
                ),
            ),
            &result,
        )
    })
}

async fn read_taildrive_shares(
    executable: &crate::domain::source::LocalExecutable,
    timeout: Duration,
    cancellation: &Cancellation,
    alpha_enabled: bool,
) -> Result<Vec<TaildriveShare>, ServiceFailure> {
    if !alpha_enabled {
        return Err(unsupported_service(
            "drive list",
            "Taildrive is alpha and is disabled for this run",
        ));
    }
    if !executable.capabilities.drive {
        return Err(unsupported_service(
            "drive list",
            "Taildrive is not advertised by this CLI",
        ));
    }
    let result = run_service_command(
        transfers::drive_list_command(&executable.path, timeout),
        cancellation,
    )
    .await
    .map_err(|error| error.failure)?;
    let output = process::decode_utf8(&result.stdout).map_err(|error| {
        annotate_service_failure(service_failure_from_process("drive list", error), &result)
    })?;
    transfers::parse_drive_list(output).map_err(|error| {
        annotate_service_failure(
            ServiceFailure::new(
                ServiceFailureKind::DecodeFailed,
                "drive list",
                "Taildrive shares could not be decoded",
                format!(
                    "{} (CLI {})",
                    bounded_task_detail(&error.to_string()),
                    executable.version
                ),
            ),
            &result,
        )
    })
}

async fn run_service_task(
    executable: crate::domain::source::LocalExecutable,
    timeout: Duration,
    request: ServiceActionRequest,
    cancellation: Cancellation,
    queue: EventQueue,
    task_id: TaskId,
) -> ServiceTaskOutcome {
    let action_request = request.clone();
    let mut outcome = ServiceTaskOutcome {
        request: action_request,
        result: Err(unsupported_service(
            "service task",
            "service task did not run",
        )),
        exit_status: None,
        stdout_truncated: false,
        stderr_truncated: false,
    };
    let result = match request {
        ServiceActionRequest::Serve { mapping, .. } => {
            if !executable.capabilities.serve
                || !executable
                    .capabilities
                    .supports_service_listener(&mapping.listener, false)
            {
                Err(unsupported_service(
                    "serve",
                    "Serve is not advertised by this CLI",
                ))
            } else {
                match services::mapping_command(&executable.path, timeout, &mapping, true) {
                    Ok(command) => {
                        let run = run_service_command(command, &cancellation).await;
                        outcome_from_run(run, |run| async {
                            verify_serve_mapping(&executable, timeout, &cancellation, mapping, run)
                                .await
                        })
                        .await
                    }
                    Err(error) => Err(ServiceFailure::new(
                        ServiceFailureKind::Unsupported,
                        "serve",
                        "Serve request is invalid",
                        error.to_string(),
                    )),
                }
            }
        }
        ServiceActionRequest::ServeReset => {
            if !executable.capabilities.serve {
                Err(unsupported_service(
                    "serve reset",
                    "Serve is not advertised by this CLI",
                ))
            } else {
                let run = run_service_command(
                    services::serve_reset_command(&executable.path, timeout),
                    &cancellation,
                )
                .await;
                outcome_from_run(run, |run| async {
                    verify_serve_reset(&executable, timeout, &cancellation, run).await
                })
                .await
            }
        }
        ServiceActionRequest::Funnel { mapping, .. } => {
            if !executable.capabilities.funnel
                || !executable
                    .capabilities
                    .supports_service_listener(&mapping.listener, true)
            {
                Err(unsupported_service(
                    "funnel",
                    "Funnel is not advertised by this CLI",
                ))
            } else {
                match services::mapping_command(&executable.path, timeout, &mapping, true) {
                    Ok(command) => {
                        let run = run_service_command(command, &cancellation).await;
                        outcome_from_run(run, |run| async {
                            verify_funnel_mapping(&executable, timeout, &cancellation, mapping, run)
                                .await
                        })
                        .await
                    }
                    Err(error) => Err(ServiceFailure::new(
                        ServiceFailureKind::Unsupported,
                        "funnel",
                        "Funnel request is invalid",
                        error.to_string(),
                    )),
                }
            }
        }
        ServiceActionRequest::FunnelReset => {
            if !executable.capabilities.funnel {
                Err(unsupported_service(
                    "funnel reset",
                    "Funnel is not advertised by this CLI",
                ))
            } else {
                let run = run_service_command(
                    services::funnel_reset_command(&executable.path, timeout),
                    &cancellation,
                )
                .await;
                outcome_from_run(run, |run| async {
                    verify_funnel_reset(&executable, timeout, &cancellation, run).await
                })
                .await
            }
        }
        ServiceActionRequest::TaildropSend(request) => {
            if !executable.capabilities.taildrop {
                Err(unsupported_service(
                    "file cp",
                    "Taildrop is not advertised by this CLI",
                ))
            } else {
                let files = request
                    .files
                    .iter()
                    .map(|file| file.path.clone())
                    .collect::<Vec<_>>();
                match transfers::taildrop_send_command(
                    &executable.path,
                    timeout,
                    &files,
                    &request.target.command_target,
                ) {
                    Ok(command) => {
                        if files.iter().any(|path| {
                            std::fs::metadata(path)
                                .map(|metadata| !metadata.is_file())
                                .unwrap_or(true)
                        }) {
                            Err(ServiceFailure::new(
                                ServiceFailureKind::CommandFailed,
                                "file cp",
                                "Taildrop file selection is no longer valid",
                                "one or more selected files are missing or no longer regular files",
                            ))
                        } else {
                            let (run, filenames) =
                                run_transfer_command(command, &cancellation, &queue, task_id).await;
                            match run {
                                Ok(run) => Ok((
                                    ServiceTaskData::Transfer {
                                        summary: format!(
                                            "Taildrop send completed to {}",
                                            request.target.display_name
                                        ),
                                        filenames: if filenames.is_empty() {
                                            files
                                                .iter()
                                                .filter_map(|path| {
                                                    path.file_name().map(|name| {
                                                        name.to_string_lossy().into_owned()
                                                    })
                                                })
                                                .collect()
                                        } else {
                                            filenames
                                        },
                                    },
                                    run,
                                )),
                                Err(error) => Err(error.failure),
                            }
                        }
                    }
                    Err(error) => Err(ServiceFailure::new(
                        ServiceFailureKind::CommandFailed,
                        "file cp",
                        "Taildrop send request is invalid",
                        error.to_string(),
                    )),
                }
            }
        }
        ServiceActionRequest::TaildropReceive(request) => {
            if !executable.capabilities.taildrop {
                Err(unsupported_service(
                    "file get",
                    "Taildrop is not advertised by this CLI",
                ))
            } else if let Err(error) = validate_receive_directory(&request.directory) {
                Err(ServiceFailure::new(
                    ServiceFailureKind::PermissionDenied,
                    "file get",
                    "Taildrop destination is unavailable",
                    error,
                ))
            } else {
                match transfers::taildrop_receive_command(
                    &executable.path,
                    timeout,
                    &request.directory,
                    request.conflict,
                    request.wait,
                ) {
                    Ok(command) => {
                        let (run, filenames) =
                            run_transfer_command(command, &cancellation, &queue, task_id).await;
                        match run {
                            Ok(run) => Ok((
                                ServiceTaskData::Transfer {
                                    summary: format!(
                                        "Taildrop receive completed in {}",
                                        request.directory.display()
                                    ),
                                    filenames,
                                },
                                run,
                            )),
                            Err(error) => Err(error.failure),
                        }
                    }
                    Err(error) => Err(ServiceFailure::new(
                        ServiceFailureKind::CommandFailed,
                        "file get",
                        "Taildrop receive request is invalid",
                        error.to_string(),
                    )),
                }
            }
        }
        ServiceActionRequest::TaildriveShare {
            normalized_name,
            path,
            ..
        } => {
            if !executable.capabilities.drive {
                Err(unsupported_service(
                    "drive share",
                    "Taildrive is not advertised by this CLI",
                ))
            } else if !std::fs::metadata(&path)
                .map(|metadata| metadata.is_dir())
                .unwrap_or(false)
            {
                Err(ServiceFailure::new(
                    ServiceFailureKind::CommandFailed,
                    "drive share",
                    "Taildrive share directory is unavailable",
                    "the directory no longer exists or is not a directory",
                ))
            } else {
                match transfers::drive_share_command(
                    &executable.path,
                    timeout,
                    &normalized_name,
                    &path,
                ) {
                    Ok(command) => {
                        let run = run_service_command(command, &cancellation).await;
                        outcome_from_run(run, |run| async {
                            verify_drive_share(
                                &executable,
                                timeout,
                                &cancellation,
                                normalized_name,
                                path,
                                run,
                            )
                            .await
                        })
                        .await
                    }
                    Err(error) => Err(ServiceFailure::new(
                        ServiceFailureKind::CommandFailed,
                        "drive share",
                        "Taildrive share request is invalid",
                        error.to_string(),
                    )),
                }
            }
        }
        ServiceActionRequest::TaildriveRename {
            old_name,
            normalized_name,
            ..
        } => {
            if !executable.capabilities.drive {
                Err(unsupported_service(
                    "drive rename",
                    "Taildrive is not advertised by this CLI",
                ))
            } else {
                match transfers::drive_rename_command(
                    &executable.path,
                    timeout,
                    &old_name,
                    &normalized_name,
                ) {
                    Ok(command) => {
                        let run = run_service_command(command, &cancellation).await;
                        outcome_from_run(run, |run| async {
                            verify_drive_rename(
                                &executable,
                                timeout,
                                &cancellation,
                                old_name,
                                normalized_name,
                                run,
                            )
                            .await
                        })
                        .await
                    }
                    Err(error) => Err(ServiceFailure::new(
                        ServiceFailureKind::CommandFailed,
                        "drive rename",
                        "Taildrive rename request is invalid",
                        error.to_string(),
                    )),
                }
            }
        }
        ServiceActionRequest::TaildriveUnshare { name } => {
            if !executable.capabilities.drive {
                Err(unsupported_service(
                    "drive unshare",
                    "Taildrive is not advertised by this CLI",
                ))
            } else {
                match transfers::drive_unshare_command(&executable.path, timeout, &name) {
                    Ok(command) => {
                        let run = run_service_command(command, &cancellation).await;
                        outcome_from_run(run, |run| async {
                            verify_drive_unshare(&executable, timeout, &cancellation, name, run)
                                .await
                        })
                        .await
                    }
                    Err(error) => Err(ServiceFailure::new(
                        ServiceFailureKind::CommandFailed,
                        "drive unshare",
                        "Taildrive unshare request is invalid",
                        error.to_string(),
                    )),
                }
            }
        }
        ServiceActionRequest::Certificate(request) => {
            if !executable.capabilities.certificate {
                Err(unsupported_service(
                    "cert",
                    "certificate acquisition is not advertised",
                ))
            } else {
                match certificates::certificate_command(&executable.path, timeout, &request) {
                    Ok(command) => match run_service_command(command, &cancellation).await {
                        Ok(run) => match certificates::verify_certificate_outputs(
                            &request,
                            crate::local::now(),
                        ) {
                            Ok(value) => Ok((ServiceTaskData::Certificate(value), run)),
                            Err(error) => Err(annotate_service_failure(
                                ServiceFailure::new(
                                    ServiceFailureKind::CommandFailed,
                                    "cert",
                                    "certificate outputs could not be verified",
                                    error.to_string(),
                                ),
                                &run,
                            )),
                        },
                        Err(error) => Err(error.failure),
                    },
                    Err(error) => Err(ServiceFailure::new(
                        ServiceFailureKind::CommandFailed,
                        "cert",
                        "certificate request is invalid",
                        error.to_string(),
                    )),
                }
            }
        }
        ServiceActionRequest::Metrics => {
            if !executable.capabilities.metrics {
                Err(unsupported_service(
                    "metrics print",
                    "metrics are not advertised by this CLI",
                ))
            } else {
                match run_service_command(
                    services::metrics_command(&executable.path, timeout, 256 * 1024),
                    &cancellation,
                )
                .await
                {
                    Ok(run) => Ok((
                        ServiceTaskData::Metrics(crate::domain::service::MetricsOutput {
                            text: services::redacted_metrics(&run.stdout),
                            captured_at: crate::local::now(),
                            truncated: run.truncated_stdout,
                        }),
                        run,
                    )),
                    Err(error) => Err(error.failure),
                }
            }
        }
        ServiceActionRequest::BugReport(request) => {
            if !executable.capabilities.bugreport {
                Err(unsupported_service(
                    "bugreport",
                    "bug reports are not advertised by this CLI",
                ))
            } else {
                match services::bugreport_command(
                    &executable.path,
                    timeout,
                    request.note.as_deref(),
                    request.diagnose,
                ) {
                    Ok(command) => match run_service_command(command, &cancellation).await {
                        Ok(run) => match process::decode_utf8(&run.stdout)
                            .map_err(|error| {
                                annotate_service_failure(
                                    service_failure_from_process("bugreport", error),
                                    &run,
                                )
                            })
                            .and_then(|output| {
                                services::parse_bugreport_identifier(output).map_err(|error| {
                                    annotate_service_failure(
                                        ServiceFailure::new(
                                            ServiceFailureKind::DecodeFailed,
                                            "bugreport",
                                            "bug-report identifier could not be decoded",
                                            bounded_task_detail(&error.to_string()),
                                        ),
                                        &run,
                                    )
                                })
                            }) {
                            Ok(identifier) => Ok((
                                ServiceTaskData::BugReport(
                                    crate::domain::service::BugReportResult {
                                        identifier,
                                        observed_at: crate::local::now(),
                                    },
                                ),
                                run,
                            )),
                            Err(error) => Err(error),
                        },
                        Err(error) => Err(error.failure),
                    },
                    Err(error) => Err(ServiceFailure::new(
                        ServiceFailureKind::CommandFailed,
                        "bugreport",
                        "bug-report request is invalid",
                        error.to_string(),
                    )),
                }
            }
        }
    };
    match result {
        Ok((data, run)) => {
            outcome.result = Ok(data);
            outcome.exit_status = run.exit_status;
            outcome.stdout_truncated = run.truncated_stdout;
            outcome.stderr_truncated = run.truncated_stderr;
        }
        Err(failure) => {
            outcome.exit_status = failure.exit_status;
            outcome.stdout_truncated = failure.stdout_truncated;
            outcome.stderr_truncated = failure.stderr_truncated;
            outcome.result = Err(failure);
        }
    }
    outcome
}

async fn outcome_from_run<F, Fut>(
    run: Result<crate::local::process::LocalCommandResult, ServiceRunError>,
    verify: F,
) -> Result<(ServiceTaskData, crate::local::process::LocalCommandResult), ServiceFailure>
where
    F: FnOnce(crate::local::process::LocalCommandResult) -> Fut,
    Fut: std::future::Future<
            Output = Result<
                (ServiceTaskData, crate::local::process::LocalCommandResult),
                ServiceFailure,
            >,
        >,
{
    match run {
        Ok(run) => {
            let exit_status = run.exit_status;
            let stdout_truncated = run.truncated_stdout;
            let stderr_truncated = run.truncated_stderr;
            verify(run).await.map_err(|mut failure| {
                if failure.exit_status.is_none() {
                    failure.exit_status = exit_status;
                }
                failure.stdout_truncated |= stdout_truncated;
                failure.stderr_truncated |= stderr_truncated;
                failure
            })
        }
        Err(error) => Err(error.failure),
    }
}

async fn verify_serve_mapping(
    executable: &crate::domain::source::LocalExecutable,
    timeout: Duration,
    cancellation: &Cancellation,
    requested: crate::domain::service::ServiceMapping,
    run: crate::local::process::LocalCommandResult,
) -> Result<(ServiceTaskData, crate::local::process::LocalCommandResult), ServiceFailure> {
    let status = read_serve_status(executable, timeout, cancellation).await?;
    let verified = status.mappings.iter().any(|actual| {
        actual.exact_identity_matches(&requested)
            && actual.backend == requested.backend
            && actual.proxy_protocol == requested.proxy_protocol
    });
    Ok((
        ServiceTaskData::Serve {
            status,
            verified,
            summary: if verified {
                "Serve command completed and state verified".to_owned()
            } else {
                "Serve command completed but fresh state did not match".to_owned()
            },
        },
        run,
    ))
}

async fn verify_serve_reset(
    executable: &crate::domain::source::LocalExecutable,
    timeout: Duration,
    cancellation: &Cancellation,
    run: crate::local::process::LocalCommandResult,
) -> Result<(ServiceTaskData, crate::local::process::LocalCommandResult), ServiceFailure> {
    let status = read_serve_status(executable, timeout, cancellation).await?;
    let verified = status.mappings.is_empty();
    Ok((
        ServiceTaskData::Serve {
            status,
            verified,
            summary: if verified {
                "Serve reset completed and state verified".to_owned()
            } else {
                "Serve reset completed but mappings remain".to_owned()
            },
        },
        run,
    ))
}

async fn verify_funnel_mapping(
    executable: &crate::domain::source::LocalExecutable,
    timeout: Duration,
    cancellation: &Cancellation,
    requested: crate::domain::service::ServiceMapping,
    run: crate::local::process::LocalCommandResult,
) -> Result<(ServiceTaskData, crate::local::process::LocalCommandResult), ServiceFailure> {
    let status = read_funnel_status(executable, timeout, cancellation).await?;
    let verified = status.mappings.iter().any(|actual| {
        actual.exact_identity_matches(&requested)
            && actual.backend == requested.backend
            && actual.proxy_protocol == requested.proxy_protocol
    });
    Ok((
        ServiceTaskData::Funnel {
            status,
            verified,
            summary: if verified {
                "PUBLIC Funnel command completed and state verified".to_owned()
            } else {
                "PUBLIC Funnel command completed but fresh state did not match".to_owned()
            },
        },
        run,
    ))
}

async fn verify_funnel_reset(
    executable: &crate::domain::source::LocalExecutable,
    timeout: Duration,
    cancellation: &Cancellation,
    run: crate::local::process::LocalCommandResult,
) -> Result<(ServiceTaskData, crate::local::process::LocalCommandResult), ServiceFailure> {
    let status = read_funnel_status(executable, timeout, cancellation).await?;
    let verified = status.mappings.is_empty();
    Ok((
        ServiceTaskData::Funnel {
            status,
            verified,
            summary: if verified {
                "PUBLIC Funnel reset completed and state verified".to_owned()
            } else {
                "PUBLIC Funnel reset completed but mappings remain".to_owned()
            },
        },
        run,
    ))
}

async fn verify_drive_share(
    executable: &crate::domain::source::LocalExecutable,
    timeout: Duration,
    cancellation: &Cancellation,
    name: String,
    path: std::path::PathBuf,
    run: crate::local::process::LocalCommandResult,
) -> Result<(ServiceTaskData, crate::local::process::LocalCommandResult), ServiceFailure> {
    let shares = read_taildrive_shares(executable, timeout, cancellation, true).await?;
    let verified = shares
        .iter()
        .any(|share| share.name == name && share.path == path);
    Ok((
        ServiceTaskData::Taildrive {
            shares,
            verified,
            summary: if verified {
                "Taildrive share completed and state verified".to_owned()
            } else {
                "Taildrive share completed but fresh state did not match".to_owned()
            },
        },
        run,
    ))
}

async fn verify_drive_rename(
    executable: &crate::domain::source::LocalExecutable,
    timeout: Duration,
    cancellation: &Cancellation,
    old_name: String,
    new_name: String,
    run: crate::local::process::LocalCommandResult,
) -> Result<(ServiceTaskData, crate::local::process::LocalCommandResult), ServiceFailure> {
    let shares = read_taildrive_shares(executable, timeout, cancellation, true).await?;
    let verified = !shares.iter().any(|share| share.name == old_name)
        && shares.iter().any(|share| share.name == new_name);
    Ok((
        ServiceTaskData::Taildrive {
            shares,
            verified,
            summary: if verified {
                "Taildrive rename completed and state verified".to_owned()
            } else {
                "Taildrive rename completed but fresh state did not match".to_owned()
            },
        },
        run,
    ))
}

async fn verify_drive_unshare(
    executable: &crate::domain::source::LocalExecutable,
    timeout: Duration,
    cancellation: &Cancellation,
    name: String,
    run: crate::local::process::LocalCommandResult,
) -> Result<(ServiceTaskData, crate::local::process::LocalCommandResult), ServiceFailure> {
    let shares = read_taildrive_shares(executable, timeout, cancellation, true).await?;
    let verified = !shares.iter().any(|share| share.name == name);
    Ok((
        ServiceTaskData::Taildrive {
            shares,
            verified,
            summary: if verified {
                "Taildrive unshare completed and state verified".to_owned()
            } else {
                "Taildrive unshare completed but the share remains".to_owned()
            },
        },
        run,
    ))
}

async fn run_transfer_command(
    command: crate::local::process::LocalCommand,
    cancellation: &Cancellation,
    queue: &EventQueue,
    task_id: TaskId,
) -> (
    Result<crate::local::process::LocalCommandResult, ServiceRunError>,
    Vec<String>,
) {
    let (sender, mut receiver) = mpsc::channel(64);
    let future = process::run_lines(command, cancellation, sender);
    tokio::pin!(future);
    let mut lines = Vec::new();
    let result = loop {
        tokio::select! {
            result = &mut future => break result,
            line = receiver.recv() => {
                if let Some(line) = line {
                    let text = String::from_utf8_lossy(&line.bytes).trim().to_owned();
                    if let Some(progress) = transfers::parse_taildrop_progress(&text, crate::local::now())
                        && let Some(percent) = progress.percent
                    {
                        queue.send(Event::Task(Box::new(TaskEvent::Progress {
                            task_id,
                            progress: Progress {
                                completed: u16::from(percent),
                                total: 100,
                            },
                            detail: text.clone(),
                        }))).await;
                    }
                    if line.stream == OutputStream::Stdout && !text.is_empty() {
                        lines.push(text);
                    }
                }
            }
        }
    };
    let result = match result {
        Ok(value) => {
            if value.result.exit_status == Some(0) {
                Ok(value.result)
            } else {
                Err(ServiceRunError {
                    failure: service_failure_from_result(&value.result),
                })
            }
        }
        Err(error) => Err(ServiceRunError {
            failure: service_failure_from_process("transfer", error),
        }),
    };
    (result, lines)
}

async fn run_service_command(
    command: crate::local::process::LocalCommand,
    cancellation: &Cancellation,
) -> Result<crate::local::process::LocalCommandResult, ServiceRunError> {
    let operation = command.operation.label();
    match process::run(command, cancellation).await {
        Ok(result) if result.exit_status == Some(0) => Ok(result),
        Ok(result) => Err(ServiceRunError {
            failure: service_failure_from_result(&result),
        }),
        Err(error) => Err(ServiceRunError {
            failure: service_failure_from_process(&operation, error),
        }),
    }
}

fn service_failure_from_result(
    result: &crate::local::process::LocalCommandResult,
) -> ServiceFailure {
    let detail = match &result.operation {
        LocalOperation::Certificate => String::new(),
        LocalOperation::Metrics => {
            let redacted = services::redacted_metrics(&result.stderr);
            safe_operator_detail(&redacted)
        }
        _ => safe_operator_detail(&String::from_utf8_lossy(&result.stderr)),
    };
    let normalized = detail.to_ascii_lowercase();
    let kind = if normalized.contains("unknown command") || normalized.contains("unknown flag") {
        ServiceFailureKind::Unsupported
    } else if normalized.contains("permission denied") || normalized.contains("not permitted") {
        ServiceFailureKind::PermissionDenied
    } else if normalized.contains("policy")
        || normalized.contains("funnel is not enabled")
        || normalized.contains("not authorized")
    {
        ServiceFailureKind::PolicyDenied
    } else if normalized.contains("cannot connect")
        || normalized.contains("daemon")
        || normalized.contains("not running")
    {
        ServiceFailureKind::DaemonUnavailable
    } else {
        ServiceFailureKind::CommandFailed
    };
    let mut failure = ServiceFailure::new(
        kind,
        result.operation.label(),
        "local service command returned an error",
        if detail.is_empty() {
            "the command returned a non-zero status"
        } else {
            detail.as_str()
        },
    );
    failure.exit_status = result.exit_status;
    failure.stdout_truncated = result.truncated_stdout;
    failure.stderr_truncated = result.truncated_stderr;
    failure
}

fn annotate_service_failure(
    mut failure: ServiceFailure,
    result: &crate::local::process::LocalCommandResult,
) -> ServiceFailure {
    failure.exit_status = result.exit_status;
    failure.stdout_truncated = result.truncated_stdout;
    failure.stderr_truncated = result.truncated_stderr;
    failure
}

fn service_failure_from_process(operation: &str, error: LocalProcessError) -> ServiceFailure {
    let kind = match error {
        LocalProcessError::NotFound => ServiceFailureKind::NotInstalled,
        LocalProcessError::PermissionDenied => ServiceFailureKind::PermissionDenied,
        LocalProcessError::TimedOut => ServiceFailureKind::TimedOut,
        LocalProcessError::Cancelled => ServiceFailureKind::Cancelled,
        LocalProcessError::OutputNotUtf8(_) => ServiceFailureKind::DecodeFailed,
        LocalProcessError::Spawn(_) | LocalProcessError::Io(_) => ServiceFailureKind::CommandFailed,
    };
    let summary = kind.label().to_owned();
    let detail = match error {
        LocalProcessError::OutputNotUtf8(_) => "command output was not valid UTF-8".to_owned(),
        LocalProcessError::NotFound => "the tailscale executable was not found".to_owned(),
        LocalProcessError::PermissionDenied => "the operating system denied the command".to_owned(),
        LocalProcessError::TimedOut => "the command exceeded the configured timeout".to_owned(),
        LocalProcessError::Cancelled => "the command was cancelled".to_owned(),
        LocalProcessError::Spawn(_) | LocalProcessError::Io(_) => {
            "the local command could not be completed".to_owned()
        }
    };
    ServiceFailure::new(kind, operation, format!("local service {summary}"), detail)
}

fn unsupported_service(operation: &str, detail: &str) -> ServiceFailure {
    ServiceFailure::new(
        ServiceFailureKind::Unsupported,
        operation,
        "local service capability is unsupported",
        detail,
    )
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
