use std::future::pending;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinSet;
use tokio::time::Instant;

use crate::domain::source::{LocalFailure, LocalFailureKind};
use crate::local::daemon::{
    LocalDaemonClient, LocalDaemonError, NotifyWatchMask, WatchInvalidation,
};
use crate::local::process::Cancellation;

pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(75);
pub const MAX_DEBOUNCE_WINDOW: Duration = Duration::from_millis(250);
pub const WATCHER_RESET_AFTER: Duration = Duration::from_secs(30);

const RECONNECT_DELAYS: [Duration; 5] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
];

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ObserverEvent {
    WatcherConnected {
        generation: u64,
    },
    WatcherDisconnected {
        generation: u64,
        failure: LocalFailure,
    },
    StatusStarted {
        generation: u64,
        attempted_at: u64,
    },
    StatusSucceeded {
        generation: u64,
        snapshot: Box<crate::domain::source::LocalSnapshot>,
    },
    StatusFailed {
        generation: u64,
        failure: LocalFailure,
    },
    PreferencesStarted {
        generation: u64,
        attempted_at: u64,
    },
    PreferencesSucceeded {
        generation: u64,
        preferences: Box<crate::domain::preference::LocalPreferences>,
    },
    PreferencesFailed {
        generation: u64,
        failure: LocalFailure,
    },
}

#[derive(Debug, Clone)]
pub struct ObserverConfig {
    pub reconcile_interval: Duration,
    pub generation: u64,
    pub initial_status_generation: u64,
    pub initial_preferences_generation: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ReadSerializers {
    status: Arc<Mutex<()>>,
    preferences: Arc<Mutex<()>>,
    status_generation: Arc<AtomicU64>,
    preferences_generation: Arc<AtomicU64>,
}

impl ReadSerializers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ensure_generations(&self, status: u64, preferences: u64) {
        self.status_generation.fetch_max(status, Ordering::AcqRel);
        self.preferences_generation
            .fetch_max(preferences, Ordering::AcqRel);
    }

    pub fn reserve_status_generation(&self, minimum: u64) -> u64 {
        reserve_generation(&self.status_generation, minimum)
    }

    pub fn reserve_preferences_generation(&self, minimum: u64) -> u64 {
        reserve_generation(&self.preferences_generation, minimum)
    }

    pub async fn status(
        &self,
        client: &LocalDaemonClient,
        cancellation: &Cancellation,
    ) -> Result<crate::local::daemon::LocalStatusSnapshot, LocalDaemonError> {
        let _guard = tokio::select! {
            guard = self.status.lock() => guard,
            () = cancellation_wait(cancellation) => return Err(LocalDaemonError::Cancelled),
        };
        client.status(cancellation).await
    }

    pub async fn preferences(
        &self,
        client: &LocalDaemonClient,
        cancellation: &Cancellation,
    ) -> Result<crate::local::daemon::LocalPreferenceSnapshot, LocalDaemonError> {
        let _guard = tokio::select! {
            guard = self.preferences.lock() => guard,
            () = cancellation_wait(cancellation) => return Err(LocalDaemonError::Cancelled),
        };
        client.preferences(cancellation).await
    }
}

