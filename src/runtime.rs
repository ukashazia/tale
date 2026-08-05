use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::event;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinSet;

use crate::action::ActionId;
use crate::admin;
use crate::admin::auth::{CredentialStore, OsCredentialStore, TokenManager};
use crate::admin::client::{AdminClient, AdminError};
use crate::admin::key_mutations::decode_created_auth_key;
use crate::admin::mutation::{
    AdminMutationOutcome, AdminMutationRequest, AdminSnapshotFields, device_fields,
    dns_preferences_fields, nameserver_fields, search_path_fields, split_dns_fields, user_fields,
};
use crate::admin::policy_mutations::{decode_preview_checked, decode_validation};
use crate::app::App;
use crate::domain::account::LocalAccount;
use crate::domain::flow::aggregate_checked_cancellable;
use crate::domain::mutation::{LocalMutation, MutationResult};
use crate::domain::operational::{LogStreamMutationDraft, OperationalMutation};
use crate::domain::policy_workflow::{PolicyDocument, PolicySelectorType, hash_bytes};
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
use crate::event::{
    self as app_event, AdminEvent, CredentialEvent, CredentialRevocationResult, Event, InputEvent,
    OperationalResult, PolicyApplyResult, PolicyEvent, ShutdownReason, SourceEvent, TaskEvent,
};
use crate::local::client::{self, LocalCliClient};
use crate::local::diagnostics;
use crate::local::process::{
    self, Cancellation, LocalOperation, LocalProcessError, OutputStream, ProcessLine,
};
use crate::local::{accounts, handoff, policy};
use crate::local::{certificates, services, transfers};
use crate::mock::{self, MOCK_NOW, MockTaskBehavior};
use crate::task::{Progress, TaskId, grace_duration};
use crate::terminal::{EditorCommand, RealTerminal};
use crate::ui;

const EVENT_CAPACITY: usize = 256;
const TICK_INTERVAL: Duration = Duration::from_millis(100);
const ADMIN_VERIFICATION_DEADLINE: Duration = Duration::from_secs(30);
const ADMIN_VERIFICATION_POLL: Duration = Duration::from_millis(250);

struct PolicyTaskContext {
    queue: EventQueue,
    token_manager: Arc<TokenManager>,
    workflow_id: u64,
    profile: String,
    credential: String,
    tailnet: String,
    timeout: Duration,
}

struct AuthKeyTaskContext {
    queue: EventQueue,
    token_manager: Arc<TokenManager>,
    result_id: u64,
    profile: String,
    credential: String,
    tailnet: String,
    timeout: Duration,
    request: crate::admin::key_mutations::AuthKeyCreateRequest,
}

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
    let mut local_observation_cancellation: Option<Cancellation> = None;
    let mut local_discovery_cancellation: Option<Cancellation> = None;
    let mut local_services_refresh_cancellation: Option<Cancellation> = None;
    let mut admin_refresh_cancellation: Option<Cancellation> = None;
    let mut admin_token_managers: BTreeMap<String, Arc<TokenManager>> = BTreeMap::new();
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
            local_observation_cancellation: &mut local_observation_cancellation,
            local_discovery_cancellation: &mut local_discovery_cancellation,
            local_services_refresh_cancellation: &mut local_services_refresh_cancellation,
            admin_refresh_cancellation: &mut admin_refresh_cancellation,
            admin_token_managers: &mut admin_token_managers,
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
    if let Some(cancellation) = local_observation_cancellation {
        cancellation.cancel();
    }
    if let Some(cancellation) = local_discovery_cancellation {
        cancellation.cancel();
    }
    if let Some(cancellation) = local_services_refresh_cancellation {
        cancellation.cancel();
    }
    if let Some(cancellation) = admin_refresh_cancellation {
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
    local_observation_cancellation: &'a mut Option<Cancellation>,
    local_discovery_cancellation: &'a mut Option<Cancellation>,
    local_services_refresh_cancellation: &'a mut Option<Cancellation>,
    admin_refresh_cancellation: &'a mut Option<Cancellation>,
    admin_token_managers: &'a mut BTreeMap<String, Arc<TokenManager>>,
    mutation_cancellations: &'a mut HashMap<u64, Cancellation>,
    terminal: &'a mut T,
    handoff_input_gate: &'a Arc<AtomicBool>,
    terminal_suspended: &'a mut bool,
}

