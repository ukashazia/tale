use std::path::PathBuf;
use std::time::Duration;

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
