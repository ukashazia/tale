use std::time::Instant;

use crossterm::event::{Event as CrosstermEvent, KeyEvent, KeyEventKind};

use crate::domain::Timestamp;
use crate::domain::device::Device;
use crate::mock::MockScenario;
use crate::task::{Progress, TaskId};

#[derive(Debug, Clone)]
pub enum Event {
    Input(InputEvent),
    Tick(Instant),
    Task(TaskEvent),
    Source(SourceEvent),
    ShutdownRequested(ShutdownReason),
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
