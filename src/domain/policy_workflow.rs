use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::Timestamp;

pub const MAX_POLICY_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_APPLY_AGE: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PolicyState {
    Opening,
    EditingExternally,
    CandidateReady,
    RemoteConflict,
    Validating,
    Invalid,
    Previewing,
    ReadyToApply,
    Applying,
    Verifying,
    Succeeded,
    SucceededUnverified,
    FailedRetained,
    OutcomeUnknown,
    Closed,
}

impl PolicyState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Opening => "opening",
            Self::EditingExternally => "editing externally",
            Self::CandidateReady => "candidate ready",
            Self::RemoteConflict => "remote conflict",
            Self::Validating => "validating",
            Self::Invalid => "invalid",
            Self::Previewing => "previewing",
            Self::ReadyToApply => "ready to apply",
            Self::Applying => "applying",
            Self::Verifying => "verifying",
            Self::Succeeded => "succeeded",
            Self::SucceededUnverified => "succeeded, verification unavailable",
            Self::FailedRetained => "failed; candidate retained",
            Self::OutcomeUnknown => "outcome unknown",
            Self::Closed => "closed",
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct PolicyDocument {
    bytes: Vec<u8>,
    hash: String,
    content_type: String,
    observed_at: Timestamp,
}

impl fmt::Debug for PolicyDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PolicyDocument")
            .field("bytes", &format_args!("<{} bytes>", self.bytes.len()))
            .field("hash", &self.hash)
            .field("content_type", &self.content_type)
            .field("observed_at", &self.observed_at)
            .finish()
    }
}

impl PolicyDocument {
    pub fn from_bytes(bytes: Vec<u8>, observed_at: Timestamp) -> Result<Self, PolicyDocumentError> {
        Self::from_bytes_with_content_type(bytes, "application/hujson".to_owned(), observed_at)
    }

    pub fn from_bytes_with_content_type(
        bytes: Vec<u8>,
        content_type: String,
        observed_at: Timestamp,
    ) -> Result<Self, PolicyDocumentError> {
        if bytes.len() > MAX_POLICY_BYTES {
            return Err(PolicyDocumentError::TooLarge);
        }
        Ok(Self {
            hash: hash_bytes(&bytes),
            content_type,
            bytes,
            observed_at,
        })
    }

    pub fn from_slice(bytes: &[u8], observed_at: Timestamp) -> Result<Self, PolicyDocumentError> {
        Self::from_bytes(bytes.to_vec(), observed_at)
    }

    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn observed_at(&self) -> Timestamp {
        self.observed_at
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PolicyDocumentError {
    TooLarge,
}

impl fmt::Display for PolicyDocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => formatter.write_str("policy candidate exceeds the 4 MiB limit"),
        }
    }
}

