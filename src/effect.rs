use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::admin::AdminRefreshResource;
use crate::admin::auth::SecretValue;
use crate::admin::mutation::AdminMutationRequest;
use crate::domain::mutation::LocalMutation;
use crate::domain::service::ServiceActionRequest;
use crate::domain::source::LocalExecutable;
use crate::local::client::ExecutableResolution;
use crate::local::diagnostics::DiagnosticRequest;
use crate::local::handoff::HandoffCommand;
use crate::mock::{MockLoadScenario, MockTaskBehavior};
use crate::task::TaskId;

#[derive(Debug, Clone)]
pub enum Effect {
    StartMockLoad {
        resource: Resource,
        generation: u64,
        scenario: MockLoadScenario,
    },
    StartMockTask {
        task_id: TaskId,
        behavior: MockTaskBehavior,
    },
    StartAdminRefresh {
        profile: String,
        tailnet: String,
        credential: String,
        environment_token: Option<Arc<SecretValue>>,
        generation: u64,
        timeout: Duration,
        audit_window_days: u64,
    },
    StartAdminResourceRefresh {
        profile: String,
        tailnet: String,
        credential: String,
        environment_token: Option<Arc<SecretValue>>,
        generation: u64,
        timeout: Duration,
        audit_window_days: u64,
        resources: Vec<AdminRefreshResource>,
    },
    StartAdminDeviceEnrichment {
        profile: String,
        credential: String,
        environment_token: Option<Arc<SecretValue>>,
        generation: u64,
        device_id: String,
        timeout: Duration,
    },
    StartAdminPreflight {
        request: AdminMutationRequest,
        tailnet: String,
        credential: String,
        environment_token: Option<Arc<SecretValue>>,
        timeout: Duration,
    },
    StartAdminMutation {
        task_id: TaskId,
        request: AdminMutationRequest,
        tailnet: String,
        credential: String,
        environment_token: Option<Arc<SecretValue>>,
        timeout: Duration,
    },
    CancelAdminRefresh,
    DropAdminToken {
        profile: String,
    },
    StartLocalDiscovery {
        generation: u64,
        resolution: ExecutableResolution,
        timeout: Duration,
    },
    StartLocalStatus {
        generation: u64,
        executable: LocalExecutable,
        timeout: Duration,
    },
    StartLocalPreferences {
        executable: LocalExecutable,
        timeout: Duration,
    },
    StartLocalAccounts {
        executable: LocalExecutable,
        timeout: Duration,
    },
    StartLocalPolicy {
        executable: LocalExecutable,
        timeout: Duration,
    },
    StartLocalMutation {
        mutation_id: u64,
        task_id: TaskId,
        executable: LocalExecutable,
        timeout: Duration,
        mutation: LocalMutation,
    },
    StartTerminalHandoff {
        task_id: TaskId,
        command: HandoffCommand,
    },
    ResumeTerminal,
    StartLocalDiagnostic {
        task_id: TaskId,
        executable: LocalExecutable,
        timeout: Duration,
        request: DiagnosticRequest,
    },
    StartLocalServicesRefresh {
        generation: u64,
        executable: LocalExecutable,
        timeout: Duration,
        alpha_enabled: bool,
    },
    StartServiceTask {
        task_id: TaskId,
        executable: LocalExecutable,
        timeout: Duration,
        request: ServiceActionRequest,
    },
    CancelLocalDiscovery,
    CancelLocalStatus,
    CancelTask {
        task_id: TaskId,
    },
    WriteConfigCandidate {
        path: PathBuf,
        bytes: Vec<u8>,
    },
    RequestShutdown,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Resource {
    Devices,
}