fn dispatch_effect<T: TerminalDriver>(effect: Effect, context: &mut DispatchContext<'_, T>) {
    let queue = context.queue;
    let tasks = &mut *context.tasks;
    let cancellations = &mut *context.cancellations;
    let local_observation_cancellation = &mut *context.local_observation_cancellation;
    let local_discovery_cancellation = &mut *context.local_discovery_cancellation;
    let local_services_refresh_cancellation = &mut *context.local_services_refresh_cancellation;
    let admin_refresh_cancellation = &mut *context.admin_refresh_cancellation;
    let admin_token_managers = &mut *context.admin_token_managers;
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
        Effect::StartAdminRefresh {
            profile,
            tailnet,
            credential,
            environment_token,
            generation,
            timeout,
            audit_window_days,
        } => {
            if let Some(previous) = admin_refresh_cancellation.take() {
                previous.cancel();
            }
            let queue = queue.clone();
            let token_manager =
                token_manager_for(admin_token_managers, &profile, environment_token);
            let cancellation = Cancellation::new();
            *admin_refresh_cancellation = Some(cancellation.clone());
            let context = AdminTaskContext {
                queue: queue.clone(),
                profile: profile.clone(),
                credential,
                token_manager,
                generation,
                cancellation,
            };
            tasks.spawn(async move {
                queue
                    .send(Event::Admin(Box::new(AdminEvent::RefreshStarted {
                        profile: profile.clone(),
                        generation,
                    })))
                    .await;
                run_admin_refresh(
                    context,
                    tailnet,
                    AdminRefreshOptions {
                        timeout,
                        audit_window_days,
                    },
                )
                .await;
            });
        }
        Effect::StartAdminResourceRefresh {
            profile,
            tailnet,
            credential,
            environment_token,
            generation,
            timeout,
            audit_window_days,
            resources,
        } => {
            if let Some(previous) = admin_refresh_cancellation.take() {
                previous.cancel();
            }
            let token_manager =
                token_manager_for(admin_token_managers, &profile, environment_token);
            let cancellation = Cancellation::new();
            *admin_refresh_cancellation = Some(cancellation.clone());
            let context = AdminTaskContext {
                queue: queue.clone(),
                profile,
                credential,
                token_manager,
                generation,
                cancellation,
            };
            tasks.spawn(async move {
                run_admin_resource_refresh(
                    context,
                    tailnet,
                    AdminResourceRefreshOptions {
                        timeout,
                        audit_window_days,
                        resources,
                    },
                )
                .await;
            });
        }
        Effect::StartAdminDeviceEnrichment {
            profile,
            credential,
            environment_token,
            generation,
            device_id,
            timeout,
        } => {
            let queue = queue.clone();
            let token_manager =
                token_manager_for(admin_token_managers, &profile, environment_token);
            let cancellation = admin_refresh_cancellation
                .as_ref()
                .filter(|value| !value.is_cancelled())
                .cloned()
                .unwrap_or_else(Cancellation::new);
            let context = AdminTaskContext {
                queue: queue.clone(),
                profile,
                credential,
                token_manager,
                generation,
                cancellation,
            };
            tasks.spawn(async move {
                run_admin_device_enrichment(context, device_id, timeout).await;
            });
        }
        Effect::StartAdminPreflight {
            request,
            tailnet,
            credential,
            environment_token,
            timeout,
        } => {
            let token_manager =
                token_manager_for(admin_token_managers, &request.profile, environment_token);
            let queue = queue.clone();
            tasks.spawn(async move {
                run_admin_preflight(queue, token_manager, request, tailnet, credential, timeout)
                    .await;
            });
        }
        Effect::StartAdminMutation {
            task_id,
            request,
            tailnet,
            credential,
            environment_token,
            timeout,
        } => {
            let token_manager =
                token_manager_for(admin_token_managers, &request.profile, environment_token);
            let queue = queue.clone();
            let cancellation = Cancellation::new();
            mutation_cancellations.insert(request.mutation_id, cancellation.clone());
            cancellations.insert(task_id, cancellation.clone());
            tasks.spawn(async move {
                run_admin_mutation(AdminMutationTask {
                    queue,
                    token_manager,
                    task_id,
                    request,
                    tailnet,
                    credential,
                    timeout,
                    cancellation,
                })
                .await;
            });
        }
        Effect::StartOperationalMutation {
            action_id,
            mutation,
            profile,
            tailnet,
            credential,
            environment_token,
            timeout,
        } => {
            let token_manager =
                token_manager_for(admin_token_managers, &profile, environment_token);
            let queue = queue.clone();
            tasks.spawn(async move {
                run_operational_mutation(
                    queue,
                    token_manager,
                    action_id,
                    mutation,
                    profile,
                    tailnet,
                    credential,
                    timeout,
                )
                .await;
            });
        }
        Effect::StartAccessExplorer {
            question,
            policy,
            profile,
            tailnet,
            credential,
            environment_token,
            timeout,
        } => {
            let token_manager =
                token_manager_for(admin_token_managers, &profile, environment_token);
            let queue = queue.clone();
            tasks.spawn(async move {
                run_access_explorer(
                    queue,
                    token_manager,
                    question,
                    policy,
                    profile,
                    tailnet,
                    credential,
                    timeout,
                )
                .await;
            });
        }
        Effect::StartHealthEvaluation {
            generation,
            snapshot,
        } => {
            let queue = queue.clone();
            tasks.spawn(async move {
                run_health_evaluation(queue, generation, snapshot).await;
            });
        }
        Effect::StartFlowAggregation {
            generation,
            messages,
            filter,
            dimensions,
            cancellation,
        } => {
            let queue = queue.clone();
            tasks.spawn(async move {
                let result = match tokio::task::spawn_blocking(move || {
                    aggregate_checked_cancellable(
                        &messages,
                        &filter,
                        &dimensions,
                        Some(cancellation.as_ref()),
                    )
                })
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err(crate::domain::flow::FlowError::Cancelled),
                };
                let _ = queue
                    .send(Event::Admin(Box::new(
                        AdminEvent::FlowAggregationFinished { generation, result },
                    )))
                    .await;
            });
        }
        Effect::StartPolicyRemoteFetch {
            workflow_id,
            profile,
            tailnet,
            credential,
            environment_token,
            timeout,
        } => {
            let token_manager =
                token_manager_for(&mut *admin_token_managers, &profile, environment_token);
            let queue = queue.clone();
            tasks.spawn(async move {
                run_policy_remote_fetch(
                    queue,
                    token_manager,
                    workflow_id,
                    profile,
                    credential,
                    tailnet,
                    timeout,
                )
                .await;
            });
        }
        Effect::StartPolicyEditor {
            workflow_id,
            command,
            path,
        } => {
            if *terminal_suspended {
                let queue = queue.clone();
                tasks.spawn(async move {
                    queue
                        .send(Event::Policy(Box::new(PolicyEvent::EditorFinished {
                            workflow_id,
                            result: Err("another interactive child owns the terminal".to_owned()),
                            path,
                            editor_success: false,
                            editor_code: None,
                        })))
                        .await;
                });
                return;
            }
            if let Err(error) = terminal.suspend_for_handoff() {
                let queue = queue.clone();
                let detail = error.to_string();
                tasks.spawn(async move {
                    queue
                        .send(Event::Policy(Box::new(PolicyEvent::EditorFinished {
                            workflow_id,
                            result: Err(detail),
                            path,
                            editor_success: false,
                            editor_code: None,
                        })))
                        .await;
                });
                return;
            }
            *terminal_suspended = true;
            handoff_input_gate.store(false, Ordering::Release);
            let queue = queue.clone();
            tasks.spawn(async move {
                let result = run_policy_editor(command, path.clone()).await;
                queue
                    .send(Event::Policy(Box::new(PolicyEvent::EditorFinished {
                        workflow_id,
                        result: result.0,
                        path,
                        editor_success: result.1,
                        editor_code: result.2,
                    })))
                    .await;
            });
        }
        Effect::StartPolicyValidate {
            workflow_id,
            profile,
            tailnet,
            credential,
            environment_token,
            timeout,
            path,
        } => {
            let token_manager =
                token_manager_for(&mut *admin_token_managers, &profile, environment_token);
            let queue = queue.clone();
            tasks.spawn(async move {
                run_policy_validate(
                    PolicyTaskContext {
                        queue,
                        token_manager,
                        workflow_id,
                        profile,
                        credential,
                        tailnet,
                        timeout,
                    },
                    path,
                )
                .await;
            });
        }
        Effect::StartPolicyPreview {
            workflow_id,
            profile,
            tailnet,
            credential,
            environment_token,
            timeout,
            path,
            selector_type,
            selector,
        } => {
            let token_manager =
                token_manager_for(&mut *admin_token_managers, &profile, environment_token);
            let queue = queue.clone();
            tasks.spawn(async move {
                run_policy_preview(
                    PolicyTaskContext {
                        queue,
                        token_manager,
                        workflow_id,
                        profile,
                        credential,
                        tailnet,
                        timeout,
                    },
                    path,
                    selector_type,
                    selector,
                )
                .await;
            });
        }
        Effect::StartPolicyApply {
            workflow_id,
            profile,
            tailnet,
            credential,
            environment_token,
            timeout,
            path,
            expected_base_hash,
            expected_candidate_hash,
        } => {
            let token_manager =
                token_manager_for(&mut *admin_token_managers, &profile, environment_token);
            let queue = queue.clone();
            tasks.spawn(async move {
                run_policy_apply(
                    PolicyTaskContext {
                        queue,
                        token_manager,
                        workflow_id,
                        profile,
                        credential,
                        tailnet,
                        timeout,
                    },
                    path,
                    expected_base_hash,
                    expected_candidate_hash,
                )
                .await;
            });
        }
        Effect::StartAuthKeyCreate {
            result_id,
            profile,
            tailnet,
            credential,
            environment_token,
            timeout,
            request,
        } => {
            let token_manager =
                token_manager_for(&mut *admin_token_managers, &profile, environment_token);
            let queue = queue.clone();
            tasks.spawn(async move {
                run_auth_key_create(AuthKeyTaskContext {
                    queue,
                    token_manager,
                    result_id,
                    profile,
                    credential,
                    tailnet,
                    timeout,
                    request,
                })
                .await;
            });
        }
        Effect::StartCredentialDetail {
            key_id,
            profile,
            tailnet,
            credential,
            environment_token,
            timeout,
        } => {
            let token_manager =
                token_manager_for(&mut *admin_token_managers, &profile, environment_token);
            let queue = queue.clone();
            tasks.spawn(async move {
                run_credential_detail(
                    queue,
                    token_manager,
                    key_id,
                    profile,
                    credential,
                    tailnet,
                    timeout,
                )
                .await;
            });
        }
        Effect::StartCredentialRevoke {
            key_id,
            profile,
            tailnet,
            credential,
            environment_token,
            timeout,
        } => {
            let token_manager =
                token_manager_for(&mut *admin_token_managers, &profile, environment_token);
            let queue = queue.clone();
            tasks.spawn(async move {
                run_credential_revoke(
                    queue,
                    token_manager,
                    key_id,
                    profile,
                    credential,
                    tailnet,
                    timeout,
                )
                .await;
            });
        }
        Effect::StartProfileCredentialRemove { profile, reference } => {
            let queue = queue.clone();
            tasks.spawn(async move {
                let store = OsCredentialStore;
                let result = store.delete(&reference).map_err(|error| error.to_string());
                queue
                    .send(Event::Credential(Box::new(CredentialEvent::LocalRemoved {
                        profile,
                        reference,
                        result,
                    })))
                    .await;
            });
        }
        Effect::CopySecret { result_id, secret } => {
            let queue = queue.clone();
            tasks.spawn(async move {
                let result = crate::clipboard::SystemClipboard::new().and_then(|mut clipboard| {
                    crate::clipboard::copy_secret(&mut clipboard, secret.as_ref())
                });
                queue
                    .send(Event::Credential(Box::new(
                        CredentialEvent::ClipboardCopied {
                            result_id,
                            result: result.map_err(|error| error.to_string()),
                        },
                    )))
                    .await;
            });
        }
        Effect::CopyText { label, text } => {
            let queue = queue.clone();
            tasks.spawn(async move {
                let result = crate::clipboard::SystemClipboard::new().and_then(|mut clipboard| {
                    crate::clipboard::ClipboardSink::set_text(&mut clipboard, &text)
                });
                queue
                    .send(Event::Credential(Box::new(
                        CredentialEvent::ClipboardTextCopied {
                            label,
                            result: result.map_err(|error| error.to_string()),
                        },
                    )))
                    .await;
            });
        }
        Effect::DropAdminToken { profile } => {
            admin_token_managers.remove(&profile);
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
                        match LocalCliClient::discover(resolved, timeout, &cancellation).await {
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
        Effect::StartLocalObservation {
            socket_path,
            timeout,
            reconcile_interval,
        } => {
            if let Some(previous) = local_observation_cancellation.take() {
                previous.cancel();
            }
            let queue = queue.clone();
            let cancellation = Cancellation::new();
            *local_observation_cancellation = Some(cancellation.clone());
            tasks.spawn(async move {
                let (sender, mut receiver) = mpsc::channel(128);
                let client = crate::local::daemon::LocalDaemonClient::new(socket_path, timeout);
                let observer = tokio::spawn(crate::local::ipn::run(
                    client,
                    crate::local::ipn::ObserverConfig { reconcile_interval },
                    cancellation.clone(),
                    sender,
                ));
                while let Some(event) = receiver.recv().await {
                    queue.send(local_event(observer_event(event))).await;
                }
                let _ = observer.await;
            });
        }
        Effect::StartLocalSnapshotRefresh {
            generation,
            socket_path,
            timeout,
        } => {
            let queue = queue.clone();
            tasks.spawn(async move {
                let client = crate::local::daemon::LocalDaemonClient::new(socket_path, timeout);
                let cancellation = Cancellation::new();
                queue
                    .send(local_event(LocalEvent::StatusStarted {
                        generation,
                        attempted_at: crate::local::now(),
                    }))
                    .await;
                queue
                    .send(local_event(LocalEvent::PreferencesStarted {
                        generation,
                        attempted_at: crate::local::now(),
                    }))
                    .await;
                let (status, preferences) = tokio::join!(
                    client.status(&cancellation),
                    client.preferences(&cancellation),
                );
                match status {
                    Ok(value) => {
                        queue
                            .send(local_event(LocalEvent::StatusSucceeded {
                                generation,
                                snapshot: Box::new(value.snapshot),
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
                match preferences {
                    Ok(value) => {
                        queue
                            .send(local_event(LocalEvent::PreferencesSucceeded {
                                generation,
                                preferences: Box::new(value.preferences),
                            }))
                            .await;
                    }
                    Err(error) => {
                        queue
                            .send(local_event(LocalEvent::PreferencesFailed {
                                generation,
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
                match accounts::list(
                    &executable.path,
                    timeout,
                    &Cancellation::new(),
                    executable.socket_path.as_deref(),
                )
                .await
                {
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
                match policy::list(
                    &executable.path,
                    timeout,
                    &Cancellation::new(),
                    executable.socket_path.as_deref(),
                )
                .await
                {
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
        Effect::CancelLocalObservation => {
            if let Some(cancellation) = local_observation_cancellation.as_ref() {
                cancellation.cancel();
            }
        }
        Effect::CancelAdminRefresh => {
            if let Some(cancellation) = admin_refresh_cancellation.as_ref() {
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

struct AdminRefreshOptions {
    timeout: Duration,
    audit_window_days: u64,
}

struct AdminResourceRefreshOptions {
    timeout: Duration,
    audit_window_days: u64,
    resources: Vec<admin::AdminRefreshResource>,
}

struct AdminTaskContext {
    queue: EventQueue,
    profile: String,
    credential: String,
    token_manager: Arc<TokenManager>,
    generation: u64,
    cancellation: Cancellation,
}

fn token_manager_for(
    managers: &mut BTreeMap<String, Arc<TokenManager>>,
    profile: &str,
    environment_token: Option<Arc<crate::admin::auth::SecretValue>>,
) -> Arc<TokenManager> {
    if let Some(manager) = managers.get(profile) {
        return Arc::clone(manager);
    }
    let store: Arc<dyn CredentialStore> = Arc::new(OsCredentialStore);
    let manager = Arc::new(TokenManager::new_with_override(store, environment_token));
    managers.insert(profile.to_owned(), Arc::clone(&manager));
    manager
}

async fn policy_token(
    token_manager: &TokenManager,
    profile: &str,
    credential: &str,
) -> Result<crate::admin::auth::AccessToken, String> {
    token_manager
        .access_token(profile, credential)
        .await
        .map_err(|error| error.to_string())
}

fn policy_document(bytes: Vec<u8>, observed_at: u64) -> Result<PolicyDocument, String> {
    PolicyDocument::from_bytes(bytes, observed_at).map_err(|error| error.to_string())
}

async fn run_policy_remote_fetch(
    queue: EventQueue,
    token_manager: Arc<TokenManager>,
    workflow_id: u64,
    profile: String,
    credential: String,
    tailnet: String,
    timeout: Duration,
) {
    let observed_at = crate::local::now();
    let result = async {
        let token = policy_token(&token_manager, &profile, &credential).await?;
        let client = AdminClient::new(timeout).map_err(|error| error.to_string())?;
        let response = client
            .get_policy(&token, &tailnet)
            .await
            .map_err(|error| error.to_string())?;
        let etag = response.value.etag.clone();
        let content_type = response.value.content_type.clone();
        let document = PolicyDocument::from_bytes_with_content_type(
            response.value.source_bytes,
            content_type.clone(),
            observed_at,
        )
        .map_err(|error| error.to_string())?;
        Ok::<_, String>((document, etag, content_type))
    }
    .await;
    let (result, etag, content_type) = match result {
        Ok((document, etag, content_type)) => (Ok(document), etag, content_type),
        Err(detail) => (Err(detail), None, String::new()),
    };
    let _ = queue
        .send(Event::Policy(Box::new(PolicyEvent::RemoteFetched {
            workflow_id,
            result,
            etag,
            content_type,
            observed_at,
        })))
        .await;
}

async fn run_policy_editor(
    command: EditorCommand,
    path: PathBuf,
) -> (Result<PolicyDocument, String>, bool, Option<i32>) {
    let exit = match command.run(&path).await {
        Ok(exit) => exit,
        Err(error) => return (Err(error.to_string()), false, None),
    };
    let result = crate::temporary::TemporaryPolicyFile::read_candidate_path(&path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| policy_document(bytes, crate::local::now()));
    (result, exit.success, exit.code)
}

async fn run_policy_validate(context: PolicyTaskContext, path: PathBuf) {
    let PolicyTaskContext {
        queue,
        token_manager,
        workflow_id,
        profile,
        credential,
        tailnet,
        timeout,
    } = context;
    let result = async {
        let bytes = crate::temporary::TemporaryPolicyFile::read_candidate_path(&path)
            .map_err(|error| error.to_string())?;
        let candidate = policy_document(bytes.clone(), crate::local::now())?;
        let token = policy_token(&token_manager, &profile, &credential).await?;
        let client = AdminClient::new(timeout).map_err(|error| error.to_string())?;
        let response = client
            .validate_policy(&token, &tailnet, &bytes)
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>(decode_validation(
            response.value,
            &candidate,
            response.meta.observed_at,
        ))
    }
    .await;
    let _ = queue
        .send(Event::Policy(Box::new(PolicyEvent::Validated {
            workflow_id,
            result,
        })))
        .await;
}

async fn run_policy_preview(
    context: PolicyTaskContext,
    path: PathBuf,
    selector_type: PolicySelectorType,
    selector: String,
) {
    let PolicyTaskContext {
        queue,
        token_manager,
        workflow_id,
        profile,
        credential,
        tailnet,
        timeout,
    } = context;
    let result = async {
        let bytes = crate::temporary::TemporaryPolicyFile::read_candidate_path(&path)
            .map_err(|error| error.to_string())?;
        let candidate = policy_document(bytes.clone(), crate::local::now())?;
        let token = policy_token(&token_manager, &profile, &credential).await?;
        let client = AdminClient::new(timeout).map_err(|error| error.to_string())?;
        let response = client
            .preview_policy(&token, &tailnet, selector_type, &selector, &bytes)
            .await
            .map_err(|error| error.to_string())?;
        decode_preview_checked(
            response.value,
            &candidate,
            selector_type,
            &selector,
            response.meta.observed_at,
        )
    }
    .await;
    let _ = queue
        .send(Event::Policy(Box::new(PolicyEvent::Previewed {
            workflow_id,
            result,
        })))
        .await;
}

async fn run_policy_apply(
    context: PolicyTaskContext,
    path: PathBuf,
    expected_base_hash: String,
    expected_candidate_hash: String,
) {
    let result = apply_policy_remote(
        &context,
        &path,
        &expected_base_hash,
        &expected_candidate_hash,
    )
    .await;
    let PolicyTaskContext {
        queue, workflow_id, ..
    } = context;
    let _ = queue
        .send(Event::Policy(Box::new(PolicyEvent::Applied {
            workflow_id,
            result,
        })))
        .await;
}

async fn apply_policy_remote(
    context: &PolicyTaskContext,
    path: &std::path::Path,
    expected_base_hash: &str,
    expected_candidate_hash: &str,
) -> PolicyApplyResult {
    let bytes = match crate::temporary::TemporaryPolicyFile::read_candidate_path(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return PolicyApplyResult::FailedRetained {
                detail: error.to_string(),
            };
        }
    };
    let candidate_hash = hash_bytes(&bytes);
    if candidate_hash != expected_candidate_hash {
        return PolicyApplyResult::FailedRetained {
            detail: "the temporary candidate changed before apply".to_owned(),
        };
    }
    let token = match policy_token(
        &context.token_manager,
        &context.profile,
        &context.credential,
    )
    .await
    {
        Ok(token) => token,
        Err(detail) => return PolicyApplyResult::FailedRetained { detail },
    };
    let client = match AdminClient::new(context.timeout) {
        Ok(client) => client,
        Err(error) => {
            return PolicyApplyResult::FailedRetained {
                detail: error.to_string(),
            };
        }
    };
    let latest = match client.get_policy(&token, &context.tailnet).await {
        Ok(response) => response,
        Err(error) => {
            return PolicyApplyResult::FailedRetained {
                detail: error.to_string(),
            };
        }
    };
    let latest_hash = hash_bytes(&latest.value.source_bytes);
    if latest_hash != expected_base_hash {
        let document = match policy_document(latest.value.source_bytes, latest.meta.observed_at) {
            Ok(document) => document,
            Err(detail) => return PolicyApplyResult::FailedRetained { detail },
        };
        return PolicyApplyResult::RemoteConflict { latest: document };
    }
    let candidate = match policy_document(bytes.clone(), crate::local::now()) {
        Ok(candidate) => candidate,
        Err(detail) => return PolicyApplyResult::FailedRetained { detail },
    };
    let validation = match client
        .validate_policy(&token, &context.tailnet, &bytes)
        .await
    {
        Ok(response) => decode_validation(response.value, &candidate, response.meta.observed_at),
        Err(error) => {
            return PolicyApplyResult::FailedRetained {
                detail: error.to_string(),
            };
        }
    };
    if !validation.valid {
        return PolicyApplyResult::FailedRetained {
            detail: validation
                .bounded_safe_detail
                .or(validation.message)
                .unwrap_or_else(|| "final server validation rejected the candidate".to_owned()),
        };
    }
    if let Err(error) = client.save_policy(&token, &context.tailnet, &bytes).await {
        return if policy_save_may_have_reached_server(&error) {
            verify_saved_policy(
                &client,
                &token,
                &context.tailnet,
                &bytes,
                candidate_hash,
                false,
            )
            .await
        } else {
            PolicyApplyResult::FailedRetained {
                detail: error.to_string(),
            }
        };
    }
    verify_saved_policy(
        &client,
        &token,
        &context.tailnet,
        &bytes,
        candidate_hash,
        true,
    )
    .await
}

async fn verify_saved_policy(
    client: &AdminClient,
    token: &crate::admin::auth::AccessToken,
    tailnet: &str,
    candidate: &[u8],
    candidate_hash: String,
    save_confirmed: bool,
) -> PolicyApplyResult {
    match client.get_policy(token, tailnet).await {
        Ok(response) if response.value.source_bytes == candidate => PolicyApplyResult::Succeeded {
            saved_hash: candidate_hash,
        },
        Ok(_) if save_confirmed => PolicyApplyResult::SucceededUnverified {
            saved_hash: candidate_hash,
        },
        Ok(_) => PolicyApplyResult::OutcomeUnknown {
            detail: "policy save outcome is unknown; remote bytes differ from the candidate"
                .to_owned(),
        },
        Err(error) => PolicyApplyResult::OutcomeUnknown {
            detail: format!("policy save outcome is unknown; verification failed: {error}"),
        },
    }
}

fn policy_save_may_have_reached_server(error: &AdminError) -> bool {
    matches!(
        error,
        AdminError::TimedOut { .. }
            | AdminError::Transport { .. }
            | AdminError::RateLimited { .. }
            | AdminError::ServerFailure { .. }
            | AdminError::UnexpectedStatus { .. }
            | AdminError::DecodeFailed { .. }
            | AdminError::BodyTooLarge { .. }
    )
}

async fn run_auth_key_create(context: AuthKeyTaskContext) {
    let AuthKeyTaskContext {
        queue,
        token_manager,
        result_id,
        profile,
        credential,
        tailnet,
        timeout,
        request,
    } = context;
    let result = async {
        let token = policy_token(&token_manager, &profile, &credential).await?;
        let client = AdminClient::new(timeout).map_err(|error| error.to_string())?;
        let response = client
            .create_auth_key(&token, &tailnet, &request)
            .await
            .map_err(|error| error.to_string())?;
        decode_created_auth_key(response.value, response.meta.observed_at)
            .map_err(|error| error.to_string())
    }
    .await;
    let event = match result {
        Ok(created) => Event::Credential(Box::new(CredentialEvent::AuthKeyCreated {
            result_id,
            metadata: created.metadata,
            secret: created.secret,
            observed_at: created.created_at,
        })),
        Err(detail) => Event::Credential(Box::new(CredentialEvent::AuthKeyCreateFailed {
            result_id,
            detail,
        })),
    };
    let _ = queue.send(event).await;
}

async fn run_credential_detail(
    queue: EventQueue,
    token_manager: Arc<TokenManager>,
    key_id: String,
    profile: String,
    credential: String,
    tailnet: String,
    timeout: Duration,
) {
    let result = async {
        let token = policy_token(&token_manager, &profile, &credential).await?;
        let client = AdminClient::new(timeout).map_err(|error| error.to_string())?;
        let response = client
            .get_key(&token, &tailnet, &key_id)
            .await
            .map_err(|error| error.to_string())?;
        crate::admin::credentials::decode_credential(response.value)
            .map_err(|error| error.to_string())
    }
    .await;
    let _ = queue
        .send(Event::Credential(Box::new(
            CredentialEvent::DetailFetched { key_id, result },
        )))
        .await;
}

async fn run_credential_revoke(
    queue: EventQueue,
    token_manager: Arc<TokenManager>,
    key_id: String,
    profile: String,
    credential: String,
    tailnet: String,
    timeout: Duration,
) {
    let result = async {
        let token = match policy_token(&token_manager, &profile, &credential).await {
            Ok(token) => token,
            Err(detail) => return CredentialRevocationResult::Failed { detail },
        };
        let client = match AdminClient::new(timeout) {
            Ok(client) => client,
            Err(error) => {
                return CredentialRevocationResult::Failed {
                    detail: error.to_string(),
                };
            }
        };
        match client.revoke_credential(&token, &tailnet, &key_id).await {
            Ok(_) => verify_credential_revocation(&client, &token, &tailnet, &key_id).await,
            Err(error) if revocation_may_have_reached_server(&error) => {
                match verify_credential_revocation(&client, &token, &tailnet, &key_id).await {
                    CredentialRevocationResult::Verified => CredentialRevocationResult::Verified,
                    CredentialRevocationResult::Failed { detail } => {
                        CredentialRevocationResult::OutcomeUnknown { detail }
                    }
                    CredentialRevocationResult::OutcomeUnknown { detail } => {
                        CredentialRevocationResult::OutcomeUnknown { detail }
                    }
                }
            }
            Err(error) => CredentialRevocationResult::Failed {
                detail: error.to_string(),
            },
        }
    }
    .await;
    let _ = queue
        .send(Event::Credential(Box::new(CredentialEvent::Revoked {
            key_id,
            result,
        })))
        .await;
}

async fn verify_credential_revocation(
    client: &AdminClient,
    token: &crate::admin::auth::AccessToken,
    tailnet: &str,
    key_id: &str,
) -> CredentialRevocationResult {
    match client.get_key(token, tailnet, key_id).await {
        Err(AdminError::NotFound { .. }) => CredentialRevocationResult::Verified,
        Err(AdminError::TimedOut { .. }) => CredentialRevocationResult::OutcomeUnknown {
            detail: "credential revocation verification timed out".to_owned(),
        },
        Err(error) => CredentialRevocationResult::OutcomeUnknown {
            detail: format!("credential revocation verification unavailable: {error}"),
        },
        Ok(response) => match crate::admin::credentials::decode_credential(response.value) {
            Ok(metadata) if metadata.invalid == Some(true) || metadata.revoked_at.is_some() => {
                CredentialRevocationResult::Verified
            }
            Ok(_) => CredentialRevocationResult::Failed {
                detail: "remote credential revocation was not confirmed".to_owned(),
            },
            Err(error) => CredentialRevocationResult::OutcomeUnknown {
                detail: format!(
                    "credential revocation verification could not decode metadata: {error}"
                ),
            },
        },
    }
}

fn revocation_may_have_reached_server(error: &AdminError) -> bool {
    matches!(
        error,
        AdminError::TimedOut { .. }
            | AdminError::Transport { .. }
            | AdminError::RateLimited { .. }
            | AdminError::ServerFailure { .. }
            | AdminError::UnexpectedStatus { .. }
            | AdminError::NotFound { .. }
    )
}

async fn run_admin_refresh(
    context: AdminTaskContext,
    tailnet: String,
    options: AdminRefreshOptions,
) {
    let AdminTaskContext {
        queue,
        profile,
        credential,
        token_manager,
        generation,
        cancellation,
    } = context;
    let token = tokio::select! {
        result = token_manager.access_token(&profile, &credential) => result,
        _ = wait_for_cancellation(cancellation.clone()) => Err(crate::admin::auth::AuthError::Cancelled),
    };
    let token = match token {
        Ok(token) => token,
        Err(error) => {
            if matches!(error, crate::admin::auth::AuthError::Cancelled) {
                queue
                    .send(Event::Admin(Box::new(AdminEvent::Failed {
                        profile,
                        generation,
                        detail: "admin refresh cancelled".to_owned(),
                    })))
                    .await;
            } else {
                queue
                    .send(Event::Admin(Box::new(AdminEvent::AuthenticationFailed {
                        profile,
                        generation,
                        detail: error.to_string(),
                    })))
                    .await;
            }
            return;
        }
    };
    let requested_scopes = match token_manager.credential_status(&credential) {
        Ok(Some(status)) => status.requested_scopes,
        Ok(None) | Err(_) => Vec::new(),
    };
    let client = match AdminClient::new(options.timeout) {
        Ok(client) => client,
        Err(error) => {
            queue
                .send(Event::Admin(Box::new(AdminEvent::Failed {
                    profile,
                    generation,
                    detail: error.to_string(),
                })))
                .await;
            return;
        }
    };
    let observed_at = crate::local::now();
    let end = match format_utc(observed_at) {
        Some(value) => value,
        None => {
            queue
                .send(Event::Admin(Box::new(AdminEvent::Failed {
                    profile,
                    generation,
                    detail: "could not construct the audit time window".to_owned(),
                })))
                .await;
            return;
        }
    };
    let window_seconds = options
        .audit_window_days
        .clamp(1, 90)
        .saturating_mul(24 * 60 * 60);
    let start = match format_utc(observed_at.saturating_sub(window_seconds)) {
        Some(value) => value,
        None => {
            queue
                .send(Event::Admin(Box::new(AdminEvent::Failed {
                    profile,
                    generation,
                    detail: "could not construct the audit start time".to_owned(),
                })))
                .await;
            return;
        }
    };
    let client = Arc::new(client);
    let tailnet = Arc::new(tailnet);
    let start = Arc::new(start);
    let end = Arc::new(end);
    let refresh_cancellation = cancellation.clone();
    let results = tokio::select! {
        results = async {
            tokio::join!(
                admin_read_with_replay(&token_manager, &profile, &credential, &token, refresh_cancellation.clone(), |token| {
                    let client = Arc::clone(&client);
                    let tailnet = Arc::clone(&tailnet);
                    Box::pin(async move { client.list_devices(token, tailnet.as_str()).await })
                }),
                admin_read_with_replay(&token_manager, &profile, &credential, &token, refresh_cancellation.clone(), |token| {
                    let client = Arc::clone(&client);
                    let tailnet = Arc::clone(&tailnet);
                    Box::pin(async move { client.list_users(token, tailnet.as_str()).await })
                }),
                admin_read_with_replay(&token_manager, &profile, &credential, &token, refresh_cancellation.clone(), |token| {
                    let client = Arc::clone(&client);
                    let tailnet = Arc::clone(&tailnet);
                    Box::pin(async move { client.get_nameservers(token, tailnet.as_str()).await })
                }),
                admin_read_with_replay(&token_manager, &profile, &credential, &token, refresh_cancellation.clone(), |token| {
                    let client = Arc::clone(&client);
                    let tailnet = Arc::clone(&tailnet);
                    Box::pin(async move { client.get_dns_preferences(token, tailnet.as_str()).await })
                }),
                admin_read_with_replay(&token_manager, &profile, &credential, &token, refresh_cancellation.clone(), |token| {
                    let client = Arc::clone(&client);
                    let tailnet = Arc::clone(&tailnet);
                    Box::pin(async move { client.get_search_paths(token, tailnet.as_str()).await })
                }),
                admin_read_with_replay(&token_manager, &profile, &credential, &token, refresh_cancellation.clone(), |token| {
                    let client = Arc::clone(&client);
                    let tailnet = Arc::clone(&tailnet);
                    Box::pin(async move { client.get_split_dns(token, tailnet.as_str()).await })
                }),
                admin_read_with_replay(&token_manager, &profile, &credential, &token, refresh_cancellation.clone(), |token| {
                    let client = Arc::clone(&client);
                    let tailnet = Arc::clone(&tailnet);
                    Box::pin(async move { client.get_policy(token, tailnet.as_str()).await })
                }),
                admin_read_with_replay(&token_manager, &profile, &credential, &token, refresh_cancellation.clone(), |token| {
                    let client = Arc::clone(&client);
                    let tailnet = Arc::clone(&tailnet);
                    Box::pin(async move { client.list_keys(token, tailnet.as_str()).await })
                }),
                admin_read_with_replay(&token_manager, &profile, &credential, &token, refresh_cancellation.clone(), |token| {
                    let client = Arc::clone(&client);
                    let tailnet = Arc::clone(&tailnet);
                    Box::pin(async move { client.get_settings(token, tailnet.as_str()).await })
                }),
                admin_read_with_replay(&token_manager, &profile, &credential, &token, refresh_cancellation.clone(), |token| {
                    let client = Arc::clone(&client);
                    let tailnet = Arc::clone(&tailnet);
                    Box::pin(async move { client.get_contacts(token, tailnet.as_str()).await })
                }),
                admin_read_with_replay(&token_manager, &profile, &credential, &token, refresh_cancellation.clone(), |token| {
                    let client = Arc::clone(&client);
                    let tailnet = Arc::clone(&tailnet);
                    let start = Arc::clone(&start);
                    let end = Arc::clone(&end);
                    Box::pin(async move { client.get_audit(token, tailnet.as_str(), start.as_str(), end.as_str()).await })
                }),
            )
        } => results,
        _ = wait_for_cancellation(cancellation) => {
            queue
                .send(Event::Admin(Box::new(AdminEvent::Failed {
                    profile,
                    generation,
                    detail: "admin refresh cancelled".to_owned(),
                })))
                .await;
            return;
        }
    };
    let (
        devices,
        users,
        nameservers,
        dns_preferences,
        search_paths,
        split_dns,
        policy,
        credentials,
        settings,
        contacts,
        activity,
    ) = results;
    let report = admin::AdminRefreshReport {
        profile,
        generation,
        observed_at,
        requested_scopes,
        devices: devices.and_then(|response| {
            admin::devices::decode_devices(response.value.devices, response.meta.observed_at)
                .map_err(|_| decode_failure("devices"))
        }),
        users: users.and_then(|response| {
            admin::users::decode_users(response.value.users, response.meta.observed_at)
                .map_err(|_| decode_failure("users"))
        }),
        routes: None,
        nameservers: nameservers.and_then(|response| {
            admin::dns::decode_nameservers(response.value, response.meta.observed_at)
                .map_err(|_| decode_failure("DNS nameservers"))
        }),
        dns_preferences: dns_preferences.map(|response| {
            admin::dns::decode_preferences(response.value, response.meta.observed_at)
        }),
        search_paths: search_paths.and_then(|response| {
            admin::dns::decode_search_paths(response.value, response.meta.observed_at)
                .map_err(|_| decode_failure("DNS search paths"))
        }),
        split_dns: split_dns.and_then(|response| {
            admin::dns::decode_split_dns(response.value, response.meta.observed_at)
                .map_err(|_| decode_failure("split DNS"))
        }),
        policy: policy.map(|response| {
            admin::policy::decode_policy(response.value, response.meta.observed_at)
        }),
        credentials: credentials.and_then(|response| {
            admin::credentials::decode_credentials(response.value.keys, response.meta.observed_at)
                .map_err(|_| decode_failure("credential metadata"))
        }),
        settings: settings.map(|response| admin::decode_settings(response.value)),
        contacts: contacts.map(|response| admin::decode_contacts(response.value)),
        activity: activity.and_then(|response| {
            admin::audit::decode_audit_with_token(
                response.value.logs,
                response.meta.observed_at,
                Some(token.as_str()),
            )
            .map(|mut snapshot| {
                snapshot.version = response.value.version;
                snapshot.tailnet = response.value.tailnet;
                snapshot.start = start.as_ref().clone();
                snapshot.end = end.as_ref().clone();
                snapshot
            })
            .map_err(|_| decode_failure("configuration audit"))
        }),
    };
    queue
        .send(Event::Admin(Box::new(AdminEvent::RefreshFinished(
            Box::new(report),
        ))))
        .await;
}

async fn admin_read_with_replay<T, F>(
    token_manager: &TokenManager,
    profile: &str,
    credential: &str,
    token: &crate::admin::auth::AccessToken,
    cancellation: Cancellation,
    request: F,
) -> Result<T, AdminError>
where
    F: for<'token> Fn(
        &'token crate::admin::auth::AccessToken,
    ) -> Pin<Box<dyn Future<Output = Result<T, AdminError>> + Send + 'token>>,
{
    let result = tokio::select! {
        result = request(token) => result,
        _ = wait_for_cancellation(cancellation.clone()) => Err(AdminError::Cancelled {
            operation: "admin read".to_owned(),
        }),
    };
    if !matches!(&result, Err(AdminError::Unauthenticated)) {
        return result;
    }
    let refreshed = tokio::select! {
        refreshed = token_manager.refresh_after_unauthenticated(profile, credential, token) => refreshed,
        _ = wait_for_cancellation(cancellation.clone()) => {
            Err(crate::admin::auth::AuthError::Cancelled)
        }
    }
    .map_err(|error| admin_refresh_error(error, "refresh admin token"))?;
    let Some(refreshed) = refreshed else {
        return result;
    };
    tokio::select! {
        result = request(&refreshed) => result,
        _ = wait_for_cancellation(cancellation.clone()) => Err(AdminError::Cancelled {
            operation: "admin read".to_owned(),
        }),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_operational_mutation(
    queue: EventQueue,
    token_manager: Arc<TokenManager>,
    action_id: ActionId,
    mutation: OperationalMutation,
    profile: String,
    tailnet: String,
    credential: String,
    timeout: Duration,
) {
    let token = match token_manager.access_token(&profile, &credential).await {
        Ok(token) => token,
        Err(error) => {
            let _ = queue
                .send(Event::Admin(Box::new(AdminEvent::OperationalFinished {
                    action_id,
                    mutation,
                    result: Err(admin_refresh_error(
                        error,
                        "authenticate operational mutation",
                    )),
                    secret: None,
                })))
                .await;
            return;
        }
    };
    let client = match AdminClient::new(timeout) {
        Ok(client) => client,
        Err(error) => {
            let _ = queue
                .send(Event::Admin(Box::new(AdminEvent::OperationalFinished {
                    action_id,
                    mutation,
                    result: Err(error),
                    secret: None,
                })))
                .await;
            return;
        }
    };
    let (result, secret) = execute_operational_mutation(&client, &token, &tailnet, &mutation).await;
    let _ = queue
        .send(Event::Admin(Box::new(AdminEvent::OperationalFinished {
            action_id,
            mutation,
            result,
            secret,
        })))
        .await;
}

#[allow(clippy::too_many_arguments)]
async fn run_access_explorer(
    queue: EventQueue,
    token_manager: Arc<TokenManager>,
    question: crate::domain::access_explorer::AccessQuestion,
    policy: crate::domain::policy_workflow::PolicyDocument,
    profile: String,
    tailnet: String,
    credential: String,
    timeout: Duration,
) {
    let result = async {
        let token = token_manager
            .access_token(&profile, &credential)
            .await
            .map_err(|error| admin_refresh_error(error, "authenticate access explorer"))?;
        let client = AdminClient::new(timeout)?;
        client
            .ask_access(&token, &tailnet, &question, &policy, crate::local::now())
            .await
    }
    .await;
    let _ = queue
        .send(Event::Admin(Box::new(AdminEvent::AccessExplorerFinished {
            result,
        })))
        .await;
}

async fn run_health_evaluation(
    queue: EventQueue,
    generation: u64,
    snapshot: crate::domain::health::HealthSnapshot,
) {
    let result = tokio::task::spawn_blocking(move || {
        let findings = snapshot.findings();
        (snapshot, findings)
    })
    .await;
    let event = match result {
        Ok((snapshot, findings)) => AdminEvent::HealthEvaluationFinished {
            generation,
            snapshot,
            findings,
        },
        Err(error) => AdminEvent::HealthEvaluationFailed {
            generation,
            detail: format!("health evaluation worker failed: {error}"),
        },
    };
    let _ = queue.send(Event::Admin(Box::new(event))).await;
}

async fn execute_operational_mutation(
    client: &AdminClient,
    token: &crate::admin::auth::AccessToken,
    tailnet: &str,
    mutation: &OperationalMutation,
) -> (
    Result<OperationalResult, AdminError>,
    Option<Arc<crate::domain::secret_result::SecretBuffer>>,
) {
    match mutation {
        OperationalMutation::Webhook(webhook) => {
            execute_webhook_mutation(client, token, tailnet, webhook).await
        }
        OperationalMutation::LogStreamReplace(draft) => {
            let replacement = replacement_from_draft(draft);
            let result = match client
                .replace_log_stream_configuration(token, tailnet, &replacement)
                .await
            {
                Err(error) => Err(error),
                Ok(_) => {
                    let configuration = client
                        .get_log_stream_configuration(token, tailnet, draft.log_type)
                        .await;
                    let status = client
                        .get_log_stream_status(token, tailnet, draft.log_type)
                        .await;
                    match (configuration, status) {
                        (Ok(configuration), Ok(status)) => Ok(OperationalResult::Completed {
                            detail: format!(
                                "{} log stream replaced and verified: {} / {}",
                                draft.log_type.wire_value(),
                                configuration.value.destination.kind,
                                status.value.status
                            ),
                        }),
                        (Err(error), _) | (_, Err(error)) => Err(error),
                    }
                }
            };
            (result, None)
        }
        OperationalMutation::LogStreamDelete(log_type) => {
            let result = match client
                .delete_log_stream_configuration(token, tailnet, *log_type)
                .await
            {
                Err(error) => Err(error),
                Ok(_) => {
                    let configuration = client
                        .get_log_stream_configuration(token, tailnet, *log_type)
                        .await;
                    let status = client
                        .get_log_stream_status(token, tailnet, *log_type)
                        .await;
                    match configuration {
                        Ok(_) => Err(AdminError::Conflict {
                            operation: "verify log-stream deletion".to_owned(),
                            detail: "the configuration is still returned after deletion".to_owned(),
                        }),
                        Err(AdminError::NotFound { .. }) => match status {
                            Ok(_) => Err(AdminError::Conflict {
                                operation: "verify log-stream deletion".to_owned(),
                                detail: "the publishing status is still returned after deletion"
                                    .to_owned(),
                            }),
                            Err(AdminError::NotFound { .. }) => Ok(OperationalResult::Completed {
                                detail: format!(
                                    "{} log stream deletion verified absent in configuration and status reads",
                                    log_type.wire_value()
                                ),
                            }),
                            Err(error) => Err(error),
                        },
                        Err(error) => Err(error),
                    }
                }
            };
            (result, None)
        }
        OperationalMutation::NetworkLogSetting { enabled } => {
            let result = match client
                .set_network_log_setting(token, tailnet, *enabled)
                .await
            {
                Err(error) => Err(error),
                Ok(_) => match client.get_network_log_setting(token, tailnet).await {
                    Err(error) => Err(error),
                    Ok(settings) => {
                        let observed = settings.value.network_flow_logging_on;
                        if observed == Some(*enabled) {
                            Ok(OperationalResult::NetworkLogSettingVerified {
                                enabled: observed,
                                detail: format!(
                                    "network-flow collection setting verified as {}",
                                    if *enabled { "enabled" } else { "disabled" }
                                ),
                            })
                        } else {
                            Err(AdminError::Conflict {
                                operation: "verify network-log setting".to_owned(),
                                detail: "the verified setting did not match the requested value"
                                    .to_owned(),
                            })
                        }
                    }
                },
            };
            (result, None)
        }
        OperationalMutation::SavedView(_) | OperationalMutation::Export(_) => (
            Err(AdminError::Unsupported {
                operation: "operational mutation".to_owned(),
                detail: "local saved-view and export operations do not use the admin runtime"
                    .to_owned(),
            }),
            None,
        ),
    }
}

async fn execute_webhook_mutation(
    client: &AdminClient,
    token: &crate::admin::auth::AccessToken,
    tailnet: &str,
    mutation: &crate::domain::webhook::WebhookMutation,
) -> (
    Result<OperationalResult, AdminError>,
    Option<Arc<crate::domain::secret_result::SecretBuffer>>,
) {
    use crate::domain::webhook::WebhookMutation;
    match mutation {
        WebhookMutation::Create(draft) => {
            let response = client
                .create_webhook(
                    token,
                    tailnet,
                    &draft.endpoint_url,
                    &draft.destination_type,
                    &draft.subscriptions,
                )
                .await;
            match response {
                Ok(response) => {
                    let secret = response.secret.map(Arc::new);
                    let verified = async {
                        let _detail = client
                            .get_webhook(token, &response.endpoint.stable_id)
                            .await?;
                        let inventory = client.list_webhooks(token, tailnet).await?;
                        Ok::<_, AdminError>(OperationalResult::WebhookVerified {
                            endpoints: inventory.value,
                            detail: "webhook created; detail and inventory verification completed"
                                .to_owned(),
                        })
                    }
                    .await;
                    (verified, secret)
                }
                Err(error) => (Err(error), None),
            }
        }
        WebhookMutation::EditSubscriptions {
            endpoint_id, after, ..
        } => {
            let result = match client
                .edit_webhook_subscriptions(token, endpoint_id, after)
                .await
            {
                Err(error) => Err(error),
                Ok(_) => {
                    let detail = client.get_webhook(token, endpoint_id).await;
                    let inventory = client.list_webhooks(token, tailnet).await;
                    match (detail, inventory) {
                        (Ok(_), Ok(inventory)) => Ok(OperationalResult::WebhookVerified {
                            endpoints: inventory.value,
                            detail: "webhook subscriptions edited; detail and inventory verification completed".to_owned(),
                        }),
                        (Err(error), _) | (_, Err(error)) => Err(error),
                    }
                }
            };
            (result, None)
        }
        WebhookMutation::Test { endpoint_id } => {
            let result = match client.test_webhook(token, endpoint_id).await {
                Err(error) => Err(error),
                Ok(response) => {
                    let detail = client.get_webhook(token, endpoint_id).await;
                    let inventory = client.list_webhooks(token, tailnet).await;
                    match (detail, inventory) {
                        (Ok(_), Ok(inventory)) => Ok(OperationalResult::WebhookVerified {
                            endpoints: inventory.value,
                            detail: format!(
                                "server acknowledged asynchronous webhook test with HTTP {}; delivery processing is not asserted",
                                response.meta.status
                            ),
                        }),
                        (Err(error), _) | (_, Err(error)) => Err(error),
                    }
                }
            };
            (result, None)
        }
        WebhookMutation::RotateSecret { endpoint_id } => {
            let response = client.rotate_webhook_secret(token, endpoint_id).await;
            match response {
                Ok(response) => {
                    let secret = response.secret.map(Arc::new);
                    let verified = async {
                        let _detail = client.get_webhook(token, endpoint_id).await?;
                        let inventory = client.list_webhooks(token, tailnet).await?;
                        Ok::<_, AdminError>(OperationalResult::WebhookVerified {
                            endpoints: inventory.value,
                            detail: "webhook secret rotated; detail and inventory verification completed".to_owned(),
                        })
                    }
                    .await;
                    (verified, secret)
                }
                Err(error) => (Err(error), None),
            }
        }
        WebhookMutation::Delete { endpoint_id, .. } => {
            let result = match client.delete_webhook(token, endpoint_id).await {
                Err(error) => Err(error),
                Ok(_) => {
                    let detail = client.get_webhook(token, endpoint_id).await;
                    let inventory = client.list_webhooks(token, tailnet).await;
                    match detail {
                        Ok(_) => Err(AdminError::Conflict {
                            operation: "verify webhook deletion".to_owned(),
                            detail: "the endpoint is still returned by detail".to_owned(),
                        }),
                        Err(AdminError::NotFound { .. }) => match inventory {
                            Ok(inventory) => {
                                if inventory
                                    .value
                                    .iter()
                                    .any(|value| value.stable_id.as_str() == endpoint_id.as_str())
                                {
                                    Err(AdminError::Conflict {
                                        operation: "verify webhook deletion".to_owned(),
                                        detail: "the endpoint is still returned by inventory"
                                            .to_owned(),
                                    })
                                } else {
                                    Ok(OperationalResult::WebhookVerified {
                                        endpoints: inventory.value,
                                        detail: "webhook deletion verified by detail and inventory"
                                            .to_owned(),
                                    })
                                }
                            }
                            Err(error) => Err(error),
                        },
                        Err(error) => Err(error),
                    }
                }
            };
            (result, None)
        }
    }
}

fn replacement_from_draft(
    draft: &LogStreamMutationDraft,
) -> crate::admin::log_streaming::LogStreamReplacement {
    crate::admin::log_streaming::LogStreamReplacement {
        log_type: draft.log_type,
        destination_type: draft.destination_type.clone(),
        url: draft.url.clone(),
        user: draft.user.clone(),
        upload_period_minutes: draft.upload_period_minutes,
        compression_format: draft.compression_format.clone(),
        token: draft
            .token
            .as_ref()
            .map(|value| crate::domain::secret_result::SecretBuffer::new(value.as_bytes())),
        s3_bucket: draft.s3_bucket.clone(),
        s3_region: draft.s3_region.clone(),
        s3_key_prefix: draft.s3_key_prefix.clone(),
        s3_authentication_type: draft.s3_authentication_type.clone(),
        s3_access_key_id: draft.s3_access_key_id.clone(),
        s3_role_arn: draft.s3_role_arn.clone(),
        gcs_bucket: draft.gcs_bucket.clone(),
        gcs_key_prefix: draft.gcs_key_prefix.clone(),
        gcs_scopes: draft.gcs_scopes.clone(),
        gcs_credentials: draft
            .gcs_credentials
            .as_ref()
            .map(|value| crate::domain::secret_result::SecretBuffer::new(value.as_bytes())),
    }
}

async fn run_admin_resource_refresh(
    context: AdminTaskContext,
    tailnet: String,
    options: AdminResourceRefreshOptions,
) {
    let AdminTaskContext {
        queue,
        profile,
        credential,
        token_manager,
        generation,
        cancellation,
    } = context;
    let token = tokio::select! {
        result = token_manager.access_token(&profile, &credential) => result,
        _ = wait_for_cancellation(cancellation.clone()) => {
            Err(crate::admin::auth::AuthError::Cancelled)
        }
    };
    let token = match token {
        Ok(token) => token,
        Err(error) => {
            queue
                .send(Event::Admin(Box::new(AdminEvent::AuthenticationFailed {
                    profile,
                    generation,
                    detail: error.to_string(),
                })))
                .await;
            return;
        }
    };
    let requested_scopes = match token_manager.credential_status(&credential) {
        Ok(Some(status)) => status.requested_scopes,
        Ok(None) | Err(_) => Vec::new(),
    };
    let client = match AdminClient::new(options.timeout) {
        Ok(client) => Arc::new(client),
        Err(error) => {
            queue
                .send(Event::Admin(Box::new(AdminEvent::Failed {
                    profile,
                    generation,
                    detail: error.to_string(),
                })))
                .await;
            return;
        }
    };
    let observed_at = crate::local::now();
    let audit_window = if options
        .resources
        .contains(&admin::AdminRefreshResource::Activity)
    {
        let end = match format_utc(observed_at) {
            Some(value) => value,
            None => {
                queue
                    .send(Event::Admin(Box::new(AdminEvent::Failed {
                        profile,
                        generation,
                        detail: "could not construct the audit time window".to_owned(),
                    })))
                    .await;
                return;
            }
        };
        let window_seconds = options
            .audit_window_days
            .clamp(1, 90)
            .saturating_mul(24 * 60 * 60);
        let start = match format_utc(observed_at.saturating_sub(window_seconds)) {
            Some(value) => value,
            None => {
                queue
                    .send(Event::Admin(Box::new(AdminEvent::Failed {
                        profile,
                        generation,
                        detail: "could not construct the audit start time".to_owned(),
                    })))
                    .await;
                return;
            }
        };
        Some((start, end))
    } else {
        None
    };
    let tailnet = Arc::new(tailnet);
    let audit_window = audit_window.map(|(start, end)| (Arc::new(start), Arc::new(end)));
    let mut results = Vec::with_capacity(options.resources.len());
    for resource in options.resources {
        if cancellation.is_cancelled() {
            return;
        }
        let result = match resource {
            admin::AdminRefreshResource::Devices => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(async move { client.list_devices(token, tailnet.as_str()).await })
                    },
                )
                .await;
                admin::AdminResourceResult::Devices(response.and_then(|response| {
                    admin::devices::decode_devices(
                        response.value.devices,
                        response.meta.observed_at,
                    )
                    .map_err(|_| decode_failure("devices"))
                }))
            }
            admin::AdminRefreshResource::DeviceRoutes(device_id) => {
                let client = Arc::clone(&client);
                let device_id = device_id.clone();
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let device_id = device_id.clone();
                        Box::pin(async move { client.get_routes(token, &device_id).await })
                    },
                )
                .await;
                admin::AdminResourceResult::DeviceRoutes(response.and_then(|response| {
                    admin::routes::decode_routes(
                        device_id,
                        response.value,
                        response.meta.observed_at,
                    )
                    .map_err(|_| decode_failure("device routes"))
                }))
            }
            admin::AdminRefreshResource::Users => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(async move { client.list_users(token, tailnet.as_str()).await })
                    },
                )
                .await;
                admin::AdminResourceResult::Users(response.and_then(|response| {
                    admin::users::decode_users(response.value.users, response.meta.observed_at)
                        .map_err(|_| decode_failure("users"))
                }))
            }
            admin::AdminRefreshResource::Nameservers => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(
                            async move { client.get_nameservers(token, tailnet.as_str()).await },
                        )
                    },
                )
                .await;
                admin::AdminResourceResult::Nameservers(response.and_then(|response| {
                    admin::dns::decode_nameservers(response.value, response.meta.observed_at)
                        .map_err(|_| decode_failure("DNS nameservers"))
                }))
            }
            admin::AdminRefreshResource::DnsPreferences => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(async move {
                            client.get_dns_preferences(token, tailnet.as_str()).await
                        })
                    },
                )
                .await;
                admin::AdminResourceResult::DnsPreferences(response.map(|response| {
                    admin::dns::decode_preferences(response.value, response.meta.observed_at)
                }))
            }
            admin::AdminRefreshResource::SearchPaths => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(
                            async move { client.get_search_paths(token, tailnet.as_str()).await },
                        )
                    },
                )
                .await;
                admin::AdminResourceResult::SearchPaths(response.and_then(|response| {
                    admin::dns::decode_search_paths(response.value, response.meta.observed_at)
                        .map_err(|_| decode_failure("DNS search paths"))
                }))
            }
            admin::AdminRefreshResource::SplitDns => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(async move { client.get_split_dns(token, tailnet.as_str()).await })
                    },
                )
                .await;
                admin::AdminResourceResult::SplitDns(response.and_then(|response| {
                    admin::dns::decode_split_dns(response.value, response.meta.observed_at)
                        .map_err(|_| decode_failure("split DNS"))
                }))
            }
            admin::AdminRefreshResource::Policy => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(async move { client.get_policy(token, tailnet.as_str()).await })
                    },
                )
                .await;
                admin::AdminResourceResult::Policy(response.map(|response| {
                    admin::policy::decode_policy(response.value, response.meta.observed_at)
                }))
            }
            admin::AdminRefreshResource::Credentials => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(async move { client.list_keys(token, tailnet.as_str()).await })
                    },
                )
                .await;
                admin::AdminResourceResult::Credentials(response.and_then(|response| {
                    admin::credentials::decode_credentials(
                        response.value.keys,
                        response.meta.observed_at,
                    )
                    .map_err(|_| decode_failure("credential metadata"))
                }))
            }
            admin::AdminRefreshResource::Settings => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(async move { client.get_settings(token, tailnet.as_str()).await })
                    },
                )
                .await;
                admin::AdminResourceResult::Settings(
                    response.map(|response| admin::decode_settings(response.value)),
                )
            }
            admin::AdminRefreshResource::Contacts => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(async move { client.get_contacts(token, tailnet.as_str()).await })
                    },
                )
                .await;
                admin::AdminResourceResult::Contacts(
                    response.map(|response| admin::decode_contacts(response.value)),
                )
            }
            admin::AdminRefreshResource::FlowLogs(window) => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let requested_window = window.clone();
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        let window = requested_window.clone();
                        Box::pin(async move {
                            client
                                .get_network_flow_logs(token, tailnet.as_str(), &window)
                                .await
                        })
                    },
                )
                .await;
                admin::AdminResourceResult::FlowLogs(Box::new(response.and_then(|response| {
                    crate::domain::flow::FlowSnapshot::from_messages(
                        window,
                        response.value,
                        crate::domain::flow::FlowMode::Raw,
                        response.meta.observed_at,
                    )
                    .map_err(|error| AdminError::DecodeFailed {
                        operation: "get network flow logs".to_owned(),
                        detail: error.to_string(),
                    })
                })))
            }
            admin::AdminRefreshResource::Webhooks => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(async move { client.list_webhooks(token, tailnet.as_str()).await })
                    },
                )
                .await;
                admin::AdminResourceResult::Webhooks(
                    response.map(|response| (response.value, response.meta)),
                )
            }
            admin::AdminRefreshResource::LogStreamConfiguration(log_type) => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(async move {
                            client
                                .get_log_stream_configuration(token, tailnet.as_str(), log_type)
                                .await
                        })
                    },
                )
                .await;
                admin::AdminResourceResult::LogStreamConfiguration {
                    log_type,
                    result: response.map(|value| value.value),
                }
            }
            admin::AdminRefreshResource::LogStreamStatus(log_type) => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(async move {
                            client
                                .get_log_stream_status(token, tailnet.as_str(), log_type)
                                .await
                        })
                    },
                )
                .await;
                admin::AdminResourceResult::LogStreamStatus {
                    log_type,
                    result: response.map(|value| value.value),
                }
            }
            admin::AdminRefreshResource::NetworkLogSettings => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let response = admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let tailnet = Arc::clone(&tailnet);
                        Box::pin(async move {
                            client
                                .get_network_log_setting(token, tailnet.as_str())
                                .await
                        })
                    },
                )
                .await;
                admin::AdminResourceResult::NetworkLogSettings(
                    response.map(|response| admin::decode_settings(response.value)),
                )
            }
            admin::AdminRefreshResource::Activity => {
                let client = Arc::clone(&client);
                let tailnet = Arc::clone(&tailnet);
                let result = match audit_window.as_ref() {
                    Some((start, end)) => admin_read_with_replay(
                        &token_manager,
                        &profile,
                        &credential,
                        &token,
                        cancellation.clone(),
                        |token| {
                            let client = Arc::clone(&client);
                            let tailnet = Arc::clone(&tailnet);
                            let start = Arc::clone(start);
                            let end = Arc::clone(end);
                            Box::pin(async move {
                                client
                                    .get_audit(
                                        token,
                                        tailnet.as_str(),
                                        start.as_str(),
                                        end.as_str(),
                                    )
                                    .await
                            })
                        },
                    )
                    .await
                    .and_then(|response| {
                        admin::audit::decode_audit_with_token(
                            response.value.logs,
                            response.meta.observed_at,
                            Some(token.as_str()),
                        )
                        .map(|mut snapshot| {
                            snapshot.version = response.value.version;
                            snapshot.tailnet = response.value.tailnet;
                            snapshot.start = start.as_ref().clone();
                            snapshot.end = end.as_ref().clone();
                            snapshot
                        })
                        .map_err(|_| decode_failure("configuration audit"))
                    }),
                    None => Err(AdminError::ValidationFailed {
                        operation: "configuration audit".to_owned(),
                        detail: "the audit window was not constructed".to_owned(),
                    }),
                };
                admin::AdminResourceResult::Activity(result)
            }
        };
        results.push(result);
    }
    queue
        .send(Event::Admin(Box::new(AdminEvent::ResourceRefreshFinished(
            Box::new(admin::AdminResourceReport {
                profile,
                generation,
                observed_at,
                requested_scopes,
                resources: results,
            }),
        ))))
        .await;
}

