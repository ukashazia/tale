use std::time::Duration;

use crate::action::ActionId;
use crate::domain::Timestamp;

pub const DETAIL_CAP: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TaskId(pub u64);

impl std::fmt::Display for TaskId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "task-{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TaskState {
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Progress {
    pub completed: u16,
    pub total: u16,
}

impl Progress {
    pub const fn fraction(self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.completed as f32 / self.total as f32
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Task {
    pub id: TaskId,
    pub action_id: ActionId,
    pub target_label: String,
    pub state: TaskState,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub progress: Option<Progress>,
    pub summary: String,
    pub detail: String,
    pub cancellable: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TaskResultKind {
    Success,
    Failure,
    Cancelled,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Notification {
    pub task_id: TaskId,
    pub message: String,
    pub kind: TaskResultKind,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone)]
pub struct TaskStore {
    tasks: Vec<Task>,
    next_id: u64,
    pub selected: Option<TaskId>,
}

impl TaskStore {
    pub const fn new() -> Self {
        Self {
            tasks: Vec::new(),
            next_id: 1,
            selected: None,
        }
    }

    pub fn create(
        &mut self,
        action_id: ActionId,
        target_label: impl Into<String>,
        started_at: Timestamp,
        cancellable: bool,
    ) -> TaskId {
        let id = TaskId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.tasks.push(Task {
            id,
            action_id,
            target_label: target_label.into(),
            state: TaskState::Queued,
            started_at,
            finished_at: None,
            progress: None,
            summary: "queued".to_owned(),
            detail: String::new(),
            cancellable,
        });
        if self.selected.is_none() {
            self.selected = Some(id);
        }
        id
    }

    pub fn get(&self, id: TaskId) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }

    pub fn get_mut(&mut self, id: TaskId) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }

    pub fn all(&self) -> &[Task] {
        &self.tasks
    }

    pub fn active(&self) -> impl Iterator<Item = &Task> {
        self.tasks.iter().filter(|task| !task.state.is_terminal())
    }

    pub fn has_active(&self) -> bool {
        self.tasks.iter().any(|task| !task.state.is_terminal())
    }

    pub fn start(&mut self, id: TaskId) -> bool {
        let Some(task) = self.get_mut(id) else {
            return false;
        };
        if task.state != TaskState::Queued {
            return false;
        }
        task.state = TaskState::Running;
        task.summary = "running".to_owned();
        true
    }

    pub fn progress(&mut self, id: TaskId, progress: Progress, detail: &str) -> bool {
        let Some(task) = self.get_mut(id) else {
            return false;
        };
        if !matches!(task.state, TaskState::Running | TaskState::Cancelling) {
            return false;
        }
        task.progress = Some(progress);
        append_detail(task, detail);
        true
    }

    pub fn request_cancel(&mut self, id: TaskId) -> bool {
        let Some(task) = self.get_mut(id) else {
            return false;
        };
        match task.state {
            TaskState::Running if task.cancellable => {
                task.state = TaskState::Cancelling;
                task.summary = "cancelling".to_owned();
                true
            }
            TaskState::Cancelling => true,
            _ => false,
        }
    }

    pub fn succeed(
        &mut self,
        id: TaskId,
        finished_at: Timestamp,
        summary: &str,
        detail: &str,
    ) -> bool {
        self.finish(id, TaskState::Succeeded, finished_at, summary, detail)
    }

    pub fn fail(
        &mut self,
        id: TaskId,
        finished_at: Timestamp,
        summary: &str,
        detail: &str,
    ) -> bool {
        self.finish(id, TaskState::Failed, finished_at, summary, detail)
    }

    pub fn cancel(&mut self, id: TaskId, finished_at: Timestamp, detail: &str) -> bool {
        self.finish(id, TaskState::Cancelled, finished_at, "cancelled", detail)
    }

    fn finish(
        &mut self,
        id: TaskId,
        state: TaskState,
        finished_at: Timestamp,
        summary: &str,
        detail: &str,
    ) -> bool {
        let Some(task) = self.get_mut(id) else {
            return false;
        };
        if task.state.is_terminal() || task.state == TaskState::Queued {
            return false;
        }
        task.state = state;
        task.finished_at = Some(finished_at);
        task.summary = summary.to_owned();
        append_detail(task, detail);
        true
    }

    pub fn evict_completed(&mut self, max_tasks: usize) {
        let completed = self
            .tasks
            .iter()
            .filter(|task| task.state.is_terminal())
            .count();
        let mut remove = completed.saturating_sub(max_tasks);
        if remove == 0 {
            return;
        }
        self.tasks.retain(|task| {
            if remove == 0 || !task.state.is_terminal() {
                true
            } else {
                remove -= 1;
                false
            }
        });
        if self.selected.is_some_and(|id| self.get(id).is_none()) {
            self.selected = self.tasks.last().map(|task| task.id);
        }
    }

    pub fn select_next(&mut self, offset: isize) {
        if self.tasks.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected
            .and_then(|id| self.tasks.iter().position(|task| task.id == id))
            .map_or(0, |position| position);
        let next = if offset.is_negative() {
            current.saturating_sub(offset.unsigned_abs())
        } else {
            current
                .saturating_add(offset as usize)
                .min(self.tasks.len().saturating_sub(1))
        };
        self.selected = self.tasks.get(next).map(|task| task.id);
    }

    pub fn notification_for(&self, id: TaskId, now: Timestamp) -> Option<Notification> {
        let task = self.get(id)?;
        let kind = match task.state {
            TaskState::Succeeded => TaskResultKind::Success,
            TaskState::Failed => TaskResultKind::Failure,
            TaskState::Cancelled => TaskResultKind::Cancelled,
            _ => return None,
        };
        Some(Notification {
            task_id: id,
            message: format!("{}: {}", task.target_label, task.summary),
            kind,
            expires_at: now.saturating_add(5),
        })
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

fn append_detail(task: &mut Task, detail: &str) {
    if detail.is_empty() {
        return;
    }
    let combined = if task.detail.is_empty() {
        detail.to_owned()
    } else {
        format!("{}\n{}", task.detail, detail)
    };
    task.detail = bounded_detail(&combined, DETAIL_CAP);
}

pub fn bounded_detail(value: &str, cap: usize) -> String {
    if value.len() <= cap {
        return value.to_owned();
    }
    let marker = "\n...[output truncated]...\n";
    if cap <= marker.len() {
        return marker[..cap].to_owned();
    }
    let available = cap - marker.len();
    let head_limit = available / 2;
    let tail_limit = available.saturating_sub(head_limit);
    let head_end = boundary_at_or_before(value, head_limit);
    let tail_start = boundary_at_or_after(value, value.len().saturating_sub(tail_limit));
    format!("{}{}{}", &value[..head_end], marker, &value[tail_start..])
}

fn boundary_at_or_before(value: &str, limit: usize) -> usize {
    if value.is_char_boundary(limit) {
        return limit;
    }
    value
        .char_indices()
        .take_while(|(index, _)| *index < limit)
        .map(|(index, _)| index)
        .last()
        .map_or(0, |index| index)
}

fn boundary_at_or_after(value: &str, limit: usize) -> usize {
    if value.is_char_boundary(limit) {
        return limit;
    }
    value
        .char_indices()
        .find(|(index, _)| *index > limit)
        .map_or(value.len(), |(index, _)| index)
}

pub const fn grace_duration() -> Duration {
    Duration::from_secs(1)
}
