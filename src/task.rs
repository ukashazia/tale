use std::collections::BTreeSet;
use std::time::Duration;

use uuid::Uuid;

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

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TaskRecordId(pub Uuid);

impl TaskRecordId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for TaskRecordId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TaskRecordId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
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
    Interrupted,
}

impl TaskState {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
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
    pub record_id: TaskRecordId,
    pub action_id: ActionId,
    pub target_label: String,
    pub state: TaskState,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub progress: Option<Progress>,
    pub summary: String,
    pub detail: String,
    pub cancellable: bool,
    pub requested_fields: Vec<String>,
    pub redacted_argv: Vec<String>,
    pub exit_status: Option<i32>,
    pub verification: Option<String>,
    pub changes: Vec<TaskChange>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TaskChange {
    pub field: String,
    pub before: Option<String>,
    pub after: Option<String>,
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
    dirty: BTreeSet<TaskRecordId>,
    session_task_ids: BTreeSet<TaskId>,
    pub selected: Option<TaskId>,
}

impl TaskStore {
    pub const fn new() -> Self {
        Self {
            tasks: Vec::new(),
            next_id: 1,
            dirty: BTreeSet::new(),
            session_task_ids: BTreeSet::new(),
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
        let record_id = TaskRecordId::new();
        self.tasks.push(Task {
            id,
            record_id,
            action_id,
            target_label: target_label.into(),
            state: TaskState::Queued,
            started_at,
            finished_at: None,
            progress: None,
            summary: "queued".to_owned(),
            detail: String::new(),
            cancellable,
            requested_fields: Vec::new(),
            redacted_argv: Vec::new(),
            exit_status: None,
            verification: None,
            changes: Vec::new(),
        });
        self.dirty.insert(record_id);
        self.session_task_ids.insert(id);
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

    fn mark_dirty(&mut self, id: TaskId) {
        if let Some(task) = self.get(id) {
            self.dirty.insert(task.record_id);
        }
    }

    pub fn take_dirty(&mut self) -> Vec<Task> {
        let dirty = std::mem::take(&mut self.dirty);
        self.tasks
            .iter()
            .filter(|task| dirty.contains(&task.record_id))
            .cloned()
            .collect()
    }

    pub fn merge_restored(&mut self, mut restored: Vec<Task>) {
        let known = self
            .tasks
            .iter()
            .map(|task| task.record_id)
            .collect::<BTreeSet<_>>();
        restored.retain(|task| !known.contains(&task.record_id));
        for task in &mut restored {
            task.id = TaskId(self.next_id);
            self.next_id = self.next_id.saturating_add(1);
        }
        restored.append(&mut self.tasks);
        self.tasks = restored;
        if self.selected.is_none() {
            self.selected = self.tasks.last().map(|task| task.id);
        }
    }

    pub fn session(&self) -> impl Iterator<Item = &Task> {
        self.tasks
            .iter()
            .filter(|task| self.session_task_ids.contains(&task.id))
    }

    pub fn session_filtered(&self, query: &str) -> impl Iterator<Item = &Task> {
        self.session()
            .filter(move |task| task_matches_query(task, query))
    }

    pub fn set_changes(&mut self, id: TaskId, changes: Vec<TaskChange>) -> bool {
        let Some(task) = self.get_mut(id) else {
            return false;
        };
        task.changes = changes;
        self.mark_dirty(id);
        true
    }

    pub fn selected_can_cancel(&self) -> bool {
        self.selected
            .and_then(|id| self.get(id))
            .is_some_and(|task| {
                task.cancellable
                    && matches!(
                        task.state,
                        TaskState::Queued | TaskState::Running | TaskState::Cancelling
                    )
            })
    }

    pub fn set_local_metadata(
        &mut self,
        id: TaskId,
        requested_fields: Vec<String>,
        redacted_argv: Vec<String>,
    ) -> bool {
        let Some(task) = self.get_mut(id) else {
            return false;
        };
        task.requested_fields = requested_fields;
        task.redacted_argv = redacted_argv;
        self.mark_dirty(id);
        true
    }

    pub fn set_exit_status(&mut self, id: TaskId, exit_status: Option<i32>) -> bool {
        let Some(task) = self.get_mut(id) else {
            return false;
        };
        task.exit_status = exit_status;
        self.mark_dirty(id);
        true
    }

    pub fn set_verification(&mut self, id: TaskId, verification: impl Into<String>) -> bool {
        let Some(task) = self.get_mut(id) else {
            return false;
        };
        task.verification = Some(verification.into());
        self.mark_dirty(id);
        true
    }

    pub fn all(&self) -> &[Task] {
        &self.tasks
    }

    pub fn filtered(&self, query: &str) -> impl Iterator<Item = &Task> {
        let query = query.to_ascii_lowercase();
        self.tasks
            .iter()
            .filter(move |task| task_matches_query(task, &query))
    }

    pub fn select_filtered_position(&mut self, query: &str, position: usize) {
        let ids = self.filtered_ids(query);
        self.selected = ids.get(position).copied();
    }

    pub fn select_filtered_first(&mut self, query: &str) {
        self.select_filtered_position(query, 0);
    }

    pub fn select_filtered_last(&mut self, query: &str) {
        let ids = self.filtered_ids(query);
        self.selected = ids.last().copied();
    }

    pub fn select_next_filtered(&mut self, query: &str, offset: isize) {
        let ids = self.filtered_ids(query);
        if ids.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected
            .and_then(|id| ids.iter().position(|candidate| *candidate == id))
            .unwrap_or(0);
        let next = if offset.is_negative() {
            current.saturating_sub(offset.unsigned_abs())
        } else {
            current
                .saturating_add(offset as usize)
                .min(ids.len().saturating_sub(1))
        };
        self.selected = ids.get(next).copied();
    }

    fn filtered_ids(&self, query: &str) -> Vec<TaskId> {
        let query = query.to_ascii_lowercase();
        self.tasks
            .iter()
            .filter(|task| task_matches_query(task, &query))
            .map(|task| task.id)
            .collect()
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
        self.mark_dirty(id);
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
        self.mark_dirty(id);
        true
    }

    pub fn request_cancel(&mut self, id: TaskId) -> bool {
        let Some(task) = self.get_mut(id) else {
            return false;
        };
        let changed = match task.state {
            TaskState::Queued if task.cancellable => {
                task.state = TaskState::Cancelling;
                task.summary = "cancelling".to_owned();
                true
            }
            TaskState::Running if task.cancellable => {
                task.state = TaskState::Cancelling;
                task.summary = "cancelling".to_owned();
                true
            }
            TaskState::Cancelling => false,
            _ => false,
        };
        if changed {
            self.mark_dirty(id);
        }
        changed
            || self
                .get(id)
                .is_some_and(|task| task.state == TaskState::Cancelling)
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
        self.mark_dirty(id);
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
        let remaining = self
            .tasks
            .iter()
            .map(|task| task.id)
            .collect::<BTreeSet<_>>();
        self.session_task_ids.retain(|id| remaining.contains(id));
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
            .unwrap_or(0);
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
            TaskState::Interrupted => TaskResultKind::Failure,
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

fn task_matches_query(task: &Task, query: &str) -> bool {
    query.is_empty()
        || [
            task.action_id.as_str(),
            task.target_label.as_str(),
            task.state.label(),
            task.summary.as_str(),
            task.detail.as_str(),
        ]
        .into_iter()
        .any(|value| crate::domain::filter::contains_matches(value, query))
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

/// Appends one line to a running detail, compacting only once the buffer has
/// grown past twice its cap. Re-bounding on every append copied the whole
/// buffer each time, which made a long stream quadratic in its own output; this
/// compacts once per `cap` bytes produced, so appending stays amortized
/// constant and the buffer never exceeds twice the cap.
pub fn push_bounded(detail: &mut String, value: &str, cap: usize) {
    crate::detail::push_bounded_ends(detail, value, cap);
}

pub fn bounded_detail(value: &str, cap: usize) -> String {
    crate::detail::bounded_ends(value, cap)
}

pub const fn grace_duration() -> Duration {
    Duration::from_secs(1)
}