fn reserve_generation(counter: &AtomicU64, minimum: u64) -> u64 {
    let mut current = counter.load(Ordering::Acquire);
    loop {
        let next = current.saturating_add(1).max(minimum);
        match counter.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return next,
            Err(observed) => current = observed,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ResourceKind {
    Status,
    Preferences,
}

#[derive(Debug)]
enum ReadResult {
    Status {
        generation: u64,
        result: Box<Result<crate::local::daemon::LocalStatusSnapshot, LocalDaemonError>>,
    },
    Preferences {
        generation: u64,
        result: Box<Result<crate::local::daemon::LocalPreferenceSnapshot, LocalDaemonError>>,
    },
}

#[derive(Debug, Clone, Copy)]
struct PendingInvalidations {
    status: bool,
    preferences: bool,
    first_at: Instant,
}

impl PendingInvalidations {
    fn new(invalidation: WatchInvalidation, now: Instant) -> Option<Self> {
        let (status, preferences) = match invalidation {
            WatchInvalidation::Status => (true, false),
            WatchInvalidation::Preferences => (false, true),
            WatchInvalidation::Both => (true, true),
            WatchInvalidation::None | WatchInvalidation::DaemonError { .. } => return None,
        };
        Some(Self {
            status,
            preferences,
            first_at: now,
        })
    }

    fn add(&mut self, invalidation: WatchInvalidation) {
        match invalidation {
            WatchInvalidation::Status => self.status = true,
            WatchInvalidation::Preferences => self.preferences = true,
            WatchInvalidation::Both => {
                self.status = true;
                self.preferences = true;
            }
            WatchInvalidation::None | WatchInvalidation::DaemonError { .. } => {}
        }
    }

    fn due_at(self) -> Instant {
        let debounce = match self.first_at.checked_add(DEBOUNCE_WINDOW) {
            Some(value) => value,
            None => self.first_at,
        };
        let maximum = match self.first_at.checked_add(MAX_DEBOUNCE_WINDOW) {
            Some(value) => value,
            None => self.first_at,
        };
        debounce.min(maximum)
    }
}

pub async fn run(
    client: LocalDaemonClient,
    config: ObserverConfig,
    cancellation: Cancellation,
    sender: mpsc::Sender<ObserverEvent>,
    serializers: ReadSerializers,
) {
    let mut reconnect_attempt = 0usize;
    serializers.ensure_generations(
        config.initial_status_generation,
        config.initial_preferences_generation,
    );
    loop {
        if cancellation.is_cancelled() {
            return;
        }
        let result = run_session(&client, &config, &cancellation, &sender, &serializers).await;
        let (reset, failure) = match result {
            SessionResult::Cancelled => return,
            SessionResult::Finished {
                reset_backoff,
                failure,
            } => (reset_backoff, failure),
        };
        if reset {
            reconnect_attempt = 0;
        }
        let failure = match failure {
            Some(failure) => failure,
            None => LocalFailure::new(
                LocalFailureKind::DaemonUnavailable,
                "watch-ipn-bus",
                "local daemon watcher disconnected",
                "the LocalAPI watch stream ended; reconnecting",
                true,
            ),
        };
        if !send(
            &sender,
            ObserverEvent::WatcherDisconnected {
                generation: config.generation,
                failure,
            },
        )
        .await
        {
            return;
        }
        if cancellation.is_cancelled() {
            return;
        }
        let delay = reconnect_delay(reconnect_attempt);
        reconnect_attempt = reconnect_attempt.saturating_add(1);
        if sleep_with_cancellation(delay, &cancellation).await {
            return;
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum SessionResult {
    Cancelled,
    Finished {
        reset_backoff: bool,
        failure: Option<LocalFailure>,
    },
}

async fn run_session(
    client: &LocalDaemonClient,
    config: &ObserverConfig,
    cancellation: &Cancellation,
    sender: &mpsc::Sender<ObserverEvent>,
    serializers: &ReadSerializers,
) -> SessionResult {
    let mut watch = match client.watch(NotifyWatchMask::tale(), cancellation).await {
        Ok(watch) => watch,
        Err(LocalDaemonError::Cancelled) => return SessionResult::Cancelled,
        Err(error) => {
            return SessionResult::Finished {
                reset_backoff: false,
                failure: Some(error.failure()),
            };
        }
    };
    if !send(
        sender,
        ObserverEvent::WatcherConnected {
            generation: config.generation,
        },
    )
    .await
    {
        return SessionResult::Cancelled;
    }

    let connected_at = Instant::now();
    let mut reads: JoinSet<ReadResult> = JoinSet::new();
    let mut status_active = false;
    let mut preferences_active = false;
    let mut status_dirty = false;
    let mut preferences_dirty = false;
    let mut status_succeeded = false;
    let mut preferences_succeeded = false;
    let mut full_resync = false;
    let mut pending = None;
    let mut reconcile_due = Instant::now().checked_add(config.reconcile_interval);

    start_read(
        ResourceKind::Status,
        client,
        cancellation,
        sender,
        serializers,
        &mut reads,
        &mut status_active,
        &mut preferences_active,
        &mut status_dirty,
        &mut preferences_dirty,
    )
    .await;
    start_read(
        ResourceKind::Preferences,
        client,
        cancellation,
        sender,
        serializers,
        &mut reads,
        &mut status_active,
        &mut preferences_active,
        &mut status_dirty,
        &mut preferences_dirty,
    )
    .await;

    loop {
        if cancellation.is_cancelled() {
            reads.abort_all();
            while reads.join_next().await.is_some() {}
            return SessionResult::Cancelled;
        }
        let pending_due = pending.map(PendingInvalidations::due_at);
        let result = tokio::select! {
            biased;
            () = cancellation_wait(cancellation) => {
                reads.abort_all();
                while reads.join_next().await.is_some() {}
                return SessionResult::Cancelled;
            }
            notification = watch.next(cancellation) => {
                match notification {
                    Ok(Some(notification)) => {
                        match notification.invalidation {
                            WatchInvalidation::DaemonError { detail } => {
                                let failure = LocalFailure::new(
                                    LocalFailureKind::DaemonUnavailable,
                                    "watch-ipn-bus",
                                    "local daemon reported a watcher error",
                                    detail,
                                    true,
                                );
                                reads.abort_all();
                                while reads.join_next().await.is_some() {}
                                return SessionResult::Finished {
                                    reset_backoff: full_resync
                                        || connected_at.elapsed() >= WATCHER_RESET_AFTER,
                                    failure: Some(failure),
                                };
                            }
                            invalidation @ (WatchInvalidation::Status
                            | WatchInvalidation::Preferences
                            | WatchInvalidation::Both) => {
                                match pending.as_mut() {
                                    Some(value) => value.add(invalidation),
                                    None => pending = PendingInvalidations::new(invalidation, Instant::now()),
                                }
                            }
                            WatchInvalidation::None => {}
                        }
                        false
                    }
                    Ok(None) => {
                        reads.abort_all();
                        while reads.join_next().await.is_some() {}
                        return SessionResult::Finished {
                            reset_backoff: full_resync || connected_at.elapsed() >= WATCHER_RESET_AFTER,
                            failure: None,
                        };
                    }
                    Err(LocalDaemonError::Cancelled) => {
                        reads.abort_all();
                        while reads.join_next().await.is_some() {}
                        return SessionResult::Cancelled;
                    }
                    Err(error) => {
                        let failure = error.failure();
                        reads.abort_all();
                        while reads.join_next().await.is_some() {}
                        return SessionResult::Finished {
                            reset_backoff: full_resync
                                || connected_at.elapsed() >= WATCHER_RESET_AFTER,
                            failure: Some(failure),
                        };
                    }
                }
            }
            joined = reads.join_next(), if status_active || preferences_active => {
                match joined {
                    Some(Ok(ReadResult::Status { generation, result })) => {
                        status_active = false;
                        match *result {
                            Ok(value) => {
                                status_succeeded = true;
                                if !send(sender, ObserverEvent::StatusSucceeded {
                                    generation,
                                    snapshot: Box::new(value.snapshot),
                                }).await {
                                    return SessionResult::Cancelled;
                                }
                            }
                            Err(LocalDaemonError::Cancelled) => return SessionResult::Cancelled,
                            Err(error) => {
                                if !send(sender, ObserverEvent::StatusFailed {
                                    generation,
                                    failure: error.failure(),
                                }).await {
                                    return SessionResult::Cancelled;
                                }
                            }
                        }
                        if status_dirty {
                            status_dirty = false;
                            start_read(
                                ResourceKind::Status,
                                client,
                                cancellation,
                                sender,
                                serializers,
                                &mut reads,
                                &mut status_active,
                                &mut preferences_active,
                                &mut status_dirty,
                                &mut preferences_dirty,
                            ).await;
                        }
                        false
                    }
                    Some(Ok(ReadResult::Preferences { generation, result })) => {
                        preferences_active = false;
                        match *result {
                            Ok(value) => {
                                preferences_succeeded = true;
                                if !send(sender, ObserverEvent::PreferencesSucceeded {
                                    generation,
                                    preferences: Box::new(value.preferences),
                                }).await {
                                    return SessionResult::Cancelled;
                                }
                            }
                            Err(LocalDaemonError::Cancelled) => return SessionResult::Cancelled,
                            Err(error) => {
                                if !send(sender, ObserverEvent::PreferencesFailed {
                                    generation,
                                    failure: error.failure(),
                                }).await {
                                    return SessionResult::Cancelled;
                                }
                            }
                        }
                        if preferences_dirty {
                            preferences_dirty = false;
                            start_read(
                                ResourceKind::Preferences,
                                client,
                                cancellation,
                                sender,
                                serializers,
                                &mut reads,
                                &mut status_active,
                                &mut preferences_active,
                                &mut status_dirty,
                                &mut preferences_dirty,
                            ).await;
                        }
                        false
                    }
                    Some(Err(error)) => return SessionResult::Finished {
                        reset_backoff: full_resync
                            || connected_at.elapsed() >= WATCHER_RESET_AFTER,
                        failure: Some(LocalFailure::new(
                            LocalFailureKind::DaemonUnavailable,
                            "watch-ipn-bus",
                            "local observer task failed",
                            error.to_string(),
                            true,
                        )),
                    },
                    None => return SessionResult::Finished {
                        reset_backoff: full_resync
                            || connected_at.elapsed() >= WATCHER_RESET_AFTER,
                        failure: None,
                    },
                }
            }
            () = sleep_until(pending_due), if pending_due.is_some() => {
                if let Some(invalidations) = pending.take() {
                    if invalidations.status {
                        if status_active { status_dirty = true; } else {
                            start_read(
                                ResourceKind::Status,
                                client,
                                cancellation,
                                sender,
                                serializers,
                                &mut reads,
                                &mut status_active,
                                &mut preferences_active,
                                &mut status_dirty,
                                &mut preferences_dirty,
                            ).await;
                        }
                    }
                    if invalidations.preferences {
                        if preferences_active { preferences_dirty = true; } else {
                            start_read(
                                ResourceKind::Preferences,
                                client,
                                cancellation,
                                sender,
                                serializers,
                                &mut reads,
                                &mut status_active,
                                &mut preferences_active,
                                &mut status_dirty,
                                &mut preferences_dirty,
                            ).await;
                        }
                    }
                }
                false
            }
            () = sleep_until(reconcile_due), if reconcile_due.is_some() => {
                reconcile_due = Instant::now().checked_add(config.reconcile_interval);
                if status_active { status_dirty = true; } else {
                    start_read(
                        ResourceKind::Status,
                        client,
                        cancellation,
                        sender,
                        serializers,
                        &mut reads,
                        &mut status_active,
                        &mut preferences_active,
                        &mut status_dirty,
                        &mut preferences_dirty,
                    ).await;
                }
                if preferences_active { preferences_dirty = true; } else {
                    start_read(
                        ResourceKind::Preferences,
                        client,
                        cancellation,
                        sender,
                        serializers,
                        &mut reads,
                        &mut status_active,
                        &mut preferences_active,
                        &mut status_dirty,
                        &mut preferences_dirty,
                    ).await;
                }
                false
            }
        };
        if result {
            continue;
        }
        if status_succeeded && preferences_succeeded && !full_resync {
            full_resync = true;
            reconcile_due = Instant::now().checked_add(config.reconcile_interval);
        }
        if full_resync && status_succeeded && preferences_succeeded && reconcile_due.is_none() {
            reconcile_due = Instant::now().checked_add(config.reconcile_interval);
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn start_read(
    resource: ResourceKind,
    client: &LocalDaemonClient,
    cancellation: &Cancellation,
    sender: &mpsc::Sender<ObserverEvent>,
    serializers: &ReadSerializers,
    reads: &mut JoinSet<ReadResult>,
    status_active: &mut bool,
    preferences_active: &mut bool,
    status_dirty: &mut bool,
    preferences_dirty: &mut bool,
) {
    match resource {
        ResourceKind::Status if *status_active => {
            *status_dirty = true;
        }
        ResourceKind::Preferences if *preferences_active => {
            *preferences_dirty = true;
        }
        ResourceKind::Status => {
            let value = serializers.reserve_status_generation(0);
            *status_active = true;
            if !send(
                sender,
                ObserverEvent::StatusStarted {
                    generation: value,
                    attempted_at: crate::local::now(),
                },
            )
            .await
            {
                return;
            }
            let client = client.clone();
            let cancellation = cancellation.clone();
            let serializers = serializers.clone();
            reads.spawn(async move {
                ReadResult::Status {
                    generation: value,
                    result: Box::new(serializers.status(&client, &cancellation).await),
                }
            });
        }
        ResourceKind::Preferences => {
            let value = serializers.reserve_preferences_generation(0);
            *preferences_active = true;
            if !send(
                sender,
                ObserverEvent::PreferencesStarted {
                    generation: value,
                    attempted_at: crate::local::now(),
                },
            )
            .await
            {
                return;
            }
            let client = client.clone();
            let cancellation = cancellation.clone();
            let serializers = serializers.clone();
            reads.spawn(async move {
                ReadResult::Preferences {
                    generation: value,
                    result: Box::new(serializers.preferences(&client, &cancellation).await),
                }
            });
        }
    }
}

async fn send(sender: &mpsc::Sender<ObserverEvent>, event: ObserverEvent) -> bool {
    sender.send(event).await.is_ok()
}

fn reconnect_delay(attempt: usize) -> Duration {
    let index = attempt.min(RECONNECT_DELAYS.len().saturating_sub(1));
    RECONNECT_DELAYS[index]
}

async fn sleep_with_cancellation(delay: Duration, cancellation: &Cancellation) -> bool {
    tokio::select! {
        () = tokio::time::sleep(delay) => false,
        () = cancellation_wait(cancellation) => true,
    }
}

async fn cancellation_wait(cancellation: &Cancellation) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => pending::<()>().await,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEBOUNCE_WINDOW, MAX_DEBOUNCE_WINDOW, PendingInvalidations, ReadSerializers,
        reconnect_delay,
    };
    use crate::local::daemon::WatchInvalidation;
    use std::time::Duration;
    use tokio::time::Instant;

    #[test]
    fn reconnect_delays_follow_the_pinned_sequence() {
        assert_eq!(reconnect_delay(0), Duration::from_millis(250));
        assert_eq!(reconnect_delay(1), Duration::from_millis(500));
        assert_eq!(reconnect_delay(2), Duration::from_secs(1));
        assert_eq!(reconnect_delay(3), Duration::from_secs(2));
        assert_eq!(reconnect_delay(4), Duration::from_secs(5));
        assert_eq!(reconnect_delay(50), Duration::from_secs(5));
    }

    #[test]
    fn invalidations_coalesce_by_resource_and_use_bounded_debounce() {
        let now = Instant::now();
        let mut pending = PendingInvalidations::new(WatchInvalidation::Status, now);
        assert!(pending.is_some());
        if let Some(pending) = pending.as_mut() {
            pending.add(WatchInvalidation::Preferences);
            assert!(pending.status);
            assert!(pending.preferences);
            assert_eq!(pending.due_at().duration_since(now), DEBOUNCE_WINDOW);
            assert!(MAX_DEBOUNCE_WINDOW > DEBOUNCE_WINDOW);
        }
    }

    #[test]
    fn read_generations_share_one_monotonic_sequence_across_callers() {
        let serializers = ReadSerializers::new();
        serializers.ensure_generations(10, 20);
        assert_eq!(serializers.reserve_status_generation(0), 11);
        assert_eq!(serializers.reserve_status_generation(50), 50);
        assert_eq!(serializers.reserve_status_generation(0), 51);
        assert_eq!(serializers.reserve_preferences_generation(0), 21);
        assert_eq!(serializers.reserve_preferences_generation(40), 40);
        assert_eq!(serializers.reserve_preferences_generation(0), 41);
    }
}
