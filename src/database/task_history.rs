use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::action::ActionId;
use crate::domain::Timestamp;
use crate::event::{DatabaseEvent, Event};
use crate::runtime::EventQueue;
use crate::task::{Progress, Task, TaskChange, TaskId, TaskRecordId, TaskState};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Debug)]
pub enum TaskHistoryCommand {
    Upsert(Vec<Task>),
}

struct TaskHistoryRepository {
    pool: SqlitePool,
    max_tasks: usize,
}

impl TaskHistoryRepository {
    async fn connect(state_dir: &Path, max_tasks: usize) -> Result<Self, sqlx::Error> {
        Ok(Self {
            pool: open(state_dir).await?,
            max_tasks,
        })
    }

    async fn restore(&self) -> Result<Vec<Task>, sqlx::Error> {
        load(&self.pool).await
    }

    async fn save(&self, tasks: &[Task]) -> Result<(), sqlx::Error> {
        upsert(&self.pool, tasks, self.max_tasks).await
    }

    async fn close(self) {
        self.pool.close().await;
    }
}

#[derive(Debug, FromRow)]
struct TaskRow {
    record_id: String,
    action_id: String,
    target_label: String,
    state: String,
    started_at: i64,
    finished_at: Option<i64>,
    progress_completed: Option<i64>,
    progress_total: Option<i64>,
    summary: String,
    detail: String,
    cancellable: bool,
    requested_fields: String,
    redacted_argv: String,
    exit_status: Option<i32>,
    verification: Option<String>,
}

#[derive(Debug, FromRow)]
struct ChangeRow {
    task_record_id: String,
    field: String,
    before_value: Option<String>,
    after_value: Option<String>,
}

pub async fn run_task_history(
    state_dir: PathBuf,
    max_tasks: usize,
    mut commands: mpsc::UnboundedReceiver<TaskHistoryCommand>,
    queue: EventQueue,
) {
    let result = TaskHistoryRepository::connect(&state_dir, max_tasks).await;
    let repository = match result {
        Ok(repository) => repository,
        Err(error) => {
            queue
                .send(Event::Database(DatabaseEvent::TaskHistoryFailed(
                    error.to_string(),
                )))
                .await;
            return;
        }
    };

    match repository.restore().await {
        Ok(tasks) => {
            if let Err(error) = repository.save(&tasks).await {
                queue
                    .send(Event::Database(DatabaseEvent::TaskHistoryFailed(
                        error.to_string(),
                    )))
                    .await;
                return;
            }
            queue
                .send(Event::Database(DatabaseEvent::TaskHistoryLoaded(tasks)))
                .await;
        }
        Err(error) => {
            queue
                .send(Event::Database(DatabaseEvent::TaskHistoryFailed(
                    error.to_string(),
                )))
                .await;
            return;
        }
    }

    while let Some(command) = commands.recv().await {
        let result = match command {
            TaskHistoryCommand::Upsert(tasks) => repository.save(&tasks).await,
        };
        if let Err(error) = result {
            queue
                .send(Event::Database(DatabaseEvent::TaskHistoryFailed(
                    error.to_string(),
                )))
                .await;
            return;
        }
    }
    repository.close().await;
}

async fn open(state_dir: &Path) -> Result<SqlitePool, sqlx::Error> {
    tokio::fs::create_dir_all(state_dir).await?;
    let path = state_dir.join("tale.sqlite3");
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    MIGRATOR.run(&pool).await?;
    protect_database_files(state_dir, &path).await?;
    Ok(pool)
}