fn admin_refresh_error(error: crate::admin::auth::AuthError, operation: &str) -> AdminError {
    match error {
        crate::admin::auth::AuthError::Cancelled => AdminError::Cancelled {
            operation: operation.to_owned(),
        },
        crate::admin::auth::AuthError::TimedOut => AdminError::TimedOut {
            operation: operation.to_owned(),
        },
        crate::admin::auth::AuthError::Unauthenticated => AdminError::Unauthenticated,
        _ => AdminError::Unauthenticated,
    }
}

async fn run_admin_device_enrichment(
    context: AdminTaskContext,
    device_id: String,
    timeout: Duration,
) {
    let AdminTaskContext {
        queue,
        profile,
        credential,
        token_manager,
        generation,
        cancellation,
    } = context;
    let token = match tokio::select! {
        result = token_manager.access_token(&profile, &credential) => result,
        _ = wait_for_cancellation(cancellation.clone()) => {
            Err(crate::admin::auth::AuthError::Cancelled)
        }
    } {
        Ok(token) => token,
        Err(error) => {
            queue
                .send(Event::Admin(Box::new(AdminEvent::DeviceEnrichmentFailed {
                    profile,
                    generation,
                    device_id: device_id.clone(),
                    detail: error.to_string(),
                })))
                .await;
            return;
        }
    };
    let client = match AdminClient::new(timeout) {
        Ok(client) => client,
        Err(error) => {
            queue
                .send(Event::Admin(Box::new(AdminEvent::DeviceEnrichmentFailed {
                    profile,
                    generation,
                    device_id: device_id.clone(),
                    detail: error.to_string(),
                })))
                .await;
            return;
        }
    };
    let client = Arc::new(client);
    let device_id = Arc::new(device_id);
    let enrichment_cancellation = cancellation.clone();
    let (device, routes, posture) = tokio::select! {
        result = async {
            tokio::join!(
                admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    enrichment_cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let device_id = Arc::clone(&device_id);
                        Box::pin(async move { client.get_device(token, device_id.as_str()).await })
                    },
                ),
                admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    enrichment_cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let device_id = Arc::clone(&device_id);
                        Box::pin(async move { client.get_routes(token, device_id.as_str()).await })
                    },
                ),
                admin_read_with_replay(
                    &token_manager,
                    &profile,
                    &credential,
                    &token,
                    enrichment_cancellation.clone(),
                    |token| {
                        let client = Arc::clone(&client);
                        let device_id = Arc::clone(&device_id);
                        Box::pin(async move { client.get_posture(token, device_id.as_str()).await })
                    },
                ),
            )
        } => result,
        _ = wait_for_cancellation(cancellation) => {
            return;
        }
    };
    let observed_at = crate::local::now();
    let device = match device.and_then(|response| {
        admin::devices::decode_device(response.value, response.meta.observed_at)
            .map_err(|_| decode_failure("device detail"))
    }) {
        Ok(device) => device,
        Err(error) => {
            queue
                .send(Event::Admin(Box::new(AdminEvent::DeviceEnrichmentFailed {
                    profile,
                    generation,
                    device_id: device_id.as_ref().clone(),
                    detail: error.to_string(),
                })))
                .await;
            return;
        }
    };
    let (routes, routes_error) = match routes {
        Ok(response) => match admin::routes::decode_routes(
            device_id.as_ref().clone(),
            response.value,
            response.meta.observed_at,
        ) {
            Ok(routes) => (Some(routes), None),
            Err(_) => (None, Some(decode_failure("device routes"))),
        },
        Err(error) => (None, Some(error)),
    };
    let (posture_present, posture_error) = match posture {
        Ok(response) => (
            Some(response.value.attributes.is_some() || response.value.expiries.is_some()),
            None,
        ),
        Err(error) => (None, Some(error)),
    };
    let mut device = device;
    device.source_observed_at = observed_at;
    queue
        .send(Event::Admin(Box::new(
            AdminEvent::DeviceEnrichmentFinished {
                profile,
                generation,
                device: Box::new(device),
                routes,
                routes_error,
                posture_present,
                posture_error,
            },
        )))
        .await;
}

