use std::path::PathBuf;
use std::time::Duration;

use crate::domain::source::LocalExecutable;
use crate::local::client::ExecutableResolution;
use crate::local::diagnostics::DiagnosticRequest;
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
    StartLocalDiagnostic {
        task_id: TaskId,
        executable: LocalExecutable,
        timeout: Duration,
        request: DiagnosticRequest,
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
