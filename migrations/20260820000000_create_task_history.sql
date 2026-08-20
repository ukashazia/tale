CREATE TABLE task_history (
    record_id TEXT PRIMARY KEY NOT NULL,
    action_id TEXT NOT NULL,
    target_label TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('queued', 'running', 'cancelling', 'succeeded', 'failed', 'cancelled', 'interrupted')
    ),
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    progress_completed INTEGER,
    progress_total INTEGER,
    summary TEXT NOT NULL,
    detail TEXT NOT NULL,
    cancellable INTEGER NOT NULL CHECK (cancellable IN (0, 1)),
    requested_fields TEXT NOT NULL,
    redacted_argv TEXT NOT NULL,
    exit_status INTEGER,
    verification TEXT
);

CREATE TABLE task_changes (
    task_record_id TEXT NOT NULL REFERENCES task_history(record_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    field TEXT NOT NULL,
    before_value TEXT,
    after_value TEXT,
    PRIMARY KEY (task_record_id, position)
);

CREATE INDEX task_history_started_at ON task_history(started_at DESC);
CREATE INDEX task_history_state ON task_history(state);
