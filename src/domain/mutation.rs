//! Local CLI mutation lifecycle.
//!
//! These mutations dispatch one local command and verify its effect against the
//! local daemon. Remote preflight conflicts, audit correlation, and editable
//! policy documents deliberately belong to their own domain workflows.

use std::sync::{Arc, Mutex};

use thiserror::Error;

use crate::action::{ActionId, Risk};
use crate::domain::preference::PreferenceRequest;
use crate::domain::route::{AdvertisementRequest, ExitNodeRequest};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MutationState {
    Editing,
    Preview,
    AwaitingConfirmation,
    Running,
    Verifying,
    Succeeded,
    Failed,
    CancelledBeforeDispatch,
    VerificationMismatch,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LocalMutation {
    Connect,
    Disconnect { accept_lose_ssh: bool },
    Preferences(PreferenceRequest),
    ExitNode(ExitNodeRequest),
    Advertisements(AdvertisementRequest),
    AccountSwitch { account_id: String },
    AccountRemove { account_id: String },
    SyspolicyReload,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum MutationResult {
    Verified {
        summary: String,
        detail: String,
        exit_status: Option<i32>,
    },
    CommandFailed {
        summary: String,
        detail: String,
        exit_status: Option<i32>,
    },
    CancelledBeforeDispatch {
        summary: String,
        detail: String,
        exit_status: Option<i32>,
    },
    ReadFailed {
        summary: String,
        detail: String,
        exit_status: Option<i32>,
    },
    VerificationMismatch {
        summary: String,
        detail: String,
        exit_status: Option<i32>,
    },
    OutcomeUnknown {
        summary: String,
        detail: String,
        exit_status: Option<i32>,
    },
}

impl MutationResult {
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Verified { .. })
    }

    pub fn summary(&self) -> &str {
        match self {
            Self::Verified { summary, .. }
            | Self::CommandFailed { summary, .. }
            | Self::CancelledBeforeDispatch { summary, .. }
            | Self::ReadFailed { summary, .. }
            | Self::VerificationMismatch { summary, .. }
            | Self::OutcomeUnknown { summary, .. } => summary,
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Verified { detail, .. }
            | Self::CommandFailed { detail, .. }
            | Self::CancelledBeforeDispatch { detail, .. }
            | Self::ReadFailed { detail, .. }
            | Self::VerificationMismatch { detail, .. }
            | Self::OutcomeUnknown { detail, .. } => detail,
        }
    }

    pub const fn exit_status(&self) -> Option<i32> {
        match self {
            Self::Verified { exit_status, .. }
            | Self::CommandFailed { exit_status, .. }
            | Self::CancelledBeforeDispatch { exit_status, .. }
            | Self::ReadFailed { exit_status, .. }
            | Self::VerificationMismatch { exit_status, .. }
            | Self::OutcomeUnknown { exit_status, .. } => *exit_status,
        }
    }
}

impl LocalMutation {
    pub const fn risk(&self) -> Risk {
        match self {
            Self::Connect
            | Self::Preferences(_)
            | Self::ExitNode(_)
            | Self::AccountSwitch { .. }
            | Self::SyspolicyReload => Risk::Reversible,
            Self::Advertisements(request) if request.accept_mac_app_connector_risk => {
                Risk::Disruptive
            }
            Self::Advertisements(_) => Risk::Reversible,
            Self::Disconnect { .. } => Risk::Disruptive,
            Self::AccountRemove { .. } => Risk::DestructiveOrSecret,
        }
    }

    pub fn action_id(&self) -> ActionId {
        match self {
            Self::Connect => ActionId::LocalConnect,
            Self::Disconnect { .. } => ActionId::LocalDisconnect,
            Self::Preferences(_) => ActionId::LocalPreferencesEdit,
            Self::ExitNode(_) => ActionId::LocalExitNodeSelect,
            Self::Advertisements(_) => ActionId::LocalRoutesEditAdvertisements,
            Self::AccountSwitch { .. } => ActionId::LocalAccountSwitch,
            Self::AccountRemove { .. } => ActionId::LocalAccountRemove,
            Self::SyspolicyReload => ActionId::LocalSyspolicyReload,
        }
    }
}

impl MutationState {
    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded
                | Self::Failed
                | Self::CancelledBeforeDispatch
                | Self::VerificationMismatch
        )
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
#[error("invalid mutation transition from {from:?} to {to:?}")]
pub struct MutationTransitionError {
    pub from: MutationState,
    pub to: MutationState,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Mutation<TTarget, TRequest> {
    pub id: u64,
    pub action_id: ActionId,
    pub target: TTarget,
    pub base_observation: TTarget,
    pub request: TRequest,
    pub risk: Risk,
    pub state: MutationState,
}

impl<TTarget: Clone, TRequest> Mutation<TTarget, TRequest> {
    pub fn new(
        id: u64,
        action_id: ActionId,
        target: TTarget,
        request: TRequest,
        risk: Risk,
    ) -> Self {
        Self {
            id,
            action_id,
            base_observation: target.clone(),
            target,
            request,
            risk,
            state: MutationState::Editing,
        }
    }