impl std::error::Error for PolicyDocumentError {}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ValidationDiagnostic {
    pub message: String,
    pub user: Option<String>,
    pub severity: Option<String>,
    pub line: Option<u64>,
    pub column: Option<u64>,
    pub range: Option<String>,
    pub source: Option<String>,
    pub destination: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub test_id: Option<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PolicyValidation {
    pub candidate_hash: String,
    pub validated_at: Timestamp,
    pub valid: bool,
    pub message: Option<String>,
    pub bounded_safe_detail: Option<String>,
    pub diagnostics: Vec<ValidationDiagnostic>,
    pub server_tests: Vec<ServerPolicyTest>,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServerPolicyTest {
    pub name: String,
    pub passed: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PolicyPreview {
    pub candidate_hash: String,
    pub selector_type: PolicySelectorType,
    pub selector: String,
    pub matches: Vec<PolicyPreviewMatch>,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PolicySelectorType {
    User,
    IpPort,
}

impl PolicySelectorType {
    pub const fn api_value(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::IpPort => "ipport",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PolicyPreviewMatch {
    pub users: Vec<String>,
    pub ports: Vec<String>,
    pub line_number: Option<u64>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PolicyDiff {
    pub base_hash: String,
    pub candidate_hash: String,
    pub base_observed_at: Timestamp,
    pub candidate_observed_at: Timestamp,
    pub text: String,
    pub additions: usize,
    pub removals: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PolicyWorkflowSummary {
    pub workflow_id: u64,
    pub profile: String,
    pub tailnet: String,
    pub state: PolicyState,
    pub base_hash: Option<String>,
    pub candidate_hash: Option<String>,
    pub latest_remote_hash: Option<String>,
    pub candidate_bytes: usize,
    pub candidate_path: Option<PathBuf>,
    pub latest_remote_path: Option<PathBuf>,
    pub validation_bound_to_candidate: bool,
    pub preview_bound_to_candidate: bool,
}

pub struct PolicyWorkflow {
    workflow_id: u64,
    profile: String,
    tailnet: String,
    state: PolicyState,
    base: Option<PolicyDocument>,
    candidate: Option<PolicyDocument>,
    latest_remote: Option<PolicyDocument>,
    candidate_path: Option<PathBuf>,
    latest_remote_path: Option<PathBuf>,
    validation: Option<PolicyValidation>,
    preview: Option<PolicyPreview>,
    diff: Option<PolicyDiff>,
    opened_at: Timestamp,
}

impl fmt::Debug for PolicyWorkflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PolicyWorkflow")
            .field("summary", &self.summary())
            .field(
                "validation",
                &self
                    .validation
                    .as_ref()
                    .map(|value| value.candidate_hash.as_str()),
            )
            .field(
                "preview",
                &self
                    .preview
                    .as_ref()
                    .map(|value| value.candidate_hash.as_str()),
            )
            .field(
                "diff",
                &self
                    .diff
                    .as_ref()
                    .map(|value| (value.additions, value.removals)),
            )
            .finish()
    }
}

impl PolicyWorkflow {
    pub fn opening(
        workflow_id: u64,
        profile: String,
        tailnet: String,
        opened_at: Timestamp,
    ) -> Self {
        Self {
            workflow_id,
            profile,
            tailnet,
            state: PolicyState::Opening,
            base: None,
            candidate: None,
            latest_remote: None,
            candidate_path: None,
            latest_remote_path: None,
            validation: None,
            preview: None,
            diff: None,
            opened_at,
        }
    }

    pub fn workflow_id(&self) -> u64 {
        self.workflow_id
    }
    pub fn profile(&self) -> &str {
        &self.profile
    }
    pub fn tailnet(&self) -> &str {
        &self.tailnet
    }
    pub fn state(&self) -> PolicyState {
        self.state
    }
    pub fn base(&self) -> Option<&PolicyDocument> {
        self.base.as_ref()
    }
    pub fn candidate(&self) -> Option<&PolicyDocument> {
        self.candidate.as_ref()
    }
    pub fn latest_remote(&self) -> Option<&PolicyDocument> {
        self.latest_remote.as_ref()
    }
    pub fn validation(&self) -> Option<&PolicyValidation> {
        self.validation.as_ref()
    }
    pub fn preview(&self) -> Option<&PolicyPreview> {
        self.preview.as_ref()
    }
    pub fn diff(&self) -> Option<&PolicyDiff> {
        self.diff.as_ref()
    }
    pub fn candidate_path(&self) -> Option<&std::path::Path> {
        self.candidate_path.as_deref()
    }
    pub fn latest_remote_path(&self) -> Option<&std::path::Path> {
        self.latest_remote_path.as_deref()
    }
    pub fn opened_at(&self) -> Timestamp {
        self.opened_at
    }

    pub fn set_base(&mut self, document: PolicyDocument) {
        self.base = Some(document);
        self.latest_remote = None;
        self.latest_remote_path = None;
        self.candidate = None;
        self.candidate_path = None;
        self.validation = None;
        self.preview = None;
        self.diff = None;
        self.state = PolicyState::EditingExternally;
    }

    pub fn set_candidate(&mut self, document: PolicyDocument, path: PathBuf) {
        self.candidate = Some(document);
        self.candidate_path = Some(path);
        self.validation = None;
        self.preview = None;
        self.diff = None;
        self.state = if self
            .latest_remote
            .as_ref()
            .zip(self.base.as_ref())
            .is_some_and(|(remote, base)| remote.hash() != base.hash())
        {
            PolicyState::RemoteConflict
        } else {
            PolicyState::CandidateReady
        };
    }

    pub fn mark_editing_externally(&mut self) {
        if self.candidate.is_some() {
            self.state = PolicyState::EditingExternally;
        }
    }

    pub fn set_latest_remote(&mut self, document: PolicyDocument) {
        self.set_latest_remote_with_path(document, None);
    }

    pub fn set_latest_remote_with_path(&mut self, document: PolicyDocument, path: Option<PathBuf>) {
        let conflict = self
            .candidate
            .as_ref()
            .zip(self.base.as_ref())
            .is_some_and(|(_, base)| document.hash() != base.hash());
        self.latest_remote = Some(document);
        self.latest_remote_path = path;
        if conflict {
            self.state = PolicyState::RemoteConflict;
        } else if self.state == PolicyState::RemoteConflict {
            self.state = if self.validation.as_ref().is_some_and(validation_is_success)
                && self.preview.as_ref().is_some_and(|preview| {
                    self.candidate
                        .as_ref()
                        .is_some_and(|candidate| preview.candidate_hash == candidate.hash())
                }) {
                PolicyState::ReadyToApply
            } else if self.validation.as_ref().is_some_and(|value| !value.valid) {
                PolicyState::Invalid
            } else {
                PolicyState::CandidateReady
            };
        }
    }

    pub fn set_validation(&mut self, validation: PolicyValidation) -> bool {
        let bound = self
            .candidate
            .as_ref()
            .is_some_and(|candidate| candidate.hash() == validation.candidate_hash);
        if bound {
            self.validation = Some(validation);
            let validation_success = self.validation.as_ref().is_some_and(validation_is_success);
            let validation_state = if !validation_success {
                PolicyState::Invalid
            } else if self.preview.as_ref().is_some_and(|preview| {
                self.candidate
                    .as_ref()
                    .is_some_and(|candidate| preview.candidate_hash == candidate.hash())
            }) {
                PolicyState::ReadyToApply
            } else {
                PolicyState::CandidateReady
            };
            self.state = if self.has_remote_conflict() {
                PolicyState::RemoteConflict
            } else {
                validation_state
            };
        }
        bound
    }

    pub fn set_preview(&mut self, preview: PolicyPreview) -> bool {
        let bound = self
            .candidate
            .as_ref()
            .is_some_and(|candidate| candidate.hash() == preview.candidate_hash);
        if bound {
            self.preview = Some(preview);
            if self.validation.as_ref().is_some_and(validation_is_success) {
                self.state = if self.has_remote_conflict() {
                    PolicyState::RemoteConflict
                } else {
                    PolicyState::ReadyToApply
                };
            } else {
                self.state = PolicyState::Invalid;
            }
        }
        bound
    }

    pub fn set_diff(&mut self, diff: PolicyDiff) -> bool {
        let bound = self
            .candidate
            .as_ref()
            .is_some_and(|candidate| candidate.hash() == diff.candidate_hash);
        if bound {
            self.diff = Some(diff);
        }
        bound
    }

    fn has_remote_conflict(&self) -> bool {
        self.latest_remote
            .as_ref()
            .zip(self.base.as_ref())
            .is_some_and(|(remote, base)| remote.hash() != base.hash())
    }

    pub fn mark_validating(&mut self) {
        self.state = PolicyState::Validating;
    }
    pub fn mark_previewing(&mut self) {
        self.state = PolicyState::Previewing;
    }
    pub fn mark_applying(&mut self) {
        self.state = PolicyState::Applying;
    }
    pub fn mark_verifying(&mut self) {
        self.state = PolicyState::Verifying;
    }
    pub fn mark_succeeded(&mut self) {
        self.state = PolicyState::Succeeded;
    }
    pub fn mark_succeeded_unverified(&mut self) {
        self.state = PolicyState::SucceededUnverified;
    }
    pub fn mark_unknown(&mut self) {
        self.state = PolicyState::OutcomeUnknown;
    }
    pub fn retain_failure(&mut self) {
        self.state = PolicyState::FailedRetained;
    }
    pub fn discard_candidate(&mut self) {
        self.candidate = None;
        self.validation = None;
        self.preview = None;
        self.diff = None;
        self.candidate_path = None;
        self.latest_remote_path = None;
        self.state = if self.base.is_some() {
            PolicyState::EditingExternally
        } else {
            PolicyState::Opening
        };
    }
    pub fn close(&mut self) {
        self.state = PolicyState::Closed;
        self.candidate = None;
        self.latest_remote = None;
        self.latest_remote_path = None;
        self.validation = None;
        self.preview = None;
        self.diff = None;
        self.candidate_path = None;
    }

    pub fn apply_guard(&self, now: Timestamp) -> Result<(), PolicyApplyGuardError> {
        if self.state != PolicyState::ReadyToApply {
            return Err(PolicyApplyGuardError::NotReady);
        }
        let base = self
            .base
            .as_ref()
            .ok_or(PolicyApplyGuardError::MissingBase)?;
        let candidate = self
            .candidate
            .as_ref()
            .ok_or(PolicyApplyGuardError::MissingCandidate)?;
        if now < candidate.observed_at()
            || now.saturating_sub(candidate.observed_at()) > MAX_APPLY_AGE.as_secs()
        {
            return Err(PolicyApplyGuardError::StaleCandidate);
        }
        if self
            .latest_remote
            .as_ref()
            .is_some_and(|remote| remote.hash() != base.hash())
        {
            return Err(PolicyApplyGuardError::RemoteChanged);
        }
        let Some(validation) = self.validation.as_ref() else {
            return Err(PolicyApplyGuardError::ValidationNotBound);
        };
        if validation.candidate_hash != candidate.hash() || !validation_is_success(validation) {
            return Err(PolicyApplyGuardError::ValidationNotBound);
        }
        if now < validation.validated_at
            || now.saturating_sub(validation.validated_at) > MAX_APPLY_AGE.as_secs()
        {
            return Err(PolicyApplyGuardError::StaleValidation);
        }
        if self
            .preview
            .as_ref()
            .is_none_or(|value| value.candidate_hash != candidate.hash())
        {
            return Err(PolicyApplyGuardError::PreviewNotBound);
        }
        Ok(())
    }

    pub fn summary(&self) -> PolicyWorkflowSummary {
        PolicyWorkflowSummary {
            workflow_id: self.workflow_id,
            profile: self.profile.clone(),
            tailnet: self.tailnet.clone(),
            state: self.state,
            base_hash: self.base.as_ref().map(|value| value.hash.clone()),
            candidate_hash: self.candidate.as_ref().map(|value| value.hash.clone()),
            latest_remote_hash: self.latest_remote.as_ref().map(|value| value.hash.clone()),
            candidate_bytes: self.candidate.as_ref().map_or(0, PolicyDocument::len),
            candidate_path: self.candidate_path.clone(),
            latest_remote_path: self.latest_remote_path.clone(),
            validation_bound_to_candidate: self
                .validation
                .as_ref()
                .zip(self.candidate.as_ref())
                .is_some_and(|(a, b)| a.candidate_hash == b.hash()),
            preview_bound_to_candidate: self
                .preview
                .as_ref()
                .zip(self.candidate.as_ref())
                .is_some_and(|(a, b)| a.candidate_hash == b.hash()),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PolicyApplyGuardError {
    NotReady,
    MissingBase,
    MissingCandidate,
    StaleCandidate,
    StaleValidation,
    RemoteChanged,
    ValidationNotBound,
    PreviewNotBound,
}

impl fmt::Display for PolicyApplyGuardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotReady => "policy is not ready to apply",
            Self::MissingBase => "policy base is unavailable",
            Self::MissingCandidate => "policy candidate is unavailable",
            Self::StaleCandidate => "policy candidate is older than five minutes",
            Self::StaleValidation => "server validation is older than five minutes",
            Self::RemoteChanged => "remote policy changed since the base was fetched",
            Self::ValidationNotBound => "server validation is not bound to the candidate",
            Self::PreviewNotBound => "server permission preview is not bound to the candidate",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for PolicyApplyGuardError {}

pub fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validation_is_success(validation: &PolicyValidation) -> bool {
    validation.valid
        && validation
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.errors.is_empty())
        && validation.server_tests.iter().all(|test| test.passed)
}

#[cfg(test)]
mod tests {
    use super::{PolicyDocument, PolicyState, PolicyWorkflow, hash_bytes};

    #[test]
    fn hash_and_bytes_are_exact() {
        let bytes = b"{\n  // comment\r\n}\n".to_vec();
        let document = PolicyDocument::from_bytes(bytes.clone(), 10)
            .map_err(|_| ())
            .ok();
        assert!(document.is_some());
        let document = document.map(|value| (value.bytes().to_vec(), value.hash().to_owned()));
        assert_eq!(
            document.as_ref().map(|value| value.0.as_slice()),
            Some(bytes.as_slice())
        );
        assert_eq!(
            document.as_ref().map(|value| value.1.as_str()),
            Some(hash_bytes(&bytes).as_str())
        );
    }

    #[test]
    fn candidate_bound_results_are_invalidated_on_replacement() {
        let mut workflow = PolicyWorkflow::opening(1, "p".to_owned(), "t".to_owned(), 1);
        let base = PolicyDocument::from_bytes(b"base".to_vec(), 1)
            .map_err(|_| ())
            .ok();
        assert!(base.is_some());
        if let Some(base) = base {
            workflow.set_base(base);
        }
        let candidate = PolicyDocument::from_bytes(b"candidate".to_vec(), 1)
            .map_err(|_| ())
            .ok();
        assert!(candidate.is_some());
        if let Some(candidate) = candidate {
            workflow.set_candidate(candidate, "/tmp/policy".into());
        }
        assert_eq!(workflow.state(), PolicyState::CandidateReady);
        let candidate = PolicyDocument::from_bytes(b"new".to_vec(), 1)
            .map_err(|_| ())
            .ok();
        assert!(candidate.is_some());
        if let Some(candidate) = candidate {
            workflow.set_candidate(candidate, "/tmp/policy".into());
        }
        assert!(workflow.validation().is_none());
        assert!(workflow.preview().is_none());
    }

    #[test]
    fn latest_remote_change_is_a_conflict_even_when_it_matches_candidate_bytes() {
        let mut workflow = PolicyWorkflow::opening(1, "p".to_owned(), "t".to_owned(), 1);
        let base = PolicyDocument::from_bytes(b"base".to_vec(), 1)
            .map_err(|_| ())
            .ok();
        let candidate = PolicyDocument::from_bytes(b"remote".to_vec(), 1)
            .map_err(|_| ())
            .ok();
        assert!(base.is_some() && candidate.is_some());
        if let (Some(base), Some(candidate)) = (base, candidate) {
            workflow.set_base(base);
            workflow.set_candidate(candidate.clone(), "/tmp/policy".into());
            workflow.set_latest_remote(candidate);
        }
        assert_eq!(workflow.state(), PolicyState::RemoteConflict);
    }
}
