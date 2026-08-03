use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::event;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinSet;

use crate::app::App;
use crate::domain::redaction::Redactor;
use crate::domain::source::{LocalFailure, LocalFailureKind};
use crate::effect::{Effect, Resource};
use crate::error::TaleError;
use crate::event::LocalEvent;
use crate::event::{self as app_event, Event, InputEvent, ShutdownReason, SourceEvent, TaskEvent};
use crate::local::client::{self, LocalClient};
use crate::local::diagnostics;
use crate::local::process::{self, Cancellation, LocalProcessError, OutputStream, ProcessLine};
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
    let mut cancellations: HashMap<TaskId, Cancellation> = HashMap::new();
    let mut local_status_cancellation: Option<Cancellation> = None;
    let mut local_discovery_cancellation: Option<Cancellation> = None;

    spawn_input_source(&mut tasks, queue.clone(), stop.clone());
    spawn_tick_source(&mut tasks, queue.clone(), stop.clone());
    spawn_signal_source(&mut tasks, queue.clone(), stop.clone());

    for effect in app.bootstrap_effects() {
        dispatch_effect(
            effect,
            &queue,
            &mut tasks,
            &mut cancellations,
            &mut local_status_cancellation,
            &mut local_discovery_cancellation,
        );
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
                    dispatch_effect(
                        effect,
                        &queue,
                        &mut tasks,
                        &mut cancellations,
                        &mut local_status_cancellation,
                        &mut local_discovery_cancellation,
                    );
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
            dispatch_effect(
                effect,
                &queue,
                &mut tasks,
                &mut cancellations,
                &mut local_status_cancellation,
                &mut local_discovery_cancellation,
            );
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
    cancellations: &mut HashMap<TaskId, Cancellation>,
    local_status_cancellation: &mut Option<Cancellation>,
    local_discovery_cancellation: &mut Option<Cancellation>,
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
