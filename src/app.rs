use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use sha2::{Digest, Sha256};

use crate::action::{self, ActionContext, ActionId, Capability};
use crate::admin::auth::SecretValue;
use crate::admin::client::AdminError;
use crate::admin::mutation::{
    AdminBatchConfirmation, AdminMutationRequest, AdminSnapshotFields, batch_target, parse_change,
};
use crate::admin::{
    self, AdminRefreshResource, AdminResource, AdminResourceResult, AdminResourceState,
    AdminSnapshot,
};
use crate::config::ResolvedConfig;
use crate::domain::access_explorer::{AccessQuestion, AccessResult, PolicySource};
use crate::domain::account::LocalAccount;
use crate::domain::activity::AuditFilters;
use crate::domain::admin_mutation::{
    AdminChange, AdminMutationState, AdminResourceLocks, AuditCorrelation, BatchMutation,
    BatchTarget, transition,
};
use crate::domain::certificate::{BugReportRequest, CertificateRequest};
use crate::domain::device::{
    ComposedDevice, Device, DeviceId, LocalDevice, SortDirection, SortField, SortSpec,
    compare_devices_by_specs, compose_exact_id,
};
use crate::domain::diagnostic::{DiagnosticResult, DiagnosticState};
use crate::domain::filter::{
    self, Comparison, FieldMatchMode, FilterExpression, FilterField, FilterTerm,
};
use crate::domain::flow::{
    AggregateDimension, FlowError, FlowFilter, FlowGeneration, FlowSnapshot, FlowWindow,
};
use crate::domain::health::Finding;
use crate::domain::log_stream::{LogStreamConfiguration, LogStreamStatus, LogType, SecretAction};
use crate::domain::mutation::{LocalMutation, MutationLock};
use crate::domain::operational::{
    ExportRequest, LogStreamMutationDraft, OperationalMutation, SavedViewMutation,
};
use crate::domain::policy::PolicySnapshot;
use crate::domain::policy_workflow::{PolicySelectorType, PolicyState, PolicyWorkflow};
use crate::domain::preference::{
    LocalPreferences, ObservedPreference, PreferenceEditability, PreferenceField, PreferenceRequest,
};
use crate::domain::redaction::{DiagnosticReportInput, redact_diagnostic_report};
use crate::domain::route::{
    AdvertisementRequest, ExitNodeCandidate, ExitNodeRequest, ExitNodeSelection,
    overlapping_routes, parse_route_set, parse_static_endpoints,
};
use crate::domain::saved_view::{
    FilterClause, FilterOperator, FilterValue, SavedView, SortDirection as SavedSortDirection,
    SortTerm,
};
use crate::domain::secret_result::{SecretInput, SecretMetadata, SecretResult};
use crate::domain::service::{
    Backend, CertificateVerification, Exposure, Listener, LocalServicesSnapshot, PathMount, Port,
    ProxyProtocol, ServiceActionRequest, ServiceCapabilities, ServiceConflictKey,
    ServiceFailureKind, ServiceMapping, ServiceResourceStatus, ServiceSection, ServiceTaskData,
};
use crate::domain::source::{
    LocalCapabilities, LocalCliState, LocalDaemonState, LocalExecutable, LocalFailure,
    LocalFailureKind, LocalPreferencesResource, LocalResource, LocalResourceStatus, LocalSnapshot,
    LocalState,
};
use crate::domain::transfer::{
    TaildriveShare, TaildropConflict, TaildropReceiveRequest, TaildropSendRequest, TaildropTarget,
    normalize_share_name, validate_receive_directory, validate_regular_file,
};
use crate::domain::webhook::{
    DestinationType, SubscriptionSet, WebhookDraft, WebhookEndpoint, WebhookMutation,
};
use crate::domain::{SourceHealth, Timestamp};
use crate::effect::{Effect, Resource};
use crate::event::{
    AdminEvent, CredentialEvent, CredentialRevocationResult, Event, InputEvent, LocalEvent,
    OperationalResult, PolicyApplyResult, PolicyEvent, ServicesEvent, ShutdownReason, SourceEvent,
    TaskEvent,
};
use crate::local::client::{ExecutableResolution, HostPlatform};
use crate::local::diagnostics::{self, DiagnosticRequest};
use crate::local::handoff::{self, HandoffCommand};
use crate::local::policy::SystemPolicyEntry;
use crate::local::{certificates, services, transfers};
use crate::mock::{MOCK_NOW, MockLoadScenario, MockTaskBehavior};
use crate::task::{Notification, TaskId, TaskState, TaskStore};
use crate::ui::theme::{Theme, ThemeId};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SourceMode {
    Mock,
    Local,
    Unavailable,
}

impl SourceMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Local => "local",
            Self::Unavailable => "local unavailable",
        }
    }

    pub const fn style_role(self) -> crate::ui::theme::StyleRole {
        match self {
            Self::Mock => crate::ui::theme::StyleRole::StateInfo,
            Self::Local => crate::ui::theme::StyleRole::SourceLocal,
            Self::Unavailable => crate::ui::theme::StyleRole::StateDisabled,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Route {
    Overview,
    Local,
    Devices,
    Users,
    Routes,
    Dns,
    Access,
    Credentials,
    Activity,
    Settings,
    Services,
}

impl Route {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Local => "local",
            Self::Devices => "devices",
            Self::Users => "users",
            Self::Routes => "routes",
            Self::Dns => "dns",
            Self::Access => "access",
            Self::Credentials => "credentials",
            Self::Activity => "activity",
            Self::Settings => "settings",
            Self::Services => "services",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "overview" | "ov" | "home" => Some(Self::Overview),
            "local" | "self" => Some(Self::Local),
            "devices" | "device" | "dev" | "nodes" => Some(Self::Devices),
            "users" | "user" => Some(Self::Users),
            "routes" | "route" | "rt" => Some(Self::Routes),
            "dns" => Some(Self::Dns),
            "access" | "policy" | "acl" | "grants" => Some(Self::Access),
            "credentials" | "credential" | "keys" | "auth" => Some(Self::Credentials),
            "activity" | "tasks" | "logs" | "events" => Some(Self::Activity),
            "settings" | "config" => Some(Self::Settings),
            "services" | "service" | "serve" | "funnel" => Some(Self::Services),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Focus {
    Collection,
    Inspector,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LineEditorState {
    pub input: String,
    pub cursor: usize,
    pub scroll: usize,
}

impl LineEditorState {
    pub fn new(input: String) -> Self {
        let cursor = input.len();
        Self {
            input,
            cursor,
            scroll: 0,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CompletionCandidate {
    pub id: String,
    pub insertion: String,
    pub label: String,
    pub description: String,
    pub alias: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommandLineState {
    pub editor: LineEditorState,
    pub candidates: Vec<CompletionCandidate>,
    pub selected_completion: Option<usize>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilterRestoration {
    pub input: String,
    pub expression: FilterExpression,
    pub selection: Option<DeviceId>,
    pub scroll: usize,
    pub task_filter: String,
    pub task_selection: Option<TaskId>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilterLineState {
    pub editor: LineEditorState,
    pub candidates: Vec<CompletionCandidate>,
    pub selected_completion: Option<usize>,
    pub error: Option<String>,
    pub restoration: FilterRestoration,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HelpSheetState {
    pub query: String,
    pub filtering: bool,
    pub scroll: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TransientKind {
    Action,
    Copy,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TransientMenuState {
    pub kind: TransientKind,
    pub actions: Vec<ActionId>,
    pub fields: Vec<CopyField>,
    pub prefix: Option<char>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum InteractionMode {
    Normal,
    CommandLine(CommandLineState),
    FilterLine(FilterLineState),
    Transient(TransientMenuState),
    HelpSheet(HelpSheetState),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DiagnosticInputKind {
    DnsQuery,
    Whois,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DiagnosticInputState {
    pub kind: DiagnosticInputKind,
    pub input: String,
    pub secondary: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ConfirmationState {
    pub action_id: ActionId,
    pub mutation: Option<LocalMutation>,
    pub admin_mutation: Option<AdminMutationRequest>,
    pub admin_batch: Option<AdminBatchConfirmation>,
    pub service_request: Option<ServiceActionRequest>,
    pub operational_mutation: Option<OperationalMutation>,
    pub handoff: Option<HandoffCommand>,
    pub prompt: String,
    pub required_phrase: Option<String>,
    pub input: String,
    pub lose_ssh_checked: bool,
    pub preview_lines: Vec<String>,
    pub redacted_argv: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
struct PendingAdminBatch {
    action_id: ActionId,
    requests: BTreeMap<u64, AdminMutationRequest>,
    ready: BTreeMap<u64, AdminMutationRequest>,
}

#[derive(Debug, Clone)]
struct AdminBatchInFlight {
    batch: BatchMutation,
    parent_task_id: TaskId,
    child_tasks: BTreeMap<u64, TaskId>,
    pending_requests: Vec<AdminMutationRequest>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OperatorFormState {
    pub action_id: ActionId,
    pub input: String,
    pub error: Option<String>,
    pub ordered_items: Option<Vec<String>>,
    pub ordered_selected: usize,
    pub ordered_editor: String,
    pub ordered_prefix: Option<String>,
    pub secret_input: Option<SecretInput>,
    pub secret_editing: bool,
}

impl OperatorFormState {
    pub fn new(action_id: ActionId, input: String, error: Option<String>) -> Self {
        Self {
            action_id,
            input,
            error,
            ordered_items: None,
            ordered_selected: 0,
            ordered_editor: String::new(),
            ordered_prefix: None,
            secret_input: None,
            secret_editing: false,
        }
    }

    pub fn ordered(
        action_id: ActionId,
        items: Vec<String>,
        prefix: Option<String>,
        error: Option<String>,
    ) -> Self {
        let editor = items.first().cloned().unwrap_or_default();
        let input = format_ordered_input(prefix.as_deref(), &items);
        Self {
            action_id,
            input,
            error,
            ordered_items: Some(items),
            ordered_selected: 0,
            ordered_editor: editor,
            ordered_prefix: prefix,
            secret_input: None,
            secret_editing: false,
        }
    }

    fn sync_ordered_input(&mut self) {
        let Some(items) = self.ordered_items.as_mut() else {
            return;
        };
        if items.is_empty() {
            if !self.ordered_editor.is_empty() {
                items.push(self.ordered_editor.clone());
                self.ordered_selected = 0;
            }
        } else if let Some(item) = items.get_mut(self.ordered_selected) {
            *item = self.ordered_editor.clone();
        }
        self.input = format_ordered_input(self.ordered_prefix.as_deref(), items);
    }

    fn select_ordered(&mut self, offset: isize) {
        self.sync_ordered_input();
        let Some(items) = self.ordered_items.as_ref() else {
            return;
        };
        if items.is_empty() {
            return;
        }
        self.ordered_selected = move_bounded_index(self.ordered_selected, items.len(), offset);
        self.ordered_editor = items
            .get(self.ordered_selected)
            .cloned()
            .unwrap_or_default();
    }

    fn move_ordered_item(&mut self, offset: isize) {
        self.sync_ordered_input();
        let Some(items) = self.ordered_items.as_mut() else {
            return;
        };
        if items.is_empty() {
            return;
        }
        let target = if offset.is_negative() {
            self.ordered_selected.saturating_sub(offset.unsigned_abs())
        } else {
            self.ordered_selected.saturating_add(offset as usize)
        };
        if target >= items.len() || target == self.ordered_selected {
            return;
        }
        items.swap(self.ordered_selected, target);
        self.ordered_selected = target;
        self.ordered_editor = items
            .get(self.ordered_selected)
            .cloned()
            .unwrap_or_default();
        self.input = format_ordered_input(self.ordered_prefix.as_deref(), items);
    }

    fn insert_ordered_item(&mut self) {
        self.sync_ordered_input();
        let Some(items) = self.ordered_items.as_mut() else {
            return;
        };
        let position = self.ordered_selected.saturating_add(1).min(items.len());
        items.insert(position, String::new());
        self.ordered_selected = position;
        self.ordered_editor.clear();
        self.input = format_ordered_input(self.ordered_prefix.as_deref(), items);
    }

    fn remove_ordered_item(&mut self) {
        self.sync_ordered_input();
        let Some(items) = self.ordered_items.as_mut() else {
            return;
        };
        if items.is_empty() {
            return;
        }
        items.remove(self.ordered_selected.min(items.len().saturating_sub(1)));
        self.ordered_selected = self.ordered_selected.min(items.len().saturating_sub(1));
        self.ordered_editor = items
            .get(self.ordered_selected)
            .cloned()
            .unwrap_or_default();
        self.input = format_ordered_input(self.ordered_prefix.as_deref(), items);
    }

    fn append_ordered_text(&mut self, text: &str) {
        if self.ordered_items.is_none() {
            self.input.push_str(text);
            return;
        }
        self.ordered_editor.push_str(text);
        self.error = None;
        self.sync_ordered_input();
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServiceFormState {
    pub action_id: ActionId,
    pub input: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServiceSectionPickerState {
    pub selected: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct AppearanceState {
    pub original: Theme,
    pub selected: ThemeId,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum HandoffInputKind {
    Ssh,
    Nc,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HandoffInputState {
    pub kind: HandoffInputKind,
    pub host: String,
    pub input: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AccountPickerState {
    pub action_id: ActionId,
    pub accounts: Vec<LocalAccount>,
    pub selected: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CopyField {
    DeviceId,
    DisplayName,
    Hostname,
    Owner,
    Addresses,
    Tags,
    PublicKey,
    Endpoint,
    DiagnosticSummary,
    Metrics,
}

impl CopyField {
    pub const fn label(self) -> &'static str {
        match self {
            Self::DeviceId => "id",
            Self::DisplayName => "name",
            Self::Hostname => "hostname",
            Self::Owner => "owner",
            Self::Addresses => "addresses",
            Self::Tags => "tags",
            Self::PublicKey => "public key",
            Self::Endpoint => "endpoint",
            Self::DiagnosticSummary => "diagnostic summary",
            Self::Metrics => "metrics",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Overlay {
    QuitConfirmation,
    TaskInspector(TaskId),
    SortPicker { selected: usize },
    DiagnosticInput(DiagnosticInputState),
    Confirmation(Box<ConfirmationState>),
    OperatorForm(OperatorFormState),
    ServiceForm(ServiceFormState),
    ServiceSectionPicker(ServiceSectionPickerState),
    Appearance(AppearanceState),
    AccountPicker(AccountPickerState),
    HandoffInput(HandoffInputState),
    PolicyEditor,
    SecretResult,
    AuditInvestigation,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ResourceIdentity {
    Device(DeviceId),
    Opaque(String),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ViewFrame {
    pub route: Route,
    pub focus: Focus,
    pub selection: Option<ResourceIdentity>,
    pub scroll_anchor: Option<ResourceIdentity>,
    pub filter_text: String,
    pub filter: FilterExpression,
    pub task_filter: String,
    pub sort: SortSpec,
    pub section: Option<ServiceSection>,
    pub saved_view: Option<String>,
}

impl ViewFrame {
    pub fn new(route: Route) -> Self {
        Self {
            route,
            focus: Focus::Collection,
            selection: None,
            scroll_anchor: None,
            filter_text: String::new(),
            filter: FilterExpression::empty(),
            task_filter: String::new(),
            sort: SortSpec::default(),
            section: None,
            saved_view: None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ViewHistory {
    pub frames: Vec<ViewFrame>,
    pub cursor: usize,
    pub capacity: usize,
}

impl ViewHistory {
    pub fn new(route: Route) -> Self {
        Self {
            frames: vec![ViewFrame::new(route)],
            cursor: 0,
            capacity: 100,
        }
    }

    pub fn current(&self) -> Option<&ViewFrame> {
        self.frames.get(self.cursor)
    }

    fn replace_current(&mut self, frame: ViewFrame) {
        if let Some(current) = self.frames.get_mut(self.cursor) {
            *current = frame;
        }
    }

    pub fn append(&mut self, frame: ViewFrame) -> bool {
        if self.current() == Some(&frame) {
            return false;
        }
        self.frames.truncate(self.cursor.saturating_add(1));
        self.frames.push(frame);
        if self.frames.len() > self.capacity {
            self.frames.remove(0);
        }
        self.cursor = self.frames.len().saturating_sub(1);
        true
    }

    pub fn backward(&mut self) -> Option<ViewFrame> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor = self.cursor.saturating_sub(1);
        self.frames.get(self.cursor).cloned()
    }

    pub fn forward(&mut self) -> Option<ViewFrame> {
        if self.cursor.saturating_add(1) >= self.frames.len() {
            return None;
        }
        self.cursor = self.cursor.saturating_add(1);
        self.frames.get(self.cursor).cloned()
    }
}

#[derive(Debug, Clone)]
pub struct DeviceResource {
    pub snapshot: Vec<Device>,
    pub generation: u64,
    pub observed_at: Option<Timestamp>,
    pub health: SourceHealth,
    pub error: Option<String>,
}

impl DeviceResource {
    pub const fn empty(mode: SourceMode) -> Self {
        Self {
            snapshot: Vec::new(),
            generation: 0,
            observed_at: None,
            health: match mode {
                SourceMode::Mock => SourceHealth::Loading,
                SourceMode::Local => SourceHealth::Loading,
                SourceMode::Unavailable => SourceHealth::Unavailable,
            },
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceViewState {
    pub selected_id: Option<DeviceId>,
    pub scroll: usize,
    pub filter_draft: String,
    pub applied_filter: FilterExpression,
    pub sort: SortSpec,
    pub sort_terms: Vec<SortSpec>,
    pub wide_columns: bool,
    pub columns: Vec<String>,
}

impl Default for DeviceViewState {
    fn default() -> Self {
        Self {
            selected_id: None,
            scroll: 0,
            filter_draft: String::new(),
            applied_filter: FilterExpression::empty(),
            sort: SortSpec::default(),
            sort_terms: vec![SortSpec::default()],
            wide_columns: false,
            columns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct DeviceVisibleCacheKey {
    devices_generation: u64,
    local_generation: u64,
    admin_generation: u64,
    now: Timestamp,
    source_mode: SourceMode,
    filter: FilterExpression,
    sort: SortSpec,
    sort_terms: Vec<SortSpec>,
}

#[derive(Debug, Clone)]
struct DeviceVisibleCache {
    key: DeviceVisibleCacheKey,
    indices: Arc<Vec<usize>>,
}

#[derive(Debug, Clone)]
pub struct Views {
    pub devices: DeviceViewState,
    pub services: ServiceViewState,
}

#[derive(Debug, Clone)]
pub struct ServiceViewState {
    pub section: ServiceSection,
    pub selected: usize,
    pub scroll: usize,
}

impl Default for ServiceViewState {
    fn default() -> Self {
        Self {
            section: ServiceSection::Serve,
            selected: 0,
            scroll: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ShutdownState {
    Running,
    Requested(ShutdownReason),
}

#[derive(Debug)]
pub struct App {
    pub view_history: ViewHistory,
    pub interaction: InteractionMode,
    pub command_history: Vec<String>,
    command_history_cursor: Option<usize>,
    pub focus: Focus,
    pub overlays: Vec<Overlay>,
    pub views: Views,
    pub devices_resource: DeviceResource,
    pub admin: AdminSnapshot,
    pub health: crate::health::HealthState,
    pub health_findings: Vec<Finding>,
    health_evaluation_generation: u64,
    pub flow_snapshot: Option<FlowSnapshot>,
    pub flow_filter: FlowFilter,
    pub flow_generation: FlowGeneration,
    flow_aggregation_generation: u64,
    flow_aggregation_cancellation: Option<Arc<AtomicBool>>,
    pub webhooks: Vec<WebhookEndpoint>,
    pub log_stream_configurations: BTreeMap<LogType, LogStreamConfiguration>,
    pub log_stream_statuses: BTreeMap<LogType, LogStreamStatus>,
    pub access_explorer_result: Option<AccessResult>,
    pub saved_views: Option<crate::saved_views::SavedViewsState>,
    pending_export_fingerprint: Option<[u8; 32]>,
    pub admin_profile_snapshots: BTreeMap<String, AdminSnapshot>,
    pub policy_workflow: Option<PolicyWorkflow>,
    policy_temp_file: Option<Arc<Mutex<crate::temporary::TemporaryPolicyFile>>>,
    latest_policy_temp_file: Option<Arc<Mutex<crate::temporary::TemporaryPolicyFile>>>,
    pub secret_result: Option<SecretResult>,
    next_policy_workflow_id: u64,
    next_secret_result_id: u64,
    pending_auth_key_request: Option<crate::admin::key_mutations::AuthKeyCreateRequest>,
    pending_auth_key_result: Option<u64>,
    pending_credential_revoke: Option<String>,
    admin_environment_token: Option<Arc<SecretValue>>,
    pub admin_user_selected: usize,
    pub admin_route_selected: usize,
    pub admin_credential_selected: usize,
    pub admin_activity_selected: usize,
    pub admin_audit_window_days: u64,
    pub audit_filters: AuditFilters,
    pub task_filter: String,
    pub composed_devices: Vec<ComposedDevice>,
    pub local_resource: LocalResource,
    pub local_preferences_resource: LocalPreferencesResource,
    pub local_state: LocalState,
    pub local_daemon_state: LocalDaemonState,
    pub local_cli_state: LocalCliState,
    pub local_executable: Option<LocalExecutable>,
    pub local_capabilities: LocalCapabilities,
    pub services_snapshot: LocalServicesSnapshot,
    pub alpha_local_features: bool,
    pub certificate_verification: Option<CertificateVerification>,
    pub service_locks: Vec<(ServiceConflictKey, TaskId)>,
    pub local_preferences: LocalPreferences,
    pub local_accounts: Vec<LocalAccount>,
    pub system_policy: Vec<SystemPolicyEntry>,
    pub system_policy_failure: Option<LocalFailure>,
    pub local_diagnostics: BTreeMap<TaskId, DiagnosticState>,
    pub local_self_id: Option<DeviceId>,
    pub tasks: TaskStore,
    pub notifications: Vec<Notification>,
    pub resolved_config: ResolvedConfig,
    /// Immutable visual language used by the next complete frame.
    pub theme: Theme,
    pub shutdown_state: ShutdownState,
    pub source_mode: SourceMode,
    pub terminal_width: u16,
    pub terminal_height: u16,
    pub now: Timestamp,
    pub tick_count: u64,
    pub runtime_error: Option<String>,
    pub copied_value: Option<String>,
    pub mutation_lock: MutationLock,
    pub mutation_in_flight: Option<u64>,
    pub admin_resource_locks: AdminResourceLocks,
    pub admin_mutations_in_flight: BTreeMap<u64, TaskId>,
    pub admin_batch_results: BTreeMap<TaskId, BatchMutation>,
    pub admin_audit_correlations: BTreeMap<u64, AuditCorrelation>,
    pub interactive_handoff_active: bool,
    render_invalidated: bool,
    local_discovery_in_flight: bool,
    local_watcher_connected: bool,
    local_services_refresh_in_flight: bool,
    admin_refresh_in_flight: bool,
    admin_next_refresh: Option<Instant>,
    admin_generation: u64,
    admin_batch_preflights: BTreeMap<u64, PendingAdminBatch>,
    aborted_admin_batch_children: BTreeSet<u64>,
    admin_batches_in_flight: BTreeMap<u64, AdminBatchInFlight>,
    admin_preflight_locks: BTreeSet<u64>,
    admin_read_locks: BTreeMap<String, u64>,
    pending_batch_retry: Option<Vec<BatchTarget>>,
    next_mutation_id: u64,
    device_visible_cache: RefCell<Option<DeviceVisibleCache>>,
}

impl App {
    pub fn new(config: ResolvedConfig) -> Self {
        let theme = Theme::new(config.ui.theme, config.ui.color.capability());
        let source_mode = if config.mock {
            SourceMode::Mock
        } else if config.no_local || !config.local.enabled {
            SourceMode::Unavailable
        } else {
            SourceMode::Local
        };
        let local_state = if config.mock {
            LocalState::Mock
        } else if source_mode == SourceMode::Unavailable {
            LocalState::Disabled
        } else {
            LocalState::DaemonUnavailable {
                detail: "local client discovery pending".to_owned(),
            }
        };
        let local_daemon_state = if config.mock {
            LocalDaemonState::Mock
        } else if source_mode == SourceMode::Unavailable {
            LocalDaemonState::Disabled
        } else {
            LocalDaemonState::Connecting
        };
        let local_cli_state = if config.mock {
            LocalCliState::Mock
        } else if source_mode == SourceMode::Unavailable {
            LocalCliState::Disabled
        } else {
            LocalCliState::Discovering
        };
        let selected_profile = config.profile.clone();
        let (tailnet, profile_read_only) = selected_profile
            .as_deref()
            .and_then(|profile| config.profiles.get(profile))
            .map_or((None, true), |profile| {
                (Some(profile.tailnet.clone()), profile.read_only)
            });
        let admin = AdminSnapshot::new(
            selected_profile,
            tailnet,
            profile_read_only || config.read_only,
            Vec::new(),
        );
        let admin_environment_token = std::env::var("TALE_ACCESS_TOKEN")
            .ok()
            .map(|value| Arc::new(SecretValue::new(value)));
        let saved_views_load = crate::saved_views::SavedViewsState::load(&config.paths.state_dir);
        let (saved_views, saved_views_error) = match saved_views_load {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(format!("saved-view state is invalid: {error}"))),
        };
        Self {
            view_history: ViewHistory::new(Route::Overview),
            interaction: InteractionMode::Normal,
            command_history: Vec::new(),
            command_history_cursor: None,
            focus: Focus::Collection,
            overlays: Vec::new(),
            views: Views {
                devices: DeviceViewState::default(),
                services: ServiceViewState::default(),
            },
            devices_resource: DeviceResource::empty(source_mode),
            admin,
            health: crate::health::HealthState::default(),
            health_findings: Vec::new(),
            health_evaluation_generation: 0,
            flow_snapshot: None,
            flow_filter: FlowFilter::default(),
            flow_generation: FlowGeneration::new(),
            flow_aggregation_generation: 0,
            flow_aggregation_cancellation: None,
            webhooks: Vec::new(),
            log_stream_configurations: BTreeMap::new(),
            log_stream_statuses: BTreeMap::new(),
            access_explorer_result: None,
            saved_views,
            pending_export_fingerprint: None,
            admin_profile_snapshots: BTreeMap::new(),
            policy_workflow: None,
            policy_temp_file: None,
            latest_policy_temp_file: None,
            secret_result: None,
            next_policy_workflow_id: 1,
            next_secret_result_id: 1,
            pending_auth_key_request: None,
            pending_auth_key_result: None,
            pending_credential_revoke: None,
            admin_environment_token,
            admin_user_selected: 0,
            admin_route_selected: 0,
            admin_credential_selected: 0,
            admin_activity_selected: 0,
            admin_audit_window_days: 1,
            audit_filters: AuditFilters::default(),
            task_filter: String::new(),
            composed_devices: Vec::new(),
            local_resource: LocalResource::new(),
            local_preferences_resource: LocalPreferencesResource::new(),
            local_state,
            local_daemon_state,
            local_cli_state,
            local_executable: None,
            local_capabilities: LocalCapabilities::default(),
            services_snapshot: LocalServicesSnapshot::new(),
            alpha_local_features: false,
            certificate_verification: None,
            service_locks: Vec::new(),
            local_preferences: LocalPreferences::empty(0),
            local_accounts: Vec::new(),
            system_policy: Vec::new(),
            system_policy_failure: None,
            local_diagnostics: BTreeMap::new(),
            local_self_id: None,
            tasks: TaskStore::new(),
            notifications: Vec::new(),
            resolved_config: config,
            theme,
            shutdown_state: ShutdownState::Running,
            source_mode,
            terminal_width: 80,
            terminal_height: 24,
            now: if source_mode == SourceMode::Mock {
                MOCK_NOW
            } else {
                crate::local::now()
            },
            tick_count: 0,
            runtime_error: saved_views_error,
            copied_value: None,
            mutation_lock: MutationLock::new(),
            mutation_in_flight: None,
            admin_resource_locks: AdminResourceLocks::new(),
            admin_mutations_in_flight: BTreeMap::new(),
            admin_batch_results: BTreeMap::new(),
            admin_audit_correlations: BTreeMap::new(),
            interactive_handoff_active: false,
            render_invalidated: true,
            local_discovery_in_flight: false,
            local_watcher_connected: false,
            local_services_refresh_in_flight: false,
            admin_refresh_in_flight: false,
            admin_next_refresh: None,
            admin_generation: 0,
            admin_batch_preflights: BTreeMap::new(),
            aborted_admin_batch_children: BTreeSet::new(),
            admin_batches_in_flight: BTreeMap::new(),
            admin_preflight_locks: BTreeSet::new(),
            admin_read_locks: BTreeMap::new(),
            pending_batch_retry: None,
            next_mutation_id: 1,
            device_visible_cache: RefCell::new(None),
        }
    }

    pub fn bootstrap_effects(&mut self) -> Vec<Effect> {
        let mut effects = self.start_admin_refresh();
        match self.source_mode {
            SourceMode::Unavailable => return effects,
            SourceMode::Mock => {
                self.devices_resource.generation = 1;
                effects.push(Effect::StartMockLoad {
                    resource: Resource::Devices,
                    generation: 1,
                    scenario: MockLoadScenario::Initial,
                });
                return effects;
            }
            SourceMode::Local => {}
        }
        self.local_resource.generation = 1;
        self.local_resource.begin(1, self.now);
        self.local_preferences_resource.begin(1, self.now);
        self.local_discovery_in_flight = true;
        effects.push(Effect::StartLocalObservation {
            socket_path: self.resolved_config.local.socket_path.clone(),
            timeout: self.resolved_config.local.command_timeout,
            reconcile_interval: self.resolved_config.local.reconcile_interval,
        });
        effects.push(Effect::StartLocalDiscovery {
            generation: 1,
            resolution: local_resolution(&self.resolved_config),
            timeout: self.resolved_config.local.command_timeout,
        });
        effects
    }

    fn recompute_health(&mut self) -> Vec<Effect> {
        self.health_evaluation_generation = self.health_evaluation_generation.saturating_add(1);
        if self.admin.profile.is_none() {
            self.health.clear();
            self.health_findings.clear();
            return Vec::new();
        }
        let snapshot = crate::health::snapshot_from_admin(
            &self.admin,
            self.now,
            self.resolved_config.admin.refresh_interval.as_secs(),
        );
        vec![Effect::StartHealthEvaluation {
            generation: self.health_evaluation_generation,
            snapshot,
        }]
    }

    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        if !matches!(event, Event::Tick(_)) {
            self.render_invalidated = true;
        }
        if !matches!(self.shutdown_state, ShutdownState::Running)
            && !matches!(event, Event::Task(_) | Event::Tick(_))
        {
            return Vec::new();
        }
        match event {
            Event::Input(input) => self.update_input(input),
            Event::Tick(tick) => self.update_tick(tick),
            Event::Task(task) => self.update_task(*task),
            Event::Source(source) => self.update_source(source),
            Event::Local(local) => self.update_local(*local),
            Event::Services(services) => self.update_services(*services),
            Event::Admin(admin) => self.update_admin(*admin),
            Event::Policy(policy) => self.update_policy(*policy),
            Event::Credential(credential) => self.update_credential(*credential),
            Event::ShutdownRequested(reason) => self.request_shutdown(reason),
        }
    }

    fn update_policy(&mut self, event: PolicyEvent) -> Vec<Effect> {
        self.access_explorer_result = None;
        match event {
            PolicyEvent::RemoteFetched {
                workflow_id,
                result,
                ..
            } => {
                if self
                    .policy_workflow
                    .as_ref()
                    .is_none_or(|workflow| workflow.workflow_id() != workflow_id)
                {
                    return Vec::new();
                }
                let document = match result {
                    Ok(document) => document,
                    Err(detail) => {
                        if let Some(workflow) = self.policy_workflow.as_mut() {
                            workflow.retain_failure();
                        }
                        self.runtime_error = Some(detail);
                        return Vec::new();
                    }
                };
                let start_editor = self
                    .policy_workflow
                    .as_ref()
                    .is_some_and(|workflow| workflow.state() == PolicyState::Opening);
                let base_hash = self
                    .policy_workflow
                    .as_ref()
                    .and_then(PolicyWorkflow::base)
                    .map(|base| base.hash().to_owned());
                let has_candidate = self
                    .policy_workflow
                    .as_ref()
                    .is_some_and(|workflow| workflow.candidate().is_some());
                let remote_changed = base_hash
                    .as_deref()
                    .is_some_and(|hash| document.hash() != hash);
                let edited_candidate = self
                    .policy_workflow
                    .as_ref()
                    .and_then(|workflow| workflow.base().zip(workflow.candidate()))
                    .is_some_and(|(base, candidate)| candidate.hash() != base.hash());
                if has_candidate && remote_changed {
                    self.close_latest_policy_temp_file();
                    let latest_file =
                        match crate::temporary::TemporaryPolicyFile::create(document.bytes()) {
                            Ok(file) => file,
                            Err(error) => {
                                self.runtime_error = Some(error.to_string());
                                return Vec::new();
                            }
                        };
                    let latest_path = latest_file.path().to_path_buf();
                    self.latest_policy_temp_file = Some(Arc::new(Mutex::new(latest_file)));
                    if let Some(workflow) = self.policy_workflow.as_mut() {
                        workflow.set_latest_remote_with_path(document, Some(latest_path));
                    }
                    self.runtime_error = Some(
                        "remote policy changed; candidate and latest remote are retained separately"
                            .to_owned(),
                    );
                    return Vec::new();
                }
                if edited_candidate {
                    self.close_latest_policy_temp_file();
                    if let Some(workflow) = self.policy_workflow.as_mut() {
                        workflow.set_latest_remote(document);
                    }
                    self.runtime_error = Some(
                        "remote policy is unchanged; the edited candidate was retained".to_owned(),
                    );
                    return Vec::new();
                }
                self.close_policy_temp_file();
                self.close_latest_policy_temp_file();
                let file = match crate::temporary::TemporaryPolicyFile::create(document.bytes()) {
                    Ok(file) => file,
                    Err(error) => {
                        if let Some(workflow) = self.policy_workflow.as_mut() {
                            workflow.retain_failure();
                        }
                        self.runtime_error = Some(error.to_string());
                        return Vec::new();
                    }
                };
                let path = file.path().to_path_buf();
                self.policy_temp_file = Some(Arc::new(Mutex::new(file)));
                self.access_explorer_result = None;
                if let Some(workflow) = self.policy_workflow.as_mut() {
                    workflow.set_base(document.clone());
                    workflow.set_candidate(document, path);
                }
                if start_editor {
                    self.start_policy_editor()
                } else {
                    Vec::new()
                }
            }
            PolicyEvent::EditorFinished {
                workflow_id,
                result,
                path,
                editor_success,
                editor_code,
            } => {
                self.interactive_handoff_active = false;
                let mut effects = vec![Effect::ResumeTerminal];
                if self
                    .policy_workflow
                    .as_ref()
                    .is_none_or(|workflow| workflow.workflow_id() != workflow_id)
                {
                    return effects;
                }
                match result {
                    Ok(candidate) => {
                        self.access_explorer_result = None;
                        if let Some(workflow) = self.policy_workflow.as_mut() {
                            workflow.set_candidate(candidate, path.clone());
                            if let Some((base, candidate)) =
                                workflow.base().zip(workflow.candidate())
                                && let Ok(diff) = crate::admin::policy_mutations::build_policy_diff(
                                    base, candidate,
                                )
                            {
                                let _ = workflow.set_diff(diff);
                            }
                        }
                        if !editor_success {
                            self.runtime_error = Some(format!(
                                "external editor returned {}; candidate retained",
                                editor_code
                                    .map_or_else(|| "signal".to_owned(), |value| value.to_string())
                            ));
                        }
                        effects.extend(self.refresh_policy_workflow());
                    }
                    Err(detail) => {
                        self.access_explorer_result = None;
                        if let Some(workflow) = self.policy_workflow.as_mut() {
                            if let Some(base) = workflow.base().cloned() {
                                workflow.set_candidate(base, path);
                            }
                            workflow.retain_failure();
                        }
                        self.runtime_error = Some(detail);
                    }
                }
                effects
            }
            PolicyEvent::Validated {
                workflow_id,
                result,
            } => {
                if let Some(workflow) = self.policy_workflow.as_mut()
                    && workflow.workflow_id() == workflow_id
                {
                    match result {
                        Ok(validation) => {
                            if !workflow.set_validation(validation) {
                                self.runtime_error = Some(
                                    "server validation result was not bound to the current candidate"
                                        .to_owned(),
                                );
                            }
                        }
                        Err(detail) => {
                            workflow.retain_failure();
                            self.runtime_error = Some(detail);
                        }
                    }
                }
                Vec::new()
            }
            PolicyEvent::Previewed {
                workflow_id,
                result,
            } => {
                if let Some(workflow) = self.policy_workflow.as_mut()
                    && workflow.workflow_id() == workflow_id
                {
                    match result {
                        Ok(preview) => {
                            if !workflow.set_preview(preview) {
                                self.runtime_error = Some(
                                    "server permission preview was not bound to the current candidate"
                                        .to_owned(),
                                );
                            }
                        }
                        Err(detail) => {
                            workflow.retain_failure();
                            self.runtime_error = Some(detail);
                        }
                    }
                }
                Vec::new()
            }
            PolicyEvent::Diffed {
                workflow_id,
                result,
            } => {
                if let Some(workflow) = self.policy_workflow.as_mut()
                    && workflow.workflow_id() == workflow_id
                {
                    match result {
                        Ok(diff) => {
                            if !workflow.set_diff(diff) {
                                self.runtime_error = Some(
                                    "policy diff was not bound to the current candidate".to_owned(),
                                );
                            }
                        }
                        Err(detail) => self.runtime_error = Some(detail),
                    }
                }
                Vec::new()
            }
            PolicyEvent::Applied {
                workflow_id,
                result,
            } => {
                if let PolicyApplyResult::RemoteConflict { latest } = &result {
                    let workflow_matches = self
                        .policy_workflow
                        .as_ref()
                        .is_some_and(|workflow| workflow.workflow_id() == workflow_id);
                    if !workflow_matches {
                        return Vec::new();
                    }
                    self.close_latest_policy_temp_file();
                    let latest_path =
                        match crate::temporary::TemporaryPolicyFile::create(latest.bytes()) {
                            Ok(file) => {
                                let path = file.path().to_path_buf();
                                self.latest_policy_temp_file = Some(Arc::new(Mutex::new(file)));
                                Some(path)
                            }
                            Err(error) => {
                                self.runtime_error = Some(error.to_string());
                                None
                            }
                        };
                    if let Some(workflow) = self.policy_workflow.as_mut() {
                        workflow.set_latest_remote_with_path(latest.clone(), latest_path);
                    }
                    self.runtime_error = Some(
                        "remote policy changed; candidate and latest remote retained for review"
                            .to_owned(),
                    );
                    return Vec::new();
                }
                let mut refresh_audit = false;
                if let Some(workflow) = self.policy_workflow.as_mut()
                    && workflow.workflow_id() == workflow_id
                {
                    match result {
                        PolicyApplyResult::Succeeded { saved_hash } => {
                            workflow.mark_verifying();
                            workflow.mark_succeeded();
                            self.runtime_error =
                                Some(format!("policy applied and verified: {saved_hash}"));
                            refresh_audit = true;
                        }
                        PolicyApplyResult::SucceededUnverified { saved_hash } => {
                            workflow.mark_succeeded_unverified();
                            self.runtime_error = Some(format!(
                                "policy save completed; verification unavailable: {saved_hash}"
                            ));
                            refresh_audit = true;
                        }
                        PolicyApplyResult::FailedRetained { detail } => {
                            workflow.retain_failure();
                            self.runtime_error = Some(detail);
                        }
                        PolicyApplyResult::OutcomeUnknown { detail } => {
                            workflow.mark_unknown();
                            self.runtime_error = Some(detail);
                        }
                        PolicyApplyResult::RemoteConflict { .. } => {}
                    }
                }
                if refresh_audit {
                    self.start_admin_resource_refresh(vec![AdminRefreshResource::Activity])
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn update_credential(&mut self, event: CredentialEvent) -> Vec<Effect> {
        match event {
            CredentialEvent::AuthKeyCreated {
                result_id,
                metadata,
                secret,
                observed_at,
            } => {
                if self.pending_auth_key_result != Some(result_id) {
                    return Vec::new();
                }
                self.pending_auth_key_result = None;
                let secret_result = SecretResult::from_handle(
                    SecretMetadata {
                        result_id,
                        credential_id: Some(metadata.id.clone()),
                        credential_type: metadata.key_type.clone(),
                        description: metadata.description.clone(),
                        created_at: observed_at,
                        expires_at: metadata.expires_at,
                        warning: "This secret is view-once. It is not listed, persisted, logged, or recoverable after close.".to_owned(),
                    },
                    secret,
                );
                self.secret_result = Some(secret_result);
                self.overlays.push(Overlay::SecretResult);
            }
            CredentialEvent::AuthKeyCreateFailed { result_id, detail } => {
                if self.pending_auth_key_result == Some(result_id) {
                    self.pending_auth_key_result = None;
                    self.runtime_error = Some(detail);
                }
            }
            CredentialEvent::DetailFetched { key_id, result } => {
                if self.pending_credential_revoke.as_deref() != Some(key_id.as_str()) {
                    return Vec::new();
                }
                self.pending_credential_revoke = None;
                match result {
                    Ok(metadata) => return self.open_credential_revoke_with_metadata(metadata),
                    Err(detail) => self.runtime_error = Some(detail),
                }
            }
            CredentialEvent::Revoked { key_id, result } => {
                if self.pending_credential_revoke.as_deref() != Some(key_id.as_str()) {
                    return Vec::new();
                }
                self.pending_credential_revoke = None;
                match result {
                    CredentialRevocationResult::Verified => {
                        self.runtime_error =
                            Some("remote credential revocation was verified".to_owned());
                        let current_profile = self.admin.profile.clone();
                        let current_reference = self
                            .admin
                            .profile
                            .as_ref()
                            .and_then(|profile| self.resolved_config.profiles.get(profile))
                            .map(|profile| profile.credential.as_str());
                        let clear_current = current_reference == Some(key_id.as_str());
                        let next_profile = current_profile.as_ref().and_then(|current| {
                            self.resolved_config
                                .profiles
                                .iter()
                                .find(|(name, profile)| {
                                    *name != current && profile.credential != key_id
                                })
                                .map(|(name, _)| name.clone())
                        });
                        let effects = self
                            .start_admin_resource_refresh(vec![AdminRefreshResource::Credentials]);
                        if clear_current {
                            if let Some(next_profile) = next_profile {
                                self.runtime_error = Some(format!(
                                    "remote credential revocation was verified; switching to configured profile {next_profile}"
                                ));
                                let mut effects = effects;
                                effects.extend(self.switch_profile(Some(next_profile)));
                                return effects;
                            }
                            let mut effects = effects;
                            effects.extend(self.clear_admin_profile());
                            return effects;
                        }
                        return effects;
                    }
                    CredentialRevocationResult::OutcomeUnknown { detail }
                    | CredentialRevocationResult::Failed { detail } => {
                        self.runtime_error = Some(detail)
                    }
                }
            }
            CredentialEvent::LocalRemoved {
                profile, result, ..
            } => {
                if self.admin.profile.as_deref() != Some(profile.as_str()) {
                    return Vec::new();
                }
                match result {
                    Ok(true) => {
                        self.runtime_error = Some(format!(
                            "removed local Tale credential for profile {profile}"
                        ));
                        return self.clear_admin_profile();
                    }
                    Ok(false) => {
                        self.runtime_error =
                            Some("local Tale credential was not present".to_owned())
                    }
                    Err(detail) => self.runtime_error = Some(detail),
                }
            }
            CredentialEvent::ClipboardCopied { result_id, result } => {
                if self
                    .secret_result
                    .as_ref()
                    .is_none_or(|value| value.metadata().result_id != result_id)
                {
                    return Vec::new();
                }
                match result {
                    Ok(()) => {
                        self.runtime_error = Some(
                            "secret copied explicitly; Tale did not clear the clipboard".to_owned(),
                        )
                    }
                    Err(detail) => self.runtime_error = Some(detail),
                }
            }
            CredentialEvent::ClipboardTextCopied { label, result } => match result {
                Ok(()) => self.copied_value = Some(label),
                Err(detail) => self.runtime_error = Some(detail),
            },
        }
        Vec::new()
    }

    fn update_tick(&mut self, tick: Instant) -> Vec<Effect> {
        self.tick_count = self.tick_count.saturating_add(1);
        self.now = if self.source_mode == SourceMode::Mock {
            MOCK_NOW.saturating_add(self.tick_count)
        } else {
            crate::local::now()
        };
        self.notifications
            .retain(|notification| notification.expires_at > self.now);
        if self.tasks.has_active() {
            self.render_invalidated = true;
        }
        let mut effects = Vec::new();
        if self.admin.profile.is_some()
            && !self.admin_refresh_in_flight
            && self.overlays.is_empty()
            && self.admin_next_refresh.is_some_and(|due| tick >= due)
        {
            effects.extend(self.start_admin_refresh());
        }
        effects
    }

    fn update_input(&mut self, input: InputEvent) -> Vec<Effect> {
        if self.interactive_handoff_active {
            return Vec::new();
        }
        match input {
            InputEvent::Resize { width, height } => {
                self.terminal_width = width;
                self.terminal_height = height;
                Vec::new()
            }
            InputEvent::Mouse(mouse) => self.handle_mouse(mouse),
            InputEvent::Paste(text) => self.handle_paste(&text),
            InputEvent::FocusGained | InputEvent::FocusLost => Vec::new(),
            InputEvent::Key(key) => self.handle_key(key),
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Vec<Effect> {
        if !self.resolved_config.ui.mouse {
            return Vec::new();
        }
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            let layout = crate::ui::layout::compute(
                ratatui::layout::Rect {
                    x: 0,
                    y: 0,
                    width: self.terminal_width,
                    height: self.terminal_height,
                },
                self,
            );
            if !matches!(self.interaction, InteractionMode::Normal) {
                return self.handle_interaction_mouse(mouse, layout.footer);
            }
            if self.resolved_config.ui.show_footer
                && contains_point(layout.footer, mouse.column, mouse.row)
            {
                let context = self.action_context();
                let mut x = layout.footer.x;
                for (action_id, hint) in action::footer_actions(context, layout.footer.width) {
                    let end = x.saturating_add(u16::try_from(hint.len()).map_or(u16::MAX, |v| v));
                    if mouse.column >= x && mouse.column < end {
                        return action_id.map_or_else(Vec::new, |id| self.dispatch_action(id));
                    }
                    x = end.saturating_add(2);
                }
                return Vec::new();
            }
        }
        let action = match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if self.focus == Focus::Collection
                    && self.mouse_in_scrollable_collection(mouse.column, mouse.row) =>
            {
                match mouse.kind {
                    MouseEventKind::ScrollUp => Some(ActionId::CollectionMoveUp),
                    MouseEventKind::ScrollDown => Some(ActionId::CollectionMoveDown),
                    _ => None,
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.focus_mouse_region(mouse.column, mouse.row);
                None
            }
            _ => None,
        };
        action.map_or_else(Vec::new, |action_id| {
            if self.action_available_for_id(action_id) {
                self.dispatch_action(action_id)
            } else {
                Vec::new()
            }
        })
    }

    fn handle_interaction_mouse(
        &mut self,
        mouse: MouseEvent,
        area: ratatui::layout::Rect,
    ) -> Vec<Effect> {
        if !contains_point(area, mouse.column, mouse.row) {
            return self.handle_interaction_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        let candidate_index = usize::from(mouse.row.saturating_sub(area.y));
        match &mut self.interaction {
            InteractionMode::CommandLine(state)
                if candidate_index < state.candidates.len().min(6) =>
            {
                if let Some(candidate) = state.candidates.get(candidate_index) {
                    apply_completion(&mut state.editor, candidate);
                    state.selected_completion = Some(candidate_index);
                }
            }
            InteractionMode::FilterLine(state)
                if candidate_index < state.candidates.len().min(6) =>
            {
                if let Some(candidate) = state.candidates.get(candidate_index) {
                    apply_completion(&mut state.editor, candidate);
                    state.selected_completion = Some(candidate_index);
                }
                return self.update_live_filter();
            }
            InteractionMode::Transient(state) => {
                let keys = transient_click_keys(state);
                let mut x = area.x;
                for (key, label) in keys {
                    let end = x.saturating_add(u16::try_from(label.len()).map_or(u16::MAX, |v| v));
                    if mouse.column >= x && mouse.column < end {
                        return self.handle_transient_key(KeyEvent::new(
                            KeyCode::Char(key),
                            KeyModifiers::NONE,
                        ));
                    }
                    x = end.saturating_add(2);
                }
            }
            InteractionMode::Normal
            | InteractionMode::HelpSheet(_)
            | InteractionMode::CommandLine(_)
            | InteractionMode::FilterLine(_) => {}
        }
        Vec::new()
    }

    fn action_context(&self) -> ActionContext {
        match self.current_route() {
            Route::Activity => ActionContext::Activity,
            Route::Devices | Route::Services if self.focus == Focus::Inspector => {
                ActionContext::Detail
            }
            Route::Devices
            | Route::Users
            | Route::Routes
            | Route::Credentials
            | Route::Services => ActionContext::Collection,
            _ => ActionContext::Root,
        }
    }

    fn focus_mouse_region(&mut self, column: u16, row: u16) {
        if self.current_route() == Route::Activity {
            self.focus = Focus::Collection;
            let Some(collection) = self.activity_task_area() else {
                return;
            };
            if !contains_point(collection, column, row) {
                return;
            }
            let first_row = collection.y.saturating_add(1);
            let row_count = usize::from(collection.height.saturating_sub(2));
            if row >= first_row && usize::from(row.saturating_sub(first_row)) < row_count {
                self.tasks.select_filtered_position(
                    &self.task_filter,
                    usize::from(row.saturating_sub(first_row)),
                );
            }
            return;
        }
        if self.current_route() != Route::Devices {
            let frame = crate::ui::layout::compute(
                ratatui::layout::Rect {
                    x: 0,
                    y: 0,
                    width: self.terminal_width,
                    height: self.terminal_height,
                },
                self,
            );
            if frame.minimum {
                return;
            }
            if self.current_route() == Route::Services
                && frame
                    .inspector
                    .is_some_and(|inspector| contains_point(inspector, column, row))
            {
                self.focus = Focus::Inspector;
                return;
            }
            self.focus = Focus::Collection;
            let area = frame
                .inspector
                .map_or(frame.content, |inspector| ratatui::layout::Rect {
                    x: frame.content.x,
                    y: frame.content.y,
                    width: inspector.x.saturating_sub(frame.content.x),
                    height: frame.content.height,
                });
            if !contains_point(area, column, row) {
                return;
            }
            let first_row = match self.current_route() {
                Route::Users | Route::Credentials => area.y.saturating_add(1),
                Route::Routes => area.y.saturating_add(2),
                Route::Services => area.y.saturating_add(3),
                _ => return,
            };
            if row < first_row {
                return;
            }
            let position = row.saturating_sub(first_row);
            match self.current_route() {
                Route::Users => {
                    let length = self.admin.users.snapshot.as_ref().map_or(0, Vec::len);
                    if usize::from(position) < length {
                        self.admin_user_selected = usize::from(position);
                    }
                }
                Route::Routes => {
                    let length = self.admin.route_observations().len();
                    if usize::from(position) < length {
                        self.admin_route_selected = usize::from(position);
                    }
                }
                Route::Credentials => {
                    let length = self
                        .admin
                        .credentials
                        .snapshot
                        .as_ref()
                        .map_or(0, |snapshot| snapshot.records.len());
                    if usize::from(position) < length {
                        self.admin_credential_selected = usize::from(position);
                    }
                }
                Route::Services if usize::from(position) < self.service_row_count() => {
                    self.views.services.selected = usize::from(position);
                }
                _ => {}
            }
            return;
        }
        let frame = crate::ui::layout::compute(
            ratatui::layout::Rect {
                x: 0,
                y: 0,
                width: self.terminal_width,
                height: self.terminal_height,
            },
            self,
        );
        if let Some(inspector) = frame.inspector
            && contains_point(inspector, column, row)
        {
            self.focus = Focus::Inspector;
            return;
        }
        let Some(collection) = self.device_collection_area(frame) else {
            self.focus = Focus::Collection;
            return;
        };
        if !contains_point(collection, column, row) {
            self.focus = Focus::Collection;
            return;
        }
        self.focus = Focus::Collection;
        let first_row = collection.y.saturating_add(2);
        let row_count = usize::from(collection.height.saturating_sub(3));
        if row >= first_row && usize::from(row.saturating_sub(first_row)) < row_count {
            let position = self
                .views
                .devices
                .scroll
                .saturating_add(usize::from(row.saturating_sub(first_row)));
            self.move_selection_to(position);
        }
    }

    fn mouse_in_scrollable_collection(&self, column: u16, row: u16) -> bool {
        if self.current_route() == Route::Devices {
            let frame = crate::ui::layout::compute(
                ratatui::layout::Rect {
                    x: 0,
                    y: 0,
                    width: self.terminal_width,
                    height: self.terminal_height,
                },
                self,
            );
            return self
                .device_collection_area(frame)
                .is_some_and(|area| contains_point(area, column, row));
        }
        if self.current_route() == Route::Activity {
            return self
                .activity_task_area()
                .is_some_and(|area| contains_point(area, column, row));
        }
        if !matches!(
            self.current_route(),
            Route::Users | Route::Routes | Route::Credentials | Route::Services
        ) {
            return false;
        }
        let frame = crate::ui::layout::compute(
            ratatui::layout::Rect {
                x: 0,
                y: 0,
                width: self.terminal_width,
                height: self.terminal_height,
            },
            self,
        );
        if frame.minimum {
            return false;
        }
        let area = frame
            .inspector
            .map_or(frame.content, |inspector| ratatui::layout::Rect {
                x: frame.content.x,
                y: frame.content.y,
                width: inspector.x.saturating_sub(frame.content.x),
                height: frame.content.height,
            });
        contains_point(area, column, row)
    }

    fn activity_task_area(&self) -> Option<ratatui::layout::Rect> {
        let frame = crate::ui::layout::compute(
            ratatui::layout::Rect {
                x: 0,
                y: 0,
                width: self.terminal_width,
                height: self.terminal_height,
            },
            self,
        );
        if frame.minimum {
            return None;
        }
        let regions = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([
                ratatui::layout::Constraint::Percentage(60),
                ratatui::layout::Constraint::Percentage(40),
            ])
            .split(frame.content);
        regions.first().copied()
    }

    fn device_collection_area(
        &self,
        frame: crate::ui::layout::FrameLayout,
    ) -> Option<ratatui::layout::Rect> {
        if frame.minimum || self.focus == Focus::Inspector {
            return None;
        }
        Some(match frame.inspector {
            Some(inspector) => ratatui::layout::Rect {
                x: frame.content.x,
                y: frame.content.y,
                width: inspector.x.saturating_sub(frame.content.x),
                height: frame.content.height,
            },
            None => frame.content,
        })
    }

    fn handle_paste(&mut self, text: &str) -> Vec<Effect> {
        match &mut self.interaction {
            InteractionMode::CommandLine(state) => {
                insert_text(&mut state.editor, text);
                state.error = None;
                self.refresh_command_completions();
                return Vec::new();
            }
            InteractionMode::FilterLine(state) => {
                insert_text(&mut state.editor, text);
                return self.update_live_filter();
            }
            InteractionMode::HelpSheet(state) if state.filtering => {
                state.query.push_str(text);
                return Vec::new();
            }
            _ => {}
        }
        let Some(overlay) = self.overlays.last_mut() else {
            return Vec::new();
        };
        match overlay {
            Overlay::DiagnosticInput(state) => state.input.push_str(text),
            Overlay::OperatorForm(state) => {
                if state.secret_editing {
                    if let Some(secret) = state.secret_input.as_mut() {
                        secret.push_str(text);
                    }
                } else {
                    state.append_ordered_text(text);
                }
                state.error = None;
            }
            Overlay::ServiceForm(state) => {
                state.input.push_str(text);
                state.error = None;
            }
            Overlay::HandoffInput(state) => {
                state.input.push_str(text);
                state.error = None;
            }
            Overlay::Confirmation(state) => {
                state.input.push_str(text);
                state.error = None;
            }
            Overlay::Appearance(_) => {}
            _ => {}
        }
        Vec::new()
    }

    fn handle_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if let Some(effect) = self.handle_text_key(key) {
            return effect;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if !self.overlays.is_empty() {
                return self.pop_overlay();
            }
            if !matches!(self.interaction, InteractionMode::Normal) {
                return self
                    .handle_interaction_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            }
            let effects = self.cancel_focused_task();
            if !effects.is_empty() {
                return effects;
            }
            return self.request_shutdown(ShutdownReason::UserQuit);
        }
        if !self.overlays.is_empty() {
            if key.code == KeyCode::Esc {
                return self.pop_overlay();
            }
            return self.handle_overlay_key(key);
        }
        if !matches!(self.interaction, InteractionMode::Normal) {
            return self.handle_interaction_key(key);
        }
        if key.code == KeyCode::Char('q') && key.modifiers.is_empty() {
            return self.handle_quit_key();
        }
        if key.code == KeyCode::Esc {
            return Vec::new();
        }

        let context = match self.current_route() {
            Route::Activity => ActionContext::Activity,
            Route::Devices if self.focus == Focus::Inspector => ActionContext::Detail,
            Route::Devices => ActionContext::Collection,
            Route::Users | Route::Routes | Route::Credentials => ActionContext::Collection,
            Route::Services if self.focus == Focus::Inspector => ActionContext::Detail,
            Route::Services => ActionContext::Collection,
            _ => ActionContext::Root,
        };
        let Some(action_id) = action::action_for_key(key, context) else {
            return Vec::new();
        };
        self.dispatch_action(action_id)
    }

    fn handle_interaction_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match &self.interaction {
            InteractionMode::CommandLine(_) => self.handle_command_line_key(key),
            InteractionMode::FilterLine(_) => self.handle_filter_line_key(key),
            InteractionMode::Transient(_) => self.handle_transient_key(key),
            InteractionMode::HelpSheet(_) => self.handle_help_sheet_key(key),
            InteractionMode::Normal => Vec::new(),
        }
    }

    fn handle_command_line_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Esc {
            self.interaction = InteractionMode::Normal;
            self.command_history_cursor = None;
            return Vec::new();
        }
        if key.code == KeyCode::Enter {
            let input = match &self.interaction {
                InteractionMode::CommandLine(state) => state.editor.input.clone(),
                _ => String::new(),
            };
            return self.accept_command(&input);
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.complete_command(key.code == KeyCode::BackTab);
            return Vec::new();
        }
        if matches!(key.code, KeyCode::Up | KeyCode::Down) {
            self.recall_command(key.code == KeyCode::Up);
            return Vec::new();
        }
        let edited = if let InteractionMode::CommandLine(state) = &mut self.interaction {
            edit_line(&mut state.editor, key)
        } else {
            false
        };
        if edited {
            if let InteractionMode::CommandLine(state) = &mut self.interaction {
                state.error = None;
                state.selected_completion = None;
            }
            self.command_history_cursor = None;
            self.refresh_command_completions();
        }
        Vec::new()
    }

    fn handle_filter_line_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Esc {
            let restoration = match &self.interaction {
                InteractionMode::FilterLine(state) => Some(state.restoration.clone()),
                _ => None,
            };
            if let Some(restoration) = restoration {
                if self.current_route() == Route::Activity {
                    self.task_filter = restoration.task_filter;
                    self.tasks.selected = restoration.task_selection;
                } else {
                    self.views.devices.filter_draft = restoration.input;
                    self.views.devices.applied_filter = restoration.expression;
                    self.views.devices.selected_id = restoration.selection;
                    self.views.devices.scroll = restoration.scroll;
                    self.reconcile_selection(None);
                }
            }
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if key.code == KeyCode::Enter {
            let (input, valid) = match &self.interaction {
                InteractionMode::FilterLine(state) => {
                    (state.editor.input.clone(), state.error.is_none())
                }
                _ => (String::new(), false),
            };
            if valid {
                return self.accept_filter(&input);
            }
            return Vec::new();
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.complete_filter(key.code == KeyCode::BackTab);
            return self.update_live_filter();
        }
        let edited = if let InteractionMode::FilterLine(state) = &mut self.interaction {
            edit_line(&mut state.editor, key)
        } else {
            false
        };
        if edited {
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.selected_completion = None;
            }
            return self.update_live_filter();
        }
        Vec::new()
    }

    fn handle_transient_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Esc {
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if key.code == KeyCode::Char('?') && key.modifiers.is_empty() {
            self.interaction = InteractionMode::HelpSheet(HelpSheetState {
                query: String::new(),
                filtering: false,
                scroll: 0,
            });
            return Vec::new();
        }
        let KeyCode::Char(character) = key.code else {
            return Vec::new();
        };
        if !key.modifiers.is_empty() {
            return Vec::new();
        }
        let (kind, actions, fields, prefix) = match &self.interaction {
            InteractionMode::Transient(state) => (
                state.kind.clone(),
                state.actions.clone(),
                state.fields.clone(),
                state.prefix,
            ),
            _ => return Vec::new(),
        };
        let mut sequence = String::new();
        if let Some(prefix) = prefix {
            sequence.push(prefix);
        }
        sequence.push(character);
        match kind {
            TransientKind::Action => {
                if let Some(action_id) = actions.iter().copied().find(|id| {
                    action::transient_sequence(*id).is_some_and(|value| value == sequence)
                }) {
                    if let Some(reason) = self.action_unavailable_reason(action_id) {
                        if let InteractionMode::Transient(state) = &mut self.interaction {
                            state.message = Some(reason);
                        }
                        return Vec::new();
                    }
                    self.interaction = InteractionMode::Normal;
                    return self.dispatch_action(action_id);
                }
                if prefix.is_none()
                    && actions.iter().any(|id| {
                        action::transient_sequence(*id)
                            .is_some_and(|value| value.len() == 2 && value.starts_with(character))
                    })
                {
                    if let InteractionMode::Transient(state) = &mut self.interaction {
                        state.prefix = Some(character);
                        state.message = None;
                    }
                    return Vec::new();
                }
            }
            TransientKind::Copy => {
                if let Some(field) = fields
                    .iter()
                    .copied()
                    .find(|field| copy_field_key(*field) == character)
                {
                    let effects = self.copy_field(field);
                    self.interaction = InteractionMode::Normal;
                    return effects;
                }
            }
        }
        if let InteractionMode::Transient(state) = &mut self.interaction {
            state.message = Some(format!("unknown key: {sequence}"));
        }
        Vec::new()
    }

    fn handle_help_sheet_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        let InteractionMode::HelpSheet(state) = &mut self.interaction else {
            return Vec::new();
        };
        match key.code {
            KeyCode::Char('/') if !state.filtering => state.filtering = true,
            KeyCode::Char(character) if state.filtering && key.modifiers.is_empty() => {
                state.query.push(character);
            }
            KeyCode::Backspace if state.filtering => {
                let _ = state.query.pop();
            }
            KeyCode::Char('j') | KeyCode::Down => state.scroll = state.scroll.saturating_add(1),
            KeyCode::Char('k') | KeyCode::Up => state.scroll = state.scroll.saturating_sub(1),
            KeyCode::PageDown => state.scroll = state.scroll.saturating_add(8),
            KeyCode::PageUp => state.scroll = state.scroll.saturating_sub(8),
            _ => {}
        }
        Vec::new()
    }

    fn update_live_filter(&mut self) -> Vec<Effect> {
        let input = match &self.interaction {
            InteractionMode::FilterLine(state) => state.editor.input.clone(),
            _ => return Vec::new(),
        };
        if self.current_route() == Route::Activity {
            self.task_filter = input;
            self.tasks.select_filtered_first(&self.task_filter);
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
                state.candidates.clear();
            }
            return Vec::new();
        }
        match filter::parse(&input) {
            Ok(expression) => {
                self.views.devices.filter_draft = input.clone();
                self.views.devices.applied_filter = expression;
                self.reconcile_selection(None);
                let candidates = self.filter_candidates(&input);
                if let InteractionMode::FilterLine(state) = &mut self.interaction {
                    state.error = None;
                    state.candidates = candidates;
                }
            }
            Err(error) => {
                let candidates = self.filter_candidates(&input);
                if let InteractionMode::FilterLine(state) = &mut self.interaction {
                    state.error = Some(error.to_string());
                    state.candidates = candidates;
                }
            }
        }
        Vec::new()
    }

    fn command_candidates(&self, input: &str) -> Vec<CompletionCandidate> {
        if let Some((route, filter)) = input.split_once(' ')
            && Route::parse(route) == Some(Route::Devices)
        {
            return self.filter_candidates(filter);
        }
        let fragment = input.split_whitespace().next().map_or("", |value| value);
        let mut candidates = route_completion_catalog()
            .into_iter()
            .filter(|candidate| completion_matches(&candidate.insertion, fragment))
            .collect::<Vec<_>>();
        if let Some(name_fragment) = fragment.strip_prefix("view:") {
            candidates = self
                .saved_views
                .as_ref()
                .map_or_else(Vec::new, crate::saved_views::SavedViewsState::names)
                .into_iter()
                .filter(|name| completion_matches(name, name_fragment))
                .take(100)
                .map(|name| CompletionCandidate {
                    id: format!("saved:{name}"),
                    insertion: format!("view:{name}"),
                    label: format!("view:{name}"),
                    description: "Saved view".to_owned(),
                    alias: false,
                })
                .collect();
        }
        sort_completion_candidates(&mut candidates, fragment);
        candidates.truncate(100);
        candidates
    }

    fn filter_candidates(&self, input: &str) -> Vec<CompletionCandidate> {
        let fragment = input.split_whitespace().last().map_or("", |value| value);
        let schema = filter::device_schema();
        let mut values = BTreeSet::new();
        if let Some((field, value_fragment)) = fragment.split_once(':') {
            match field.to_ascii_lowercase().as_str() {
                "online" => values.extend(["true".to_owned(), "false".to_owned()]),
                "os" => values.extend(
                    self.devices_resource
                        .snapshot
                        .iter()
                        .map(|device| device.os.label().to_owned()),
                ),
                "owner" => values.extend(
                    self.devices_resource
                        .snapshot
                        .iter()
                        .filter_map(|device| device.owner.clone()),
                ),
                "tag" => values.extend(
                    self.devices_resource
                        .snapshot
                        .iter()
                        .flat_map(|device| device.tags.iter().cloned()),
                ),
                _ => {}
            }
            return values
                .into_iter()
                .filter(|value| completion_matches(value, value_fragment))
                .take(100)
                .map(|value| CompletionCandidate {
                    id: format!("value:{field}:{value}"),
                    insertion: format!("{field}:{value}"),
                    label: value,
                    description: format!("Value for {field}"),
                    alias: false,
                })
                .collect();
        }
        let mut candidates = schema
            .fields
            .iter()
            .filter(|spec| {
                completion_matches(spec.canonical_name, fragment)
                    || spec
                        .aliases
                        .iter()
                        .any(|alias| completion_matches(alias, fragment))
            })
            .map(|spec| CompletionCandidate {
                id: format!("field:{}", spec.canonical_name),
                insertion: format!("{}:", spec.canonical_name),
                label: format!("{}:", spec.canonical_name),
                description: spec.description.to_owned(),
                alias: false,
            })
            .collect::<Vec<_>>();
        sort_completion_candidates(&mut candidates, fragment);
        candidates
    }

    fn refresh_command_completions(&mut self) {
        let input = match &self.interaction {
            InteractionMode::CommandLine(state) => state.editor.input.clone(),
            _ => return,
        };
        let candidates = self.command_candidates(&input);
        if let InteractionMode::CommandLine(state) = &mut self.interaction {
            state.candidates = candidates;
        }
    }

    fn complete_command(&mut self, reverse: bool) {
        let InteractionMode::CommandLine(state) = &mut self.interaction else {
            return;
        };
        cycle_completion(
            &mut state.editor,
            &state.candidates,
            &mut state.selected_completion,
            reverse,
            true,
        );
    }

    fn complete_filter(&mut self, reverse: bool) {
        let InteractionMode::FilterLine(state) = &mut self.interaction else {
            return;
        };
        cycle_completion(
            &mut state.editor,
            &state.candidates,
            &mut state.selected_completion,
            reverse,
            false,
        );
    }

    fn recall_command(&mut self, older: bool) {
        if self.command_history.is_empty() {
            return;
        }
        let last = self.command_history.len().saturating_sub(1);
        let next = match (self.command_history_cursor, older) {
            (None, true) => last,
            (Some(current), true) => current.saturating_sub(1),
            (Some(current), false) => current.saturating_add(1).min(last),
            (None, false) => return,
        };
        self.command_history_cursor = Some(next);
        if let Some(input) = self.command_history.get(next).cloned()
            && let InteractionMode::CommandLine(state) = &mut self.interaction
        {
            state.editor = LineEditorState::new(input);
            state.selected_completion = None;
            state.error = None;
        }
        self.refresh_command_completions();
    }

    fn handle_text_key(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        let overlay = self.overlays.last_mut()?;
        match overlay {
            Overlay::ServiceForm(state) => {
                match key.code {
                    KeyCode::Char(character) if key.modifiers.is_empty() => {
                        state.input.push(character);
                        state.error = None;
                    }
                    KeyCode::Backspace => {
                        let _ = state.input.pop();
                        state.error = None;
                    }
                    KeyCode::Enter => {
                        let state = state.clone();
                        return Some(self.accept_service_form(state));
                    }
                    _ => return None,
                }
                Some(Vec::new())
            }
            Overlay::DiagnosticInput(state) => {
                match key.code {
                    KeyCode::Char(character) if key.modifiers.is_empty() => {
                        state.input.push(character);
                        state.error = None;
                    }
                    KeyCode::Backspace => {
                        let _ = state.input.pop();
                        state.error = None;
                    }
                    KeyCode::Enter => {
                        let input = state.input.clone();
                        let kind = state.kind.clone();
                        return Some(self.accept_diagnostic_input(kind, &input));
                    }
                    _ => return None,
                }
                Some(Vec::new())
            }
            Overlay::OperatorForm(state) => {
                if key.code == KeyCode::Char('s')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && state.secret_input.is_some()
                {
                    state.secret_editing = !state.secret_editing;
                    state.error = None;
                    return Some(Vec::new());
                }
                if state.ordered_items.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Up, modifiers) if modifiers.is_empty() => {
                            state.select_ordered(-1);
                            return Some(Vec::new());
                        }
                        (KeyCode::Down, modifiers) if modifiers.is_empty() => {
                            state.select_ordered(1);
                            return Some(Vec::new());
                        }
                        (KeyCode::Up, modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                            state.move_ordered_item(-1);
                            return Some(Vec::new());
                        }
                        (KeyCode::Down, modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                            state.move_ordered_item(1);
                            return Some(Vec::new());
                        }
                        (KeyCode::Char('i'), modifiers)
                            if modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            state.insert_ordered_item();
                            return Some(Vec::new());
                        }
                        (KeyCode::Char('x'), modifiers)
                            if modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            state.remove_ordered_item();
                            return Some(Vec::new());
                        }
                        _ => {}
                    }
                }
                match key.code {
                    KeyCode::Char(character) if key.modifiers.is_empty() => {
                        if state.secret_editing {
                            if let Some(secret) = state.secret_input.as_mut() {
                                secret.push(character);
                            }
                        } else if state.ordered_items.is_some() {
                            state.ordered_editor.push(character);
                            state.sync_ordered_input();
                        } else {
                            state.input.push(character);
                        }
                        state.error = None;
                    }
                    KeyCode::Backspace => {
                        if state.secret_editing {
                            if let Some(secret) = state.secret_input.as_mut() {
                                secret.pop();
                            }
                        } else if state.ordered_items.is_some() {
                            let _ = state.ordered_editor.pop();
                            state.sync_ordered_input();
                        } else {
                            let _ = state.input.pop();
                        }
                        state.error = None;
                    }
                    KeyCode::Enter => {
                        state.sync_ordered_input();
                        let state = state.clone();
                        return Some(self.accept_operator_form(state));
                    }
                    _ => return None,
                }
                Some(Vec::new())
            }
            Overlay::HandoffInput(state) => {
                match key.code {
                    KeyCode::Char(character) if key.modifiers.is_empty() => {
                        state.input.push(character);
                        state.error = None;
                    }
                    KeyCode::Backspace => {
                        let _ = state.input.pop();
                        state.error = None;
                    }
                    KeyCode::Enter => {
                        let state = state.clone();
                        return Some(self.accept_handoff_input(state));
                    }
                    _ => return None,
                }
                Some(Vec::new())
            }
            Overlay::Confirmation(state) => {
                match key.code {
                    KeyCode::Char(character) if key.modifiers.is_empty() => {
                        state.input.push(character);
                        state.error = None;
                    }
                    KeyCode::Backspace => {
                        let _ = state.input.pop();
                        state.error = None;
                    }
                    KeyCode::Enter => {
                        let state = (**state).clone();
                        return Some(self.accept_confirmation(state));
                    }
                    _ => return None,
                }
                Some(Vec::new())
            }
            _ => None,
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let Some(overlay) = self.overlays.pop() else {
            return Vec::new();
        };
        match overlay {
            Overlay::QuitConfirmation => {
                if key.code == KeyCode::Enter
                    || (key.code == KeyCode::Char('y') && key.modifiers.is_empty())
                {
                    return self.request_shutdown(ShutdownReason::UserQuit);
                }
                if key.code == KeyCode::Char('n') && key.modifiers.is_empty() {
                    return Vec::new();
                }
                self.overlays.push(Overlay::QuitConfirmation);
                Vec::new()
            }
            Overlay::TaskInspector(task_id) => {
                self.overlays.push(Overlay::TaskInspector(task_id));
                Vec::new()
            }
            Overlay::SortPicker { mut selected } => {
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => selected = (selected + 1).min(19),
                    KeyCode::Char('k') | KeyCode::Up => selected = selected.saturating_sub(1),
                    KeyCode::Enter => {
                        let value = selected;
                        self.apply_sort_choice(value);
                        return Vec::new();
                    }
                    _ => {}
                }
                self.overlays.push(Overlay::SortPicker { selected });
                Vec::new()
            }
            Overlay::DiagnosticInput(state) => {
                self.overlays.push(Overlay::DiagnosticInput(state));
                Vec::new()
            }
            Overlay::OperatorForm(state) => {
                self.overlays.push(Overlay::OperatorForm(state));
                Vec::new()
            }
            Overlay::ServiceForm(state) => {
                self.overlays.push(Overlay::ServiceForm(state));
                Vec::new()
            }
            Overlay::ServiceSectionPicker(mut state) => {
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        state.selected =
                            (state.selected + 1).min(ServiceSection::ALL.len().saturating_sub(1));
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        state.selected = state.selected.saturating_sub(1);
                    }
                    KeyCode::Enter => {
                        self.views.services.section = ServiceSection::from_index(state.selected);
                        self.views.services.selected = 0;
                        return Vec::new();
                    }
                    _ => {}
                }
                self.overlays.push(Overlay::ServiceSectionPicker(state));
                Vec::new()
            }
            Overlay::Appearance(mut state) => {
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        let current = ThemeId::ALL
                            .iter()
                            .position(|id| *id == state.selected)
                            .map_or(0, |index| index);
                        state.selected =
                            ThemeId::ALL[(current + 1).min(ThemeId::ALL.len().saturating_sub(1))];
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        let current = ThemeId::ALL
                            .iter()
                            .position(|id| *id == state.selected)
                            .map_or(0, |index| index);
                        state.selected = ThemeId::ALL[current.saturating_sub(1)];
                    }
                    KeyCode::Enter => return Vec::new(),
                    _ => {}
                }
                self.theme = Theme::new(state.selected, self.theme.capability());
                self.overlays.push(Overlay::Appearance(state));
                Vec::new()
            }
            Overlay::HandoffInput(state) => {
                self.overlays.push(Overlay::HandoffInput(state));
                Vec::new()
            }
            Overlay::Confirmation(mut state) => {
                if key.code == KeyCode::Tab
                    && state.mutation.as_ref().is_some_and(|mutation| {
                        matches!(mutation, LocalMutation::Disconnect { .. })
                    })
                {
                    state.lose_ssh_checked = !state.lose_ssh_checked;
                    if state.lose_ssh_checked {
                        state.required_phrase = Some("LOSE-SSH".to_owned());
                    } else {
                        state.required_phrase = Some("DISCONNECT".to_owned());
                    }
                }
                self.overlays.push(Overlay::Confirmation(state));
                Vec::new()
            }
            Overlay::AccountPicker(mut state) => {
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        state.selected =
                            (state.selected + 1).min(state.accounts.len().saturating_sub(1));
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        state.selected = state.selected.saturating_sub(1);
                    }
                    KeyCode::Enter => return self.accept_account_picker(state),
                    _ => {}
                }
                self.overlays.push(Overlay::AccountPicker(state));
                Vec::new()
            }
            Overlay::PolicyEditor => {
                if key.code == KeyCode::Char('e') && key.modifiers.is_empty() {
                    return self.reopen_policy_editor();
                }
                if key.code == KeyCode::Char('v') && key.modifiers.is_empty() {
                    return self.validate_policy_candidate();
                }
                if key.code == KeyCode::Char('p') && key.modifiers.is_empty() {
                    return self.preview_policy_candidate();
                }
                if key.code == KeyCode::Char('d') && key.modifiers.is_empty() {
                    return self.diff_policy_candidate();
                }
                if key.code == KeyCode::Char('a') && key.modifiers.is_empty() {
                    return self.open_policy_apply_confirmation();
                }
                if key.code == KeyCode::Char('x') && key.modifiers.is_empty() {
                    return self.open_policy_discard_confirmation();
                }
                self.overlays.push(Overlay::PolicyEditor);
                Vec::new()
            }
            Overlay::SecretResult => {
                if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('c'))
                    && key.modifiers.is_empty()
                {
                    let effects = self.copy_secret_result();
                    self.overlays.push(Overlay::SecretResult);
                    return effects;
                }
                self.overlays.push(Overlay::SecretResult);
                Vec::new()
            }
            Overlay::AuditInvestigation => {
                self.overlays.push(Overlay::AuditInvestigation);
                Vec::new()
            }
        }
    }

    fn handle_quit_key(&mut self) -> Vec<Effect> {
        if self.tasks.has_active() {
            self.overlays.push(Overlay::QuitConfirmation);
            Vec::new()
        } else {
            self.request_shutdown(ShutdownReason::UserQuit)
        }
    }

    fn pop_overlay(&mut self) -> Vec<Effect> {
        if let Some(overlay) = self.overlays.pop() {
            if let Overlay::Appearance(state) = overlay {
                self.theme = state.original;
                return Vec::new();
            }
            if matches!(overlay, Overlay::SecretResult) {
                return self.close_secret_result();
            }
            let confirmation_action = match &overlay {
                Overlay::Confirmation(state) => Some(state.action_id),
                _ => None,
            };
            if confirmation_action == Some(ActionId::CollectionExport) {
                self.pending_export_fingerprint = None;
            }
            if confirmation_action == Some(ActionId::AdminCredentialAuthKeyCreate) {
                self.pending_auth_key_request = None;
            }
            if confirmation_action == Some(ActionId::AdminCredentialRevoke) {
                self.pending_credential_revoke = None;
            }
            return Vec::new();
        }
        Vec::new()
    }

    fn accept_command(&mut self, input: &str) -> Vec<Effect> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if let Some(name) = trimmed.strip_prefix("view:") {
            let name = name.trim();
            let known = self
                .saved_views
                .as_ref()
                .is_some_and(|saved_views| saved_views.store.apply(name).is_ok());
            if !known {
                if let InteractionMode::CommandLine(state) = &mut self.interaction {
                    state.error = Some(format!("unknown saved view: {name}"));
                }
                return Vec::new();
            }
            self.interaction = InteractionMode::Normal;
            self.remember_command(trimmed);
            return self.apply_saved_view_operation(SavedViewMutation::Apply {
                name: name.to_owned(),
            });
        }
        let (route_text, filter_text) =
            trimmed.split_once(' ').map_or((trimmed, ""), |parts| parts);
        let Some(route) = Route::parse(route_text) else {
            if let InteractionMode::CommandLine(state) = &mut self.interaction {
                state.error = Some("unknown route".to_owned());
            }
            return Vec::new();
        };
        if !filter_text.trim().is_empty() && route != Route::Devices && route != Route::Activity {
            if let InteractionMode::CommandLine(state) = &mut self.interaction {
                state.error = Some("filters are available for devices only".to_owned());
            }
            return Vec::new();
        }
        let parsed_filter = if !filter_text.trim().is_empty() && route == Route::Devices {
            match filter::parse(filter_text) {
                Ok(expression) => Some(expression),
                Err(error) => {
                    if let InteractionMode::CommandLine(state) = &mut self.interaction {
                        state.error = Some(error.to_string());
                    }
                    return Vec::new();
                }
            }
        } else {
            None
        };
        self.interaction = InteractionMode::Normal;
        self.remember_command(trimmed);
        let same_route = self.current_route() == route;
        if same_route {
            self.capture_current_frame();
        }
        self.navigate(route);
        if route == Route::Activity {
            self.task_filter = filter_text.trim().to_owned();
            self.tasks.select_filtered_first(&self.task_filter);
        }
        if let Some(expression) = parsed_filter {
            self.views.devices.filter_draft = filter_text.to_owned();
            self.views.devices.applied_filter = expression;
            self.reconcile_selection(None);
        }
        let frame = self.current_view_frame();
        if same_route {
            let _ = self.view_history.append(frame);
        } else {
            self.view_history.replace_current(frame);
        }
        Vec::new()
    }

    fn remember_command(&mut self, command: &str) {
        if self.command_history.last().map(String::as_str) != Some(command) {
            self.command_history.push(command.to_owned());
            if self.command_history.len() > 100 {
                self.command_history.remove(0);
            }
        }
        self.command_history_cursor = None;
    }

    fn accept_filter(&mut self, input: &str) -> Vec<Effect> {
        if self.current_route() == Route::Activity {
            self.task_filter = input.trim().to_owned();
            self.tasks.select_filtered_first(&self.task_filter);
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        match filter::parse(input) {
            Ok(expression) => {
                self.views.devices.filter_draft = input.to_owned();
                self.views.devices.applied_filter = expression;
                self.reconcile_selection(None);
                self.interaction = InteractionMode::Normal;
            }
            Err(error) => {
                if let InteractionMode::FilterLine(state) = &mut self.interaction {
                    state.error = Some(error.to_string());
                }
            }
        }
        Vec::new()
    }

    fn navigate(&mut self, route: Route) {
        if self.current_route() == route {
            return;
        }
        self.capture_current_frame();
        let frame = ViewFrame::new(route);
        let _ = self.view_history.append(frame.clone());
        self.restore_view_frame(&frame);
        self.focus = Focus::Collection;
    }

    pub fn set_route(&mut self, route: Route) {
        self.view_history = ViewHistory::new(route);
        self.restore_view_frame(&ViewFrame::new(route));
    }

    fn capture_current_frame(&mut self) {
        let frame = self.current_view_frame();
        self.view_history.replace_current(frame);
    }

    fn current_view_frame(&self) -> ViewFrame {
        let route = self.current_route();
        let selection = if route == Route::Devices {
            self.views
                .devices
                .selected_id
                .clone()
                .map(ResourceIdentity::Device)
        } else {
            None
        };
        let section = (route == Route::Services).then_some(self.views.services.section);
        ViewFrame {
            route,
            focus: self.focus,
            selection: selection.clone(),
            scroll_anchor: selection,
            filter_text: if route == Route::Devices {
                self.views.devices.filter_draft.clone()
            } else {
                String::new()
            },
            filter: if route == Route::Devices {
                self.views.devices.applied_filter.clone()
            } else {
                FilterExpression::empty()
            },
            task_filter: if route == Route::Activity {
                self.task_filter.clone()
            } else {
                String::new()
            },
            sort: self.views.devices.sort,
            section,
            saved_view: None,
        }
    }

    fn restore_view_frame(&mut self, frame: &ViewFrame) {
        self.focus = frame.focus;
        if frame.route == Route::Devices {
            self.views.devices.filter_draft = frame.filter_text.clone();
            self.views.devices.applied_filter = frame.filter.clone();
            self.views.devices.sort = frame.sort;
            self.views.devices.selected_id = match &frame.selection {
                Some(ResourceIdentity::Device(id)) => Some(id.clone()),
                _ => None,
            };
            let requested = self.views.devices.selected_id.clone();
            self.reconcile_selection(None);
            if requested.is_some() && self.views.devices.selected_id != requested {
                self.runtime_error = Some("previous selection no longer exists".to_owned());
            }
        }
        if frame.route == Route::Services {
            self.views.services.section = frame.section.unwrap_or(ServiceSection::Serve);
            self.views.services.selected = 0;
            self.views.services.scroll = 0;
        }
        if frame.route == Route::Activity {
            self.task_filter = frame.task_filter.clone();
            self.tasks.select_filtered_first(&self.task_filter);
        }
    }

    fn move_history(&mut self, forward: bool) {
        self.capture_current_frame();
        let frame = if forward {
            self.view_history.forward()
        } else {
            self.view_history.backward()
        };
        if let Some(frame) = frame {
            self.restore_view_frame(&frame);
        } else {
            self.runtime_error = Some(if forward {
                "already at newest view".to_owned()
            } else {
                "already at oldest view".to_owned()
            });
        }
    }

    pub fn dispatch_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        let Some(spec) = action::find_action(action_id) else {
            return Vec::new();
        };
        if !self.action_available(action_id, spec.capability) {
            self.runtime_error = Some(
                spec.capability
                    .reason()
                    .map_or("action unavailable", |reason| reason)
                    .to_owned(),
            );
            return Vec::new();
        }
        let activity_selection_required = matches!(
            action_id,
            ActionId::ActivityOpenActor | ActionId::ActivityOpenTarget
        );
        if matches!(spec.selection_rule, action::SelectionRule::One)
            && ((self.current_route() == Route::Devices && self.selected_device().is_none())
                || (self.current_route() == Route::Activity
                    && if activity_selection_required {
                        self.selected_admin_activity().is_none()
                    } else {
                        self.tasks.selected.is_none()
                    })
                || (self.current_route() == Route::Users && self.selected_admin_user().is_none())
                || (self.current_route() == Route::Routes && self.selected_admin_route().is_none()))
        {
            self.runtime_error = Some("select a resource before running this action".to_owned());
            return Vec::new();
        }
        match action_id {
            ActionId::AppQuit => self.handle_quit_key(),
            ActionId::ViewCommandLine => {
                self.interaction = InteractionMode::CommandLine(CommandLineState {
                    editor: LineEditorState::new(String::new()),
                    candidates: self.command_candidates(""),
                    selected_completion: None,
                    error: None,
                });
                Vec::new()
            }
            ActionId::ViewFilter => {
                if !matches!(self.current_route(), Route::Devices | Route::Activity) {
                    self.runtime_error = Some("this view has no filter schema".to_owned());
                    return Vec::new();
                }
                let input = if self.current_route() == Route::Activity {
                    self.task_filter.clone()
                } else {
                    self.views.devices.filter_draft.clone()
                };
                let restoration = FilterRestoration {
                    input: input.clone(),
                    expression: self.views.devices.applied_filter.clone(),
                    selection: self.views.devices.selected_id.clone(),
                    scroll: self.views.devices.scroll,
                    task_filter: self.task_filter.clone(),
                    task_selection: self.tasks.selected,
                };
                self.interaction = InteractionMode::FilterLine(FilterLineState {
                    editor: LineEditorState::new(input),
                    candidates: self.filter_candidates(""),
                    selected_completion: None,
                    error: None,
                    restoration,
                });
                Vec::new()
            }
            ActionId::ViewRefresh => self.start_refresh(false),
            ActionId::ViewRefreshAll => self.start_refresh(true),
            ActionId::ViewHelp => {
                self.interaction = InteractionMode::HelpSheet(HelpSheetState {
                    query: String::new(),
                    filtering: false,
                    scroll: 0,
                });
                Vec::new()
            }
            ActionId::ViewTasks => {
                self.navigate(Route::Activity);
                Vec::new()
            }
            ActionId::ViewHistoryBack => {
                self.move_history(false);
                Vec::new()
            }
            ActionId::ViewHistoryForward => {
                self.move_history(true);
                Vec::new()
            }
            ActionId::ViewServices => {
                self.navigate(Route::Services);
                Vec::new()
            }
            ActionId::ProfileSelect => self.select_next_profile(),
            ActionId::ProfileClear => self.clear_admin_profile(),
            ActionId::AdminRefreshCurrent => self.start_admin_current_view_refresh(),
            ActionId::AdminRefreshAll => self.start_admin_refresh(),
            ActionId::ViewUsers => {
                self.navigate(Route::Users);
                Vec::new()
            }
            ActionId::ViewRoutes => {
                self.navigate(Route::Routes);
                Vec::new()
            }
            ActionId::ViewDns => {
                self.navigate(Route::Dns);
                Vec::new()
            }
            ActionId::ViewAccess => {
                self.navigate(Route::Access);
                Vec::new()
            }
            ActionId::ViewCredentials => {
                self.navigate(Route::Credentials);
                Vec::new()
            }
            ActionId::UsersOpenDevices => self.open_user_devices(),
            ActionId::RoutesOpenDevice => self.open_route_device(),
            ActionId::DnsOpenLocalDiagnostics => {
                self.navigate(Route::Dns);
                Vec::new()
            }
            ActionId::AccessCopySource => {
                if let Some(policy) = self.admin.policy.snapshot.as_ref() {
                    self.overlays.push(Overlay::Confirmation(Box::new(
                        ConfirmationState {
                            action_id,
                            mutation: None,
                            admin_mutation: None,
                            admin_batch: None,
                            service_request: None,
                            operational_mutation: None,
                            handoff: None,
                            prompt: "The full policy source may contain sensitive access rules. Copy it to the clipboard?"
                                .to_owned(),
                            required_phrase: Some("COPY-POLICY".to_owned()),
                            input: String::new(),
                            lose_ssh_checked: false,
                            preview_lines: vec![
                                format!("{} bytes", policy.source_bytes.len()),
                                format!("sha256 {}", policy.content_hash),
                            ],
                            redacted_argv: Vec::new(),
                            error: None,
                        },
                    )));
                } else {
                    self.runtime_error =
                        Some("policy source is not currently available".to_owned());
                }
                Vec::new()
            }
            ActionId::ActivitySelectWindow => {
                self.admin_audit_window_days = match self.admin_audit_window_days {
                    1 => 7,
                    7 => 30,
                    30 => 90,
                    _ => 1,
                };
                self.runtime_error = Some(format!(
                    "configuration audit window: previous {} day{}",
                    self.admin_audit_window_days,
                    if self.admin_audit_window_days == 1 {
                        ""
                    } else {
                        "s"
                    }
                ));
                self.start_admin_current_view_refresh()
            }
            ActionId::ActivityOpenActor => self.open_audit_reference(false),
            ActionId::ActivityOpenTarget => self.open_audit_reference(true),
            ActionId::SettingsInspectCapabilities => {
                self.runtime_error = Some(format!(
                    "observed admin capabilities: {}",
                    self.admin
                        .capabilities
                        .iter()
                        .map(|(name, state)| format!("{name}={}", state.label()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                Vec::new()
            }
            ActionId::SettingsAppearance => {
                self.overlays.push(Overlay::Appearance(AppearanceState {
                    original: self.theme,
                    selected: self.theme.id(),
                }));
                Vec::new()
            }
            ActionId::CollectionMoveUp => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_next_filtered(&self.task_filter, -1);
                    self.move_admin_activity_selection(-1);
                } else if self.current_route() == Route::Services {
                    self.move_service_selection(-1);
                } else if self.current_route() == Route::Users {
                    self.move_admin_user_selection(-1);
                } else if self.current_route() == Route::Routes {
                    self.move_admin_route_selection(-1);
                } else if self.current_route() == Route::Credentials {
                    self.move_admin_credential_selection(-1);
                } else {
                    self.move_selection(-1);
                }
                Vec::new()
            }
            ActionId::CollectionMoveDown => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_next_filtered(&self.task_filter, 1);
                    self.move_admin_activity_selection(1);
                } else if self.current_route() == Route::Services {
                    self.move_service_selection(1);
                } else if self.current_route() == Route::Users {
                    self.move_admin_user_selection(1);
                } else if self.current_route() == Route::Routes {
                    self.move_admin_route_selection(1);
                } else if self.current_route() == Route::Credentials {
                    self.move_admin_credential_selection(1);
                } else {
                    self.move_selection(1);
                }
                Vec::new()
            }
            ActionId::CollectionFirst => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_filtered_first(&self.task_filter);
                    self.admin_activity_selected = 0;
                } else if self.current_route() == Route::Services {
                    self.views.services.selected = 0;
                    self.views.services.scroll = 0;
                } else if self.current_route() == Route::Users {
                    self.admin_user_selected = 0;
                } else if self.current_route() == Route::Routes {
                    self.admin_route_selected = 0;
                } else if self.current_route() == Route::Credentials {
                    self.admin_credential_selected = 0;
                } else {
                    self.move_selection_to(0);
                }
                Vec::new()
            }
            ActionId::CollectionLast => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_filtered_last(&self.task_filter);
                    self.admin_activity_selected = self
                        .admin
                        .activity
                        .snapshot
                        .as_ref()
                        .map_or(0, |snapshot| snapshot.events.len().saturating_sub(1));
                } else if self.current_route() == Route::Services {
                    self.views.services.selected = self.service_row_count().saturating_sub(1);
                    if self.views.services.section == ServiceSection::Metrics {
                        self.views.services.scroll = self.metrics_max_scroll();
                    }
                } else if self.current_route() == Route::Users {
                    self.admin_user_selected = self
                        .admin
                        .users
                        .snapshot
                        .as_ref()
                        .map_or(0, |users| users.len().saturating_sub(1));
                } else if self.current_route() == Route::Routes {
                    self.admin_route_selected =
                        self.admin.route_observations().len().saturating_sub(1);
                } else if self.current_route() == Route::Credentials {
                    self.admin_credential_selected = self
                        .admin
                        .credentials
                        .snapshot
                        .as_ref()
                        .map_or(0, |snapshot| snapshot.records.len().saturating_sub(1));
                } else {
                    self.move_selection_to(usize::MAX);
                }
                Vec::new()
            }
            ActionId::CollectionPageUp => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_next_filtered(&self.task_filter, -5);
                    self.move_admin_activity_selection(-5);
                } else if self.current_route() == Route::Services {
                    self.move_service_selection(-5);
                } else if self.current_route() == Route::Users {
                    self.move_admin_user_selection(-5);
                } else if self.current_route() == Route::Routes {
                    self.move_admin_route_selection(-5);
                } else if self.current_route() == Route::Credentials {
                    self.move_admin_credential_selection(-5);
                } else {
                    self.move_selection(-5);
                }
                Vec::new()
            }
            ActionId::CollectionPageDown => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_next_filtered(&self.task_filter, 5);
                    self.move_admin_activity_selection(5);
                } else if self.current_route() == Route::Services {
                    self.move_service_selection(5);
                } else if self.current_route() == Route::Users {
                    self.move_admin_user_selection(5);
                } else if self.current_route() == Route::Routes {
                    self.move_admin_route_selection(5);
                } else if self.current_route() == Route::Credentials {
                    self.move_admin_credential_selection(5);
                } else {
                    self.move_selection(5);
                }
                Vec::new()
            }
            ActionId::CollectionOpen => {
                if self.current_route() == Route::Activity {
                    if let Some(task_id) = self.tasks.selected {
                        self.overlays.push(Overlay::TaskInspector(task_id));
                    }
                } else if self.current_route() == Route::Services && self.terminal_width < 80 {
                    self.overlays
                        .push(Overlay::ServiceSectionPicker(ServiceSectionPickerState {
                            selected: self.views.services.section.index(),
                        }));
                } else if self.current_route() == Route::Services
                    || self.selected_device().is_some()
                {
                    let selected_id = self.selected_device().map(|device| device.id.0.clone());
                    self.focus = Focus::Inspector;
                    if let Some(effect) = self.start_admin_device_enrichment(selected_id) {
                        return vec![effect];
                    }
                }
                Vec::new()
            }
            ActionId::CollectionSort => {
                self.overlays.push(Overlay::SortPicker { selected: 0 });
                Vec::new()
            }
            ActionId::CollectionWideColumns => {
                if self.current_route() == Route::Devices {
                    self.views.devices.wide_columns = !self.views.devices.wide_columns;
                }
                Vec::new()
            }
            ActionId::ResourceActions => {
                let actions = self.contextual_actions();
                if let Err(error) = action::validate_transient_sequences(&actions) {
                    self.runtime_error = Some(error);
                    return Vec::new();
                }
                self.interaction = InteractionMode::Transient(TransientMenuState {
                    kind: TransientKind::Action,
                    actions,
                    fields: Vec::new(),
                    prefix: None,
                    message: None,
                });
                Vec::new()
            }
            ActionId::ResourceCopy => {
                let fields = self.contextual_copy_fields();
                self.interaction = InteractionMode::Transient(TransientMenuState {
                    kind: TransientKind::Copy,
                    actions: Vec::new(),
                    fields,
                    prefix: None,
                    message: None,
                });
                Vec::new()
            }
            ActionId::TaskCancel => self.cancel_focused_task(),
            ActionId::MockSuccess => self.start_task(
                ActionId::MockSuccess,
                MockTaskBehavior::DelayedSuccess,
                true,
            ),
            ActionId::MockFailure => self.start_task(
                ActionId::MockFailure,
                MockTaskBehavior::DelayedFailure,
                true,
            ),
            ActionId::MockCancellable => self.start_task(
                ActionId::MockCancellable,
                MockTaskBehavior::CancellableLong,
                true,
            ),
            ActionId::MockNonCancellable => self.start_task(
                ActionId::MockNonCancellable,
                MockTaskBehavior::NonCancellable,
                false,
            ),
            ActionId::LocalDiagnostics => self.open_local_diagnostics(),
            ActionId::LocalProbeConnection => self.start_probe_connection(),
            ActionId::LocalNetcheck => {
                self.start_local_diagnostic(DiagnosticRequest::Netcheck { live: false })
            }
            ActionId::LocalNetcheckLive => {
                self.start_local_diagnostic(DiagnosticRequest::Netcheck { live: true })
            }
            ActionId::LocalDnsStatus => self.start_local_diagnostic(DiagnosticRequest::DnsStatus),
            ActionId::LocalDnsQuery => {
                self.overlays
                    .push(Overlay::DiagnosticInput(DiagnosticInputState {
                        kind: DiagnosticInputKind::DnsQuery,
                        input: String::new(),
                        secondary: "A".to_owned(),
                        error: None,
                    }));
                Vec::new()
            }
            ActionId::LocalWhois => self.open_whois_input(),
            ActionId::DiagnosticCopy => {
                let value = self.diagnostic_summary();
                self.copy_text("diagnostic summary", value)
            }
            ActionId::LocalConnect => self.open_mutation_confirmation(LocalMutation::Connect),
            ActionId::LocalDisconnect => {
                self.open_mutation_confirmation(LocalMutation::Disconnect {
                    accept_lose_ssh: false,
                })
            }
            ActionId::LocalPreferencesEdit => {
                self.open_operator_form(ActionId::LocalPreferencesEdit)
            }
            ActionId::LocalExitNodeSelect => self.open_operator_form(ActionId::LocalExitNodeSelect),
            ActionId::LocalRoutesEditAdvertisements => {
                self.open_operator_form(ActionId::LocalRoutesEditAdvertisements)
            }
            ActionId::LocalAccountSwitch => self.open_account_picker(ActionId::LocalAccountSwitch),
            ActionId::LocalAccountLogin => self.open_login_confirmation(),
            ActionId::LocalAccountLogout => self.open_logout_confirmation(),
            ActionId::LocalAccountRemove => self.open_account_picker(ActionId::LocalAccountRemove),
            ActionId::LocalSshOpen => self.open_handoff_input(HandoffInputKind::Ssh),
            ActionId::LocalNcOpen => self.open_handoff_input(HandoffInputKind::Nc),
            ActionId::LocalSyspolicyReload => {
                self.open_mutation_confirmation(LocalMutation::SyspolicyReload)
            }
            ActionId::ServicesSectionNext => {
                self.change_service_section(1);
                Vec::new()
            }
            ActionId::ServicesSectionPrevious => {
                self.change_service_section(-1);
                Vec::new()
            }
            ActionId::ServicesServeRefresh
            | ActionId::ServicesFunnelRefresh
            | ActionId::ServicesDriveRefresh
            | ActionId::ServicesMetricsRefresh => self.start_services_action(action_id),
            ActionId::ServicesServeCreate
            | ActionId::ServicesServeEdit
            | ActionId::ServicesServeReset
            | ActionId::ServicesFunnelCreate
            | ActionId::ServicesFunnelEdit
            | ActionId::ServicesFunnelReset
            | ActionId::ServicesTaildropSend
            | ActionId::ServicesTaildropReceive
            | ActionId::ServicesDriveShare
            | ActionId::ServicesDriveRename
            | ActionId::ServicesDriveUnshare
            | ActionId::ServicesCertificateObtain
            | ActionId::ServicesBugReportCreate => self.open_service_action(action_id),
            ActionId::ServicesDriveEnableAlpha => {
                self.alpha_local_features = true;
                self.start_services_refresh()
            }
            ActionId::AdminDeviceRename
            | ActionId::AdminDeviceTagsReplace
            | ActionId::AdminDeviceApprove
            | ActionId::AdminDeviceRevokeApproval
            | ActionId::AdminDeviceKeyExpiryConfigure
            | ActionId::AdminDeviceKeyExpireNow
            | ActionId::AdminDeviceDelete
            | ActionId::AdminRoutesReplaceApprovals
            | ActionId::AdminDnsPreferencesEdit
            | ActionId::AdminDnsNameserversReplace
            | ActionId::AdminDnsSearchPathsReplace
            | ActionId::AdminDnsSplitCreate
            | ActionId::AdminDnsSplitEdit
            | ActionId::AdminDnsSplitRemove
            | ActionId::AdminUserApprove
            | ActionId::AdminUserRoleChange
            | ActionId::AdminUserSuspend
            | ActionId::AdminUserRestore
            | ActionId::AdminUserDelete => self.open_admin_form(action_id),
            ActionId::AdminPolicyEdit => self.open_policy_workflow(),
            ActionId::AdminPolicyEditorReopen => self.reopen_policy_editor(),
            ActionId::AdminPolicyCandidateDiscard => self.open_policy_discard_confirmation(),
            ActionId::AdminPolicyRemoteRefresh => self.refresh_policy_workflow(),
            ActionId::AdminPolicyValidate => self.validate_policy_candidate(),
            ActionId::AdminPolicyPreview => self.preview_policy_candidate(),
            ActionId::AdminPolicyDiff => self.diff_policy_candidate(),
            ActionId::AdminPolicyApply => self.open_policy_apply_confirmation(),
            ActionId::AdminPolicyWorkflowClose => self.open_policy_close_confirmation(),
            ActionId::AdminCredentialAuthKeyCreate => self.open_auth_key_confirmation(),
            ActionId::SecretResultCopy => self.copy_secret_result(),
            ActionId::SecretResultClose => self.close_secret_result(),
            ActionId::AdminCredentialRevoke => self.open_credential_revoke_confirmation(),
            ActionId::ProfileCredentialRemove => self.open_profile_credential_confirmation(),
            ActionId::AuditFilterTime
            | ActionId::AuditFilterActor
            | ActionId::AuditFilterAction
            | ActionId::AuditFilterTarget => self.open_audit_filter(action_id),
            ActionId::AuditOpenTarget => self.open_audit_reference(true),
            ActionId::AuditOpenPolicyDiff => self.open_audit_investigation(),
            ActionId::BatchReviewOutcomes => self.open_selected_batch_result(),
            ActionId::BatchRetrySelected => self.retry_selected_batch(),
            ActionId::ActivityFlowsSelectWindow => {
                let now =
                    time::OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(self.now as i64);
                let window = crate::domain::flow::FlowWindow::previous_hour(now);
                let input = window.query_values().map_or_else(
                    |_| String::new(),
                    |(start, end)| format!("start={start};end={end}"),
                );
                self.overlays
                    .push(Overlay::OperatorForm(OperatorFormState::new(
                        action_id, input, None,
                    )));
                Vec::new()
            }
            ActionId::ActivityFlowsAggregate => {
                if let Some(snapshot) = self.flow_snapshot.as_ref() {
                    if !snapshot.complete {
                        self.runtime_error = Some(
                            "flow aggregation is disabled for a partial bounded response; choose a narrower window"
                                .to_owned(),
                        );
                        return Vec::new();
                    }
                    let dimensions = vec![
                        AggregateDimension::ReportingNode,
                        AggregateDimension::TrafficClass,
                        AggregateDimension::Protocol,
                    ];
                    let messages = snapshot.messages.clone();
                    self.cancel_flow_aggregation();
                    self.flow_aggregation_generation =
                        self.flow_aggregation_generation.saturating_add(1);
                    let generation = self.flow_aggregation_generation;
                    let cancellation = Arc::new(AtomicBool::new(false));
                    self.flow_aggregation_cancellation = Some(Arc::clone(&cancellation));
                    self.runtime_error = Some("aggregating the bounded flow window".to_owned());
                    return vec![Effect::StartFlowAggregation {
                        generation,
                        messages,
                        filter: self.flow_filter.clone(),
                        dimensions,
                        cancellation,
                    }];
                } else {
                    self.runtime_error = Some(
                        "flow aggregation requires a completed bounded flow window".to_owned(),
                    );
                }
                Vec::new()
            }
            ActionId::ActivityFlowsOpenDevice => {
                self.navigate(Route::Devices);
                Vec::new()
            }
            ActionId::OverviewHealthOpenResource | ActionId::OverviewHealthRunSuggestedAction => {
                self.dispatch_health_action(action_id)
            }
            ActionId::AdminWebhookCreate
            | ActionId::AdminWebhookEdit
            | ActionId::AdminWebhookTest
            | ActionId::AdminWebhookRotateSecret
            | ActionId::AdminWebhookDelete
            | ActionId::AdminLogStreamReplace
            | ActionId::AdminLogStreamDelete
            | ActionId::AdminNetworkLogsSettings => self.open_phase_eight_action(action_id),
            ActionId::SavedViewCreate
            | ActionId::SavedViewReplace
            | ActionId::SavedViewRename
            | ActionId::SavedViewDelete
            | ActionId::SavedViewApply
            | ActionId::CollectionExport
            | ActionId::AccessExplorerAsk
            | ActionId::AccessExplorerOpenRule => self.open_phase_eight_local_action(action_id),
        }
    }

    fn action_available(&self, action_id: ActionId, capability: Capability) -> bool {
        if action_id == ActionId::ResourceCopy {
            return !self.contextual_copy_fields().is_empty();
        }
        match capability {
            Capability::Available if is_admin_action(action_id) => {
                self.admin_action_available(action_id)
            }
            Capability::Available => self.local_action_available(action_id),
            Capability::MockOnly => self.source_mode == SourceMode::Mock,
            Capability::Disabled(_) => false,
        }
    }

    fn action_available_for_id(&self, action_id: ActionId) -> bool {
        action::find_action(action_id)
            .is_some_and(|spec| self.action_available(action_id, spec.capability))
    }

    fn admin_action_available(&self, action_id: ActionId) -> bool {
        match action_id {
            ActionId::ProfileSelect => !self.resolved_config.profiles.is_empty(),
            ActionId::ProfileClear => self.admin.profile.is_some(),
            ActionId::ViewDns => true,
            ActionId::AdminRefreshCurrent | ActionId::AdminRefreshAll => {
                self.admin.profile.is_some()
            }
            ActionId::ViewUsers
            | ActionId::ViewRoutes
            | ActionId::ViewAccess
            | ActionId::ViewCredentials => self.admin.profile.is_some(),
            ActionId::UsersOpenDevices => self.admin.users.snapshot.is_some(),
            ActionId::RoutesOpenDevice => self.admin.routes.snapshot.is_some(),
            ActionId::DnsOpenLocalDiagnostics => true,
            ActionId::AccessCopySource => self.admin.policy.snapshot.is_some(),
            ActionId::ActivitySelectWindow
            | ActionId::ActivityOpenActor
            | ActionId::ActivityOpenTarget
            | ActionId::ActivityFlowsSelectWindow
            | ActionId::ActivityFlowsAggregate
            | ActionId::ActivityFlowsOpenDevice
            | ActionId::AccessExplorerAsk
            | ActionId::AccessExplorerOpenRule
            | ActionId::OverviewHealthOpenResource
            | ActionId::OverviewHealthRunSuggestedAction => {
                self.phase_eight_read_available(action_id)
            }
            ActionId::AdminWebhookCreate
            | ActionId::AdminWebhookEdit
            | ActionId::AdminWebhookTest
            | ActionId::AdminWebhookRotateSecret
            | ActionId::AdminWebhookDelete
            | ActionId::AdminLogStreamReplace
            | ActionId::AdminLogStreamDelete
            | ActionId::AdminNetworkLogsSettings => self.phase_eight_mutation_available(action_id),
            ActionId::SettingsInspectCapabilities => self.admin.profile.is_some(),
            ActionId::SettingsAppearance => true,
            ActionId::AdminPolicyEdit
            | ActionId::AdminPolicyEditorReopen
            | ActionId::AdminPolicyCandidateDiscard
            | ActionId::AdminPolicyRemoteRefresh
            | ActionId::AdminPolicyValidate
            | ActionId::AdminPolicyPreview
            | ActionId::AdminPolicyDiff
            | ActionId::AdminPolicyApply
            | ActionId::AdminPolicyWorkflowClose
            | ActionId::AdminCredentialAuthKeyCreate
            | ActionId::AdminCredentialRevoke
            | ActionId::ProfileCredentialRemove => self.phase_seven_admin_available(action_id),
            ActionId::AuditFilterTime
            | ActionId::AuditFilterActor
            | ActionId::AuditFilterAction
            | ActionId::AuditFilterTarget
            | ActionId::AuditOpenTarget
            | ActionId::AuditOpenPolicyDiff => self.admin.profile.is_some(),
            action_id if is_admin_mutation_action(action_id) => {
                self.admin_mutation_available(action_id)
            }
            ActionId::BatchReviewOutcomes => self
                .tasks
                .selected
                .is_some_and(|task_id| self.admin_batch_results.contains_key(&task_id)),
            ActionId::BatchRetrySelected => self.tasks.selected.is_some_and(|task_id| {
                self.admin_batch_results.get(&task_id).is_some_and(|batch| {
                    batch.child_outcomes.values().any(|outcome| {
                        !matches!(
                            outcome,
                            crate::domain::admin_mutation::BatchChildOutcome::VerifiedSuccess
                        )
                    })
                })
            }),
            _ => false,
        }
    }

    fn admin_mutation_available(&self, action_id: ActionId) -> bool {
        if self.admin.profile.is_none()
            || self.admin.profile_read_only
            || self.resolved_config.read_only
        {
            return false;
        }
        let scope = match action_id {
            ActionId::AdminRoutesReplaceApprovals => "devices:routes",
            action_id if is_admin_dns_action(action_id) => "dns",
            action_id if is_admin_user_action(action_id) => "users",
            _ => "devices:core",
        };
        if !self.admin_scope_allowed(scope) {
            return false;
        }
        match action_id {
            ActionId::AdminPolicyApply => self
                .policy_workflow
                .as_ref()
                .is_some_and(|workflow| workflow.state() == PolicyState::ReadyToApply),
            ActionId::AdminPolicyCandidateDiscard => self.policy_workflow.is_some(),
            ActionId::AdminCredentialAuthKeyCreate => self.admin_scope_allowed("auth_keys:write"),
            ActionId::AdminCredentialRevoke => self
                .selected_credential()
                .is_some_and(|credential| !credential.id.is_empty()),
            ActionId::ProfileCredentialRemove => true,
            ActionId::AdminRoutesReplaceApprovals => {
                self.admin.routes.state == AdminResourceState::Ready
                    && self
                        .admin
                        .route_observations()
                        .iter()
                        .any(|route| route.complete)
            }
            action_id if is_admin_device_action(action_id) => {
                self.admin.devices.state == AdminResourceState::Ready
                    && self.selected_admin_device().is_some()
            }
            action_id if is_admin_user_action(action_id) => {
                self.admin.users.state == AdminResourceState::Ready
                    && self.selected_admin_user().is_some()
            }
            ActionId::AdminDnsPreferencesEdit => {
                self.admin.dns_preferences.state == AdminResourceState::Ready
            }
            ActionId::AdminDnsNameserversReplace => {
                self.admin.nameservers.state == AdminResourceState::Ready
            }
            ActionId::AdminDnsSearchPathsReplace => {
                self.admin.search_paths.state == AdminResourceState::Ready
            }
            ActionId::AdminDnsSplitCreate
            | ActionId::AdminDnsSplitEdit
            | ActionId::AdminDnsSplitRemove => {
                self.admin.split_dns.state == AdminResourceState::Ready
            }
            _ => false,
        }
    }

    fn phase_seven_admin_available(&self, action_id: ActionId) -> bool {
        if self.admin.profile.is_none() {
            return false;
        }
        if matches!(
            action_id,
            ActionId::AdminPolicyEdit | ActionId::AdminPolicyEditorReopen
        ) && !crate::temporary::policy_editing_supported()
        {
            return false;
        }
        if matches!(action_id, ActionId::ProfileCredentialRemove) {
            return self
                .resolved_config
                .profiles
                .contains_key(self.admin.profile.as_deref().map_or("", |value| value));
        }
        if matches!(action_id, ActionId::AdminCredentialAuthKeyCreate)
            && !self.admin_scope_allowed("auth_keys:write")
        {
            return false;
        }
        if matches!(action_id, ActionId::AdminCredentialRevoke) {
            let Some(credential) = self.selected_credential() else {
                return false;
            };
            let credential_type = crate::admin::key_mutations::remote_credential_type(credential);
            let Some(read_scope) = credential_type.read_scope() else {
                return false;
            };
            let Some(write_scope) = credential_type.write_scope() else {
                return false;
            };
            if !credential_type.supported_for_revoke()
                || !self.admin_scope_allowed(read_scope)
                || !self.admin_scope_allowed(write_scope)
            {
                return false;
            }
        }
        if matches!(
            action_id,
            ActionId::AdminPolicyApply
                | ActionId::AdminCredentialAuthKeyCreate
                | ActionId::AdminCredentialRevoke
        ) && (self.resolved_config.read_only || self.admin.profile_read_only)
        {
            return false;
        }
        match action_id {
            ActionId::AdminPolicyEdit | ActionId::AdminPolicyRemoteRefresh => {
                self.admin_scope_allowed("policy_file:read")
            }
            ActionId::AdminPolicyEditorReopen => self
                .policy_workflow
                .as_ref()
                .is_some_and(|workflow| workflow.candidate_path().is_some()),
            ActionId::AdminPolicyCandidateDiscard
            | ActionId::AdminPolicyValidate
            | ActionId::AdminPolicyPreview
            | ActionId::AdminPolicyDiff
            | ActionId::AdminPolicyWorkflowClose => self.policy_workflow.is_some(),
            ActionId::AdminPolicyApply => {
                self.admin_scope_allowed("policy_file:write")
                    && self
                        .policy_workflow
                        .as_ref()
                        .is_some_and(|workflow| workflow.state() == PolicyState::ReadyToApply)
            }
            ActionId::AdminCredentialAuthKeyCreate => true,
            ActionId::AdminCredentialRevoke => self.selected_credential().is_some(),
            _ => false,
        }
    }

    fn phase_eight_read_available(&self, action_id: ActionId) -> bool {
        if self.admin.profile.is_none() {
            return false;
        }
        let scope = match action_id {
            ActionId::ActivityFlowsSelectWindow
            | ActionId::ActivityFlowsAggregate
            | ActionId::ActivityFlowsOpenDevice => "logs:network:read",
            ActionId::AccessExplorerAsk | ActionId::AccessExplorerOpenRule => "policy_file:read",
            _ => return true,
        };
        self.admin_scope_allowed(scope)
    }

    fn phase_eight_mutation_available(&self, action_id: ActionId) -> bool {
        if self.admin.profile.is_none()
            || self.admin.profile_read_only
            || self.resolved_config.read_only
        {
            return false;
        }
        let scope = match action_id {
            ActionId::AdminWebhookCreate
            | ActionId::AdminWebhookEdit
            | ActionId::AdminWebhookTest
            | ActionId::AdminWebhookRotateSecret
            | ActionId::AdminWebhookDelete => "webhooks",
            ActionId::AdminLogStreamReplace | ActionId::AdminLogStreamDelete => "log_streaming",
            ActionId::AdminNetworkLogsSettings => "logs:network",
            _ => return false,
        };
        self.admin_scope_allowed(scope)
    }

    fn admin_scope_allowed(&self, scope: &str) -> bool {
        self.admin.requested_scopes.is_empty()
            || self.admin.requested_scopes.iter().any(|value| {
                value == scope
                    || value == "*"
                    || value == "all"
                    || value.ends_with(":*") && scope.starts_with(value.trim_end_matches('*'))
                    || scope
                        .strip_suffix(":read")
                        .or_else(|| scope.strip_suffix(":write"))
                        .is_some_and(|base| value == base)
            })
    }

    pub fn action_is_available(&self, action_id: ActionId) -> bool {
        action::find_action(action_id)
            .is_some_and(|spec| self.action_available(action_id, spec.capability))
    }

    pub fn action_unavailable_reason(&self, action_id: ActionId) -> Option<String> {
        if self.action_is_available(action_id) {
            return None;
        }
        if self.source_mode != SourceMode::Local
            && matches!(
                action_id,
                ActionId::LocalDiagnostics
                    | ActionId::LocalProbeConnection
                    | ActionId::LocalNetcheck
                    | ActionId::LocalNetcheckLive
                    | ActionId::LocalDnsStatus
                    | ActionId::LocalDnsQuery
                    | ActionId::LocalWhois
                    | ActionId::DiagnosticCopy
            )
        {
            return Some("local observer is disabled".to_owned());
        }
        if self.source_mode != SourceMode::Local && is_mutating_action(action_id) {
            return Some("local operator is disabled".to_owned());
        }
        if self.resolved_config.read_only && is_mutating_action(action_id) {
            return Some("read-only mode blocks local mutations".to_owned());
        }
        if matches!(
            action_id,
            ActionId::AdminPolicyEdit | ActionId::AdminPolicyEditorReopen
        ) && !crate::temporary::policy_editing_supported()
        {
            return Some(
                "policy editing is unavailable: secure user-only temporary storage is unsupported on this platform"
                    .to_owned(),
            );
        }
        if is_admin_mutation_action(action_id)
            && (self.resolved_config.read_only || self.admin.profile_read_only)
        {
            return Some("read-only mode blocks admin mutations".to_owned());
        }
        if is_admin_mutation_action(action_id) && self.admin.profile.is_none() {
            return Some("an authenticated admin profile is required".to_owned());
        }
        if is_service_write_action(action_id) && self.resolved_config.read_only {
            return Some("read-only mode blocks local service mutations".to_owned());
        }
        if is_local_verification_mutation(action_id) && !self.local_daemon_state.is_live() {
            return Some(
                "local daemon observation is not live; mutation verification is unavailable"
                    .to_owned(),
            );
        }
        if is_taildrive_action(action_id)
            && action_id != ActionId::ServicesDriveEnableAlpha
            && !self.alpha_local_features
        {
            return Some("Taildrive is alpha and disabled until enabled for this run".to_owned());
        }
        if is_phase_four_action(action_id) && self.local_executable.is_none() {
            return Some("tailscale executable has not been discovered".to_owned());
        }
        if self.local_executable.is_none()
            && matches!(
                action_id,
                ActionId::LocalDiagnostics
                    | ActionId::LocalProbeConnection
                    | ActionId::LocalNetcheck
                    | ActionId::LocalNetcheckLive
                    | ActionId::LocalDnsStatus
                    | ActionId::LocalDnsQuery
                    | ActionId::LocalWhois
                    | ActionId::LocalConnect
                    | ActionId::LocalDisconnect
                    | ActionId::LocalPreferencesEdit
                    | ActionId::LocalExitNodeSelect
                    | ActionId::LocalRoutesEditAdvertisements
                    | ActionId::LocalAccountSwitch
                    | ActionId::LocalAccountLogin
                    | ActionId::LocalAccountLogout
                    | ActionId::LocalAccountRemove
                    | ActionId::LocalSshOpen
                    | ActionId::LocalNcOpen
                    | ActionId::LocalSyspolicyReload
            )
        {
            return Some("tailscale executable has not been discovered".to_owned());
        }
        if matches!(
            action_id,
            ActionId::LocalPreferencesEdit
                | ActionId::LocalExitNodeSelect
                | ActionId::LocalRoutesEditAdvertisements
        ) && !self.local_preferences_ready()
        {
            return Some("current preferences are not verified".to_owned());
        }
        let reason = match action_id {
            ActionId::LocalProbeConnection => "ping is unavailable for this client",
            ActionId::LocalNetcheck => "one-shot netcheck is unavailable for this client",
            ActionId::LocalNetcheckLive => "live netcheck is unavailable for this client",
            ActionId::LocalDnsStatus => "DNS status is unavailable for this client",
            ActionId::LocalDnsQuery => "DNS query is unavailable for this client",
            ActionId::LocalWhois => "whois is unavailable for this client",
            ActionId::LocalConnect => "connect is unavailable for this client",
            ActionId::LocalDisconnect => "disconnect is unavailable for this client",
            ActionId::LocalPreferencesEdit => "preference editing is unavailable for this client",
            ActionId::LocalExitNodeSelect => "exit-node selection is unavailable for this client",
            ActionId::LocalRoutesEditAdvertisements => {
                "advertisement editing is unavailable for this client"
            }
            ActionId::LocalAccountSwitch => "account switching is unavailable for this client",
            ActionId::LocalAccountLogin => "account login is unavailable for this client",
            ActionId::LocalAccountLogout => "account logout is unavailable for this client",
            ActionId::LocalAccountRemove => "account removal is unavailable for this client",
            ActionId::LocalSshOpen => "Tailscale SSH is unavailable for this client",
            ActionId::LocalNcOpen => "Tailscale netcat is unavailable for this client",
            ActionId::LocalSyspolicyReload => "system policy reload is unavailable for this client",
            action_id if is_admin_mutation_action(action_id) => {
                "the selected admin resource or mutation scope is unavailable"
            }
            _ => "capability unavailable",
        };
        Some(reason.to_owned())
    }

    fn local_action_available(&self, action_id: ActionId) -> bool {
        if action_id == ActionId::ViewServices {
            return true;
        }
        if is_phase_four_action(action_id) && self.source_mode != SourceMode::Local {
            return false;
        }
        if is_phase_three_action(action_id) && self.source_mode != SourceMode::Local {
            return false;
        }
        if is_mutating_action(action_id) && self.resolved_config.read_only {
            return false;
        }
        if is_service_write_action(action_id) && self.resolved_config.read_only {
            return false;
        }
        if is_local_verification_mutation(action_id) && !self.local_daemon_state.is_live() {
            return false;
        }
        if matches!(
            action_id,
            ActionId::LocalPreferencesEdit
                | ActionId::LocalExitNodeSelect
                | ActionId::LocalRoutesEditAdvertisements
        ) && !self.local_preferences_ready()
        {
            return false;
        }
        let capabilities = self.local_capabilities;
        match action_id {
            ActionId::LocalConnect => capabilities.connect,
            ActionId::LocalDisconnect => capabilities.disconnect,
            ActionId::LocalPreferencesEdit
            | ActionId::LocalExitNodeSelect
            | ActionId::LocalRoutesEditAdvertisements => capabilities.set,
            ActionId::LocalAccountSwitch => capabilities.accounts,
            ActionId::LocalAccountLogin => capabilities.account_login,
            ActionId::LocalAccountLogout => capabilities.account_logout,
            ActionId::LocalAccountRemove => capabilities.account_remove,
            ActionId::LocalSshOpen => capabilities.ssh,
            ActionId::LocalNcOpen => capabilities.nc,
            ActionId::LocalSyspolicyReload => capabilities.syspolicy,
            ActionId::ServicesServeRefresh
            | ActionId::ServicesServeCreate
            | ActionId::ServicesServeEdit
            | ActionId::ServicesServeReset => capabilities.serve,
            ActionId::ServicesFunnelRefresh
            | ActionId::ServicesFunnelCreate
            | ActionId::ServicesFunnelEdit
            | ActionId::ServicesFunnelReset => capabilities.funnel,
            ActionId::ServicesTaildropSend | ActionId::ServicesTaildropReceive => {
                capabilities.taildrop
            }
            ActionId::ServicesDriveRefresh
            | ActionId::ServicesDriveShare
            | ActionId::ServicesDriveRename
            | ActionId::ServicesDriveUnshare => capabilities.drive && self.alpha_local_features,
            ActionId::ServicesDriveEnableAlpha => capabilities.drive,
            ActionId::ServicesCertificateObtain => capabilities.certificate,
            ActionId::ServicesMetricsRefresh => capabilities.metrics,
            ActionId::ServicesBugReportCreate => capabilities.bugreport,
            _ => self.local_observer_action_available(action_id),
        }
    }

    fn local_observer_action_available(&self, action_id: ActionId) -> bool {
        if !matches!(
            action_id,
            ActionId::LocalDiagnostics
                | ActionId::LocalProbeConnection
                | ActionId::LocalNetcheck
                | ActionId::LocalNetcheckLive
                | ActionId::LocalDnsStatus
                | ActionId::LocalDnsQuery
                | ActionId::LocalWhois
                | ActionId::DiagnosticCopy
        ) {
            return true;
        }
        if self.source_mode != SourceMode::Local {
            return false;
        }
        if action_id == ActionId::DiagnosticCopy {
            return true;
        }
        if self.local_executable.is_none() {
            return false;
        }
        match action_id {
            ActionId::LocalProbeConnection => self.local_capabilities.ping,
            ActionId::LocalNetcheck => self.local_capabilities.netcheck_json,
            ActionId::LocalNetcheckLive => self.local_capabilities.netcheck_json_line,
            ActionId::LocalDnsStatus => self.local_capabilities.dns_status_json,
            ActionId::LocalDnsQuery => self.local_capabilities.dns_query_json,
            ActionId::LocalWhois => self.local_capabilities.whois_json,
            ActionId::LocalDiagnostics => true,
            ActionId::DiagnosticCopy => true,
            _ => true,
        }
    }

    fn local_preferences_ready(&self) -> bool {
        self.local_preferences.want_running.observed_at != 0
            && self.local_preferences.accept_dns.value.is_some()
    }

    pub fn preferences_ready(&self) -> bool {
        self.local_preferences_ready()
    }

    fn open_operator_form(&mut self, action_id: ActionId) -> Vec<Effect> {
        if !self.local_preferences_ready() {
            self.runtime_error =
                Some("current preferences are not verified; editing is unavailable".to_owned());
            return Vec::new();
        }
        let input = match action_id {
            ActionId::LocalPreferencesEdit => String::new(),
            ActionId::LocalExitNodeSelect => "none".to_owned(),
            ActionId::LocalRoutesEditAdvertisements => String::new(),
            _ => String::new(),
        };
        self.overlays
            .push(Overlay::OperatorForm(OperatorFormState::new(
                action_id, input, None,
            )));
        Vec::new()
    }

    fn open_admin_form(&mut self, action_id: ActionId) -> Vec<Effect> {
        let input = match action_id {
            ActionId::AdminDeviceRename => self
                .selected_admin_device()
                .and_then(|device| device.name.clone().or_else(|| device.hostname.clone()))
                .unwrap_or_default(),
            ActionId::AdminDeviceTagsReplace => self
                .selected_admin_device()
                .map_or_else(String::new, |device| device.tags.join(",")),
            ActionId::AdminDeviceKeyExpiryConfigure => self
                .selected_admin_device()
                .and_then(|device| device.key_expiry_disabled)
                .map_or_else(
                    || "on".to_owned(),
                    |value| if value { "on" } else { "off" }.to_owned(),
                ),
            ActionId::AdminRoutesReplaceApprovals => self
                .selected_admin_route()
                .or_else(|| {
                    self.admin
                        .route_observations()
                        .into_iter()
                        .find(|route| route.complete)
                })
                .map_or_else(String::new, |route| route.enabled.join(",")),
            ActionId::AdminDnsPreferencesEdit => self
                .admin
                .dns_preferences
                .snapshot
                .as_ref()
                .and_then(|value| value.magic_dns)
                .map_or_else(
                    || "off".to_owned(),
                    |value| if value { "on" } else { "off" }.to_owned(),
                ),
            ActionId::AdminDnsNameserversReplace => self
                .admin
                .nameservers
                .snapshot
                .as_ref()
                .map_or_else(String::new, |value| value.values.join(",")),
            ActionId::AdminDnsSearchPathsReplace => self
                .admin
                .search_paths
                .snapshot
                .as_ref()
                .map_or_else(String::new, |value| value.values.join(",")),
            ActionId::AdminDnsSplitEdit | ActionId::AdminDnsSplitRemove => self
                .admin
                .split_dns
                .snapshot
                .as_ref()
                .and_then(|value| value.entries.first())
                .map_or_else(String::new, |(domain, resolvers)| {
                    if action_id == ActionId::AdminDnsSplitRemove {
                        domain.clone()
                    } else {
                        format!(
                            "{domain}={}",
                            resolvers
                                .as_ref()
                                .map_or_else(String::new, |values| values.join(","))
                        )
                    }
                }),
            ActionId::AdminUserRoleChange => self
                .selected_admin_user()
                .and_then(|user| user.role.clone())
                .unwrap_or_else(|| "member".to_owned()),
            ActionId::AdminWebhookCreate => String::new(),
            ActionId::AdminWebhookEdit => {
                self.webhooks.first().map_or_else(String::new, |webhook| {
                    format!(
                        "categories={};events={}",
                        webhook.subscriptions.wire_categories().join(","),
                        webhook.subscriptions.wire_events().join(",")
                    )
                })
            }
            ActionId::AdminLogStreamReplace => self
                .log_stream_configurations
                .get(&LogType::Network)
                .or_else(|| self.log_stream_configurations.get(&LogType::Configuration))
                .map_or_else(
                    || "type=network;destination=;url=;secret=replace".to_owned(),
                    |configuration| {
                        format!(
                            "type={};destination={};url={};secret=replace",
                            configuration.log_type.wire_value(),
                            configuration.destination.kind,
                            configuration.destination.identity
                        )
                    },
                ),
            ActionId::AdminNetworkLogsSettings => self
                .admin
                .settings
                .snapshot
                .as_ref()
                .and_then(|settings| settings.network_flow_logging_on)
                .map_or_else(
                    || "on".to_owned(),
                    |value| if value { "on" } else { "off" }.to_owned(),
                ),
            _ => String::new(),
        };
        let mut state = admin_operator_form_state(action_id, input, None);
        if action_id == ActionId::AdminLogStreamReplace {
            state.secret_input = Some(SecretInput::new());
        }
        self.overlays.push(Overlay::OperatorForm(state));
        Vec::new()
    }

    fn selected_webhook(&self) -> Option<&WebhookEndpoint> {
        self.webhooks.first()
    }

    fn open_phase_eight_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        match action_id {
            ActionId::AdminWebhookCreate
            | ActionId::AdminWebhookEdit
            | ActionId::AdminLogStreamReplace
            | ActionId::AdminNetworkLogsSettings => self.open_admin_form(action_id),
            ActionId::AdminWebhookTest => {
                let Some(webhook) = self.selected_webhook() else {
                    self.runtime_error = Some("no observed webhook is available".to_owned());
                    return Vec::new();
                };
                self.open_operational_confirmation(
                    action_id,
                    OperationalMutation::Webhook(WebhookMutation::Test {
                        endpoint_id: webhook.stable_id.clone(),
                    }),
                )
            }
            ActionId::AdminWebhookRotateSecret => {
                let Some(webhook) = self.selected_webhook() else {
                    self.runtime_error = Some("no observed webhook is available".to_owned());
                    return Vec::new();
                };
                self.open_operational_confirmation(
                    action_id,
                    OperationalMutation::Webhook(WebhookMutation::RotateSecret {
                        endpoint_id: webhook.stable_id.clone(),
                    }),
                )
            }
            ActionId::AdminWebhookDelete => {
                let Some(webhook) = self.selected_webhook() else {
                    self.runtime_error = Some("no observed webhook is available".to_owned());
                    return Vec::new();
                };
                self.open_operational_confirmation(
                    action_id,
                    OperationalMutation::Webhook(WebhookMutation::Delete {
                        endpoint_id: webhook.stable_id.clone(),
                        endpoint_label: webhook.endpoint_url.clone(),
                    }),
                )
            }
            ActionId::AdminLogStreamDelete => {
                let log_type = self
                    .log_stream_configurations
                    .keys()
                    .next()
                    .copied()
                    .map_or(LogType::Network, |value| value);
                self.open_operational_confirmation(
                    action_id,
                    OperationalMutation::LogStreamDelete(log_type),
                )
            }
            _ => Vec::new(),
        }
    }

    fn open_operational_confirmation(
        &mut self,
        action_id: ActionId,
        mutation: OperationalMutation,
    ) -> Vec<Effect> {
        self.pending_export_fingerprint = match &mutation {
            OperationalMutation::Export(request) => match self.export_fingerprint(request) {
                Ok(fingerprint) => Some(fingerprint),
                Err(error) => {
                    self.runtime_error = Some(format!("export preview unavailable: {error}"));
                    return Vec::new();
                }
            },
            _ => None,
        };
        let required_phrase = match &mutation {
            OperationalMutation::Webhook(WebhookMutation::Test { .. }) => None,
            OperationalMutation::Webhook(WebhookMutation::RotateSecret { .. }) => {
                Some("ROTATE WEBHOOK SECRET".to_owned())
            }
            OperationalMutation::Webhook(WebhookMutation::Delete { .. }) => {
                Some("DELETE WEBHOOK".to_owned())
            }
            OperationalMutation::LogStreamDelete(_) => Some("DELETE LOG STREAM".to_owned()),
            OperationalMutation::Webhook(_)
            | OperationalMutation::LogStreamReplace(_)
            | OperationalMutation::NetworkLogSetting { .. } => {
                Some("APPLY OPERATIONAL CHANGE".to_owned())
            }
            OperationalMutation::SavedView(SavedViewMutation::Replace { .. })
            | OperationalMutation::SavedView(SavedViewMutation::Delete { .. }) => None,
            OperationalMutation::SavedView(
                SavedViewMutation::Create(_)
                | SavedViewMutation::Rename { .. }
                | SavedViewMutation::Apply { .. },
            ) => None,
            OperationalMutation::Export(request) if request.path.exists() => {
                Some("OVERWRITE EXPORT".to_owned())
            }
            OperationalMutation::Export(_) => None,
        };
        let prompt = match &mutation {
            OperationalMutation::Webhook(WebhookMutation::Test { .. }) => {
                "Queue a server-side webhook test? Tale will report acknowledgement only.".to_owned()
            }
            OperationalMutation::Webhook(WebhookMutation::RotateSecret { .. }) => {
                "Rotate this webhook's write-only signing secret? The new secret is shown once.".to_owned()
            }
            OperationalMutation::Webhook(WebhookMutation::Delete { .. }) => {
                "Delete this webhook after a final typed confirmation?".to_owned()
            }
            OperationalMutation::LogStreamDelete(_) => {
                "Delete this log-stream configuration?".to_owned()
            }
            OperationalMutation::Webhook(_)
            | OperationalMutation::LogStreamReplace(_)
            | OperationalMutation::NetworkLogSetting { .. } => {
                "Apply this typed operational change?".to_owned()
            }
            OperationalMutation::SavedView(_) => {
                "Apply this saved-view operation? The document stores only query and presentation state.".to_owned()
            }
            OperationalMutation::Export(_) => {
                "Write this allowlisted deterministic export?".to_owned()
            }
        };
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: Some(mutation.clone()),
                handoff: None,
                prompt,
                required_phrase,
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines: vec![
                mutation.preview(),
                "No automatic retry will be attempted; a verification read follows the mutation."
                    .to_owned(),
            ],
                redacted_argv: Vec::new(),
                error: None,
            })));
        Vec::new()
    }

    fn dispatch_health_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        let Some(finding) = self.health_findings.first() else {
            self.runtime_error = Some("no derived health finding is available".to_owned());
            return Vec::new();
        };
        if action_id == ActionId::OverviewHealthOpenResource {
            self.runtime_error = Some(format!(
                "{} · observed facts: {} · source: {}",
                finding.title,
                finding
                    .observed_facts
                    .iter()
                    .map(|fact| format!("{}={}", fact.label, fact.value))
                    .collect::<Vec<_>>()
                    .join(", "),
                if finding.source_ids.is_empty() {
                    "not returned".to_owned()
                } else {
                    finding.source_ids.join(", ")
                }
            ));
            match finding.rule_id.as_str() {
                "device-key-expired" | "device-key-expiring" | "device-approval-pending" => {
                    self.navigate(Route::Devices);
                }
                "user-approval-pending" => self.navigate(Route::Users),
                "route-overlap-review" => self.navigate(Route::Routes),
                _ => {}
            }
            return Vec::new();
        }
        let Some(suggested) = finding.suggested_action_ids.first() else {
            self.runtime_error = Some(
                "this derived finding has no suggested action; inspect its observed facts"
                    .to_owned(),
            );
            return Vec::new();
        };
        let action = match suggested.as_str() {
            "admin.device.approve" => ActionId::AdminDeviceApprove,
            "admin.device.key_expire_now" => ActionId::AdminDeviceKeyExpireNow,
            "admin.routes.replace_approvals" => ActionId::AdminRoutesReplaceApprovals,
            "admin.user.approve" => ActionId::AdminUserApprove,
            _ => {
                self.runtime_error = Some(format!(
                    "suggested action {suggested} is not registered in the current action catalog"
                ));
                return Vec::new();
            }
        };
        if !self.action_available_for_id(action) {
            self.runtime_error = self.action_unavailable_reason(action);
            return Vec::new();
        }
        self.dispatch_action(action)
    }

    fn open_phase_eight_local_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        let input = match action_id {
            ActionId::SavedViewCreate => {
                "name=;route=devices;columns=id,name;filter=;sort=id:ascending;wide=false"
                    .to_owned()
            }
            ActionId::SavedViewReplace => {
                "name=;route=devices;columns=id,name;filter=;sort=id:ascending;wide=false"
                    .to_owned()
            }
            ActionId::SavedViewRename => "name=;new=;".to_owned(),
            ActionId::SavedViewDelete | ActionId::SavedViewApply => "name=".to_owned(),
            ActionId::CollectionExport => "format=json;path=;collection=devices".to_owned(),
            ActionId::AccessExplorerAsk => "source=;destination=;port=;policy=current".to_owned(),
            ActionId::AccessExplorerOpenRule => {
                if let Some(result) = self.access_explorer_result.as_ref() {
                    self.runtime_error = Some(format!(
                        "authoritative preview locations: {}",
                        result
                            .rule_locations
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                } else {
                    self.runtime_error =
                        Some("no authoritative Access Explorer result is available".to_owned());
                }
                return Vec::new();
            }
            _ => String::new(),
        };
        self.overlays
            .push(Overlay::OperatorForm(OperatorFormState::new(
                action_id, input, None,
            )));
        Vec::new()
    }

    fn admin_base_snapshot(
        &self,
        change: &AdminChange,
    ) -> Result<(String, AdminSnapshotFields), String> {
        match change {
            AdminChange::DeviceRoutes { .. } => {
                let route = self.selected_admin_route().ok_or_else(|| {
                    "select a route advertiser before editing approvals".to_owned()
                })?;
                Ok((
                    route.device_id.clone(),
                    crate::admin::mutation::route_fields(&route.advertised, &route.enabled),
                ))
            }
            AdminChange::DeviceRename { .. }
            | AdminChange::DeviceTags { .. }
            | AdminChange::DeviceApproval { .. }
            | AdminChange::DeviceKeyExpiry { .. }
            | AdminChange::DeviceExpireNow
            | AdminChange::DeviceDelete => {
                let device = self
                    .selected_admin_device()
                    .ok_or_else(|| "select a verified admin device before editing it".to_owned())?;
                Ok((
                    device.stable_id.clone(),
                    crate::admin::mutation::device_fields(device),
                ))
            }
            AdminChange::UserApproval
            | AdminChange::UserRole { .. }
            | AdminChange::UserSuspend
            | AdminChange::UserRestore
            | AdminChange::UserDelete => {
                let user = self
                    .selected_admin_user()
                    .ok_or_else(|| "select a verified admin user before editing it".to_owned())?;
                Ok((user.id.clone(), crate::admin::mutation::user_fields(user)))
            }
            AdminChange::DnsNameservers { .. } => Ok((
                "tailnet".to_owned(),
                crate::admin::mutation::nameserver_fields(
                    self.admin
                        .nameservers
                        .snapshot
                        .as_ref()
                        .ok_or_else(|| "DNS nameservers are not verified".to_owned())?,
                ),
            )),
            AdminChange::DnsPreferences { .. } => Ok((
                "tailnet".to_owned(),
                crate::admin::mutation::dns_preferences_fields(
                    self.admin
                        .dns_preferences
                        .snapshot
                        .as_ref()
                        .ok_or_else(|| "DNS preferences are not verified".to_owned())?,
                ),
            )),
            AdminChange::DnsSearchPaths { .. } => Ok((
                "tailnet".to_owned(),
                crate::admin::mutation::search_path_fields(
                    self.admin
                        .search_paths
                        .snapshot
                        .as_ref()
                        .ok_or_else(|| "DNS search paths are not verified".to_owned())?,
                ),
            )),
            AdminChange::DnsSplitMapping { .. } => Ok((
                "tailnet".to_owned(),
                crate::admin::mutation::split_dns_fields(
                    self.admin
                        .split_dns
                        .snapshot
                        .as_ref()
                        .ok_or_else(|| "split DNS is not verified".to_owned())?,
                ),
            )),
        }
    }

    fn start_admin_preflight(&mut self, request: AdminMutationRequest) -> Vec<Effect> {
        if !self.admin_resource_locks.try_hold(
            request.mutation_id,
            request
                .change
                .lock_keys(&request.profile, &request.target_id),
        ) {
            self.runtime_error =
                Some("a conflicting admin mutation or read is running for this target".to_owned());
            return Vec::new();
        }
        self.admin_preflight_locks.insert(request.mutation_id);
        let Some(profile_config) = self.resolved_config.profiles.get(&request.profile) else {
            self.release_admin_preflight_lock(request.mutation_id);
            return Vec::new();
        };
        let Some(tailnet) = self.admin.tailnet.clone() else {
            self.release_admin_preflight_lock(request.mutation_id);
            return Vec::new();
        };
        vec![Effect::StartAdminPreflight {
            request,
            tailnet,
            credential: profile_config.credential.clone(),
            environment_token: self.admin_environment_token.clone(),
            timeout: self.resolved_config.admin.request_timeout,
        }]
    }

    fn release_admin_preflight_lock(&mut self, mutation_id: u64) {
        if self.admin_preflight_locks.remove(&mutation_id) {
            self.admin_resource_locks.release(mutation_id);
        }
    }

    fn release_admin_read_lock(&mut self, device_id: &str) {
        if let Some(owner) = self.admin_read_locks.remove(device_id) {
            self.admin_resource_locks.release(owner);
        }
    }

    fn release_all_admin_read_locks(&mut self) {
        let owners = self.admin_read_locks.values().copied().collect::<Vec<_>>();
        self.admin_read_locks.clear();
        for owner in owners {
            self.admin_resource_locks.release(owner);
        }
    }

    fn open_account_picker(&mut self, action_id: ActionId) -> Vec<Effect> {
        if self.local_accounts.is_empty() {
            self.runtime_error = Some("no local account profiles were returned".to_owned());
            return Vec::new();
        }
        self.overlays
            .push(Overlay::AccountPicker(AccountPickerState {
                action_id,
                accounts: self.local_accounts.clone(),
                selected: self
                    .local_accounts
                    .iter()
                    .position(|account| account.active)
                    .map_or(0, |value| value),
            }));
        Vec::new()
    }

    fn admin_policy_context(&self) -> Option<(String, String, String)> {
        let profile = self.admin.profile.clone()?;
        let tailnet = self.admin.tailnet.clone()?;
        let credential = self
            .resolved_config
            .profiles
            .get(&profile)?
            .credential
            .clone();
        Some((profile, tailnet, credential))
    }

    fn open_policy_workflow(&mut self) -> Vec<Effect> {
        if self.policy_workflow.is_some() {
            self.runtime_error = Some("a policy workflow is already open".to_owned());
            return Vec::new();
        }
        let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
            self.runtime_error = Some("an authenticated admin profile is required".to_owned());
            return Vec::new();
        };
        let workflow_id = self.next_policy_workflow_id;
        self.next_policy_workflow_id = self.next_policy_workflow_id.saturating_add(1);
        self.policy_workflow = Some(PolicyWorkflow::opening(
            workflow_id,
            profile.clone(),
            tailnet.clone(),
            self.now,
        ));
        self.overlays.push(Overlay::PolicyEditor);
        vec![Effect::StartPolicyRemoteFetch {
            workflow_id,
            profile,
            tailnet,
            credential,
            environment_token: self.admin_environment_token.clone(),
            timeout: self.resolved_config.admin.request_timeout,
        }]
    }

    fn refresh_policy_workflow(&mut self) -> Vec<Effect> {
        let Some(workflow) = self.policy_workflow.as_ref() else {
            return self.open_policy_workflow();
        };
        let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
            self.runtime_error = Some("an authenticated admin profile is required".to_owned());
            return Vec::new();
        };
        vec![Effect::StartPolicyRemoteFetch {
            workflow_id: workflow.workflow_id(),
            profile,
            tailnet,
            credential,
            environment_token: self.admin_environment_token.clone(),
            timeout: self.resolved_config.admin.request_timeout,
        }]
    }

    fn start_policy_editor(&mut self) -> Vec<Effect> {
        let Some(workflow) = self.policy_workflow.as_ref() else {
            return Vec::new();
        };
        let Some(path) = workflow.candidate_path().map(PathBuf::from) else {
            self.runtime_error = Some("the policy temporary file is unavailable".to_owned());
            return Vec::new();
        };
        let command = match crate::terminal::EditorCommand::from_environment() {
            Ok(command) => command,
            Err(error) => {
                self.runtime_error = Some(error.to_string());
                if let Some(workflow) = self.policy_workflow.as_mut() {
                    workflow.retain_failure();
                }
                return Vec::new();
            }
        };
        let workflow_id = workflow.workflow_id();
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.mark_editing_externally();
        }
        self.interactive_handoff_active = true;
        vec![Effect::StartPolicyEditor {
            workflow_id,
            command,
            path,
        }]
    }

    fn reopen_policy_editor(&mut self) -> Vec<Effect> {
        if self.policy_workflow.is_none() {
            return self.open_policy_workflow();
        }
        self.start_policy_editor()
    }

    fn discard_policy_candidate(&mut self) -> Vec<Effect> {
        let base = self
            .policy_workflow
            .as_ref()
            .and_then(PolicyWorkflow::base)
            .cloned();
        let Some(base) = base else {
            self.runtime_error = Some("the policy base is unavailable".to_owned());
            return Vec::new();
        };
        self.close_policy_temp_file();
        self.close_latest_policy_temp_file();
        match crate::temporary::TemporaryPolicyFile::create(base.bytes()) {
            Ok(file) => {
                let path = file.path().to_path_buf();
                self.policy_temp_file = Some(Arc::new(Mutex::new(file)));
                self.access_explorer_result = None;
                if let Some(workflow) = self.policy_workflow.as_mut() {
                    workflow.discard_candidate();
                    workflow.set_candidate(base, path);
                }
            }
            Err(error) => self.runtime_error = Some(error.to_string()),
        }
        Vec::new()
    }

    fn validate_policy_candidate(&mut self) -> Vec<Effect> {
        let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
            self.runtime_error = Some("an authenticated admin profile is required".to_owned());
            return Vec::new();
        };
        if !self.sync_policy_candidate_file() {
            return Vec::new();
        }
        let Some(workflow) = self.policy_workflow.as_ref() else {
            return Vec::new();
        };
        let Some(path) = workflow.candidate_path().map(PathBuf::from) else {
            self.runtime_error = Some("the policy candidate is unavailable".to_owned());
            return Vec::new();
        };
        let workflow_id = workflow.workflow_id();
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.mark_validating();
        }
        vec![Effect::StartPolicyValidate {
            workflow_id,
            profile,
            tailnet,
            credential,
            environment_token: self.admin_environment_token.clone(),
            timeout: self.resolved_config.admin.request_timeout,
            path,
        }]
    }

    fn preview_policy_candidate(&mut self) -> Vec<Effect> {
        let selector = self
            .selected_admin_user()
            .map_or_else(|| "autogroup:members".to_owned(), |user| user.id.clone());
        self.overlays
            .push(Overlay::OperatorForm(OperatorFormState::new(
                ActionId::AdminPolicyPreview,
                format!("type=user;previewFor={selector}"),
                None,
            )));
        Vec::new()
    }

    fn start_policy_preview(
        &mut self,
        selector_type: PolicySelectorType,
        selector: String,
    ) -> Vec<Effect> {
        let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
            self.runtime_error = Some("an authenticated admin profile is required".to_owned());
            return Vec::new();
        };
        if !self.sync_policy_candidate_file() {
            return Vec::new();
        }
        let Some(workflow) = self.policy_workflow.as_ref() else {
            return Vec::new();
        };
        let Some(path) = workflow.candidate_path().map(PathBuf::from) else {
            self.runtime_error = Some("the policy candidate is unavailable".to_owned());
            return Vec::new();
        };
        let workflow_id = workflow.workflow_id();
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.mark_previewing();
        }
        vec![Effect::StartPolicyPreview {
            workflow_id,
            profile,
            tailnet,
            credential,
            environment_token: self.admin_environment_token.clone(),
            timeout: self.resolved_config.admin.request_timeout,
            path,
            selector_type,
            selector,
        }]
    }

    fn diff_policy_candidate(&mut self) -> Vec<Effect> {
        if !self.sync_policy_candidate_file() {
            return Vec::new();
        }
        let Some(workflow) = self.policy_workflow.as_mut() else {
            return Vec::new();
        };
        let Some((base, candidate)) = workflow.base().zip(workflow.candidate()) else {
            self.runtime_error = Some("both policy base and candidate are required".to_owned());
            return Vec::new();
        };
        match crate::admin::policy_mutations::build_policy_diff(base, candidate) {
            Ok(diff) => {
                let _ = workflow.set_diff(diff);
            }
            Err(error) => self.runtime_error = Some(error.to_string()),
        }
        Vec::new()
    }

    fn open_policy_apply_confirmation(&mut self) -> Vec<Effect> {
        if !self.sync_policy_candidate_file() {
            return Vec::new();
        }
        let Some(workflow) = self.policy_workflow.as_ref() else {
            return Vec::new();
        };
        if let Err(error) = workflow.apply_guard(self.now) {
            self.runtime_error = Some(error.to_string());
            return Vec::new();
        }
        let Some(candidate) = workflow.candidate() else {
            return Vec::new();
        };
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: ActionId::AdminPolicyApply,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: None,
                handoff: None,
                prompt: "Apply this exact policy candidate to the remote tailnet?".to_owned(),
                required_phrase: Some("APPLY POLICY".to_owned()),
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines: vec![
                    format!(
                        "base hash: {}",
                        workflow.base().map_or("not returned", |value| value.hash())
                    ),
                    format!("candidate hash: {}", candidate.hash()),
                    format!(
                        "base observed: {}",
                        workflow
                            .base()
                            .map_or("not returned".to_owned(), |value| value
                                .observed_at()
                                .to_string())
                    ),
                    format!("candidate observed: {}", candidate.observed_at()),
                    format!("candidate bytes: {}", candidate.len()),
                    format!("validation bound: {}", workflow.validation().is_some()),
                    format!(
                        "validation/tests: {}",
                        workflow.validation().map_or_else(
                            || "not returned".to_owned(),
                            |value| if value.valid {
                                "server passed".to_owned()
                            } else {
                                "server failed".to_owned()
                            }
                        )
                    ),
                    format!("permission preview bound: {}", workflow.preview().is_some()),
                    format!(
                        "diff: {}",
                        workflow.diff().map_or_else(
                            || "not computed; press d for the complete textual diff".to_owned(),
                            |value| format!(
                                "+{} -{}; press d for the complete textual diff",
                                value.additions, value.removals
                            )
                        )
                    ),
                    "final server validation runs immediately before one save request".to_owned(),
                    "remote bytes are fetched and compared after save".to_owned(),
                    "the final hash check is not a server-atomic compare-and-swap".to_owned(),
                ],
                redacted_argv: Vec::new(),
                error: None,
            })));
        Vec::new()
    }

    fn sync_policy_candidate_file(&mut self) -> bool {
        let Some((path, expected_hash, content_type)) =
            self.policy_workflow.as_ref().and_then(|workflow| {
                workflow
                    .candidate()
                    .zip(workflow.candidate_path())
                    .map(|(candidate, path)| {
                        (
                            path.to_path_buf(),
                            candidate.hash().to_owned(),
                            candidate.content_type().to_owned(),
                        )
                    })
            })
        else {
            return true;
        };
        let bytes = match crate::temporary::TemporaryPolicyFile::read_candidate_path(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                if let Some(workflow) = self.policy_workflow.as_mut() {
                    workflow.retain_failure();
                }
                self.runtime_error = Some(error.to_string());
                return false;
            }
        };
        if crate::domain::policy_workflow::hash_bytes(&bytes) == expected_hash {
            return true;
        }
        let document =
            match crate::domain::policy_workflow::PolicyDocument::from_bytes_with_content_type(
                bytes,
                content_type,
                self.now,
            ) {
                Ok(document) => document,
                Err(error) => {
                    if let Some(workflow) = self.policy_workflow.as_mut() {
                        workflow.retain_failure();
                    }
                    self.runtime_error = Some(error.to_string());
                    return false;
                }
            };
        self.access_explorer_result = None;
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.set_candidate(document, path);
        }
        self.runtime_error = Some(
            "the temporary candidate changed; validation, preview, and diff were invalidated"
                .to_owned(),
        );
        false
    }

    fn open_policy_discard_confirmation(&mut self) -> Vec<Effect> {
        let Some(workflow) = self.policy_workflow.as_ref() else {
            self.runtime_error = Some("the policy workflow is not open".to_owned());
            return Vec::new();
        };
        let replacing_remote =
            workflow.latest_remote().is_some() && workflow.state() == PolicyState::RemoteConflict;
        let phrase = if replacing_remote {
            "REPLACE POLICY CANDIDATE"
        } else {
            "DISCARD POLICY CANDIDATE"
        };
        let mut preview_lines = vec![
            format!(
                "base hash: {}",
                workflow.base().map_or("not returned", |value| value.hash())
            ),
            format!(
                "candidate hash: {}",
                workflow
                    .candidate()
                    .map_or("not returned", |value| value.hash())
            ),
            format!(
                "candidate path: {}",
                workflow
                    .candidate_path()
                    .map_or("not retained".to_owned(), |value| value
                        .display()
                        .to_string())
            ),
        ];
        if replacing_remote {
            preview_lines.extend([
                format!(
                    "latest remote hash: {}",
                    workflow
                        .latest_remote()
                        .map_or("not returned", |value| value.hash())
                ),
                format!(
                    "latest remote path: {}",
                    workflow
                        .latest_remote_path()
                        .map_or("not retained".to_owned(), |value| value
                            .display()
                            .to_string())
                ),
                "replace candidate with latest remote bytes; no merge will be attempted".to_owned(),
            ]);
        } else {
            preview_lines
                .push("the candidate will be replaced with the unchanged base bytes".to_owned());
        }
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: ActionId::AdminPolicyCandidateDiscard,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: None,
                handoff: None,
                prompt: if replacing_remote {
                    "Replace the retained candidate with the latest remote policy?".to_owned()
                } else {
                    "Discard the retained policy candidate?".to_owned()
                },
                required_phrase: Some(phrase.to_owned()),
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines,
                redacted_argv: Vec::new(),
                error: None,
            })));
        Vec::new()
    }

    fn open_policy_close_confirmation(&mut self) -> Vec<Effect> {
        let Some(workflow) = self.policy_workflow.as_ref() else {
            return Vec::new();
        };
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: ActionId::AdminPolicyWorkflowClose,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: None,
                handoff: None,
                prompt: "Close the policy workflow and remove its temporary files?".to_owned(),
                required_phrase: Some("CLOSE POLICY WORKFLOW".to_owned()),
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines: vec![
                    format!("state: {}", workflow.state().label()),
                    format!(
                        "candidate path: {}",
                        workflow
                            .candidate_path()
                            .map_or("not retained".to_owned(), |value| value
                                .display()
                                .to_string())
                    ),
                    "closing destroys the candidate and any retained latest-remote copy".to_owned(),
                ],
                redacted_argv: Vec::new(),
                error: None,
            })));
        Vec::new()
    }

    fn replace_policy_candidate_with_latest(&mut self) -> Vec<Effect> {
        let Some(latest) = self
            .policy_workflow
            .as_ref()
            .and_then(PolicyWorkflow::latest_remote)
            .cloned()
        else {
            self.runtime_error = Some("the latest remote policy is unavailable".to_owned());
            return Vec::new();
        };
        self.close_policy_temp_file();
        let file = match crate::temporary::TemporaryPolicyFile::create(latest.bytes()) {
            Ok(file) => file,
            Err(error) => {
                self.runtime_error = Some(error.to_string());
                return Vec::new();
            }
        };
        let path = file.path().to_path_buf();
        self.policy_temp_file = Some(Arc::new(Mutex::new(file)));
        self.close_latest_policy_temp_file();
        self.access_explorer_result = None;
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.set_base(latest.clone());
            workflow.set_candidate(latest, path);
        }
        Vec::new()
    }

    fn close_policy_workflow(&mut self) -> Vec<Effect> {
        self.close_policy_temp_file();
        self.close_latest_policy_temp_file();
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.close();
        }
        self.policy_workflow = None;
        self.pending_auth_key_request = None;
        self.pending_credential_revoke = None;
        self.overlays
            .retain(|overlay| !matches!(overlay, Overlay::PolicyEditor));
        Vec::new()
    }

    fn close_policy_temp_file(&mut self) {
        if let Some(file) = self.policy_temp_file.take() {
            match file.lock() {
                Ok(mut file) => {
                    if let Err(error) = file.close() {
                        self.runtime_error = Some(error.to_string());
                    }
                }
                Err(_) => {
                    self.runtime_error =
                        Some("policy temporary storage could not be locked".to_owned())
                }
            }
        }
    }

    fn close_latest_policy_temp_file(&mut self) {
        if let Some(file) = self.latest_policy_temp_file.take() {
            match file.lock() {
                Ok(mut file) => {
                    if let Err(error) = file.close() {
                        self.runtime_error = Some(error.to_string());
                    }
                }
                Err(_) => {
                    self.runtime_error =
                        Some("latest remote policy storage could not be locked".to_owned());
                }
            }
        }
    }

    fn open_auth_key_confirmation(&mut self) -> Vec<Effect> {
        self.overlays
            .push(Overlay::OperatorForm(OperatorFormState::new(
                ActionId::AdminCredentialAuthKeyCreate,
                "description=tale-generated;expiry=7d;reusable=false;ephemeral=true;preauthorized=false;tags="
                    .to_owned(),
                None,
            )));
        Vec::new()
    }

    fn open_auth_key_form_with_request(
        &mut self,
        request: crate::admin::key_mutations::AuthKeyCreateRequest,
    ) -> Vec<Effect> {
        if let Err(error) = request.validate() {
            self.runtime_error = Some(error.to_string());
            return Vec::new();
        }
        self.pending_auth_key_request = Some(request.clone());
        let expiry_days = request.expiry_seconds / (24 * 60 * 60);
        let tags = request.tags.join(",");
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: ActionId::AdminCredentialAuthKeyCreate,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: None,
                handoff: None,
                prompt:
                    "Create this auth key? The secret will be shown once and cannot be recovered."
                        .to_owned(),
                required_phrase: Some("CREATE AUTH KEY".to_owned()),
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines: vec![
                    format!(
                        "profile: {}",
                        self.admin.profile.as_deref().unwrap_or("not selected")
                    ),
                    format!(
                        "tailnet: {}",
                        self.admin.tailnet.as_deref().unwrap_or("not selected")
                    ),
                    "endpoint: POST /tailnet/{tailnet}/keys".to_owned(),
                    "scope: auth_keys".to_owned(),
                    "type: auth".to_owned(),
                    format!(
                        "description: {}",
                        request.description.as_deref().unwrap_or("none")
                    ),
                    format!("expiry: {expiry_days} days"),
                    format!("reusable: {}", request.reusable),
                    format!("ephemeral: {}", request.ephemeral),
                    format!("preauthorized: {}", request.preauthorized),
                    format!(
                        "tags: {}",
                        if tags.is_empty() {
                            "none"
                        } else {
                            tags.as_str()
                        }
                    ),
                    format!(
                        "expires at: {}",
                        self.now.saturating_add(request.expiry_seconds)
                    ),
                ],
                redacted_argv: Vec::new(),
                error: None,
            })));
        Vec::new()
    }

    fn selected_credential(&self) -> Option<&crate::domain::credential::CredentialMetadata> {
        self.admin
            .credentials
            .snapshot
            .as_ref()?
            .records
            .get(self.admin_credential_selected)
    }

    fn open_credential_revoke_confirmation(&mut self) -> Vec<Effect> {
        let Some(credential) = self.selected_credential() else {
            self.runtime_error = Some("select a credential before revoking it".to_owned());
            return Vec::new();
        };
        let credential_type = crate::admin::key_mutations::remote_credential_type(credential);
        if !credential_type.supported_for_revoke() {
            self.runtime_error = Some(
                "the selected credential type has no supported Phase 7 revocation contract"
                    .to_owned(),
            );
            return Vec::new();
        }
        let Some(read_scope) = credential_type.read_scope() else {
            self.runtime_error = Some("the selected credential read scope is unknown".to_owned());
            return Vec::new();
        };
        let Some(write_scope) = credential_type.write_scope() else {
            self.runtime_error = Some("the selected credential write scope is unknown".to_owned());
            return Vec::new();
        };
        if !self.admin_scope_allowed(read_scope) || !self.admin_scope_allowed(write_scope) {
            self.runtime_error = Some(format!(
                "revocation requires the selected credential's {read_scope} and {write_scope} scopes"
            ));
            return Vec::new();
        }
        let key_id = credential.id.clone();
        let Some((profile, tailnet, credential_reference)) = self.admin_policy_context() else {
            self.runtime_error = Some("an authenticated admin profile is required".to_owned());
            return Vec::new();
        };
        self.pending_credential_revoke = Some(key_id.clone());
        vec![Effect::StartCredentialDetail {
            key_id,
            profile,
            tailnet,
            credential: credential_reference,
            environment_token: self.admin_environment_token.clone(),
            timeout: self.resolved_config.admin.request_timeout,
        }]
    }

    fn open_credential_revoke_with_metadata(
        &mut self,
        credential: crate::domain::credential::CredentialMetadata,
    ) -> Vec<Effect> {
        let credential_type = crate::admin::key_mutations::remote_credential_type(&credential);
        if !credential_type.supported_for_revoke() {
            self.runtime_error = Some(
                "the selected credential type has no supported Phase 7 revocation contract"
                    .to_owned(),
            );
            return Vec::new();
        }
        let Some(read_scope) = credential_type.read_scope() else {
            self.runtime_error = Some("the selected credential read scope is unknown".to_owned());
            return Vec::new();
        };
        let Some(write_scope) = credential_type.write_scope() else {
            self.runtime_error = Some("the selected credential write scope is unknown".to_owned());
            return Vec::new();
        };
        if !self.admin_scope_allowed(read_scope) || !self.admin_scope_allowed(write_scope) {
            self.runtime_error = Some(format!(
                "revocation requires the selected credential's {read_scope} and {write_scope} scopes"
            ));
            return Vec::new();
        }
        if credential.invalid == Some(true) || credential.revoked_at.is_some() {
            self.runtime_error = Some("the credential is already invalid or revoked".to_owned());
            return Vec::new();
        }
        let phrase = format!("REVOKE {}", credential.id);
        self.pending_credential_revoke = Some(credential.id.clone());
        let references = self
            .resolved_config
            .profiles
            .iter()
            .filter(|(_, profile)| profile.credential == credential.id)
            .map(|(profile, _)| format!("{profile} -> {}", credential.id))
            .collect::<Vec<_>>();
        let display_list = |values: &[String]| {
            if values.is_empty() {
                "none returned".to_owned()
            } else {
                values.join(",")
            }
        };
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
            action_id: ActionId::AdminCredentialRevoke,
            mutation: None,
            admin_mutation: None,
            admin_batch: None,
            service_request: None,
            operational_mutation: None,
            handoff: None,
            prompt:
                "Revoke this remote credential? Tale will issue one DELETE and then read it back."
                    .to_owned(),
            required_phrase: Some(phrase),
            input: String::new(),
            lose_ssh_checked: false,
            preview_lines: vec![
                format!("id: {}", credential.id),
                format!("type: {}", credential.key_type),
                format!(
                    "description: {}",
                    credential
                        .description
                        .as_deref()
                        .map_or("not returned", |value| value)
                ),
                format!(
                    "owner: {}",
                    credential
                        .user_id
                        .as_deref()
                        .map_or("not returned", |value| value)
                ),
                format!(
                    "created: {}",
                    credential
                        .created_at
                        .map_or_else(|| "not returned".to_owned(), |value| value.to_string())
                ),
                format!(
                    "expires: {}",
                    credential
                        .expires_at
                        .map_or_else(|| "not returned".to_owned(), |value| value.to_string())
                ),
                format!(
                    "last used: {}",
                    credential
                        .last_used_at
                        .map_or_else(|| "not returned".to_owned(), |value| value.to_string())
                ),
                format!("scopes: {}", display_list(&credential.scopes)),
                format!("tags: {}", display_list(&credential.tags)),
                format!(
                    "known dependents: {}",
                    display_list(&credential.known_dependents)
                ),
                format!(
                    "known Tale profile references: {}",
                    if references.is_empty() {
                        "none".to_owned()
                    } else {
                        references.join(", ")
                    }
                ),
                "remote revocation and local keyring removal are separate actions".to_owned(),
            ],
            redacted_argv: vec!["DELETE /tailnet/{tailnet}/keys/{exact-id}".to_owned()],
            error: None,
        })));
        Vec::new()
    }

    fn open_profile_credential_confirmation(&mut self) -> Vec<Effect> {
        let Some(profile) = self.admin.profile.clone() else {
            self.runtime_error = Some("an active profile is required".to_owned());
            return Vec::new();
        };
        let Some(configuration) = self.resolved_config.profiles.get(&profile) else {
            self.runtime_error = Some("the active profile configuration is unavailable".to_owned());
            return Vec::new();
        };
        self.overlays.push(Overlay::Confirmation(Box::new(ConfirmationState {
            action_id: ActionId::ProfileCredentialRemove,
            mutation: None,
            admin_mutation: None,
            admin_batch: None,
            service_request: None,
            operational_mutation: None,
            handoff: None,
            prompt: "Remove this local Tale credential from the OS keyring? This does not revoke any remote credential.".to_owned(),
            required_phrase: Some("REMOVE LOCAL CREDENTIAL".to_owned()),
            input: String::new(),
            lose_ssh_checked: false,
            preview_lines: vec![format!("profile: {profile}"), format!("keyring reference: {}", configuration.credential)],
            redacted_argv: Vec::new(),
            error: None,
        })));
        Vec::new()
    }

    fn open_audit_investigation(&mut self) -> Vec<Effect> {
        self.overlays.push(Overlay::AuditInvestigation);
        Vec::new()
    }

    fn open_audit_filter(&mut self, action_id: ActionId) -> Vec<Effect> {
        let input = match action_id {
            ActionId::AuditFilterTime => format!(
                "start={};end={}",
                self.audit_filters
                    .start
                    .map_or(String::new(), format_audit_timestamp),
                self.audit_filters
                    .end
                    .map_or(String::new(), format_audit_timestamp)
            ),
            ActionId::AuditFilterActor => format!(
                "id={};display={}",
                self.audit_filters.actor_id.as_deref().unwrap_or(""),
                self.audit_filters.actor_display.as_deref().unwrap_or("")
            ),
            ActionId::AuditFilterAction => format!(
                "action={}",
                self.audit_filters.action.as_deref().unwrap_or("")
            ),
            ActionId::AuditFilterTarget => format!(
                "type={};id={};text={}",
                self.audit_filters.target_type.as_deref().unwrap_or(""),
                self.audit_filters.target_id.as_deref().unwrap_or(""),
                self.audit_filters.text.as_deref().unwrap_or("")
            ),
            _ => String::new(),
        };
        self.overlays
            .push(Overlay::OperatorForm(OperatorFormState::new(
                action_id, input, None,
            )));
        Vec::new()
    }

    fn accept_audit_filter(&mut self, state: OperatorFormState) -> Vec<Effect> {
        let parsed = parse_audit_filter(state.action_id, &state.input);
        match parsed {
            Ok(filters) => {
                match state.action_id {
                    ActionId::AuditFilterTime => {
                        self.audit_filters.start = filters.start;
                        self.audit_filters.end = filters.end;
                    }
                    ActionId::AuditFilterActor => {
                        self.audit_filters.actor_id = filters.actor_id;
                        self.audit_filters.actor_display = filters.actor_display;
                    }
                    ActionId::AuditFilterAction => self.audit_filters.action = filters.action,
                    ActionId::AuditFilterTarget => {
                        self.audit_filters.target_type = filters.target_type;
                        self.audit_filters.target_id = filters.target_id;
                        self.audit_filters.text = filters.text;
                    }
                    _ => {}
                }
                self.admin_activity_selected = 0;
                self.overlays.pop();
                self.open_audit_investigation()
            }
            Err(error) => {
                if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                    current.error = Some(error);
                }
                Vec::new()
            }
        }
    }

    fn copy_secret_result(&mut self) -> Vec<Effect> {
        let Some(result) = self.secret_result.as_mut() else {
            self.runtime_error = Some("no one-time secret is open".to_owned());
            return Vec::new();
        };
        let result_id = result.metadata().result_id;
        let Some(secret) = result.mark_copy_requested() else {
            self.runtime_error = Some("the one-time secret has already been closed".to_owned());
            return Vec::new();
        };
        vec![Effect::CopySecret { result_id, secret }]
    }

    fn close_secret_result(&mut self) -> Vec<Effect> {
        if let Some(result) = self.secret_result.as_mut() {
            result.close();
        }
        self.secret_result = None;
        self.overlays
            .retain(|overlay| !matches!(overlay, Overlay::SecretResult));
        if self.admin.profile.is_some() {
            self.start_admin_resource_refresh(vec![AdminRefreshResource::Credentials])
        } else {
            Vec::new()
        }
    }

    fn open_login_confirmation(&mut self) -> Vec<Effect> {
        let Some(executable) = self.local_executable.as_ref() else {
            self.runtime_error = Some("local executable has not been discovered".to_owned());
            return Vec::new();
        };
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: ActionId::LocalAccountLogin,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: None,
                handoff: Some(local_handoff_command(
                    handoff::login_command(&executable.path),
                    executable.socket_path.as_deref(),
                )),
                prompt: "Open Tailscale login in the terminal; Tale will not collect credentials."
                    .to_owned(),
                required_phrase: None,
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines: vec!["login runs in the inherited terminal".to_owned()],
                redacted_argv: vec!["login".to_owned()],
                error: None,
            })));
        Vec::new()
    }

    fn open_logout_confirmation(&mut self) -> Vec<Effect> {
        let Some(executable) = self.local_executable.as_ref() else {
            self.runtime_error = Some("local executable has not been discovered".to_owned());
            return Vec::new();
        };
        self.overlays.push(Overlay::Confirmation(Box::new(ConfirmationState {
            action_id: ActionId::LocalAccountLogout,
            mutation: None,
            admin_mutation: None,
            admin_batch: None,
            service_request: None,
            operational_mutation: None,
            handoff: Some(local_handoff_command(
                handoff::logout_command(&executable.path),
                executable.socket_path.as_deref(),
            )),
            prompt: "Log out this local account; the node key will be invalidated and reauthentication will be required.".to_owned(),
            required_phrase: Some("LOGOUT".to_owned()),
            input: String::new(),
            lose_ssh_checked: false,
            preview_lines: vec!["logout invalidates the current local node key".to_owned()],
            redacted_argv: vec!["logout".to_owned()],
            error: None,
        })));
        Vec::new()
    }

    fn open_handoff_input(&mut self, kind: HandoffInputKind) -> Vec<Effect> {
        let Some(host) = self
            .selected_local_device()
            .and_then(LocalDevice::preferred_target)
            .map(str::to_owned)
        else {
            self.runtime_error = Some("selected device has no DNS name or Tailscale IP".to_owned());
            return Vec::new();
        };
        self.overlays.push(Overlay::HandoffInput(HandoffInputState {
            kind,
            host,
            input: match kind {
                HandoffInputKind::Ssh => String::new(),
                HandoffInputKind::Nc => "443".to_owned(),
            },
            error: None,
        }));
        Vec::new()
    }

    fn open_mutation_confirmation(&mut self, mutation: LocalMutation) -> Vec<Effect> {
        if let Err(error) = self.validate_mutation_request(&mutation) {
            self.runtime_error = Some(error);
            return Vec::new();
        }
        let (prompt, required_phrase, lose_ssh_checked) = match &mutation {
            LocalMutation::Connect => (
                "Connect this local node without changing existing preferences.".to_owned(),
                None,
                false,
            ),
            LocalMutation::Disconnect { .. } => (
                "Disconnect this local node. Connectivity will stop and may terminate a terminal session over Tailscale.".to_owned(),
                Some("DISCONNECT".to_owned()),
                false,
            ),
            LocalMutation::SyspolicyReload => (
                "Reload local system policy and verify it with a fresh policy read.".to_owned(),
                None,
                false,
            ),
            LocalMutation::Preferences(_) => (
                "Apply the submitted local preference fields and verify fresh daemon state.".to_owned(),
                None,
                false,
            ),
            LocalMutation::ExitNode(_) => (
                "Change the exit-node selection on this local node only.".to_owned(),
                None,
                false,
            ),
            LocalMutation::Advertisements(_) => (
                "This device will advertise; a tailnet administrator may still need to approve the route.".to_owned(),
                match &mutation {
                    LocalMutation::Advertisements(request)
                        if request.accept_mac_app_connector_risk =>
                    {
                        Some("MAC-APP-CONNECTOR".to_owned())
                    }
                    _ => None,
                },
                false,
            ),
            LocalMutation::AccountSwitch { .. } => (
                "Switch this local client profile and clear the current tailnet selection.".to_owned(),
                None,
                false,
            ),
            LocalMutation::AccountRemove { account_id } => {
                let label = self
                    .local_accounts
                    .iter()
                    .find(|account| account.id == *account_id)
                    .map_or_else(|| account_id.clone(), |account| account.display_label().to_owned());
                (
                    format!("Remove the local account profile {label}. This does not delete the Tailscale account or user."),
                    Some(label),
                    false,
                )
            }
        };
        let preview_lines = self.mutation_preview_lines(&mutation);
        let redacted_argv = mutation_metadata(
            self.local_executable
                .as_ref()
                .map_or(std::path::Path::new("tailscale"), |value| {
                    value.path.as_path()
                }),
            &mutation,
            self.resolved_config.local.command_timeout,
        )
        .1;
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: mutation.action_id(),
                mutation: Some(mutation),
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: None,
                handoff: None,
                prompt,
                required_phrase,
                input: String::new(),
                lose_ssh_checked,
                preview_lines,
                redacted_argv,
                error: None,
            })));
        Vec::new()
    }

    fn validate_mutation_request(&self, mutation: &LocalMutation) -> Result<(), String> {
        match mutation {
            LocalMutation::Disconnect { .. }
                if policy_forces(&self.system_policy, "AlwaysOn.Enabled")
                    || policy_forces(&self.system_policy, "ForceEnabled") =>
            {
                Err("disconnect is blocked by the local always-on system policy".to_owned())
            }
            LocalMutation::Preferences(request) => {
                if request.is_empty() {
                    return Err("at least one preference field must be changed".to_owned());
                }
                for field in request.changed_fields() {
                    if !preference_field_editable(&self.local_preferences, field) {
                        return Err(format!(
                            "{} is unknown, policy managed, or unsupported",
                            field.label()
                        ));
                    }
                }
                Ok(())
            }
            LocalMutation::ExitNode(_) if policy_disallows_exit_override(&self.system_policy) => {
                Err("exit-node selection is blocked by the local system policy".to_owned())
            }
            LocalMutation::ExitNode(_) => {
                if !self.local_preferences.exit_node_allow_lan_access.can_edit()
                    || !self.local_preferences.auto_exit_node.can_edit()
                    || !self.local_preferences.exit_node_id.can_edit()
                    || !self.local_preferences.exit_node_ip.can_edit()
                {
                    Err("exit-node current state is incomplete or not editable".to_owned())
                } else {
                    Ok(())
                }
            }
            LocalMutation::Advertisements(request) => {
                if request.is_empty() {
                    return Err("at least one advertisement field must be changed".to_owned());
                }
                if request.advertise_connector == Some(true)
                    && !request.accept_mac_app_connector_risk
                {
                    return Err(
                        "enabling the app connector requires accept-risk=mac-app-connector"
                            .to_owned(),
                    );
                }
                if request.accept_mac_app_connector_risk
                    && request.advertise_connector != Some(true)
                {
                    return Err(
                        "mac-app-connector risk acceptance requires connector=true".to_owned()
                    );
                }
                if request.advertise_exit_node.is_some()
                    && policy_forces(&self.system_policy, "AdvertiseExitNode")
                {
                    return Err(
                        "exit-node advertisement is controlled by the local system policy"
                            .to_owned(),
                    );
                }
                if request.routes.is_some() && !self.local_preferences.advertised_routes.can_edit()
                {
                    return Err("advertised routes are unknown or not editable".to_owned());
                }
                if request.advertise_exit_node.is_some()
                    && !self.local_preferences.advertised_exit_node.can_edit()
                {
                    return Err("advertised exit-node state is unknown or not editable".to_owned());
                }
                if request.advertise_connector.is_some()
                    && !self.local_preferences.app_connector.can_edit()
                {
                    return Err("app-connector state is unknown or not editable".to_owned());
                }
                if request.relay_server_port.is_some()
                    && !self.local_preferences.relay_server_port_disabled.can_edit()
                {
                    return Err("relay-server port state is unknown or not editable".to_owned());
                }
                if request.relay_server_static_endpoints.is_some()
                    && !self
                        .local_preferences
                        .relay_server_static_endpoints
                        .can_edit()
                {
                    return Err("relay-server endpoints are unknown or not editable".to_owned());
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn accept_operator_form(&mut self, state: OperatorFormState) -> Vec<Effect> {
        if matches!(
            state.action_id,
            ActionId::AuditFilterTime
                | ActionId::AuditFilterActor
                | ActionId::AuditFilterAction
                | ActionId::AuditFilterTarget
        ) {
            return self.accept_audit_filter(state);
        }
        if state.action_id == ActionId::AdminPolicyPreview {
            return match parse_policy_preview_request(&state.input) {
                Ok((selector_type, selector)) => {
                    self.overlays.pop();
                    self.start_policy_preview(selector_type, selector)
                }
                Err(error) => {
                    if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                        current.error = Some(error);
                    }
                    Vec::new()
                }
            };
        }
        if state.action_id == ActionId::AdminCredentialAuthKeyCreate {
            return match parse_auth_key_request(&state.input) {
                Ok(request) => {
                    self.overlays.pop();
                    self.open_auth_key_form_with_request(request)
                }
                Err(error) => {
                    if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                        current.error = Some(error);
                    }
                    Vec::new()
                }
            };
        }
        if state.action_id == ActionId::ActivityFlowsSelectWindow {
            return match parse_flow_window_form(&state.input, self.now) {
                Ok((window, filter)) => {
                    let mut filter = filter;
                    if let Err(error) = self.resolve_flow_filter_labels(&mut filter) {
                        if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                            current.error = Some(error);
                        }
                        return Vec::new();
                    }
                    self.overlays.pop();
                    self.cancel_flow_aggregation();
                    self.flow_aggregation_generation =
                        self.flow_aggregation_generation.saturating_add(1);
                    self.flow_filter = filter;
                    self.flow_snapshot = None;
                    self.flow_generation.begin();
                    self.start_admin_resource_refresh(vec![AdminRefreshResource::FlowLogs(window)])
                }
                Err(error) => {
                    if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                        current.error = Some(error);
                    }
                    Vec::new()
                }
            };
        }
        if matches!(
            state.action_id,
            ActionId::AdminWebhookCreate
                | ActionId::AdminWebhookEdit
                | ActionId::AdminLogStreamReplace
                | ActionId::AdminNetworkLogsSettings
        ) {
            return self.accept_phase_eight_form(state);
        }
        if matches!(
            state.action_id,
            ActionId::SavedViewCreate
                | ActionId::SavedViewReplace
                | ActionId::SavedViewRename
                | ActionId::SavedViewDelete
                | ActionId::SavedViewApply
                | ActionId::CollectionExport
        ) {
            return self.accept_phase_eight_local_form(state);
        }
        if state.action_id == ActionId::AccessExplorerAsk {
            return self.accept_access_explorer_form(state);
        }
        if is_admin_mutation_action(state.action_id) {
            return self.accept_admin_form(state);
        }
        let result = match state.action_id {
            ActionId::LocalPreferencesEdit => {
                parse_preference_request(&state.input).map(LocalMutation::Preferences)
            }
            ActionId::LocalExitNodeSelect => self
                .parse_exit_node_request(&state.input)
                .map(LocalMutation::ExitNode),
            ActionId::LocalRoutesEditAdvertisements => {
                parse_advertisement_request(&state.input).map(LocalMutation::Advertisements)
            }
            _ => Err("this form is not a local operator form".to_owned()),
        };
        match result {
            Ok(mutation) => {
                self.overlays.pop();
                self.open_mutation_confirmation(mutation)
            }
            Err(error) => {
                if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                    current.error = Some(error);
                }
                Vec::new()
            }
        }
    }

    fn accept_phase_eight_form(&mut self, state: OperatorFormState) -> Vec<Effect> {
        let result = match state.action_id {
            ActionId::AdminWebhookCreate => parse_webhook_create(&state.input),
            ActionId::AdminWebhookEdit => self.parse_webhook_edit(&state.input),
            ActionId::AdminLogStreamReplace => {
                parse_log_stream_draft(&state.input, state.secret_input.as_ref())
            }
            ActionId::AdminNetworkLogsSettings => parse_network_log_setting(&state.input),
            _ => Err("this is not a Phase 8 operational form".to_owned()),
        };
        match result {
            Ok(mutation) => {
                self.overlays.pop();
                self.open_operational_confirmation(state.action_id, mutation)
            }
            Err(error) => {
                if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                    current.error = Some(error);
                }
                Vec::new()
            }
        }
    }

    fn parse_webhook_edit(&self, input: &str) -> Result<OperationalMutation, String> {
        let endpoint = self
            .selected_webhook()
            .ok_or_else(|| "no observed webhook is available".to_owned())?;
        let fields = parse_operational_fields(input)?;
        ensure_operational_fields(&fields, &["categories", "events"])?;
        let categories = csv_field(&fields, "categories");
        let events = csv_field(&fields, "events");
        let after = endpoint
            .subscriptions
            .edit_known(categories, events)
            .map_err(|error| error.to_string())?;
        Ok(OperationalMutation::Webhook(
            WebhookMutation::EditSubscriptions {
                endpoint_id: endpoint.stable_id.clone(),
                endpoint_url: endpoint.endpoint_url.clone(),
                destination_type: endpoint.destination_type.clone(),
                before: endpoint.subscriptions.clone(),
                after,
            },
        ))
    }

    fn resolve_flow_filter_labels(&self, filter: &mut FlowFilter) -> Result<(), String> {
        let Some(devices) = self.admin.devices.snapshot.as_ref() else {
            if filter.reporting_node_label.is_some()
                || filter.source_node_label.is_some()
                || filter.destination_node_label.is_some()
            {
                return Err(
                    "flow label filters require an observed device snapshot for exact ID resolution"
                        .to_owned(),
                );
            }
            return Ok(());
        };
        let resolve = |label: &mut Option<String>| -> Result<Option<String>, String> {
            let Some(label) = label.as_deref() else {
                return Ok(None);
            };
            let matches = devices
                .iter()
                .filter(|device| device.display_name() == label)
                .map(|device| device.stable_id.clone())
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [stable_id] => Ok(Some(stable_id.clone())),
                [] => Err(format!(
                    "flow label {label} was not returned by the device source"
                )),
                _ => Err(format!(
                    "flow label {label} is ambiguous; use a stable node ID"
                )),
            }
        };
        if filter.reporting_node_id.is_none() {
            filter.reporting_node_id = resolve(&mut filter.reporting_node_label)?;
        }
        if filter.source_node_id.is_none() {
            filter.source_node_id = resolve(&mut filter.source_node_label)?;
        }
        if filter.destination_node_id.is_none() {
            filter.destination_node_id = resolve(&mut filter.destination_node_label)?;
        }
        Ok(())
    }

    fn cancel_flow_aggregation(&mut self) {
        if let Some(cancellation) = self.flow_aggregation_cancellation.take() {
            cancellation.store(true, Ordering::Relaxed);
        }
    }

    fn accept_phase_eight_local_form(&mut self, state: OperatorFormState) -> Vec<Effect> {
        let result = match state.action_id {
            ActionId::SavedViewCreate | ActionId::SavedViewReplace => {
                parse_saved_view_form(&state.input).map(|view| {
                    if state.action_id == ActionId::SavedViewCreate {
                        OperationalMutation::SavedView(SavedViewMutation::Create(view))
                    } else {
                        OperationalMutation::SavedView(SavedViewMutation::Replace {
                            name: view.name.clone(),
                            view,
                        })
                    }
                })
            }
            ActionId::SavedViewRename => {
                parse_rename_form(&state.input).map(|(name, replacement)| {
                    OperationalMutation::SavedView(SavedViewMutation::Rename { name, replacement })
                })
            }
            ActionId::SavedViewDelete => parse_name_form(&state.input)
                .map(|name| OperationalMutation::SavedView(SavedViewMutation::Delete { name })),
            ActionId::SavedViewApply => parse_name_form(&state.input)
                .map(|name| OperationalMutation::SavedView(SavedViewMutation::Apply { name })),
            ActionId::CollectionExport => parse_export_form(&state.input),
            _ => Err("this is not a local Phase 8 form".to_owned()),
        };
        match result {
            Ok(mutation) => {
                self.overlays.pop();
                self.open_operational_confirmation(state.action_id, mutation)
            }
            Err(error) => {
                if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                    current.error = Some(error);
                }
                Vec::new()
            }
        }
    }

    fn accept_access_explorer_form(&mut self, state: OperatorFormState) -> Vec<Effect> {
        let result = parse_access_question(&state.input).and_then(|question| {
            let policy = match question.policy_source {
                PolicySource::CurrentRemote => self
                    .admin
                    .policy
                    .snapshot
                    .as_ref()
                    .ok_or_else(|| "current remote policy is not observed".to_owned())
                    .and_then(|snapshot| {
                        crate::domain::policy_workflow::PolicyDocument::from_bytes_with_content_type(
                            snapshot.source_bytes.clone(),
                            snapshot.content_type.clone(),
                            snapshot.fetched_at,
                        )
                        .map_err(|error| error.to_string())
                    })?,
                PolicySource::ActiveCandidate => self
                    .policy_workflow
                    .as_ref()
                    .and_then(|workflow| workflow.candidate().cloned())
                    .ok_or_else(|| "an active policy candidate is not available".to_owned())?,
            };
            let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
                return Err("an authenticated admin profile is required".to_owned());
            };
            Ok(Effect::StartAccessExplorer {
                question,
                policy,
                profile,
                tailnet,
                credential,
                environment_token: self.admin_environment_token.clone(),
                timeout: self.resolved_config.admin.request_timeout,
            })
        });
        match result {
            Ok(effect) => {
                self.overlays.pop();
                vec![effect]
            }
            Err(error) => {
                if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                    current.error = Some(error);
                }
                Vec::new()
            }
        }
    }

    fn accept_admin_form(&mut self, state: OperatorFormState) -> Vec<Effect> {
        let change = match parse_change(state.action_id, &state.input) {
            Ok(change) => change,
            Err(error) => {
                if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                    current.error = Some(error);
                }
                return Vec::new();
            }
        };
        if let AdminChange::DnsSplitMapping {
            domain,
            resolvers,
            create,
        } = &change
            && let Some(entries) = self.admin.split_dns.snapshot.as_ref()
        {
            let exists = entries
                .entries
                .iter()
                .any(|(value, _)| value.eq_ignore_ascii_case(domain));
            let valid_operation = matches!(
                (create, resolvers.is_some(), exists),
                (true, true, false) | (false, true, true) | (false, false, true)
            );
            if !valid_operation {
                let error = if *create {
                    "split-DNS create requires a suffix that is not already present"
                } else if resolvers.is_some() {
                    "split-DNS edit requires an existing suffix"
                } else {
                    "split-DNS remove requires an existing suffix"
                };
                if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                    current.error = Some(error.to_owned());
                }
                return Vec::new();
            }
        }
        if !self.admin_mutation_available(state.action_id) {
            let reason = self
                .action_unavailable_reason(state.action_id)
                .unwrap_or_else(|| "admin mutation is unavailable".to_owned());
            if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                current.error = Some(reason);
            }
            return Vec::new();
        }
        if state.action_id == ActionId::AdminRoutesReplaceApprovals {
            return self.accept_admin_batch_form(state, change);
        }
        let Some(profile) = self.admin.profile.clone() else {
            return Vec::new();
        };
        let (target_id, base_snapshot) = match self.admin_base_snapshot(&change) {
            Ok(value) => value,
            Err(error) => {
                if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                    current.error = Some(error);
                }
                return Vec::new();
            }
        };
        let mutation_id = self.next_mutation_id;
        self.next_mutation_id = self.next_mutation_id.saturating_add(1);
        let mut request = crate::domain::admin_mutation::AdminMutation::new(
            mutation_id,
            profile,
            target_id,
            base_snapshot,
            change.clone(),
            state.action_id,
            change.risk(),
        );
        if let Err(error) = request.begin_preflight() {
            self.runtime_error = Some(error.to_string());
            return Vec::new();
        }
        let effects = self.start_admin_preflight(request);
        if effects.is_empty() {
            if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                current.error = Some(
                    "a conflicting admin mutation or read is running; preview again".to_owned(),
                );
            }
            return Vec::new();
        }
        self.overlays.pop();
        effects
    }

    fn accept_admin_batch_form(
        &mut self,
        state: OperatorFormState,
        change: AdminChange,
    ) -> Vec<Effect> {
        let AdminChange::DeviceRoutes { routes } = change else {
            if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                current.error = Some("this batch action only supports route approvals".to_owned());
            }
            return Vec::new();
        };
        let Some(profile) = self.admin.profile.clone() else {
            if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                current.error = Some("an authenticated admin profile is required".to_owned());
            }
            return Vec::new();
        };
        if !self.resolved_config.profiles.contains_key(&profile) {
            if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                current.error = Some("admin profile configuration is unavailable".to_owned());
            }
            return Vec::new();
        }
        if self.admin.tailnet.is_none() {
            if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                current.error = Some("admin tailnet is not selected".to_owned());
            }
            return Vec::new();
        }
        let observations = self
            .admin
            .route_observations()
            .into_iter()
            .filter(|route| route.complete)
            .collect::<Vec<_>>();
        if observations.is_empty() {
            if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                current.error = Some("no complete route advertisers are available".to_owned());
            }
            return Vec::new();
        }
        let parent_id = self.next_mutation_id;
        self.next_mutation_id = self.next_mutation_id.saturating_add(1);
        let action_id = state.action_id;
        let mut requests: BTreeMap<u64, AdminMutationRequest> = BTreeMap::new();
        let mut effects = Vec::new();
        for observation in observations {
            let requested = match crate::admin::route_mutations::validate_replacement(
                &observation.advertised,
                &observation.enabled,
                &routes,
            ) {
                Ok(value) => value,
                Err(error) => {
                    if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                        current.error = Some(format!(
                            "{} cannot receive the same replacement: {error}",
                            observation.device_id
                        ));
                    }
                    return Vec::new();
                }
            };
            let mutation_id = self.next_mutation_id;
            self.next_mutation_id = self.next_mutation_id.saturating_add(1);
            let mut request = crate::domain::admin_mutation::AdminMutation::new(
                mutation_id,
                profile.clone(),
                observation.device_id.clone(),
                crate::admin::mutation::route_fields(&observation.advertised, &observation.enabled),
                AdminChange::DeviceRoutes { routes: requested },
                action_id,
                AdminChange::DeviceRoutes { routes: Vec::new() }.risk(),
            );
            if let Err(error) = request.begin_preflight() {
                self.runtime_error = Some(error.to_string());
                return Vec::new();
            }
            let preflight_effects = self.start_admin_preflight(request.clone());
            if preflight_effects.is_empty() {
                for previous in requests.values() {
                    self.release_admin_preflight_lock(previous.mutation_id);
                }
                if let Some(Overlay::OperatorForm(current)) = self.overlays.last_mut() {
                    current.error = Some(
                        "a route advertiser is already being read or changed; preview again"
                            .to_owned(),
                    );
                }
                return Vec::new();
            }
            effects.extend(preflight_effects);
            requests.insert(mutation_id, request);
        }
        self.admin_batch_preflights.insert(
            parent_id,
            PendingAdminBatch {
                action_id,
                requests,
                ready: BTreeMap::new(),
            },
        );
        self.overlays.pop();
        effects
    }

    fn begin_retry_batch_preflight(&mut self, targets: Vec<BatchTarget>) -> Vec<Effect> {
        let Some(profile) = self.admin.profile.clone() else {
            self.runtime_error = Some("an authenticated admin profile is required".to_owned());
            return Vec::new();
        };
        if !self.resolved_config.profiles.contains_key(&profile) || self.admin.tailnet.is_none() {
            self.runtime_error = Some("admin profile or tailnet is no longer available".to_owned());
            return Vec::new();
        }
        let observations = self.admin.route_observations();
        let parent_id = self.next_mutation_id;
        self.next_mutation_id = self.next_mutation_id.saturating_add(1);
        let mut requests: BTreeMap<u64, AdminMutationRequest> = BTreeMap::new();
        let mut effects = Vec::new();
        for target in targets {
            let Some(observation) = observations
                .iter()
                .find(|observation| observation.device_id == target.target_id)
            else {
                self.runtime_error = Some(format!(
                    "failed target {} is no longer in fresh route state",
                    target.target_id
                ));
                return Vec::new();
            };
            let Some(route_text) = target.requested_change.strip_prefix("routes=") else {
                self.runtime_error = Some(format!(
                    "failed target {} has no reconstructable route request",
                    target.target_id
                ));
                return Vec::new();
            };
            let requested = crate::admin::route_mutations::canonical_enabled_routes(
                &route_text
                    .split(',')
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            )
            .and_then(|requested| {
                crate::admin::route_mutations::validate_replacement(
                    &observation.advertised,
                    &observation.enabled,
                    &requested,
                )
            });
            let requested = match requested {
                Ok(requested) => requested,
                Err(error) => {
                    self.runtime_error = Some(format!(
                        "fresh route preflight for {} rejected the old request: {error}",
                        target.target_id
                    ));
                    return Vec::new();
                }
            };
            let mutation_id = self.next_mutation_id;
            self.next_mutation_id = self.next_mutation_id.saturating_add(1);
            let mut request = crate::domain::admin_mutation::AdminMutation::new(
                mutation_id,
                profile.clone(),
                target.target_id,
                crate::admin::mutation::route_fields(&observation.advertised, &observation.enabled),
                AdminChange::DeviceRoutes { routes: requested },
                ActionId::AdminRoutesReplaceApprovals,
                AdminChange::DeviceRoutes { routes: Vec::new() }.risk(),
            );
            if request.begin_preflight().is_err() {
                self.runtime_error = Some("could not begin retry preflight".to_owned());
                return Vec::new();
            }
            let preflight_effects = self.start_admin_preflight(request.clone());
            if preflight_effects.is_empty() {
                for previous in requests.values() {
                    self.release_admin_preflight_lock(previous.mutation_id);
                }
                self.runtime_error = Some(
                    "a failed target is already being read or changed; no retry was started"
                        .to_owned(),
                );
                return Vec::new();
            }
            effects.extend(preflight_effects);
            requests.insert(mutation_id, request);
        }
        self.admin_batch_preflights.insert(
            parent_id,
            PendingAdminBatch {
                action_id: ActionId::AdminRoutesReplaceApprovals,
                requests,
                ready: BTreeMap::new(),
            },
        );
        effects
    }

    fn mutation_preview_lines(&self, mutation: &LocalMutation) -> Vec<String> {
        match mutation {
            LocalMutation::Connect => vec![format!(
                "state: {} -> running; existing preferences are preserved",
                self.local_state.label()
            )],
            LocalMutation::Disconnect { accept_lose_ssh } => vec![format!(
                "state: {} -> stopped; lose-SSH risk accepted: {}",
                self.local_state.label(),
                accept_lose_ssh
            )],
            LocalMutation::Preferences(request) => {
                let mut lines = Vec::new();
                if let Some(value) = request.accept_dns {
                    lines.push(format!(
                        "accept DNS: {} -> {value}",
                        boolean_text(self.local_preferences.accept_dns.value)
                    ));
                }
                if let Some(value) = request.accept_routes {
                    lines.push(format!(
                        "accept routes: {} -> {value}",
                        boolean_text(self.local_preferences.accept_routes.value)
                    ));
                }
                if let Some(value) = request.shields_up {
                    lines.push(format!(
                        "shields up: {} -> {value}",
                        boolean_text(self.local_preferences.shields_up.value)
                    ));
                    lines.push("warning: inbound connections will be blocked".to_owned());
                }
                if let Some(value) = request.ssh {
                    lines.push(format!(
                        "Tailscale SSH: {} -> {value}",
                        boolean_text(self.local_preferences.ssh.value)
                    ));
                }
                if let Some(value) = request.automatic_update {
                    lines.push(format!(
                        "automatic update: {} -> {value}",
                        boolean_text(self.local_preferences.automatic_update.value)
                    ));
                }
                if let Some(value) = request.update_check {
                    lines.push(format!(
                        "update check: {} -> {value}",
                        boolean_text(self.local_preferences.update_check.value)
                    ));
                }
                if let Some(value) = request.report_posture {
                    lines.push(format!(
                        "posture reporting: {} -> {value}",
                        boolean_text(self.local_preferences.report_posture.value)
                    ));
                    lines.push("management-plane posture data reporting changes".to_owned());
                }
                if let Some(value) = request.hostname.as_deref() {
                    lines.push(format!(
                        "hostname: {} -> {value}",
                        text_value(self.local_preferences.hostname.value.as_deref())
                    ));
                }
                if let Some(value) = request.nickname.as_deref() {
                    lines.push(format!(
                        "nickname: {} -> {value}",
                        text_value(self.local_preferences.nickname.value.as_deref())
                    ));
                    lines.push("nickname is scoped to the active account profile".to_owned());
                }
                if let Some(value) = request.web_client {
                    lines.push(format!(
                        "web client: {} -> {value}",
                        boolean_text(self.local_preferences.web_client.value)
                    ));
                    lines.push("web client exposes port 5252 to the tailnet".to_owned());
                }
                lines
            }
            LocalMutation::ExitNode(request) => {
                let mut lines = vec![format!(
                    "exit node: {} -> {}; LAN access -> {}",
                    self.local_preferences.selected_exit_label(),
                    request.target(),
                    request.allow_lan_access
                )];
                if let crate::domain::route::ExitNodeSelection::Device { device_id, .. } =
                    &request.selection
                    && let Some(candidate) = self
                        .exit_node_candidates()
                        .into_iter()
                        .find(|candidate| candidate.device_id == *device_id)
                {
                    if candidate.online == Some(false) {
                        lines.push("warning: selected exit node is offline".to_owned());
                    }
                    if candidate.last_probe_ms.is_none() {
                        lines.push(
                            "latency: not probed; run the Phase-2 ping action before relying on this choice"
                                .to_owned(),
                        );
                    }
                }
                lines
            }
            LocalMutation::Advertisements(request) => {
                let routes = match request.canonical_routes() {
                    Some(routes) if routes.is_empty() => "none".to_owned(),
                    Some(routes) => routes
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                    None => self
                        .local_preferences
                        .advertised_routes
                        .value
                        .as_ref()
                        .map_or_else(
                            || "not returned".to_owned(),
                            |routes| {
                                if routes.is_empty() {
                                    "none".to_owned()
                                } else {
                                    routes.join(",")
                                }
                            },
                        ),
                };
                let current_routes = self
                    .local_preferences
                    .advertised_routes
                    .value
                    .as_ref()
                    .map_or_else(
                        || "not returned".to_owned(),
                        |routes| {
                            if routes.is_empty() {
                                "none".to_owned()
                            } else {
                                routes.join(",")
                            }
                        },
                    );
                let mut lines = vec![format!(
                    "complete advertised route set: {current_routes} -> {routes}"
                )];
                if let Some(value) = request.advertise_exit_node {
                    lines.push(format!(
                        "exit-node advertisement: {} -> {value}",
                        boolean_text(self.local_preferences.advertised_exit_node.value)
                    ));
                }
                if let Some(value) = request.advertise_connector {
                    lines.push(format!(
                        "app connector: {} -> {value}",
                        boolean_text(self.local_preferences.app_connector.value)
                    ));
                }
                if let Some(value) = request.relay_server_port {
                    let current = match (
                        self.local_preferences.relay_server_port.value,
                        self.local_preferences.relay_server_port_disabled.value,
                    ) {
                        (Some(value), _) => value.to_string(),
                        (None, Some(true)) => "disabled".to_owned(),
                        _ => "unknown".to_owned(),
                    };
                    let requested =
                        value.map_or_else(|| "disabled".to_owned(), |value| value.to_string());
                    lines.push(format!("relay server port: {current} -> {requested}"));
                }
                if let Some(value) = request.relay_server_static_endpoints.as_ref() {
                    let current = self
                        .local_preferences
                        .relay_server_static_endpoints
                        .value
                        .as_ref()
                        .map_or_else(|| "unknown".to_owned(), |value| value.join(","));
                    lines.push(format!(
                        "relay static endpoints: {current} -> {}",
                        crate::domain::route::format_static_endpoints(value)
                    ));
                }
                if request.accept_mac_app_connector_risk {
                    lines.push("explicit mac-app-connector risk acceptance is required".to_owned());
                }
                if let Some(routes) = request.canonical_routes() {
                    for (left, right) in overlapping_routes(&routes) {
                        lines.push(format!("warning: overlapping routes {left} and {right}"));
                    }
                }
                lines.push("local advertisement does not imply administrator approval".to_owned());
                lines
            }
            LocalMutation::AccountSwitch { account_id } => {
                let label = self
                    .local_accounts
                    .iter()
                    .find(|account| account.id == *account_id)
                    .map_or("selected local profile", |account| account.display_label());
                let current = self
                    .local_accounts
                    .iter()
                    .find(|account| account.active)
                    .map_or("not returned", |account| account.display_label());
                vec![format!("active account: {current} -> {label}")]
            }
            LocalMutation::AccountRemove { account_id } => {
                let label = self
                    .local_accounts
                    .iter()
                    .find(|account| account.id == *account_id)
                    .map_or("selected local profile", |account| account.display_label());
                vec![format!(
                    "remove local profile {label}; the Tailscale account or user is not deleted"
                )]
            }
            LocalMutation::SyspolicyReload => {
                vec!["reload local system policy -> fresh list verification".to_owned()]
            }
        }
    }

    fn parse_exit_node_request(&self, input: &str) -> Result<ExitNodeRequest, String> {
        let mut parts = input.split_ascii_whitespace();
        let selection_text = parts.next().map_or("none", |value| value);
        let mut allow_lan_access = false;
        if let Some(option) = parts.next() {
            let (name, value) = option
                .split_once('=')
                .ok_or_else(|| "exit-node option must be lan=true or lan=false".to_owned())?;
            if name != "lan" {
                return Err("exit-node option must be lan=true or lan=false".to_owned());
            }
            allow_lan_access = parse_bool(value)?;
        }
        if parts.next().is_some() {
            return Err("enter an exit target and optional lan=true/false".to_owned());
        }
        let selection = if selection_text.is_empty() || selection_text == "none" {
            ExitNodeSelection::None
        } else if selection_text == "auto:any" {
            ExitNodeSelection::AutoAny
        } else {
            let candidate = self
                .exit_node_candidates()
                .into_iter()
                .find(|candidate| {
                    candidate.device_id.0 == selection_text
                        || candidate.dns_name.as_deref() == Some(selection_text)
                        || candidate
                            .tailscale_ips
                            .iter()
                            .any(|ip| ip == selection_text)
                })
                .ok_or_else(|| {
                    "exit target must be a current candidate ID, DNS name, or IP".to_owned()
                })?;
            let target = candidate
                .stable_target()
                .ok_or_else(|| "selected exit candidate has no stable target".to_owned())?;
            ExitNodeSelection::Device {
                device_id: candidate.device_id,
                target,
            }
        };
        if matches!(selection, ExitNodeSelection::None) && allow_lan_access {
            return Err("LAN access cannot be enabled when no exit node is selected".to_owned());
        }
        Ok(ExitNodeRequest {
            selection,
            allow_lan_access,
        })
    }

    fn accept_account_picker(&mut self, state: AccountPickerState) -> Vec<Effect> {
        let Some(account) = state.accounts.get(state.selected) else {
            self.runtime_error = Some("no account is selected".to_owned());
            return Vec::new();
        };
        self.overlays.pop();
        match state.action_id {
            ActionId::LocalAccountSwitch => {
                self.open_mutation_confirmation(LocalMutation::AccountSwitch {
                    account_id: account.id.clone(),
                })
            }
            ActionId::LocalAccountRemove => {
                self.open_mutation_confirmation(LocalMutation::AccountRemove {
                    account_id: account.id.clone(),
                })
            }
            _ => Vec::new(),
        }
    }

    fn accept_handoff_input(&mut self, state: HandoffInputState) -> Vec<Effect> {
        let Some(executable) = self.local_executable.as_ref() else {
            self.runtime_error = Some("local executable has not been discovered".to_owned());
            return Vec::new();
        };
        let command = match state.kind {
            HandoffInputKind::Ssh => handoff::ssh_command(
                &executable.path,
                if state.input.is_empty() {
                    None
                } else {
                    Some(state.input.as_str())
                },
                &state.host,
            ),
            HandoffInputKind::Nc => {
                handoff::nc_command(&executable.path, &state.host, &state.input)
            }
        };
        match command {
            Ok(command) => {
                let command = local_handoff_command(command, executable.socket_path.as_deref());
                let redacted_argv = redacted_argv(&command.args());
                self.overlays.pop();
                self.overlays
                    .push(Overlay::Confirmation(Box::new(ConfirmationState {
                        action_id: match state.kind {
                            HandoffInputKind::Ssh => ActionId::LocalSshOpen,
                            HandoffInputKind::Nc => ActionId::LocalNcOpen,
                        },
                        mutation: None,
                        admin_mutation: None,
                        admin_batch: None,
                        service_request: None,
                        operational_mutation: None,
                        handoff: Some(command),
                        prompt: "Pause Tale and open the selected interactive terminal session."
                            .to_owned(),
                        required_phrase: None,
                        input: String::new(),
                        lose_ssh_checked: false,
                        preview_lines: vec![
                        "the child receives only the selected host and supplied port or username"
                            .to_owned(),
                    ],
                        redacted_argv,
                        error: None,
                    })));
                Vec::new()
            }
            Err(error) => {
                if let Some(Overlay::HandoffInput(current)) = self.overlays.last_mut() {
                    current.error = Some(error.to_string());
                }
                Vec::new()
            }
        }
    }

    fn accept_admin_batch_confirmation(
        &mut self,
        confirmation: AdminBatchConfirmation,
    ) -> Vec<Effect> {
        let Some(profile_config) = self
            .admin
            .profile
            .as_ref()
            .and_then(|profile| self.resolved_config.profiles.get(profile))
        else {
            self.set_confirmation_error("admin profile configuration is unavailable");
            return Vec::new();
        };
        let Some(tailnet) = self.admin.tailnet.clone() else {
            self.set_confirmation_error("admin tailnet is no longer selected");
            return Vec::new();
        };
        if !self.admin_mutation_available(confirmation.batch.action_id) {
            let reason = self
                .action_unavailable_reason(confirmation.batch.action_id)
                .unwrap_or_else(|| "admin batch mutation is no longer available".to_owned());
            self.set_confirmation_error(&reason);
            return Vec::new();
        }
        let target_ids = confirmation
            .requests
            .iter()
            .map(|request| request.target_id.clone())
            .collect::<Vec<_>>();
        if !confirmation.batch.target_list_is_unchanged(&target_ids) {
            self.set_confirmation_error("the immutable batch target list changed; preview again");
            return Vec::new();
        }
        for request in &confirmation.requests {
            let Some(preflight) = request.preflight.as_ref() else {
                self.set_confirmation_error("every batch target requires a fresh preflight");
                return Vec::new();
            };
            if !preflight.is_fresh_at(self.now, request.risk) {
                self.set_confirmation_error("a batch preflight expired; preview again");
                return Vec::new();
            }
        }
        let mut held = Vec::new();
        for request in &confirmation.requests {
            if self.admin_resource_locks.try_hold(
                request.mutation_id,
                request
                    .change
                    .lock_keys(&request.profile, &request.target_id),
            ) {
                held.push(request.mutation_id);
            } else {
                for mutation_id in held {
                    self.admin_resource_locks.release(mutation_id);
                }
                self.set_confirmation_error(
                    "a conflicting admin mutation or read is running for a batch target",
                );
                return Vec::new();
            }
        }
        let mut requests = confirmation.requests;
        let concurrency = confirmation.batch.max_concurrency.clamp(1, 4);
        for request in requests.iter_mut().take(concurrency) {
            if let Err(error) = transition(&mut request.state, AdminMutationState::Dispatching) {
                for mutation_id in &held {
                    self.admin_resource_locks.release(*mutation_id);
                }
                self.runtime_error = Some(error.to_string());
                return Vec::new();
            }
        }
        let parent_task_id = self.tasks.create(
            confirmation.batch.action_id,
            format!("{} route advertisers", requests.len()),
            self.now,
            true,
        );
        let _ = self.tasks.set_local_metadata(
            parent_task_id,
            vec!["batch parent".to_owned()],
            Vec::new(),
        );
        let mut batch = confirmation.batch;
        batch.parent_task_id = parent_task_id.0;
        let mut child_tasks = BTreeMap::new();
        let mut effects = Vec::new();
        let pending_requests = requests.split_off(requests.len().min(concurrency));
        for request in requests {
            let task_id = self.tasks.create(
                request.action_id,
                format!("route advertiser {}", request.target_id),
                self.now,
                true,
            );
            let _ = self.tasks.set_local_metadata(
                task_id,
                vec![request.change.audit_action_class().to_owned()],
                Vec::new(),
            );
            self.admin_mutations_in_flight
                .insert(request.mutation_id, task_id);
            child_tasks.insert(request.mutation_id, task_id);
            effects.push(Effect::StartAdminMutation {
                task_id,
                request,
                tailnet: tailnet.clone(),
                credential: profile_config.credential.clone(),
                environment_token: self.admin_environment_token.clone(),
                timeout: self.resolved_config.admin.request_timeout,
            });
        }
        let _ = self.tasks.start(parent_task_id);
        self.admin_batches_in_flight.insert(
            parent_task_id.0,
            AdminBatchInFlight {
                batch,
                parent_task_id,
                child_tasks,
                pending_requests,
            },
        );
        self.overlays.pop();
        effects
    }

    fn set_confirmation_error(&mut self, error: &str) {
        if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
            current.error = Some(error.to_owned());
        }
    }

    fn extend_admin_refresh_for_owned_devices(
        &self,
        request: &AdminMutationRequest,
        mut resources: Vec<AdminRefreshResource>,
    ) -> Vec<AdminRefreshResource> {
        if !matches!(
            request.change.resource_kind(),
            crate::domain::admin_mutation::AdminResourceKind::User
        ) {
            return resources;
        }
        if let Some(devices) = self.admin.devices.snapshot.as_ref() {
            for device in devices
                .iter()
                .filter(|device| device.user_id.as_deref() == Some(request.target_id.as_str()))
            {
                let resource = AdminRefreshResource::DeviceRoutes(device.stable_id.clone());
                if !resources.contains(&resource) {
                    resources.push(resource);
                }
            }
        }
        resources
    }

    fn admin_mutation_target_is_current(&self, request: &AdminMutationRequest) -> bool {
        match request.change.resource_kind() {
            crate::domain::admin_mutation::AdminResourceKind::Device => self
                .selected_admin_device()
                .is_some_and(|device| device.stable_id == request.target_id),
            crate::domain::admin_mutation::AdminResourceKind::DeviceRoutes => self
                .admin
                .route_observations()
                .iter()
                .any(|route| route.device_id == request.target_id),
            crate::domain::admin_mutation::AdminResourceKind::User => self
                .selected_admin_user()
                .is_some_and(|user| user.id == request.target_id),
            crate::domain::admin_mutation::AdminResourceKind::TailnetDns => {
                request.target_id == "tailnet"
            }
        }
    }

    fn open_selected_batch_result(&mut self) -> Vec<Effect> {
        let Some(task_id) = self.tasks.selected else {
            self.runtime_error = Some("select a completed batch task first".to_owned());
            return Vec::new();
        };
        if !self.admin_batch_results.contains_key(&task_id) {
            self.runtime_error = Some("the selected task has no batch outcomes".to_owned());
            return Vec::new();
        }
        self.overlays.push(Overlay::TaskInspector(task_id));
        Vec::new()
    }

    fn retry_selected_batch(&mut self) -> Vec<Effect> {
        let Some(task_id) = self.tasks.selected else {
            self.runtime_error = Some("select a completed batch task first".to_owned());
            return Vec::new();
        };
        let Some(batch) = self.admin_batch_results.get(&task_id) else {
            self.runtime_error = Some("the selected task has no batch outcomes".to_owned());
            return Vec::new();
        };
        let failed = batch
            .targets
            .iter()
            .filter(|target| {
                batch
                    .child_outcomes
                    .get(&target.target_id)
                    .is_some_and(|outcome| {
                        !matches!(
                            outcome,
                            crate::domain::admin_mutation::BatchChildOutcome::VerifiedSuccess
                        )
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        if failed.is_empty() {
            self.runtime_error = Some("there are no failed targets to retry".to_owned());
            return Vec::new();
        }
        self.pending_batch_retry = Some(failed);
        self.navigate(Route::Routes);
        self.runtime_error = Some(
            "fetching fresh route state for the failed targets before a new preview".to_owned(),
        );
        self.start_admin_resource_refresh(vec![AdminRefreshResource::Devices])
    }

    fn accept_confirmation(&mut self, state: ConfirmationState) -> Vec<Effect> {
        if let Some(required) = state.required_phrase.as_deref()
            && state.input != required
        {
            if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
                current.error = Some(format!("type {required} exactly to confirm"));
            }
            return Vec::new();
        }
        let overwrite_confirmed = state.required_phrase.as_deref() == Some("OVERWRITE EXPORT");
        if let Some(OperationalMutation::Export(request)) = state.operational_mutation.as_ref()
            && let Some(expected) = self.pending_export_fingerprint
            && self.export_fingerprint(request).ok() != Some(expected)
        {
            self.set_confirmation_error(
                "the export source changed after preview; refresh and review the export again",
            );
            return Vec::new();
        }
        if let Some(OperationalMutation::Export(request)) = state.operational_mutation.as_ref()
            && request.path.exists()
            && !overwrite_confirmed
        {
            self.set_confirmation_error(
                "the export target appeared after preview; review the overwrite confirmation again",
            );
            return Vec::new();
        }
        if let Some(mutation) = state.operational_mutation.clone() {
            self.pending_export_fingerprint = None;
            return self.accept_operational_mutation(
                state.action_id,
                mutation,
                overwrite_confirmed,
            );
        }
        if let Some(batch) = state.admin_batch.clone() {
            return self.accept_admin_batch_confirmation(batch);
        }
        if state.action_id == ActionId::AccessCopySource {
            if let Some(source) = self
                .admin
                .policy
                .snapshot
                .as_ref()
                .and_then(PolicySnapshot::as_str)
            {
                self.copied_value = Some(source.to_owned());
                self.runtime_error = Some(
                    "full policy source copied after explicit privacy confirmation".to_owned(),
                );
            } else {
                self.runtime_error = Some("policy source is no longer available".to_owned());
            }
            self.overlays.pop();
            return Vec::new();
        }
        if state.action_id == ActionId::AdminPolicyCandidateDiscard {
            self.overlays.pop();
            if state.required_phrase.as_deref() == Some("REPLACE POLICY CANDIDATE") {
                return self.replace_policy_candidate_with_latest();
            }
            return self.discard_policy_candidate();
        }
        if state.action_id == ActionId::AdminPolicyWorkflowClose {
            self.overlays.pop();
            return self.close_policy_workflow();
        }
        if state.action_id == ActionId::AdminPolicyApply {
            if self.resolved_config.read_only
                || self.admin.profile_read_only
                || !self.admin_scope_allowed("policy_file:write")
            {
                self.set_confirmation_error(
                    "policy apply is no longer permitted by the current read-only mode or scope",
                );
                return Vec::new();
            }
            if !self.sync_policy_candidate_file() {
                return Vec::new();
            }
            let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
                self.runtime_error = Some("an authenticated admin profile is required".to_owned());
                return Vec::new();
            };
            let Some(workflow) = self.policy_workflow.as_mut() else {
                self.runtime_error = Some("the policy workflow is no longer open".to_owned());
                return Vec::new();
            };
            if let Err(error) = workflow.apply_guard(self.now) {
                if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
                    current.error = Some(error.to_string());
                }
                return Vec::new();
            }
            let Some(path) = workflow.candidate_path().map(PathBuf::from) else {
                self.runtime_error = Some("the policy candidate is unavailable".to_owned());
                return Vec::new();
            };
            let Some(base_hash) = workflow.base().map(|value| value.hash().to_owned()) else {
                self.runtime_error = Some("the policy base is unavailable".to_owned());
                return Vec::new();
            };
            let Some(candidate_hash) = workflow.candidate().map(|value| value.hash().to_owned())
            else {
                self.runtime_error = Some("the policy candidate is unavailable".to_owned());
                return Vec::new();
            };
            workflow.mark_applying();
            self.overlays.pop();
            return vec![Effect::StartPolicyApply {
                workflow_id: workflow.workflow_id(),
                profile,
                tailnet,
                credential,
                environment_token: self.admin_environment_token.clone(),
                timeout: self.resolved_config.admin.request_timeout,
                path,
                expected_base_hash: base_hash,
                expected_candidate_hash: candidate_hash,
            }];
        }
        if state.action_id == ActionId::AdminCredentialAuthKeyCreate {
            if self.resolved_config.read_only
                || self.admin.profile_read_only
                || !self.admin_scope_allowed("auth_keys:write")
            {
                self.set_confirmation_error(
                    "auth-key creation is no longer permitted by the current read-only mode or scope",
                );
                return Vec::new();
            }
            let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
                self.runtime_error = Some("an authenticated admin profile is required".to_owned());
                return Vec::new();
            };
            let Some(request) = self.pending_auth_key_request.take() else {
                self.runtime_error = Some("the auth-key request is no longer available".to_owned());
                return Vec::new();
            };
            if let Err(error) = request.validate() {
                self.runtime_error = Some(error.to_string());
                return Vec::new();
            }
            let result_id = self.next_secret_result_id;
            self.next_secret_result_id = self.next_secret_result_id.saturating_add(1);
            self.pending_auth_key_result = Some(result_id);
            self.overlays.pop();
            return vec![Effect::StartAuthKeyCreate {
                result_id,
                profile,
                tailnet,
                credential,
                environment_token: self.admin_environment_token.clone(),
                timeout: self.resolved_config.admin.request_timeout,
                request,
            }];
        }
        if state.action_id == ActionId::AdminCredentialRevoke {
            let Some(key_id) = state
                .required_phrase
                .as_deref()
                .and_then(|value| value.strip_prefix("REVOKE "))
            else {
                self.runtime_error = Some("the credential revoke target is unavailable".to_owned());
                return Vec::new();
            };
            if self.pending_credential_revoke.as_deref() != Some(key_id) {
                self.set_confirmation_error(
                    "the credential detail is no longer current; reopen revocation",
                );
                return Vec::new();
            }
            if self.resolved_config.read_only || self.admin.profile_read_only {
                self.set_confirmation_error("read-only mode blocks remote credential revocation");
                return Vec::new();
            }
            let Some(selected) = self.selected_credential() else {
                self.set_confirmation_error("the selected credential is no longer available");
                return Vec::new();
            };
            let credential_type = crate::admin::key_mutations::remote_credential_type(selected);
            let Some(read_scope) = credential_type.read_scope() else {
                self.set_confirmation_error("the selected credential read scope is unknown");
                return Vec::new();
            };
            let Some(write_scope) = credential_type.write_scope() else {
                self.set_confirmation_error("the selected credential write scope is unknown");
                return Vec::new();
            };
            if selected.id != key_id
                || !credential_type.supported_for_revoke()
                || selected.invalid == Some(true)
                || selected.revoked_at.is_some()
                || !self.admin_scope_allowed(read_scope)
                || !self.admin_scope_allowed(write_scope)
            {
                self.set_confirmation_error(
                    "the selected credential changed or is no longer revocable; reopen revocation",
                );
                return Vec::new();
            }
            let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
                self.runtime_error = Some("an authenticated admin profile is required".to_owned());
                return Vec::new();
            };
            self.overlays.pop();
            return vec![Effect::StartCredentialRevoke {
                key_id: key_id.to_owned(),
                profile,
                tailnet,
                credential,
                environment_token: self.admin_environment_token.clone(),
                timeout: self.resolved_config.admin.request_timeout,
            }];
        }
        if state.action_id == ActionId::ProfileCredentialRemove {
            let Some(profile) = self.admin.profile.clone() else {
                self.runtime_error = Some("an active profile is required".to_owned());
                return Vec::new();
            };
            let Some(configuration) = self.resolved_config.profiles.get(&profile) else {
                self.runtime_error =
                    Some("the active profile configuration is unavailable".to_owned());
                return Vec::new();
            };
            let reference = configuration.credential.clone();
            self.overlays.pop();
            return vec![Effect::StartProfileCredentialRemove { profile, reference }];
        }
        if let Some(mut request) = state.admin_mutation {
            if !self.admin_mutation_available(request.action_id) {
                let reason = self
                    .action_unavailable_reason(request.action_id)
                    .unwrap_or_else(|| "admin mutation is no longer available".to_owned());
                if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
                    current.error = Some(reason);
                }
                return Vec::new();
            }
            if !self.admin_mutation_target_is_current(&request) {
                if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
                    current.error = Some(
                        "the selected admin target changed; discard this preview and start again"
                            .to_owned(),
                    );
                }
                return Vec::new();
            }
            let Some(preflight) = request.preflight.as_ref() else {
                if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
                    current.error = Some("fresh preflight is required before dispatch".to_owned());
                }
                return Vec::new();
            };
            if !preflight.is_fresh_at(self.now, request.risk) {
                if let Err(error) = transition(&mut request.state, AdminMutationState::Preflighting)
                {
                    self.runtime_error = Some(error.to_string());
                    return Vec::new();
                }
                self.overlays.pop();
                return self.start_admin_preflight(request);
            }
            let lock_keys = request
                .change
                .lock_keys(&request.profile, &request.target_id);
            if !self
                .admin_resource_locks
                .try_hold(request.mutation_id, lock_keys)
            {
                if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
                    current.error =
                        Some("a conflicting admin mutation or read is running".to_owned());
                }
                return Vec::new();
            }
            if let Err(error) = transition(&mut request.state, AdminMutationState::Dispatching) {
                self.admin_resource_locks.release(request.mutation_id);
                self.runtime_error = Some(error.to_string());
                return Vec::new();
            }
            let Some(profile_config) = self.resolved_config.profiles.get(&request.profile) else {
                self.admin_resource_locks.release(request.mutation_id);
                self.runtime_error = Some("admin profile configuration disappeared".to_owned());
                return Vec::new();
            };
            let Some(tailnet) = self.admin.tailnet.clone() else {
                self.admin_resource_locks.release(request.mutation_id);
                self.runtime_error = Some("admin tailnet is no longer selected".to_owned());
                return Vec::new();
            };
            let task_id = self.tasks.create(
                request.action_id,
                format!(
                    "{} {}",
                    request.change.resource_kind().label(),
                    request.target_id
                ),
                self.now,
                true,
            );
            let _ = self.tasks.set_local_metadata(
                task_id,
                vec![request.change.audit_action_class().to_owned()],
                Vec::new(),
            );
            self.admin_mutations_in_flight
                .insert(request.mutation_id, task_id);
            self.overlays.pop();
            return vec![Effect::StartAdminMutation {
                task_id,
                request,
                tailnet,
                credential: profile_config.credential.clone(),
                environment_token: self.admin_environment_token.clone(),
                timeout: self.resolved_config.admin.request_timeout,
            }];
        }
        if let Some(mut request) = state.service_request {
            if self.resolved_config.read_only && is_service_write_action(request.action_id()) {
                self.runtime_error =
                    Some("read-only mode blocks local service mutations".to_owned());
                return Vec::new();
            }
            if request.action_id() == ActionId::ServicesCertificateObtain
                && let ServiceActionRequest::Certificate(certificate) = &mut request
            {
                let overwrites =
                    certificate.certificate_path.exists() || certificate.key_path.exists();
                if overwrites && !certificate.overwrites_existing {
                    certificate.overwrites_existing = true;
                    self.overlays.pop();
                    return self.open_service_confirmation(request);
                }
            }
            self.overlays.pop();
            return self.start_service_request(request);
        }
        if let Some(mut mutation) = state.mutation {
            if self.resolved_config.read_only {
                self.runtime_error = Some("read-only mode blocks local mutations".to_owned());
                return Vec::new();
            }
            if self.mutation_in_flight.is_some() {
                if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
                    current.error = Some("another local mutation is already running".to_owned());
                }
                return Vec::new();
            }
            if let LocalMutation::Disconnect { accept_lose_ssh } = &mut mutation {
                *accept_lose_ssh = state.lose_ssh_checked;
            }
            let mutation_id = self.next_mutation_id;
            self.next_mutation_id = self.next_mutation_id.saturating_add(1);
            if !self.mutation_lock.hold(mutation_id) {
                self.runtime_error = Some("local mutation lock is held".to_owned());
                return Vec::new();
            }
            let Some(executable) = self.local_executable.clone() else {
                self.mutation_lock.release(mutation_id);
                self.runtime_error = Some("local executable has not been discovered".to_owned());
                return Vec::new();
            };
            let task_id = self.tasks.create(
                mutation.action_id(),
                mutation_target_label(&mutation),
                self.now,
                true,
            );
            let (fields, argv) = mutation_metadata(
                &executable.path,
                &mutation,
                self.resolved_config.local.command_timeout,
            );
            let _ = self.tasks.set_local_metadata(task_id, fields, argv);
            self.mutation_in_flight = Some(mutation_id);
            self.overlays.pop();
            return vec![Effect::StartLocalMutation {
                mutation_id,
                task_id,
                executable,
                timeout: self.resolved_config.local.command_timeout,
                mutation,
            }];
        }
        if let Some(command) = state.handoff {
            if self.resolved_config.read_only
                && matches!(
                    state.action_id,
                    ActionId::LocalAccountLogin | ActionId::LocalAccountLogout
                )
            {
                self.runtime_error = Some("read-only mode blocks local account changes".to_owned());
                return Vec::new();
            }
            let args = command.args();
            let task_id = self.tasks.create(
                state.action_id,
                match state.action_id {
                    ActionId::LocalAccountLogin => "tailscale login",
                    ActionId::LocalAccountLogout => "tailscale logout",
                    ActionId::LocalSshOpen => "Tailscale SSH",
                    ActionId::LocalNcOpen => "Tailscale netcat",
                    _ => "interactive terminal",
                },
                self.now,
                false,
            );
            let requested_fields = match state.action_id {
                ActionId::LocalSshOpen => vec!["host".to_owned(), "username".to_owned()],
                ActionId::LocalNcOpen => vec!["host".to_owned(), "port".to_owned()],
                ActionId::LocalAccountLogin | ActionId::LocalAccountLogout => Vec::new(),
                _ => Vec::new(),
            };
            let _ = self
                .tasks
                .set_local_metadata(task_id, requested_fields, redacted_argv(&args));
            self.interactive_handoff_active = true;
            self.overlays.pop();
            return vec![Effect::StartTerminalHandoff { task_id, command }];
        }
        Vec::new()
    }

    fn accept_operational_mutation(
        &mut self,
        action_id: ActionId,
        mutation: OperationalMutation,
        overwrite_confirmed: bool,
    ) -> Vec<Effect> {
        if matches!(
            mutation,
            OperationalMutation::SavedView(_) | OperationalMutation::Export(_)
        ) {
            self.overlays.pop();
            return self.apply_local_operational_mutation(mutation, overwrite_confirmed);
        }
        if !self.phase_eight_mutation_available(action_id) {
            self.set_confirmation_error(
                "the operational mutation is no longer permitted by profile, scope, or read-only mode",
            );
            return Vec::new();
        }
        let Some(profile) = self.admin.profile.clone() else {
            self.set_confirmation_error("an authenticated admin profile is required");
            return Vec::new();
        };
        let Some(profile_config) = self.resolved_config.profiles.get(&profile) else {
            self.set_confirmation_error("admin profile configuration is unavailable");
            return Vec::new();
        };
        let Some(tailnet) = self.admin.tailnet.clone() else {
            self.set_confirmation_error("admin tailnet is no longer selected");
            return Vec::new();
        };
        self.overlays.pop();
        vec![Effect::StartOperationalMutation {
            action_id,
            mutation,
            profile,
            tailnet,
            credential: profile_config.credential.clone(),
            environment_token: self.admin_environment_token.clone(),
            timeout: self.resolved_config.admin.request_timeout,
        }]
    }

    fn apply_local_operational_mutation(
        &mut self,
        mutation: OperationalMutation,
        overwrite_confirmed: bool,
    ) -> Vec<Effect> {
        match mutation {
            OperationalMutation::SavedView(operation) => self.apply_saved_view_operation(operation),
            OperationalMutation::Export(request) => match self.build_export_document(&request) {
                Ok(document) => {
                    let format = if request.format == "csv" {
                        crate::export::ExportFormat::Csv
                    } else {
                        crate::export::ExportFormat::Json
                    };
                    match crate::export::write_atomic(
                        &document,
                        &request.path,
                        format,
                        overwrite_confirmed,
                    ) {
                        Ok(path) => {
                            self.runtime_error = Some(format!(
                                "deterministic {} export written to {}",
                                request.format,
                                path.display()
                            ));
                        }
                        Err(error) => self.runtime_error = Some(error.to_string()),
                    }
                    Vec::new()
                }
                Err(error) => {
                    self.runtime_error = Some(error);
                    Vec::new()
                }
            },
            _ => Vec::new(),
        }
    }

    fn apply_saved_view_operation(&mut self, operation: SavedViewMutation) -> Vec<Effect> {
        let Some(saved_views) = self.saved_views.as_mut() else {
            self.runtime_error = Some("saved-view state is unavailable".to_owned());
            return Vec::new();
        };
        let result = match operation {
            SavedViewMutation::Create(view) => {
                saved_views.store.create(view, &saved_views.registry)
            }
            SavedViewMutation::Replace { name, view } => {
                saved_views
                    .store
                    .replace(&name, view, &saved_views.registry)
            }
            SavedViewMutation::Rename { name, replacement } => {
                saved_views.store.rename(&name, replacement)
            }
            SavedViewMutation::Delete { name } => saved_views.store.delete(&name),
            SavedViewMutation::Apply { name } => {
                let view = match saved_views.store.apply(&name) {
                    Ok(view) => view.clone(),
                    Err(error) => {
                        self.runtime_error = Some(error.to_string());
                        return Vec::new();
                    }
                };
                match self.apply_saved_view_to_ui(&view) {
                    Ok(()) => {
                        self.runtime_error = Some(format!("saved view {name} applied"));
                        return Vec::new();
                    }
                    Err(error) => {
                        self.runtime_error = Some(error);
                        return Vec::new();
                    }
                }
            }
        };
        match result {
            Ok(()) => self.runtime_error = Some("saved-view file updated atomically".to_owned()),
            Err(error) => self.runtime_error = Some(error.to_string()),
        }
        Vec::new()
    }

    fn apply_saved_view_to_ui(&mut self, view: &SavedView) -> Result<(), String> {
        let route = Route::parse(&view.route)
            .filter(|route| route.label() == view.route)
            .ok_or_else(|| format!("saved view route is not canonical: {}", view.route))?;
        if route != Route::Devices
            && (view.wide_columns
                || !view.columns.is_empty()
                || !view.filters.is_empty()
                || !view.sort.is_empty())
        {
            return Err(format!(
                "saved view route {} has no active structured-view adapter",
                view.route
            ));
        }
        let same_route = self.current_route() == route;
        if same_route {
            self.capture_current_frame();
        }
        self.navigate(route);
        if route == Route::Devices {
            let terms = view
                .filters
                .iter()
                .map(saved_filter_to_term)
                .collect::<Result<Vec<_>, _>>()?;
            let filter_text = view
                .filters
                .iter()
                .map(saved_filter_to_cli)
                .collect::<Result<Vec<_>, _>>()?
                .join(" ");
            let expression = FilterExpression { terms };
            self.views.devices.filter_draft = filter_text;
            self.views.devices.applied_filter = expression;
            let sort_terms = view
                .sort
                .iter()
                .map(saved_sort_to_device)
                .collect::<Result<Vec<_>, _>>()?;
            self.views.devices.sort_terms = if sort_terms.is_empty() {
                vec![SortSpec::default()]
            } else {
                sort_terms
            };
            self.views.devices.sort = self
                .views
                .devices
                .sort_terms
                .first()
                .copied()
                .map_or(SortSpec::default(), |value| value);
            self.views.devices.wide_columns =
                view.wide_columns || view.columns.iter().any(|column| column == "version");
            self.views.devices.columns = view.columns.clone();
            self.reconcile_selection(None);
        }
        let mut frame = self.current_view_frame();
        frame.saved_view = Some(view.name.clone());
        if same_route {
            let _ = self.view_history.append(frame);
        } else {
            self.view_history.replace_current(frame);
        }
        Ok(())
    }

    fn export_fingerprint(&self, request: &ExportRequest) -> Result<[u8; 32], String> {
        let mut document = self.build_export_document(request)?;
        document.metadata.export_timestamp = None;
        let bytes = document
            .json_bytes_in_order()
            .map_err(|error| error.to_string())?;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hasher.finalize();
        let mut fingerprint = [0_u8; 32];
        fingerprint.copy_from_slice(&digest);
        Ok(fingerprint)
    }

    fn build_export_document(
        &self,
        request: &ExportRequest,
    ) -> Result<crate::domain::export::ExportDocument, String> {
        use crate::domain::export::{ExportCollection, ExportMetadata, ExportRow, ExportSource};
        let source_id = self.admin.profile.as_ref().map_or_else(
            || "admin:unselected".to_owned(),
            |value| format!("admin:{value}"),
        );
        let active_filter = if request.collection == ExportCollection::Devices {
            canonical_device_filter(&self.views.devices.applied_filter)
        } else {
            "none".to_owned()
        };
        let active_sort = if request.collection == ExportCollection::Devices {
            canonical_device_sort(&self.device_sort_terms())
        } else {
            "stable_key".to_owned()
        };
        let export_route = match request.collection {
            ExportCollection::Devices => "devices",
            ExportCollection::Users => "users",
            ExportCollection::Routes => "routes",
            ExportCollection::Dns => "dns",
            ExportCollection::CredentialMetadata => "credentials",
            ExportCollection::Audit => "activity",
            ExportCollection::HealthFindings => "overview",
            ExportCollection::FlowLogs => "activity",
        };
        let metadata = |observed_at: Timestamp, complete: bool| ExportMetadata {
            schema: request.collection,
            schema_version: 1,
            tale_version: env!("CARGO_PKG_VERSION").to_owned(),
            sources: vec![ExportSource {
                id: source_id.clone(),
                observed_at,
            }],
            observed_at,
            route: export_route.to_owned(),
            active_filter: active_filter.clone(),
            active_sort: active_sort.clone(),
            truncated: false,
            complete,
            export_timestamp: format_export_timestamp(self.now),
        };
        let (observed_at, complete, rows) =
            match request.collection {
                ExportCollection::Devices => {
                    if self.devices_resource.observed_at.is_none()
                        && self.devices_resource.snapshot.is_empty()
                        && self.devices_resource.health != SourceHealth::Healthy
                    {
                        return Err("device collection is not currently observed".to_owned());
                    }
                    let observed_at = self
                        .devices_resource
                        .observed_at
                        .map_or(self.now, |value| value);
                    let rows = self
                        .visible_indices()
                        .into_iter()
                        .filter_map(|index| self.devices_resource.snapshot.get(index))
                        .map(|device| ExportRow::Device {
                            id: device.id.0.clone(),
                            name: device.display_name.clone(),
                            addresses: sorted_strings(&device.addresses),
                            source: source_id.clone(),
                            observed_at,
                        })
                        .collect();
                    (
                        observed_at,
                        self.devices_resource.health == SourceHealth::Healthy,
                        rows,
                    )
                }
                ExportCollection::Users => {
                    let values =
                        self.admin.users.snapshot.as_ref().ok_or_else(|| {
                            "user collection is not currently observed".to_owned()
                        })?;
                    let observed_at = self.admin.users.observed_at.map_or(self.now, |value| value);
                    let rows = values
                        .iter()
                        .map(|user| ExportRow::User {
                            id: user.id.clone(),
                            name: user.label().to_owned(),
                            role: user
                                .role
                                .clone()
                                .unwrap_or_else(|| "not returned".to_owned()),
                            source: source_id.clone(),
                            observed_at,
                        })
                        .collect();
                    (
                        observed_at,
                        self.admin.users.state == AdminResourceState::Ready,
                        rows,
                    )
                }
                ExportCollection::Routes => {
                    let observations = self.admin.route_observations();
                    let observed_at = self
                        .admin
                        .routes
                        .observed_at
                        .map_or(self.now, |value| value);
                    let rows = observations
                        .iter()
                        .flat_map(|observation| {
                            observation.advertised.iter().map(|cidr| ExportRow::Route {
                                id: format!("{}:{cidr}", observation.device_id),
                                cidr: cidr
                                    .parse::<crate::domain::route::IpNet>()
                                    .map_or_else(|_| cidr.clone(), |value| value.to_string()),
                                advertiser: observation.device_id.clone(),
                                approval: if observation.enabled.iter().any(|value| value == cidr) {
                                    "approved".to_owned()
                                } else {
                                    "not approved".to_owned()
                                },
                                source: source_id.clone(),
                                observed_at: observation.observed_at,
                            })
                        })
                        .collect();
                    (
                        observed_at,
                        self.admin.routes.state == AdminResourceState::Ready,
                        rows,
                    )
                }
                ExportCollection::Dns => {
                    let values = self
                        .admin
                        .nameservers
                        .snapshot
                        .as_ref()
                        .ok_or_else(|| "DNS collection is not currently observed".to_owned())?;
                    let observed_at = self
                        .admin
                        .nameservers
                        .observed_at
                        .map_or(self.now, |value| value);
                    let sorted_values = sorted_strings(&values.values);
                    let rows = sorted_values
                        .iter()
                        .enumerate()
                        .map(|(index, value)| ExportRow::Dns {
                            name: format!("nameserver-{index}"),
                            value: value.clone(),
                            source: source_id.clone(),
                            observed_at,
                        })
                        .collect();
                    (
                        observed_at,
                        self.admin.nameservers.state == AdminResourceState::Ready,
                        rows,
                    )
                }
                ExportCollection::CredentialMetadata => {
                    let values = self.admin.credentials.snapshot.as_ref().ok_or_else(|| {
                        "credential metadata is not currently observed".to_owned()
                    })?;
                    let rows = values
                        .records
                        .iter()
                        .map(|record| ExportRow::CredentialMetadata {
                            id: record.id.clone(),
                            credential_type: record.key_type.clone(),
                            status: credential_status(record, self.now),
                            created_at: record.created_at,
                            expires_at: record.expires_at,
                            source: source_id.clone(),
                            observed_at: values.observed_at,
                        })
                        .collect();
                    (values.observed_at, !values.partial, rows)
                }
                ExportCollection::Audit => {
                    let values =
                        self.admin.activity.snapshot.as_ref().ok_or_else(|| {
                            "audit collection is not currently observed".to_owned()
                        })?;
                    let rows = values
                        .events
                        .iter()
                        .filter(|event| self.audit_filters.matches(event))
                        .map(|event| ExportRow::Audit {
                            event_id: audit_export_id(event),
                            event_time: format_export_timestamp(event.event_time)
                                .unwrap_or_else(|| event.event_time_text.clone()),
                            action: event
                                .action
                                .clone()
                                .unwrap_or_else(|| "not returned".to_owned()),
                            actor: event.actor.as_ref().map_or_else(
                                || "not returned".to_owned(),
                                |actor| {
                                    actor
                                        .id
                                        .clone()
                                        .or(actor.display.clone())
                                        .unwrap_or_else(|| "not returned".to_owned())
                                },
                            ),
                            target: event.target.as_ref().map_or_else(
                                || "not returned".to_owned(),
                                |target| {
                                    target
                                        .id
                                        .clone()
                                        .or(target.display.clone())
                                        .unwrap_or_else(|| "not returned".to_owned())
                                },
                            ),
                            source: source_id.clone(),
                            observed_at: values.observed_at,
                        })
                        .collect();
                    (values.observed_at, !values.delayed, rows)
                }
                ExportCollection::HealthFindings => {
                    let observed_at = self
                        .health
                        .snapshot
                        .as_ref()
                        .map_or(0, |snapshot| snapshot.now);
                    let rows = self
                        .health_findings
                        .iter()
                        .map(|finding| ExportRow::HealthFinding {
                            id: finding.id.clone(),
                            rule_id: finding.rule_id.clone(),
                            severity: finding.severity.label().to_owned(),
                            title: finding.title.clone(),
                            affected_resource_ids: finding.affected_resource_ids.clone(),
                            source_ids: finding.source_ids.clone(),
                            derived: finding.derived,
                            observed_at: finding.observed_at,
                        })
                        .collect();
                    (observed_at, self.health.snapshot.is_some(), rows)
                }
                ExportCollection::FlowLogs => {
                    let snapshot = self
                        .flow_snapshot
                        .as_ref()
                        .ok_or_else(|| "no bounded flow window is currently observed".to_owned())?;
                    let rows = snapshot
                        .messages
                        .iter()
                        .flat_map(|message| {
                            message
                                .records()
                                .filter(|record| snapshot.filter.matches(record))
                                .map(|record| {
                                    let source = record.connection.canonical_src();
                                    let destination = record.connection.canonical_dst();
                                    ExportRow::FlowLog {
                                        reporting_node: record.node_id,
                                        logged: canonical_wire_timestamp(&record.logged),
                                        start: canonical_wire_timestamp(&record.start),
                                        end: canonical_wire_timestamp(&record.end),
                                        traffic_class: record.class.label().to_owned(),
                                        protocol: record.connection.proto,
                                        source,
                                        destination,
                                        tx_packets: record.connection.tx_packets,
                                        tx_bytes: record.connection.tx_bytes,
                                        rx_packets: record.connection.rx_packets,
                                        rx_bytes: record.connection.rx_bytes,
                                    }
                                })
                        })
                        .collect();
                    (snapshot.observed_at, snapshot.complete, rows)
                }
            };
        let mut document = crate::domain::export::ExportDocument {
            metadata: metadata(observed_at, complete),
            rows,
        };
        if request.collection != ExportCollection::Devices {
            document.sort_rows();
        }
        Ok(document)
    }

    pub fn exit_node_candidates(&self) -> Vec<ExitNodeCandidate> {
        let Some(snapshot) = self.local_resource.snapshot.as_ref() else {
            return Vec::new();
        };
        let mut candidates = snapshot
            .peers
            .iter()
            .filter(|device| device.exit_node_option)
            .map(|device| ExitNodeCandidate {
                device_id: device.id.clone(),
                display_name: device.display_name.clone(),
                dns_name: device.dns_name.clone(),
                tailscale_ips: device.tailscale_ips.clone(),
                online: device.online,
                path: device.path.clone(),
                last_probe_ms: match &device.path {
                    crate::domain::device::ConnectionPath::Direct { latency_ms } => *latency_ms,
                    _ => None,
                },
                selected: self.local_preferences.exit_node_id.value.as_ref() == Some(&device.id.0)
                    || self
                        .local_preferences
                        .exit_node_ip
                        .value
                        .as_deref()
                        .is_some_and(|ip| {
                            device.tailscale_ips.iter().any(|candidate| candidate == ip)
                        }),
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            online_rank(left.online)
                .cmp(&online_rank(right.online))
                .then_with(|| probe_rank(left.last_probe_ms).cmp(&probe_rank(right.last_probe_ms)))
                .then_with(|| left.path.label().cmp(right.path.label()))
                .then_with(|| {
                    left.display_name
                        .to_ascii_lowercase()
                        .cmp(&right.display_name.to_ascii_lowercase())
                })
                .then_with(|| left.device_id.cmp(&right.device_id))
        });
        candidates
    }

    fn start_refresh(&mut self, all: bool) -> Vec<Effect> {
        let mut effects = if all {
            self.start_admin_refresh()
        } else {
            self.start_admin_selected_refresh()
        };
        if self.current_route() == Route::Services {
            effects.extend(self.start_services_refresh());
            return effects;
        }
        match self.source_mode {
            SourceMode::Unavailable => {
                if self.admin.profile.is_none() {
                    self.runtime_error = Some("local integration is disabled".to_owned());
                }
                effects
            }
            SourceMode::Mock => {
                self.devices_resource.generation =
                    self.devices_resource.generation.saturating_add(1);
                let generation = self.devices_resource.generation;
                self.devices_resource.health = SourceHealth::Loading;
                self.devices_resource.error = None;
                let scenario = if generation.is_multiple_of(5) {
                    MockLoadScenario::Failure
                } else if generation.is_multiple_of(3) {
                    MockLoadScenario::Stale
                } else {
                    MockLoadScenario::Success
                };
                effects.push(Effect::StartMockLoad {
                    resource: Resource::Devices,
                    generation,
                    scenario,
                });
                effects
            }
            SourceMode::Local => {
                let generation = self.local_resource.generation.saturating_add(1);
                if self.local_discovery_in_flight {
                    effects.push(Effect::CancelLocalDiscovery);
                }
                self.local_resource.begin(generation, self.now);
                self.local_discovery_in_flight = false;
                self.local_preferences_resource.begin(generation, self.now);
                if self.local_executable.is_none() {
                    self.local_discovery_in_flight = true;
                    effects.push(Effect::StartLocalDiscovery {
                        generation,
                        resolution: local_resolution(&self.resolved_config),
                        timeout: self.resolved_config.local.command_timeout,
                    });
                } else {
                    effects.push(Effect::StartLocalSnapshotRefresh {
                        generation,
                        socket_path: self.resolved_config.local.socket_path.clone(),
                        timeout: self.resolved_config.local.command_timeout,
                    });
                }
                effects
            }
        }
    }

    fn select_next_profile(&mut self) -> Vec<Effect> {
        let mut profiles = self.resolved_config.profiles.keys();
        let profile = match self.resolved_config.profile.as_deref() {
            None => profiles.next().cloned(),
            Some(current) => {
                let mut next = false;
                profiles
                    .find_map(|candidate| {
                        if next {
                            Some(candidate.clone())
                        } else if candidate == current {
                            next = true;
                            None
                        } else {
                            None
                        }
                    })
                    .or_else(|| self.resolved_config.profiles.keys().next().cloned())
            }
        };
        match profile {
            Some(profile) => self.switch_profile(Some(profile)),
            None => {
                self.runtime_error = Some("no admin profiles are configured".to_owned());
                Vec::new()
            }
        }
    }

    fn clear_admin_profile(&mut self) -> Vec<Effect> {
        self.switch_profile(None)
    }

    pub fn switch_profile(&mut self, profile: Option<String>) -> Vec<Effect> {
        if self.resolved_config.profile == profile {
            return Vec::new();
        }
        if !self.admin_mutations_in_flight.is_empty()
            || !self.admin_batches_in_flight.is_empty()
            || !self.admin_batch_preflights.is_empty()
        {
            self.runtime_error = Some(
                "finish or cancel the active admin mutation before switching profiles".to_owned(),
            );
            return Vec::new();
        }
        self.close_policy_temp_file();
        self.close_latest_policy_temp_file();
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.close();
        }
        self.policy_workflow = None;
        self.pending_auth_key_request = None;
        self.pending_auth_key_result = None;
        self.pending_credential_revoke = None;
        if let Some(result) = self.secret_result.as_mut() {
            result.close();
        }
        self.secret_result = None;
        self.overlays
            .retain(|overlay| !matches!(overlay, Overlay::PolicyEditor | Overlay::SecretResult));
        let preflight_locks = self
            .admin_preflight_locks
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for mutation_id in preflight_locks {
            self.release_admin_preflight_lock(mutation_id);
        }
        self.release_all_admin_read_locks();
        let previous_profile = self.resolved_config.profile.clone();
        if let Some(previous) = previous_profile.as_ref() {
            self.admin_profile_snapshots
                .insert(previous.clone(), self.admin.clone());
        }
        let (tailnet, profile_read_only) = profile
            .as_deref()
            .and_then(|name| self.resolved_config.profiles.get(name))
            .map_or((None, true), |profile| {
                (Some(profile.tailnet.clone()), profile.read_only)
            });
        self.resolved_config.profile = profile.clone();
        self.admin_environment_token = None;
        let restored = profile
            .as_ref()
            .and_then(|name| self.admin_profile_snapshots.remove(name));
        self.admin = match restored {
            Some(snapshot) => snapshot,
            None => AdminSnapshot::new(
                profile.clone(),
                tailnet.clone(),
                profile_read_only || self.resolved_config.read_only,
                Vec::new(),
            ),
        };
        self.admin.profile = profile.clone();
        self.admin.tailnet = tailnet;
        self.admin.profile_read_only = profile_read_only || self.resolved_config.read_only;
        self.admin_generation = self.admin_generation.saturating_add(1);
        self.health_evaluation_generation = self.health_evaluation_generation.saturating_add(1);
        self.health.clear();
        self.health_findings.clear();
        self.cancel_flow_aggregation();
        self.flow_aggregation_generation = self.flow_aggregation_generation.saturating_add(1);
        self.flow_snapshot = None;
        self.admin_refresh_in_flight = false;
        self.admin_next_refresh = None;
        self.composed_devices.clear();
        self.admin_user_selected = 0;
        self.admin_route_selected = 0;
        self.admin_credential_selected = 0;
        self.admin_activity_selected = 0;
        let mut effects = vec![Effect::CancelAdminRefresh];
        if let Some(previous) = previous_profile {
            effects.push(Effect::DropAdminToken { profile: previous });
        }
        self.update_composed_devices();
        effects.extend(self.start_admin_refresh());
        effects
    }

    fn start_admin_refresh(&mut self) -> Vec<Effect> {
        self.release_all_admin_read_locks();
        let Some(profile) = self.admin.profile.clone() else {
            return Vec::new();
        };
        let Some(profile_config) = self.resolved_config.profiles.get(&profile) else {
            return Vec::new();
        };
        let Some(tailnet) = self.admin.tailnet.clone() else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        if self.admin_refresh_in_flight {
            effects.push(Effect::CancelAdminRefresh);
        }
        self.admin_generation = self.admin_generation.saturating_add(1);
        let generation = self.admin_generation;
        self.admin_refresh_in_flight = true;
        self.admin_next_refresh = None;
        self.admin.devices.begin(generation);
        self.admin.users.begin(generation);
        self.admin.routes.generation = generation;
        self.admin.routes.state = AdminResourceState::Idle;
        self.admin.posture.generation = generation;
        self.admin.posture.state = AdminResourceState::Idle;
        self.admin.posture.error = None;
        self.admin.nameservers.begin(generation);
        self.admin.dns_preferences.begin(generation);
        self.admin.search_paths.begin(generation);
        self.admin.split_dns.begin(generation);
        self.admin.policy.begin(generation);
        self.admin.credentials.begin(generation);
        self.admin.settings.begin(generation);
        self.admin.contacts.begin(generation);
        self.admin.activity.begin(generation);
        effects.push(Effect::StartAdminRefresh {
            profile,
            tailnet,
            credential: profile_config.credential.clone(),
            environment_token: self.admin_environment_token.clone(),
            generation,
            timeout: self.resolved_config.admin.request_timeout,
            audit_window_days: self.admin_audit_window_days,
        });
        effects
    }

    fn start_admin_current_view_refresh(&mut self) -> Vec<Effect> {
        let resources = match self.current_route() {
            Route::Overview | Route::Services => vec![
                AdminRefreshResource::Devices,
                AdminRefreshResource::Users,
                AdminRefreshResource::Nameservers,
                AdminRefreshResource::DnsPreferences,
                AdminRefreshResource::SearchPaths,
                AdminRefreshResource::SplitDns,
                AdminRefreshResource::Policy,
                AdminRefreshResource::Credentials,
                AdminRefreshResource::Settings,
                AdminRefreshResource::Contacts,
                AdminRefreshResource::Activity,
                AdminRefreshResource::Webhooks,
                AdminRefreshResource::LogStreamConfiguration(
                    crate::domain::log_stream::LogType::Configuration,
                ),
                AdminRefreshResource::LogStreamStatus(
                    crate::domain::log_stream::LogType::Configuration,
                ),
                AdminRefreshResource::LogStreamConfiguration(
                    crate::domain::log_stream::LogType::Network,
                ),
                AdminRefreshResource::LogStreamStatus(crate::domain::log_stream::LogType::Network),
            ],
            Route::Devices => vec![AdminRefreshResource::Devices],
            Route::Users => vec![AdminRefreshResource::Users],
            Route::Routes => {
                if let Some(route) = self.selected_admin_route() {
                    return self
                        .start_admin_device_enrichment(Some(route.device_id))
                        .into_iter()
                        .collect();
                }
                vec![AdminRefreshResource::Devices]
            }
            Route::Dns => vec![
                AdminRefreshResource::Nameservers,
                AdminRefreshResource::DnsPreferences,
                AdminRefreshResource::SearchPaths,
                AdminRefreshResource::SplitDns,
            ],
            Route::Access => vec![AdminRefreshResource::Policy],
            Route::Credentials => vec![AdminRefreshResource::Credentials],
            Route::Activity => vec![
                AdminRefreshResource::Activity,
                AdminRefreshResource::Webhooks,
                AdminRefreshResource::LogStreamConfiguration(
                    crate::domain::log_stream::LogType::Configuration,
                ),
                AdminRefreshResource::LogStreamStatus(
                    crate::domain::log_stream::LogType::Configuration,
                ),
                AdminRefreshResource::LogStreamConfiguration(
                    crate::domain::log_stream::LogType::Network,
                ),
                AdminRefreshResource::LogStreamStatus(crate::domain::log_stream::LogType::Network),
            ],
            Route::Settings => vec![
                AdminRefreshResource::Settings,
                AdminRefreshResource::Contacts,
                AdminRefreshResource::NetworkLogSettings,
                AdminRefreshResource::LogStreamConfiguration(
                    crate::domain::log_stream::LogType::Configuration,
                ),
                AdminRefreshResource::LogStreamStatus(
                    crate::domain::log_stream::LogType::Configuration,
                ),
                AdminRefreshResource::LogStreamConfiguration(
                    crate::domain::log_stream::LogType::Network,
                ),
                AdminRefreshResource::LogStreamStatus(crate::domain::log_stream::LogType::Network),
            ],
            Route::Local => vec![AdminRefreshResource::Devices],
        };
        self.start_admin_resource_refresh(resources)
    }

    fn start_admin_selected_refresh(&mut self) -> Vec<Effect> {
        match self.current_route() {
            Route::Devices => self
                .start_admin_device_enrichment(
                    self.views
                        .devices
                        .selected_id
                        .as_ref()
                        .map(|id| id.0.clone()),
                )
                .map_or_else(
                    || self.start_admin_resource_refresh(vec![AdminRefreshResource::Devices]),
                    |effect| vec![effect],
                ),
            Route::Routes => self
                .selected_admin_route()
                .map(|route| {
                    self.start_admin_device_enrichment(Some(route.device_id))
                        .into_iter()
                        .collect()
                })
                .unwrap_or_else(|| {
                    self.start_admin_resource_refresh(vec![AdminRefreshResource::Devices])
                }),
            Route::Users => self.start_admin_resource_refresh(vec![AdminRefreshResource::Users]),
            Route::Dns => self.start_admin_current_view_refresh(),
            Route::Access => self.start_admin_resource_refresh(vec![AdminRefreshResource::Policy]),
            Route::Credentials => {
                self.start_admin_resource_refresh(vec![AdminRefreshResource::Credentials])
            }
            Route::Activity => {
                self.start_admin_resource_refresh(vec![AdminRefreshResource::Activity])
            }
            Route::Settings => self.start_admin_current_view_refresh(),
            Route::Overview | Route::Local | Route::Services => {
                self.start_admin_current_view_refresh()
            }
        }
    }

    fn start_admin_resource_refresh(
        &mut self,
        resources: Vec<AdminRefreshResource>,
    ) -> Vec<Effect> {
        self.release_all_admin_read_locks();
        let Some(profile) = self.admin.profile.clone() else {
            return Vec::new();
        };
        let Some(profile_config) = self.resolved_config.profiles.get(&profile) else {
            return Vec::new();
        };
        let Some(tailnet) = self.admin.tailnet.clone() else {
            return Vec::new();
        };
        if resources.is_empty() {
            return Vec::new();
        }
        self.admin_generation = self.admin_generation.saturating_add(1);
        let generation = self.admin_generation;
        self.admin_refresh_in_flight = true;
        self.admin_next_refresh = None;
        for resource in &resources {
            match resource {
                AdminRefreshResource::Devices => self.admin.devices.begin(generation),
                AdminRefreshResource::DeviceRoutes(_) => self.admin.routes.begin(generation),
                AdminRefreshResource::Users => self.admin.users.begin(generation),
                AdminRefreshResource::Nameservers => self.admin.nameservers.begin(generation),
                AdminRefreshResource::DnsPreferences => {
                    self.admin.dns_preferences.begin(generation)
                }
                AdminRefreshResource::SearchPaths => self.admin.search_paths.begin(generation),
                AdminRefreshResource::SplitDns => self.admin.split_dns.begin(generation),
                AdminRefreshResource::Policy => self.admin.policy.begin(generation),
                AdminRefreshResource::Credentials => self.admin.credentials.begin(generation),
                AdminRefreshResource::Settings => self.admin.settings.begin(generation),
                AdminRefreshResource::Contacts => self.admin.contacts.begin(generation),
                AdminRefreshResource::Activity => self.admin.activity.begin(generation),
                AdminRefreshResource::FlowLogs(_)
                | AdminRefreshResource::Webhooks
                | AdminRefreshResource::LogStreamConfiguration(_)
                | AdminRefreshResource::LogStreamStatus(_)
                | AdminRefreshResource::NetworkLogSettings => {}
            }
        }
        vec![Effect::StartAdminResourceRefresh {
            profile,
            tailnet,
            credential: profile_config.credential.clone(),
            environment_token: self.admin_environment_token.clone(),
            generation,
            timeout: self.resolved_config.admin.request_timeout,
            audit_window_days: self.admin_audit_window_days,
            resources,
        }]
    }

    fn start_admin_device_enrichment(&mut self, selected_id: Option<String>) -> Option<Effect> {
        let profile = self.admin.profile.clone()?;
        let device_id = selected_id?;
        let profile_config = self.resolved_config.profiles.get(&profile)?;
        let admin_device = self
            .admin
            .devices
            .snapshot
            .as_ref()?
            .iter()
            .find(|device| {
                device.stable_id == device_id || device.exact_node_id() == Some(device_id.as_str())
            })?;
        let stable_id = admin_device.stable_id.clone();
        if self.admin_read_locks.contains_key(&stable_id) {
            return None;
        }
        let owner = self.next_mutation_id;
        self.next_mutation_id = self.next_mutation_id.saturating_add(1);
        if !self.admin_resource_locks.try_hold(
            owner,
            [crate::domain::admin_mutation::AdminResourceLockKey::new(
                profile.clone(),
                crate::domain::admin_mutation::AdminResourceKind::Device,
                stable_id.clone(),
            )],
        ) {
            return None;
        }
        self.admin_read_locks.insert(stable_id.clone(), owner);
        Some(Effect::StartAdminDeviceEnrichment {
            profile,
            credential: profile_config.credential.clone(),
            environment_token: self.admin_environment_token.clone(),
            generation: self.admin_generation,
            device_id: stable_id,
            timeout: self.resolved_config.admin.request_timeout,
        })
    }

    fn update_admin(&mut self, event: AdminEvent) -> Vec<Effect> {
        match event {
            AdminEvent::RefreshStarted {
                profile,
                generation,
            } => {
                if self.admin.profile.as_deref() == Some(profile.as_str())
                    && generation == self.admin_generation
                {
                    self.admin_refresh_in_flight = true;
                }
            }
            AdminEvent::RefreshFinished(report) => {
                if self.admin.profile.as_deref() != Some(report.profile.as_str())
                    || report.generation != self.admin_generation
                {
                    return Vec::new();
                }
                self.admin_refresh_in_flight = false;
                self.admin.requested_scopes = report.requested_scopes.clone();
                self.admin_next_refresh = Some(instant_after(
                    Instant::now(),
                    self.resolved_config.admin.refresh_interval,
                ));
                let generation = report.generation;
                let observed_at = report.observed_at;
                apply_admin_result(
                    &mut self.admin.devices,
                    generation,
                    observed_at,
                    report.devices,
                );
                apply_admin_result(&mut self.admin.users, generation, observed_at, report.users);
                if let Some(routes) = report.routes {
                    apply_admin_result(&mut self.admin.routes, generation, observed_at, routes);
                }
                apply_admin_result(
                    &mut self.admin.nameservers,
                    generation,
                    observed_at,
                    report.nameservers,
                );
                apply_admin_result(
                    &mut self.admin.dns_preferences,
                    generation,
                    observed_at,
                    report.dns_preferences,
                );
                apply_admin_result(
                    &mut self.admin.search_paths,
                    generation,
                    observed_at,
                    report.search_paths,
                );
                apply_admin_result(
                    &mut self.admin.split_dns,
                    generation,
                    observed_at,
                    report.split_dns,
                );
                apply_admin_result(
                    &mut self.admin.policy,
                    generation,
                    observed_at,
                    report.policy,
                );
                apply_admin_result(
                    &mut self.admin.credentials,
                    generation,
                    observed_at,
                    report.credentials,
                );
                apply_admin_result(
                    &mut self.admin.settings,
                    generation,
                    observed_at,
                    report.settings,
                );
                apply_admin_result(
                    &mut self.admin.contacts,
                    generation,
                    observed_at,
                    report.contacts,
                );
                apply_admin_result(
                    &mut self.admin.activity,
                    generation,
                    observed_at,
                    report.activity,
                );
                self.refresh_admin_capabilities();
                self.sync_admin_display_devices();
                self.update_composed_devices();
                return self.recompute_health();
            }
            AdminEvent::ResourceRefreshFinished(report) => {
                if self.admin.profile.as_deref() != Some(report.profile.as_str())
                    || report.generation != self.admin_generation
                {
                    return Vec::new();
                }
                self.admin_refresh_in_flight = false;
                self.admin.requested_scopes = report.requested_scopes;
                self.admin_next_refresh = Some(instant_after(
                    Instant::now(),
                    self.resolved_config.admin.refresh_interval,
                ));
                for resource in report.resources {
                    match resource {
                        AdminResourceResult::Devices(result) => {
                            apply_admin_result(
                                &mut self.admin.devices,
                                report.generation,
                                report.observed_at,
                                result,
                            );
                            if self.admin.devices.state == AdminResourceState::Ready {
                                self.admin.routes.generation = report.generation;
                                self.admin.routes.observed_at = Some(report.observed_at);
                                self.admin.routes.state = AdminResourceState::Ready;
                                self.admin.routes.snapshot = None;
                            }
                        }
                        AdminResourceResult::DeviceRoutes(result) => match result {
                            Ok(routes) => {
                                let device_id = routes.device_id.clone();
                                let advertised = routes.advertised.clone();
                                let enabled = routes.enabled.clone();
                                let routes_observed_at = routes.observed_at;
                                if let Some(existing) =
                                    self.admin.routes.snapshot.as_mut().and_then(|values| {
                                        values.iter_mut().find(|value| value.device_id == device_id)
                                    })
                                {
                                    *existing = routes;
                                } else {
                                    self.admin
                                        .routes
                                        .snapshot
                                        .get_or_insert_with(Vec::new)
                                        .push(routes);
                                }
                                if let Some(device) =
                                    self.admin.devices.snapshot.as_mut().and_then(|values| {
                                        values.iter_mut().find(|value| value.stable_id == device_id)
                                    })
                                {
                                    device.advertised_routes_returned = true;
                                    device.advertised_routes = advertised;
                                    device.enabled_routes_returned = true;
                                    device.enabled_routes = enabled;
                                }
                                self.admin.routes.generation = report.generation;
                                self.admin.routes.observed_at = Some(routes_observed_at);
                                self.admin.routes.state = AdminResourceState::Ready;
                                self.admin.routes.error = None;
                            }
                            Err(error) => {
                                self.admin.routes.generation = report.generation;
                                self.admin.routes.state = if self.admin.routes.snapshot.is_some() {
                                    AdminResourceState::Stale
                                } else {
                                    admin_state_for_error(&error)
                                };
                                self.admin.routes.error = Some(error.to_string());
                            }
                        },
                        AdminResourceResult::Users(result) => apply_admin_result(
                            &mut self.admin.users,
                            report.generation,
                            report.observed_at,
                            result,
                        ),
                        AdminResourceResult::Nameservers(result) => apply_admin_result(
                            &mut self.admin.nameservers,
                            report.generation,
                            report.observed_at,
                            result,
                        ),
                        AdminResourceResult::DnsPreferences(result) => apply_admin_result(
                            &mut self.admin.dns_preferences,
                            report.generation,
                            report.observed_at,
                            result,
                        ),
                        AdminResourceResult::SearchPaths(result) => apply_admin_result(
                            &mut self.admin.search_paths,
                            report.generation,
                            report.observed_at,
                            result,
                        ),
                        AdminResourceResult::SplitDns(result) => apply_admin_result(
                            &mut self.admin.split_dns,
                            report.generation,
                            report.observed_at,
                            result,
                        ),
                        AdminResourceResult::Policy(result) => {
                            self.access_explorer_result = None;
                            apply_admin_result(
                                &mut self.admin.policy,
                                report.generation,
                                report.observed_at,
                                result,
                            )
                        }
                        AdminResourceResult::Credentials(result) => apply_admin_result(
                            &mut self.admin.credentials,
                            report.generation,
                            report.observed_at,
                            result,
                        ),
                        AdminResourceResult::Settings(result) => apply_admin_result(
                            &mut self.admin.settings,
                            report.generation,
                            report.observed_at,
                            result,
                        ),
                        AdminResourceResult::Contacts(result) => apply_admin_result(
                            &mut self.admin.contacts,
                            report.generation,
                            report.observed_at,
                            result,
                        ),
                        AdminResourceResult::Activity(result) => apply_admin_result(
                            &mut self.admin.activity,
                            report.generation,
                            report.observed_at,
                            result,
                        ),
                        AdminResourceResult::FlowLogs(result) => match *result {
                            Ok(mut snapshot) => {
                                self.cancel_flow_aggregation();
                                self.flow_aggregation_generation =
                                    self.flow_aggregation_generation.saturating_add(1);
                                snapshot.set_filter(self.flow_filter.clone());
                                snapshot.aggregates = None;
                                self.flow_snapshot = Some(snapshot);
                                let generation = self.flow_generation.generation;
                                let _ = self.flow_generation.cancel(generation);
                            }
                            Err(error) => {
                                self.cancel_flow_aggregation();
                                self.flow_aggregation_generation =
                                    self.flow_aggregation_generation.saturating_add(1);
                                self.flow_snapshot = None;
                                self.runtime_error = Some(error.to_string());
                                let generation = self.flow_generation.generation;
                                let _ = self.flow_generation.cancel(generation);
                            }
                        },
                        AdminResourceResult::Webhooks(result) => match result {
                            Ok((webhooks, _meta)) => self.webhooks = webhooks,
                            Err(error) => self.runtime_error = Some(error.to_string()),
                        },
                        AdminResourceResult::LogStreamConfiguration { log_type, result } => {
                            match result {
                                Ok(configuration) => {
                                    self.log_stream_configurations
                                        .insert(configuration.log_type, configuration);
                                }
                                Err(error @ AdminError::NotFound { .. }) => {
                                    self.log_stream_configurations.remove(&log_type);
                                    self.runtime_error = Some(error.to_string());
                                }
                                Err(error) => self.runtime_error = Some(error.to_string()),
                            }
                        }
                        AdminResourceResult::LogStreamStatus { log_type, result } => match result {
                            Ok(status) => {
                                self.log_stream_statuses.insert(status.log_type, status);
                            }
                            Err(error @ AdminError::NotFound { .. }) => {
                                self.log_stream_statuses.remove(&log_type);
                                self.runtime_error = Some(error.to_string());
                            }
                            Err(error) => self.runtime_error = Some(error.to_string()),
                        },
                        AdminResourceResult::NetworkLogSettings(result) => {
                            apply_admin_result(
                                &mut self.admin.settings,
                                report.generation,
                                report.observed_at,
                                result,
                            );
                        }
                    }
                }
                self.refresh_admin_capabilities();
                self.sync_admin_display_devices();
                self.update_composed_devices();
                let health_effects = self.recompute_health();
                if let Some(targets) = self.pending_batch_retry.take() {
                    if self.admin.devices.state != AdminResourceState::Ready {
                        self.runtime_error = Some(
                            "fresh device state for failed targets was not available; no retry was started"
                                .to_owned(),
                        );
                    } else {
                        return self.begin_retry_batch_preflight(targets);
                    }
                }
                return health_effects;
            }
            AdminEvent::AuthenticationFailed {
                profile,
                generation,
                detail,
            } => {
                if self.admin.profile.as_deref() != Some(profile.as_str())
                    || generation != self.admin_generation
                {
                    return Vec::new();
                }
                self.admin_refresh_in_flight = false;
                self.admin_next_refresh = Some(instant_after(
                    Instant::now(),
                    self.resolved_config.admin.refresh_interval,
                ));
                mark_admin_unauthenticated(&mut self.admin.devices, generation, detail.clone());
                mark_admin_unauthenticated(&mut self.admin.users, generation, detail.clone());
                mark_admin_unauthenticated(&mut self.admin.routes, generation, detail.clone());
                mark_admin_unauthenticated(&mut self.admin.posture, generation, detail.clone());
                mark_admin_unauthenticated(&mut self.admin.nameservers, generation, detail.clone());
                mark_admin_unauthenticated(
                    &mut self.admin.dns_preferences,
                    generation,
                    detail.clone(),
                );
                mark_admin_unauthenticated(
                    &mut self.admin.search_paths,
                    generation,
                    detail.clone(),
                );
                mark_admin_unauthenticated(&mut self.admin.split_dns, generation, detail.clone());
                mark_admin_unauthenticated(&mut self.admin.policy, generation, detail.clone());
                mark_admin_unauthenticated(&mut self.admin.credentials, generation, detail.clone());
                mark_admin_unauthenticated(&mut self.admin.settings, generation, detail.clone());
                mark_admin_unauthenticated(&mut self.admin.contacts, generation, detail.clone());
                mark_admin_unauthenticated(&mut self.admin.activity, generation, detail);
                self.refresh_admin_capabilities();
                self.sync_admin_display_devices();
                self.update_composed_devices();
                return self.recompute_health();
            }
            AdminEvent::DeviceEnrichmentFinished {
                profile,
                generation,
                device,
                routes,
                routes_error,
                posture_present,
                posture_error,
            } => {
                self.release_admin_read_lock(&device.stable_id);
                if self.admin.profile.as_deref() != Some(profile.as_str())
                    || generation != self.admin_generation
                {
                    return Vec::new();
                }
                if let Some(devices) = self.admin.devices.snapshot.as_mut()
                    && let Some(existing) = devices
                        .iter_mut()
                        .find(|existing| existing.stable_id == device.stable_id)
                {
                    *existing = *device;
                    existing.posture_present = posture_present;
                }
                if let Some(routes) = routes {
                    self.admin.routes.generation = generation;
                    let routes_observed_at = routes.observed_at;
                    if let Some(existing) = self.admin.routes.snapshot.as_mut().and_then(|values| {
                        values
                            .iter_mut()
                            .find(|value| value.device_id == routes.device_id)
                    }) {
                        *existing = routes;
                    } else {
                        self.admin
                            .routes
                            .snapshot
                            .get_or_insert_with(Vec::new)
                            .push(routes);
                    }
                    self.admin.routes.state = AdminResourceState::Ready;
                    self.admin.routes.observed_at = Some(routes_observed_at);
                }
                if let Some(error) = routes_error {
                    self.admin.routes.generation = generation;
                    apply_admin_result(&mut self.admin.routes, generation, self.now, Err(error));
                }
                match posture_error {
                    Some(error) => {
                        self.admin.posture.generation = generation;
                        apply_admin_result(
                            &mut self.admin.posture,
                            generation,
                            self.now,
                            Err(error),
                        );
                    }
                    None if posture_present.is_some() => {
                        self.admin.posture.generation = generation;
                        self.admin.posture.succeed(generation, (), self.now);
                    }
                    None => {}
                }
                self.refresh_admin_capabilities();
                self.update_composed_devices();
                return self.recompute_health();
            }
            AdminEvent::DeviceEnrichmentFailed {
                profile,
                generation,
                device_id,
                detail,
            } => {
                self.release_admin_read_lock(&device_id);
                if self.admin.profile.as_deref() != Some(profile.as_str())
                    || generation != self.admin_generation
                {
                    return Vec::new();
                }
                let resource = &mut self.admin.routes;
                resource.generation = generation;
                resource.state = if resource.snapshot.is_some() {
                    AdminResourceState::Stale
                } else {
                    AdminResourceState::Failed
                };
                resource.error = Some(format!("device {device_id}: {detail}"));
                self.admin.posture.generation = generation;
                self.admin.posture.state = if self.admin.posture.snapshot.is_some() {
                    AdminResourceState::Stale
                } else {
                    AdminResourceState::Failed
                };
                self.admin.posture.error = Some(format!("device {device_id}: {detail}"));
                self.refresh_admin_capabilities();
            }
            AdminEvent::PreflightFinished {
                mut request,
                result,
                observed_at,
                owned_device_context,
            } => {
                self.release_admin_preflight_lock(request.mutation_id);
                if self
                    .aborted_admin_batch_children
                    .remove(&request.mutation_id)
                {
                    return Vec::new();
                }
                if let Some(parent_id) =
                    self.admin_batch_preflights
                        .iter()
                        .find_map(|(parent_id, pending)| {
                            pending
                                .requests
                                .contains_key(&request.mutation_id)
                                .then_some(*parent_id)
                        })
                {
                    return self.finish_admin_batch_preflight(
                        parent_id,
                        *request,
                        result,
                        observed_at,
                        owned_device_context,
                    );
                }
                if self.admin.profile.as_deref() != Some(request.profile.as_str()) {
                    return Vec::new();
                }
                let fresh = match result {
                    Ok(fresh) => fresh,
                    Err(error) => {
                        self.runtime_error = Some(format!(
                            "fresh preflight for {} failed: {error}",
                            request.action_id.as_str()
                        ));
                        self.overlays
                            .push(Overlay::OperatorForm(admin_operator_form_state(
                                request.action_id,
                                admin_change_input(&request.change),
                                Some(error.to_string()),
                            )));
                        return Vec::new();
                    }
                };
                if let Some(conflict) = crate::admin::mutation::preflight_conflict(
                    &request.base_snapshot,
                    &fresh,
                    &request.change,
                ) {
                    let detail = conflict
                        .fields
                        .iter()
                        .map(|field| {
                            format!(
                                "{}: base=[{}] fresh=[{}] requested=[{}]",
                                field.field, field.base, field.fresh, field.requested
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let _ = transition(&mut request.state, AdminMutationState::ConflictDetected);
                    self.runtime_error = Some(format!("admin preflight conflict:\n{detail}"));
                    self.overlays
                        .push(Overlay::OperatorForm(admin_operator_form_state(
                            request.action_id,
                            admin_change_input(&request.change),
                            Some(detail),
                        )));
                    return Vec::new();
                }
                let preflight = crate::domain::admin_mutation::AdminPreflight {
                    observed_at,
                    snapshot: fresh.clone(),
                    fields: fresh.values.clone(),
                };
                if let Err(error) = request.set_preflight(preflight) {
                    self.runtime_error = Some(error.to_string());
                    return Vec::new();
                }
                let mut preview = crate::admin::mutation::preview_lines(
                    &request.base_snapshot,
                    &fresh,
                    &request.change,
                );
                preview.extend(admin_preview_context(&request, &fresh));
                preview.extend(owned_device_context);
                let (prompt, required_phrase) = admin_confirmation_text(&request, &fresh);
                self.overlays
                    .push(Overlay::Confirmation(Box::new(ConfirmationState {
                        action_id: request.action_id,
                        mutation: None,
                        admin_mutation: Some(*request),
                        admin_batch: None,
                        service_request: None,
                        operational_mutation: None,
                        handoff: None,
                        prompt,
                        required_phrase,
                        input: String::new(),
                        lose_ssh_checked: false,
                        preview_lines: preview,
                        redacted_argv: Vec::new(),
                        error: None,
                    })));
            }
            AdminEvent::MutationFinished {
                task_id,
                request,
                outcome,
                refresh_resources,
                refresh_local_dns,
            } => {
                if let Some(parent_id) =
                    self.admin_batches_in_flight
                        .iter()
                        .find_map(|(parent_id, batch)| {
                            batch
                                .child_tasks
                                .contains_key(&request.mutation_id)
                                .then_some(*parent_id)
                        })
                {
                    return self.finish_admin_batch_child(
                        parent_id,
                        *request,
                        *outcome,
                        refresh_resources,
                        refresh_local_dns,
                    );
                }
                self.admin_resource_locks.release(request.mutation_id);
                self.admin_mutations_in_flight.remove(&request.mutation_id);
                let _ = self.tasks.set_verification(
                    task_id,
                    format!(
                        "{}; audit candidates: {}",
                        outcome.verification,
                        outcome.audit.candidate_event_ids.len()
                    ),
                );
                let task_succeeded = self
                    .tasks
                    .get(task_id)
                    .is_some_and(|task| task.state == TaskState::Succeeded);
                if task_succeeded {
                    self.add_notification(
                        task_id,
                        crate::task::TaskResultKind::Success,
                        "admin mutation verified",
                    );
                } else {
                    self.add_notification(
                        task_id,
                        crate::task::TaskResultKind::Failure,
                        &outcome.detail,
                    );
                }
                let refresh_resources =
                    self.extend_admin_refresh_for_owned_devices(&request, refresh_resources);
                let mut effects = if refresh_resources.is_empty() {
                    Vec::new()
                } else {
                    self.start_admin_resource_refresh(refresh_resources)
                };
                if refresh_local_dns && self.source_mode == SourceMode::Local {
                    effects.extend(self.start_local_diagnostic(DiagnosticRequest::DnsStatus));
                }
                return effects;
            }
            AdminEvent::OperationalFinished {
                action_id,
                mutation,
                result,
                secret,
            } => {
                match result {
                    Ok(OperationalResult::WebhookVerified { endpoints, detail }) => {
                        self.webhooks = endpoints;
                        self.runtime_error = Some(detail);
                    }
                    Ok(OperationalResult::NetworkLogSettingVerified { enabled, detail }) => {
                        if let Some(value) = enabled
                            && let Some(settings) = self.admin.settings.snapshot.as_mut()
                        {
                            settings.network_flow_logging_on = Some(value);
                        }
                        self.runtime_error = Some(detail);
                    }
                    Ok(OperationalResult::Completed { detail }) => {
                        self.runtime_error = Some(detail);
                    }
                    Err(error) => {
                        self.runtime_error = Some(error.to_string());
                    }
                }
                if let Some(secret) = secret {
                    let credential_id = match &mutation {
                        OperationalMutation::Webhook(WebhookMutation::Create(_)) => None,
                        OperationalMutation::Webhook(WebhookMutation::RotateSecret {
                            endpoint_id,
                        }) => Some(endpoint_id.clone()),
                        _ => None,
                    };
                    let result_id = self.next_secret_result_id;
                    self.next_secret_result_id = self.next_secret_result_id.saturating_add(1);
                    self.secret_result = Some(SecretResult::from_handle(
                        SecretMetadata {
                            result_id,
                            credential_id,
                            credential_type: "webhook signing secret".to_owned(),
                            description: Some("one-time webhook signing secret".to_owned()),
                            created_at: self.now,
                            expires_at: None,
                            warning: "This secret is view-once. It is not listed, persisted, logged, or recoverable after close.".to_owned(),
                        },
                        secret,
                    ));
                    self.overlays.push(Overlay::SecretResult);
                }
                let refresh = match action_id {
                    ActionId::AdminWebhookCreate
                    | ActionId::AdminWebhookEdit
                    | ActionId::AdminWebhookTest
                    | ActionId::AdminWebhookRotateSecret
                    | ActionId::AdminWebhookDelete
                    | ActionId::AdminLogStreamReplace
                    | ActionId::AdminLogStreamDelete
                    | ActionId::AdminNetworkLogsSettings => self.start_admin_current_view_refresh(),
                    _ => Vec::new(),
                };
                return refresh;
            }
            AdminEvent::AccessExplorerFinished { result } => match result {
                Ok(result) => {
                    self.access_explorer_result = Some(result);
                    self.runtime_error = Some(
                        "Access Explorer result is authoritative only for the documented policy preview request"
                            .to_owned(),
                    );
                }
                Err(error) => self.runtime_error = Some(error.to_string()),
            },
            AdminEvent::HealthEvaluationFinished {
                generation,
                snapshot,
                findings,
            } => {
                if generation == self.health_evaluation_generation && self.admin.profile.is_some() {
                    self.health.replace_evaluated(snapshot, findings.clone());
                    self.health_findings = findings;
                }
            }
            AdminEvent::HealthEvaluationFailed { generation, detail } => {
                if generation == self.health_evaluation_generation {
                    self.runtime_error = Some(detail);
                }
            }
            AdminEvent::FlowAggregationFinished { generation, result } => {
                if generation != self.flow_aggregation_generation {
                    return Vec::new();
                }
                self.flow_aggregation_cancellation = None;
                match result {
                    Ok(rows) => {
                        if let Some(snapshot) = self.flow_snapshot.as_mut() {
                            snapshot.mode = crate::domain::flow::FlowMode::Aggregate(vec![
                                AggregateDimension::ReportingNode,
                                AggregateDimension::TrafficClass,
                                AggregateDimension::Protocol,
                            ]);
                            snapshot.aggregates = Some(rows);
                            self.runtime_error = Some(
                                "flow counters aggregated in a cancellable generation".to_owned(),
                            );
                        }
                    }
                    Err(FlowError::Cancelled) => {}
                    Err(error) => self.runtime_error = Some(error.to_string()),
                }
            }
            AdminEvent::AuditCorrelationFinished {
                task_id,
                mutation_id,
                correlation,
            } => {
                self.admin_audit_correlations
                    .insert(mutation_id, correlation.clone());
                let detail = if correlation.candidate_event_ids.is_empty() {
                    format!("mutation {mutation_id}: no matching audit event observed")
                } else if correlation.is_ambiguous() {
                    format!(
                        "mutation {mutation_id}: ambiguous audit candidates [{}]",
                        correlation.candidate_event_ids.join(", ")
                    )
                } else {
                    format!(
                        "mutation {mutation_id}: audit candidate {}",
                        correlation
                            .candidate_event_ids
                            .first()
                            .map_or("not returned", String::as_str)
                    )
                };
                let _ = self.tasks.set_verification(task_id, detail);
            }
            AdminEvent::Failed {
                profile,
                generation,
                detail,
            } => {
                if self.admin.profile.as_deref() == Some(profile.as_str())
                    && generation == self.admin_generation
                {
                    self.admin_refresh_in_flight = false;
                    self.admin_next_refresh = Some(instant_after(
                        Instant::now(),
                        self.resolved_config.admin.refresh_interval,
                    ));
                    mark_admin_failed(&mut self.admin.devices, generation, detail.clone());
                    mark_admin_failed(&mut self.admin.users, generation, detail.clone());
                    mark_admin_failed(&mut self.admin.routes, generation, detail.clone());
                    mark_admin_failed(&mut self.admin.posture, generation, detail.clone());
                    mark_admin_failed(&mut self.admin.nameservers, generation, detail.clone());
                    mark_admin_failed(&mut self.admin.dns_preferences, generation, detail.clone());
                    mark_admin_failed(&mut self.admin.search_paths, generation, detail.clone());
                    mark_admin_failed(&mut self.admin.split_dns, generation, detail.clone());
                    mark_admin_failed(&mut self.admin.policy, generation, detail.clone());
                    mark_admin_failed(&mut self.admin.credentials, generation, detail.clone());
                    mark_admin_failed(&mut self.admin.settings, generation, detail.clone());
                    mark_admin_failed(&mut self.admin.contacts, generation, detail.clone());
                    mark_admin_failed(&mut self.admin.activity, generation, detail);
                    self.refresh_admin_capabilities();
                    self.sync_admin_display_devices();
                    self.update_composed_devices();
                }
            }
        }
        Vec::new()
    }

    fn finish_admin_batch_preflight(
        &mut self,
        parent_id: u64,
        mut request: AdminMutationRequest,
        result: Result<AdminSnapshotFields, AdminError>,
        observed_at: Timestamp,
        owned_device_context: Vec<String>,
    ) -> Vec<Effect> {
        let Some(mut pending) = self.admin_batch_preflights.remove(&parent_id) else {
            return Vec::new();
        };
        if self.admin.profile.as_deref() != Some(request.profile.as_str()) {
            return Vec::new();
        }
        let fresh = match result {
            Ok(fresh) => fresh,
            Err(error) => {
                self.aborted_admin_batch_children
                    .extend(pending.requests.keys().copied());
                self.runtime_error = Some(format!(
                    "batch preflight for {} failed: {error}",
                    request.target_id
                ));
                self.overlays
                    .push(Overlay::OperatorForm(admin_operator_form_state(
                        pending.action_id,
                        admin_change_input(&request.change),
                        Some(error.to_string()),
                    )));
                return Vec::new();
            }
        };
        if let Some(conflict) = crate::admin::mutation::preflight_conflict(
            &request.base_snapshot,
            &fresh,
            &request.change,
        ) {
            self.aborted_admin_batch_children
                .extend(pending.requests.keys().copied());
            let detail = conflict
                .fields
                .iter()
                .map(|field| {
                    format!(
                        "{}: base=[{}] fresh=[{}] requested=[{}]",
                        field.field, field.base, field.fresh, field.requested
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            self.runtime_error = Some(format!(
                "batch preflight conflict for {}:\n{detail}",
                request.target_id
            ));
            self.overlays
                .push(Overlay::OperatorForm(admin_operator_form_state(
                    pending.action_id,
                    admin_change_input(&request.change),
                    Some(detail),
                )));
            return Vec::new();
        }
        let preflight = crate::domain::admin_mutation::AdminPreflight {
            observed_at,
            snapshot: fresh.clone(),
            fields: fresh.values.clone(),
        };
        if let Err(error) = request.set_preflight(preflight) {
            self.aborted_admin_batch_children
                .extend(pending.requests.keys().copied());
            self.runtime_error = Some(error.to_string());
            return Vec::new();
        }
        pending.ready.insert(request.mutation_id, request);
        if pending.ready.len() != pending.requests.len() {
            self.admin_batch_preflights.insert(parent_id, pending);
            return Vec::new();
        }
        let requests = pending.ready.into_values().collect::<Vec<_>>();
        let mut targets = requests
            .iter()
            .map(batch_target)
            .collect::<Vec<BatchTarget>>();
        if let Some(devices) = self.admin.devices.snapshot.as_ref() {
            for target in &mut targets {
                if let Some(device) = devices
                    .iter()
                    .find(|device| device.stable_id == target.target_id)
                {
                    target.target_label = device.display_name().to_owned();
                }
            }
        }
        let batch = BatchMutation::new(parent_id, pending.action_id, targets, 4);
        let mut preview = vec![format!(
            "immutable target list: {} route advertisers",
            requests.len()
        )];
        for request in &requests {
            preview.push(format!("target: {}", request.target_id));
            if let Some(preflight) = request.preflight.as_ref() {
                preview.extend(
                    crate::admin::mutation::preview_lines(
                        &request.base_snapshot,
                        &preflight.snapshot,
                        &request.change,
                    )
                    .into_iter()
                    .map(|line| format!("  {line}")),
                );
                preview.extend(
                    admin_preview_context(request, &preflight.snapshot)
                        .into_iter()
                        .map(|line| format!("  {line}")),
                );
            }
        }
        preview.extend(owned_device_context);
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: pending.action_id,
                mutation: None,
                admin_mutation: None,
                admin_batch: Some(AdminBatchConfirmation { batch, requests }),
                service_request: None,
                operational_mutation: None,
                handoff: None,
                prompt: "Apply this immutable route-approval batch? Each advertiser is verified independently; failures remain per-target."
                    .to_owned(),
                required_phrase: None,
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines: preview,
                redacted_argv: Vec::new(),
                error: None,
            })));
        Vec::new()
    }

    fn finish_admin_batch_child(
        &mut self,
        parent_id: u64,
        request: AdminMutationRequest,
        outcome: crate::admin::mutation::AdminMutationOutcome,
        refresh_resources: Vec<AdminRefreshResource>,
        refresh_local_dns: bool,
    ) -> Vec<Effect> {
        let Some(mut in_flight) = self.admin_batches_in_flight.remove(&parent_id) else {
            return Vec::new();
        };
        self.admin_resource_locks.release(request.mutation_id);
        self.admin_mutations_in_flight.remove(&request.mutation_id);
        let child_outcome = match outcome.state {
            AdminMutationState::Succeeded => {
                crate::domain::admin_mutation::BatchChildOutcome::VerifiedSuccess
            }
            AdminMutationState::SucceededUnverified => {
                crate::domain::admin_mutation::BatchChildOutcome::SucceededUnverified
            }
            AdminMutationState::OutcomeUnknown => {
                crate::domain::admin_mutation::BatchChildOutcome::OutcomeUnknown
            }
            AdminMutationState::Failed
                if outcome.detail.contains("not dispatched")
                    || outcome.verification.contains("not dispatched") =>
            {
                crate::domain::admin_mutation::BatchChildOutcome::FailedBeforeDispatch
            }
            _ => crate::domain::admin_mutation::BatchChildOutcome::Failed,
        };
        in_flight.batch.record(request.target_id, child_outcome);
        let is_route_batch = in_flight.batch.action_id == ActionId::AdminRoutesReplaceApprovals;
        let mut effects = if is_route_batch || refresh_resources.is_empty() {
            Vec::new()
        } else {
            self.start_admin_resource_refresh(refresh_resources)
        };
        if refresh_local_dns && self.source_mode == SourceMode::Local {
            effects.extend(self.start_local_diagnostic(DiagnosticRequest::DnsStatus));
        }
        if !in_flight.pending_requests.is_empty()
            && self
                .tasks
                .get(in_flight.parent_task_id)
                .is_some_and(|task| task.state != TaskState::Cancelling)
        {
            let mut next = in_flight.pending_requests.remove(0);
            let Some(profile_config) = self.resolved_config.profiles.get(&next.profile) else {
                in_flight.batch.record(
                    next.target_id,
                    crate::domain::admin_mutation::BatchChildOutcome::CancelledBeforeDispatch,
                );
                self.admin_resource_locks.release(next.mutation_id);
                self.admin_batches_in_flight.insert(parent_id, in_flight);
                return effects;
            };
            let Some(tailnet) = self.admin.tailnet.clone() else {
                in_flight.batch.record(
                    next.target_id,
                    crate::domain::admin_mutation::BatchChildOutcome::CancelledBeforeDispatch,
                );
                self.admin_resource_locks.release(next.mutation_id);
                self.admin_batches_in_flight.insert(parent_id, in_flight);
                return effects;
            };
            if transition(&mut next.state, AdminMutationState::Dispatching).is_err() {
                in_flight.batch.record(
                    next.target_id,
                    crate::domain::admin_mutation::BatchChildOutcome::FailedBeforeDispatch,
                );
                self.admin_resource_locks.release(next.mutation_id);
                self.admin_batches_in_flight.insert(parent_id, in_flight);
                return effects;
            }
            let next_task_id = self.tasks.create(
                next.action_id,
                format!("route advertiser {}", next.target_id),
                self.now,
                true,
            );
            let _ = self.tasks.set_local_metadata(
                next_task_id,
                vec![next.change.audit_action_class().to_owned()],
                Vec::new(),
            );
            self.admin_mutations_in_flight
                .insert(next.mutation_id, next_task_id);
            in_flight.child_tasks.insert(next.mutation_id, next_task_id);
            effects.push(Effect::StartAdminMutation {
                task_id: next_task_id,
                request: next,
                tailnet,
                credential: profile_config.credential.clone(),
                environment_token: self.admin_environment_token.clone(),
                timeout: self.resolved_config.admin.request_timeout,
            });
        }
        let complete = in_flight.pending_requests.is_empty()
            && in_flight.batch.child_outcomes.len() == in_flight.batch.targets.len();
        if !complete {
            self.admin_batches_in_flight.insert(parent_id, in_flight);
            return effects;
        }
        let has_failure = in_flight.batch.child_outcomes.values().any(|outcome| {
            !matches!(
                outcome,
                crate::domain::admin_mutation::BatchChildOutcome::VerifiedSuccess
            )
        });
        let parent_cancelling = self
            .tasks
            .get(in_flight.parent_task_id)
            .is_some_and(|task| task.state == TaskState::Cancelling);
        let summary = if parent_cancelling {
            "admin batch cancelled; review per-target outcomes"
        } else if has_failure && in_flight.batch.verified_count() > 0 {
            "admin batch partially succeeded; review per-target outcomes"
        } else if has_failure {
            "admin batch failed; review per-target outcomes"
        } else {
            "admin batch verified for every target"
        };
        let detail = format!(
            "{} of {} targets verified",
            in_flight
                .batch
                .child_outcomes
                .values()
                .filter(|outcome| {
                    **outcome == crate::domain::admin_mutation::BatchChildOutcome::VerifiedSuccess
                })
                .count(),
            in_flight.batch.targets.len()
        );
        if parent_cancelling {
            let _ = self
                .tasks
                .cancel(in_flight.parent_task_id, self.now, &detail);
        } else if has_failure {
            let _ = self
                .tasks
                .fail(in_flight.parent_task_id, self.now, summary, &detail);
        } else {
            let _ = self
                .tasks
                .succeed(in_flight.parent_task_id, self.now, summary, &detail);
        }
        self.admin_batch_results
            .insert(in_flight.parent_task_id, in_flight.batch);
        if is_route_batch {
            let mut resources = vec![AdminRefreshResource::Devices];
            resources.extend(
                self.admin_batch_results
                    .get(&in_flight.parent_task_id)
                    .into_iter()
                    .flat_map(|batch| batch.targets.iter())
                    .map(|target| AdminRefreshResource::DeviceRoutes(target.target_id.clone())),
            );
            effects.extend(self.start_admin_resource_refresh(resources));
        }
        effects
    }

    fn refresh_admin_capabilities(&mut self) {
        let entries = [
            ("devices", self.admin.devices.state),
            ("users", self.admin.users.state),
            ("routes", self.admin.routes.state),
            ("devices.posture", self.admin.posture.state),
            ("dns.nameservers", self.admin.nameservers.state),
            ("dns.preferences", self.admin.dns_preferences.state),
            ("dns.search_paths", self.admin.search_paths.state),
            ("dns.split", self.admin.split_dns.state),
            ("access", self.admin.policy.state),
            ("credentials", self.admin.credentials.state),
            ("settings", self.admin.settings.state),
            ("contacts", self.admin.contacts.state),
            ("activity", self.admin.activity.state),
        ];
        for (name, state) in entries {
            self.admin
                .capabilities
                .insert(name.to_owned(), capability_for_state(state));
        }
    }

    fn update_composed_devices(&mut self) {
        let local = self.local_resource.snapshot.as_ref().map(|snapshot| {
            let mut devices = Vec::with_capacity(snapshot.peers.len().saturating_add(1));
            devices.push(snapshot.self_node.clone());
            devices.extend(snapshot.peers.clone());
            devices
        });
        let admin = self.admin.devices.snapshot.clone();
        self.composed_devices = match (local.as_deref(), admin.as_deref()) {
            (Some(local), Some(admin)) => compose_exact_id(local, admin),
            (Some(local), None) => local
                .iter()
                .cloned()
                .map(|device| ComposedDevice {
                    id: device.id.0.clone(),
                    local: Some(device),
                    admin: None,
                })
                .collect(),
            (None, Some(admin)) => admin
                .iter()
                .cloned()
                .map(|device| ComposedDevice {
                    id: device.stable_id.clone(),
                    local: None,
                    admin: Some(device),
                })
                .collect(),
            (None, None) => Vec::new(),
        };
        if self.source_mode == SourceMode::Local && self.admin.profile.is_some() {
            let display = self
                .composed_devices
                .iter()
                .map(Self::display_device_from_composed)
                .collect::<Vec<_>>();
            self.reconcile_selection(Some(&display));
            self.devices_resource.snapshot = display;
            self.devices_resource.observed_at = self.local_resource.last_success_at;
            self.devices_resource.health = match self.local_resource.status {
                LocalResourceStatus::NeverLoaded => SourceHealth::Unavailable,
                LocalResourceStatus::Loading => SourceHealth::Loading,
                LocalResourceStatus::Fresh => SourceHealth::Healthy,
                LocalResourceStatus::Stale => SourceHealth::Stale,
                LocalResourceStatus::Failed => SourceHealth::Error,
            };
            self.devices_resource.error = self
                .local_resource
                .failure
                .as_ref()
                .map(|failure| failure.detail.clone());
            self.reconcile_selection(None);
        }
    }

    fn display_device_from_composed(composed: &ComposedDevice) -> Device {
        match (&composed.local, &composed.admin) {
            (Some(local), _) => local.to_display_device(),
            (None, Some(admin)) => admin.to_display_device(),
            (None, None) => Device {
                id: DeviceId::new(composed.id.clone()),
                display_name: "not returned".to_owned(),
                hostname: "not returned".to_owned(),
                owner: None,
                owner_label: None,
                os: crate::domain::device::OperatingSystem::Unknown("not returned".to_owned()),
                version: "not returned".to_owned(),
                liveness: crate::domain::device::Liveness::Unknown,
                path: crate::domain::device::ConnectionPath::Unknown(
                    "no source snapshot".to_owned(),
                ),
                addresses: Vec::new(),
                advertised_routes: Vec::new(),
                tags: Vec::new(),
                last_seen: None,
                created_at: None,
                rx_bytes: None,
                tx_bytes: None,
                capabilities: crate::domain::device::DeviceCapabilities {
                    exit_node: false,
                    exit_node_option: false,
                    subnet_router: false,
                    ssh: false,
                    funnel: false,
                    shared: false,
                    expired: false,
                    approved: true,
                },
            },
        }
    }

    fn sync_admin_display_devices(&mut self) {
        if self.source_mode == SourceMode::Local && self.local_resource.snapshot.is_some() {
            return;
        }
        if let Some(devices) = self.admin.devices.snapshot.as_ref() {
            let display = devices
                .iter()
                .map(|device| device.to_display_device())
                .collect::<Vec<_>>();
            self.reconcile_selection(Some(&display));
            self.devices_resource.snapshot = display;
            self.devices_resource.generation = self.admin.devices.generation;
            self.devices_resource.observed_at = self.admin.devices.observed_at;
            self.devices_resource.health = SourceHealth::from_admin_state(self.admin.devices.state);
            self.devices_resource.error = self.admin.devices.error.clone();
            self.reconcile_selection(None);
        } else if self.admin.profile.is_some() {
            self.devices_resource.health = SourceHealth::from_admin_state(self.admin.devices.state);
            self.devices_resource.error = self.admin.devices.error.clone();
        }
    }

    fn change_service_section(&mut self, offset: isize) {
        let current = self.views.services.section.index();
        let length = ServiceSection::ALL.len();
        let next = if offset.is_negative() {
            current.saturating_sub(offset.unsigned_abs())
        } else {
            current
                .saturating_add(offset as usize)
                .min(length.saturating_sub(1))
        };
        self.views.services.section = ServiceSection::from_index(next);
        self.views.services.selected = 0;
        self.views.services.scroll = 0;
        self.focus = Focus::Collection;
    }

    fn move_service_selection(&mut self, offset: isize) {
        if self.views.services.section == ServiceSection::Metrics {
            let current = self.views.services.scroll;
            let next = if offset.is_negative() {
                current.saturating_sub(offset.unsigned_abs())
            } else {
                current.saturating_add(offset as usize)
            };
            self.views.services.scroll = next.min(self.metrics_max_scroll());
            return;
        }
        let count = self.service_row_count();
        if count == 0 {
            self.views.services.selected = 0;
            return;
        }
        let current = self.views.services.selected;
        self.views.services.selected = if offset.is_negative() {
            current.saturating_sub(offset.unsigned_abs())
        } else {
            current
                .saturating_add(offset as usize)
                .min(count.saturating_sub(1))
        };
        self.views.services.scroll = self.views.services.selected;
    }

    fn service_row_count(&self) -> usize {
        match self.views.services.section {
            ServiceSection::Serve => self
                .services_snapshot
                .serve
                .value
                .as_ref()
                .map_or(0, |status| status.mappings.len()),
            ServiceSection::Funnel => self
                .services_snapshot
                .funnel
                .value
                .as_ref()
                .map_or(0, |status| status.mappings.len()),
            ServiceSection::Taildrop => self
                .services_snapshot
                .taildrop_targets
                .value
                .as_ref()
                .map_or(0, Vec::len),
            ServiceSection::Taildrive => self
                .services_snapshot
                .taildrive
                .value
                .as_ref()
                .map_or(0, Vec::len),
            ServiceSection::Certificates => self
                .services_snapshot
                .certificate_domains
                .value
                .as_ref()
                .map_or(0, Vec::len),
            ServiceSection::Metrics => usize::from(self.services_snapshot.metrics.value.is_some()),
            ServiceSection::BugReport => {
                usize::from(self.services_snapshot.bug_report.value.is_some())
            }
        }
    }

    pub fn selected_service_mapping(&self) -> Option<ServiceMapping> {
        let selected = self.views.services.selected;
        match self.views.services.section {
            ServiceSection::Serve => self
                .services_snapshot
                .serve
                .value
                .as_ref()
                .and_then(|status| status.mappings.get(selected))
                .cloned(),
            ServiceSection::Funnel => self
                .services_snapshot
                .funnel
                .value
                .as_ref()
                .and_then(|status| status.mappings.get(selected))
                .cloned(),
            _ => None,
        }
    }

    pub fn selected_taildrop_target(&self) -> Option<TaildropTarget> {
        self.services_snapshot
            .taildrop_targets
            .value
            .as_ref()
            .and_then(|targets| targets.get(self.views.services.selected))
            .cloned()
    }

    pub fn selected_taildrive_share(&self) -> Option<TaildriveShare> {
        self.services_snapshot
            .taildrive
            .value
            .as_ref()
            .and_then(|shares| shares.get(self.views.services.selected))
            .cloned()
    }

    fn metrics_max_scroll(&self) -> usize {
        let line_count = self
            .services_snapshot
            .metrics
            .value
            .as_ref()
            .map_or(0, |metrics| metrics.text.lines().count());
        let viewport = usize::from(self.terminal_height.saturating_sub(8)).max(1);
        line_count.saturating_sub(viewport)
    }

    pub fn contextual_actions(&self) -> Vec<ActionId> {
        let mut actions = if self.current_route() == Route::Services {
            self.service_actions_for_section()
        } else if self.admin.profile.is_some() && self.current_route() == Route::Devices {
            vec![
                ActionId::AdminDeviceRename,
                ActionId::AdminDeviceTagsReplace,
                ActionId::AdminDeviceApprove,
                ActionId::AdminDeviceRevokeApproval,
                ActionId::AdminDeviceKeyExpiryConfigure,
                ActionId::AdminDeviceKeyExpireNow,
                ActionId::AdminDeviceDelete,
            ]
        } else if self.admin.profile.is_some() && self.current_route() == Route::Users {
            vec![
                ActionId::AdminUserApprove,
                ActionId::AdminUserRoleChange,
                ActionId::AdminUserSuspend,
                ActionId::AdminUserRestore,
                ActionId::AdminUserDelete,
            ]
        } else if self.admin.profile.is_some() && self.current_route() == Route::Routes {
            vec![ActionId::AdminRoutesReplaceApprovals]
        } else if self.admin.profile.is_some() && self.current_route() == Route::Dns {
            vec![
                ActionId::AdminDnsPreferencesEdit,
                ActionId::AdminDnsNameserversReplace,
                ActionId::AdminDnsSearchPathsReplace,
                ActionId::AdminDnsSplitCreate,
                ActionId::AdminDnsSplitEdit,
                ActionId::AdminDnsSplitRemove,
            ]
        } else if self.source_mode == SourceMode::Mock {
            vec![
                ActionId::MockSuccess,
                ActionId::MockFailure,
                ActionId::MockCancellable,
                ActionId::MockNonCancellable,
            ]
        } else if self.source_mode == SourceMode::Local {
            vec![
                ActionId::LocalConnect,
                ActionId::LocalDisconnect,
                ActionId::LocalPreferencesEdit,
                ActionId::LocalExitNodeSelect,
                ActionId::LocalRoutesEditAdvertisements,
                ActionId::LocalAccountSwitch,
                ActionId::LocalAccountLogin,
                ActionId::LocalAccountLogout,
                ActionId::LocalAccountRemove,
                ActionId::LocalSyspolicyReload,
                ActionId::LocalProbeConnection,
                ActionId::LocalWhois,
                ActionId::LocalSshOpen,
                ActionId::LocalNcOpen,
                ActionId::DiagnosticCopy,
            ]
        } else {
            Vec::new()
        };
        actions.extend(self.phase_eight_resource_actions());
        actions
    }

    pub fn contextual_copy_fields(&self) -> Vec<CopyField> {
        if self.current_route() == Route::Services
            && self.views.services.section == ServiceSection::Metrics
        {
            return vec![CopyField::Metrics];
        }
        if self.current_route() != Route::Devices {
            return Vec::new();
        }
        let mut fields = vec![
            CopyField::DeviceId,
            CopyField::DisplayName,
            CopyField::Hostname,
            CopyField::Owner,
            CopyField::Addresses,
            CopyField::Tags,
        ];
        if self.source_mode == SourceMode::Local {
            fields.push(CopyField::PublicKey);
            fields.push(CopyField::Endpoint);
        }
        fields
    }

    fn service_actions_for_section(&self) -> Vec<ActionId> {
        match self.views.services.section {
            ServiceSection::Serve => vec![
                ActionId::ServicesServeRefresh,
                ActionId::ServicesServeCreate,
                ActionId::ServicesServeEdit,
                ActionId::ServicesServeReset,
            ],
            ServiceSection::Funnel => vec![
                ActionId::ServicesFunnelRefresh,
                ActionId::ServicesFunnelCreate,
                ActionId::ServicesFunnelEdit,
                ActionId::ServicesFunnelReset,
            ],
            ServiceSection::Taildrop => vec![
                ActionId::ServicesTaildropSend,
                ActionId::ServicesTaildropReceive,
            ],
            ServiceSection::Taildrive => {
                let mut actions = vec![ActionId::ServicesDriveRefresh];
                if self.alpha_local_features {
                    actions.extend([
                        ActionId::ServicesDriveShare,
                        ActionId::ServicesDriveRename,
                        ActionId::ServicesDriveUnshare,
                    ]);
                } else {
                    actions.push(ActionId::ServicesDriveEnableAlpha);
                }
                actions
            }
            ServiceSection::Certificates => vec![ActionId::ServicesCertificateObtain],
            ServiceSection::Metrics => vec![ActionId::ServicesMetricsRefresh],
            ServiceSection::BugReport => vec![ActionId::ServicesBugReportCreate],
        }
    }

    fn phase_eight_resource_actions(&self) -> Vec<ActionId> {
        let mut actions = vec![
            ActionId::SavedViewCreate,
            ActionId::SavedViewReplace,
            ActionId::SavedViewRename,
            ActionId::SavedViewDelete,
            ActionId::SavedViewApply,
            ActionId::CollectionExport,
        ];
        match self.current_route() {
            Route::Overview => actions.extend([
                ActionId::OverviewHealthOpenResource,
                ActionId::OverviewHealthRunSuggestedAction,
            ]),
            Route::Access => actions.extend([
                ActionId::AccessExplorerAsk,
                ActionId::AccessExplorerOpenRule,
            ]),
            Route::Activity => actions.extend([
                ActionId::ActivityFlowsSelectWindow,
                ActionId::ActivityFlowsAggregate,
                ActionId::ActivityFlowsOpenDevice,
                ActionId::AdminWebhookCreate,
                ActionId::AdminWebhookEdit,
                ActionId::AdminWebhookTest,
                ActionId::AdminWebhookRotateSecret,
                ActionId::AdminWebhookDelete,
                ActionId::AdminLogStreamReplace,
                ActionId::AdminLogStreamDelete,
                ActionId::AdminNetworkLogsSettings,
            ]),
            Route::Settings => actions.extend([
                ActionId::SettingsAppearance,
                ActionId::AdminLogStreamReplace,
                ActionId::AdminLogStreamDelete,
                ActionId::AdminNetworkLogsSettings,
            ]),
            Route::Devices
            | Route::Users
            | Route::Routes
            | Route::Dns
            | Route::Credentials
            | Route::Local
            | Route::Services => {}
        }
        actions
    }

    fn open_service_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        if !self.action_is_available(action_id) {
            self.runtime_error = self
                .action_unavailable_reason(action_id)
                .or_else(|| Some("service action is unavailable".to_owned()));
            return Vec::new();
        }
        match action_id {
            ActionId::ServicesServeReset => {
                self.open_service_confirmation(ServiceActionRequest::ServeReset)
            }
            ActionId::ServicesFunnelReset => {
                self.open_service_confirmation(ServiceActionRequest::FunnelReset)
            }
            ActionId::ServicesServeCreate | ActionId::ServicesServeEdit => {
                let edit = action_id == ActionId::ServicesServeEdit;
                let input = if edit {
                    self.selected_service_mapping()
                        .filter(|mapping| mapping.exposure == Exposure::Tailnet)
                        .map(|mapping| service_form_mapping(&mapping))
                        .unwrap_or_default()
                } else {
                    "listener=https;port=443;path=/;backend=3000;proxy=none".to_owned()
                };
                if edit && input.is_empty() {
                    self.runtime_error = Some("select a Serve mapping to edit".to_owned());
                    return Vec::new();
                }
                self.overlays.push(Overlay::ServiceForm(ServiceFormState {
                    action_id,
                    input,
                    error: None,
                }));
                Vec::new()
            }
            ActionId::ServicesFunnelCreate | ActionId::ServicesFunnelEdit => {
                let edit = action_id == ActionId::ServicesFunnelEdit;
                let input = if edit {
                    self.selected_service_mapping()
                        .filter(|mapping| mapping.exposure == Exposure::Public)
                        .map(|mapping| service_form_mapping(&mapping))
                        .unwrap_or_default()
                } else {
                    "listener=https;port=443;path=/;backend=3000;proxy=none".to_owned()
                };
                if edit && input.is_empty() {
                    self.runtime_error = Some("select a PUBLIC Funnel mapping to edit".to_owned());
                    return Vec::new();
                }
                self.overlays.push(Overlay::ServiceForm(ServiceFormState {
                    action_id,
                    input,
                    error: None,
                }));
                Vec::new()
            }
            ActionId::ServicesTaildropSend => {
                let target = self
                    .selected_taildrop_target()
                    .filter(TaildropTarget::available)
                    .map_or_else(String::new, |target| target.command_target);
                self.overlays.push(Overlay::ServiceForm(ServiceFormState {
                    action_id,
                    input: format!("target={target};files="),
                    error: None,
                }));
                Vec::new()
            }
            ActionId::ServicesTaildropReceive => {
                self.overlays.push(Overlay::ServiceForm(ServiceFormState {
                    action_id,
                    input: "directory=;conflict=rename;wait=false".to_owned(),
                    error: None,
                }));
                Vec::new()
            }
            ActionId::ServicesDriveShare => {
                self.overlays.push(Overlay::ServiceForm(ServiceFormState {
                    action_id,
                    input: "name=;path=".to_owned(),
                    error: None,
                }));
                Vec::new()
            }
            ActionId::ServicesDriveRename => {
                let input = self.selected_taildrive_share().map_or_else(
                    || "old=;new=".to_owned(),
                    |share| format!("old={};new={}", share.name, share.name),
                );
                self.overlays.push(Overlay::ServiceForm(ServiceFormState {
                    action_id,
                    input,
                    error: None,
                }));
                Vec::new()
            }
            ActionId::ServicesDriveUnshare => {
                let Some(share) = self.selected_taildrive_share() else {
                    self.runtime_error = Some("select a Taildrive share to unshare".to_owned());
                    return Vec::new();
                };
                self.open_service_confirmation(ServiceActionRequest::TaildriveUnshare {
                    name: share.name,
                })
            }
            ActionId::ServicesCertificateObtain => {
                let domain = self
                    .services_snapshot
                    .certificate_domains
                    .value
                    .as_ref()
                    .and_then(|domains| domains.first())
                    .cloned()
                    .unwrap_or_default();
                self.overlays.push(Overlay::ServiceForm(ServiceFormState {
                    action_id,
                    input: format!("domain={domain};cert=;key=;min-validity="),
                    error: None,
                }));
                Vec::new()
            }
            ActionId::ServicesMetricsRefresh => {
                self.start_service_request(ServiceActionRequest::Metrics)
            }
            ActionId::ServicesBugReportCreate => {
                self.overlays.push(Overlay::ServiceForm(ServiceFormState {
                    action_id,
                    input: "diagnose=false;note=".to_owned(),
                    error: None,
                }));
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn accept_service_form(&mut self, state: ServiceFormState) -> Vec<Effect> {
        match self.parse_service_form(&state) {
            Ok(request) => {
                self.overlays.pop();
                if request.action_id() == ActionId::ServicesMetricsRefresh {
                    self.start_service_request(request)
                } else {
                    self.open_service_confirmation(request)
                }
            }
            Err(error) => {
                if let Some(Overlay::ServiceForm(current)) = self.overlays.last_mut() {
                    current.error = Some(error);
                }
                Vec::new()
            }
        }
    }

    fn parse_service_form(&self, state: &ServiceFormState) -> Result<ServiceActionRequest, String> {
        let fields = parse_service_fields(&state.input)?;
        match state.action_id {
            ActionId::ServicesServeCreate | ActionId::ServicesServeEdit => {
                let mapping = self.parse_mapping_form(&fields, Exposure::Tailnet)?;
                if state.action_id == ActionId::ServicesServeEdit
                    && !self
                        .selected_service_mapping()
                        .is_some_and(|selected| selected.exact_identity_matches(&mapping))
                {
                    return Err(
                        "Serve edit cannot replace a different listener and mount; create a new mapping instead"
                            .to_owned(),
                    );
                }
                Ok(ServiceActionRequest::Serve {
                    mapping,
                    edit: state.action_id == ActionId::ServicesServeEdit,
                })
            }
            ActionId::ServicesFunnelCreate | ActionId::ServicesFunnelEdit => {
                let mapping = self.parse_mapping_form(&fields, Exposure::Public)?;
                if state.action_id == ActionId::ServicesFunnelEdit
                    && !self
                        .selected_service_mapping()
                        .is_some_and(|selected| selected.exact_identity_matches(&mapping))
                {
                    return Err(
                        "PUBLIC Funnel edit cannot replace a different listener and mount; create a new mapping instead"
                            .to_owned(),
                    );
                }
                Ok(ServiceActionRequest::Funnel {
                    mapping,
                    edit: state.action_id == ActionId::ServicesFunnelEdit,
                })
            }
            ActionId::ServicesTaildropSend => {
                let target_name = required_field(&fields, "target")?;
                let target = self
                    .services_snapshot
                    .taildrop_targets
                    .value
                    .as_ref()
                    .and_then(|targets| {
                        targets
                            .iter()
                            .find(|target| target.command_target == target_name)
                    })
                    .cloned()
                    .ok_or_else(|| {
                        "target must exactly match a discovered Taildrop target".to_owned()
                    })?;
                if !target.available() {
                    return Err("the selected Taildrop target is unavailable".to_owned());
                }
                let files = required_field(&fields, "files")?
                    .split('|')
                    .filter(|path| !path.is_empty())
                    .map(std::path::PathBuf::from)
                    .map(|path| validate_regular_file(&path))
                    .collect::<Result<Vec<_>, _>>()?;
                if files.is_empty() {
                    return Err("select at least one existing regular file".to_owned());
                }
                Ok(ServiceActionRequest::TaildropSend(TaildropSendRequest {
                    files,
                    target,
                }))
            }
            ActionId::ServicesTaildropReceive => {
                let directory = std::path::PathBuf::from(required_field(&fields, "directory")?);
                validate_receive_directory(&directory)?;
                let conflict = TaildropConflict::parse(required_field(&fields, "conflict")?)
                    .ok_or_else(|| "conflict must be skip, overwrite, or rename".to_owned())?;
                let wait = parse_bool_field(&fields, "wait")?;
                Ok(ServiceActionRequest::TaildropReceive(
                    TaildropReceiveRequest {
                        directory,
                        conflict,
                        wait,
                    },
                ))
            }
            ActionId::ServicesDriveShare => {
                let input_name = required_field(&fields, "name")?.to_owned();
                let normalized_name = normalize_share_name(&input_name)?;
                let path = std::path::PathBuf::from(required_field(&fields, "path")?);
                if !std::fs::metadata(&path)
                    .map(|metadata| metadata.is_dir())
                    .unwrap_or(false)
                {
                    return Err("share path must be an existing directory".to_owned());
                }
                if self
                    .services_snapshot
                    .taildrive
                    .value
                    .as_ref()
                    .is_some_and(|shares| shares.iter().any(|share| share.name == normalized_name))
                {
                    return Err("a share with that normalized name already exists".to_owned());
                }
                Ok(ServiceActionRequest::TaildriveShare {
                    input_name,
                    normalized_name,
                    path,
                })
            }
            ActionId::ServicesDriveRename => {
                let old_name = required_field(&fields, "old")?.to_owned();
                let input_name = required_field(&fields, "new")?.to_owned();
                let normalized_name = normalize_share_name(&input_name)?;
                if !self
                    .services_snapshot
                    .taildrive
                    .value
                    .as_ref()
                    .is_some_and(|shares| shares.iter().any(|share| share.name == old_name))
                {
                    return Err("old share name was not returned by the current list".to_owned());
                }
                if self
                    .services_snapshot
                    .taildrive
                    .value
                    .as_ref()
                    .is_some_and(|shares| {
                        shares
                            .iter()
                            .any(|share| share.name == normalized_name && share.name != old_name)
                    })
                {
                    return Err("new normalized share name already exists".to_owned());
                }
                Ok(ServiceActionRequest::TaildriveRename {
                    old_name,
                    input_name,
                    normalized_name,
                })
            }
            ActionId::ServicesCertificateObtain => {
                let domain = required_field(&fields, "domain")?.to_owned();
                let certificate_path = std::path::PathBuf::from(required_field(&fields, "cert")?);
                let key_path = std::path::PathBuf::from(required_field(&fields, "key")?);
                let min_validity = optional_field(&fields, "min-validity")
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
                let request = CertificateRequest {
                    domain,
                    certificate_path,
                    key_path,
                    min_validity,
                    overwrites_existing: false,
                };
                let eligible = self
                    .services_snapshot
                    .certificate_domains
                    .value
                    .clone()
                    .unwrap_or_default();
                request.validate(&eligible)?;
                let overwrites_existing =
                    request.certificate_path.exists() || request.key_path.exists();
                Ok(ServiceActionRequest::Certificate(CertificateRequest {
                    overwrites_existing,
                    ..request
                }))
            }
            ActionId::ServicesBugReportCreate => {
                let diagnose = parse_bool_field(&fields, "diagnose")?;
                let note = optional_field(&fields, "note")
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
                let request = BugReportRequest { note, diagnose };
                request.validate()?;
                Ok(ServiceActionRequest::BugReport(request))
            }
            _ => Err("this service action does not accept a form".to_owned()),
        }
    }

    fn parse_mapping_form(
        &self,
        fields: &BTreeMap<String, String>,
        exposure: Exposure,
    ) -> Result<ServiceMapping, String> {
        let listener_name = required_field(fields, "listener")?;
        let port = required_field(fields, "port")?
            .parse::<Port>()
            .map_err(|error| error.to_string())?;
        let listener = match listener_name.to_ascii_lowercase().as_str() {
            "https" => Listener::Https(port),
            "http" if exposure == Exposure::Tailnet => Listener::Http(port),
            "tcp" => Listener::Tcp(port),
            "tls-terminated-tcp" | "tls_terminated_tcp" => Listener::TlsTerminatedTcp(port),
            _ => return Err("listener is unsupported for this section".to_owned()),
        };
        let is_public = exposure == Exposure::Public;
        let mount = PathMount::parse(optional_field(fields, "path").unwrap_or("/"))
            .map_err(|error| error.to_string())?;
        let backend = Backend::parse(required_field(fields, "backend")?)
            .map_err(|error| error.to_string())?;
        if matches!(backend, Backend::UnixSocket(_)) && !cfg!(unix) {
            return Err("Unix socket backends are unavailable on this platform".to_owned());
        }
        let proxy_protocol =
            ProxyProtocol::parse(optional_field(fields, "proxy").unwrap_or("none"))
                .map_err(|error| error.to_string())?;
        let mapping = ServiceMapping {
            exposure,
            listener,
            mount,
            backend,
            proxy_protocol,
            hostname: optional_field(fields, "hostname").map(str::to_owned),
        };
        if !self
            .local_capabilities
            .supports_service_listener(&mapping.listener, is_public)
        {
            return Err(format!(
                "{} listeners are unsupported by this CLI",
                mapping.listener.label()
            ));
        }
        mapping.validate().map_err(|error| error.to_string())?;
        Ok(mapping)
    }

    fn open_service_confirmation(&mut self, request: ServiceActionRequest) -> Vec<Effect> {
        let Some((preview_lines, redacted_argv)) = self.service_preview(&request) else {
            self.runtime_error = Some("service command preview is unavailable".to_owned());
            return Vec::new();
        };
        let (prompt, required_phrase) = service_confirmation_text(&request);
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: request.action_id(),
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: Some(request),
                operational_mutation: None,
                handoff: None,
                prompt,
                required_phrase,
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines,
                redacted_argv,
                error: None,
            })));
        Vec::new()
    }

    fn service_preview(
        &self,
        request: &ServiceActionRequest,
    ) -> Option<(Vec<String>, Vec<String>)> {
        let command_path = self
            .local_executable
            .as_ref()
            .map_or(std::path::Path::new("tailscale"), |value| {
                value.path.as_path()
            });
        let timeout = self.resolved_config.local.command_timeout;
        let command = match request {
            ServiceActionRequest::Serve { mapping, .. }
            | ServiceActionRequest::Funnel { mapping, .. } => {
                services::mapping_command(command_path, timeout, mapping, true).ok()?
            }
            ServiceActionRequest::ServeReset => {
                services::serve_reset_command(command_path, timeout)
            }
            ServiceActionRequest::FunnelReset => {
                services::funnel_reset_command(command_path, timeout)
            }
            ServiceActionRequest::TaildropSend(request) => transfers::taildrop_send_command(
                command_path,
                timeout,
                &request
                    .files
                    .iter()
                    .map(|file| file.path.clone())
                    .collect::<Vec<_>>(),
                &request.target.command_target,
            )
            .ok()?,
            ServiceActionRequest::TaildropReceive(request) => transfers::taildrop_receive_command(
                command_path,
                timeout,
                &request.directory,
                request.conflict,
                request.wait,
            )
            .ok()?,
            ServiceActionRequest::TaildriveShare {
                normalized_name,
                path: share_path,
                ..
            } => transfers::drive_share_command(command_path, timeout, normalized_name, share_path)
                .ok()?,
            ServiceActionRequest::TaildriveRename {
                old_name,
                normalized_name,
                ..
            } => transfers::drive_rename_command(command_path, timeout, old_name, normalized_name)
                .ok()?,
            ServiceActionRequest::TaildriveUnshare { name } => {
                transfers::drive_unshare_command(command_path, timeout, name).ok()?
            }
            ServiceActionRequest::Certificate(request) => {
                certificates::certificate_command(command_path, timeout, request).ok()?
            }
            ServiceActionRequest::Metrics => {
                services::metrics_command(command_path, timeout, 256 * 1024)
            }
            ServiceActionRequest::BugReport(request) => services::bugreport_command(
                command_path,
                timeout,
                request.note.as_deref(),
                request.diagnose,
            )
            .ok()?,
        };
        let argv = command
            .args
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let mut preview = vec!["direct argv (each line is one argument):".to_owned()];
        preview.extend(
            argv.iter()
                .enumerate()
                .map(|(index, value)| format!("  argv[{index}] = {value:?}")),
        );
        if matches!(
            request,
            ServiceActionRequest::Funnel { .. } | ServiceActionRequest::FunnelReset
        ) {
            preview.insert(0, "PUBLIC: this changes public reachability".to_owned());
        }
        if let ServiceActionRequest::TaildriveShare {
            input_name,
            normalized_name,
            ..
        } = request
        {
            preview.push(format!("input share name: {input_name}"));
            preview.push(format!("normalized share name: {normalized_name}"));
        }
        Some((preview, argv))
    }

    fn start_service_request(&mut self, request: ServiceActionRequest) -> Vec<Effect> {
        let action_id = request.action_id();
        if !self.action_is_available(action_id) {
            self.runtime_error = self
                .action_unavailable_reason(action_id)
                .or_else(|| Some("service action is unavailable".to_owned()));
            return Vec::new();
        }
        if let Err(error) = self.revalidate_service_request(&request) {
            self.runtime_error = Some(error);
            return Vec::new();
        }
        if let Some(key) = request.conflict_key()
            && self.service_locks.iter().any(|(held, _)| held == &key)
        {
            self.runtime_error = Some("another task is changing this service resource".to_owned());
            return Vec::new();
        }
        let Some(executable) = self.local_executable.clone() else {
            self.runtime_error = Some("tailscale executable has not been discovered".to_owned());
            return Vec::new();
        };
        match &request {
            ServiceActionRequest::Metrics => {
                self.services_snapshot
                    .metrics
                    .begin(self.services_snapshot.generation);
            }
            ServiceActionRequest::BugReport(_) => {
                self.services_snapshot
                    .bug_report
                    .begin(self.services_snapshot.generation);
            }
            _ => {}
        }
        let task_id = self
            .tasks
            .create(action_id, request.target_label(), self.now, true);
        if let Some(key) = request.conflict_key() {
            self.service_locks.push((key, task_id));
        }
        if let Some((fields, argv)) = self.service_task_metadata(&request) {
            let _ = self.tasks.set_local_metadata(task_id, fields, argv);
        }
        vec![Effect::StartServiceTask {
            task_id,
            executable,
            timeout: self.resolved_config.local.command_timeout,
            request,
        }]
    }

    fn revalidate_service_request(&self, request: &ServiceActionRequest) -> Result<(), String> {
        match request {
            ServiceActionRequest::Serve { mapping, edit } => {
                mapping.validate().map_err(|error| error.to_string())?;
                if mapping.exposure != Exposure::Tailnet {
                    return Err("Serve requests must remain tailnet-only".to_owned());
                }
                if !self
                    .local_capabilities
                    .supports_service_listener(&mapping.listener, false)
                {
                    return Err(format!(
                        "{} Serve listeners are unsupported by this CLI",
                        mapping.listener.label()
                    ));
                }
                if *edit
                    && !self
                        .services_snapshot
                        .serve
                        .value
                        .as_ref()
                        .is_some_and(|status| {
                            status
                                .mappings
                                .iter()
                                .any(|actual| actual.exact_identity_matches(mapping))
                        })
                {
                    return Err(
                        "the selected Serve mapping changed; refresh and create or edit again"
                            .to_owned(),
                    );
                }
                validate_mapping_backend(mapping)
            }
            ServiceActionRequest::ServeReset => Ok(()),
            ServiceActionRequest::Funnel { mapping, edit } => {
                mapping.validate().map_err(|error| error.to_string())?;
                if mapping.exposure != Exposure::Public {
                    return Err("Funnel requests must remain PUBLIC".to_owned());
                }
                if matches!(mapping.listener, Listener::Http(_)) {
                    return Err("HTTP is not offered as a public Funnel listener".to_owned());
                }
                if !self
                    .local_capabilities
                    .supports_service_listener(&mapping.listener, true)
                {
                    return Err(format!(
                        "{} Funnel listeners are unsupported by this CLI",
                        mapping.listener.label()
                    ));
                }
                if *edit
                    && !self
                        .services_snapshot
                        .funnel
                        .value
                        .as_ref()
                        .is_some_and(|status| {
                            status
                                .mappings
                                .iter()
                                .any(|actual| actual.exact_identity_matches(mapping))
                        })
                {
                    return Err(
                        "the selected PUBLIC Funnel mapping changed; refresh and edit again"
                            .to_owned(),
                    );
                }
                validate_mapping_backend(mapping)
            }
            ServiceActionRequest::FunnelReset => Ok(()),
            ServiceActionRequest::TaildropSend(request) => {
                let target = self
                    .services_snapshot
                    .taildrop_targets
                    .value
                    .as_ref()
                    .and_then(|targets| {
                        targets
                            .iter()
                            .find(|target| target.command_target == request.target.command_target)
                    })
                    .ok_or_else(|| "the Taildrop target is no longer listed".to_owned())?;
                if !target.available() {
                    return Err("the selected Taildrop target is no longer available".to_owned());
                }
                for file in &request.files {
                    validate_regular_file(&file.path)
                        .map_err(|error| format!("{}: {error}", file.path.display()))?;
                }
                Ok(())
            }
            ServiceActionRequest::TaildropReceive(request) => {
                validate_receive_directory(&request.directory)
            }
            ServiceActionRequest::TaildriveShare {
                normalized_name,
                path,
                ..
            } => {
                if !self.alpha_local_features {
                    return Err("Taildrive is alpha and disabled for this run".to_owned());
                }
                if !std::fs::metadata(path)
                    .map(|metadata| metadata.is_dir())
                    .unwrap_or(false)
                {
                    return Err("share path must remain an existing directory".to_owned());
                }
                if self
                    .services_snapshot
                    .taildrive
                    .value
                    .as_ref()
                    .is_some_and(|shares| shares.iter().any(|share| share.name == *normalized_name))
                {
                    return Err("a share with that normalized name now exists".to_owned());
                }
                Ok(())
            }
            ServiceActionRequest::TaildriveRename {
                old_name,
                normalized_name,
                ..
            } => {
                let shares = self
                    .services_snapshot
                    .taildrive
                    .value
                    .as_ref()
                    .ok_or_else(|| "Taildrive shares are no longer verified".to_owned())?;
                if !shares.iter().any(|share| share.name == *old_name) {
                    return Err("the old Taildrive share no longer exists".to_owned());
                }
                if shares
                    .iter()
                    .any(|share| share.name == *normalized_name && share.name != *old_name)
                {
                    return Err("the new normalized Taildrive name now exists".to_owned());
                }
                Ok(())
            }
            ServiceActionRequest::TaildriveUnshare { name } => {
                if !self
                    .services_snapshot
                    .taildrive
                    .value
                    .as_ref()
                    .is_some_and(|shares| shares.iter().any(|share| share.name == *name))
                {
                    return Err("the selected Taildrive share is no longer listed".to_owned());
                }
                Ok(())
            }
            ServiceActionRequest::Certificate(request) => {
                if self.services_snapshot.certificate_domains.status != ServiceResourceStatus::Ready
                {
                    return Err("certificate domains are no longer verified".to_owned());
                }
                let Some(eligible) = self.services_snapshot.certificate_domains.value.as_deref()
                else {
                    return Err("certificate domains are no longer verified".to_owned());
                };
                request.validate(eligible)
            }
            ServiceActionRequest::Metrics => Ok(()),
            ServiceActionRequest::BugReport(request) => request.validate(),
        }
    }

    fn service_task_metadata(
        &self,
        request: &ServiceActionRequest,
    ) -> Option<(Vec<String>, Vec<String>)> {
        let (_, argv) = self.service_preview(request)?;
        let fields = match request {
            ServiceActionRequest::Serve { mapping, .. }
            | ServiceActionRequest::Funnel { mapping, .. } => {
                vec![
                    "listener".to_owned(),
                    "mount".to_owned(),
                    mapping.backend.label().to_owned(),
                ]
            }
            ServiceActionRequest::TaildropSend(request) => {
                let mut fields = vec!["target".to_owned()];
                fields.extend((0..request.files.len()).map(|index| format!("file-{index}")));
                fields
            }
            ServiceActionRequest::TaildropReceive(request) => {
                vec!["directory".to_owned(), request.conflict.label().to_owned()]
            }
            ServiceActionRequest::TaildriveShare { .. } => {
                vec!["share name".to_owned(), "directory".to_owned()]
            }
            ServiceActionRequest::TaildriveRename { .. } => {
                vec!["old name".to_owned(), "new name".to_owned()]
            }
            ServiceActionRequest::TaildriveUnshare { .. } => vec!["share name".to_owned()],
            ServiceActionRequest::Certificate(_) => {
                vec![
                    "domain".to_owned(),
                    "certificate path".to_owned(),
                    "key path".to_owned(),
                ]
            }
            ServiceActionRequest::Metrics => Vec::new(),
            ServiceActionRequest::BugReport(_) => vec!["diagnostic note".to_owned()],
            ServiceActionRequest::ServeReset | ServiceActionRequest::FunnelReset => Vec::new(),
        };
        Some((fields, argv))
    }

    fn start_services_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        match action_id {
            ActionId::ServicesMetricsRefresh => {
                self.open_service_action(ActionId::ServicesMetricsRefresh)
            }
            _ => self.start_services_refresh(),
        }
    }

    fn start_services_refresh(&mut self) -> Vec<Effect> {
        if self.source_mode != SourceMode::Local {
            self.runtime_error = Some("local services require the local source".to_owned());
            return Vec::new();
        }
        let Some(executable) = self.local_executable.clone() else {
            self.runtime_error = Some("tailscale executable has not been discovered".to_owned());
            return Vec::new();
        };
        let generation = self.services_snapshot.generation.saturating_add(1);
        self.services_snapshot.begin(generation);
        self.local_services_refresh_in_flight = true;
        vec![Effect::StartLocalServicesRefresh {
            generation,
            executable,
            timeout: self.resolved_config.local.command_timeout,
            alpha_enabled: self.alpha_local_features,
        }]
    }

    fn start_task(
        &mut self,
        action_id: ActionId,
        behavior: MockTaskBehavior,
        cancellable: bool,
    ) -> Vec<Effect> {
        let id = self
            .tasks
            .create(action_id, "mock simulation", self.now, cancellable);
        vec![Effect::StartMockTask {
            task_id: id,
            behavior,
        }]
    }

    fn open_local_diagnostics(&mut self) -> Vec<Effect> {
        let actions = vec![
            ActionId::LocalNetcheck,
            ActionId::LocalNetcheckLive,
            ActionId::LocalDnsStatus,
            ActionId::LocalDnsQuery,
            ActionId::LocalWhois,
            ActionId::DiagnosticCopy,
        ];
        if let Err(error) = action::validate_transient_sequences(&actions) {
            self.runtime_error = Some(error);
            return Vec::new();
        }
        self.interaction = InteractionMode::Transient(TransientMenuState {
            kind: TransientKind::Action,
            actions,
            fields: Vec::new(),
            prefix: None,
            message: None,
        });
        Vec::new()
    }

    fn start_probe_connection(&mut self) -> Vec<Effect> {
        let target = self
            .selected_local_device()
            .and_then(LocalDevice::preferred_target)
            .map(str::to_owned);
        let Some(target) = target else {
            self.runtime_error = Some("selected peer has no DNS name or Tailscale IP".to_owned());
            return Vec::new();
        };
        self.start_local_diagnostic(DiagnosticRequest::Ping { target })
    }

    fn start_local_diagnostic(&mut self, request: DiagnosticRequest) -> Vec<Effect> {
        let Some(executable) = self.local_executable.clone() else {
            self.runtime_error = Some("local executable has not been discovered".to_owned());
            return Vec::new();
        };
        if !self.request_capability_available(&request) {
            self.runtime_error = Some(format!(
                "{} is unavailable for this client",
                request.label()
            ));
            return Vec::new();
        }
        if let DiagnosticRequest::DnsQuery { name, record_type } = &request
            && let Err(error) = diagnostics::validate_dns_query(name, record_type.label())
        {
            self.runtime_error = Some(error);
            return Vec::new();
        }
        if let DiagnosticRequest::Whois { target, .. } = &request
            && let Err(error) = diagnostics::validate_whois_target(target)
        {
            self.runtime_error = Some(error);
            return Vec::new();
        }
        let action_id = diagnostic_action(&request);
        let target_label = match &request {
            DiagnosticRequest::Ping { target } => format!("ping target {target}"),
            DiagnosticRequest::DnsQuery { name, record_type } => {
                format!("dns query {name} {}", record_type.label())
            }
            DiagnosticRequest::Whois { target, .. } => format!("whois {target}"),
            _ => request.label().to_owned(),
        };
        let task_id = self.tasks.create(action_id, target_label, self.now, true);
        self.local_diagnostics
            .insert(task_id, DiagnosticState::new(request.label()));
        vec![Effect::StartLocalDiagnostic {
            task_id,
            executable,
            timeout: self.resolved_config.local.command_timeout,
            request,
        }]
    }

    fn request_capability_available(&self, request: &DiagnosticRequest) -> bool {
        match request {
            DiagnosticRequest::Ping { .. } => self.local_capabilities.ping,
            DiagnosticRequest::Netcheck { live } => {
                if *live {
                    self.local_capabilities.netcheck_json_line
                } else {
                    self.local_capabilities.netcheck_json
                }
            }
            DiagnosticRequest::DnsStatus => self.local_capabilities.dns_status_json,
            DiagnosticRequest::DnsQuery { .. } => self.local_capabilities.dns_query_json,
            DiagnosticRequest::Whois { .. } => self.local_capabilities.whois_json,
        }
    }

    fn open_whois_input(&mut self) -> Vec<Effect> {
        let seed = match self
            .selected_local_device()
            .and_then(|device| device.tailscale_ips.first())
        {
            Some(value) => value.clone(),
            None => String::new(),
        };
        self.overlays
            .push(Overlay::DiagnosticInput(DiagnosticInputState {
                kind: DiagnosticInputKind::Whois,
                input: seed,
                secondary: String::new(),
                error: None,
            }));
        Vec::new()
    }

    fn accept_diagnostic_input(&mut self, kind: DiagnosticInputKind, input: &str) -> Vec<Effect> {
        match kind {
            DiagnosticInputKind::DnsQuery => {
                let mut parts = input.split_ascii_whitespace();
                let Some(name) = parts.next() else {
                    return self.set_diagnostic_input_error("enter a DNS name");
                };
                let record_type = parts.next().map_or("A", |value| value);
                if parts.next().is_some() {
                    return self
                        .set_diagnostic_input_error("enter a DNS name and optional record type");
                }
                match diagnostics::validate_dns_query(name, record_type) {
                    Ok(record_type) => {
                        self.overlays.pop();
                        self.start_local_diagnostic(DiagnosticRequest::DnsQuery {
                            name: name.to_owned(),
                            record_type,
                        })
                    }
                    Err(error) => self.set_diagnostic_input_error(&error),
                }
            }
            DiagnosticInputKind::Whois => {
                let mut parts = input.split_ascii_whitespace();
                let Some(target) = parts.next() else {
                    return self.set_diagnostic_input_error("enter an IP address or IP:port");
                };
                let protocol = match parts.next() {
                    None => None,
                    Some("tcp") => Some(diagnostics::WhoisProtocol::Tcp),
                    Some("udp") => Some(diagnostics::WhoisProtocol::Udp),
                    Some(_) => {
                        return self.set_diagnostic_input_error("protocol must be tcp or udp");
                    }
                };
                if parts.next().is_some() {
                    return self
                        .set_diagnostic_input_error("enter an IP address and optional protocol");
                }
                match diagnostics::validate_whois_target(target) {
                    Ok(_) => {
                        self.overlays.pop();
                        self.start_local_diagnostic(DiagnosticRequest::Whois {
                            target: target.to_owned(),
                            protocol,
                        })
                    }
                    Err(error) => self.set_diagnostic_input_error(&error),
                }
            }
        }
    }

    fn set_diagnostic_input_error(&mut self, error: &str) -> Vec<Effect> {
        if let Some(Overlay::DiagnosticInput(state)) = self.overlays.last_mut() {
            state.error = Some(error.to_owned());
        }
        Vec::new()
    }

    fn diagnostic_summary(&self) -> String {
        let snapshot = self.local_resource.snapshot.as_ref();
        let selected = self.selected_local_device();
        let diagnostic = self.local_diagnostics.values().last();
        let (ping, netcheck, dns) =
            diagnostic_result_parts(diagnostic.and_then(|state| state.result.as_ref()));
        let mut names = Vec::new();
        let mut addresses = Vec::new();
        let mut paths = Vec::new();
        let mut public_endpoints = Vec::new();
        if let Some(snapshot) = snapshot {
            names.push(snapshot.self_node.display_name.clone());
            names.extend(snapshot.current_tailnet.iter().cloned());
            addresses.extend(snapshot.self_node.tailscale_ips.iter().cloned());
        }
        if let Some(device) = selected {
            names.push(device.display_name.clone());
            names.extend(device.dns_name.iter().cloned());
            addresses.extend(device.tailscale_ips.iter().cloned());
            if let Some(endpoint) = device.current_endpoint.as_deref() {
                public_endpoints.push(endpoint.to_owned());
            }
            paths.push(device.path.label().to_owned());
        }
        let health_categories =
            snapshot.map_or_else(Vec::new, |value| value.health_messages.clone());
        let input = DiagnosticReportInput {
            tale_version: env!("CARGO_PKG_VERSION").to_owned(),
            tailscale_version: self
                .local_executable
                .as_ref()
                .map_or_else(|| "not returned".to_owned(), |value| value.version.clone()),
            platform: std::env::consts::OS.to_owned(),
            local_state: self.local_state.label().to_owned(),
            health_categories,
            peer_identity: selected.and_then(|device| device.public_key.clone()),
            peer_os: selected.map(|device| device.os.label().to_owned()),
            peer_path: selected.map(|device| device.path.label().to_owned()),
            ping,
            netcheck,
            dns,
            observed_at: snapshot.map_or(self.now, |value| value.observed_at),
            stale: self.local_resource.status == LocalResourceStatus::Stale,
            names,
            addresses,
            paths,
            public_endpoints,
        };
        redact_diagnostic_report(&input).text
    }

    fn cancel_focused_task(&mut self) -> Vec<Effect> {
        let Some(id) = self.tasks.selected else {
            return Vec::new();
        };
        if !self.tasks.request_cancel(id) {
            return Vec::new();
        }
        let mut effects = vec![Effect::CancelTask { task_id: id }];
        if let Some(batch) = self.admin_batches_in_flight.get_mut(&id.0) {
            let pending = std::mem::take(&mut batch.pending_requests);
            for request in pending {
                batch.batch.record(
                    request.target_id,
                    crate::domain::admin_mutation::BatchChildOutcome::CancelledBeforeDispatch,
                );
                self.admin_resource_locks.release(request.mutation_id);
            }
            let children = batch.child_tasks.values().copied().collect::<Vec<_>>();
            for child in children {
                if self.tasks.request_cancel(child) {
                    effects.push(Effect::CancelTask { task_id: child });
                }
            }
        }
        effects
    }

    fn request_shutdown(&mut self, reason: ShutdownReason) -> Vec<Effect> {
        if matches!(self.shutdown_state, ShutdownState::Running) {
            self.shutdown_state = ShutdownState::Requested(reason);
            self.close_policy_temp_file();
            self.close_latest_policy_temp_file();
            if let Some(workflow) = self.policy_workflow.as_mut() {
                workflow.close();
            }
            self.policy_workflow = None;
            self.pending_auth_key_result = None;
            if let Some(result) = self.secret_result.as_mut() {
                result.close();
            }
            self.secret_result = None;
            self.overlays.clear();
            self.admin_environment_token = None;
            self.render_invalidated = true;
        }
        self.tasks
            .active()
            .filter(|task| task.cancellable)
            .map(|task| Effect::CancelTask { task_id: task.id })
            .chain(std::iter::once(Effect::RequestShutdown))
            .collect()
    }

    fn update_task(&mut self, event: TaskEvent) -> Vec<Effect> {
        match event {
            TaskEvent::Started { task_id } => {
                let _ = self.tasks.start(task_id);
            }
            TaskEvent::Progress {
                task_id,
                progress,
                detail,
            } => {
                let _ = self.tasks.progress(task_id, progress, &detail);
            }
            TaskEvent::Succeeded {
                task_id,
                finished_at,
                summary,
                detail,
            } => {
                if self.tasks.succeed(task_id, finished_at, &summary, &detail) {
                    self.add_notification(task_id, crate::task::TaskResultKind::Success, &summary);
                    self.tasks
                        .evict_completed(self.resolved_config.history.max_tasks);
                }
            }
            TaskEvent::Failed {
                task_id,
                finished_at,
                summary,
                detail,
            } => {
                if self.tasks.fail(task_id, finished_at, &summary, &detail) {
                    self.add_notification(task_id, crate::task::TaskResultKind::Failure, &summary);
                    self.tasks
                        .evict_completed(self.resolved_config.history.max_tasks);
                }
            }
            TaskEvent::Cancelled {
                task_id,
                finished_at,
                detail,
            } => {
                if self.tasks.cancel(task_id, finished_at, &detail) {
                    self.add_notification(
                        task_id,
                        crate::task::TaskResultKind::Cancelled,
                        "cancelled",
                    );
                    self.tasks
                        .evict_completed(self.resolved_config.history.max_tasks);
                }
            }
            TaskEvent::DiagnosticProgress {
                task_id,
                progress,
                detail,
                sample,
                netcheck,
            } => {
                return self.update_local(LocalEvent::DiagnosticProgress {
                    task_id,
                    progress,
                    detail,
                    sample,
                    netcheck,
                });
            }
            TaskEvent::DiagnosticResult { task_id, result } => {
                return self.update_local(LocalEvent::DiagnosticResult { task_id, result });
            }
        }
        Vec::new()
    }

    fn add_notification(
        &mut self,
        task_id: TaskId,
        kind: crate::task::TaskResultKind,
        message: &str,
    ) {
        self.notifications.push(Notification {
            task_id,
            message: message.to_owned(),
            kind,
            expires_at: self.now.saturating_add(5),
        });
    }

    fn update_source(&mut self, event: SourceEvent) -> Vec<Effect> {
        match event {
            SourceEvent::LoadStarted { generation, .. } => {
                if generation >= self.devices_resource.generation {
                    self.devices_resource.generation = generation;
                    self.devices_resource.health = SourceHealth::Loading;
                }
            }
            SourceEvent::LoadSucceeded {
                generation,
                devices,
                observed_at,
            } => {
                if generation < self.devices_resource.generation {
                    return Vec::new();
                }
                self.reconcile_selection(Some(&devices));
                self.devices_resource.generation = generation;
                self.devices_resource.snapshot = devices;
                self.devices_resource.observed_at = Some(observed_at);
                self.devices_resource.health = if self.now.saturating_sub(observed_at) > 60 {
                    SourceHealth::Stale
                } else {
                    SourceHealth::Healthy
                };
                self.devices_resource.error = None;
                self.reconcile_selection(None);
                self.update_composed_devices();
            }
            SourceEvent::LoadFailed { generation, detail } => {
                if generation < self.devices_resource.generation {
                    return Vec::new();
                }
                self.devices_resource.health = SourceHealth::Error;
                self.devices_resource.error = Some(detail);
            }
            SourceEvent::InputFailed(detail) => {
                self.runtime_error = Some(detail);
                return self.request_shutdown(ShutdownReason::EventSourceFailure);
            }
        }
        Vec::new()
    }

    fn update_local(&mut self, event: LocalEvent) -> Vec<Effect> {
        match event {
            LocalEvent::DiscoveryStarted { generation } => {
                if generation >= self.local_resource.generation {
                    self.local_discovery_in_flight = true;
                    self.local_resource.begin(generation, self.now);
                }
            }
            LocalEvent::DiscoverySucceeded {
                generation,
                executable,
            } => {
                if generation < self.local_resource.generation {
                    return Vec::new();
                }
                self.local_discovery_in_flight = false;
                self.local_executable = Some(executable.clone());
                self.local_capabilities = executable.capabilities;
                self.local_cli_state = LocalCliState::Available;
                let mut effects = Vec::new();
                if self.local_capabilities.accounts {
                    effects.push(Effect::StartLocalAccounts {
                        executable: executable.clone(),
                        timeout: self.resolved_config.local.command_timeout,
                    });
                }
                if self.local_capabilities.syspolicy {
                    effects.push(Effect::StartLocalPolicy {
                        executable,
                        timeout: self.resolved_config.local.command_timeout,
                    });
                }
                effects.extend(self.start_services_refresh());
                return effects;
            }
            LocalEvent::DiscoveryFailed {
                generation,
                failure,
            } => {
                if generation < self.local_resource.generation {
                    return Vec::new();
                }
                self.local_discovery_in_flight = false;
                self.local_cli_state = match failure.kind {
                    LocalFailureKind::ExecutableMissing => LocalCliState::Missing,
                    LocalFailureKind::ExecutableDenied | LocalFailureKind::PermissionDenied => {
                        LocalCliState::PermissionDenied
                    }
                    LocalFailureKind::UnsupportedClient => LocalCliState::Unsupported {
                        detail: failure.detail,
                    },
                    _ => LocalCliState::Unavailable {
                        detail: failure.detail,
                    },
                };
            }
            LocalEvent::StatusStarted {
                generation,
                attempted_at,
            } => {
                if generation >= self.local_resource.generation {
                    self.local_resource.begin(generation, attempted_at);
                }
            }
            LocalEvent::StatusSucceeded {
                generation,
                snapshot,
            } => {
                if generation < self.local_resource.generation {
                    return Vec::new();
                }
                let snapshot = *snapshot;
                if self.local_watcher_connected {
                    self.local_daemon_state = LocalDaemonState::Live;
                }
                self.local_state = snapshot.backend_state.clone();
                self.apply_local_snapshot(&snapshot);
                self.services_snapshot.command_version = Some(snapshot.client_version.clone());
                self.services_snapshot.certificate_domains.succeed(
                    self.services_snapshot.generation,
                    snapshot.observed_at,
                    snapshot.cert_domains.clone(),
                );
                self.local_resource.succeed(generation, snapshot);
                self.update_composed_devices();
                let mut effects = Vec::new();
                if self.local_executable.is_some()
                    && self.local_cli_state == LocalCliState::Available
                {
                    effects.extend(self.start_services_refresh());
                }
                return effects;
            }
            LocalEvent::StatusFailed {
                generation,
                failure,
            } => {
                if generation < self.local_resource.generation {
                    return Vec::new();
                }
                self.local_daemon_state = match failure.kind {
                    LocalFailureKind::PermissionDenied => LocalDaemonState::PermissionDenied {
                        detail: failure.detail.clone(),
                    },
                    LocalFailureKind::UnsupportedClient => LocalDaemonState::Unsupported {
                        detail: failure.detail.clone(),
                    },
                    _ => LocalDaemonState::Unavailable {
                        detail: failure.detail.clone(),
                    },
                };
                self.local_state = state_for_failure(&failure, self.local_executable.as_ref());
                self.local_resource.fail(generation, failure.clone());
                let service_failure = service_failure_from_local_failure(&failure);
                self.devices_resource.health = if self.local_resource.snapshot.is_some() {
                    SourceHealth::Stale
                } else {
                    SourceHealth::Error
                };
                self.devices_resource.error = Some(failure.detail);
                self.update_composed_devices();
                self.services_snapshot
                    .certificate_domains
                    .fail(self.services_snapshot.generation, service_failure);
            }
            LocalEvent::PreferencesStarted {
                generation,
                attempted_at,
            } => {
                if generation >= self.local_preferences_resource.generation {
                    self.local_preferences_resource
                        .begin(generation, attempted_at);
                }
            }
            LocalEvent::PreferencesSucceeded {
                generation,
                preferences,
            } => {
                if self
                    .local_preferences_resource
                    .succeed(generation, *preferences)
                {
                    if let Some(preferences) = self.local_preferences_resource.snapshot.clone() {
                        self.local_preferences = preferences;
                    }
                    apply_system_policy_editability(
                        &mut self.local_preferences,
                        &self.system_policy,
                    );
                }
            }
            LocalEvent::PreferencesFailed {
                generation,
                failure,
            } => {
                if self
                    .local_preferences_resource
                    .fail(generation, failure.clone())
                {
                    self.devices_resource.error = Some(failure.detail.clone());
                }
            }
            LocalEvent::WatcherConnected => {
                self.local_watcher_connected = true;
                self.local_daemon_state = LocalDaemonState::Connecting;
            }
            LocalEvent::WatcherDisconnected { failure } => {
                self.local_watcher_connected = false;
                self.local_daemon_state = match failure.kind {
                    LocalFailureKind::PermissionDenied => LocalDaemonState::PermissionDenied {
                        detail: failure.detail.clone(),
                    },
                    LocalFailureKind::UnsupportedClient => LocalDaemonState::Unsupported {
                        detail: failure.detail.clone(),
                    },
                    _ => LocalDaemonState::Reconnecting,
                };
                self.local_resource.mark_stale();
                self.local_preferences_resource.mark_stale();
                self.devices_resource.health = if self.local_resource.snapshot.is_some() {
                    SourceHealth::Stale
                } else {
                    SourceHealth::Loading
                };
                self.devices_resource.error = Some(failure.detail);
            }
            LocalEvent::AccountsSucceeded { accounts } => {
                self.local_accounts = accounts;
            }
            LocalEvent::AccountsFailed { failure } => {
                self.devices_resource.error = Some(failure.detail);
            }
            LocalEvent::PolicySucceeded { entries } => {
                self.system_policy = entries;
                self.system_policy_failure = None;
                apply_system_policy_editability(&mut self.local_preferences, &self.system_policy);
            }
            LocalEvent::PolicyFailed { failure } => {
                self.system_policy_failure = Some(failure);
            }
            LocalEvent::MutationFinished {
                mutation_id,
                task_id,
                mutation,
                result,
                snapshot,
                preferences,
                accounts,
                policy,
                ..
            } => {
                if self.mutation_in_flight != Some(mutation_id) {
                    return Vec::new();
                }
                let needs_login = matches!(&mutation, LocalMutation::Connect)
                    && snapshot.as_ref().is_some_and(|snapshot| {
                        matches!(&snapshot.backend_state, LocalState::NeedsLogin { .. })
                    });
                let account_changed = matches!(
                    &mutation,
                    LocalMutation::AccountSwitch { .. } | LocalMutation::AccountRemove { .. }
                );
                let account_refresh_required = account_changed
                    && !matches!(
                        &result,
                        crate::domain::mutation::MutationResult::CommandFailed { .. }
                            | crate::domain::mutation::MutationResult::CancelledBeforeDispatch { .. }
                    );
                self.mutation_lock.release(mutation_id);
                self.mutation_in_flight = None;
                let detail = result.detail().to_owned();
                let summary = result.summary().to_owned();
                let cancelled_before_dispatch = matches!(
                    &result,
                    crate::domain::mutation::MutationResult::CancelledBeforeDispatch { .. }
                );
                let _ = self.tasks.set_exit_status(task_id, result.exit_status());
                let _ = self.tasks.set_verification(
                    task_id,
                    if cancelled_before_dispatch {
                        "not dispatched"
                    } else if result.is_success() {
                        "verified"
                    } else {
                        "not verified"
                    },
                );
                if cancelled_before_dispatch {
                    let _ = self.tasks.cancel(task_id, self.now, &detail);
                    self.add_notification(
                        task_id,
                        crate::task::TaskResultKind::Cancelled,
                        &summary,
                    );
                } else if result.is_success() {
                    let _ = self.tasks.succeed(task_id, self.now, &summary, &detail);
                    self.add_notification(task_id, crate::task::TaskResultKind::Success, &summary);
                } else {
                    let _ = self.tasks.fail(task_id, self.now, &summary, &detail);
                    self.add_notification(task_id, crate::task::TaskResultKind::Failure, &summary);
                }
                self.tasks
                    .evict_completed(self.resolved_config.history.max_tasks);
                if account_refresh_required {
                    self.invalidate_local_state();
                }
                if let Some(snapshot) = snapshot {
                    self.apply_fresh_snapshot(*snapshot);
                }
                if let Some(preferences) = preferences {
                    self.local_preferences = *preferences;
                }
                if let Some(accounts) = accounts {
                    self.local_accounts = accounts;
                }
                if let Some(policy) = policy {
                    self.system_policy = policy;
                    self.system_policy_failure = None;
                }
                apply_system_policy_editability(&mut self.local_preferences, &self.system_policy);
                if needs_login {
                    return self.open_login_confirmation();
                }
                if account_refresh_required {
                    return self.start_account_rediscovery();
                }
            }
            LocalEvent::HandoffFinished { task_id, result } => {
                self.interactive_handoff_active = false;
                let mut effects = vec![Effect::ResumeTerminal];
                let refresh_after_handoff = self.tasks.get(task_id).is_some_and(|task| {
                    matches!(
                        task.action_id,
                        ActionId::LocalAccountLogin | ActionId::LocalAccountLogout
                    )
                });
                match result {
                    Ok(result) => {
                        let summary = format!(
                            "{} exited with status {}",
                            result.operation.label(),
                            result
                                .exit_status
                                .map_or_else(|| "signal".to_owned(), |value| value.to_string())
                        );
                        let _ = self.tasks.set_exit_status(task_id, result.exit_status);
                        let _ = self.tasks.set_verification(task_id, "not applicable");
                        let completed = result.exit_status == Some(0);
                        if completed {
                            let _ = self.tasks.succeed(
                                task_id,
                                self.now,
                                &summary,
                                "interactive terminal handoff completed",
                            );
                        } else {
                            let _ = self.tasks.fail(
                                task_id,
                                self.now,
                                "interactive terminal child returned a non-zero status",
                                &summary,
                            );
                        }
                        self.add_notification(
                            task_id,
                            if completed {
                                crate::task::TaskResultKind::Success
                            } else {
                                crate::task::TaskResultKind::Failure
                            },
                            if completed {
                                &summary
                            } else {
                                "interactive terminal handoff failed"
                            },
                        );
                        self.tasks
                            .evict_completed(self.resolved_config.history.max_tasks);
                        if refresh_after_handoff {
                            effects.extend(self.start_refresh(false));
                        }
                    }
                    Err(detail) => {
                        let _ = self.tasks.fail(
                            task_id,
                            self.now,
                            "interactive handoff failed",
                            &detail,
                        );
                        self.add_notification(
                            task_id,
                            crate::task::TaskResultKind::Failure,
                            "interactive handoff failed",
                        );
                        self.tasks
                            .evict_completed(self.resolved_config.history.max_tasks);
                    }
                }
                return effects;
            }
            LocalEvent::TerminalResumeFailed { detail } => {
                self.runtime_error = Some(format!("could not re-enter Tale terminal: {detail}"));
                return self.request_shutdown(ShutdownReason::RenderFailure);
            }
            LocalEvent::DiagnosticProgress {
                task_id,
                progress,
                detail,
                sample,
                netcheck,
            } => {
                if let Some(state) = self.local_diagnostics.get_mut(&task_id) {
                    if let Some(sample) = sample {
                        state.samples.push(sample);
                    }
                    if let Some(netcheck) = netcheck {
                        state.netcheck = Some(netcheck);
                    }
                }
                let _ = self.tasks.progress(task_id, progress, &detail);
            }
            LocalEvent::DiagnosticResult { task_id, result } => {
                let linked_device_id = match &result {
                    DiagnosticResult::Whois(whois) => whois.machine_id.as_ref().and_then(|id| {
                        self.local_resource.snapshot.as_ref().and_then(|snapshot| {
                            if snapshot.self_node.id.0 == *id {
                                Some(snapshot.self_node.id.clone())
                            } else {
                                snapshot
                                    .peers
                                    .iter()
                                    .find(|device| device.id.0 == *id)
                                    .map(|device| device.id.clone())
                            }
                        })
                    }),
                    _ => None,
                };
                if let Some(state) = self.local_diagnostics.get_mut(&task_id) {
                    state.linked_device_id = linked_device_id;
                    state.result = Some(result);
                }
            }
        }
        Vec::new()
    }

    fn update_services(&mut self, event: ServicesEvent) -> Vec<Effect> {
        match event {
            ServicesEvent::RefreshFinished {
                generation,
                observed_at,
                command_version,
                serve,
                funnel,
                taildrop_targets,
                taildrive,
            } => {
                if generation < self.services_snapshot.generation {
                    return Vec::new();
                }
                self.local_services_refresh_in_flight = false;
                self.services_snapshot.generation = generation;
                self.services_snapshot.observed_at = Some(observed_at);
                self.services_snapshot.command_version = Some(command_version);
                apply_service_resource(
                    &mut self.services_snapshot.serve,
                    generation,
                    observed_at,
                    serve,
                );
                apply_service_resource(
                    &mut self.services_snapshot.funnel,
                    generation,
                    observed_at,
                    funnel,
                );
                apply_service_resource(
                    &mut self.services_snapshot.taildrop_targets,
                    generation,
                    observed_at,
                    taildrop_targets,
                );
                if self.alpha_local_features {
                    apply_service_resource(
                        &mut self.services_snapshot.taildrive,
                        generation,
                        observed_at,
                        taildrive,
                    );
                } else {
                    self.services_snapshot.taildrive.status = ServiceResourceStatus::Unsupported;
                    self.services_snapshot.taildrive.failure = None;
                }
                self.update_service_capabilities();
            }
            ServicesEvent::TaskFinished {
                task_id,
                request,
                result,
                exit_status,
                stdout_truncated,
                stderr_truncated,
            } => {
                if let Some(key) = request.conflict_key() {
                    self.service_locks
                        .retain(|(held, held_task)| held != &key || held_task != &task_id);
                }
                let action_id = request.action_id();
                let mut refresh = matches!(
                    &request,
                    ServiceActionRequest::Serve { .. }
                        | ServiceActionRequest::ServeReset
                        | ServiceActionRequest::Funnel { .. }
                        | ServiceActionRequest::FunnelReset
                        | ServiceActionRequest::TaildriveShare { .. }
                        | ServiceActionRequest::TaildriveRename { .. }
                        | ServiceActionRequest::TaildriveUnshare { .. }
                );
                match result {
                    Ok(data) => {
                        let (summary, detail, verification) = match &data {
                            ServiceTaskData::Serve {
                                summary, verified, ..
                            } => {
                                refresh = true;
                                (
                                    summary.clone(),
                                    if *verified {
                                        "fresh Serve status matched the request".to_owned()
                                    } else {
                                        "fresh Serve status did not match the request".to_owned()
                                    },
                                    if *verified {
                                        "verified"
                                    } else {
                                        "succeeded unverified"
                                    },
                                )
                            }
                            ServiceTaskData::Funnel {
                                summary, verified, ..
                            } => {
                                refresh = true;
                                (
                                    summary.clone(),
                                    if *verified {
                                        "fresh PUBLIC Funnel status matched the request"
                                            .to_owned()
                                    } else {
                                        "fresh PUBLIC Funnel status did not match the request"
                                            .to_owned()
                                    },
                                    if *verified {
                                        "verified"
                                    } else {
                                        "succeeded unverified"
                                    },
                                )
                            }
                            ServiceTaskData::Taildrive {
                                summary, verified, ..
                            } => {
                                refresh = true;
                                (
                                    summary.clone(),
                                    if *verified {
                                        "fresh Taildrive share list matched the request"
                                            .to_owned()
                                    } else {
                                        "fresh Taildrive share list did not match the request"
                                            .to_owned()
                                    },
                                    if *verified {
                                        "verified"
                                    } else {
                                        "succeeded unverified"
                                    },
                                )
                            }
                            ServiceTaskData::TaildropTargets(_) => (
                                "Taildrop targets refreshed".to_owned(),
                                "target discovery completed".to_owned(),
                                "not applicable",
                            ),
                            ServiceTaskData::Transfer { summary, .. } => (
                                summary.clone(),
                                "the CLI reported a successful transfer; remote cleanup is not attempted"
                                    .to_owned(),
                                "not applicable",
                            ),
                            ServiceTaskData::Certificate(value) => {
                                self.certificate_verification = Some(value.clone());
                                (
                                    "certificate outputs verified".to_owned(),
                                    format!(
                                        "certificate and key metadata are non-empty for {}",
                                        value.domain
                                    ),
                                    "verified",
                                )
                            }
                            ServiceTaskData::Metrics(value) => {
                                self.services_snapshot.metrics.succeed(
                                    self.services_snapshot.generation,
                                    value.captured_at,
                                    value.clone(),
                                );
                                self.views.services.scroll = 0;
                                (
                                    "metrics captured".to_owned(),
                                    if value.truncated {
                                        "metrics output was truncated at the task output cap"
                                            .to_owned()
                                    } else {
                                        "bounded metrics output captured".to_owned()
                                    },
                                    "not applicable",
                                )
                            }
                            ServiceTaskData::BugReport(value) => {
                                self.services_snapshot.bug_report.succeed(
                                    self.services_snapshot.generation,
                                    value.observed_at,
                                    value.clone(),
                                );
                                (
                                    "diagnostic bug report created".to_owned(),
                                    "Tailscale returned a report identifier; Tale did not upload or share it"
                                        .to_owned(),
                                    "not applicable",
                                )
                            }
                        };
                        if let ServiceTaskData::Serve { status, .. } = &data {
                            self.services_snapshot.serve.succeed(
                                self.services_snapshot.generation,
                                self.now,
                                status.clone(),
                            );
                        }
                        if let ServiceTaskData::Funnel { status, .. } = &data {
                            self.services_snapshot.funnel.succeed(
                                self.services_snapshot.generation,
                                self.now,
                                status.clone(),
                            );
                        }
                        if let ServiceTaskData::Taildrive { shares, .. } = &data {
                            self.services_snapshot.taildrive.succeed(
                                self.services_snapshot.generation,
                                self.now,
                                shares.clone(),
                            );
                        }
                        let _ = self.tasks.set_exit_status(task_id, exit_status);
                        let _ = self.tasks.set_verification(task_id, verification);
                        let truncation = if stdout_truncated || stderr_truncated {
                            "; command output was truncated at the configured cap"
                        } else {
                            ""
                        };
                        let _ = self.tasks.succeed(
                            task_id,
                            self.now,
                            &summary,
                            &format!("{detail}{truncation}"),
                        );
                        self.add_notification(
                            task_id,
                            crate::task::TaskResultKind::Success,
                            &summary,
                        );
                    }
                    Err(failure) => {
                        match action_id {
                            ActionId::ServicesMetricsRefresh => self
                                .services_snapshot
                                .metrics
                                .fail(self.services_snapshot.generation, failure.clone()),
                            ActionId::ServicesBugReportCreate => self
                                .services_snapshot
                                .bug_report
                                .fail(self.services_snapshot.generation, failure.clone()),
                            _ => {}
                        }
                        let summary = failure.summary.clone();
                        let mut detail = failure.detail.clone();
                        if failure.stdout_truncated || failure.stderr_truncated {
                            detail.push_str("; command output was truncated at the configured cap");
                        }
                        let _ = self.tasks.set_exit_status(task_id, exit_status);
                        let _ = self.tasks.set_verification(task_id, "not verified");
                        if failure.kind == ServiceFailureKind::Cancelled {
                            let _ = self.tasks.cancel(task_id, self.now, &detail);
                            self.add_notification(
                                task_id,
                                crate::task::TaskResultKind::Cancelled,
                                &summary,
                            );
                        } else {
                            let _ = self.tasks.fail(task_id, self.now, &summary, &detail);
                            self.add_notification(
                                task_id,
                                crate::task::TaskResultKind::Failure,
                                &summary,
                            );
                        }
                    }
                }
                self.tasks
                    .evict_completed(self.resolved_config.history.max_tasks);
                if refresh {
                    return self.start_services_refresh();
                }
                let _ = action_id;
            }
        }
        Vec::new()
    }

    fn update_service_capabilities(&mut self) {
        self.services_snapshot.capabilities = ServiceCapabilities {
            serve: capability_state(self.local_capabilities.serve, "Serve"),
            funnel: capability_state(self.local_capabilities.funnel, "Funnel"),
            taildrop: capability_state(self.local_capabilities.taildrop, "Taildrop"),
            taildrive: if self.alpha_local_features {
                capability_state(self.local_capabilities.drive, "Taildrive")
            } else {
                crate::domain::service::CapabilityState::unsupported(
                    "Taildrive is alpha and disabled for this run",
                )
            },
            certificates: capability_state(self.local_capabilities.certificate, "certificates"),
            metrics: capability_state(self.local_capabilities.metrics, "metrics"),
            bug_report: capability_state(self.local_capabilities.bugreport, "bug reports"),
        };
    }

    fn apply_local_snapshot(&mut self, snapshot: &LocalSnapshot) {
        let mut devices = Vec::with_capacity(snapshot.peers.len().saturating_add(1));
        devices.push(snapshot.self_node.to_display_device());
        devices.extend(snapshot.peers.iter().map(LocalDevice::to_display_device));
        self.local_self_id = Some(snapshot.self_node.id.clone());
        self.reconcile_selection(Some(&devices));
        self.devices_resource.snapshot = devices;
        self.devices_resource.generation = self.local_resource.generation;
        self.devices_resource.observed_at = Some(snapshot.observed_at);
        self.devices_resource.health = SourceHealth::Healthy;
        self.devices_resource.error = None;
        self.reconcile_selection(None);
        self.update_composed_devices();
    }

    fn apply_fresh_snapshot(&mut self, snapshot: LocalSnapshot) {
        let generation = self.local_resource.generation.saturating_add(1);
        self.local_resource.generation = generation;
        self.local_state = snapshot.backend_state.clone();
        self.apply_local_snapshot(&snapshot);
        let _ = self.local_resource.succeed(generation, snapshot);
        self.update_composed_devices();
    }

    fn invalidate_local_state(&mut self) {
        self.local_resource.snapshot = None;
        self.local_resource.status = LocalResourceStatus::NeverLoaded;
        self.local_resource.generation = self.local_resource.generation.saturating_add(1);
        self.devices_resource.snapshot.clear();
        self.devices_resource.observed_at = None;
        self.devices_resource.health = SourceHealth::Loading;
        self.devices_resource.error = None;
        self.views.devices.selected_id = None;
        self.views.devices.scroll = 0;
        self.local_self_id = None;
        self.local_capabilities = LocalCapabilities::default();
        self.services_snapshot = LocalServicesSnapshot::new();
        self.alpha_local_features = false;
        self.local_diagnostics.clear();
        self.local_preferences = LocalPreferences::empty(self.now);
        self.system_policy.clear();
        self.system_policy_failure = None;
        self.update_composed_devices();
    }

    fn start_account_rediscovery(&mut self) -> Vec<Effect> {
        if self.local_executable.is_none() {
            return Vec::new();
        }
        let generation = self.local_resource.generation.saturating_add(1);
        self.local_resource.generation = generation;
        self.local_discovery_in_flight = true;
        vec![Effect::StartLocalDiscovery {
            generation,
            resolution: local_resolution(&self.resolved_config),
            timeout: self.resolved_config.local.command_timeout,
        }]
    }

    fn reconcile_selection(&mut self, replacement: Option<&Vec<Device>>) {
        let old_visible = self.visible_indices_for(&self.devices_resource.snapshot);
        let old_position = self.views.devices.selected_id.as_ref().and_then(|id| {
            old_visible.iter().position(|index| {
                self.devices_resource
                    .snapshot
                    .get(*index)
                    .is_some_and(|device| &device.id == id)
            })
        });
        let selected_id = self.views.devices.selected_id.clone();
        if let Some(devices) = replacement {
            if selected_id
                .as_ref()
                .is_some_and(|id| devices.iter().any(|device| &device.id == id))
            {
                return;
            }
            let target = old_position.map_or(0, |position| position);
            self.views.devices.selected_id = devices
                .get(target.min(devices.len().saturating_sub(1)))
                .map(|device| device.id.clone());
        } else {
            let visible = self.visible_indices();
            if let Some(id) = selected_id
                && visible
                    .iter()
                    .any(|index| self.devices_resource.snapshot[*index].id == id)
            {
                return;
            }
            let target = old_position.map_or(0, |position| position);
            self.views.devices.selected_id = visible
                .get(target.min(visible.len().saturating_sub(1)))
                .and_then(|index| self.devices_resource.snapshot.get(*index))
                .map(|device| device.id.clone());
        }
        self.views.devices.scroll = 0;
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        self.visible_indices_arc().as_ref().clone()
    }

    pub fn visible_indices_arc(&self) -> Arc<Vec<usize>> {
        let key = DeviceVisibleCacheKey {
            devices_generation: self.devices_resource.generation,
            local_generation: self.local_resource.generation,
            admin_generation: self.admin.devices.generation,
            now: self.now,
            source_mode: self.source_mode,
            filter: self.views.devices.applied_filter.clone(),
            sort: self.views.devices.sort,
            sort_terms: self.views.devices.sort_terms.clone(),
        };
        if let Some(cache) = self.device_visible_cache.borrow().as_ref()
            && cache.key == key
        {
            return Arc::clone(&cache.indices);
        }
        let indices = Arc::new(self.visible_indices_for(&self.devices_resource.snapshot));
        *self.device_visible_cache.borrow_mut() = Some(DeviceVisibleCache {
            key,
            indices: Arc::clone(&indices),
        });
        indices
    }

    fn visible_indices_for(&self, devices: &[Device]) -> Vec<usize> {
        let requires_admin_data = self.views.devices.applied_filter.requires_admin_data();
        let sort_terms = self.device_sort_terms();
        let mut indices: Vec<usize> = devices
            .iter()
            .enumerate()
            .filter(|(_, device)| {
                let dns_name = if self.source_mode == SourceMode::Local {
                    self.local_dns_name(&device.id)
                } else {
                    None
                };
                let common_matches = self
                    .views
                    .devices
                    .applied_filter
                    .matches_with_dns(device, dns_name, self.now);
                let admin_matches = if requires_admin_data {
                    self.admin
                        .devices
                        .snapshot
                        .as_ref()
                        .and_then(|admin_devices| {
                            admin_devices
                                .iter()
                                .find(|admin| admin.stable_id == device.id.0)
                        })
                        .is_some_and(|admin| {
                            self.views
                                .devices
                                .applied_filter
                                .matches_admin(admin, self.now)
                        })
                } else {
                    true
                };
                common_matches && admin_matches
            })
            .map(|(index, _)| index)
            .collect();
        indices.sort_by(|left, right| {
            let left_device = devices.get(*left);
            let right_device = devices.get(*right);
            match (left_device, right_device) {
                (Some(left), Some(right)) if self.source_mode == SourceMode::Local => {
                    self.compare_local_devices(left, right)
                }
                (Some(left), Some(right)) => compare_devices_by_specs(left, right, &sort_terms),
                _ => left.cmp(right),
            }
        });
        indices
    }

    fn local_dns_name(&self, id: &DeviceId) -> Option<&str> {
        self.local_resource.snapshot.as_ref().and_then(|snapshot| {
            if &snapshot.self_node.id == id {
                snapshot.self_node.dns_name.as_deref()
            } else {
                snapshot
                    .peers
                    .iter()
                    .find(|device| &device.id == id)
                    .and_then(|device| device.dns_name.as_deref())
            }
        })
    }

    fn compare_local_devices(&self, left: &Device, right: &Device) -> std::cmp::Ordering {
        let sort_terms = self.device_sort_terms();
        if sort_terms.len() == 1 && sort_terms[0] == SortSpec::default() {
            let left_self = self.local_self_id.as_ref().is_some_and(|id| id == &left.id);
            let right_self = self
                .local_self_id
                .as_ref()
                .is_some_and(|id| id == &right.id);
            return right_self
                .cmp(&left_self)
                .then_with(|| {
                    right
                        .liveness
                        .eq(&crate::domain::device::Liveness::Online)
                        .cmp(&left.liveness.eq(&crate::domain::device::Liveness::Online))
                })
                .then_with(|| {
                    self.local_active(&right.id)
                        .cmp(&self.local_active(&left.id))
                })
                .then_with(|| {
                    left.display_name
                        .to_lowercase()
                        .cmp(&right.display_name.to_lowercase())
                })
                .then_with(|| left.id.cmp(&right.id));
        }
        compare_devices_by_specs(left, right, &sort_terms)
    }

    fn device_sort_terms(&self) -> Vec<SortSpec> {
        if self.views.devices.sort_terms.is_empty()
            || self.views.devices.sort_terms.first() != Some(&self.views.devices.sort)
        {
            vec![self.views.devices.sort]
        } else {
            self.views.devices.sort_terms.clone()
        }
    }

    fn local_active(&self, id: &DeviceId) -> bool {
        self.selected_local_by_id(id)
            .is_some_and(|device| device.active)
    }

    fn selected_local_by_id(&self, id: &DeviceId) -> Option<&LocalDevice> {
        let snapshot = self.local_resource.snapshot.as_ref()?;
        if &snapshot.self_node.id == id {
            return Some(&snapshot.self_node);
        }
        snapshot.peers.iter().find(|device| &device.id == id)
    }

    fn move_selection(&mut self, offset: isize) {
        let visible = self.visible_indices_arc();
        if visible.is_empty() {
            self.views.devices.selected_id = None;
            return;
        }
        let current = self
            .views
            .devices
            .selected_id
            .as_ref()
            .and_then(|id| {
                visible
                    .iter()
                    .position(|index| self.devices_resource.snapshot[*index].id == *id)
            })
            .map_or(0, |position| position);
        let next = if offset.is_negative() {
            current.saturating_sub(offset.unsigned_abs())
        } else {
            current
                .saturating_add(offset as usize)
                .min(visible.len().saturating_sub(1))
        };
        self.views.devices.selected_id = visible
            .get(next)
            .and_then(|index| self.devices_resource.snapshot.get(*index))
            .map(|device| device.id.clone());
        self.ensure_device_selection_visible(next);
    }

    fn move_selection_to(&mut self, position: usize) {
        let visible = self.visible_indices_arc();
        if visible.is_empty() {
            self.views.devices.selected_id = None;
            self.views.devices.scroll = 0;
            return;
        }
        let index = if position == usize::MAX {
            visible.len().saturating_sub(1)
        } else {
            position.min(visible.len().saturating_sub(1))
        };
        self.views.devices.selected_id = visible
            .get(index)
            .and_then(|value| self.devices_resource.snapshot.get(*value))
            .map(|device| device.id.clone());
        self.ensure_device_selection_visible(index);
    }

    fn ensure_device_selection_visible(&mut self, position: usize) {
        let viewport = self.device_viewport_rows();
        if position < self.views.devices.scroll {
            self.views.devices.scroll = position;
        } else if position >= self.views.devices.scroll.saturating_add(viewport) {
            self.views.devices.scroll = position.saturating_add(1).saturating_sub(viewport);
        }
    }

    fn device_viewport_rows(&self) -> usize {
        usize::from(self.terminal_height.saturating_sub(8)).max(1)
    }

    fn move_admin_user_selection(&mut self, offset: isize) {
        let length = self.admin.users.snapshot.as_ref().map_or(0, Vec::len);
        self.admin_user_selected = move_bounded_index(self.admin_user_selected, length, offset);
    }

    fn move_admin_route_selection(&mut self, offset: isize) {
        let length = self.admin.route_observations().len();
        self.admin_route_selected = move_bounded_index(self.admin_route_selected, length, offset);
    }

    fn move_admin_credential_selection(&mut self, offset: isize) {
        let length = self
            .admin
            .credentials
            .snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.records.len());
        self.admin_credential_selected =
            move_bounded_index(self.admin_credential_selected, length, offset);
    }

    fn selected_admin_user(&self) -> Option<&crate::domain::user::AdminUser> {
        self.admin
            .users
            .snapshot
            .as_ref()
            .and_then(|users| users.get(self.admin_user_selected))
    }

    fn selected_admin_device(&self) -> Option<&crate::domain::device::AdminDevice> {
        let selected = self.views.devices.selected_id.as_ref()?.0.as_str();
        self.admin
            .devices
            .snapshot
            .as_ref()?
            .iter()
            .find(|device| device.stable_id == selected || device.exact_node_id() == Some(selected))
    }

    fn selected_admin_route(&self) -> Option<crate::admin::routes::AdminRouteObservation> {
        self.admin
            .route_observations()
            .into_iter()
            .nth(self.admin_route_selected)
    }

    fn move_admin_activity_selection(&mut self, offset: isize) {
        let length = self.admin.activity.snapshot.as_ref().map_or(0, |snapshot| {
            snapshot.filtered_events(&self.audit_filters).len()
        });
        self.admin_activity_selected =
            move_bounded_index(self.admin_activity_selected, length, offset);
    }

    fn selected_admin_activity(&self) -> Option<&crate::domain::activity::AuditEvent> {
        self.admin.activity.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .filtered_events(&self.audit_filters)
                .into_iter()
                .nth(self.admin_activity_selected)
        })
    }

    pub(crate) fn selected_audit_event_for_view(
        &self,
    ) -> Option<&crate::domain::activity::AuditEvent> {
        self.selected_admin_activity()
    }

    fn open_audit_reference(&mut self, target: bool) -> Vec<Effect> {
        let selected = self.selected_admin_activity().cloned();
        let (kind, id) = selected.as_ref().map_or((None, None), |event| {
            if target {
                (
                    event
                        .target
                        .as_ref()
                        .and_then(|value| value.kind.as_deref().map(str::to_ascii_lowercase)),
                    event.target.as_ref().and_then(|value| value.id.clone()),
                )
            } else {
                (
                    event
                        .actor
                        .as_ref()
                        .and_then(|value| value.kind.as_deref().map(str::to_ascii_lowercase)),
                    event.actor.as_ref().and_then(|value| value.id.clone()),
                )
            }
        });
        let Some(id) = id else {
            self.runtime_error =
                Some("the selected audit record has no exact reference ID".to_owned());
            return Vec::new();
        };
        if target {
            match kind.as_deref() {
                Some("dns") | Some("nameserver") | Some("searchpath") => {
                    self.navigate(Route::Dns);
                    return Vec::new();
                }
                Some("route") | Some("device_route") => {
                    if self
                        .admin
                        .route_observations()
                        .iter()
                        .any(|route| route.device_id == id)
                    {
                        self.navigate(Route::Routes);
                        return Vec::new();
                    }
                    self.runtime_error = Some(
                        "the exact audit route reference is not in the current snapshot".to_owned(),
                    );
                    return Vec::new();
                }
                Some("credential") | Some("key") | Some("auth_key") => {
                    if let Some(index) =
                        self.admin
                            .credentials
                            .snapshot
                            .as_ref()
                            .and_then(|snapshot| {
                                snapshot.records.iter().position(|record| record.id == id)
                            })
                    {
                        self.admin_credential_selected = index;
                        self.navigate(Route::Credentials);
                        return Vec::new();
                    }
                    self.runtime_error = Some(
                        "the exact audit credential reference is not in the current snapshot"
                            .to_owned(),
                    );
                    return Vec::new();
                }
                Some("policy") | Some("acl") | Some("access") => {
                    self.navigate(Route::Access);
                    return Vec::new();
                }
                _ => {}
            }
        }
        match kind.as_deref() {
            Some("user") => {
                if let Some(index) = self
                    .admin
                    .users
                    .snapshot
                    .as_ref()
                    .and_then(|users| users.iter().position(|user| user.id == id))
                {
                    self.admin_user_selected = index;
                    self.navigate(Route::Users);
                    return Vec::new();
                }
            }
            Some("device") | Some("node") => {
                if let Some(index) =
                    self.admin.devices.snapshot.as_ref().and_then(|devices| {
                        devices.iter().position(|device| device.stable_id == id)
                    })
                {
                    let device_id = self
                        .admin
                        .devices
                        .snapshot
                        .as_ref()
                        .and_then(|devices| devices.get(index))
                        .map(|device| device.stable_id.clone());
                    self.views.devices.selected_id = device_id.map(DeviceId::new);
                    self.navigate(Route::Devices);
                    self.focus = Focus::Inspector;
                    return self
                        .views
                        .devices
                        .selected_id
                        .as_ref()
                        .map(|device_id| device_id.0.clone())
                        .and_then(|device_id| self.start_admin_device_enrichment(Some(device_id)))
                        .into_iter()
                        .collect();
                }
            }
            Some("credential") | Some("key") | Some("auth_key") => {
                if let Some(index) = self
                    .admin
                    .credentials
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.records.iter().position(|record| record.id == id))
                {
                    self.admin_credential_selected = index;
                    self.navigate(Route::Credentials);
                    return Vec::new();
                }
            }
            _ => {}
        }
        self.runtime_error =
            Some("the exact audit reference is not in the current snapshot".to_owned());
        Vec::new()
    }

    fn open_user_devices(&mut self) -> Vec<Effect> {
        let Some(user_id) = self.selected_admin_user().map(|user| user.id.clone()) else {
            return Vec::new();
        };
        self.views.devices.filter_draft = format!("owner:{user_id}");
        self.views.devices.applied_filter = FilterExpression {
            terms: vec![FilterTerm::Field {
                field: FilterField::Owner,
                negated: false,
                values: vec![user_id],
                comparison: None,
            }],
        };
        self.views.devices.selected_id = None;
        self.navigate(Route::Devices);
        self.reconcile_selection(None);
        Vec::new()
    }

    fn open_route_device(&mut self) -> Vec<Effect> {
        let Some(route) = self.selected_admin_route() else {
            return Vec::new();
        };
        let device_id = DeviceId::new(route.device_id);
        if !self
            .devices_resource
            .snapshot
            .iter()
            .any(|device| device.id == device_id)
        {
            self.runtime_error =
                Some("route advertiser is not in the current device snapshot".to_owned());
            return Vec::new();
        }
        self.views.devices.selected_id = Some(device_id);
        self.views.devices.filter_draft.clear();
        self.views.devices.applied_filter = FilterExpression::empty();
        self.navigate(Route::Devices);
        self.focus = Focus::Inspector;
        let selected = self
            .views
            .devices
            .selected_id
            .as_ref()
            .map(|id| id.0.clone());
        self.start_admin_device_enrichment(selected)
            .into_iter()
            .collect()
    }

    fn apply_sort_choice(&mut self, choice: usize) {
        let fields = [
            SortField::Name,
            SortField::Liveness,
            SortField::Owner,
            SortField::Os,
            SortField::Path,
            SortField::LastSeen,
            SortField::Rx,
            SortField::Tx,
            SortField::DeviceId,
            SortField::Version,
        ];
        let field = fields
            .get(choice / 2)
            .copied()
            .map_or(SortField::LastSeen, |value| value);
        let direction = if choice.is_multiple_of(2) {
            SortDirection::Ascending
        } else {
            SortDirection::Descending
        };
        self.views.devices.sort = SortSpec { field, direction };
        self.views.devices.sort_terms = vec![self.views.devices.sort];
        self.reconcile_selection(None);
    }

    fn copy_field(&mut self, field: CopyField) -> Vec<Effect> {
        if field == CopyField::DiagnosticSummary {
            let value = self.diagnostic_summary();
            return self.copy_text(field.label(), value);
        }
        if field == CopyField::Metrics {
            let value = self
                .services_snapshot
                .metrics
                .value
                .as_ref()
                .map_or_else(String::new, |metrics| metrics.text.clone());
            return self.copy_text(field.label(), value);
        }
        if matches!(field, CopyField::PublicKey | CopyField::Endpoint) {
            let value = self.selected_local_device().and_then(|device| match field {
                CopyField::PublicKey => device.public_key.clone(),
                CopyField::Endpoint => device.current_endpoint.clone(),
                _ => None,
            });
            let value = match value {
                Some(value) => value,
                None => "not returned".to_owned(),
            };
            return self.copy_text(field.label(), value);
        }
        let Some(device) = self.selected_device() else {
            return Vec::new();
        };
        let value = match field {
            CopyField::DeviceId => device.id.to_string(),
            CopyField::DisplayName => device.display_name.clone(),
            CopyField::Hostname => device.hostname.clone(),
            CopyField::Owner => match device.owner.clone().or_else(|| device.owner_label.clone()) {
                Some(owner) => owner,
                None => "not returned".to_owned(),
            },
            CopyField::Addresses => device.addresses.join(", "),
            CopyField::Tags => device.tags.join(", "),
            CopyField::PublicKey | CopyField::Endpoint => "not returned".to_owned(),
            CopyField::DiagnosticSummary | CopyField::Metrics => "not returned".to_owned(),
        };
        self.copy_text(field.label(), value)
    }

    fn copy_text(&mut self, label: &str, text: String) -> Vec<Effect> {
        if self.source_mode == SourceMode::Mock {
            self.copied_value = Some(label.to_owned());
            Vec::new()
        } else {
            vec![Effect::CopyText {
                label: label.to_owned(),
                text,
            }]
        }
    }

    pub fn selected_device(&self) -> Option<&Device> {
        let id = self.views.devices.selected_id.as_ref()?;
        self.devices_resource
            .snapshot
            .iter()
            .find(|device| &device.id == id)
    }

    pub fn selected_local_device(&self) -> Option<&LocalDevice> {
        let id = self.views.devices.selected_id.as_ref()?;
        let snapshot = self.local_resource.snapshot.as_ref()?;
        if &snapshot.self_node.id == id {
            return Some(&snapshot.self_node);
        }
        snapshot.peers.iter().find(|device| &device.id == id)
    }

    pub fn current_route(&self) -> Route {
        self.view_history
            .current()
            .map_or(Route::Overview, |frame| frame.route)
    }

    pub fn overlay_title(&self) -> Option<&'static str> {
        self.overlays.last().map(|overlay| match overlay {
            Overlay::QuitConfirmation => "quit",
            Overlay::TaskInspector(_) => "task",
            Overlay::SortPicker { .. } => "sort",
            Overlay::DiagnosticInput(_) => "diagnostic input",
            Overlay::Confirmation(_) => "confirm local action",
            Overlay::OperatorForm(_) => "local operator form",
            Overlay::ServiceForm(_) => "local service form",
            Overlay::ServiceSectionPicker(_) => "service section",
            Overlay::Appearance(_) => "appearance",
            Overlay::AccountPicker(_) => "local accounts",
            Overlay::HandoffInput(_) => "terminal handoff",
            Overlay::PolicyEditor => "policy workflow",
            Overlay::SecretResult => "secret result",
            Overlay::AuditInvestigation => "audit investigation",
        })
    }

    pub fn clear_render_invalidated(&mut self) {
        self.render_invalidated = false;
    }

    pub const fn render_invalidated(&self) -> bool {
        self.render_invalidated
    }

    pub fn has_active_spinner(&self) -> bool {
        self.tasks
            .active()
            .any(|task| matches!(task.state, TaskState::Running | TaskState::Cancelling))
    }

    pub fn focused_task(&self) -> Option<&crate::task::Task> {
        self.tasks.selected.and_then(|id| self.tasks.get(id))
    }
}

fn apply_system_policy_editability(
    preferences: &mut LocalPreferences,
    policy: &[SystemPolicyEntry],
) {
    if policy_forces_any(policy, &["UseTailscaleDNSSettings"]) {
        mark_policy_managed(&mut preferences.accept_dns);
    }
    if policy_forces_any(policy, &["UseTailscaleSubnets"]) {
        mark_policy_managed(&mut preferences.accept_routes);
    }
    if policy_forces_any(policy, &["AllowIncomingConnections"]) {
        mark_policy_managed(&mut preferences.shields_up);
    }
    if policy_forces_any(policy, &["PostureChecking"]) {
        mark_policy_managed(&mut preferences.report_posture);
    }
    if policy_present_any(policy, &["CheckUpdates", "SUEnableAutomaticChecks"]) {
        mark_policy_managed(&mut preferences.update_check);
    }
    if policy_present_any(
        policy,
        &["InstallUpdates", "ApplyUpdates", "SUAutomaticallyUpdate"],
    ) {
        mark_policy_managed(&mut preferences.automatic_update);
    }
    if policy_present(policy, "Hostname") {
        mark_policy_managed(&mut preferences.hostname);
    }
    if policy_present(policy, "ExitNodeID") {
        mark_policy_managed(&mut preferences.exit_node_id);
    }
    if policy_present(policy, "ExitNodeIP") {
        mark_policy_managed(&mut preferences.exit_node_ip);
    }
    if policy_forces_any(policy, &["ExitNodeAllowLANAccess"]) {
        mark_policy_managed(&mut preferences.exit_node_allow_lan_access);
    }
    if policy_present(policy, "AdvertiseExitNode") {
        mark_policy_managed(&mut preferences.advertised_exit_node);
    }
}

fn parse_operational_fields(input: &str) -> Result<BTreeMap<String, String>, String> {
    let mut fields = BTreeMap::new();
    for part in input
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let (name, value) = part
            .split_once('=')
            .ok_or_else(|| format!("operational field must be name=value: {part}"))?;
        let name = name.trim();
        if name.is_empty()
            || fields
                .insert(name.to_owned(), value.trim().to_owned())
                .is_some()
        {
            return Err(format!("duplicate or empty operational field: {name}"));
        }
    }
    Ok(fields)
}

fn required_operational_field(
    fields: &BTreeMap<String, String>,
    name: &str,
) -> Result<String, String> {
    let value = fields
        .get(name)
        .cloned()
        .ok_or_else(|| format!("operational field {name} is required"))?;
    if value.trim().is_empty() {
        return Err(format!("operational field {name} is required"));
    }
    Ok(value)
}

fn csv_field(fields: &BTreeMap<String, String>, name: &str) -> Vec<String> {
    fields.get(name).map_or_else(Vec::new, |value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

fn parse_webhook_create(input: &str) -> Result<OperationalMutation, String> {
    let fields = parse_operational_fields(input)?;
    ensure_operational_fields(&fields, &["url", "provider", "categories", "events"])?;
    let endpoint_url = required_operational_field(&fields, "url")?;
    let provider = required_operational_field(&fields, "provider")?;
    let subscriptions = SubscriptionSet::from_wire(
        csv_field(&fields, "categories"),
        csv_field(&fields, "events"),
    )
    .map_err(|error| error.to_string())?;
    let draft = WebhookDraft {
        endpoint_url,
        destination_type: DestinationType::from_wire(&provider),
        subscriptions,
    };
    draft.validate().map_err(|error| error.to_string())?;
    Ok(OperationalMutation::Webhook(WebhookMutation::Create(draft)))
}

fn parse_log_stream_draft(
    input: &str,
    secret_input: Option<&SecretInput>,
) -> Result<OperationalMutation, String> {
    let fields = parse_operational_fields(input)?;
    ensure_operational_fields(
        &fields,
        &[
            "type",
            "destination",
            "url",
            "user",
            "period",
            "compression",
            "secret",
            "s3-bucket",
            "s3-region",
            "s3-prefix",
            "s3-auth",
            "s3-access-key",
            "s3-role",
            "gcs-bucket",
            "gcs-prefix",
            "gcs-scopes",
        ],
    )?;
    let log_type = match required_operational_field(&fields, "type")?.as_str() {
        "configuration" => LogType::Configuration,
        "network" => LogType::Network,
        value => return Err(format!("unsupported log-stream type {value}")),
    };
    let destination_type = required_operational_field(&fields, "destination")?.to_ascii_lowercase();
    if !crate::admin::log_streaming::is_supported_destination(&destination_type) {
        return Err(format!(
            "destination {destination_type} is unavailable in Tale because its documented fields are not adopted"
        ));
    }
    let url = fields.get("url").cloned().unwrap_or_else(String::new);
    let secret_action = match fields.get("secret").map(String::as_str) {
        Some("replace") => SecretAction::Replace,
        Some(value) => return Err(format!("secret must be replace, not {value}")),
        None => {
            return Err(
                "log-stream replacement requires secret=replace and a write-only secret in Ctrl+S input"
                    .to_owned(),
            );
        }
    };
    let token = if secret_action == SecretAction::Replace {
        let value = secret_input
            .filter(|secret| !secret.is_empty())
            .ok_or_else(|| {
                "secret=replace requires a write-only secret in Ctrl+S input".to_owned()
            })?;
        Some(Arc::new(crate::domain::secret_result::SecretBuffer::new(
            value.as_str(),
        )))
    } else {
        None
    };
    let upload_period_minutes = fields
        .get("period")
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "period must be an integer number of minutes".to_owned())
        })
        .transpose()?;
    let gcs_scopes = csv_field(&fields, "gcs-scopes");
    let gcs_credentials = if destination_type == "gcs" {
        token.clone()
    } else {
        None
    };
    let token = if destination_type == "gcs" {
        None
    } else {
        token
    };
    Ok(OperationalMutation::LogStreamReplace(
        LogStreamMutationDraft {
            log_type,
            destination_type,
            url,
            user: optional_operational_field(&fields, "user"),
            upload_period_minutes,
            compression_format: optional_operational_field(&fields, "compression"),
            token,
            s3_bucket: optional_operational_field(&fields, "s3-bucket"),
            s3_region: optional_operational_field(&fields, "s3-region"),
            s3_key_prefix: optional_operational_field(&fields, "s3-prefix"),
            s3_authentication_type: optional_operational_field(&fields, "s3-auth"),
            s3_access_key_id: optional_operational_field(&fields, "s3-access-key"),
            s3_role_arn: optional_operational_field(&fields, "s3-role"),
            gcs_bucket: optional_operational_field(&fields, "gcs-bucket"),
            gcs_key_prefix: optional_operational_field(&fields, "gcs-prefix"),
            gcs_scopes,
            gcs_credentials,
            secret_action,
        },
    ))
}

fn optional_operational_field(fields: &BTreeMap<String, String>, name: &str) -> Option<String> {
    fields
        .get(name)
        .filter(|value| !value.trim().is_empty())
        .cloned()
}

fn ensure_operational_fields(
    fields: &BTreeMap<String, String>,
    allowed: &[&str],
) -> Result<(), String> {
    if let Some(field) = fields
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(format!(
            "operational field {field} is not supported by this typed form"
        ));
    }
    Ok(())
}

fn parse_network_log_setting(input: &str) -> Result<OperationalMutation, String> {
    let value = input.trim().to_ascii_lowercase();
    let enabled = match value.as_str() {
        "on" => true,
        "off" => false,
        _ => return Err("network-log setting must be on or off".to_owned()),
    };
    Ok(OperationalMutation::NetworkLogSetting { enabled })
}

fn parse_flow_window_form(input: &str, now: Timestamp) -> Result<(FlowWindow, FlowFilter), String> {
    let fields = parse_operational_fields(input)?;
    let allowed = [
        "start",
        "end",
        "reporting",
        "reporting-name",
        "source",
        "source-name",
        "destination",
        "destination-name",
        "protocol",
        "source-address",
        "destination-address",
        "class",
        "source-port",
        "destination-port",
        "min-bytes",
    ];
    if fields
        .keys()
        .any(|field| !allowed.contains(&field.as_str()))
    {
        return Err("flow form contains an unsupported field".to_owned());
    }
    let now = i64::try_from(now)
        .ok()
        .and_then(|value| time::OffsetDateTime::from_unix_timestamp(value).ok())
        .ok_or_else(|| "flow clock is outside the supported timestamp range".to_owned())?;
    let start = required_operational_field(&fields, "start")?;
    let end = required_operational_field(&fields, "end")?;
    let window = FlowWindow::from_rfc3339(&start, &end, now).map_err(|error| error.to_string())?;
    let traffic_class = fields
        .get("class")
        .filter(|value| !value.trim().is_empty())
        .map(|value| match value.as_str() {
            "virtual" => Ok(crate::domain::flow::TrafficClass::Virtual),
            "subnet" => Ok(crate::domain::flow::TrafficClass::Subnet),
            "exit" => Ok(crate::domain::flow::TrafficClass::Exit),
            "physical" => Ok(crate::domain::flow::TrafficClass::Physical),
            _ => Err("flow traffic class must be virtual, subnet, exit, or physical".to_owned()),
        })
        .transpose()?;
    let filter = FlowFilter {
        reporting_node_id: optional_operational_field(&fields, "reporting"),
        reporting_node_label: optional_operational_field(&fields, "reporting-name"),
        source_node_id: optional_operational_field(&fields, "source"),
        source_node_label: optional_operational_field(&fields, "source-name"),
        destination_node_id: optional_operational_field(&fields, "destination"),
        destination_node_label: optional_operational_field(&fields, "destination-name"),
        protocol: optional_operational_field(&fields, "protocol"),
        source_address: optional_operational_field(&fields, "source-address"),
        destination_address: optional_operational_field(&fields, "destination-address"),
        traffic_class,
        source_port: parse_optional_flow_port(&fields, "source-port")?,
        destination_port: parse_optional_flow_port(&fields, "destination-port")?,
        minimum_bytes: parse_optional_flow_u64(&fields, "min-bytes")?,
    };
    filter.validate().map_err(|error| error.to_string())?;
    Ok((window, filter))
}

fn parse_optional_flow_port(
    fields: &BTreeMap<String, String>,
    name: &str,
) -> Result<Option<u16>, String> {
    fields
        .get(name)
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .parse::<u16>()
                .map_err(|_| format!("{name} must be an integer port"))
        })
        .transpose()
}

fn parse_optional_flow_u64(
    fields: &BTreeMap<String, String>,
    name: &str,
) -> Result<Option<u64>, String> {
    fields
        .get(name)
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| format!("{name} must be a non-negative integer"))
        })
        .transpose()
}

fn parse_access_question(input: &str) -> Result<AccessQuestion, String> {
    let fields = parse_operational_fields(input)?;
    ensure_operational_fields(&fields, &["source", "destination", "port", "policy"])?;
    let policy_source = match fields.get("policy").map_or("current", String::as_str) {
        "current" => PolicySource::CurrentRemote,
        "candidate" => PolicySource::ActiveCandidate,
        value => return Err(format!("policy must be current or candidate, not {value}")),
    };
    Ok(AccessQuestion {
        source_selector: required_operational_field(&fields, "source")?,
        destination_selector: required_operational_field(&fields, "destination")?,
        protocol_or_port: optional_operational_field(&fields, "port"),
        ssh_user: None,
        application_capability: None,
        policy_source,
    })
}

fn parse_saved_view_form(input: &str) -> Result<SavedView, String> {
    let fields = parse_operational_fields(input)?;
    ensure_operational_fields(
        &fields,
        &["name", "route", "wide", "columns", "filter", "sort"],
    )?;
    let name = required_operational_field(&fields, "name")?;
    let route = required_operational_field(&fields, "route")?;
    let wide_columns = fields
        .get("wide")
        .map_or(Ok(false), |value| match value.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err("wide must be true or false".to_owned()),
        })?;
    let filters = match fields.get("filter") {
        Some(value) => value
            .split("||")
            .filter(|value| !value.trim().is_empty())
            .map(parse_saved_filter)
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    let sort = match fields.get("sort") {
        Some(value) => value
            .split(',')
            .filter(|value| !value.trim().is_empty())
            .map(parse_saved_sort)
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    Ok(SavedView {
        name,
        route,
        wide_columns,
        columns: csv_field(&fields, "columns"),
        filters,
        sort,
    })
}

fn parse_saved_filter(value: &str) -> Result<FilterClause, String> {
    let mut parts = value.splitn(3, '|');
    let field = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "saved filter field is required".to_owned())?;
    let operator = match parts
        .next()
        .map(str::trim)
        .ok_or_else(|| "saved filter operator is required".to_owned())?
    {
        "equals" => FilterOperator::Equals,
        "not_equals" => FilterOperator::NotEquals,
        "contains" => FilterOperator::Contains,
        "starts_with" => FilterOperator::StartsWith,
        "greater_than" => FilterOperator::GreaterThan,
        "less_than" => FilterOperator::LessThan,
        value => return Err(format!("unsupported saved filter operator {value}")),
    };
    let raw_value = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "saved filter value is required".to_owned())?;
    let value = if let Some(value) = raw_value.strip_prefix("number:") {
        FilterValue::Number(
            value
                .parse::<i64>()
                .map_err(|_| "saved numeric filter value is invalid".to_owned())?,
        )
    } else if let Some(value) = raw_value.strip_prefix("boolean:") {
        FilterValue::Boolean(match value {
            "true" => true,
            "false" => false,
            _ => return Err("saved boolean filter value is invalid".to_owned()),
        })
    } else {
        FilterValue::Text(raw_value.to_owned())
    };
    Ok(FilterClause {
        field,
        operator,
        value,
    })
}

fn parse_saved_sort(value: &str) -> Result<SortTerm, String> {
    let (field, direction) = value
        .split_once(':')
        .ok_or_else(|| "saved sort must be field:ascending|descending".to_owned())?;
    let descending = match direction.trim() {
        "ascending" => false,
        "descending" => true,
        _ => return Err("saved sort direction is invalid".to_owned()),
    };
    if field.trim().is_empty() {
        return Err("saved sort field is required".to_owned());
    }
    Ok(SortTerm {
        field: field.trim().to_owned(),
        direction: if descending {
            SavedSortDirection::Descending
        } else {
            SavedSortDirection::Ascending
        },
    })
}

fn parse_name_form(input: &str) -> Result<String, String> {
    let fields = parse_operational_fields(input)?;
    ensure_operational_fields(&fields, &["name"])?;
    required_operational_field(&fields, "name")
}

fn parse_rename_form(input: &str) -> Result<(String, String), String> {
    let fields = parse_operational_fields(input)?;
    ensure_operational_fields(&fields, &["name", "new"])?;
    Ok((
        required_operational_field(&fields, "name")?,
        required_operational_field(&fields, "new")?,
    ))
}

fn parse_export_form(input: &str) -> Result<OperationalMutation, String> {
    let fields = parse_operational_fields(input)?;
    ensure_operational_fields(&fields, &["format", "path", "collection"])?;
    let format = required_operational_field(&fields, "format")?.to_ascii_lowercase();
    if format != "json" && format != "csv" {
        return Err("export format must be json or csv".to_owned());
    }
    let collection = match required_operational_field(&fields, "collection")?.as_str() {
        "devices" => crate::domain::export::ExportCollection::Devices,
        "users" => crate::domain::export::ExportCollection::Users,
        "routes" => crate::domain::export::ExportCollection::Routes,
        "dns" => crate::domain::export::ExportCollection::Dns,
        "credentials_metadata" => crate::domain::export::ExportCollection::CredentialMetadata,
        "audit" => crate::domain::export::ExportCollection::Audit,
        "health_findings" => crate::domain::export::ExportCollection::HealthFindings,
        "flow_logs" => crate::domain::export::ExportCollection::FlowLogs,
        value => return Err(format!("unsupported export collection {value}")),
    };
    let path = PathBuf::from(required_operational_field(&fields, "path")?);
    Ok(OperationalMutation::Export(ExportRequest {
        collection,
        format,
        path,
    }))
}

fn saved_filter_to_term(filter: &FilterClause) -> Result<FilterTerm, String> {
    let field = match filter.field.as_str() {
        "id" => FilterField::Id,
        "name" => FilterField::Name,
        "owner" => FilterField::Owner,
        "os" => FilterField::Os,
        "path" => FilterField::Path,
        "tag" => FilterField::Tag,
        "last_seen" => FilterField::LastSeen,
        "online" | "state" => FilterField::Online,
        "approval" => FilterField::Approval,
        "key_expiry" => FilterField::KeyExpiry,
        "version" => FilterField::ClientVersion,
        "sharing" => FilterField::Sharing,
        "posture" => FilterField::Posture,
        "route_role" => FilterField::RouteRole,
        value => return Err(format!("saved device filter field {value} is unavailable")),
    };
    let value = match &filter.value {
        FilterValue::Text(value) => value.clone(),
        FilterValue::Number(value) => value.to_string(),
        FilterValue::Boolean(value) => value.to_string(),
    };
    let (negated, mode) = match filter.operator {
        FilterOperator::Equals => (false, FieldMatchMode::Exact),
        FilterOperator::NotEquals => (true, FieldMatchMode::Exact),
        FilterOperator::Contains => (false, FieldMatchMode::Contains),
        FilterOperator::StartsWith => (false, FieldMatchMode::StartsWith),
        FilterOperator::GreaterThan | FilterOperator::LessThan => {
            return Err(format!(
                "saved operator {} is unavailable for device filtering",
                filter.operator.wire_value()
            ));
        }
    };
    Ok(FilterTerm::StructuredField {
        field,
        negated,
        value,
        mode,
    })
}

fn saved_filter_to_cli(filter: &FilterClause) -> Result<String, String> {
    let field = match filter.field.as_str() {
        "last_seen" => "lastseen",
        "key_expiry" => "keyexpiry",
        "route_role" => "route-role",
        "state" => "state",
        value => value,
    };
    let value = match &filter.value {
        FilterValue::Text(value) => value.clone(),
        FilterValue::Number(value) => value.to_string(),
        FilterValue::Boolean(value) => value.to_string(),
    };
    let operator = filter.operator.wire_value();
    match operator {
        "equals" => Ok(format!("{field}:{value}")),
        "not_equals" => Ok(format!("!{field}:{value}")),
        "contains" => Ok(format!("{field}:contains={value}")),
        "starts_with" => Ok(format!("{field}:starts_with={value}")),
        "greater_than" | "less_than" => Err(format!(
            "saved operator {operator} cannot be translated to the device filter grammar"
        )),
        _ => Err(format!("saved operator {operator} is not supported")),
    }
}

fn saved_sort_to_device(sort: &SortTerm) -> Result<SortSpec, String> {
    let field = match sort.field.as_str() {
        "id" => SortField::DeviceId,
        "name" => SortField::Name,
        "owner" => SortField::Owner,
        "os" => SortField::Os,
        "path" => SortField::Path,
        "last_seen" => SortField::LastSeen,
        "version" => SortField::Version,
        "state" | "online" => SortField::Liveness,
        "rx" => SortField::Rx,
        "tx" => SortField::Tx,
        value => return Err(format!("saved device sort field {value} is unavailable")),
    };
    Ok(SortSpec {
        field,
        direction: match sort.direction {
            SavedSortDirection::Ascending => SortDirection::Ascending,
            SavedSortDirection::Descending => SortDirection::Descending,
        },
    })
}

fn sorted_strings(values: &[String]) -> Vec<String> {
    let mut values = values.to_vec();
    values.sort();
    values.dedup();
    values
}

fn canonical_device_filter(expression: &FilterExpression) -> String {
    if expression.terms.is_empty() {
        "none".to_owned()
    } else {
        expression
            .terms
            .iter()
            .map(canonical_filter_term)
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

fn canonical_filter_term(term: &FilterTerm) -> String {
    match term {
        FilterTerm::Text(value) => format!("text={value}"),
        FilterTerm::Field {
            field,
            negated,
            values,
            comparison,
        } => {
            let field = match field {
                FilterField::Id => "id",
                FilterField::Name => "name",
                FilterField::Online => "online",
                FilterField::Owner => "owner",
                FilterField::Os => "os",
                FilterField::Path => "path",
                FilterField::Tag => "tag",
                FilterField::LastSeen => "last_seen",
                FilterField::Property => "property",
                FilterField::Approval => "approval",
                FilterField::KeyExpiry => "key_expiry",
                FilterField::ClientVersion => "version",
                FilterField::Sharing => "sharing",
                FilterField::Posture => "posture",
                FilterField::RouteRole => "route_role",
            };
            let value = if let Some(comparison) = comparison {
                let (operator, duration) = match comparison {
                    Comparison::Less(value) => ("less", value),
                    Comparison::LessOrEqual(value) => ("less_or_equal", value),
                    Comparison::Greater(value) => ("greater", value),
                    Comparison::GreaterOrEqual(value) => ("greater_or_equal", value),
                    Comparison::Equal(value) => ("equal", value),
                };
                format!("{operator}:{}s", duration.as_secs())
            } else {
                let mut values = values.clone();
                values.sort();
                values.dedup();
                values.join(",")
            };
            format!("{}{}:{value}", if *negated { "!" } else { "" }, field)
        }
        FilterTerm::StructuredField {
            field,
            negated,
            value,
            mode,
        } => {
            let field = match field {
                FilterField::Id => "id",
                FilterField::Name => "name",
                FilterField::Online => "online",
                FilterField::Owner => "owner",
                FilterField::Os => "os",
                FilterField::Path => "path",
                FilterField::Tag => "tag",
                FilterField::LastSeen => "last_seen",
                FilterField::Property => "property",
                FilterField::Approval => "approval",
                FilterField::KeyExpiry => "key_expiry",
                FilterField::ClientVersion => "version",
                FilterField::Sharing => "sharing",
                FilterField::Posture => "posture",
                FilterField::RouteRole => "route_role",
            };
            let mode = match mode {
                FieldMatchMode::Exact => "equals",
                FieldMatchMode::Contains => "contains",
                FieldMatchMode::StartsWith => "starts_with",
            };
            format!(
                "{}{}:{mode}={value}",
                if *negated { "!" } else { "" },
                field
            )
        }
    }
}

fn canonical_device_sort(sorts: &[SortSpec]) -> String {
    if sorts.is_empty() {
        return "stable_key".to_owned();
    }
    sorts
        .iter()
        .map(|sort| {
            let field = match sort.field {
                SortField::Name => "name",
                SortField::Liveness => "state",
                SortField::Owner => "owner",
                SortField::Os => "os",
                SortField::Path => "path",
                SortField::LastSeen => "last_seen",
                SortField::Rx => "rx",
                SortField::Tx => "tx",
                SortField::DeviceId => "id",
                SortField::Version => "version",
            };
            format!("{field}:{}", sort.direction.label())
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn contains_point(area: ratatui::layout::Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

fn audit_export_id(event: &crate::domain::activity::AuditEvent) -> String {
    let actor = event.actor.as_ref().map_or("not-returned", |value| {
        value
            .id
            .as_deref()
            .or(value.display.as_deref())
            .map_or("not-returned", |value| value)
    });
    let target = event.target.as_ref().map_or("not-returned", |value| {
        value
            .id
            .as_deref()
            .or(value.display.as_deref())
            .map_or("not-returned", |value| value)
    });
    [
        event.event_time.to_string(),
        event
            .event_group_id
            .as_deref()
            .map_or("", |value| value)
            .to_owned(),
        event
            .event_type
            .as_deref()
            .map_or("", |value| value)
            .to_owned(),
        event.action.as_deref().map_or("", |value| value).to_owned(),
        actor.to_owned(),
        target.to_owned(),
        event.origin.as_deref().map_or("", |value| value).to_owned(),
    ]
    .join("\u{0}")
}

fn format_export_timestamp(value: Timestamp) -> Option<String> {
    i64::try_from(value)
        .ok()
        .and_then(|seconds| time::OffsetDateTime::from_unix_timestamp(seconds).ok())
        .and_then(|date| {
            date.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
}

fn canonical_wire_timestamp(value: &str) -> String {
    let Ok(timestamp) =
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
    else {
        return value.to_owned();
    };
    match timestamp
        .to_offset(time::UtcOffset::UTC)
        .format(&time::format_description::well_known::Rfc3339)
    {
        Ok(formatted) => formatted,
        Err(_) => value.to_owned(),
    }
}

fn credential_status(
    record: &crate::domain::credential::CredentialMetadata,
    now: Timestamp,
) -> String {
    if record.revoked_at.is_some() {
        "revoked".to_owned()
    } else if record.invalid == Some(true) {
        "invalid".to_owned()
    } else if record.expires_at.is_some_and(|value| value <= now) {
        "expired".to_owned()
    } else {
        "observed".to_owned()
    }
}

fn mark_policy_managed<T>(preference: &mut ObservedPreference<T>) {
    if preference.value.is_some() {
        preference.editability = PreferenceEditability::PolicyManaged;
    }
}

fn policy_present(policy: &[SystemPolicyEntry], name: &str) -> bool {
    policy.iter().any(|entry| {
        entry.name.eq_ignore_ascii_case(name) && entry.error.is_none() && entry.value.is_some()
    })
}

fn policy_present_any(policy: &[SystemPolicyEntry], names: &[&str]) -> bool {
    names.iter().any(|name| policy_present(policy, name))
}

fn policy_forces_any(policy: &[SystemPolicyEntry], names: &[&str]) -> bool {
    policy.iter().any(|entry| {
        names
            .iter()
            .any(|name| entry.name.eq_ignore_ascii_case(name))
            && entry.error.is_none()
            && entry.value.as_deref().is_some_and(|value| {
                value.eq_ignore_ascii_case("always")
                    || value.eq_ignore_ascii_case("never")
                    || value.eq_ignore_ascii_case("true")
                    || value.eq_ignore_ascii_case("false")
            })
    })
}

fn policy_forces(policy: &[SystemPolicyEntry], name: &str) -> bool {
    policy.iter().any(|entry| {
        entry.name.eq_ignore_ascii_case(name)
            && entry.error.is_none()
            && entry
                .value
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    })
}

fn policy_disallows_exit_override(policy: &[SystemPolicyEntry]) -> bool {
    policy.iter().any(|entry| {
        entry.name.eq_ignore_ascii_case("ExitNode.AllowOverride")
            && entry.error.is_none()
            && entry
                .value
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("false"))
    })
}

fn route_completion_catalog() -> Vec<CompletionCandidate> {
    let mut candidates = Vec::new();
    for route in [
        Route::Overview,
        Route::Local,
        Route::Devices,
        Route::Users,
        Route::Routes,
        Route::Dns,
        Route::Access,
        Route::Credentials,
        Route::Activity,
        Route::Settings,
        Route::Services,
    ] {
        candidates.push(CompletionCandidate {
            id: format!("route:{}", route.label()),
            insertion: route.label().to_owned(),
            label: route.label().to_owned(),
            description: format!("{} view", route.label()),
            alias: false,
        });
        for alias in route_aliases(route) {
            candidates.push(CompletionCandidate {
                id: format!("alias:{}:{alias}", route.label()),
                insertion: (*alias).to_owned(),
                label: (*alias).to_owned(),
                description: format!("Alias for {}", route.label()),
                alias: true,
            });
        }
    }
    candidates
}

fn completion_matches(value: &str, fragment: &str) -> bool {
    let value = value.to_ascii_lowercase();
    let fragment = fragment.to_ascii_lowercase();
    value.starts_with(&fragment) || value.contains(&fragment)
}

fn sort_completion_candidates(candidates: &mut [CompletionCandidate], fragment: &str) {
    let fragment = fragment.to_ascii_lowercase();
    candidates.sort_by(|left, right| {
        let left_prefix = left.insertion.to_ascii_lowercase().starts_with(&fragment);
        let right_prefix = right.insertion.to_ascii_lowercase().starts_with(&fragment);
        right_prefix
            .cmp(&left_prefix)
            .then_with(|| left.alias.cmp(&right.alias))
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn edit_line(editor: &mut LineEditorState, key: KeyEvent) -> bool {
    match (key.code, key.modifiers) {
        (KeyCode::Char(character), modifiers)
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let mut encoded = [0_u8; 4];
            insert_text(editor, character.encode_utf8(&mut encoded));
            true
        }
        (KeyCode::Left, _) => {
            editor.cursor = previous_scalar_boundary(&editor.input, editor.cursor);
            true
        }
        (KeyCode::Right, _) => {
            editor.cursor = next_scalar_boundary(&editor.input, editor.cursor);
            true
        }
        (KeyCode::Home, _) => {
            editor.cursor = 0;
            true
        }
        (KeyCode::End, _) => {
            editor.cursor = editor.input.len();
            true
        }
        (KeyCode::Backspace, _) => {
            let previous = previous_scalar_boundary(&editor.input, editor.cursor);
            if previous != editor.cursor {
                editor.input.replace_range(previous..editor.cursor, "");
                editor.cursor = previous;
            }
            true
        }
        (KeyCode::Delete, _) => {
            let next = next_scalar_boundary(&editor.input, editor.cursor);
            if next != editor.cursor {
                editor.input.replace_range(editor.cursor..next, "");
            }
            true
        }
        (KeyCode::Char('w'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            let before = &editor.input[..editor.cursor];
            let trimmed = before.trim_end_matches(char::is_whitespace);
            let start = trimmed
                .char_indices()
                .rev()
                .find(|(_, character)| character.is_whitespace())
                .map_or(0, |(index, character)| index + character.len_utf8());
            editor.input.replace_range(start..editor.cursor, "");
            editor.cursor = start;
            true
        }
        (KeyCode::Char('u'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            editor.input.replace_range(..editor.cursor, "");
            editor.cursor = 0;
            true
        }
        (KeyCode::Char('k'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            editor.input.truncate(editor.cursor);
            true
        }
        _ => false,
    }
}

fn insert_text(editor: &mut LineEditorState, text: &str) {
    editor.input.insert_str(editor.cursor, text);
    editor.cursor = editor.cursor.saturating_add(text.len());
}

fn previous_scalar_boundary(value: &str, cursor: usize) -> usize {
    value[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_scalar_boundary(value: &str, cursor: usize) -> usize {
    value[cursor..]
        .char_indices()
        .nth(1)
        .map_or(value.len(), |(index, _)| cursor.saturating_add(index))
}

fn cycle_completion(
    editor: &mut LineEditorState,
    candidates: &[CompletionCandidate],
    selected: &mut Option<usize>,
    reverse: bool,
    add_route_delimiter: bool,
) {
    if candidates.is_empty() {
        return;
    }
    let start = editor.input[..editor.cursor]
        .rfind(char::is_whitespace)
        .map_or(0, |position| position.saturating_add(1));
    let fragment = &editor.input[start..editor.cursor];
    if selected.is_none() && !reverse {
        let prefix_candidates = candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .insertion
                    .to_ascii_lowercase()
                    .starts_with(&fragment.to_ascii_lowercase())
            })
            .map(|candidate| candidate.insertion.as_str())
            .collect::<Vec<_>>();
        if let Some(common) = longest_common_prefix(&prefix_candidates)
            && common.len() > fragment.len()
        {
            editor.input.replace_range(start..editor.cursor, &common);
            editor.cursor = start.saturating_add(common.len());
            if add_route_delimiter
                && prefix_candidates.len() == 1
                && start == 0
                && !common.starts_with("view:")
            {
                insert_text(editor, " ");
            }
            return;
        }
    }
    let index = match (*selected, reverse) {
        (None, false) => 0,
        (None, true) => candidates.len().saturating_sub(1),
        (Some(current), false) => current.saturating_add(1) % candidates.len(),
        (Some(0), true) => candidates.len().saturating_sub(1),
        (Some(current), true) => current.saturating_sub(1),
    };
    if let Some(candidate) = candidates.get(index) {
        editor
            .input
            .replace_range(start..editor.cursor, &candidate.insertion);
        editor.cursor = start.saturating_add(candidate.insertion.len());
        *selected = Some(index);
    }
}

fn apply_completion(editor: &mut LineEditorState, candidate: &CompletionCandidate) {
    let start = editor.input[..editor.cursor]
        .rfind(char::is_whitespace)
        .map_or(0, |position| position.saturating_add(1));
    editor
        .input
        .replace_range(start..editor.cursor, &candidate.insertion);
    editor.cursor = start.saturating_add(candidate.insertion.len());
}

fn transient_click_keys(state: &TransientMenuState) -> Vec<(char, String)> {
    match state.kind {
        TransientKind::Action => {
            let mut entries = vec![(char::MAX, "Actions".to_owned())];
            for id in &state.actions {
                let Some(sequence) = action::transient_sequence(*id) else {
                    continue;
                };
                if let Some(prefix) = state.prefix {
                    if !sequence.starts_with(prefix) || sequence.len() != 2 {
                        continue;
                    }
                } else if sequence.len() == 2 {
                    let key = sequence.chars().next().map_or(' ', |value| value);
                    if entries.iter().any(|(existing, _)| *existing == key) {
                        continue;
                    }
                    entries.push((key, format!("{key} …")));
                    continue;
                }
                let key = sequence.chars().last().map_or(' ', |value| value);
                let label = action::find_action(*id).map_or(id.as_str(), |spec| spec.label);
                entries.push((key, format!("{key} {label}")));
            }
            entries
        }
        TransientKind::Copy => state
            .fields
            .iter()
            .map(|field| {
                let key = copy_field_key(*field);
                (key, format!("{key} {}", field.label()))
            })
            .collect(),
    }
}

fn longest_common_prefix(values: &[&str]) -> Option<String> {
    let first = values.first()?;
    let mut end = first.len();
    for value in values.iter().skip(1) {
        end = first
            .char_indices()
            .take_while(|(index, character)| {
                value
                    .get(*index..)
                    .is_some_and(|tail| tail.starts_with(*character))
            })
            .map(|(index, character)| index.saturating_add(character.len_utf8()))
            .last()
            .map_or(0, |value| value)
            .min(end);
    }
    first.get(..end).map(str::to_owned)
}

const fn copy_field_key(field: CopyField) -> char {
    match field {
        CopyField::DeviceId => 'i',
        CopyField::DisplayName => 'n',
        CopyField::Hostname => 'h',
        CopyField::Owner => 'o',
        CopyField::Addresses => 'a',
        CopyField::Tags => 't',
        CopyField::PublicKey => 'p',
        CopyField::Endpoint => 'e',
        CopyField::DiagnosticSummary => 'd',
        CopyField::Metrics => 'm',
    }
}

fn parse_service_fields(input: &str) -> Result<BTreeMap<String, String>, String> {
    let mut fields = BTreeMap::new();
    let mut remaining = input;
    while !remaining.is_empty() {
        let (item, rest) = remaining
            .split_once(';')
            .map_or((remaining, ""), |(item, rest)| (item, rest));
        if item.is_empty() {
            remaining = rest;
            continue;
        }
        let (key, value) = item
            .split_once('=')
            .ok_or_else(|| format!("field {item:?} must use key=value"))?;
        let key = key.trim().to_ascii_lowercase();
        if key.is_empty() || key.chars().any(char::is_control) {
            return Err("form field name is invalid".to_owned());
        }
        let allow_note_controls = key == "note";
        if value.chars().any(|character| {
            character.is_control()
                && !(allow_note_controls && (character == '\n' || character == '\t'))
        }) {
            return Err(format!("{key} contains a control character"));
        }
        if allow_note_controls && !rest.is_empty() {
            let mut note = value.to_owned();
            note.push(';');
            note.push_str(rest);
            if note
                .chars()
                .any(|character| character.is_control() && character != '\n' && character != '\t')
            {
                return Err("note contains a disallowed control character".to_owned());
            }
            fields.insert(key, note);
            break;
        }
        fields.insert(key, value.to_owned());
        remaining = rest;
    }
    Ok(fields)
}

fn required_field<'a>(fields: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, String> {
    fields
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

fn optional_field<'a>(fields: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    fields.get(name).map(String::as_str)
}

fn parse_bool_field(fields: &BTreeMap<String, String>, name: &str) -> Result<bool, String> {
    match required_field(fields, name)?.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        _ => Err(format!("{name} must be true or false")),
    }
}

fn service_form_mapping(mapping: &ServiceMapping) -> String {
    let listener = mapping.listener.label();
    let proxy = mapping
        .proxy_protocol
        .cli_value()
        .map_or("none", |value| value);
    format!(
        "listener={listener};port={};path={};backend={};proxy={}",
        mapping.listener.port(),
        mapping.mount.as_path(),
        mapping.backend.argument(),
        proxy
    )
}

fn service_confirmation_text(request: &ServiceActionRequest) -> (String, Option<String>) {
    match request {
        ServiceActionRequest::Funnel { .. } => (
            "PUBLIC Funnel reachability will change. Tailnet policy or node capability may still deny the operation."
                .to_owned(),
            Some("PUBLIC-FUNNEL".to_owned()),
        ),
        ServiceActionRequest::FunnelReset => (
            "PUBLIC Funnel reset removes every public mapping on this node. Tailnet policy or node capability may still deny the operation."
                .to_owned(),
            Some("RESET-FUNNEL".to_owned()),
        ),
        ServiceActionRequest::ServeReset => (
            "Reset removes every Serve mapping on this node.".to_owned(),
            Some("RESET-SERVE".to_owned()),
        ),
        ServiceActionRequest::TaildropReceive(request)
            if request.conflict == TaildropConflict::Overwrite => (
                format!(
                    "Taildrop overwrite may replace files in {}.",
                    request.directory.display()
                ),
                Some("OVERWRITE".to_owned()),
            ),
        ServiceActionRequest::TaildriveUnshare { name } => (
            format!("Unsharing {name} removes access for existing Taildrive clients."),
            Some("UNSHARE".to_owned()),
        ),
        ServiceActionRequest::Certificate(request) if request.overwrites_existing => (
            format!(
                "Overwrite the certificate file {} and key file {}. Key contents will never be displayed.",
                request.certificate_path.display(),
                request.key_path.display()
            ),
            Some("OVERWRITE".to_owned()),
        ),
        ServiceActionRequest::BugReport(_) => (
            "Tailscale will receive a diagnostic report. Tale will display only the returned identifier."
                .to_owned(),
            None,
        ),
        ServiceActionRequest::TaildropReceive(request) => (
            format!(
                "Receive one Taildrop batch into {} with conflict behavior {}.",
                request.directory.display(),
                request.conflict.label()
            ),
            None,
        ),
        ServiceActionRequest::Certificate(request) => (
            format!(
                "Obtain a certificate for {} at {} and {}. Key contents will never be displayed.",
                request.domain,
                request.certificate_path.display(),
                request.key_path.display()
            ),
            None,
        ),
        ServiceActionRequest::TaildriveShare {
            input_name,
            normalized_name,
            path,
        } => (
            format!(
                "Create alpha Taildrive share {input_name:?} as {normalized_name:?} for {}.",
                path.display()
            ),
            None,
        ),
        ServiceActionRequest::TaildriveRename {
            old_name,
            input_name,
            normalized_name,
        } => (
            format!(
                "Rename alpha Taildrive share {old_name:?} to {input_name:?} ({normalized_name:?})."
            ),
            None,
        ),
        ServiceActionRequest::Serve { .. } => (
            "Apply this tailnet-only Serve mapping after reviewing the direct command preview."
                .to_owned(),
            None,
        ),
        ServiceActionRequest::TaildropSend(request) => (
            format!(
                "Send {} file(s) to Taildrop target {}.",
                request.files.len(),
                request.target.display_name
            ),
            None,
        ),
        ServiceActionRequest::Metrics => (
            "Capture bounded local metrics from the installed CLI.".to_owned(),
            None,
        ),
    }
}

fn apply_service_resource<T>(
    resource: &mut crate::domain::service::ServiceResource<T>,
    generation: u64,
    observed_at: Timestamp,
    result: Result<T, crate::domain::service::ServiceFailure>,
) {
    match result {
        Ok(value) => resource.succeed(generation, observed_at, value),
        Err(failure) => resource.fail(generation, failure),
    }
}

fn service_failure_from_local_failure(
    failure: &LocalFailure,
) -> crate::domain::service::ServiceFailure {
    let kind = match failure.kind {
        LocalFailureKind::ExecutableMissing => ServiceFailureKind::NotInstalled,
        LocalFailureKind::ExecutableDenied | LocalFailureKind::PermissionDenied => {
            ServiceFailureKind::PermissionDenied
        }
        LocalFailureKind::UnsupportedClient => ServiceFailureKind::Unsupported,
        LocalFailureKind::DaemonUnavailable | LocalFailureKind::NeedsLogin => {
            ServiceFailureKind::DaemonUnavailable
        }
        LocalFailureKind::InvalidOutput => ServiceFailureKind::DecodeFailed,
        LocalFailureKind::TimedOut => ServiceFailureKind::TimedOut,
        LocalFailureKind::Cancelled => ServiceFailureKind::Cancelled,
        LocalFailureKind::Transport => ServiceFailureKind::CommandFailed,
    };
    crate::domain::service::ServiceFailure::new(
        kind,
        failure.operation.clone(),
        failure.summary.clone(),
        failure.detail.clone(),
    )
}

fn validate_mapping_backend(mapping: &ServiceMapping) -> Result<(), String> {
    let path = match &mapping.backend {
        Backend::UnixSocket(path) | Backend::FileSystemPath(path) => Some(path),
        Backend::Port(_) | Backend::HttpUrl(_) | Backend::HttpsInsecureUrl(_) => None,
    };
    if let Some(path) = path {
        std::fs::metadata(path)
            .map_err(|_| format!("backend path {} no longer exists", path.display()))?;
    }
    Ok(())
}

fn capability_state(
    supported: bool,
    label: &'static str,
) -> crate::domain::service::CapabilityState {
    if supported {
        crate::domain::service::CapabilityState::available()
    } else {
        crate::domain::service::CapabilityState::unsupported(format!(
            "{label} is not advertised by the installed CLI"
        ))
    }
}

fn is_mutating_action(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::LocalConnect
            | ActionId::LocalDisconnect
            | ActionId::LocalPreferencesEdit
            | ActionId::LocalExitNodeSelect
            | ActionId::LocalRoutesEditAdvertisements
            | ActionId::LocalAccountSwitch
            | ActionId::LocalAccountLogin
            | ActionId::LocalAccountLogout
            | ActionId::LocalAccountRemove
            | ActionId::LocalSyspolicyReload
            | ActionId::ServicesServeCreate
            | ActionId::ServicesServeEdit
            | ActionId::ServicesServeReset
            | ActionId::ServicesFunnelCreate
            | ActionId::ServicesFunnelEdit
            | ActionId::ServicesFunnelReset
            | ActionId::ServicesTaildropSend
            | ActionId::ServicesTaildropReceive
            | ActionId::ServicesDriveShare
            | ActionId::ServicesDriveRename
            | ActionId::ServicesDriveUnshare
            | ActionId::ServicesCertificateObtain
            | ActionId::ServicesBugReportCreate
    )
}

fn is_local_verification_mutation(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::LocalConnect
            | ActionId::LocalDisconnect
            | ActionId::LocalPreferencesEdit
            | ActionId::LocalExitNodeSelect
            | ActionId::LocalRoutesEditAdvertisements
            | ActionId::LocalAccountSwitch
            | ActionId::LocalAccountRemove
            | ActionId::LocalSyspolicyReload
    )
}

fn is_admin_action(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::ProfileSelect
            | ActionId::ProfileClear
            | ActionId::AdminRefreshCurrent
            | ActionId::AdminRefreshAll
            | ActionId::ViewUsers
            | ActionId::ViewRoutes
            | ActionId::ViewDns
            | ActionId::ViewAccess
            | ActionId::ViewCredentials
            | ActionId::UsersOpenDevices
            | ActionId::RoutesOpenDevice
            | ActionId::DnsOpenLocalDiagnostics
            | ActionId::AccessCopySource
            | ActionId::ActivitySelectWindow
            | ActionId::ActivityOpenActor
            | ActionId::ActivityOpenTarget
            | ActionId::SettingsInspectCapabilities
            | ActionId::SettingsAppearance
            | ActionId::AdminDeviceRename
            | ActionId::AdminDeviceTagsReplace
            | ActionId::AdminDeviceApprove
            | ActionId::AdminDeviceRevokeApproval
            | ActionId::AdminDeviceKeyExpiryConfigure
            | ActionId::AdminDeviceKeyExpireNow
            | ActionId::AdminDeviceDelete
            | ActionId::AdminRoutesReplaceApprovals
            | ActionId::AdminDnsPreferencesEdit
            | ActionId::AdminDnsNameserversReplace
            | ActionId::AdminDnsSearchPathsReplace
            | ActionId::AdminDnsSplitCreate
            | ActionId::AdminDnsSplitEdit
            | ActionId::AdminDnsSplitRemove
            | ActionId::AdminUserApprove
            | ActionId::AdminUserRoleChange
            | ActionId::AdminUserSuspend
            | ActionId::AdminUserRestore
            | ActionId::AdminUserDelete
            | ActionId::AdminPolicyEdit
            | ActionId::AdminPolicyEditorReopen
            | ActionId::AdminPolicyCandidateDiscard
            | ActionId::AdminPolicyRemoteRefresh
            | ActionId::AdminPolicyValidate
            | ActionId::AdminPolicyPreview
            | ActionId::AdminPolicyDiff
            | ActionId::AdminPolicyApply
            | ActionId::AdminPolicyWorkflowClose
            | ActionId::AdminCredentialAuthKeyCreate
            | ActionId::AdminCredentialRevoke
            | ActionId::ProfileCredentialRemove
            | ActionId::AuditFilterTime
            | ActionId::AuditFilterActor
            | ActionId::AuditFilterAction
            | ActionId::AuditFilterTarget
            | ActionId::AuditOpenTarget
            | ActionId::AuditOpenPolicyDiff
            | ActionId::BatchReviewOutcomes
            | ActionId::BatchRetrySelected
            | ActionId::OverviewHealthOpenResource
            | ActionId::OverviewHealthRunSuggestedAction
            | ActionId::ActivityFlowsSelectWindow
            | ActionId::ActivityFlowsAggregate
            | ActionId::ActivityFlowsOpenDevice
            | ActionId::AdminWebhookCreate
            | ActionId::AdminWebhookEdit
            | ActionId::AdminWebhookTest
            | ActionId::AdminWebhookRotateSecret
            | ActionId::AdminWebhookDelete
            | ActionId::AdminLogStreamReplace
            | ActionId::AdminLogStreamDelete
            | ActionId::AdminNetworkLogsSettings
            | ActionId::AccessExplorerAsk
            | ActionId::AccessExplorerOpenRule
    )
}

fn is_admin_mutation_action(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::AdminDeviceRename
            | ActionId::AdminDeviceTagsReplace
            | ActionId::AdminDeviceApprove
            | ActionId::AdminDeviceRevokeApproval
            | ActionId::AdminDeviceKeyExpiryConfigure
            | ActionId::AdminDeviceKeyExpireNow
            | ActionId::AdminDeviceDelete
            | ActionId::AdminRoutesReplaceApprovals
            | ActionId::AdminDnsPreferencesEdit
            | ActionId::AdminDnsNameserversReplace
            | ActionId::AdminDnsSearchPathsReplace
            | ActionId::AdminDnsSplitCreate
            | ActionId::AdminDnsSplitEdit
            | ActionId::AdminDnsSplitRemove
            | ActionId::AdminUserApprove
            | ActionId::AdminUserRoleChange
            | ActionId::AdminUserSuspend
            | ActionId::AdminUserRestore
            | ActionId::AdminUserDelete
            | ActionId::AdminPolicyCandidateDiscard
            | ActionId::AdminPolicyApply
            | ActionId::AdminCredentialAuthKeyCreate
            | ActionId::AdminCredentialRevoke
            | ActionId::ProfileCredentialRemove
    )
}

fn is_admin_device_action(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::AdminDeviceRename
            | ActionId::AdminDeviceTagsReplace
            | ActionId::AdminDeviceApprove
            | ActionId::AdminDeviceRevokeApproval
            | ActionId::AdminDeviceKeyExpiryConfigure
            | ActionId::AdminDeviceKeyExpireNow
            | ActionId::AdminDeviceDelete
    )
}

fn is_admin_dns_action(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::AdminDnsPreferencesEdit
            | ActionId::AdminDnsNameserversReplace
            | ActionId::AdminDnsSearchPathsReplace
            | ActionId::AdminDnsSplitCreate
            | ActionId::AdminDnsSplitEdit
            | ActionId::AdminDnsSplitRemove
    )
}

fn is_admin_user_action(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::AdminUserApprove
            | ActionId::AdminUserRoleChange
            | ActionId::AdminUserSuspend
            | ActionId::AdminUserRestore
            | ActionId::AdminUserDelete
    )
}

fn parse_auth_key_request(
    input: &str,
) -> Result<crate::admin::key_mutations::AuthKeyCreateRequest, String> {
    let mut seen = BTreeSet::new();
    let mut description = None;
    let mut expiry_seconds = None;
    let mut reusable = None;
    let mut ephemeral = None;
    let mut preauthorized = None;
    let mut tags = None;
    for part in input.split(';') {
        let (key, value) = part
            .split_once('=')
            .ok_or_else(|| "auth-key fields use key=value separated by semicolons".to_owned())?;
        if !seen.insert(key) {
            return Err(format!("auth-key field repeated: {key}"));
        }
        match key {
            "description" => {
                description = (!value.is_empty()).then_some(value.to_owned());
            }
            "expiry" => {
                let days_text = value.strip_suffix('d').ok_or_else(|| {
                    "expiry must use a whole number of days, such as 7d".to_owned()
                })?;
                let days = days_text
                    .parse::<u64>()
                    .map_err(|_| "expiry must use a whole number of days, such as 7d".to_owned())?;
                expiry_seconds = days.checked_mul(24 * 60 * 60);
                if expiry_seconds.is_none() {
                    return Err("expiry is too large".to_owned());
                }
            }
            "reusable" => reusable = Some(parse_auth_key_bool(value, key)?),
            "ephemeral" => ephemeral = Some(parse_auth_key_bool(value, key)?),
            "preauthorized" | "preapproved" => {
                preauthorized = Some(parse_auth_key_bool(value, key)?);
            }
            "tags" => {
                tags = Some(if value.is_empty() {
                    Vec::new()
                } else {
                    value.split(',').map(str::to_owned).collect()
                });
            }
            _ => return Err(format!("unknown auth-key field: {key}")),
        }
    }
    let expiry_seconds = expiry_seconds.ok_or_else(|| "auth-key expiry is required".to_owned())?;
    let reusable = reusable.ok_or_else(|| "auth-key reusable is required".to_owned())?;
    let ephemeral = ephemeral.ok_or_else(|| "auth-key ephemeral is required".to_owned())?;
    let preauthorized =
        preauthorized.ok_or_else(|| "auth-key preauthorized is required".to_owned())?;
    let tags = tags.ok_or_else(|| "auth-key tags are required".to_owned())?;
    let request = crate::admin::key_mutations::AuthKeyCreateRequest {
        description,
        expiry_seconds,
        reusable,
        ephemeral,
        preauthorized,
        tags,
    };
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}

fn parse_policy_preview_request(input: &str) -> Result<(PolicySelectorType, String), String> {
    let mut selector_type = None;
    let mut selector = None;
    let mut seen = BTreeSet::new();
    for part in input.split(';') {
        let (key, value) = part.split_once('=').ok_or_else(|| {
            "policy preview fields use key=value separated by semicolons".to_owned()
        })?;
        if !seen.insert(key) {
            return Err(format!("policy preview field repeated: {key}"));
        }
        match key {
            "type" => {
                selector_type = Some(match value {
                    "user" => PolicySelectorType::User,
                    "ipport" => PolicySelectorType::IpPort,
                    _ => return Err("policy preview type must be user or ipport".to_owned()),
                });
            }
            "previewFor" => {
                if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                    return Err(
                        "policy preview previewFor must be non-empty, bounded, and textual"
                            .to_owned(),
                    );
                }
                selector = Some(value.to_owned());
            }
            _ => return Err(format!("unknown policy preview field: {key}")),
        }
    }
    let selector_type =
        selector_type.ok_or_else(|| "policy preview type is required".to_owned())?;
    let selector = selector.ok_or_else(|| "policy preview previewFor is required".to_owned())?;
    Ok((selector_type, selector))
}

fn parse_auth_key_bool(value: &str, field: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("auth-key {field} must be true or false")),
    }
}

fn parse_audit_filter(action_id: ActionId, input: &str) -> Result<AuditFilters, String> {
    let mut fields = BTreeMap::new();
    for part in input.split(';') {
        let (key, value) = part
            .split_once('=')
            .ok_or_else(|| "audit filters use key=value separated by semicolons".to_owned())?;
        if fields.insert(key, value).is_some() {
            return Err(format!("audit filter field repeated: {key}"));
        }
    }
    let expected = match action_id {
        ActionId::AuditFilterTime => &["start", "end"][..],
        ActionId::AuditFilterActor => &["id", "display"][..],
        ActionId::AuditFilterAction => &["action"][..],
        ActionId::AuditFilterTarget => &["type", "id", "text"][..],
        _ => return Err("this is not an audit filter form".to_owned()),
    };
    if fields.keys().any(|key| !expected.contains(key)) {
        return Err("audit filter contains an unsupported field".to_owned());
    }
    let mut filters = AuditFilters::default();
    match action_id {
        ActionId::AuditFilterTime => {
            filters.start = parse_optional_audit_time(fields.get("start").copied())?;
            filters.end = parse_optional_audit_time(fields.get("end").copied())?;
            if filters
                .start
                .zip(filters.end)
                .is_some_and(|(start, end)| start > end)
            {
                return Err("audit start must not be after audit end".to_owned());
            }
        }
        ActionId::AuditFilterActor => {
            filters.actor_id = optional_audit_text(fields.get("id").copied());
            filters.actor_display = optional_audit_text(fields.get("display").copied());
        }
        ActionId::AuditFilterAction => {
            filters.action = optional_audit_text(fields.get("action").copied());
        }
        ActionId::AuditFilterTarget => {
            filters.target_type = optional_audit_text(fields.get("type").copied());
            filters.target_id = optional_audit_text(fields.get("id").copied());
            filters.text = optional_audit_text(fields.get("text").copied());
        }
        _ => {}
    }
    Ok(filters)
}

fn optional_audit_text(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn parse_optional_audit_time(value: Option<&str>) -> Result<Option<Timestamp>, String> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let parsed = time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map_err(|_| "audit times must be RFC3339 UTC values".to_owned())?;
    u64::try_from(parsed.unix_timestamp())
        .map(Some)
        .map_err(|_| "audit time must not be before the Unix epoch".to_owned())
}

fn format_audit_timestamp(value: Timestamp) -> String {
    i64::try_from(value)
        .ok()
        .and_then(|seconds| time::OffsetDateTime::from_unix_timestamp(seconds).ok())
        .and_then(|date| {
            date.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_else(|| value.to_string())
}

fn admin_change_input(change: &AdminChange) -> String {
    match change {
        AdminChange::DeviceRename { name } => name.clone(),
        AdminChange::DeviceTags { tags } => tags.join(","),
        AdminChange::DeviceApproval { .. }
        | AdminChange::DeviceExpireNow
        | AdminChange::DeviceDelete
        | AdminChange::UserApproval
        | AdminChange::UserSuspend
        | AdminChange::UserRestore
        | AdminChange::UserDelete => String::new(),
        AdminChange::DeviceKeyExpiry { disabled } => {
            if *disabled { "on" } else { "off" }.to_owned()
        }
        AdminChange::DeviceRoutes { routes } => routes.join(","),
        AdminChange::DnsNameservers { values } => values.join(","),
        AdminChange::DnsPreferences { magic_dns } => {
            if *magic_dns { "on" } else { "off" }.to_owned()
        }
        AdminChange::DnsSearchPaths { values } => values.join(","),
        AdminChange::DnsSplitMapping {
            domain, resolvers, ..
        } => resolvers.as_ref().map_or_else(
            || domain.clone(),
            |values| format!("{domain}={}", values.join(",")),
        ),
        AdminChange::UserRole { role } => role.clone(),
    }
}

fn admin_operator_form_state(
    action_id: ActionId,
    input: String,
    error: Option<String>,
) -> OperatorFormState {
    match action_id {
        ActionId::AdminDnsNameserversReplace | ActionId::AdminDnsSearchPathsReplace => {
            OperatorFormState::ordered(
                action_id,
                input
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect(),
                None,
                error,
            )
        }
        ActionId::AdminDnsSplitCreate | ActionId::AdminDnsSplitEdit => {
            let (prefix, values) = input.split_once('=').map_or_else(
                || (None, Vec::new()),
                |(domain, values)| {
                    (
                        Some(format!("{domain}=")),
                        values
                            .split(',')
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned)
                            .collect(),
                    )
                },
            );
            OperatorFormState::ordered(action_id, values, prefix, error)
        }
        _ => OperatorFormState::new(action_id, input, error),
    }
}

fn format_ordered_input(prefix: Option<&str>, values: &[String]) -> String {
    let values = values.join(",");
    match prefix {
        Some(prefix) => format!("{prefix}{values}"),
        None => values,
    }
}

fn admin_preview_context(
    request: &AdminMutationRequest,
    fields: &AdminSnapshotFields,
) -> Vec<String> {
    let change = &request.change;
    match change {
        AdminChange::DeviceRename { .. }
        | AdminChange::DeviceTags { .. }
        | AdminChange::DeviceApproval { .. }
        | AdminChange::DeviceKeyExpiry { .. }
        | AdminChange::DeviceExpireNow
        | AdminChange::DeviceDelete => {
            let mut lines = vec![format!("stable device ID: {}", request.target_id)];
            lines.extend([
                format!(
                    "owner: {}",
                    fields
                        .values
                        .get("owner")
                        .filter(|value| !value.is_empty())
                        .map_or("not returned", String::as_str)
                ),
                format!(
                    "tags: {}",
                    fields
                        .values
                        .get("tags")
                        .filter(|value| !value.is_empty())
                        .map_or("none", String::as_str)
                ),
                format!(
                    "approval: {}",
                    fields
                        .values
                        .get("authorized")
                        .filter(|value| !value.is_empty())
                        .map_or("unknown", String::as_str)
                ),
                format!(
                    "online observation: {}",
                    fields
                        .values
                        .get("connectedToControl")
                        .filter(|value| !value.is_empty())
                        .map_or("unknown", String::as_str)
                ),
                format!(
                    "key expiry disabled: {}",
                    fields
                        .values
                        .get("keyExpiryDisabled")
                        .filter(|value| !value.is_empty())
                        .map_or("unknown", String::as_str)
                ),
                format!(
                    "key expiry timestamp: {}",
                    fields
                        .values
                        .get("expires")
                        .filter(|value| !value.is_empty())
                        .map_or("not returned", String::as_str)
                ),
                format!(
                    "advertised routes: {}",
                    fields
                        .values
                        .get("advertisedRoutes")
                        .filter(|value| !value.is_empty())
                        .map_or("none", String::as_str)
                ),
                format!(
                    "approved routes: {}",
                    fields
                        .values
                        .get("enabledRoutes")
                        .filter(|value| !value.is_empty())
                        .map_or("none", String::as_str)
                ),
            ]);
            if matches!(change, AdminChange::DeviceRename { .. }) {
                lines.push(format!(
                    "current MagicDNS/hostname: {}",
                    fields
                        .values
                        .get("hostname")
                        .filter(|value| !value.is_empty())
                        .map_or("not returned", String::as_str)
                ));
            }
            if matches!(change, AdminChange::DeviceTags { .. }) {
                lines.push(
                    "tag replacement may change device ownership identity; resulting identity is shown only after verification"
                        .to_owned(),
                );
            }
            if matches!(change, AdminChange::DeviceApproval { .. }) {
                lines.push("device approval is independent of Tailnet Lock signing".to_owned());
            }
            lines
        }
        AdminChange::DeviceRoutes { .. } => vec![
            format!("advertiser: {}", request.target_id),
            format!(
                "advertised: {}",
                fields
                    .values
                    .get("advertisedRoutes")
                    .filter(|value| !value.is_empty())
                    .map_or("none", String::as_str)
            ),
            format!(
                "currently approved: {}",
                fields
                    .values
                    .get("enabledRoutes")
                    .filter(|value| !value.is_empty())
                    .map_or("none", String::as_str)
            ),
            "admin route approval does not advertise routes on the device".to_owned(),
        ],
        AdminChange::UserApproval
        | AdminChange::UserRole { .. }
        | AdminChange::UserSuspend
        | AdminChange::UserRestore
        | AdminChange::UserDelete => vec![
            format!(
                "user ID: {}",
                fields
                    .values
                    .get("id")
                    .filter(|value| !value.is_empty())
                    .map_or(request.target_id.as_str(), String::as_str)
            ),
            format!(
                "login: {}",
                fields
                    .values
                    .get("loginName")
                    .filter(|value| !value.is_empty())
                    .map_or("not returned", String::as_str)
            ),
            format!(
                "status: {}",
                fields
                    .values
                    .get("status")
                    .filter(|value| !value.is_empty())
                    .map_or("unknown", String::as_str)
            ),
            format!(
                "role: {}",
                fields
                    .values
                    .get("role")
                    .filter(|value| !value.is_empty())
                    .map_or("unknown", String::as_str)
            ),
            format!(
                "owned device count: {}",
                fields
                    .values
                    .get("deviceCount")
                    .filter(|value| !value.is_empty())
                    .map_or("unknown", String::as_str)
            ),
            "role meanings and access enforcement remain server authoritative".to_owned(),
        ],
        AdminChange::DnsNameservers { .. }
        | AdminChange::DnsPreferences { .. }
        | AdminChange::DnsSearchPaths { .. }
        | AdminChange::DnsSplitMapping { .. } => {
            vec!["configuration changes are not claimed to have reached every client".to_owned()]
        }
    }
}

fn admin_confirmation_text(
    request: &AdminMutationRequest,
    fields: &AdminSnapshotFields,
) -> (String, Option<String>) {
    let phrase = match request.change {
        AdminChange::DeviceApproval { authorized: false }
        | AdminChange::DeviceExpireNow
        | AdminChange::DeviceDelete => fields
            .values
            .get("name")
            .filter(|value| !value.is_empty())
            .cloned()
            .or_else(|| fields.values.get("hostname").cloned()),
        AdminChange::UserSuspend | AdminChange::UserDelete => fields
            .values
            .get("loginName")
            .filter(|value| !value.is_empty())
            .cloned(),
        _ => None,
    };
    let prompt = match request.change {
        AdminChange::DeviceApproval { authorized: false } => {
            "Revoke approval for this device? This does not create or remove a Tailnet Lock signature."
        }
        AdminChange::DeviceExpireNow => {
            "Expire the current device key now? The device may disconnect and must reauthenticate."
        }
        AdminChange::DeviceDelete => {
            "Delete this device from the tailnet? Local profiles, users, and other route advertisers remain unchanged."
        }
        AdminChange::UserSuspend => {
            "Suspend this user? Review the complete owned-device context before dispatch."
        }
        AdminChange::UserDelete => {
            "Delete this user? Local Tale profiles and keyring records remain unchanged."
        }
        _ => "Apply this verified admin change exactly once?",
    };
    (prompt.to_owned(), phrase)
}

fn is_service_write_action(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::ServicesServeCreate
            | ActionId::ServicesServeEdit
            | ActionId::ServicesServeReset
            | ActionId::ServicesFunnelCreate
            | ActionId::ServicesFunnelEdit
            | ActionId::ServicesFunnelReset
            | ActionId::ServicesTaildropSend
            | ActionId::ServicesTaildropReceive
            | ActionId::ServicesDriveShare
            | ActionId::ServicesDriveRename
            | ActionId::ServicesDriveUnshare
            | ActionId::ServicesCertificateObtain
            | ActionId::ServicesBugReportCreate
    )
}

fn is_taildrive_action(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::ServicesDriveRefresh
            | ActionId::ServicesDriveShare
            | ActionId::ServicesDriveRename
            | ActionId::ServicesDriveUnshare
            | ActionId::ServicesDriveEnableAlpha
    )
}

fn is_phase_four_action(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::ServicesServeRefresh
            | ActionId::ServicesServeCreate
            | ActionId::ServicesServeEdit
            | ActionId::ServicesServeReset
            | ActionId::ServicesFunnelRefresh
            | ActionId::ServicesFunnelCreate
            | ActionId::ServicesFunnelEdit
            | ActionId::ServicesFunnelReset
            | ActionId::ServicesTaildropSend
            | ActionId::ServicesTaildropReceive
            | ActionId::ServicesDriveRefresh
            | ActionId::ServicesDriveShare
            | ActionId::ServicesDriveRename
            | ActionId::ServicesDriveUnshare
            | ActionId::ServicesCertificateObtain
            | ActionId::ServicesMetricsRefresh
            | ActionId::ServicesBugReportCreate
            | ActionId::ServicesDriveEnableAlpha
    )
}

fn is_phase_three_action(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::LocalConnect
            | ActionId::LocalDisconnect
            | ActionId::LocalPreferencesEdit
            | ActionId::LocalExitNodeSelect
            | ActionId::LocalRoutesEditAdvertisements
            | ActionId::LocalAccountSwitch
            | ActionId::LocalAccountLogin
            | ActionId::LocalAccountLogout
            | ActionId::LocalAccountRemove
            | ActionId::LocalSshOpen
            | ActionId::LocalNcOpen
            | ActionId::LocalSyspolicyReload
    )
}

fn parse_preference_request(input: &str) -> Result<PreferenceRequest, String> {
    let mut request = PreferenceRequest::default();
    if input.trim().is_empty() {
        return Err("enter at least one field=value pair".to_owned());
    }
    for pair in input.split(',') {
        let (name, value) = pair
            .trim()
            .split_once('=')
            .ok_or_else(|| "preferences use comma-separated field=value pairs".to_owned())?;
        match name.trim() {
            "accept-dns" => request.accept_dns = Some(parse_bool(value)?),
            "accept-routes" => request.accept_routes = Some(parse_bool(value)?),
            "shields-up" => request.shields_up = Some(parse_bool(value)?),
            "ssh" => request.ssh = Some(parse_bool(value)?),
            "auto-update" => request.automatic_update = Some(parse_bool(value)?),
            "update-check" => request.update_check = Some(parse_bool(value)?),
            "report-posture" => request.report_posture = Some(parse_bool(value)?),
            "hostname" => request.hostname = Some(value.to_owned()),
            "nickname" => request.nickname = Some(value.to_owned()),
            "webclient" => request.web_client = Some(parse_bool(value)?),
            _ => return Err(format!("unsupported preference field: {name}")),
        }
    }
    if request.hostname.as_deref().is_some_and(str::is_empty)
        || request.nickname.as_deref().is_some_and(str::is_empty)
    {
        return Err("hostname and nickname must be non-empty".to_owned());
    }
    Ok(request)
}

fn parse_advertisement_request(input: &str) -> Result<AdvertisementRequest, String> {
    if input.trim().is_empty() {
        return Err("enter semicolon-separated advertisement fields".to_owned());
    }
    let mut request = AdvertisementRequest::default();
    for pair in input.split(';') {
        let (name, value) = pair
            .trim()
            .split_once('=')
            .ok_or_else(|| "advertisements use field=value;field=value".to_owned())?;
        let value = value.trim();
        match name.trim() {
            "routes" => {
                request.routes = Some(if value.trim().is_empty() || value == "empty" {
                    Vec::new()
                } else {
                    parse_route_set(value).map_err(|error| error.to_string())?
                });
            }
            "exit" => request.advertise_exit_node = Some(parse_bool(value)?),
            "connector" => request.advertise_connector = Some(parse_bool(value)?),
            "relay-port" => {
                request.relay_server_port = Some(if value == "empty" || value.is_empty() {
                    None
                } else {
                    Some(
                        value
                            .parse::<u16>()
                            .map_err(|_| "relay-port must be empty, 0, or 1-65535".to_owned())?,
                    )
                });
            }
            "relay-endpoints" => {
                request.relay_server_static_endpoints =
                    Some(if value.trim().is_empty() || value == "empty" {
                        Vec::new()
                    } else {
                        parse_static_endpoints(value).map_err(|error| error.to_string())?
                    });
            }
            "accept-risk" if value == "mac-app-connector" => {
                request.accept_mac_app_connector_risk = true;
            }
            _ => return Err(format!("unsupported advertisement field: {name}")),
        }
    }
    if request.is_empty() {
        return Err("no advertisement fields were changed".to_owned());
    }
    if request.advertise_connector == Some(true) && !request.accept_mac_app_connector_risk {
        return Err("enabling the app connector requires accept-risk=mac-app-connector".to_owned());
    }
    if request.accept_mac_app_connector_risk && request.advertise_connector != Some(true) {
        return Err("mac-app-connector risk acceptance requires connector=true".to_owned());
    }
    Ok(request)
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err("boolean values must be true or false".to_owned()),
    }
}

fn boolean_text(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "on",
        Some(false) => "off",
        None => "unknown",
    }
}

fn preference_field_editable(preferences: &LocalPreferences, field: PreferenceField) -> bool {
    match field {
        PreferenceField::AcceptDns => preferences.accept_dns.can_edit(),
        PreferenceField::AcceptRoutes => preferences.accept_routes.can_edit(),
        PreferenceField::ShieldsUp => preferences.shields_up.can_edit(),
        PreferenceField::Ssh => preferences.ssh.can_edit(),
        PreferenceField::AutomaticUpdate => preferences.automatic_update.can_edit(),
        PreferenceField::UpdateCheck => preferences.update_check.can_edit(),
        PreferenceField::ReportPosture => preferences.report_posture.can_edit(),
        PreferenceField::Hostname => preferences.hostname.can_edit(),
        PreferenceField::Nickname => preferences.nickname.can_edit(),
        PreferenceField::WebClient => preferences.web_client.can_edit(),
    }
}

fn text_value(value: Option<&str>) -> &str {
    match value {
        Some(value) => value,
        None => "unknown",
    }
}

fn mutation_target_label(mutation: &LocalMutation) -> String {
    match mutation {
        LocalMutation::Connect => "local node".to_owned(),
        LocalMutation::Disconnect { .. } => "local node".to_owned(),
        LocalMutation::Preferences(request) => request
            .changed_fields()
            .iter()
            .map(|field| field.label())
            .collect::<Vec<_>>()
            .join(", "),
        LocalMutation::ExitNode(_) => "exit node selection".to_owned(),
        LocalMutation::Advertisements(_) => "local advertisements".to_owned(),
        LocalMutation::AccountSwitch { .. } => "account profile switch".to_owned(),
        LocalMutation::AccountRemove { .. } => "local account profile removal".to_owned(),
        LocalMutation::SyspolicyReload => "system policy".to_owned(),
    }
}

fn mutation_metadata(
    path: &std::path::Path,
    mutation: &LocalMutation,
    timeout: Duration,
) -> (Vec<String>, Vec<String>) {
    let fields = match mutation {
        LocalMutation::Preferences(request) => request
            .changed_fields()
            .iter()
            .map(|field| field.label().to_owned())
            .collect(),
        LocalMutation::ExitNode(_) => vec!["exit node".to_owned(), "LAN access".to_owned()],
        LocalMutation::Advertisements(request) => {
            let mut values = Vec::new();
            if request.routes.is_some() {
                values.push("advertised routes".to_owned());
            }
            if request.advertise_exit_node.is_some() {
                values.push("advertised exit node".to_owned());
            }
            if request.advertise_connector.is_some() {
                values.push("app connector".to_owned());
            }
            if request.relay_server_port.is_some() {
                values.push("relay server port".to_owned());
            }
            if request.relay_server_static_endpoints.is_some() {
                values.push("relay static endpoints".to_owned());
            }
            values
        }
        LocalMutation::Connect | LocalMutation::Disconnect { .. } => vec!["state".to_owned()],
        LocalMutation::AccountSwitch { .. } | LocalMutation::AccountRemove { .. } => {
            vec!["account_id".to_owned()]
        }
        LocalMutation::SyspolicyReload => vec!["system policy".to_owned()],
    };
    let command = match mutation {
        LocalMutation::Connect => Some(crate::local::client::up_command(path, timeout)),
        LocalMutation::Disconnect { accept_lose_ssh } => Some(crate::local::client::down_command(
            path,
            timeout,
            *accept_lose_ssh,
        )),
        LocalMutation::Preferences(request) => {
            crate::local::client::set_command(path, timeout, request).ok()
        }
        LocalMutation::ExitNode(request) => Some(crate::local::client::exit_node_command(
            path, timeout, request,
        )),
        LocalMutation::Advertisements(request) => {
            crate::local::client::advertisement_command(path, timeout, request).ok()
        }
        LocalMutation::AccountSwitch { account_id } => {
            crate::local::accounts::switch_command(path, timeout, account_id).ok()
        }
        LocalMutation::AccountRemove { account_id } => {
            crate::local::accounts::remove_command(path, timeout, account_id).ok()
        }
        LocalMutation::SyspolicyReload => Some(crate::local::policy::reload_command(path, timeout)),
    };
    let argv = command.map_or_else(Vec::new, |command| redacted_argv(&command.args));
    (fields, argv)
}

fn redacted_argv(args: &[std::ffi::OsString]) -> Vec<String> {
    let mut redactor = crate::domain::redaction::Redactor::new();
    args.iter()
        .enumerate()
        .map(|(index, arg)| {
            let value = arg.to_string_lossy();
            if index > 0
                && !value.starts_with('-')
                && !matches!(value.as_ref(), "set" | "switch" | "remove")
            {
                return redactor.identity(&value);
            }
            if let Some((prefix, raw_value)) = value.split_once('=') {
                let redacted_value = raw_value
                    .split(',')
                    .map(|part| {
                        if part.is_empty()
                            || matches!(part, "true" | "false" | "auto:any")
                            || part.parse::<u16>().is_ok()
                        {
                            part.to_owned()
                        } else if matches!(prefix, "--hostname" | "--nickname" | "--exit-node") {
                            redactor.identity(part)
                        } else {
                            redactor.text(part)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                return format!("{prefix}={redacted_value}");
            }
            redactor.text(&value)
        })
        .collect()
}

fn route_aliases(route: Route) -> &'static [&'static str] {
    match route {
        Route::Overview => &["ov", "home"],
        Route::Local => &["self"],
        Route::Devices => &["device", "dev", "nodes"],
        Route::Users => &["user"],
        Route::Routes => &["route", "rt"],
        Route::Dns => &[],
        Route::Access => &["policy", "acl", "grants"],
        Route::Credentials => &["credential", "keys", "auth"],
        Route::Activity => &["tasks", "logs", "events"],
        Route::Settings => &["config"],
        Route::Services => &["service", "serve", "funnel"],
    }
}

fn apply_admin_result<T>(
    resource: &mut AdminResource<T>,
    generation: u64,
    observed_at: Timestamp,
    result: Result<T, AdminError>,
) {
    match result {
        Ok(snapshot) => resource.succeed(generation, snapshot, observed_at),
        Err(error) => {
            let failure_state = admin_state_for_error(&error);
            let state = if resource.snapshot.is_some()
                && matches!(
                    error,
                    AdminError::Transport { .. }
                        | AdminError::TimedOut { .. }
                        | AdminError::Cancelled { .. }
                        | AdminError::UnexpectedStatus { .. }
                        | AdminError::DecodeFailed { .. }
                        | AdminError::BodyTooLarge { .. }
                        | AdminError::NotFound { .. }
                        | AdminError::ValidationFailed { .. }
                        | AdminError::Conflict { .. }
                        | AdminError::RateLimited { .. }
                        | AdminError::ServerFailure { .. }
                ) {
                AdminResourceState::Stale
            } else {
                failure_state
            };
            resource.generation = generation;
            resource.state = state;
            resource.last_failure = Some(failure_state);
            resource.error = Some(error.to_string());
        }
    }
}

fn mark_admin_unauthenticated<T>(resource: &mut AdminResource<T>, generation: u64, detail: String) {
    resource.generation = generation;
    resource.state = AdminResourceState::Unauthenticated;
    resource.last_failure = Some(AdminResourceState::Unauthenticated);
    resource.error = Some(detail);
}

fn mark_admin_failed<T>(resource: &mut AdminResource<T>, generation: u64, detail: String) {
    resource.generation = generation;
    resource.last_failure = Some(AdminResourceState::Failed);
    resource.state = if resource.snapshot.is_some() {
        AdminResourceState::Stale
    } else {
        AdminResourceState::Failed
    };
    resource.error = Some(detail);
}

fn admin_state_for_error(error: &AdminError) -> AdminResourceState {
    match error {
        AdminError::Unauthenticated => AdminResourceState::Unauthenticated,
        AdminError::Forbidden { .. } => AdminResourceState::Forbidden,
        AdminError::PlanRestricted { .. } => AdminResourceState::PlanRestricted,
        AdminError::Unsupported { .. } => AdminResourceState::Unsupported,
        AdminError::Transport { .. }
        | AdminError::TimedOut { .. }
        | AdminError::Cancelled { .. }
        | AdminError::UnexpectedStatus { .. }
        | AdminError::DecodeFailed { .. }
        | AdminError::BodyTooLarge { .. }
        | AdminError::NotFound { .. }
        | AdminError::ValidationFailed { .. }
        | AdminError::Conflict { .. }
        | AdminError::RateLimited { .. }
        | AdminError::ServerFailure { .. } => AdminResourceState::Failed,
    }
}

fn capability_for_state(state: AdminResourceState) -> admin::CapabilityState {
    match state {
        AdminResourceState::Idle | AdminResourceState::Loading => {
            admin::CapabilityState::Configured
        }
        AdminResourceState::Ready => admin::CapabilityState::Available,
        AdminResourceState::Stale | AdminResourceState::Failed => admin::CapabilityState::Failed,
        AdminResourceState::Forbidden => admin::CapabilityState::Forbidden,
        AdminResourceState::PlanRestricted => admin::CapabilityState::PlanRestricted,
        AdminResourceState::Unsupported => admin::CapabilityState::Unsupported,
        AdminResourceState::Unauthenticated => admin::CapabilityState::Unauthenticated,
    }
}

fn online_rank(value: Option<bool>) -> u8 {
    match value {
        Some(true) => 0,
        Some(false) => 1,
        None => 2,
    }
}

fn move_bounded_index(current: usize, length: usize, offset: isize) -> usize {
    if length == 0 {
        return 0;
    }
    let current = current.min(length.saturating_sub(1));
    if offset.is_negative() {
        current.saturating_sub(offset.unsigned_abs())
    } else {
        current
            .saturating_add(offset as usize)
            .min(length.saturating_sub(1))
    }
}

fn probe_rank(value: Option<u16>) -> (u8, u16) {
    value.map_or((1, u16::MAX), |value| (0, value))
}

fn local_resolution(config: &ResolvedConfig) -> ExecutableResolution {
    let configured = PathBuf::from(&config.local.tailscale_path);
    let (cli_path, environment_path, config_path) = match config.local.tailscale_path_source {
        crate::config::ValueSource::Cli => (Some(configured), None, None),
        crate::config::ValueSource::Environment => (None, Some(configured.into_os_string()), None),
        crate::config::ValueSource::File => (None, None, Some(configured)),
        crate::config::ValueSource::Default => (None, None, None),
    };
    ExecutableResolution {
        cli_path,
        environment_path,
        config_path,
        socket_path: Some(config.local.socket_path.clone()),
        path: std::env::var_os("PATH"),
        platform: if cfg!(windows) {
            HostPlatform::Windows
        } else {
            HostPlatform::Unix
        },
    }
}

fn local_handoff_command(command: HandoffCommand, socket_path: Option<&Path>) -> HandoffCommand {
    match socket_path {
        Some(path) => command.with_socket_path(path),
        None => command,
    }
}

fn diagnostic_action(request: &DiagnosticRequest) -> ActionId {
    match request {
        DiagnosticRequest::Ping { .. } => ActionId::LocalProbeConnection,
        DiagnosticRequest::Netcheck { live: false } => ActionId::LocalNetcheck,
        DiagnosticRequest::Netcheck { live: true } => ActionId::LocalNetcheckLive,
        DiagnosticRequest::DnsStatus => ActionId::LocalDnsStatus,
        DiagnosticRequest::DnsQuery { .. } => ActionId::LocalDnsQuery,
        DiagnosticRequest::Whois { .. } => ActionId::LocalWhois,
    }
}

fn state_for_failure(failure: &LocalFailure, executable: Option<&LocalExecutable>) -> LocalState {
    match failure.kind {
        LocalFailureKind::ExecutableMissing => LocalState::ExecutableMissing,
        LocalFailureKind::ExecutableDenied => LocalState::ExecutableDenied,
        LocalFailureKind::UnsupportedClient => LocalState::UnsupportedClient {
            version: executable.map_or_else(|| "unknown".to_owned(), |value| value.version.clone()),
            reason: failure.detail.clone(),
        },
        LocalFailureKind::PermissionDenied => LocalState::PermissionDenied {
            operation: failure.operation.clone(),
            detail: failure.detail.clone(),
        },
        LocalFailureKind::NeedsLogin => LocalState::NeedsLogin { auth_url: None },
        LocalFailureKind::DaemonUnavailable
        | LocalFailureKind::InvalidOutput
        | LocalFailureKind::TimedOut
        | LocalFailureKind::Cancelled
        | LocalFailureKind::Transport => LocalState::DaemonUnavailable {
            detail: failure.detail.clone(),
        },
    }
}

fn diagnostic_result_parts(
    result: Option<&DiagnosticResult>,
) -> (
    Option<crate::domain::diagnostic::PingSummary>,
    Option<crate::domain::diagnostic::NetcheckObservation>,
    Option<crate::domain::diagnostic::DnsQueryResult>,
) {
    match result {
        Some(DiagnosticResult::Ping(value)) => (Some(value.clone()), None, None),
        Some(DiagnosticResult::Netcheck(value)) => (None, Some(value.clone()), None),
        Some(DiagnosticResult::DnsQuery(value)) => (None, None, Some(value.clone())),
        Some(DiagnosticResult::DnsStatus(_)) | Some(DiagnosticResult::Whois(_)) | None => {
            (None, None, None)
        }
    }
}

fn instant_after(base: Instant, delay: Duration) -> Instant {
    match base.checked_add(delay) {
        Some(value) => value,
        None => base,
    }
}