async fn run_admin_preflight(
    queue: EventQueue,
    token_manager: Arc<TokenManager>,
    request: AdminMutationRequest,
    tailnet: String,
    credential: String,
    timeout: Duration,
) {
    let profile = request.profile.clone();
    let cancellation = Cancellation::new();
    let token = tokio::select! {
        result = token_manager.access_token(&profile, &credential) => result,
        _ = wait_for_cancellation(cancellation.clone()) => Err(crate::admin::auth::AuthError::Cancelled),
    };
    let result = match token {
        Ok(token) => match AdminClient::new(timeout) {
            Ok(client) => {
                let context = AdminReadContext {
                    client: &client,
                    token_manager: &token_manager,
                    profile: &profile,
                    credential: &credential,
                    token: &token,
                    tailnet: &tailnet,
                    cancellation,
                };
                fetch_admin_preflight(&context, &request).await
            }
            Err(error) => Err(error),
        },
        Err(error) => Err(admin_refresh_error(error, "preflight admin mutation")),
    };
    let (result, observed_at, owned_device_context) = match result {
        Ok((fields, observed_at, owned_device_context)) => {
            (Ok(fields), observed_at, owned_device_context)
        }
        Err(error) => (Err(error), crate::local::now(), Vec::new()),
    };
    queue
        .send(Event::Admin(Box::new(AdminEvent::PreflightFinished {
            request: Box::new(request),
            result,
            observed_at,
            owned_device_context,
        })))
        .await;
}

