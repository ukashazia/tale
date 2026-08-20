use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::admin::AdminRefreshResource;
use crate::admin::key_mutations::AuthKeyCreateRequest;
use crate::admin::mutation::AdminMutationRequest;
use crate::domain::Timestamp;
use crate::domain::access_explorer::AccessQuestion;
use crate::domain::flow::{AggregateDimension, FlowFilter, FlowMessage};
use crate::domain::health::HealthSnapshot;
use crate::domain::mutation::LocalMutation;
use crate::domain::operational::OperationalMutation;
use crate::domain::policy_workflow::PolicyDocument;
use crate::domain::policy_workflow::PolicySelectorType;
use crate::domain::secret_result::SecretBuffer;
use crate::domain::service::ServiceActionRequest;
use crate::domain::source::LocalExecutable;
use crate::local::client::ExecutableResolution;
use crate::local::diagnostics::DiagnosticRequest;
use crate::local::handoff::HandoffCommand;
use crate::mock::{MockLoadScenario, MockTaskBehavior};
use crate::task::{Task, TaskId};

/// A profile and the store reference its secret lives under. The backend itself
/// is resolved by the runtime, which already caches one per profile.
#[derive(Debug, Clone)]
pub struct ProfileCredentialRef {
    pub profile: String,
    pub credential: String,
}

#[derive(Debug, Clone)]
pub enum Effect {
    PersistTaskHistory(Vec<Task>),
    StartMockLoad {
        resource: Resource,
        generation: u64,
        scenario: MockLoadScenario,
    },
    StartMockTask {
        task_id: TaskId,
        behavior: MockTaskBehavior,
        started_at: Timestamp,
    },
    StartAdminRefresh {
        profile: String,
        tailnet: String,
        credential: String,
        generation: u64,
        timeout: Duration,
        audit_window_days: u64,
    },
    StartAdminResourceRefresh {
        profile: String,
        tailnet: String,
        credential: String,
        generation: u64,
        timeout: Duration,
        audit_window_days: u64,
        resources: Vec<AdminRefreshResource>,
    },
    StartAdminDeviceEnrichment {
        profile: String,
        credential: String,
        generation: u64,
        device_id: String,
        timeout: Duration,
    },
    StartAdminPreflight {
        request: AdminMutationRequest,
        tailnet: String,
        credential: String,
        timeout: Duration,
    },
    StartAdminMutation {
        task_id: TaskId,
        request: AdminMutationRequest,
        tailnet: String,
        credential: String,
        timeout: Duration,
    },
    StartOperationalMutation {
        operation_id: u64,
        admin_generation: u64,
        action_id: crate::action::ActionId,
        mutation: OperationalMutation,
        profile: String,
        tailnet: String,
        credential: String,
        timeout: Duration,
    },
    StartAccessExplorer {
        question: AccessQuestion,
        policy: PolicyDocument,
        profile: String,
        tailnet: String,
        credential: String,
        timeout: Duration,
    },
    StartHealthEvaluation {
        generation: u64,
        snapshot: HealthSnapshot,
    },
    StartFlowAggregation {
        generation: u64,
        messages: Vec<FlowMessage>,
        filter: FlowFilter,
        dimensions: Vec<AggregateDimension>,
        cancellation: Arc<AtomicBool>,
    },
    StartPolicyRemoteFetch {
        workflow_id: u64,
        profile: String,
        tailnet: String,
        credential: String,
        timeout: Duration,
    },
    StartPolicyEditor {
        workflow_id: u64,
        command: crate::terminal::EditorCommand,
        path: PathBuf,
    },
    StartPolicyValidate {
        workflow_id: u64,
        profile: String,
        tailnet: String,
        credential: String,
        timeout: Duration,
        path: PathBuf,
    },
    StartPolicyPreview {
        workflow_id: u64,
        profile: String,
        tailnet: String,
        credential: String,
        timeout: Duration,
        path: PathBuf,
        selector_type: PolicySelectorType,
        selector: String,
    },
    StartPolicyApply {
        workflow_id: u64,
        profile: String,
        tailnet: String,
        credential: String,
        timeout: Duration,
        path: PathBuf,
        expected_base_hash: String,
        expected_candidate_hash: String,
    },
    StartAuthKeyCreate {
        result_id: u64,
        admin_generation: u64,
        profile: String,
        tailnet: String,
        credential: String,
        timeout: Duration,
        request: AuthKeyCreateRequest,
    },
    StartCredentialDetail {
        key_id: String,
        profile: String,
        tailnet: String,
        credential: String,
        timeout: Duration,
    },
    StartCredentialRevoke {
        key_id: String,
        profile: String,
        tailnet: String,
        credential: String,
        timeout: Duration,
    },
    StartProfileCredentialRemove {
        profile: String,
        reference: String,
    },
    /// Read what each profile's credential store holds. Local reads only: this
    /// is what `:profiles` can report without spending a single request.
    InspectProfileCredentials {
        profiles: Vec<ProfileCredentialRef>,
    },
    /// The one request `:profiles` makes, and only because the user asked for
    /// this profile to become active.
    StartProfileProbe {
        profile: String,
        tailnet: String,
        credential: String,
        timeout: Duration,
    },
    CopySecret {
        result_id: u64,
        secret: Arc<SecretBuffer>,
    },
    CopyText {
        text: String,
    },
    CancelAdminRefresh,
    DropAdminToken {
        profile: String,
    },
    StartLocalDiscovery {
        generation: u64,
        resolution: ExecutableResolution,
        timeout: Duration,
    },
    StartLocalObservation {
        generation: u64,
        initial_status_generation: u64,
        initial_preferences_generation: u64,
        socket_path: PathBuf,
        timeout: Duration,
        reconcile_interval: Duration,
    },
    StartLocalSnapshotRefresh {
        generation: u64,
        socket_path: PathBuf,
        timeout: Duration,
    },
    StartLocalAccounts {
        executable: LocalExecutable,
        timeout: Duration,
    },
    StartLocalPolicy {
        executable: LocalExecutable,
        timeout: Duration,
    },
    StartLocalMutation {
        mutation_id: u64,
        task_id: TaskId,
        executable: LocalExecutable,
        timeout: Duration,
        mutation: LocalMutation,
    },
    StartTerminalHandoff {
        task_id: TaskId,
        command: HandoffCommand,
    },
    ResumeTerminal,
    StartLocalDiagnostic {
        task_id: TaskId,
        executable: LocalExecutable,
        timeout: Duration,
        request: DiagnosticRequest,
    },
    StartLocalServicesRefresh {
        generation: u64,
        executable: LocalExecutable,
        timeout: Duration,
        alpha_enabled: bool,
    },
    StartServiceTask {
        task_id: TaskId,
        executable: LocalExecutable,
        timeout: Duration,
        request: ServiceActionRequest,
    },
    CancelLocalDiscovery,
    CancelLocalObservation,
    CancelTask {
        task_id: TaskId,
    },
    WriteConfigCandidate {
        path: PathBuf,
        bytes: Vec<u8>,
    },
    RequestShutdown,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Resource {
    Devices,
}
