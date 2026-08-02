use std::path::PathBuf;

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