struct AdminReadContext<'a> {
    client: &'a AdminClient,
    token_manager: &'a TokenManager,
    profile: &'a str,
    credential: &'a str,
    token: &'a crate::admin::auth::AccessToken,
    tailnet: &'a str,
    cancellation: Cancellation,
}

async fn fetch_admin_preflight(
    context: &AdminReadContext<'_>,
    request: &AdminMutationRequest,
) -> Result<(AdminSnapshotFields, crate::domain::Timestamp, Vec<String>), AdminError> {
    let client = context.client;
    let token_manager = context.token_manager;
    let profile = context.profile;
    let credential = context.credential;
    let token = context.token;
    let tailnet = context.tailnet;
    let cancellation = context.cancellation.clone();
    match &request.change {
        crate::domain::admin_mutation::AdminChange::DeviceRoutes { .. } => {
            let device_id = request.target_id.clone();
            let response = admin_read_with_replay(
                token_manager,
                profile,
                credential,
                token,
                cancellation,
                |token| {
                    let client = client.clone();
                    let device_id = device_id.clone();
                    Box::pin(async move { client.get_routes(token, &device_id).await })
                },
            )
            .await?;
            let routes = admin::routes::decode_routes(
                request.target_id.clone(),
                response.value,
                response.meta.observed_at,
            )
            .map_err(|_| decode_failure("device routes"))?;
            Ok((
                crate::admin::mutation::route_fields(&routes.advertised, &routes.enabled),
                routes.observed_at,
                Vec::new(),
            ))
        }
        crate::domain::admin_mutation::AdminChange::DeviceRename { .. }
        | crate::domain::admin_mutation::AdminChange::DeviceTags { .. }
        | crate::domain::admin_mutation::AdminChange::DeviceApproval { .. }
        | crate::domain::admin_mutation::AdminChange::DeviceKeyExpiry { .. }
        | crate::domain::admin_mutation::AdminChange::DeviceExpireNow
        | crate::domain::admin_mutation::AdminChange::DeviceDelete => {
            let device_id = request.target_id.clone();
            let response = admin_read_with_replay(
                token_manager,
                profile,
                credential,
                token,
                cancellation.clone(),
                |token| {
                    let client = client.clone();
                    let device_id = device_id.clone();
                    Box::pin(async move { client.get_device(token, &device_id).await })
                },
            )
            .await?;
            let device = admin::devices::decode_device(response.value, response.meta.observed_at)
                .map_err(|_| decode_failure("device detail"))?;
            let mut fields = device_fields(&device);
            if matches!(
                &request.change,
                crate::domain::admin_mutation::AdminChange::DeviceDelete
            ) {
                let routes = admin_read_with_replay(
                    token_manager,
                    profile,
                    credential,
                    token,
                    cancellation,
                    |token| {
                        let client = client.clone();
                        let device_id = request.target_id.clone();
                        Box::pin(async move { client.get_routes(token, &device_id).await })
                    },
                )
                .await?;
                let routes = admin::routes::decode_routes(
                    request.target_id.clone(),
                    routes.value,
                    routes.meta.observed_at,
                )
                .map_err(|_| decode_failure("device routes"))?;
                fields
                    .values
                    .insert("advertisedRoutes".to_owned(), routes.advertised.join(","));
                fields
                    .values
                    .insert("enabledRoutes".to_owned(), routes.enabled.join(","));
            }
            Ok((fields, response.meta.observed_at, Vec::new()))
        }
        crate::domain::admin_mutation::AdminChange::UserApproval
        | crate::domain::admin_mutation::AdminChange::UserRole { .. }
        | crate::domain::admin_mutation::AdminChange::UserSuspend
        | crate::domain::admin_mutation::AdminChange::UserRestore
        | crate::domain::admin_mutation::AdminChange::UserDelete => {
            let user_id = request.target_id.clone();
            let response = admin_read_with_replay(
                token_manager,
                profile,
                credential,
                token,
                cancellation.clone(),
                |token| {
                    let client = client.clone();
                    let user_id = user_id.clone();
                    Box::pin(async move { client.get_user(token, &user_id).await })
                },
            )
            .await?;
            let user = admin::users::decode_user(response.value)
                .map_err(|_| decode_failure("user detail"))?;
            let devices = admin_read_with_replay(
                token_manager,
                profile,
                credential,
                token,
                cancellation,
                |token| {
                    let client = client.clone();
                    let tailnet = tailnet.to_owned();
                    Box::pin(async move { client.list_devices(token, &tailnet).await })
                },
            )
            .await?;
            let devices =
                admin::devices::decode_devices(devices.value.devices, devices.meta.observed_at)
                    .map_err(|_| decode_failure("devices for user preflight"))?;
            Ok((
                user_fields(&user),
                response.meta.observed_at.max(user.created_at.unwrap_or(0)),
                crate::admin::user_mutations::owned_device_context(&user, &devices),
            ))
        }
        crate::domain::admin_mutation::AdminChange::DnsNameservers { .. } => {
            let response = admin_read_with_replay(
                token_manager,
                profile,
                credential,
                token,
                cancellation,
                |token| {
                    let client = client.clone();
                    let tailnet = tailnet.to_owned();
                    Box::pin(async move { client.get_nameservers(token, &tailnet).await })
                },
            )
            .await?;
            let value = admin::dns::decode_nameservers(response.value, response.meta.observed_at)
                .map_err(|_| decode_failure("DNS nameservers"))?;
            Ok((nameserver_fields(&value), value.observed_at, Vec::new()))
        }
        crate::domain::admin_mutation::AdminChange::DnsPreferences { .. } => {
            let response = admin_read_with_replay(
                token_manager,
                profile,
                credential,
                token,
                cancellation,
                |token| {
                    let client = client.clone();
                    let tailnet = tailnet.to_owned();
                    Box::pin(async move { client.get_dns_preferences(token, &tailnet).await })
                },
            )
            .await?;
            let value = admin::dns::decode_preferences(response.value, response.meta.observed_at);
            Ok((
                dns_preferences_fields(&value),
                value.observed_at,
                Vec::new(),
            ))
        }
        crate::domain::admin_mutation::AdminChange::DnsSearchPaths { .. } => {
            let response = admin_read_with_replay(
                token_manager,
                profile,
                credential,
                token,
                cancellation,
                |token| {
                    let client = client.clone();
                    let tailnet = tailnet.to_owned();
                    Box::pin(async move { client.get_search_paths(token, &tailnet).await })
                },
            )
            .await?;
            let value = admin::dns::decode_search_paths(response.value, response.meta.observed_at)
                .map_err(|_| decode_failure("DNS search paths"))?;
            Ok((search_path_fields(&value), value.observed_at, Vec::new()))
        }
        crate::domain::admin_mutation::AdminChange::DnsSplitMapping { .. } => {
            let response = admin_read_with_replay(
                token_manager,
                profile,
                credential,
                token,
                cancellation,
                |token| {
                    let client = client.clone();
                    let tailnet = tailnet.to_owned();
                    Box::pin(async move { client.get_split_dns(token, &tailnet).await })
                },
            )
            .await?;
            let value = admin::dns::decode_split_dns(response.value, response.meta.observed_at)
                .map_err(|_| decode_failure("split DNS"))?;
            Ok((split_dns_fields(&value), value.observed_at, Vec::new()))
        }
    }
}