#[cfg(unix)]
async fn protect_database_files(state_dir: &Path, database: &Path) -> Result<(), sqlx::Error> {
    use std::os::unix::fs::PermissionsExt;

    tokio::fs::set_permissions(state_dir, std::fs::Permissions::from_mode(0o700)).await?;
    for path in [
        database.to_path_buf(),
        database.with_extension("sqlite3-wal"),
        database.with_extension("sqlite3-shm"),
    ] {
        match tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
async fn protect_database_files(_state_dir: &Path, _database: &Path) -> Result<(), sqlx::Error> {
    Ok(())
}

async fn load(pool: &SqlitePool) -> Result<Vec<Task>, sqlx::Error> {
    let rows = sqlx::query_as::<_, TaskRow>(
        "SELECT record_id, action_id, target_label, state, started_at, finished_at, \
         progress_completed, progress_total, summary, detail, cancellable, requested_fields, \
         redacted_argv, exit_status, verification FROM task_history ORDER BY started_at, record_id",
    )
    .fetch_all(pool)
    .await?;
    let changes = sqlx::query_as::<_, ChangeRow>(
        "SELECT task_record_id, field, before_value, after_value \
         FROM task_changes ORDER BY task_record_id, position",
    )
    .fetch_all(pool)
    .await?;
    let now = crate::local::now();
    rows.into_iter()
        .map(|row| task_from_row(row, &changes, now))
        .collect()
}

fn task_from_row(row: TaskRow, changes: &[ChangeRow], now: Timestamp) -> Result<Task, sqlx::Error> {
    let record_id = Uuid::from_str(&row.record_id).map_err(decode_error)?;
    let action_id = ActionId::parse(&row.action_id)
        .ok_or_else(|| decode_error(format!("unknown action {}", row.action_id)))?;
    let mut state = parse_state(&row.state)?;
    let mut finished_at = optional_timestamp(row.finished_at)?;
    let mut summary = row.summary;
    let mut cancellable = row.cancellable;
    if !state.is_terminal() {
        state = TaskState::Interrupted;
        finished_at = Some(now);
        summary = "Tale stopped before this task finished".to_owned();
        cancellable = false;
    }
    let progress = match (row.progress_completed, row.progress_total) {
        (Some(completed), Some(total)) => Some(Progress {
            completed: u16::try_from(completed).map_err(decode_error)?,
            total: u16::try_from(total).map_err(decode_error)?,
        }),
        (None, None) => None,
        _ => return Err(decode_error("incomplete task progress")),
    };
    let task_changes = changes
        .iter()
        .filter(|change| change.task_record_id == row.record_id)
        .map(|change| TaskChange {
            field: change.field.clone(),
            before: change.before_value.clone(),
            after: change.after_value.clone(),
        })
        .collect();
    Ok(Task {
        id: TaskId(0),
        record_id: TaskRecordId(record_id),
        action_id,
        target_label: row.target_label,
        state,
        started_at: timestamp(row.started_at)?,
        finished_at,
        progress,
        summary,
        detail: row.detail,
        cancellable,
        requested_fields: decode_strings(&row.requested_fields)?,
        redacted_argv: decode_strings(&row.redacted_argv)?,
        exit_status: row.exit_status,
        verification: row.verification,
        changes: task_changes,
    })
}

async fn upsert(pool: &SqlitePool, tasks: &[Task], max_tasks: usize) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    for task in tasks {
        upsert_task(&mut transaction, task).await?;
    }
    let keep = i64::try_from(max_tasks).map_err(decode_error)?;
    sqlx::query(
        "DELETE FROM task_history WHERE record_id IN (\
         SELECT record_id FROM task_history WHERE state IN \
         ('succeeded', 'failed', 'cancelled', 'interrupted') \
         ORDER BY started_at DESC, record_id DESC LIMIT -1 OFFSET ?\
         )",
    )
    .bind(keep)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await
}

async fn upsert_task(
    transaction: &mut Transaction<'_, Sqlite>,
    task: &Task,
) -> Result<(), sqlx::Error> {
    let requested_fields = encode_strings(&task.requested_fields)?;
    let redacted_argv = encode_strings(&task.redacted_argv)?;
    let (completed, total) = task.progress.map_or((None, None), |progress| {
        (
            Some(i64::from(progress.completed)),
            Some(i64::from(progress.total)),
        )
    });
    sqlx::query(
        "INSERT INTO task_history (record_id, action_id, target_label, state, started_at, \
         finished_at, progress_completed, progress_total, summary, detail, cancellable, \
         requested_fields, redacted_argv, exit_status, verification) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(record_id) DO UPDATE SET action_id=excluded.action_id, \
         target_label=excluded.target_label, state=excluded.state, started_at=excluded.started_at, \
         finished_at=excluded.finished_at, progress_completed=excluded.progress_completed, \
         progress_total=excluded.progress_total, summary=excluded.summary, detail=excluded.detail, \
         cancellable=excluded.cancellable, requested_fields=excluded.requested_fields, \
         redacted_argv=excluded.redacted_argv, exit_status=excluded.exit_status, \
         verification=excluded.verification",
    )
    .bind(task.record_id.to_string())
    .bind(task.action_id.as_str())
    .bind(&task.target_label)
    .bind(task.state.label())
    .bind(i64::try_from(task.started_at).map_err(decode_error)?)
    .bind(
        task.finished_at
            .map(i64::try_from)
            .transpose()
            .map_err(decode_error)?,
    )
    .bind(completed)
    .bind(total)
    .bind(&task.summary)
    .bind(&task.detail)
    .bind(task.cancellable)
    .bind(requested_fields)
    .bind(redacted_argv)
    .bind(task.exit_status)
    .bind(&task.verification)
    .execute(&mut **transaction)
    .await?;
    sqlx::query("DELETE FROM task_changes WHERE task_record_id = ?")
        .bind(task.record_id.to_string())
        .execute(&mut **transaction)
        .await?;
    for (position, change) in task.changes.iter().enumerate() {
        sqlx::query(
            "INSERT INTO task_changes \
             (task_record_id, position, field, before_value, after_value) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(task.record_id.to_string())
        .bind(i64::try_from(position).map_err(decode_error)?)
        .bind(&change.field)
        .bind(&change.before)
        .bind(&change.after)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

fn parse_state(value: &str) -> Result<TaskState, sqlx::Error> {
    match value {
        "queued" => Ok(TaskState::Queued),
        "running" => Ok(TaskState::Running),
        "cancelling" => Ok(TaskState::Cancelling),
        "succeeded" => Ok(TaskState::Succeeded),
        "failed" => Ok(TaskState::Failed),
        "cancelled" => Ok(TaskState::Cancelled),
        "interrupted" => Ok(TaskState::Interrupted),
        _ => Err(decode_error(format!("unknown task state {value}"))),
    }
}

fn timestamp(value: i64) -> Result<Timestamp, sqlx::Error> {
    u64::try_from(value).map_err(decode_error)
}

fn optional_timestamp(value: Option<i64>) -> Result<Option<Timestamp>, sqlx::Error> {
    value.map(timestamp).transpose()
}

fn encode_strings(values: &[String]) -> Result<String, sqlx::Error> {
    serde_json::to_string(values).map_err(decode_error)
}

fn decode_strings(value: &str) -> Result<Vec<String>, sqlx::Error> {
    serde_json::from_str(value).map_err(decode_error)
}

fn decode_error(error: impl std::fmt::Display) -> sqlx::Error {
    sqlx::Error::Decode(error.to_string().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn task(action_id: ActionId, started_at: Timestamp, state: TaskState) -> Task {
        Task {
            id: TaskId(started_at),
            record_id: TaskRecordId::new(),
            action_id,
            target_label: "machine alpha".to_owned(),
            state,
            started_at,
            finished_at: state.is_terminal().then_some(started_at.saturating_add(1)),
            progress: None,
            summary: state.label().to_owned(),
            detail: "safe task detail".to_owned(),
            cancellable: !state.is_terminal(),
            requested_fields: vec!["name".to_owned()],
            redacted_argv: vec!["tailscale".to_owned(), "set".to_owned()],
            exit_status: state.is_terminal().then_some(0),
            verification: Some("machine name is now beta".to_owned()),
            changes: vec![TaskChange {
                field: "machine name".to_owned(),
                before: Some("alpha".to_owned()),
                after: Some("beta".to_owned()),
            }],
        }
    }

    #[tokio::test]
    async fn task_and_changes_survive_reopening_the_database() {
        let directory = TempDir::new();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let first = open(directory.path()).await;
        assert!(first.is_ok());
        let Some(first) = first.ok() else {
            return;
        };
        let original = task(ActionId::AdminDeviceRename, 10, TaskState::Succeeded);
        assert!(
            upsert(&first, std::slice::from_ref(&original), 200)
                .await
                .is_ok()
        );
        first.close().await;

        let reopened = open(directory.path()).await;
        assert!(reopened.is_ok());
        let Some(reopened) = reopened.ok() else {
            return;
        };
        let restored = load(&reopened).await;
        assert!(restored.is_ok());
        let Some(restored) = restored.ok() else {
            return;
        };
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].record_id, original.record_id);
        assert_eq!(restored[0].changes, original.changes);
        assert_eq!(restored[0].action_id, ActionId::AdminDeviceRename);
    }

    #[tokio::test]
    async fn unfinished_work_restores_as_interrupted() {
        let directory = TempDir::new();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let pool = open(directory.path()).await;
        assert!(pool.is_ok());
        let Some(pool) = pool.ok() else {
            return;
        };
        let running = task(ActionId::AdminDeviceRename, 10, TaskState::Running);
        assert!(upsert(&pool, &[running], 200).await.is_ok());
        let restored = load(&pool).await;
        assert!(restored.is_ok());
        let Some(restored) = restored.ok() else {
            return;
        };
        assert_eq!(restored[0].state, TaskState::Interrupted);
        assert!(!restored[0].cancellable);
        assert_eq!(
            restored[0].summary,
            "Tale stopped before this task finished"
        );
    }

    #[tokio::test]
    async fn retention_keeps_the_newest_terminal_tasks() {
        let directory = TempDir::new();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let pool = open(directory.path()).await;
        assert!(pool.is_ok());
        let Some(pool) = pool.ok() else {
            return;
        };
        let tasks = (1..=4)
            .map(|started_at| {
                task(
                    ActionId::AdminDeviceRename,
                    started_at,
                    TaskState::Succeeded,
                )
            })
            .collect::<Vec<_>>();
        assert!(upsert(&pool, &tasks, 2).await.is_ok());
        let restored = load(&pool).await;
        assert!(restored.is_ok());
        let Some(restored) = restored.ok() else {
            return;
        };
        assert_eq!(
            restored
                .iter()
                .map(|task| task.started_at)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn database_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TempDir::new();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let pool = open(directory.path()).await;
        assert!(pool.is_ok());
        let Some(pool) = pool.ok() else {
            return;
        };
        let metadata = tokio::fs::metadata(directory.path().join("tale.sqlite3")).await;
        assert!(metadata.is_ok());
        let Some(metadata) = metadata.ok() else {
            return;
        };
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        pool.close().await;
    }

    #[tokio::test]
    async fn persisted_history_contains_only_redacted_command_metadata() {
        let directory = TempDir::new();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let pool = open(directory.path()).await;
        assert!(pool.is_ok());
        let Some(pool) = pool.ok() else {
            return;
        };
        let secret = "tskey-auth-secret-canary";
        let mut redacted = task(ActionId::AdminDeviceRename, 10, TaskState::Succeeded);
        redacted.redacted_argv = vec!["tailscale".to_owned(), "<redacted>".to_owned()];
        assert!(upsert(&pool, &[redacted], 200).await.is_ok());
        pool.close().await;
        let bytes = tokio::fs::read(directory.path().join("tale.sqlite3")).await;
        assert!(bytes.is_ok());
        let Some(bytes) = bytes.ok() else {
            return;
        };
        assert!(
            !bytes
                .windows(secret.len())
                .any(|window| window == secret.as_bytes())
        );
        assert!(
            bytes
                .windows("<redacted>".len())
                .any(|window| window == b"<redacted>")
        );
    }
}