    pub fn transition(&mut self, next: MutationState) -> Result<(), MutationTransitionError> {
        if can_transition(self.state, next) {
            self.state = next;
            Ok(())
        } else {
            Err(MutationTransitionError {
                from: self.state,
                to: next,
            })
        }
    }
}

pub fn can_transition(from: MutationState, to: MutationState) -> bool {
    match from {
        MutationState::Editing => matches!(
            to,
            MutationState::Preview | MutationState::CancelledBeforeDispatch
        ),
        MutationState::Preview => matches!(
            to,
            MutationState::Editing
                | MutationState::AwaitingConfirmation
                | MutationState::CancelledBeforeDispatch
        ),
        MutationState::AwaitingConfirmation => matches!(
            to,
            MutationState::Running
                | MutationState::CancelledBeforeDispatch
                | MutationState::Editing
        ),
        MutationState::Running => matches!(to, MutationState::Verifying | MutationState::Failed),
        MutationState::Verifying => matches!(
            to,
            MutationState::Succeeded | MutationState::Failed | MutationState::VerificationMismatch
        ),
        MutationState::Succeeded
        | MutationState::Failed
        | MutationState::CancelledBeforeDispatch
        | MutationState::VerificationMismatch => false,
    }
}

#[derive(Clone, Default)]
pub struct MutationLock {
    owner: Arc<Mutex<Option<u64>>>,
}

impl std::fmt::Debug for MutationLock {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MutationLock")
            .field("held", &self.is_held())
            .finish()
    }
}

impl MutationLock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acquire(&self, mutation_id: u64) -> Option<MutationLease> {
        let mut owner = match self.owner.lock() {
            Ok(owner) => owner,
            Err(poisoned) => poisoned.into_inner(),
        };
        if owner.is_some() {
            return None;
        }
        *owner = Some(mutation_id);
        Some(MutationLease {
            owner: Arc::clone(&self.owner),
            mutation_id,
        })
    }

    pub fn hold(&self, mutation_id: u64) -> bool {
        let mut owner = match self.owner.lock() {
            Ok(owner) => owner,
            Err(poisoned) => poisoned.into_inner(),
        };
        if owner.is_some() {
            return false;
        }
        *owner = Some(mutation_id);
        true
    }

    pub fn release(&self, mutation_id: u64) {
        let mut owner = match self.owner.lock() {
            Ok(owner) => owner,
            Err(poisoned) => poisoned.into_inner(),
        };
        if *owner == Some(mutation_id) {
            *owner = None;
        }
    }

    pub fn is_held(&self) -> bool {
        match self.owner.lock() {
            Ok(owner) => owner.is_some(),
            Err(poisoned) => poisoned.into_inner().is_some(),
        }
    }

    pub fn owner(&self) -> Option<u64> {
        match self.owner.lock() {
            Ok(owner) => *owner,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }
}

pub struct MutationLease {
    owner: Arc<Mutex<Option<u64>>>,
    mutation_id: u64,
}

impl std::fmt::Debug for MutationLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MutationLease")
            .field("mutation_id", &self.mutation_id)
            .finish()
    }
}

impl Drop for MutationLease {
    fn drop(&mut self) {
        let mut owner = match self.owner.lock() {
            Ok(owner) => owner,
            Err(poisoned) => poisoned.into_inner(),
        };
        if *owner == Some(self.mutation_id) {
            *owner = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_accepts_only_the_declared_state_transitions() {
        let states = [
            MutationState::Editing,
            MutationState::Preview,
            MutationState::AwaitingConfirmation,
            MutationState::Running,
            MutationState::Verifying,
            MutationState::Succeeded,
        ];
        for pair in states.windows(2) {
            assert!(can_transition(pair[0], pair[1]));
        }
        assert!(!can_transition(
            MutationState::Succeeded,
            MutationState::Editing
        ));
        assert!(!can_transition(
            MutationState::Running,
            MutationState::Succeeded
        ));
        assert!(can_transition(
            MutationState::Running,
            MutationState::Failed
        ));
        assert!(can_transition(
            MutationState::Verifying,
            MutationState::VerificationMismatch
        ));
    }

    #[test]
    fn lock_rejects_a_second_owner_and_releases_the_first() {
        let lock = MutationLock::new();
        assert!(lock.hold(1));
        assert!(!lock.hold(2));
        assert_eq!(lock.owner(), Some(1));
        lock.release(2);
        assert_eq!(lock.owner(), Some(1));
        lock.release(1);
        assert!(!lock.is_held());
        let lease = lock.acquire(3);
        assert!(lease.is_some());
        drop(lease);
        assert!(!lock.is_held());
    }
}
