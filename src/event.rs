use std::time::Instant;

use crossterm::event::{Event as CrosstermEvent, KeyEvent, KeyEventKind};

use crate::admin::routes::AdminRouteObservation;
use crate::admin::{AdminRefreshReport, AdminResourceReport};
use crate::domain::Timestamp;
use crate::domain::account::LocalAccount;
use crate::domain::device::AdminDevice;
use crate::domain::device::Device;
use crate::domain::diagnostic::{DiagnosticResult, NetcheckObservation, PingSample};
use crate::domain::mutation::{LocalMutation, MutationResult};
use crate::domain::preference::LocalPreferences;
use crate::domain::service::{
    FunnelStatus, ServeStatus, ServiceActionRequest, ServiceFailure, ServiceTaskData,
};
use crate::domain::source::{LocalExecutable, LocalFailure, LocalSnapshot};
use crate::domain::transfer::{TaildriveShare, TaildropTarget};
use crate::local::handoff::HandoffResult;
use crate::local::policy::SystemPolicyEntry;
use crate::mock::MockScenario;
use crate::task::{Progress, TaskId};

#[derive(Debug, Clone)]
pub enum Event {
    Input(InputEvent),
    Tick(Instant),
    Task(Box<TaskEvent>),
    Source(SourceEvent),
    Local(Box<LocalEvent>),
    Services(Box<ServicesEvent>),
    Admin(Box<AdminEvent>),
    ShutdownRequested(ShutdownReason),
}

#[derive(Debug, Clone)]
pub enum AdminEvent {
    RefreshStarted {
        profile: String,
        generation: u64,
    },
    RefreshFinished(Box<AdminRefreshReport>),
    ResourceRefreshFinished(Box<AdminResourceReport>),
    AuthenticationFailed {
        profile: String,
        generation: u64,
        detail: String,
    },
    DeviceEnrichmentFinished {
        profile: String,
        generation: u64,
        device: Box<AdminDevice>,
        routes: Option<AdminRouteObservation>,
        routes_error: Option<crate::admin::client::AdminError>,
        posture_present: Option<bool>,
        posture_error: Option<crate::admin::client::AdminError>,
    },
    DeviceEnrichmentFailed {
        profile: String,
        generation: u64,
        device_id: String,
        detail: String,
    },
    Failed {
        profile: String,
        generation: u64,
        detail: String,
    },
}

#[derive(Debug, Clone)]
pub enum InputEvent {
    Key(KeyEvent),
    Resize { width: u16, height: u16 },
    Paste(String),
    FocusGained,
    FocusLost,
}

#[derive(Debug, Clone)]
pub enum TaskEvent {
    Started {
        task_id: TaskId,
    },
    Progress {
        task_id: TaskId,
        progress: Progress,
        detail: String,
    },
    Succeeded {
        task_id: TaskId,
        finished_at: Timestamp,
        summary: String,
        detail: String,
    },
    Failed {
        task_id: TaskId,
        finished_at: Timestamp,
        summary: String,
        detail: String,
    },
    Cancelled {
        task_id: TaskId,
        finished_at: Timestamp,
        detail: String,
    },
    DiagnosticProgress {
        task_id: TaskId,
        progress: Progress,
        detail: String,
        sample: Option<PingSample>,
        netcheck: Option<NetcheckObservation>,
    },
    DiagnosticResult {
        task_id: TaskId,
        result: DiagnosticResult,
    },
}

#[derive(Debug, Clone)]
pub enum SourceEvent {
    LoadStarted {
        generation: u64,
        scenario: MockScenario,
    },
    LoadSucceeded {
        generation: u64,
        devices: Vec<Device>,
        observed_at: Timestamp,
    },
    LoadFailed {
        generation: u64,
        detail: String,
    },
    InputFailed(String),
}

#[derive(Debug, Clone)]
pub enum LocalEvent {
    DiscoveryStarted {
        generation: u64,
    },
    DiscoverySucceeded {
        generation: u64,
        executable: LocalExecutable,
    },
    DiscoveryFailed {
        generation: u64,
        failure: LocalFailure,
    },
    StatusStarted {
        generation: u64,
        attempted_at: Timestamp,
    },
    StatusSucceeded {
        generation: u64,
        snapshot: Box<LocalSnapshot>,
    },
    StatusFailed {
        generation: u64,
        failure: LocalFailure,
    },
    PreferencesSucceeded {
        preferences: Box<LocalPreferences>,
    },
    PreferencesFailed {
        failure: LocalFailure,
    },
    AccountsSucceeded {
        accounts: Vec<LocalAccount>,
    },
    AccountsFailed {
        failure: LocalFailure,
    },
    PolicySucceeded {
        entries: Vec<SystemPolicyEntry>,
    },
    PolicyFailed {
        failure: LocalFailure,
    },
    MutationFinished {
        mutation_id: u64,
        task_id: TaskId,
        action_id: crate::action::ActionId,
        mutation: LocalMutation,
        result: MutationResult,
        snapshot: Option<Box<LocalSnapshot>>,
        preferences: Option<Box<LocalPreferences>>,
        accounts: Option<Vec<LocalAccount>>,
        policy: Option<Vec<SystemPolicyEntry>>,
    },
    HandoffFinished {
        task_id: TaskId,
        result: Result<HandoffResult, String>,
    },
    TerminalResumeFailed {
        detail: String,
    },
    DiagnosticProgress {
        task_id: TaskId,
        progress: Progress,
        detail: String,
        sample: Option<PingSample>,
        netcheck: Option<NetcheckObservation>,
    },
    DiagnosticResult {
        task_id: TaskId,
        result: DiagnosticResult,
    },
}

#[derive(Debug, Clone)]
pub enum ServicesEvent {
    RefreshFinished {
        generation: u64,
        observed_at: Timestamp,
        command_version: String,
        serve: Result<ServeStatus, ServiceFailure>,
        funnel: Result<FunnelStatus, ServiceFailure>,
        taildrop_targets: Result<Vec<TaildropTarget>, ServiceFailure>,
        taildrive: Result<Vec<TaildriveShare>, ServiceFailure>,
    },
    TaskFinished {
        task_id: TaskId,
        request: ServiceActionRequest,
        result: Result<ServiceTaskData, ServiceFailure>,
        exit_status: Option<i32>,
        stdout_truncated: bool,
        stderr_truncated: bool,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ShutdownReason {
    UserQuit,
    Signal,
    RenderFailure,
    EventSourceFailure,
}

pub fn from_terminal_event(event: CrosstermEvent) -> Option<InputEvent> {
    match event {
        CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => Some(InputEvent::Key(key)),
        CrosstermEvent::Resize(width, height) => Some(InputEvent::Resize { width, height }),
        CrosstermEvent::Paste(text) => Some(InputEvent::Paste(text)),
        CrosstermEvent::FocusGained => Some(InputEvent::FocusGained),
        CrosstermEvent::FocusLost => Some(InputEvent::FocusLost),
        _ => None,
    }
}