struct AdminMutationTask {
    queue: EventQueue,
    token_manager: Arc<TokenManager>,
    task_id: TaskId,
    request: AdminMutationRequest,
    tailnet: String,
    credential: String,
    timeout: Duration,
    cancellation: Cancellation,
}

async fn run_admin_mutation(task: AdminMutationTask) {
    let AdminMutationTask {
        queue,
        token_manager,
        task_id,
        request,
        tailnet,
        credential,
        timeout,
        cancellation,
    } = task;
    let profile = request.profile.clone();
    let mutation_id = request.mutation_id;
    queue
        .send(Event::Task(Box::new(TaskEvent::Started { task_id })))
        .await;
    let token = tokio::select! {
        result = token_manager.access_token(&profile, &credential) => result,
        _ = wait_for_cancellation(cancellation.clone()) => Err(crate::admin::auth::AuthError::Cancelled),
    };
    let token = match token {
        Ok(token) => token,
        Err(error) => {
            let outcome = AdminMutationOutcome {
                mutation_id,
                state: crate::domain::admin_mutation::AdminMutationState::Failed,
                detail: error.to_string(),
                verification: "not attempted; mutation was not dispatched".to_owned(),
                audit: crate::domain::admin_mutation::AuditCorrelation::none(),
            };
            send_admin_mutation_finished(queue, task_id, request, outcome, false).await;
            return;
        }
    };
    let client = match AdminClient::new(timeout) {
        Ok(client) => client,
        Err(error) => {
            let outcome = AdminMutationOutcome {
                mutation_id,
                state: crate::domain::admin_mutation::AdminMutationState::Failed,
                detail: error.to_string(),
                verification: "not attempted; mutation was not dispatched".to_owned(),
                audit: crate::domain::admin_mutation::AuditCorrelation::none(),
            };
            send_admin_mutation_finished(queue, task_id, request, outcome, false).await;
            return;
        }
    };
    let dispatched_at = crate::local::now();
    let mutation_result = tokio::select! {
        result = dispatch_admin_change(&client, &token, &request, &tailnet) => result,
        _ = wait_for_cancellation(cancellation.clone()) => Err(AdminError::Cancelled {
            operation: request.action_id.as_str().to_owned(),
        }),
    };
    let read_context = AdminReadContext {
        client: &client,
        token_manager: &token_manager,
        profile: &profile,
        credential: &credential,
        token: &token,
        tailnet: &tailnet,
        cancellation: cancellation.clone(),
    };
    let verification = verify_admin_change_until(&read_context, &request).await;
    let uncertain_request = mutation_result
        .as_ref()
        .err()
        .is_some_and(uncertain_admin_error);
    let (state, detail, verification_text) = match (&mutation_result, &verification) {
        (Ok(_), Ok(VerificationResult::Verified(detail))) => (
            crate::domain::admin_mutation::AdminMutationState::Succeeded,
            "mutation returned success".to_owned(),
            detail.clone(),
        ),
        (Err(error), Ok(VerificationResult::Verified(detail))) if uncertain_request => (
            crate::domain::admin_mutation::AdminMutationState::Succeeded,
            format!(
                "mutation response was uncertain after {}; read matches",
                error
            ),
            detail.clone(),
        ),
        (Err(error), Ok(VerificationResult::Verified(detail))) => (
            crate::domain::admin_mutation::AdminMutationState::Failed,
            format!("mutation was rejected; authoritative state already matched: {error}"),
            detail.clone(),
        ),
        (Ok(_), Ok(VerificationResult::Mismatch(detail))) => (
            crate::domain::admin_mutation::AdminMutationState::Failed,
            "authoritative verification returned a mismatch".to_owned(),
            detail.clone(),
        ),
        (Ok(_), Err(error)) => (
            crate::domain::admin_mutation::AdminMutationState::SucceededUnverified,
            "mutation returned success but the authoritative read failed".to_owned(),
            error.to_string(),
        ),
        (Err(error), Ok(VerificationResult::Mismatch(detail))) if uncertain_request => (
            crate::domain::admin_mutation::AdminMutationState::OutcomeUnknown,
            format!("mutation outcome is unknown after {}", error),
            detail.clone(),
        ),
        (Err(error), Err(verification_error)) if uncertain_request => (
            crate::domain::admin_mutation::AdminMutationState::OutcomeUnknown,
            format!(
                "mutation outcome is unknown after {}; verification failed",
                error
            ),
            verification_error.to_string(),
        ),
        (Err(error), Ok(VerificationResult::Mismatch(detail))) => (
            crate::domain::admin_mutation::AdminMutationState::Failed,
            error.to_string(),
            detail.clone(),
        ),
        (Err(error), Err(verification_error)) => (
            crate::domain::admin_mutation::AdminMutationState::Failed,
            error.to_string(),
            verification_error.to_string(),
        ),
    };
    let should_correlate = state == crate::domain::admin_mutation::AdminMutationState::Succeeded;
    let outcome = AdminMutationOutcome {
        mutation_id,
        state,
        detail,
        verification: verification_text,
        audit: crate::domain::admin_mutation::AuditCorrelation::none(),
    };
    let audit_job = should_correlate.then(|| {
        (
            queue.clone(),
            token_manager.clone(),
            client.clone(),
            profile.clone(),
            credential.clone(),
            request.clone(),
            tailnet.clone(),
            cancellation.clone(),
        )
    });
    send_admin_mutation_finished(
        queue,
        task_id,
        request,
        outcome,
        state == crate::domain::admin_mutation::AdminMutationState::Succeeded,
    )
    .await;
    if let Some((
        audit_queue,
        audit_token_manager,
        audit_client,
        audit_profile,
        audit_credential,
        audit_request,
        audit_tailnet,
        audit_cancellation,
    )) = audit_job
    {
        tokio::spawn(async move {
            run_admin_audit_correlation(AdminAuditJob {
                queue: audit_queue,
                token_manager: audit_token_manager,
                client: audit_client,
                profile: audit_profile,
                credential: audit_credential,
                task_id,
                request: audit_request,
                tailnet: audit_tailnet,
                dispatched_at,
                cancellation: audit_cancellation,
            })
            .await;
        });
    }
}

