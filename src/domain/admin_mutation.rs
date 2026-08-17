//! Control-plane mutation lifecycle.
//!
//! Admin writes require fresh remote preflight data, resource-scoped locking,
//! verification, optional audit correlation, and batch outcomes. Those states
//! are not aliases for the smaller local-command lifecycle.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use crate::action::{ActionId, Risk};
use crate::domain::Timestamp;

pub const REMOTE_WRITE_PREFLIGHT_MAX_AGE: Timestamp = 30;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum AdminResourceKind {
    Device,
    DeviceRoutes,
    User,
    TailnetDns,
}

impl AdminResourceKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::DeviceRoutes => "device routes",
            Self::User => "user",
            Self::TailnetDns => "tailnet DNS",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AdminResourceLockKey {
    pub profile: String,
    pub kind: AdminResourceKind,
    pub resource_id: String,
}

impl AdminResourceLockKey {
    pub fn new(
        profile: impl Into<String>,
        kind: AdminResourceKind,
        resource_id: impl Into<String>,
    ) -> Self {
        Self {
            profile: profile.into(),
            kind,
            resource_id: resource_id.into(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AdminChange {
    DeviceRename {
        name: String,
    },
    DeviceTags {
        tags: Vec<String>,
    },
    DeviceApproval {
        authorized: bool,
    },
    DeviceKeyExpiry {
        disabled: bool,
    },
    DeviceExpireNow,
    DeviceDelete,
    DeviceRoutes {
        routes: Vec<String>,
    },
    DnsNameservers {
        values: Vec<String>,
    },
    DnsPreferences {
        magic_dns: bool,
    },
    DnsSearchPaths {
        values: Vec<String>,
    },
    DnsSplitMapping {
        domain: String,
        resolvers: Option<Vec<String>>,
        create: bool,
    },
    UserApproval,
    UserRole {
        role: String,
    },
    UserSuspend,
    UserRestore,
    UserDelete,
}

impl AdminChange {
    pub const fn action_id(&self) -> ActionId {
        match self {
            Self::DeviceRename { .. } => ActionId::AdminDeviceRename,
            Self::DeviceTags { .. } => ActionId::AdminDeviceTagsReplace,
            Self::DeviceApproval { authorized: true } => ActionId::AdminDeviceApprove,
            Self::DeviceApproval { authorized: false } => ActionId::AdminDeviceRevokeApproval,
            Self::DeviceKeyExpiry { .. } => ActionId::AdminDeviceKeyExpiryConfigure,
            Self::DeviceExpireNow => ActionId::AdminDeviceKeyExpireNow,
            Self::DeviceDelete => ActionId::AdminDeviceDelete,
            Self::DeviceRoutes { .. } => ActionId::AdminRoutesReplaceApprovals,
            Self::DnsNameservers { .. } => ActionId::AdminDnsNameserversReplace,
            Self::DnsPreferences { .. } => ActionId::AdminDnsPreferencesEdit,
            Self::DnsSearchPaths { .. } => ActionId::AdminDnsSearchPathsReplace,
            Self::DnsSplitMapping { create: true, .. } => ActionId::AdminDnsSplitCreate,
            Self::DnsSplitMapping {
                resolvers: Some(_), ..
            } => ActionId::AdminDnsSplitEdit,
            Self::DnsSplitMapping {
                resolvers: None, ..
            } => ActionId::AdminDnsSplitRemove,
            Self::UserApproval => ActionId::AdminUserApprove,
            Self::UserRole { .. } => ActionId::AdminUserRoleChange,
            Self::UserSuspend => ActionId::AdminUserSuspend,
            Self::UserRestore => ActionId::AdminUserRestore,
            Self::UserDelete => ActionId::AdminUserDelete,
        }
    }

    pub const fn risk(&self) -> Risk {
        match self {
            Self::DeviceRename { .. } => Risk::Reversible,
            Self::DeviceTags { .. }
            | Self::DeviceApproval { .. }
            | Self::DeviceKeyExpiry { .. }
            | Self::DeviceRoutes { .. }
            | Self::DnsNameservers { .. }
            | Self::DnsPreferences { .. }
            | Self::DnsSearchPaths { .. }
            | Self::DnsSplitMapping { .. }
            | Self::UserApproval
            | Self::UserRole { .. }
            | Self::UserRestore => Risk::Disruptive,
            Self::DeviceExpireNow | Self::DeviceDelete | Self::UserSuspend | Self::UserDelete => {
                Risk::DestructiveOrSecret
            }
        }
    }

    pub const fn resource_kind(&self) -> AdminResourceKind {
        match self {
            Self::DeviceRoutes { .. } => AdminResourceKind::DeviceRoutes,
            Self::DnsNameservers { .. }
            | Self::DnsPreferences { .. }
            | Self::DnsSearchPaths { .. }
            | Self::DnsSplitMapping { .. } => AdminResourceKind::TailnetDns,
            Self::UserApproval
            | Self::UserRole { .. }
            | Self::UserSuspend
            | Self::UserRestore
            | Self::UserDelete => AdminResourceKind::User,
            Self::DeviceRename { .. }
            | Self::DeviceTags { .. }
            | Self::DeviceApproval { .. }
            | Self::DeviceKeyExpiry { .. }
            | Self::DeviceExpireNow
            | Self::DeviceDelete => AdminResourceKind::Device,
        }
    }

    pub fn lock_keys(&self, profile: &str, target_id: &str) -> Vec<AdminResourceLockKey> {
        let primary = AdminResourceLockKey::new(profile, self.resource_kind(), target_id);
        if matches!(self, Self::DeviceDelete) {
            vec![
                primary,
                AdminResourceLockKey::new(profile, AdminResourceKind::DeviceRoutes, target_id),
            ]
        } else {
            vec![primary]
        }
    }

    pub const fn audit_action_class(&self) -> &'static str {
        match self {
            Self::DeviceRename { .. } => "Update name for node",
            Self::DeviceTags { .. } => "Update tags for node",
            Self::DeviceApproval { authorized: true } => "Approve node",
            Self::DeviceApproval { authorized: false } => "Approve node",
            Self::DeviceKeyExpiry { disabled: true } => "Disable key expiry for node",
            Self::DeviceKeyExpiry { disabled: false } => "Enable key expiry for node",
            Self::DeviceExpireNow => "Expire node key",
            Self::DeviceDelete => "Delete node",
            Self::DeviceRoutes { .. } => "Update approved routes for node",
            Self::DnsNameservers { .. }
            | Self::DnsPreferences { .. }
            | Self::DnsSearchPaths { .. }
            | Self::DnsSplitMapping { .. } => "Update DNS configuration for tailnet",
            Self::UserApproval => "Approve user",
            Self::UserRole { .. } => "Update role for user",
            Self::UserSuspend => "Suspend user",
            Self::UserRestore => "Restore user",
            Self::UserDelete => "Delete user",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminPreflight<T> {
    pub observed_at: Timestamp,
    pub snapshot: T,
    pub fields: BTreeMap<String, String>,
}

impl<T> AdminPreflight<T> {
    pub fn is_fresh_at(&self, now: Timestamp) -> bool {
        now >= self.observed_at
            && now.saturating_sub(self.observed_at) <= REMOTE_WRITE_PREFLIGHT_MAX_AGE
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminMutation<TTarget, TChange> {
    pub mutation_id: u64,
    pub action_id: ActionId,
    pub profile: String,
    pub target_id: String,
    pub base_snapshot: TTarget,
    pub change: TChange,
    pub risk: Risk,
    pub preflight: Option<AdminPreflight<TTarget>>,
    pub state: AdminMutationState,
}

impl<TTarget: Clone, TChange> AdminMutation<TTarget, TChange> {
    pub fn new(
        mutation_id: u64,
        profile: impl Into<String>,
        target_id: impl Into<String>,
        base_snapshot: TTarget,
        change: TChange,
        action_id: ActionId,
        risk: Risk,
    ) -> Self {
        Self {
            mutation_id,
            action_id,
            profile: profile.into(),
            target_id: target_id.into(),
            base_snapshot,
            change,
            risk,
            preflight: None,
            state: AdminMutationState::Editing,
        }
    }

    pub fn begin_preflight(&mut self) -> Result<(), AdminMutationTransitionError> {
        transition(&mut self.state, AdminMutationState::Preflighting)
    }

    pub fn set_preflight(
        &mut self,
        preflight: AdminPreflight<TTarget>,
    ) -> Result<(), AdminMutationTransitionError> {
        self.preflight = Some(preflight);
        transition(&mut self.state, AdminMutationState::AwaitingConfirmation)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AdminMutationState {
    Editing,
    Preflighting,
    ConflictDetected,
    AwaitingConfirmation,
    Dispatching,
    Verifying,
    CorrelatingAudit,
    Succeeded,
    SucceededUnverified,
    PartiallySucceeded,
    Failed,
    CancelledBeforeDispatch,
    OutcomeUnknown,
}

impl AdminMutationState {
    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded
                | Self::SucceededUnverified
                | Self::PartiallySucceeded
                | Self::Failed
                | Self::CancelledBeforeDispatch
                | Self::OutcomeUnknown
        )
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct AdminMutationTransitionError {
    pub from: AdminMutationState,
    pub to: AdminMutationState,
}

impl std::fmt::Display for AdminMutationTransitionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid admin mutation transition from {:?} to {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for AdminMutationTransitionError {}

pub fn can_transition(from: AdminMutationState, to: AdminMutationState) -> bool {
    match from {
        AdminMutationState::Editing => matches!(
            to,
            AdminMutationState::Preflighting | AdminMutationState::CancelledBeforeDispatch
        ),
        AdminMutationState::Preflighting => matches!(
            to,
            AdminMutationState::ConflictDetected
                | AdminMutationState::AwaitingConfirmation
                | AdminMutationState::Failed
                | AdminMutationState::CancelledBeforeDispatch
        ),
        AdminMutationState::ConflictDetected => matches!(
            to,
            AdminMutationState::Editing
                | AdminMutationState::Preflighting
                | AdminMutationState::CancelledBeforeDispatch
        ),
        AdminMutationState::AwaitingConfirmation => matches!(
            to,
            AdminMutationState::Dispatching
                | AdminMutationState::Editing
                | AdminMutationState::CancelledBeforeDispatch
        ),
        AdminMutationState::Dispatching => matches!(
            to,
            AdminMutationState::Verifying
                | AdminMutationState::Failed
                | AdminMutationState::OutcomeUnknown
        ),
        AdminMutationState::Verifying => matches!(
            to,
            AdminMutationState::CorrelatingAudit
                | AdminMutationState::Failed
                | AdminMutationState::OutcomeUnknown
        ),
        AdminMutationState::CorrelatingAudit => matches!(
            to,
            AdminMutationState::Succeeded
                | AdminMutationState::SucceededUnverified
                | AdminMutationState::PartiallySucceeded
                | AdminMutationState::Failed
                | AdminMutationState::OutcomeUnknown
        ),
        AdminMutationState::Succeeded
        | AdminMutationState::SucceededUnverified
        | AdminMutationState::PartiallySucceeded
        | AdminMutationState::Failed
        | AdminMutationState::CancelledBeforeDispatch
        | AdminMutationState::OutcomeUnknown => false,
    }
}

pub fn transition(
    state: &mut AdminMutationState,
    next: AdminMutationState,
) -> Result<(), AdminMutationTransitionError> {
    if can_transition(*state, next) {
        *state = next;
        Ok(())
    } else {
        Err(AdminMutationTransitionError {
            from: *state,
            to: next,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FieldConflict {
    pub field: String,
    pub base: String,
    pub fresh: String,
    pub requested: String,
}

pub fn compare_preflight(
    base: &BTreeMap<String, String>,
    fresh: &BTreeMap<String, String>,
    requested: &BTreeMap<String, String>,
) -> Vec<FieldConflict> {
    let mut keys = BTreeSet::new();
    keys.extend(base.keys().cloned());
    keys.extend(fresh.keys().cloned());
    keys.extend(requested.keys().cloned());
    keys.into_iter()
        .filter_map(|field| {
            let base_value = base.get(&field).cloned().unwrap_or_default();
            let fresh_value = fresh.get(&field).cloned().unwrap_or_default();
            let requested_value = requested.get(&field).cloned().unwrap_or_default();
            (base_value != fresh_value).then_some(FieldConflict {
                field,
                base: base_value,
                fresh: fresh_value,
                requested: requested_value,
            })
        })
        .collect()
}

#[derive(Clone, Default)]
pub struct AdminResourceLocks {
    owners: Arc<Mutex<BTreeMap<AdminResourceLockKey, u64>>>,
}

impl std::fmt::Debug for AdminResourceLocks {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdminResourceLocks")
            .field("held", &self.held_count())
            .finish()
    }
}

impl AdminResourceLocks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acquire(
        &self,
        mutation_id: u64,
        keys: impl IntoIterator<Item = AdminResourceLockKey>,
    ) -> Option<AdminResourceLockLease> {
        let keys = keys.into_iter().collect::<BTreeSet<_>>();
        let mut owners = match self.owners.lock() {
            Ok(owners) => owners,
            Err(poisoned) => poisoned.into_inner(),
        };
        if keys
            .iter()
            .any(|key| owners.get(key).is_some_and(|owner| *owner != mutation_id))
        {
            return None;
        }
        for key in &keys {
            owners.insert(key.clone(), mutation_id);
        }
        Some(AdminResourceLockLease {
            owners: Arc::clone(&self.owners),
            mutation_id,
            keys: keys.into_iter().collect(),
        })
    }

    pub fn try_hold(
        &self,
        mutation_id: u64,
        keys: impl IntoIterator<Item = AdminResourceLockKey>,
    ) -> bool {
        let keys = keys.into_iter().collect::<BTreeSet<_>>();
        let mut owners = match self.owners.lock() {
            Ok(owners) => owners,
            Err(poisoned) => poisoned.into_inner(),
        };
        if keys
            .iter()
            .any(|key| owners.get(key).is_some_and(|owner| *owner != mutation_id))
        {
            return false;
        }
        for key in keys {
            owners.insert(key, mutation_id);
        }
        true
    }

    pub fn release(&self, mutation_id: u64) {
        let mut owners = match self.owners.lock() {
            Ok(owners) => owners,
            Err(poisoned) => poisoned.into_inner(),
        };
        owners.retain(|_, owner| *owner != mutation_id);
    }

    pub fn held_count(&self) -> usize {
        match self.owners.lock() {
            Ok(owners) => owners.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }

    pub fn is_held(&self, key: &AdminResourceLockKey) -> bool {
        match self.owners.lock() {
            Ok(owners) => owners.contains_key(key),
            Err(poisoned) => poisoned.into_inner().contains_key(key),
        }
    }
}

pub struct AdminResourceLockLease {
    owners: Arc<Mutex<BTreeMap<AdminResourceLockKey, u64>>>,
    mutation_id: u64,
    keys: Vec<AdminResourceLockKey>,
}

impl std::fmt::Debug for AdminResourceLockLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdminResourceLockLease")
            .field("mutation_id", &self.mutation_id)
            .field("key_count", &self.keys.len())
            .finish()
    }
}

impl Drop for AdminResourceLockLease {
    fn drop(&mut self) {
        let mut owners = match self.owners.lock() {
            Ok(owners) => owners,
            Err(poisoned) => poisoned.into_inner(),
        };
        for key in &self.keys {
            if owners
                .get(key)
                .is_some_and(|owner| *owner == self.mutation_id)
            {
                owners.remove(key);
            }
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BatchTarget {
    pub target_id: String,
    pub target_label: String,
    pub requested_change: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BatchChildOutcome {
    VerifiedSuccess,
    SucceededUnverified,
    FailedBeforeDispatch,
    OutcomeUnknown,
    CancelledBeforeDispatch,
    Failed,
}

impl BatchChildOutcome {
    pub const fn label(self) -> &'static str {
        match self {
            Self::VerifiedSuccess => "VerifiedSuccess",
            Self::SucceededUnverified => "SucceededUnverified",
            Self::FailedBeforeDispatch => "FailedBeforeDispatch",
            Self::OutcomeUnknown => "OutcomeUnknown",
            Self::CancelledBeforeDispatch => "CancelledBeforeDispatch",
            Self::Failed => "Failed",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BatchMutation {
    pub parent_task_id: u64,
    pub action_id: ActionId,
    pub targets: Vec<BatchTarget>,
    pub max_concurrency: usize,
    pub child_outcomes: BTreeMap<String, BatchChildOutcome>,
}

impl BatchMutation {
    pub fn new(
        parent_task_id: u64,
        action_id: ActionId,
        targets: Vec<BatchTarget>,
        max_concurrency: usize,
    ) -> Self {
        let mut unique = BTreeSet::new();
        let targets = targets
            .into_iter()
            .filter(|target| unique.insert(target.target_id.clone()))
            .collect::<Vec<_>>();
        Self {
            parent_task_id,
            action_id,
            targets,
            max_concurrency: max_concurrency.clamp(1, 4),
            child_outcomes: BTreeMap::new(),
        }
    }

    pub fn target_list_is_unchanged(&self, target_ids: &[String]) -> bool {
        self.targets
            .iter()
            .map(|target| target.target_id.as_str())
            .eq(target_ids.iter().map(String::as_str))
    }

    pub fn record(&mut self, target_id: impl Into<String>, outcome: BatchChildOutcome) {
        self.child_outcomes.insert(target_id.into(), outcome);
    }

    pub fn has_partial_failure(&self) -> bool {
        self.has_failures()
            && self
                .child_outcomes
                .values()
                .any(|outcome| *outcome == BatchChildOutcome::VerifiedSuccess)
    }

    pub fn has_failures(&self) -> bool {
        self.child_outcomes.values().any(|outcome| {
            matches!(
                outcome,
                BatchChildOutcome::FailedBeforeDispatch
                    | BatchChildOutcome::OutcomeUnknown
                    | BatchChildOutcome::CancelledBeforeDispatch
                    | BatchChildOutcome::Failed
                    | BatchChildOutcome::SucceededUnverified
            )
        })
    }

    pub fn verified_count(&self) -> usize {
        self.child_outcomes
            .values()
            .filter(|outcome| **outcome == BatchChildOutcome::VerifiedSuccess)
            .count()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuditCorrelation {
    pub candidate_event_ids: Vec<String>,
    pub polling_stopped: bool,
}

impl AuditCorrelation {
    pub const fn none() -> Self {
        Self {
            candidate_event_ids: Vec::new(),
            polling_stopped: false,
        }
    }

    pub fn is_ambiguous(&self) -> bool {
        self.candidate_event_ids.len() > 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_requires_preflight_dispatch_verify_and_correlation() {
        let states = [
            AdminMutationState::Editing,
            AdminMutationState::Preflighting,
            AdminMutationState::AwaitingConfirmation,
            AdminMutationState::Dispatching,
            AdminMutationState::Verifying,
            AdminMutationState::CorrelatingAudit,
            AdminMutationState::Succeeded,
        ];
        for pair in states.windows(2) {
            assert!(can_transition(pair[0], pair[1]));
        }
        assert!(!can_transition(
            AdminMutationState::Succeeded,
            AdminMutationState::Editing
        ));
        assert!(can_transition(
            AdminMutationState::Dispatching,
            AdminMutationState::OutcomeUnknown
        ));
    }

    #[test]
    fn every_remote_write_preflight_expires() {
        let preflight = AdminPreflight {
            observed_at: 100,
            snapshot: (),
            fields: BTreeMap::new(),
        };
        assert!(preflight.is_fresh_at(130));
        assert!(!preflight.is_fresh_at(131));
        assert!(!preflight.is_fresh_at(99));
    }

    #[test]
    fn resource_locks_serialize_same_key_and_allow_unrelated_keys() {
        let locks = AdminResourceLocks::new();
        let first = locks.acquire(
            1,
            [AdminResourceLockKey::new(
                "ops",
                AdminResourceKind::Device,
                "device-a",
            )],
        );
        assert!(first.is_some());
        let same = locks.acquire(
            2,
            [AdminResourceLockKey::new(
                "ops",
                AdminResourceKind::Device,
                "device-a",
            )],
        );
        assert!(same.is_none());
        let other = locks.acquire(
            3,
            [AdminResourceLockKey::new(
                "ops",
                AdminResourceKind::Device,
                "device-b",
            )],
        );
        assert!(other.is_some());
        drop(first);
        assert!(
            locks
                .acquire(
                    2,
                    [AdminResourceLockKey::new(
                        "ops",
                        AdminResourceKind::Device,
                        "device-a",
                    )]
                )
                .is_some()
        );
    }

    #[test]
    fn conflict_comparison_keeps_base_fresh_and_requested_values() {
        let base = BTreeMap::from([(String::from("name"), String::from("old"))]);
        let fresh = BTreeMap::from([(String::from("name"), String::from("changed"))]);
        let requested = BTreeMap::from([(String::from("name"), String::from("requested"))]);
        assert_eq!(
            compare_preflight(&base, &fresh, &requested),
            vec![FieldConflict {
                field: String::from("name"),
                base: String::from("old"),
                fresh: String::from("changed"),
                requested: String::from("requested"),
            }]
        );
    }
}
