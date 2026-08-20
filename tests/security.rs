use tale::admin::auth::SecretValue;
use tale::domain::secret_result::{SecretBuffer, SecretMetadata, SecretResult};

#[test]
fn secret_canaries_are_redacted_and_view_once_results_close() {
    let canary = "fictional-secret-canary";
    let credential = SecretValue::new(canary);
    assert!(!format!("{credential:?}").contains(canary));

    let metadata = SecretMetadata {
        result_id: 1,
        credential_id: Some("fictional-id".to_owned()),
        credential_type: "auth key".to_owned(),
        description: Some("one-time result".to_owned()),
        created_at: 1,
        expires_at: None,
        warning: "copy once".to_owned(),
    };
    let mut result = SecretResult::new(metadata, SecretBuffer::new(canary));
    assert!(!format!("{result:?}").contains(canary));
    result.close();
    assert!(result.is_closed());
}

#[cfg(unix)]
#[tokio::test]
async fn task_history_database_files_are_created_owner_only() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use tale::database::run_task_history;
    use tale::event::{DatabaseEvent, Event};
    use tale::runtime::EventQueue;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    let directory = TempDir::new();
    assert!(directory.is_ok());
    let Ok(directory) = directory else {
        return;
    };
    let state_dir = directory.path().join("state").join("tale");
    let (sender, receiver) = mpsc::unbounded_channel();
    let queue = EventQueue::new();
    let worker = tokio::spawn(run_task_history(
        state_dir.clone(),
        200,
        receiver,
        queue.clone(),
    ));

    let loaded = tokio::time::timeout(Duration::from_secs(5), queue.recv()).await;
    assert!(matches!(
        loaded,
        Ok(Event::Database(DatabaseEvent::TaskHistoryLoaded(_)))
    ));

    // Checked while the pool is still open, so the write-ahead log and shared
    // memory sidecars exist; SQLite removes both on a clean close.
    let database = state_dir.join("tale.sqlite3");
    for path in [
        database.with_extension("sqlite3-wal"),
        database.with_extension("sqlite3-shm"),
        database,
    ] {
        let metadata = fs::metadata(&path);
        assert!(metadata.is_ok(), "{} should exist", path.display());
        if let Ok(metadata) = metadata {
            assert_eq!(
                metadata.permissions().mode() & 0o777,
                0o600,
                "{} should be owner-only",
                path.display()
            );
        }
    }

    let metadata = fs::metadata(&state_dir);
    assert!(metadata.is_ok());
    if let Ok(metadata) = metadata {
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    }

    drop(sender);
    let joined = tokio::time::timeout(Duration::from_secs(5), worker).await;
    assert!(joined.is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn task_history_tightens_a_permissive_state_directory() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use tale::database::run_task_history;
    use tale::event::{DatabaseEvent, Event};
    use tale::runtime::EventQueue;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    let directory = TempDir::new();
    assert!(directory.is_ok());
    let Ok(directory) = directory else {
        return;
    };
    let state_dir = directory.path().join("tale");
    assert!(fs::create_dir_all(&state_dir).is_ok());
    assert!(fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o755)).is_ok());

    let (sender, receiver) = mpsc::unbounded_channel();
    let queue = EventQueue::new();
    let worker = tokio::spawn(run_task_history(
        state_dir.clone(),
        200,
        receiver,
        queue.clone(),
    ));

    let loaded = tokio::time::timeout(Duration::from_secs(5), queue.recv()).await;
    assert!(matches!(
        loaded,
        Ok(Event::Database(DatabaseEvent::TaskHistoryLoaded(_)))
    ));

    let metadata = fs::metadata(&state_dir);
    assert!(metadata.is_ok());
    if let Ok(metadata) = metadata {
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    }

    drop(sender);
    let joined = tokio::time::timeout(Duration::from_secs(5), worker).await;
    assert!(joined.is_ok());
}