async fn dispatch_admin_change(
    client: &AdminClient,
    token: &crate::admin::auth::AccessToken,
    request: &AdminMutationRequest,
    tailnet: &str,
) -> Result<(), AdminError> {
    use crate::domain::admin_mutation::AdminChange;
    match &request.change {
        AdminChange::DeviceRename { name } => {
            crate::admin::device_mutations::validate_machine_name(name).map_err(|detail| {
                AdminError::ValidationFailed {
                    operation: request.action_id.as_str().to_owned(),
                    detail,
                }
            })?;
            client
                .set_device_name(token, &request.target_id, name)
                .await
                .map(|_| ())
        }
        AdminChange::DeviceTags { tags } => {
            let tags = crate::admin::device_mutations::canonical_tags(tags).map_err(|detail| {
                AdminError::ValidationFailed {
                    operation: request.action_id.as_str().to_owned(),
                    detail,
                }
            })?;
            client
                .set_device_tags(token, &request.target_id, &tags)
                .await
                .map(|_| ())
        }
        AdminChange::DeviceApproval { authorized } => client
            .set_device_authorized(token, &request.target_id, *authorized)
            .await
            .map(|_| ()),
        AdminChange::DeviceKeyExpiry { disabled } => client
            .set_device_key_expiry(token, &request.target_id, *disabled)
            .await
            .map(|_| ()),
        AdminChange::DeviceExpireNow => client
            .expire_device_key(token, &request.target_id)
            .await
            .map(|_| ()),
        AdminChange::DeviceDelete => client
            .delete_device(token, &request.target_id)
            .await
            .map(|_| ()),
        AdminChange::DeviceRoutes { routes } => {
            let routes = crate::admin::route_mutations::canonical_enabled_routes(routes).map_err(
                |detail| AdminError::ValidationFailed {
                    operation: request.action_id.as_str().to_owned(),
                    detail,
                },
            )?;
            let advertised = request
                .preflight
                .as_ref()
                .and_then(|preflight| preflight.fields.get("advertisedRoutes"))
                .map_or_else(Vec::new, |value| {
                    value.split(',').map(str::to_owned).collect::<Vec<_>>()
                });
            let currently_enabled = request
                .preflight
                .as_ref()
                .and_then(|preflight| preflight.fields.get("enabledRoutes"))
                .map_or_else(Vec::new, |value| {
                    value.split(',').map(str::to_owned).collect::<Vec<_>>()
                });
            let routes = crate::admin::route_mutations::validate_replacement(
                &advertised,
                &currently_enabled,
                &routes,
            )
            .map_err(|detail| AdminError::ValidationFailed {
                operation: request.action_id.as_str().to_owned(),
                detail,
            })?;
            client
                .set_device_routes(token, &request.target_id, &routes)
                .await
                .map(|_| ())
        }
        AdminChange::DnsNameservers { values } => {
            let values = crate::admin::dns_mutations::canonical_resolvers(values, "nameserver")
                .map_err(|detail| AdminError::ValidationFailed {
                    operation: request.action_id.as_str().to_owned(),
                    detail,
                })?;
            client
                .set_nameservers(token, tailnet, &values)
                .await
                .map(|_| ())
        }
        AdminChange::DnsPreferences { magic_dns } => client
            .set_dns_preferences(token, tailnet, *magic_dns)
            .await
            .map(|_| ()),
        AdminChange::DnsSearchPaths { values } => {
            let values =
                crate::admin::dns_mutations::canonical_ordered_values(values, "search path")
                    .map_err(|detail| AdminError::ValidationFailed {
                        operation: request.action_id.as_str().to_owned(),
                        detail,
                    })?;
            for value in &values {
                crate::admin::dns_mutations::validate_domain(value).map_err(|detail| {
                    AdminError::ValidationFailed {
                        operation: request.action_id.as_str().to_owned(),
                        detail,
                    }
                })?;
            }
            client
                .set_search_paths(token, tailnet, &values)
                .await
                .map(|_| ())
        }
        AdminChange::DnsSplitMapping {
            domain, resolvers, ..
        } => {
            let body =
                crate::admin::dns_mutations::split_mapping_body(domain, resolvers.as_deref())
                    .map_err(|detail| AdminError::ValidationFailed {
                        operation: request.action_id.as_str().to_owned(),
                        detail,
                    })?;
            client
                .patch_split_dns(token, tailnet, body)
                .await
                .map(|_| ())
        }
        AdminChange::UserApproval => client
            .approve_user(token, &request.target_id)
            .await
            .map(|_| ()),
        AdminChange::UserRole { role } => {
            let role = crate::admin::user_mutations::validate_role(role).map_err(|detail| {
                AdminError::ValidationFailed {
                    operation: request.action_id.as_str().to_owned(),
                    detail,
                }
            })?;
            client
                .set_user_role(token, &request.target_id, &role)
                .await
                .map(|_| ())
        }
        AdminChange::UserSuspend => client
            .suspend_user(token, &request.target_id)
            .await
            .map(|_| ()),
        AdminChange::UserRestore => client
            .restore_user(token, &request.target_id)
            .await
            .map(|_| ()),
        AdminChange::UserDelete => client
            .delete_user(token, &request.target_id)
            .await
            .map(|_| ()),
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum VerificationResult {
    Verified(String),
    Mismatch(String),
}

async fn verify_admin_change_until(
    context: &AdminReadContext<'_>,
    request: &AdminMutationRequest,
) -> Result<VerificationResult, AdminError> {
    let deadline = Instant::now() + ADMIN_VERIFICATION_DEADLINE;
    loop {
        match verify_admin_change(context, request).await {
            Ok(VerificationResult::Verified(detail)) => {
                return Ok(VerificationResult::Verified(detail));
            }
            Ok(VerificationResult::Mismatch(detail)) => {
                if Instant::now() >= deadline {
                    return Ok(VerificationResult::Mismatch(detail));
                }
            }
            Err(error) if retryable_verification_error(&error) => {
                if Instant::now() >= deadline {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            continue;
        }
        let wait = remaining.min(ADMIN_VERIFICATION_POLL);
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = wait_for_cancellation(context.cancellation.clone()) => {
                return Err(AdminError::Cancelled {
                    operation: "verify admin mutation".to_owned(),
                });
            }
        }
    }
}

fn retryable_verification_error(error: &AdminError) -> bool {
    matches!(
        error,
        AdminError::Transport { .. }
            | AdminError::TimedOut { .. }
            | AdminError::RateLimited { .. }
            | AdminError::ServerFailure { .. }
    )
}

async fn verify_admin_change(
    context: &AdminReadContext<'_>,
    request: &AdminMutationRequest,
) -> Result<VerificationResult, AdminError> {
    let client = context.client;
    let token_manager = context.token_manager;
    let profile = context.profile;
    let credential = context.credential;
    let token = context.token;
    let tailnet = context.tailnet;
    let cancellation = context.cancellation.clone();
    use crate::domain::admin_mutation::AdminChange;
    match &request.change {
        AdminChange::DeviceDelete => match admin_read_with_replay(
            token_manager,
            profile,
            credential,
            token,
            cancellation,
            |token| {
                let client = client.clone();
                let device_id = request.target_id.clone();
                Box::pin(async move { client.get_device(token, &device_id).await })
            },
        )
        .await
        {
            Err(AdminError::NotFound { .. }) => Ok(VerificationResult::Verified(
                "GET device returned not found; deletion is authoritative".to_owned(),
            )),
            Ok(_) => Ok(VerificationResult::Mismatch(
                "device still exists after delete".to_owned(),
            )),
            Err(error) => Err(error),
        },
        AdminChange::DeviceRename { name } => {
            let device = read_device(
                client,
                token_manager,
                profile,
                credential,
                token,
                request,
                cancellation,
            )
            .await?;
            let device = admin::devices::decode_device(device.value, device.meta.observed_at)
                .map_err(|_| decode_failure("device verification"))?;
            Ok(
                match crate::admin::device_mutations::verify_name(&device, name) {
                    Ok(()) => VerificationResult::Verified(format!(
                        "canonical name: {}",
                        device.display_name()
                    )),
                    Err(detail) => VerificationResult::Mismatch(detail),
                },
            )
        }
        AdminChange::DeviceTags { tags } => {
            let device = read_device(
                client,
                token_manager,
                profile,
                credential,
                token,
                request,
                cancellation,
            )
            .await?;
            let device = admin::devices::decode_device(device.value, device.meta.observed_at)
                .map_err(|_| decode_failure("device verification"))?;
            Ok(
                match crate::admin::device_mutations::verify_tags(&device, tags) {
                    Ok(()) => {
                        VerificationResult::Verified("complete returned tag set matches".to_owned())
                    }
                    Err(detail) => VerificationResult::Mismatch(detail),
                },
            )
        }
        AdminChange::DeviceApproval { authorized } => {
            let device = read_device(
                client,
                token_manager,
                profile,
                credential,
                token,
                request,
                cancellation,
            )
            .await?;
            let device = admin::devices::decode_device(device.value, device.meta.observed_at)
                .map_err(|_| decode_failure("device verification"))?;
            Ok(
                match crate::admin::device_mutations::verify_approval(&device, *authorized) {
                    Ok(()) => VerificationResult::Verified(format!("approval: {authorized}")),
                    Err(detail) => VerificationResult::Mismatch(detail),
                },
            )
        }
        AdminChange::DeviceKeyExpiry { disabled } => {
            let device = read_device(
                client,
                token_manager,
                profile,
                credential,
                token,
                request,
                cancellation,
            )
            .await?;
            let device = admin::devices::decode_device(device.value, device.meta.observed_at)
                .map_err(|_| decode_failure("device verification"))?;
            Ok(
                match crate::admin::device_mutations::verify_key_expiry(&device, *disabled) {
                    Ok(()) => VerificationResult::Verified(format!(
                        "key expiry disabled: {disabled}; server expiry timestamp: {:?}",
                        device.expires_at
                    )),
                    Err(detail) => VerificationResult::Mismatch(detail),
                },
            )
        }
        AdminChange::DeviceExpireNow => {
            let device = read_device(
                client,
                token_manager,
                profile,
                credential,
                token,
                request,
                cancellation,
            )
            .await?;
            let observed_at = device.meta.observed_at;
            let device = admin::devices::decode_device(device.value, observed_at)
                .map_err(|_| decode_failure("device verification"))?;
            Ok(
                match crate::admin::device_mutations::verify_expire_now(&device, observed_at) {
                    Ok(()) => VerificationResult::Verified(format!(
                        "server expiry timestamp: {:?}",
                        device.expires_at
                    )),
                    Err(detail) => VerificationResult::Mismatch(detail),
                },
            )
        }
        AdminChange::DeviceRoutes { routes } => {
            let routes_response = read_routes(
                client,
                token_manager,
                profile,
                credential,
                token,
                request,
                cancellation,
            )
            .await?;
            let observation = admin::routes::decode_routes(
                request.target_id.clone(),
                routes_response.value,
                routes_response.meta.observed_at,
            )
            .map_err(|_| decode_failure("route verification"))?;
            Ok(
                match crate::admin::route_mutations::verify_enabled_routes(&observation, routes) {
                    Ok(()) => VerificationResult::Verified(
                        "complete enabled route set matches".to_owned(),
                    ),
                    Err(detail) => VerificationResult::Mismatch(detail),
                },
            )
        }
        AdminChange::DnsNameservers { values } => {
            let response = read_dns_nameservers(
                client,
                token_manager,
                profile,
                credential,
                token,
                tailnet,
                cancellation,
            )
            .await?;
            let value = admin::dns::decode_nameservers(response.value, response.meta.observed_at)
                .map_err(|_| decode_failure("DNS nameserver verification"))?;
            Ok(
                match crate::admin::dns_mutations::verify_nameservers(&value, values) {
                    Ok(()) => VerificationResult::Verified(
                        "complete ordered nameserver list matches".to_owned(),
                    ),
                    Err(detail) => VerificationResult::Mismatch(detail),
                },
            )
        }
        AdminChange::DnsPreferences { magic_dns } => {
            let response = read_dns_preferences(
                client,
                token_manager,
                profile,
                credential,
                token,
                tailnet,
                cancellation,
            )
            .await?;
            let value = admin::dns::decode_preferences(response.value, response.meta.observed_at);
            Ok(
                match crate::admin::dns_mutations::verify_preferences(&value, *magic_dns) {
                    Ok(()) => VerificationResult::Verified(format!("MagicDNS: {magic_dns}")),
                    Err(detail) => VerificationResult::Mismatch(detail),
                },
            )
        }
        AdminChange::DnsSearchPaths { values } => {
            let response = read_search_paths(
                client,
                token_manager,
                profile,
                credential,
                token,
                tailnet,
                cancellation,
            )
            .await?;
            let value = admin::dns::decode_search_paths(response.value, response.meta.observed_at)
                .map_err(|_| decode_failure("DNS search-path verification"))?;
            Ok(
                match crate::admin::dns_mutations::verify_search_paths(&value, values) {
                    Ok(()) => VerificationResult::Verified(
                        "complete ordered search-path list matches".to_owned(),
                    ),
                    Err(detail) => VerificationResult::Mismatch(detail),
                },
            )
        }
        AdminChange::DnsSplitMapping {
            domain, resolvers, ..
        } => {
            let response = read_split_dns(
                client,
                token_manager,
                profile,
                credential,
                token,
                tailnet,
                cancellation,
            )
            .await?;
            let value = admin::dns::decode_split_dns(response.value, response.meta.observed_at)
                .map_err(|_| decode_failure("split-DNS verification"))?;
            Ok(
                match crate::admin::dns_mutations::verify_split_mapping(
                    &value,
                    domain,
                    resolvers.as_deref(),
                ) {
                    Ok(()) => VerificationResult::Verified(format!(
                        "split-DNS mapping verified for {domain}"
                    )),
                    Err(detail) => VerificationResult::Mismatch(detail),
                },
            )
        }
        AdminChange::UserDelete => match read_user(
            client,
            token_manager,
            profile,
            credential,
            token,
            request,
            cancellation,
        )
        .await
        {
            Err(AdminError::NotFound { .. }) => Ok(VerificationResult::Verified(
                "GET user returned not found; deletion is authoritative".to_owned(),
            )),
            Ok(_) => Ok(VerificationResult::Mismatch(
                "user still exists after delete".to_owned(),
            )),
            Err(error) => Err(error),
        },
        AdminChange::UserApproval => {
            verify_user_status(context, request, &["approved", "active"]).await
        }
        AdminChange::UserSuspend => verify_user_status(context, request, &["suspended"]).await,
        AdminChange::UserRestore => {
            verify_user_status(context, request, &["active", "approved"]).await
        }
        AdminChange::UserRole { role } => {
            let user = read_user(
                client,
                token_manager,
                profile,
                credential,
                token,
                request,
                cancellation,
            )
            .await?;
            let user = admin::users::decode_user(user.value)
                .map_err(|_| decode_failure("user verification"))?;
            Ok(
                match crate::admin::user_mutations::verify_role(&user, role) {
                    Ok(()) => VerificationResult::Verified(format!("role: {role}")),
                    Err(detail) => VerificationResult::Mismatch(detail),
                },
            )
        }
    }
}

async fn read_device(
    client: &AdminClient,
    token_manager: &TokenManager,
    profile: &str,
    credential: &str,
    token: &crate::admin::auth::AccessToken,
    request: &AdminMutationRequest,
    cancellation: Cancellation,
) -> Result<crate::admin::client::ApiResponse<crate::admin::dto::DeviceDto>, AdminError> {
    let device_id = request.target_id.clone();
    admin_read_with_replay(
        token_manager,
        profile,
        credential,
        token,
        cancellation,
        |token| {
            let client = client.clone();
            let device_id = device_id.clone();
            Box::pin(async move { client.get_device(token, &device_id).await })
        },
    )
    .await
}

async fn read_routes(
    client: &AdminClient,
    token_manager: &TokenManager,
    profile: &str,
    credential: &str,
    token: &crate::admin::auth::AccessToken,
    request: &AdminMutationRequest,
    cancellation: Cancellation,
) -> Result<crate::admin::client::ApiResponse<crate::admin::dto::DeviceRoutesDto>, AdminError> {
    let device_id = request.target_id.clone();
    admin_read_with_replay(
        token_manager,
        profile,
        credential,
        token,
        cancellation,
        |token| {
            let client = client.clone();
            let device_id = device_id.clone();
            Box::pin(async move { client.get_routes(token, &device_id).await })
        },
    )
    .await
}

async fn read_user(
    client: &AdminClient,
    token_manager: &TokenManager,
    profile: &str,
    credential: &str,
    token: &crate::admin::auth::AccessToken,
    request: &AdminMutationRequest,
    cancellation: Cancellation,
) -> Result<crate::admin::client::ApiResponse<crate::admin::dto::UserDto>, AdminError> {
    let user_id = request.target_id.clone();
    admin_read_with_replay(
        token_manager,
        profile,
        credential,
        token,
        cancellation,
        |token| {
            let client = client.clone();
            let user_id = user_id.clone();
            Box::pin(async move { client.get_user(token, &user_id).await })
        },
    )
    .await
}

async fn verify_user_status(
    context: &AdminReadContext<'_>,
    request: &AdminMutationRequest,
    expected: &[&str],
) -> Result<VerificationResult, AdminError> {
    let user = read_user(
        context.client,
        context.token_manager,
        context.profile,
        context.credential,
        context.token,
        request,
        context.cancellation.clone(),
    )
    .await?;
    let user =
        admin::users::decode_user(user.value).map_err(|_| decode_failure("user verification"))?;
    Ok(
        match crate::admin::user_mutations::verify_status(&user, expected) {
            Ok(()) => VerificationResult::Verified(format!(
                "status: {}",
                user.status.as_deref().unwrap_or("unknown")
            )),
            Err(detail) => VerificationResult::Mismatch(detail),
        },
    )
}

async fn read_dns_nameservers(
    client: &AdminClient,
    token_manager: &TokenManager,
    profile: &str,
    credential: &str,
    token: &crate::admin::auth::AccessToken,
    tailnet: &str,
    cancellation: Cancellation,
) -> Result<crate::admin::client::ApiResponse<crate::admin::dto::NameserversResponse>, AdminError> {
    let tailnet = tailnet.to_owned();
    admin_read_with_replay(
        token_manager,
        profile,
        credential,
        token,
        cancellation,
        |token| {
            let client = client.clone();
            let tailnet = tailnet.clone();
            Box::pin(async move { client.get_nameservers(token, &tailnet).await })
        },
    )
    .await
}

async fn read_dns_preferences(
    client: &AdminClient,
    token_manager: &TokenManager,
    profile: &str,
    credential: &str,
    token: &crate::admin::auth::AccessToken,
    tailnet: &str,
    cancellation: Cancellation,
) -> Result<crate::admin::client::ApiResponse<crate::admin::dto::DnsPreferencesDto>, AdminError> {
    let tailnet = tailnet.to_owned();
    admin_read_with_replay(
        token_manager,
        profile,
        credential,
        token,
        cancellation,
        |token| {
            let client = client.clone();
            let tailnet = tailnet.clone();
            Box::pin(async move { client.get_dns_preferences(token, &tailnet).await })
        },
    )
    .await
}

async fn read_search_paths(
    client: &AdminClient,
    token_manager: &TokenManager,
    profile: &str,
    credential: &str,
    token: &crate::admin::auth::AccessToken,
    tailnet: &str,
    cancellation: Cancellation,
) -> Result<crate::admin::client::ApiResponse<crate::admin::dto::SearchPathsDto>, AdminError> {
    let tailnet = tailnet.to_owned();
    admin_read_with_replay(
        token_manager,
        profile,
        credential,
        token,
        cancellation,
        |token| {
            let client = client.clone();
            let tailnet = tailnet.clone();
            Box::pin(async move { client.get_search_paths(token, &tailnet).await })
        },
    )
    .await
}

async fn read_split_dns(
    client: &AdminClient,
    token_manager: &TokenManager,
    profile: &str,
    credential: &str,
    token: &crate::admin::auth::AccessToken,
    tailnet: &str,
    cancellation: Cancellation,
) -> Result<crate::admin::client::ApiResponse<serde_json::Map<String, serde_json::Value>>, AdminError>
{
    let tailnet = tailnet.to_owned();
    admin_read_with_replay(
        token_manager,
        profile,
        credential,
        token,
        cancellation,
        |token| {
            let client = client.clone();
            let tailnet = tailnet.clone();
            Box::pin(async move { client.get_split_dns(token, &tailnet).await })
        },
    )
    .await
}

fn uncertain_admin_error(error: &AdminError) -> bool {
    matches!(
        error,
        AdminError::Transport { .. }
            | AdminError::TimedOut { .. }
            | AdminError::ServerFailure { .. }
            | AdminError::RateLimited { .. }
            | AdminError::Cancelled { .. }
    )
}

struct AdminAuditJob {
    queue: EventQueue,
    token_manager: Arc<TokenManager>,
    client: AdminClient,
    profile: String,
    credential: String,
    task_id: TaskId,
    request: AdminMutationRequest,
    tailnet: String,
    dispatched_at: crate::domain::Timestamp,
    cancellation: Cancellation,
}

async fn run_admin_audit_correlation(job: AdminAuditJob) {
    let AdminAuditJob {
        queue,
        token_manager,
        client,
        profile,
        credential,
        task_id,
        request,
        tailnet,
        dispatched_at,
        cancellation,
    } = job;
    let token = tokio::select! {
        result = token_manager.access_token(&profile, &credential) => result,
        _ = wait_for_cancellation(cancellation.clone()) => Err(crate::admin::auth::AuthError::Cancelled),
    };
    let correlation = match token {
        Ok(token) => {
            let context = AdminReadContext {
                client: &client,
                token_manager: &token_manager,
                profile: &profile,
                credential: &credential,
                token: &token,
                tailnet: &tailnet,
                cancellation,
            };
            correlate_admin_audit(&context, &request, dispatched_at).await
        }
        Err(_) => crate::domain::admin_mutation::AuditCorrelation::none(),
    };
    queue
        .send(Event::Admin(Box::new(
            AdminEvent::AuditCorrelationFinished {
                task_id,
                mutation_id: request.mutation_id,
                correlation,
            },
        )))
        .await;
}

async fn correlate_admin_audit(
    context: &AdminReadContext<'_>,
    request: &AdminMutationRequest,
    dispatched_at: crate::domain::Timestamp,
) -> crate::domain::admin_mutation::AuditCorrelation {
    let deadline = dispatched_at.saturating_add(120);
    let start = match format_utc(dispatched_at.saturating_sub(5)) {
        Some(value) => value,
        None => return crate::domain::admin_mutation::AuditCorrelation::none(),
    };
    loop {
        if context.cancellation.is_cancelled() {
            return crate::domain::admin_mutation::AuditCorrelation {
                candidate_event_ids: Vec::new(),
                polling_stopped: true,
            };
        }
        let now = crate::local::now();
        let end = match format_utc(now) {
            Some(value) => value,
            None => return crate::domain::admin_mutation::AuditCorrelation::none(),
        };
        let response = read_audit(context, &start, &end).await;
        let Ok(response) = response else {
            return crate::domain::admin_mutation::AuditCorrelation::none();
        };
        let snapshot = match admin::audit::decode_audit_with_token(
            response.value.logs,
            response.meta.observed_at,
            None,
        ) {
            Ok(snapshot) => snapshot,
            Err(_) => return crate::domain::admin_mutation::AuditCorrelation::none(),
        };
        let correlation = crate::admin::mutation::correlate_audit(
            &snapshot.events,
            &request.target_id,
            request.change.audit_action_class(),
            None,
            dispatched_at,
            now,
        );
        if !correlation.candidate_event_ids.is_empty() || now >= deadline {
            return correlation;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn read_audit(
    context: &AdminReadContext<'_>,
    start: &str,
    end: &str,
) -> Result<crate::admin::client::ApiResponse<crate::admin::dto::AuditResponse>, AdminError> {
    let tailnet = context.tailnet.to_owned();
    let start = start.to_owned();
    let end = end.to_owned();
    admin_read_with_replay(
        context.token_manager,
        context.profile,
        context.credential,
        context.token,
        context.cancellation.clone(),
        |token| {
            let client = context.client.clone();
            let tailnet = tailnet.clone();
            let start = start.clone();
            let end = end.clone();
            Box::pin(async move { client.get_audit(token, &tailnet, &start, &end).await })
        },
    )
    .await
}

async fn send_admin_mutation_finished(
    queue: EventQueue,
    task_id: TaskId,
    request: AdminMutationRequest,
    outcome: AdminMutationOutcome,
    verified: bool,
) {
    let summary = match outcome.state {
        crate::domain::admin_mutation::AdminMutationState::Succeeded => "admin mutation verified",
        crate::domain::admin_mutation::AdminMutationState::SucceededUnverified => {
            "admin mutation succeeded but is unverified"
        }
        crate::domain::admin_mutation::AdminMutationState::OutcomeUnknown => {
            "admin mutation outcome is unknown"
        }
        _ => "admin mutation failed",
    };
    queue
        .send(Event::Task(Box::new(if verified {
            TaskEvent::Succeeded {
                task_id,
                finished_at: crate::local::now(),
                summary: summary.to_owned(),
                detail: outcome.verification.clone(),
            }
        } else {
            TaskEvent::Failed {
                task_id,
                finished_at: crate::local::now(),
                summary: summary.to_owned(),
                detail: format!("{}; {}", outcome.detail, outcome.verification),
            }
        })))
        .await;
    let refresh_resources = refresh_resources_for_change(&request.change, &request.target_id);
    let refresh_local_dns = matches!(
        request.change,
        crate::domain::admin_mutation::AdminChange::DnsNameservers { .. }
            | crate::domain::admin_mutation::AdminChange::DnsPreferences { .. }
            | crate::domain::admin_mutation::AdminChange::DnsSearchPaths { .. }
            | crate::domain::admin_mutation::AdminChange::DnsSplitMapping { .. }
    );
    queue
        .send(Event::Admin(Box::new(AdminEvent::MutationFinished {
            task_id,
            request: Box::new(request),
            outcome: Box::new(outcome),
            refresh_resources,
            refresh_local_dns,
        })))
        .await;
}

fn refresh_resources_for_change(
    change: &crate::domain::admin_mutation::AdminChange,
    target_id: &str,
) -> Vec<admin::AdminRefreshResource> {
    use crate::domain::admin_mutation::AdminChange;
    match change {
        AdminChange::DeviceRoutes { .. } => vec![
            admin::AdminRefreshResource::Devices,
            admin::AdminRefreshResource::DeviceRoutes(target_id.to_owned()),
        ],
        AdminChange::DeviceRename { .. }
        | AdminChange::DeviceTags { .. }
        | AdminChange::DeviceApproval { .. }
        | AdminChange::DeviceKeyExpiry { .. }
        | AdminChange::DeviceExpireNow
        | AdminChange::DeviceDelete => vec![admin::AdminRefreshResource::Devices],
        AdminChange::UserApproval
        | AdminChange::UserRole { .. }
        | AdminChange::UserSuspend
        | AdminChange::UserRestore
        | AdminChange::UserDelete => vec![
            admin::AdminRefreshResource::Users,
            admin::AdminRefreshResource::Devices,
            admin::AdminRefreshResource::Credentials,
        ],
        AdminChange::DnsNameservers { .. } => vec![
            admin::AdminRefreshResource::Nameservers,
            admin::AdminRefreshResource::DnsPreferences,
        ],
        AdminChange::DnsPreferences { .. } => vec![admin::AdminRefreshResource::DnsPreferences],
        AdminChange::DnsSearchPaths { .. } => vec![admin::AdminRefreshResource::SearchPaths],
        AdminChange::DnsSplitMapping { .. } => vec![admin::AdminRefreshResource::SplitDns],
    }
}

async fn wait_for_cancellation(cancellation: Cancellation) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn format_utc(timestamp: crate::domain::Timestamp) -> Option<String> {
    let seconds = i64::try_from(timestamp).ok()?;
    let date = time::OffsetDateTime::from_unix_timestamp(seconds).ok()?;
    date.format(&time::format_description::well_known::Rfc3339)
        .ok()
}

fn decode_failure(resource: &str) -> AdminError {
    AdminError::DecodeFailed {
        operation: resource.to_owned(),
        detail: "the response contained invalid documented fields".to_owned(),
    }
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
        executable.socket_path.as_deref(),
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
        executable.socket_path.as_deref(),
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
        executable.socket_path.as_deref(),
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
        executable.socket_path.as_deref(),
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
                        let run = run_service_command(
                            command,
                            &cancellation,
                            executable.socket_path.as_deref(),
                        )
                        .await;
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
                    executable.socket_path.as_deref(),
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
                        let run = run_service_command(
                            command,
                            &cancellation,
                            executable.socket_path.as_deref(),
                        )
                        .await;
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
                    executable.socket_path.as_deref(),
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
                            let (run, filenames) = run_transfer_command(
                                command,
                                &cancellation,
                                &queue,
                                task_id,
                                executable.socket_path.as_deref(),
                            )
                            .await;
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
                        let (run, filenames) = run_transfer_command(
                            command,
                            &cancellation,
                            &queue,
                            task_id,
                            executable.socket_path.as_deref(),
                        )
                        .await;
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
                        let run = run_service_command(
                            command,
                            &cancellation,
                            executable.socket_path.as_deref(),
                        )
                        .await;
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
                        let run = run_service_command(
                            command,
                            &cancellation,
                            executable.socket_path.as_deref(),
                        )
                        .await;
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
                        let run = run_service_command(
                            command,
                            &cancellation,
                            executable.socket_path.as_deref(),
                        )
                        .await;
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
                    Ok(command) => match run_service_command(
                        command,
                        &cancellation,
                        executable.socket_path.as_deref(),
                    )
                    .await
                    {
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
                    executable.socket_path.as_deref(),
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
                    Ok(command) => match run_service_command(
                        command,
                        &cancellation,
                        executable.socket_path.as_deref(),
                    )
                    .await
                    {
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
    socket_path: Option<&Path>,
) -> (
    Result<crate::local::process::LocalCommandResult, ServiceRunError>,
    Vec<String>,
) {
    let (sender, mut receiver) = mpsc::channel(64);
    let future = process::run_lines(
        local_cli_command(command, socket_path),
        cancellation,
        sender,
    );
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
    socket_path: Option<&Path>,
) -> Result<crate::local::process::LocalCommandResult, ServiceRunError> {
    let command = local_cli_command(command, socket_path);
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

fn local_cli_command(
    command: crate::local::process::LocalCommand,
    socket_path: Option<&Path>,
) -> crate::local::process::LocalCommand {
    match socket_path {
        Some(path) => command.with_socket_path(path.as_os_str().to_os_string()),
        None => command,
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
    let client = LocalCliClient::new(executable.clone(), timeout);
    let socket_path = match executable.socket_path.clone() {
        Some(path) => path,
        None => crate::local::daemon::documented_socket_path(),
    };
    let daemon = crate::local::daemon::LocalDaemonClient::new(socket_path, timeout);
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
            match client::set_command(&executable.path, timeout, request) {
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
            client::exit_node_command(&executable.path, timeout, request)
        }
        LocalMutation::Advertisements(request) => {
            match client::advertisement_command(&executable.path, timeout, request) {
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
            match daemon.status(&Cancellation::new()).await {
                Ok(value) => snapshot = Some(Box::new(value.snapshot)),
                Err(error) => read_error = Some(error.failure().detail),
            }
        }
        LocalMutation::Preferences(_)
        | LocalMutation::ExitNode(_)
        | LocalMutation::Advertisements(_) => {
            match daemon.preferences(&Cancellation::new()).await {
                Ok(value) => preferences = Some(Box::new(value.preferences)),
                Err(error) => read_error = Some(error.failure().detail),
            }
        }
        LocalMutation::AccountSwitch { .. } | LocalMutation::AccountRemove { .. } => {
            match accounts::list(
                &executable.path,
                timeout,
                &Cancellation::new(),
                executable.socket_path.as_deref(),
            )
            .await
            {
                Ok(value) => account_values = Some(value),
                Err(error) => read_error = Some(safe_operator_detail(&error.to_string())),
            }
            if read_error.is_none() {
                match daemon.status(&Cancellation::new()).await {
                    Ok(value) => snapshot = Some(Box::new(value.snapshot)),
                    Err(error) => read_error = Some(error.failure().detail),
                }
            }
            if read_error.is_none() {
                match daemon.preferences(&Cancellation::new()).await {
                    Ok(value) => preferences = Some(Box::new(value.preferences)),
                    Err(error) => read_error = Some(error.failure().detail),
                }
            }
        }
        LocalMutation::SyspolicyReload => {
            match policy::list(
                &executable.path,
                timeout,
                &Cancellation::new(),
                executable.socket_path.as_deref(),
            )
            .await
            {
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

fn observer_event(event: crate::local::ipn::ObserverEvent) -> LocalEvent {
    match event {
        crate::local::ipn::ObserverEvent::WatcherConnected => LocalEvent::WatcherConnected,
        crate::local::ipn::ObserverEvent::WatcherDisconnected { failure } => {
            LocalEvent::WatcherDisconnected { failure }
        }
        crate::local::ipn::ObserverEvent::StatusStarted {
            generation,
            attempted_at,
        } => LocalEvent::StatusStarted {
            generation,
            attempted_at,
        },
        crate::local::ipn::ObserverEvent::StatusSucceeded {
            generation,
            snapshot,
        } => LocalEvent::StatusSucceeded {
            generation,
            snapshot,
        },
        crate::local::ipn::ObserverEvent::StatusFailed {
            generation,
            failure,
        } => LocalEvent::StatusFailed {
            generation,
            failure,
        },
        crate::local::ipn::ObserverEvent::PreferencesStarted {
            generation,
            attempted_at,
        } => LocalEvent::PreferencesStarted {
            generation,
            attempted_at,
        },
        crate::local::ipn::ObserverEvent::PreferencesSucceeded {
            generation,
            preferences,
        } => LocalEvent::PreferencesSucceeded {
            generation,
            preferences,
        },
        crate::local::ipn::ObserverEvent::PreferencesFailed {
            generation,
            failure,
        } => LocalEvent::PreferencesFailed {
            generation,
            failure,
        },
    }
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
                executable.socket_path.as_deref(),
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
                executable.socket_path.as_deref(),
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
                executable.socket_path.as_deref(),
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
                executable.socket_path.as_deref(),
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
                executable.socket_path.as_deref(),
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
    socket_path: Option<&Path>,
) {
    let (sender, receiver) = mpsc::channel::<ProcessLine>(128);
    let process = process::run_lines(
        local_cli_command(command, socket_path),
        &cancellation,
        sender,
    );
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
    socket_path: Option<&Path>,
) {
    let result = match process::run(local_cli_command(command, socket_path), &cancellation).await {
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
