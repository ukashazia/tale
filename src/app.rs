mod action_reducer;
mod admin_reducer;
mod collection_reducer;
mod credential_reducer;
mod diagnostics_reducer;
mod interaction_reducer;
mod local_reducer;
mod operational_reducer;
mod policy_reducer;
mod service_reducer;
mod source_reducer;
mod task_reducer;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as MatcherConfig, Matcher, Utf32Str};
use sha2::{Digest, Sha256};

use crate::action::{self, ActionContext, ActionId, Capability};
use crate::admin::client::AdminError;
use crate::admin::mutation::{
    AdminBatchConfirmation, AdminMutationRequest, AdminSnapshotFields, batch_target,
};
use crate::admin::{
    self, AdminRefreshResource, AdminResource, AdminResourceResult, AdminResourceState,
    AdminSnapshot,
};
use crate::config::{
    ResolvedConfig, SettingDisplay, SettingSortField, SettingSortSpec, ValueSource,
};
use crate::domain::access_explorer::{AccessQuestion, AccessResult, PolicySource};
use crate::domain::account::{LocalAccount, LocalSection};
use crate::domain::activity::AuditFilters;
use crate::domain::admin_mutation::{
    AdminChange, AdminMutationState, AdminResourceLocks, AuditCorrelation, BatchMutation,
    BatchTarget, transition,
};
use crate::domain::certificate::{BugReportRequest, CertificateRequest};
use crate::domain::device::{
    AdminDevice, ComposedDevice, Device, DeviceId, LocalDevice, SortDirection, SortField, SortSpec,
    compare_devices_by_specs, compose_exact_id, same_tailnet,
};
use crate::domain::diagnostic::{DiagnosticResult, DiagnosticState};
use crate::domain::filter::{
    self, Comparison, FieldMatchMode, FilterExpression, FilterField, FilterFieldSpec, FilterSchema,
    FilterTerm, FilterValueKind,
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
use crate::domain::profile::{
    CredentialPresence, ProbeState, ProfileSortField, ProfileSortSpec, ProfileStatus,
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
    Backend, CertificateVerification, Exposure, FunnelStatus, Listener, LocalServicesSnapshot,
    PathMount, Port, ProxyProtocol, ServeStatus, ServiceActionRequest, ServiceCapabilities,
    ServiceConflictKey, ServiceFailureKind, ServiceMapping, ServiceResourceStatus, ServiceSection,
    ServiceSortField, ServiceTaskData,
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
use crate::mock::{self, MOCK_NOW, MockLoadScenario, MockTaskBehavior};
use crate::paths;
use crate::task::{Notification, TaskId, TaskState, TaskStore};
use crate::ui::theme::Theme;

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
    /// What this client resolved for itself, and what decided each value.
    Config,
    Local,
    /// The local client and every configured admin profile, and which of them
    /// the rest of the app is reading from.
    Profiles,
    Devices,
    Users,
    Routes,
    Dns,
    Access,
    Credentials,
    Tasks,
    Audit,
    Services,
    Diagnostics,
}

impl Route {
    pub const DEFAULT: Self = Self::Devices;

    pub const fn label(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Config => "config",
            Self::Local => "local",
            Self::Profiles => "profiles",
            Self::Devices => "devices",
            Self::Users => "users",
            Self::Routes => "routes",
            Self::Dns => "dns",
            Self::Access => "access",
            Self::Credentials => "credentials",
            Self::Tasks => "tasks",
            Self::Audit => "audit",
            Self::Services => "services",
            Self::Diagnostics => "diagnostics",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "overview" => Some(Self::Overview),
            "config" => Some(Self::Config),
            "local" => Some(Self::Local),
            "profiles" => Some(Self::Profiles),
            "devices" => Some(Self::Devices),
            "users" => Some(Self::Users),
            "routes" => Some(Self::Routes),
            "dns" => Some(Self::Dns),
            "access" => Some(Self::Access),
            "credentials" => Some(Self::Credentials),
            // One page per subject: `tasks` is what this client did, `audit` is
            // what the tailnet was told. The old `activity` name meant both and
            // so described neither.
            "tasks" => Some(Self::Tasks),
            "audit" => Some(Self::Audit),
            "services" => Some(Self::Services),
            "diagnostics" => Some(Self::Diagnostics),
            _ => None,
        }
    }

    pub const fn requires_admin_profile(self) -> bool {
        matches!(
            self,
            Self::Users | Self::Routes | Self::Access | Self::Credentials | Self::Audit
        )
    }

    pub const fn requires_local_daemon(self) -> bool {
        matches!(self, Self::Services | Self::Diagnostics)
    }

    pub const fn requires_observation_source(self) -> bool {
        matches!(self, Self::Devices | Self::Dns)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Focus {
    Collection,
    Inspector,
}

/// Whether the local client and the active profile are talking about the same
/// tailnet. Tale reads two independent sources and used to assume they agreed;
/// this is the assumption made explicit so it can be false.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SourceAlignment {
    /// Only one source is in play, so there is nothing to reconcile.
    Single,
    SameTailnet,
    /// This machine is on one tailnet and the active profile administers
    /// another. Both are legitimate; neither describes the other.
    Divergent {
        local: String,
        admin: String,
    },
    /// Both sources are present but at least one has not yet said which tailnet
    /// it is on.
    Undetermined,
}

/// Which source `:devices` is showing.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DeviceViewSource {
    /// The local client's peers. No profile is active.
    Local,
    /// The active profile's devices, carrying local detail because both sources
    /// are on the same tailnet.
    Composed,
    /// The active profile's devices alone, because this machine is not on that
    /// tailnet or has not proven that it is.
    Admin,
}

impl DeviceViewSource {
    /// Whether the rows on screen are peers this machine can actually reach.
    /// Ping, whois, SSH and Taildrop all go through the local daemon, so they
    /// are meaningless against a tailnet it is not on.
    pub const fn is_locally_reachable(self) -> bool {
        matches!(self, Self::Local | Self::Composed)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Composed => "local + admin",
            Self::Admin => "admin",
        }
    }
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FilterSuggestionKind {
    Field,
    Operator,
    Value,
}

/// One offer in the filter tray. `insertion` replaces the token under the cursor.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilterSuggestion {
    pub kind: FilterSuggestionKind,
    pub text: String,
    pub insertion: String,
    pub note: String,
    pub matches: Vec<u32>,
    score: u32,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilterSuggestionSection {
    pub label: String,
    pub suggestions: Vec<FilterSuggestion>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NavigationCandidate {
    pub route: Route,
    pub label: String,
    pub description: String,
    pub description_matches: Vec<u32>,
    score: u32,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommandLineState {
    pub editor: LineEditorState,
    pub generation: u64,
    pub candidates: Vec<NavigationCandidate>,
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
    /// The cursor `:profiles` had, which the filter moves and Esc puts back.
    pub profile_selection: usize,
    /// The cursor `:config` had before live filtering began.
    pub config_selection: usize,
    pub collection_selection: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilterErrorReport {
    pub message: String,
    pub expected: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilterLineState {
    pub editor: LineEditorState,
    pub generation: u64,
    pub sections: Vec<FilterSuggestionSection>,
    pub selected_completion: Option<usize>,
    pub error: Option<FilterErrorReport>,
    pub restoration: FilterRestoration,
    pub purpose: FilterLinePurpose,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum FilterLinePurpose {
    Collection,
    DetailSearch {
        route: Route,
        scroll: usize,
        query: String,
        match_line: Option<usize>,
    },
}

impl FilterLineState {
    /// Tray order, which is also the order `Tab` walks.
    pub fn suggestions(&self) -> impl Iterator<Item = &FilterSuggestion> {
        self.sections
            .iter()
            .flat_map(|section| section.suggestions.iter())
    }

    pub fn suggestion_count(&self) -> usize {
        self.sections
            .iter()
            .map(|section| section.suggestions.len())
            .sum()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TransientKind {
    Action,
    Copy,
    /// Any menu that asks the user to pick one value. Same grammar as `a`:
    /// bottom-anchored, grouped, direct keys, no row cursor.
    Choice,
}

/// What picking a choice does. One menu type serves every picker.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ChoiceOutcome {
    Sort(SortSpec),
    ServiceSort(ServiceSortSpec),
    ProfileSort(ProfileSortSpec),
    ConfigSort(SettingSortSpec),
    TaskSort(TaskSortSpec),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MenuChoice {
    /// One or two keys, exactly like an action mnemonic. A two-key sequence
    /// drills down: the first key replaces the menu with the variants of one
    /// subject, and the second picks among them.
    pub sequence: String,
    /// Heading for this choice at the top level.
    pub group: String,
    /// The thing being chosen, shown as the top-level label and as the heading
    /// once drilled in. Empty when the menu is a single flat level.
    pub subject: String,
    pub label: String,
    /// True for the value already in force, so the menu shows where you are.
    pub active: bool,
    pub outcome: ChoiceOutcome,
}

/// The copy key that opens the per-address level, and the key that takes all
/// of them once inside it.
pub const ADDRESS_PREFIX: char = 'a';

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TransientMenuState {
    pub kind: TransientKind,
    pub title: &'static str,
    pub actions: Vec<ActionId>,
    pub choices: Vec<MenuChoice>,
    pub fields: Vec<CopyField>,
    /// Individual addresses, so copying one does not mean copying all of them.
    pub addresses: Vec<String>,
    pub prefix: Option<char>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum InteractionMode {
    Normal,
    CommandLine(CommandLineState),
    FilterLine(FilterLineState),
    Transient(TransientMenuState),
    HelpSheet,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ConfirmationState {
    pub action_id: ActionId,
    pub admin_generation: u64,
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

/// One option of a `Choice` field. The value is what the action reads; the
/// label is what the user picks between, so an opaque identifier can still be
/// chosen by the name the rest of the screen calls it.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FormChoice {
    pub value: String,
    pub label: String,
}

impl FormChoice {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }

    /// An option whose value reads well enough to be its own label.
    pub fn plain(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            label: value.clone(),
            value,
        }
    }
}

/// What a form field accepts. The kind decides how it is edited, so the user
/// never has to know a separator or spell out a value the field already knows.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum FieldKind {
    /// Free text. `hint` is shown in place of an empty value.
    Text { hint: &'static str },
    /// One of a fixed set, cycled with Left and Right.
    Choice { options: Vec<FormChoice> },
    /// Yes or no, toggled with Space.
    Toggle,
    /// An ordered set of values, edited one entry at a time inside the form.
    /// The field value is the entries joined by commas.
    List { hint: &'static str },
    /// A write-only secret. The typed characters never reach the field value;
    /// they are held once, zeroized, on the form itself.
    Secret,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FormField {
    pub key: &'static str,
    pub label: &'static str,
    /// One line explaining the field, shown while it is selected.
    pub help: &'static str,
    pub kind: FieldKind,
    pub value: String,
    /// Why the field cannot be changed here, when something outside the form
    /// decides it. A locked field is still shown: the user can see what holds.
    pub locked: Option<String>,
}

impl FormField {
    pub fn text(
        key: &'static str,
        label: &'static str,
        help: &'static str,
        hint: &'static str,
        value: impl Into<String>,
    ) -> Self {
        Self {
            key,
            label,
            help,
            kind: FieldKind::Text { hint },
            value: value.into(),
            locked: None,
        }
    }

    pub fn choice(
        key: &'static str,
        label: &'static str,
        help: &'static str,
        options: impl IntoIterator<Item = FormChoice>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            key,
            label,
            help,
            kind: FieldKind::Choice {
                options: options.into_iter().collect(),
            },
            value: value.into(),
            locked: None,
        }
    }

    /// A choice between values that read well enough to be their own labels.
    pub fn options(
        key: &'static str,
        label: &'static str,
        help: &'static str,
        options: &[&str],
        value: impl Into<String>,
    ) -> Self {
        Self::choice(
            key,
            label,
            help,
            options.iter().copied().map(FormChoice::plain),
            value,
        )
    }

    pub fn secret(key: &'static str, label: &'static str, help: &'static str) -> Self {
        Self {
            key,
            label,
            help,
            kind: FieldKind::Secret,
            value: String::new(),
            locked: None,
        }
    }

    pub fn toggle(key: &'static str, label: &'static str, help: &'static str, value: bool) -> Self {
        Self {
            key,
            label,
            help,
            kind: FieldKind::Toggle,
            value: if value { "yes" } else { "no" }.to_owned(),
            locked: None,
        }
    }

    pub fn list(
        key: &'static str,
        label: &'static str,
        help: &'static str,
        hint: &'static str,
        entries: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            key,
            label,
            help,
            kind: FieldKind::List { hint },
            value: entries
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .join(","),
            locked: None,
        }
    }

    /// States that the field is shown but not answered here, and why.
    pub fn locked(mut self, reason: impl Into<String>) -> Self {
        self.locked = Some(reason.into());
        self
    }

    /// The entries of a list field, in order and without the empty ones.
    pub fn entries(&self) -> Vec<String> {
        self.value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// The label the selected value is shown under.
    pub fn display(&self) -> &str {
        match &self.kind {
            FieldKind::Choice { options } => options
                .iter()
                .find(|option| option.value == self.value)
                .map_or(self.value.as_str(), |option| option.label.as_str()),
            _ => self.value.as_str(),
        }
    }

    /// Moves a choice or toggle to its next value; other kinds ignore this.
    fn cycle(&mut self, forward: bool) {
        let options: Vec<&str> = match &self.kind {
            FieldKind::Choice { options } => {
                options.iter().map(|option| option.value.as_str()).collect()
            }
            FieldKind::Toggle => vec!["no", "yes"],
            FieldKind::Text { .. } | FieldKind::List { .. } | FieldKind::Secret => return,
        };
        let Some(length) = std::num::NonZeroUsize::new(options.len()) else {
            return;
        };
        let length = length.get();
        let current = options
            .iter()
            .position(|option| *option == self.value)
            .unwrap_or(0);
        let next = if forward {
            current.saturating_add(1) % length
        } else {
            current.checked_sub(1).unwrap_or(length.saturating_sub(1))
        };
        self.value = options.get(next).map_or("", |option| option).to_owned();
    }

    pub const fn is_text(&self) -> bool {
        matches!(self.kind, FieldKind::Text { .. })
    }

    const fn is_list(&self) -> bool {
        matches!(self.kind, FieldKind::List { .. })
    }

    pub const fn is_secret(&self) -> bool {
        matches!(self.kind, FieldKind::Secret)
    }
}

/// The entries of a list field while it is open, so a set that has an order can
/// be reordered without spelling the whole set out again.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ListEditor {
    pub entries: Vec<String>,
    pub selected: usize,
}

impl ListEditor {
    fn new(field: &FormField) -> Self {
        Self {
            entries: field.entries(),
            selected: 0,
        }
    }

    fn joined(&self) -> String {
        self.entries
            .iter()
            .map(|entry| entry.trim())
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>()
            .join(",")
    }

    fn select(&mut self, offset: isize) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = move_bounded_index(self.selected, self.entries.len(), offset);
    }

    fn move_entry(&mut self, offset: isize) {
        if self.entries.is_empty() {
            return;
        }
        let target = if offset.is_negative() {
            self.selected.saturating_sub(offset.unsigned_abs())
        } else {
            self.selected.saturating_add(offset.unsigned_abs())
        };
        if target >= self.entries.len() || target == self.selected {
            return;
        }
        self.entries.swap(self.selected, target);
        self.selected = target;
    }

    fn insert(&mut self) {
        let position = if self.entries.is_empty() {
            0
        } else {
            self.selected.saturating_add(1).min(self.entries.len())
        };
        self.entries.insert(position, String::new());
        self.selected = position;
    }

    fn remove(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.entries
            .remove(self.selected.min(self.entries.len().saturating_sub(1)));
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    fn edit<F: FnOnce(&mut String)>(&mut self, change: F) {
        if self.entries.is_empty() {
            self.entries.push(String::new());
            self.selected = 0;
        }
        if let Some(entry) = self.entries.get_mut(self.selected) {
            change(entry);
        }
    }
}

/// What a form is made of, before it becomes an overlay.
pub struct FormShape {
    pub title: &'static str,
    pub subject: Vec<(&'static str, String)>,
    pub fields: Vec<FormField>,
}

impl FormShape {
    pub fn new(
        title: &'static str,
        subject: Vec<(&'static str, String)>,
        fields: Vec<FormField>,
    ) -> Self {
        Self {
            title,
            subject,
            fields,
        }
    }
}

/// A form the user fills in field by field. Anything already known — the row
/// they selected, the machine they are on — is stated, not asked for.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FormState {
    pub action_id: ActionId,
    pub title: &'static str,
    /// Context the form acts on but does not ask for, as label and value.
    pub subject: Vec<(&'static str, String)>,
    pub fields: Vec<FormField>,
    pub selected: usize,
    /// Byte offset of the insertion point in an open text field.
    pub cursor: usize,
    /// The value held before editing began, restored if the edit is abandoned.
    pub draft: Option<String>,
    /// The open list field's entries, while one is open.
    pub list: Option<ListEditor>,
    /// The write-only secret a `Secret` field has been given, if any.
    pub secret: Option<SecretInput>,
    pub error: Option<String>,
}

impl FormState {
    fn selected_field_mut(&mut self) -> Option<&mut FormField> {
        self.fields.get_mut(self.selected)
    }

    pub fn selected_field(&self) -> Option<&FormField> {
        self.fields.get(self.selected)
    }

    pub const fn is_editing(&self) -> bool {
        self.draft.is_some()
    }

    /// The row past the last field submits the form, so Enter means the same
    /// thing everywhere: act on what is selected.
    pub fn on_submit_row(&self) -> bool {
        self.selected >= self.fields.len()
    }

    /// Why the selected field cannot be edited, if something else decides it.
    fn locked_reason(&self) -> Option<&str> {
        self.selected_field()
            .and_then(|field| field.locked.as_deref())
    }

    fn begin_edit(&mut self) {
        let Some(field) = self.fields.get(self.selected) else {
            return;
        };
        self.draft = Some(field.value.clone());
        self.cursor = field.value.len();
        self.list = field.is_list().then(|| ListEditor::new(field));
        if field.is_secret() {
            // A secret is written once: opening the field starts it over rather
            // than editing something the form cannot show.
            self.secret = Some(SecretInput::new());
        }
    }

    /// How many characters the secret holds, so it can be shown as its length
    /// without ever showing its value.
    pub fn secret_length(&self) -> usize {
        self.secret
            .as_ref()
            .map_or(0, |secret| secret.as_str().chars().count())
    }

    fn commit_edit(&mut self) {
        if let Some(list) = self.list.take() {
            let joined = list.joined();
            if let Some(field) = self.fields.get_mut(self.selected) {
                field.value = joined;
            }
        }
        self.draft = None;
    }

    fn abandon_edit(&mut self) {
        self.list = None;
        if let Some(previous) = self.draft.take()
            && let Some(field) = self.fields.get_mut(self.selected)
        {
            field.value = previous;
        }
        self.cursor = 0;
    }

    fn move_selection(&mut self, offset: isize) {
        // One past the fields is the submit row.
        let length = self.fields.len().saturating_add(1);
        let step = offset.rem_euclid(length as isize).unsigned_abs();
        self.selected = self.selected.saturating_add(step) % length;
    }

    pub fn value(&self, key: &str) -> &str {
        self.fields
            .iter()
            .find(|field| field.key == key)
            .map_or("", |field| field.value.as_str())
    }

    /// The entries of a list field, in order.
    pub fn entries(&self, key: &str) -> Vec<String> {
        self.fields
            .iter()
            .find(|field| field.key == key)
            .map(FormField::entries)
            .unwrap_or_default()
    }

    /// Whether a toggle or yes/no choice is on.
    pub fn is_yes(&self, key: &str) -> bool {
        self.value(key) == "yes"
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CopyField {
    DeviceId,
    DisplayName,
    Hostname,
    DnsName,
    Owner,
    Addresses,
    Tags,
    PublicKey,
    Endpoint,
    DiagnosticSummary,
    Metrics,
    ServiceUrl,
    ServiceListener,
    ServiceBackend,
    UserId,
    UserName,
    UserLogin,
    TaskId,
    TaskResult,
    TaskCommand,
    TaskOutput,
    ProfileName,
    ProfileTailnet,
    ProfileAccount,
    ProfileCredential,
    ProfileBackend,
    ConfigSetting,
    ConfigValue,
    ConfigSource,
}

/// The headings the copy menu offers, in the order it shows them.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CopyGroup {
    Configuration,
    Service,
    Identity,
    Network,
    Diagnostics,
}

impl CopyGroup {
    pub const ALL: [Self; 5] = [
        Self::Configuration,
        Self::Service,
        Self::Identity,
        Self::Network,
        Self::Diagnostics,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Configuration => "Configuration",
            Self::Service => "Service",
            Self::Identity => "Identity",
            Self::Network => "Network",
            Self::Diagnostics => "Diagnostics",
        }
    }
}

impl CopyField {
    /// Which heading this field appears under. A total match rather than a
    /// list of members held beside the menu: the list version silently dropped
    /// any field nobody remembered to add to it.
    pub const fn group(self) -> CopyGroup {
        match self {
            Self::ConfigSetting | Self::ConfigValue | Self::ConfigSource => {
                CopyGroup::Configuration
            }
            Self::ServiceUrl | Self::ServiceListener | Self::ServiceBackend => CopyGroup::Service,
            Self::DeviceId
            | Self::DisplayName
            | Self::Hostname
            | Self::DnsName
            | Self::Owner
            | Self::Tags
            | Self::UserId
            | Self::UserName
            | Self::UserLogin
            | Self::TaskId
            | Self::ProfileName
            | Self::ProfileTailnet
            | Self::ProfileAccount
            | Self::ProfileCredential => CopyGroup::Identity,
            Self::Addresses | Self::PublicKey | Self::Endpoint | Self::ProfileBackend => {
                CopyGroup::Network
            }
            Self::DiagnosticSummary
            | Self::Metrics
            | Self::TaskResult
            | Self::TaskCommand
            | Self::TaskOutput => CopyGroup::Diagnostics,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::DeviceId => "id",
            Self::DisplayName => "name",
            Self::Hostname => "hostname",
            Self::DnsName => "DNS name",
            Self::Owner => "owner",
            Self::Addresses => "addresses",
            Self::Tags => "tags",
            Self::PublicKey => "public key",
            Self::Endpoint => "endpoint",
            Self::DiagnosticSummary => "diagnostic summary",
            Self::Metrics => "metrics",
            Self::ServiceUrl => "url",
            Self::ServiceListener => "listener",
            Self::ServiceBackend => "backend",
            Self::UserId => "id",
            Self::UserName => "name",
            Self::UserLogin => "login",
            Self::TaskId => "id",
            Self::TaskResult => "result",
            Self::TaskCommand => "command",
            Self::TaskOutput => "output",
            Self::ProfileName => "profile",
            Self::ProfileTailnet => "tailnet",
            Self::ProfileAccount => "account",
            Self::ProfileCredential => "credential",
            Self::ProfileBackend => "store path",
            Self::ConfigSetting => "setting",
            Self::ConfigValue => "value",
            Self::ConfigSource => "source",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Overlay {
    QuitConfirmation,
    TaskInspector(TaskId),
    Confirmation(Box<ConfirmationState>),
    Form(FormState),
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
    pub local_section: Option<LocalSection>,
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
            local_section: None,
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
    /// First visible line in the full-screen device detail opened by Enter.
    /// The side inspector always stays at its summary's first line.
    pub detail_scroll: usize,
    pub detail_search: String,
    pub detail_search_match: Option<usize>,
    pub filter_draft: String,
    pub applied_filter: FilterExpression,
    pub sort: SortSpec,
    pub sort_terms: Vec<SortSpec>,
    pub wide_columns: bool,
    /// Whether the side inspector shares the pane with the table. Off by
    /// default: the table is what the route is for, and the inspector repeats
    /// a row it is already showing. `i` brings it in; it says nothing about
    /// focus.
    pub inspector: bool,
    pub columns: Vec<String>,
}

impl Default for DeviceViewState {
    fn default() -> Self {
        Self {
            selected_id: None,
            scroll: 0,
            detail_scroll: 0,
            detail_search: String::new(),
            detail_search_match: None,
            filter_draft: String::new(),
            applied_filter: FilterExpression::empty(),
            sort: SortSpec::default(),
            sort_terms: vec![SortSpec::default()],
            wide_columns: false,
            inspector: false,
            columns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct DeviceVisibleCacheKey {
    devices_generation: u64,
    local_generation: u64,
    admin_generation: u64,
    now: Option<Timestamp>,
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
    pub overview: OverviewViewState,
    pub local: LocalViewState,
    pub devices: DeviceViewState,
    pub services: ServiceViewState,
    pub diagnostics: DiagnosticsViewState,
    pub users: UserViewState,
    pub routes: CollectionViewState,
    pub credentials: CollectionViewState,
    pub audit: CollectionViewState,
    pub tasks: TaskViewState,
    pub profiles: ProfileViewState,
    pub config: ConfigViewState,
}

#[derive(Debug, Clone, Default)]
pub struct LocalViewState {
    pub section: LocalSection,
    pub selected: usize,
    pub scroll: usize,
}

/// What `:config` remembers while its resolved settings are projected as rows.
#[derive(Debug, Clone, Default)]
pub struct ConfigViewState {
    pub selected: usize,
    pub sort: SettingSortSpec,
    pub filter: String,
}

/// What `:overview` remembers between refreshes. Findings are derived again
/// whenever their authoritative snapshots change, so an ID is the only stable
/// cursor: an index would silently move to a different problem after a refresh.
#[derive(Debug, Clone, Default)]
pub struct OverviewViewState {
    pub selected_id: Option<String>,
}

/// What `:profiles` remembers between frames. The list is short and derived from
/// configuration rather than fetched, so the cursor is an index into it.
#[derive(Debug, Clone, Default)]
pub struct ProfileViewState {
    pub inspector: bool,
    pub selected: usize,
    pub sort: ProfileSortSpec,
    /// Free text, matched against the whole row. There is no expression to
    /// parse, so there is no such thing as an invalid one.
    pub filter: String,
}

/// One row of `:profiles`. The two variants are the two unrelated things the
/// page lists: the client on this machine, and a credential for somebody's
/// control plane. Keeping them apart in the type is what stops the page from
/// pretending a stored API token says anything about the local daemon.
#[derive(Debug, Clone, Copy)]
pub enum ProfileRow<'a> {
    Local {
        tailnet: Option<&'a str>,
        account: Option<&'a str>,
        state: &'static str,
        active: bool,
    },
    Admin {
        name: &'a str,
        config: &'a crate::config::ProfileConfig,
        status: Option<&'a ProfileStatus>,
        active: bool,
    },
}

impl<'a> ProfileRow<'a> {
    /// The configured profile's name, or `None` for the local client, which has
    /// no name because it is not a profile.
    pub const fn name(&self) -> Option<&'a str> {
        match self {
            Self::Local { .. } => None,
            Self::Admin { name, .. } => Some(name),
        }
    }

    pub const fn label(&self) -> &'a str {
        match self {
            Self::Local { .. } => "local",
            Self::Admin { name, .. } => name,
        }
    }

    pub const fn active(&self) -> bool {
        match self {
            Self::Local { active, .. } | Self::Admin { active, .. } => *active,
        }
    }

    pub fn tailnet(&self) -> Option<&'a str> {
        match self {
            Self::Local { tailnet, .. } => *tailnet,
            Self::Admin { config, .. } => Some(config.tailnet.as_str()),
        }
    }

    /// One word for the row's condition. For the local client that is the
    /// daemon's state; for a profile it is the store's answer, or the control
    /// plane's once the user has asked for one.
    pub fn state_label(&self) -> &'static str {
        match self {
            Self::Local { state, .. } => state,
            Self::Admin { status, .. } => status.map_or("reading", ProfileStatus::label),
        }
    }

    /// What activating this row would permit. The local client is bounded by
    /// the daemon, not by Tale, so it makes no claim here.
    pub const fn access_label(&self) -> &'static str {
        match self {
            Self::Local { .. } => "-",
            Self::Admin { config, .. } => {
                if config.read_only {
                    "read-only"
                } else {
                    "read-write"
                }
            }
        }
    }

    /// Why the row is in the state it is in, when there is more to say than the
    /// one word.
    pub fn detail(&self) -> Option<&'a str> {
        match self {
            Self::Local { .. } => None,
            Self::Admin { status, .. } => {
                let status = (*status)?;
                match status.presence.as_ref() {
                    Some(CredentialPresence::Unreadable { detail }) => Some(detail.as_str()),
                    _ => status.probe.detail(),
                }
            }
        }
    }

    /// Whether the row answers to a free-text query. Every column the table can
    /// show is part of the haystack, so what is on screen is what is searched.
    fn matches(&self, needle: &str) -> bool {
        let mut haystack = vec![
            self.label().to_ascii_lowercase(),
            self.state_label().to_ascii_lowercase(),
            self.access_label().to_ascii_lowercase(),
        ];
        if let Some(tailnet) = self.tailnet() {
            haystack.push(tailnet.to_ascii_lowercase());
        }
        match self {
            Self::Local { account, .. } => {
                if let Some(account) = account {
                    haystack.push(account.to_ascii_lowercase());
                }
                haystack.push("local".to_owned());
            }
            Self::Admin { config, .. } => {
                haystack.push(config.credential.to_ascii_lowercase());
                haystack.push(config.credential_backend.label().to_ascii_lowercase());
            }
        }
        haystack
            .iter()
            .any(|value| filter::contains_matches(value, needle))
    }

    fn ordering_key(&self, field: ProfileSortField) -> String {
        match field {
            ProfileSortField::Name => self.label().to_owned(),
            ProfileSortField::Tailnet => self.tailnet().unwrap_or_default().to_owned(),
            ProfileSortField::State => self.state_label().to_owned(),
            ProfileSortField::Access => self.access_label().to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TaskSortField {
    Recency,
    State,
    Duration,
}

impl TaskSortField {
    pub const ALL: [Self; 3] = [Self::Recency, Self::State, Self::Duration];

    pub const fn key(self) -> char {
        match self {
            Self::Recency => 'r',
            Self::State => 's',
            Self::Duration => 't',
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Recency => "recency",
            Self::State => "state",
            Self::Duration => "time took",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct TaskSortSpec {
    pub field: TaskSortField,
    pub direction: SortDirection,
}

impl Default for TaskSortSpec {
    fn default() -> Self {
        Self {
            field: TaskSortField::Recency,
            direction: SortDirection::Descending,
        }
    }
}

/// What `:tasks` remembers between frames. The selection itself lives in the
/// task store, beside the history it indexes into.
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct TaskViewState {
    /// Whether the side inspector shares the pane with the table. Off by
    /// default, the way devices and users are: the table is what the route is
    /// for, and a task's output is long enough to want the full width when you
    /// do ask for it.
    pub inspector: bool,
    pub sort: TaskSortSpec,
    pub show_history: bool,
}


/// What `:users` remembers between frames. The selection itself lives in
/// `admin_user_selected`, beside the other admin cursors.
#[derive(Debug, Clone, Default)]
pub struct UserViewState {
    pub filter: String,
    /// Whether the side inspector shares the pane with the table. Off by
    /// default, the way the devices pane is: the table is what the route is
    /// for, and the inspector repeats a row already on screen.
    pub inspector: bool,
}

/// The shared view state for admin collections whose cursor lives beside the
/// resource snapshot. The inspector starts closed on every collection route;
/// `i` is the one key that both shows and hides it.
#[derive(Debug, Clone, Default)]
pub struct CollectionViewState {
    pub inspector: bool,
    pub filter: String,
}

#[derive(Debug, Clone)]
pub struct ServiceViewState {
    pub inspector: bool,
    pub section: ServiceSection,
    pub selected: usize,
    pub scroll: usize,
    pub filter_draft: String,
    pub applied_filter: FilterExpression,
    pub sort: ServiceSortSpec,
}

impl Default for ServiceViewState {
    fn default() -> Self {
        Self {
            inspector: false,
            section: ServiceSection::Serve,
            selected: 0,
            scroll: 0,
            filter_draft: String::new(),
            applied_filter: FilterExpression::empty(),
            sort: ServiceSortSpec::default(),
        }
    }
}

/// How the mapping table is ordered. Exposure first, so the public rows — the
/// ones that carry risk — sit together at a predictable end of the list.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ServiceSortSpec {
    pub field: ServiceSortField,
    pub direction: SortDirection,
}

impl Default for ServiceSortSpec {
    fn default() -> Self {
        Self {
            field: ServiceSortField::Exposure,
            direction: SortDirection::Ascending,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DiagnosticsViewState {
    pub section: DiagnosticsSection,
    pub scroll: usize,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum DiagnosticsSection {
    #[default]
    Client,
    DnsStatus,
}

impl DiagnosticsSection {
    pub const ALL: [Self; 2] = [Self::Client, Self::DnsStatus];
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ShutdownState {
    Running,
    Requested(ShutdownReason),
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum PolicyWorkflowView {
    #[default]
    Actions,
    Validation,
    Preview,
    Diff,
}

#[derive(Debug)]
pub struct App {
    pub view_history: ViewHistory,
    pub interaction: InteractionMode,
    next_completion_generation: u64,
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
    /// What `:profiles` knows about each configured profile, keyed by name.
    pub profile_statuses: BTreeMap<String, ProfileStatus>,
    /// The profile whose activation probe is outstanding. One at a time: a
    /// second attempt supersedes the first rather than racing it.
    profile_probe_in_flight: Option<String>,
    /// An admin-only destination requested before a profile was active. Profile
    /// activation resumes it only after the credential probe succeeds.
    pending_navigation_route: Option<Route>,
    opened_task_return: bool,
    pub policy_workflow: Option<PolicyWorkflow>,
    pub policy_workflow_view: PolicyWorkflowView,
    policy_temp_file: Option<Arc<Mutex<crate::temporary::TemporaryPolicyFile>>>,
    latest_policy_temp_file: Option<Arc<Mutex<crate::temporary::TemporaryPolicyFile>>>,
    pub secret_result: Option<SecretResult>,
    next_policy_workflow_id: u64,
    next_secret_result_id: u64,
    pending_auth_key_request: Option<crate::admin::key_mutations::AuthKeyCreateRequest>,
    pending_auth_key_result: Option<u64>,
    pending_operational_mutation: Option<u64>,
    next_operational_mutation_id: u64,
    pending_credential_revoke: Option<String>,
    pub admin_user_selected: usize,
    pub admin_route_selected: usize,
    pub admin_credential_selected: usize,
    pub admin_activity_selected: usize,
    pub admin_audit_window_days: u64,
    pub audit_filters: AuditFilters,
    pub task_filter: String,
    pub detail_scroll: usize,
    pub detail_search: String,
    pub detail_search_match: Option<usize>,
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
    pub local_accounts_failure: Option<LocalFailure>,
    pub system_policy: Vec<SystemPolicyEntry>,
    pub system_policy_failure: Option<LocalFailure>,
    pub local_diagnostics: BTreeMap<TaskId, DiagnosticState>,
    pub tasks: TaskStore,
    pub task_history_loading: bool,
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
    mock_clock_started_at: Option<Instant>,
    pub runtime_error: Option<String>,
    /// Non-error feedback about a completed UI interaction. Unlike
    /// `runtime_error`, this never affects process success and renders at the
    /// informational level below errors and task results.
    pub status_notice: Option<String>,
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
    local_observer_generation: u64,
    /// CLI discovery runs subprocesses and finishes long after the daemon
    /// watcher has moved on. It gets its own counter so a newer status snapshot
    /// never discards a discovery result.
    local_discovery_generation: u64,
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
        let admin = if config.mock {
            mock::admin_snapshot()
        } else {
            AdminSnapshot::new(
                selected_profile,
                tailnet,
                profile_read_only || config.read_only,
                Vec::new(),
            )
        };
        let local_preferences_resource = if config.mock {
            let mut resource = LocalPreferencesResource::new();
            let _ = resource.succeed(1, mock::local_preferences());
            resource
        } else {
            LocalPreferencesResource::new()
        };
        let (saved_views, saved_views_error) = if config.experimental_features.saved_views {
            match crate::saved_views::SavedViewsState::load(&config.paths.state_dir) {
                Ok(value) => (Some(value), None),
                Err(error) => (None, Some(format!("saved-view state is invalid: {error}"))),
            }
        } else {
            (None, None)
        };
        let initial_route = if source_mode == SourceMode::Unavailable && admin.profile.is_none() {
            Route::Overview
        } else {
            Route::DEFAULT
        };
        Self {
            view_history: ViewHistory::new(initial_route),
            interaction: InteractionMode::Normal,
            next_completion_generation: 0,
            focus: Focus::Collection,
            overlays: Vec::new(),
            views: Views {
                overview: OverviewViewState::default(),
                local: LocalViewState::default(),
                devices: DeviceViewState::default(),
                services: ServiceViewState::default(),
                diagnostics: DiagnosticsViewState::default(),
                users: UserViewState::default(),
                routes: CollectionViewState::default(),
                credentials: CollectionViewState::default(),
                audit: CollectionViewState::default(),
                tasks: TaskViewState::default(),
                profiles: ProfileViewState::default(),
                config: ConfigViewState::default(),
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
            profile_statuses: BTreeMap::new(),
            profile_probe_in_flight: None,
            pending_navigation_route: None,
            opened_task_return: false,
            policy_workflow: None,
            policy_workflow_view: PolicyWorkflowView::Actions,
            policy_temp_file: None,
            latest_policy_temp_file: None,
            secret_result: None,
            next_policy_workflow_id: 1,
            next_secret_result_id: 1,
            pending_auth_key_request: None,
            pending_auth_key_result: None,
            pending_operational_mutation: None,
            next_operational_mutation_id: 1,
            pending_credential_revoke: None,
            admin_user_selected: 0,
            admin_route_selected: 0,
            admin_credential_selected: 0,
            admin_activity_selected: 0,
            admin_audit_window_days: 1,
            audit_filters: AuditFilters::default(),
            task_filter: String::new(),
            detail_scroll: 0,
            detail_search: String::new(),
            detail_search_match: None,
            composed_devices: Vec::new(),
            local_resource: if config.mock {
                mock::local_resource()
            } else {
                LocalResource::new()
            },
            local_preferences_resource,
            local_state,
            local_daemon_state,
            local_cli_state,
            local_executable: None,
            local_capabilities: if config.mock {
                LocalCapabilities::all_supported()
            } else {
                LocalCapabilities::default()
            },
            services_snapshot: if source_mode == SourceMode::Mock {
                mock_services_snapshot()
            } else {
                LocalServicesSnapshot::new()
            },
            alpha_local_features: false,
            certificate_verification: None,
            service_locks: Vec::new(),
            local_preferences: if config.mock {
                mock::local_preferences()
            } else {
                LocalPreferences::empty(0)
            },
            local_accounts: Vec::new(),
            local_accounts_failure: None,
            system_policy: Vec::new(),
            system_policy_failure: None,
            local_diagnostics: if config.mock {
                mock::local_diagnostics()
            } else {
                BTreeMap::new()
            },
            tasks: TaskStore::new(),
            task_history_loading: config.history.persist_tasks && !config.mock,
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
            mock_clock_started_at: None,
            runtime_error: saved_views_error,
            status_notice: None,
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
            local_observer_generation: 0,
            local_discovery_generation: 0,
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
        // Every profile's credential is read up front because it is a local read
        // and `:profiles` is useless without it. Nothing is sent anywhere.
        if let Some(effect) = self.inspect_profile_credentials() {
            effects.push(effect);
        }
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
        self.local_discovery_generation = self.local_discovery_generation.saturating_add(1);
        self.local_observer_generation = self.local_observer_generation.saturating_add(1);
        effects.push(Effect::StartLocalObservation {
            generation: self.local_observer_generation,
            initial_status_generation: self.local_resource.generation,
            initial_preferences_generation: self.local_preferences_resource.generation,
            socket_path: self.resolved_config.local.socket_path.clone(),
            timeout: self.resolved_config.local.command_timeout,
            reconcile_interval: self.resolved_config.local.reconcile_interval,
        });
        effects.push(Effect::StartLocalDiscovery {
            generation: self.local_discovery_generation,
            resolution: local_resolution(&self.resolved_config),
            timeout: self.resolved_config.local.command_timeout,
        });
        effects
    }

    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        let input = matches!(event, Event::Input(_));
        let task_count = self.tasks.all().len();
        if !matches!(event, Event::Tick(_)) {
            self.render_invalidated = true;
        }
        if !matches!(self.shutdown_state, ShutdownState::Running)
            && !matches!(event, Event::Task(_) | Event::Tick(_))
        {
            return Vec::new();
        }
        let mut effects = match event {
            Event::Input(input) => self.update_input(input),
            Event::Tick(tick) => self.update_tick(tick),
            Event::Task(task) => self.update_task(*task),
            Event::Source(source) => self.update_source(source),
            Event::Local(local) => self.update_local(*local),
            Event::Services(services) => self.update_services(*services),
            Event::Admin(admin) => self.update_admin(*admin),
            Event::Policy(policy) => self.update_policy(*policy),
            Event::Credential(credential) => self.update_credential(*credential),
            Event::Database(database) => self.update_database(database),
            Event::ShutdownRequested(reason) => self.request_shutdown(reason),
        };
        if input && let Some(task_id) = self.tasks.all().get(task_count).map(|task| task.id) {
            self.navigate(Route::Tasks);
            self.task_filter.clear();
            self.tasks.selected = Some(task_id);
            self.focus = Focus::Inspector;
            self.opened_task_return = true;
        }
        if self.resolved_config.history.persist_tasks && !self.resolved_config.mock {
            let dirty = self.tasks.take_dirty();
            if !dirty.is_empty() {
                effects.push(Effect::PersistTaskHistory(dirty));
            }
        }
        effects
    }

    fn update_database(&mut self, event: crate::event::DatabaseEvent) -> Vec<Effect> {
        self.task_history_loading = false;
        match event {
            crate::event::DatabaseEvent::TaskHistoryLoaded(tasks) => {
                self.tasks.merge_restored(tasks);
                self.tasks
                    .evict_completed(self.resolved_config.history.max_tasks);
                self.select_task_position(0);
            }
            crate::event::DatabaseEvent::TaskHistoryFailed(_) => {
                self.status_notice = Some("Task history is unavailable".to_owned());
            }
        }
        Vec::new()
    }

    fn update_tick(&mut self, tick: Instant) -> Vec<Effect> {
        self.tick_count = self.tick_count.saturating_add(1);
        self.now = if self.source_mode == SourceMode::Mock {
            let started_at = *self.mock_clock_started_at.get_or_insert(tick);
            MOCK_NOW.saturating_add(tick.saturating_duration_since(started_at).as_secs())
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

    pub fn current_route(&self) -> Route {
        self.view_history
            .current()
            .map_or(Route::Overview, |frame| frame.route)
    }

    pub fn overlay_title(&self) -> Option<&'static str> {
        self.overlays.last().map(|overlay| match overlay {
            Overlay::QuitConfirmation => "quit",
            Overlay::TaskInspector(_) => "task",
            Overlay::Confirmation(_) => "confirm local action",
            Overlay::Form(_) => "form",
            Overlay::SecretResult => "secret result",
            Overlay::AuditInvestigation => "audit investigation",
        })
    }

    pub fn clear_render_invalidated(&mut self) {
        self.render_invalidated = false;
    }

    pub fn set_terminal_size(&mut self, width: u16, height: u16) {
        if self.terminal_width == width && self.terminal_height == height {
            return;
        }
        self.terminal_width = width;
        self.terminal_height = height;
        self.clamp_device_detail_scroll();
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

    pub fn filtered_tasks(&self) -> Vec<&crate::task::Task> {
        let source: Vec<_> = if self.views.tasks.show_history {
            self.tasks.filtered(&self.task_filter).collect()
        } else {
            self.tasks.session_filtered(&self.task_filter).collect()
        };
        let mut tasks = source;
        let sort = self.views.tasks.sort;
        tasks.sort_by(|left, right| {
            let order = match sort.field {
                TaskSortField::Recency => left.started_at.cmp(&right.started_at),
                TaskSortField::State => left
                    .state
                    .label()
                    .cmp(right.state.label())
                    .then_with(|| left.started_at.cmp(&right.started_at)),
                TaskSortField::Duration => left
                    .finished_at
                    .unwrap_or(self.now)
                    .saturating_sub(left.started_at)
                    .cmp(
                        &right
                            .finished_at
                            .unwrap_or(self.now)
                            .saturating_sub(right.started_at),
                    )
                    .then_with(|| left.started_at.cmp(&right.started_at)),
            };
            if sort.direction == SortDirection::Descending {
                order.reverse()
            } else {
                order
            }
        });
        tasks
    }

    pub fn filtered_task_count(&self) -> usize {
        self.filtered_tasks().len()
    }

    pub fn visible_task_count(&self) -> usize {
        if self.views.tasks.show_history {
            self.tasks.all().len()
        } else {
            self.tasks.session().count()
        }
    }

    pub fn current_session_failed_task_count(&self) -> usize {
        self.tasks
            .session()
            .filter(|task| task.state == TaskState::Failed)
            .count()
    }

    pub fn select_task_position(&mut self, position: usize) {
        let tasks = self.filtered_tasks();
        let index = position.min(tasks.len().saturating_sub(1));
        self.tasks.selected = tasks.get(index).map(|task| task.id);
    }

    pub fn move_task_selection(&mut self, offset: isize) {
        let tasks = self.filtered_tasks();
        let current = self
            .tasks
            .selected
            .and_then(|selected| tasks.iter().position(|task| task.id == selected))
            .unwrap_or(0);
        let next = move_bounded_index(current, tasks.len(), offset);
        self.tasks.selected = tasks.get(next).map(|task| task.id);
    }

    /// Where the selection sits in the filtered history. The table and the
    /// mouse both size their window from this, so a click lands on the row the
    /// pointer is over rather than on the one the list happens to start with.
    pub fn task_cursor(&self) -> usize {
        let Some(selected) = self.tasks.selected else {
            return 0;
        };
        self.filtered_tasks()
            .iter()
            .position(|task| task.id == selected)
            .unwrap_or(0)
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

fn webhook_create_from_form(state: &FormState) -> Result<OperationalMutation, String> {
    let endpoint_url = required_form_value(state, "url", "where notifications are posted")?;
    let subscriptions =
        SubscriptionSet::from_wire(state.entries("categories"), state.entries("events"))
            .map_err(|error| error.to_string())?;
    let draft = WebhookDraft {
        endpoint_url,
        destination_type: DestinationType::from_wire(state.value("provider")),
        subscriptions,
    };
    draft.validate().map_err(|error| error.to_string())?;
    Ok(OperationalMutation::Webhook(WebhookMutation::Create(draft)))
}

/// A replacement always carries a fresh write-only secret, so the form reads
/// the one it was given and never puts it anywhere the screen can reach.
fn log_stream_from_form(state: &FormState) -> Result<OperationalMutation, String> {
    let log_type = if state.value("type") == "configuration" {
        LogType::Configuration
    } else {
        LogType::Network
    };
    let destination_type = state.value("destination").to_owned();
    if !crate::admin::log_streaming::is_supported_destination(&destination_type) {
        return Err(format!(
            "destination {destination_type} is unavailable in Tale because its documented fields are not adopted"
        ));
    }
    let secret = state
        .secret
        .as_ref()
        .filter(|secret| !secret.is_empty())
        .ok_or_else(|| "replacing a log stream requires a new secret".to_owned())?;
    let token = Some(Arc::new(crate::domain::secret_result::SecretBuffer::new(
        secret.as_str(),
    )));
    let is_gcs = destination_type == "gcs";
    Ok(OperationalMutation::LogStreamReplace(
        LogStreamMutationDraft {
            log_type,
            destination_type,
            url: state.value("url").to_owned(),
            user: optional_form_text(state, "user"),
            upload_period_minutes: optional_form_number(state, "period", "number of minutes")?,
            compression_format: optional_form_text(state, "compression"),
            token: if is_gcs { None } else { token.clone() },
            s3_bucket: optional_form_text(state, "s3-bucket"),
            s3_region: optional_form_text(state, "s3-region"),
            s3_key_prefix: optional_form_text(state, "s3-prefix"),
            s3_authentication_type: optional_form_text(state, "s3-auth"),
            s3_access_key_id: optional_form_text(state, "s3-access-key"),
            s3_role_arn: optional_form_text(state, "s3-role"),
            gcs_bucket: optional_form_text(state, "gcs-bucket"),
            gcs_key_prefix: optional_form_text(state, "gcs-prefix"),
            gcs_scopes: state.entries("gcs-scopes"),
            gcs_credentials: if is_gcs { token } else { None },
            secret_action: SecretAction::Replace,
        },
    ))
}

/// The payload shapes a webhook endpoint can be sent, offered rather than typed.
const WEBHOOK_PROVIDERS: &[&str] = &["none", "slack", "discord", "googlechat", "mattermost"];

/// The destinations Tale can replace a log stream with. Azure, private and
/// Vector destinations are absent because their fields are not adopted here.
const LOG_STREAM_DESTINATIONS: &[&str] = &[
    "splunk", "elastic", "panther", "cribl", "datadog", "axiom", "s3", "gcs",
];

/// The traffic classes a flow can be narrowed to, offered rather than typed.
const TRAFFIC_CLASSES: &[&str] = &[ANY, "virtual", "subnet", "exit", "physical"];

/// A log stream asks for what its destination needs and nothing else, so the
/// field list is built from the destination rather than shown all at once.
fn log_stream_fields(destination: &str, seed: &BTreeMap<&str, String>) -> Vec<FormField> {
    let seeded = |key: &str| seed.get(key).cloned().unwrap_or_default();
    let mut fields = vec![
        FormField::options(
            "type",
            "Logs",
            "Which log the stream carries",
            &["network", "configuration"],
            if seeded("type") == "configuration" {
                "configuration"
            } else {
                "network"
            },
        ),
        FormField::options(
            "destination",
            "Destination",
            "Where the logs are sent; this decides what else is asked for",
            LOG_STREAM_DESTINATIONS,
            if LOG_STREAM_DESTINATIONS.contains(&destination) {
                destination.to_owned()
            } else {
                "splunk".to_owned()
            },
        ),
    ];
    match destination {
        "s3" => fields.extend([
            FormField::text(
                "s3-bucket",
                "Bucket",
                "The S3 bucket the logs are written to",
                "bucket name",
                seeded("s3-bucket"),
            ),
            FormField::text(
                "s3-region",
                "Region",
                "The region the bucket lives in",
                "us-east-1",
                seeded("s3-region"),
            ),
            FormField::text(
                "s3-prefix",
                "Key prefix",
                "The prefix every written object shares",
                "none",
                seeded("s3-prefix"),
            ),
            FormField::text(
                "s3-auth",
                "Authentication",
                "How the bucket is authenticated to",
                "accesskey or rolearn",
                seeded("s3-auth"),
            ),
            FormField::text(
                "s3-access-key",
                "Access key",
                "The access key id, when authenticating with a key",
                "none",
                seeded("s3-access-key"),
            ),
            FormField::text(
                "s3-role",
                "Role",
                "The role ARN, when authenticating with a role",
                "none",
                seeded("s3-role"),
            ),
        ]),
        "gcs" => fields.extend([
            FormField::text(
                "gcs-bucket",
                "Bucket",
                "The Cloud Storage bucket the logs are written to",
                "bucket name",
                seeded("gcs-bucket"),
            ),
            FormField::text(
                "gcs-prefix",
                "Key prefix",
                "The prefix every written object shares",
                "none",
                seeded("gcs-prefix"),
            ),
            FormField::list(
                "gcs-scopes",
                "Scopes",
                "The OAuth scopes the credential is used with",
                "none",
                seeded("gcs-scopes")
                    .split(',')
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            ),
        ]),
        _ => fields.push(FormField::text(
            "url",
            "Endpoint",
            "Where the logs are posted",
            "https://host.example/path",
            seeded("url"),
        )),
    }
    fields.extend([
        FormField::text(
            "user",
            "Username",
            "The username the destination expects, when it needs one",
            "none",
            seeded("user"),
        ),
        FormField::text(
            "period",
            "Upload every",
            "Whole minutes between uploads; empty leaves it to the destination",
            "minutes",
            seeded("period"),
        ),
        FormField::text(
            "compression",
            "Compression",
            "The compression format the destination expects, when it needs one",
            "none",
            seeded("compression"),
        ),
        FormField::secret(
            "secret",
            "Secret",
            "The write-only token or credential; replacing a stream always sets it",
        ),
    ]);
    fields
}

fn flow_window_from_form(
    state: &FormState,
    now: Timestamp,
) -> Result<(FlowWindow, FlowFilter), String> {
    let now = i64::try_from(now)
        .ok()
        .and_then(|value| time::OffsetDateTime::from_unix_timestamp(value).ok())
        .ok_or_else(|| "flow clock is outside the supported timestamp range".to_owned())?;
    let window = FlowWindow::from_rfc3339(state.value("start"), state.value("end"), now)
        .map_err(|error| error.to_string())?;
    let traffic_class = match state.value("class") {
        "virtual" => Some(crate::domain::flow::TrafficClass::Virtual),
        "subnet" => Some(crate::domain::flow::TrafficClass::Subnet),
        "exit" => Some(crate::domain::flow::TrafficClass::Exit),
        "physical" => Some(crate::domain::flow::TrafficClass::Physical),
        _ => None,
    };
    let filter = FlowFilter {
        reporting_node_id: optional_form_text(state, "reporting"),
        reporting_node_label: None,
        source_node_id: optional_form_text(state, "source"),
        source_node_label: None,
        destination_node_id: optional_form_text(state, "destination"),
        destination_node_label: None,
        protocol: optional_form_text(state, "protocol"),
        source_address: optional_form_text(state, "source-address"),
        destination_address: optional_form_text(state, "destination-address"),
        traffic_class,
        source_port: optional_form_number(state, "source-port", "port")?,
        destination_port: optional_form_number(state, "destination-port", "port")?,
        minimum_bytes: optional_form_number(state, "min-bytes", "byte count")?,
    };
    filter.validate().map_err(|error| error.to_string())?;
    Ok((window, filter))
}

fn optional_form_text(state: &FormState, key: &str) -> Option<String> {
    let value = state.value(key).trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn optional_form_number<T: std::str::FromStr>(
    state: &FormState,
    key: &str,
    what: &str,
) -> Result<Option<T>, String> {
    let value = state.value(key).trim();
    if value.is_empty() {
        return Ok(None);
    }
    value
        .parse::<T>()
        .map(Some)
        .map_err(|_| format!("{value} is not a {what}"))
}

fn access_question_from_form(state: &FormState) -> Result<AccessQuestion, String> {
    let source_selector = state.value("source").trim().to_owned();
    if source_selector.is_empty() {
        return Err("the question needs a source".to_owned());
    }
    let destination_selector = state.value("destination").trim().to_owned();
    if destination_selector.is_empty() {
        return Err("the question needs a destination".to_owned());
    }
    let port = state.value("port").trim();
    Ok(AccessQuestion {
        source_selector,
        destination_selector,
        protocol_or_port: (!port.is_empty()).then(|| port.to_owned()),
        ssh_user: None,
        application_capability: None,
        policy_source: if state.value("policy") == "candidate" {
            PolicySource::ActiveCandidate
        } else {
            PolicySource::CurrentRemote
        },
    })
}

/// A field that has to be answered, reported by what it is for rather than by
/// the name of the key behind it.
fn required_form_value(state: &FormState, key: &str, what: &str) -> Result<String, String> {
    let value = state.value(key).trim();
    if value.is_empty() {
        Err(format!("choose {what}"))
    } else {
        Ok(value.to_owned())
    }
}

fn export_from_form(state: &FormState) -> Result<OperationalMutation, String> {
    let collection = match state.value("collection") {
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
    Ok(OperationalMutation::Export(ExportRequest {
        collection,
        format: state.value("format").to_owned(),
        path: expand_form_path(Path::new(&required_form_value(
            state,
            "path",
            "where to write the file",
        )?))?,
    }))
}

fn saved_filter_from_term(term: &FilterTerm) -> Result<FilterClause, String> {
    let (field, operator, value) = match term {
        FilterTerm::Text(_) => {
            return Err(
                "a free-text search cannot be saved exactly; use a field filter first".to_owned(),
            );
        }
        FilterTerm::Field {
            field,
            negated,
            values,
            comparison,
        } => {
            let field = saved_device_filter_field(*field)?;
            if let Some(comparison) = comparison {
                if *negated {
                    return Err("a negated duration comparison cannot be saved exactly".to_owned());
                }
                let (operator, duration) = match comparison {
                    Comparison::Less(duration) => (FilterOperator::LessThan, duration),
                    Comparison::Greater(duration) => (FilterOperator::GreaterThan, duration),
                    Comparison::LessOrEqual(_) | Comparison::GreaterOrEqual(_) => {
                        return Err(
                            "inclusive duration comparisons cannot be saved exactly".to_owned()
                        );
                    }
                };
                let seconds = i64::try_from(duration.as_secs())
                    .map_err(|_| "filter duration is too large to save".to_owned())?;
                (field, operator, FilterValue::Number(seconds))
            } else {
                let [value] = values.as_slice() else {
                    return Err(
                        "a multi-value filter cannot be saved exactly; use one value per field"
                            .to_owned(),
                    );
                };
                (
                    field,
                    if *negated {
                        FilterOperator::NotEquals
                    } else {
                        FilterOperator::Equals
                    },
                    FilterValue::Text(value.clone()),
                )
            }
        }
        FilterTerm::StructuredField {
            field,
            negated,
            value,
            mode,
        } => (
            saved_device_filter_field(*field)?,
            if *negated {
                FilterOperator::NotEquals
            } else {
                match mode {
                    FieldMatchMode::Exact => FilterOperator::Equals,
                    FieldMatchMode::Contains => FilterOperator::Contains,
                    FieldMatchMode::StartsWith => FilterOperator::StartsWith,
                }
            },
            FilterValue::Text(value.clone()),
        ),
    };
    Ok(FilterClause {
        field: field.to_owned(),
        operator,
        value,
    })
}

fn saved_device_filter_field(field: FilterField) -> Result<&'static str, String> {
    match field {
        FilterField::Id => Ok("id"),
        FilterField::Name => Ok("name"),
        FilterField::Online => Ok("online"),
        FilterField::Owner => Ok("owner"),
        FilterField::Os => Ok("os"),
        FilterField::Path => Ok("path"),
        FilterField::Tag => Ok("tag"),
        FilterField::LastSeen => Ok("last_seen"),
        FilterField::Approval => Ok("approval"),
        FilterField::KeyExpiry => Ok("key_expiry"),
        FilterField::ClientVersion => Ok("version"),
        FilterField::Sharing => Ok("sharing"),
        FilterField::Posture => Ok("posture"),
        FilterField::RouteRole => Ok("route_role"),
        FilterField::Property
        | FilterField::Exposure
        | FilterField::Listener
        | FilterField::Port
        | FilterField::Mount
        | FilterField::Backend => {
            Err("this filter field is not available in device saved views".to_owned())
        }
    }
}

fn saved_sort_from_device(sort: SortSpec) -> SortTerm {
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
    SortTerm {
        field: field.to_owned(),
        direction: match sort.direction {
            SortDirection::Ascending => SavedSortDirection::Ascending,
            SortDirection::Descending => SavedSortDirection::Descending,
        },
    }
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
    if matches!(
        filter.operator,
        FilterOperator::GreaterThan | FilterOperator::LessThan
    ) {
        let FilterValue::Number(seconds) = filter.value else {
            return Err("saved duration comparison must use whole seconds".to_owned());
        };
        let seconds = u64::try_from(seconds)
            .map_err(|_| "saved duration comparison cannot be negative".to_owned())?;
        return Ok(FilterTerm::Field {
            field,
            negated: false,
            values: Vec::new(),
            comparison: Some(match filter.operator {
                FilterOperator::GreaterThan => Comparison::Greater(Duration::from_secs(seconds)),
                FilterOperator::LessThan => Comparison::Less(Duration::from_secs(seconds)),
                _ => return Err("saved duration operator is invalid".to_owned()),
            }),
        });
    }
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
            return Err("saved duration operator was not handled".to_owned());
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
        // A bare term is already a substring match in the filter grammar.
        "contains" => Ok(format!("{field}:{value}")),
        "starts_with" => Ok(format!("{field}:starts_with={value}")),
        "greater_than" | "less_than" => {
            let FilterValue::Number(seconds) = filter.value else {
                return Err("saved duration comparison must use whole seconds".to_owned());
            };
            let comparison = if operator == "greater_than" { ">" } else { "<" };
            Ok(format!("{field}:{comparison}{seconds}s"))
        }
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
                FilterField::Exposure => "exposure",
                FilterField::Listener => "listener",
                FilterField::Port => "port",
                FilterField::Mount => "mount",
                FilterField::Backend => "backend",
            };
            let value = if let Some(comparison) = comparison {
                let (operator, duration) = match comparison {
                    Comparison::Less(value) => ("less", value),
                    Comparison::LessOrEqual(value) => ("less_or_equal", value),
                    Comparison::Greater(value) => ("greater", value),
                    Comparison::GreaterOrEqual(value) => ("greater_or_equal", value),
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
                FilterField::Exposure => "exposure",
                FilterField::Listener => "listener",
                FilterField::Port => "port",
                FilterField::Mount => "mount",
                FilterField::Backend => "backend",
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

fn navigation_candidates(query: &str) -> Vec<NavigationCandidate> {
    let pattern = (!query.is_empty()).then(|| {
        Pattern::new(
            query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        )
    });
    let mut matcher = Matcher::new(MatcherConfig::DEFAULT);
    let mut candidates = navigation_catalog()
        .into_iter()
        .filter_map(|(route, label, description)| {
            let Some(pattern) = &pattern else {
                return Some(NavigationCandidate {
                    route,
                    label: label.to_owned(),
                    description: description.to_owned(),
                    description_matches: Vec::new(),
                    score: 0,
                });
            };
            let searchable = format!("{label} {description}");
            let mut characters = Vec::new();
            let haystack = Utf32Str::new(&searchable, &mut characters);
            let mut indices = Vec::new();
            let score = pattern.indices(haystack, &mut matcher, &mut indices)?;
            indices.sort_unstable();
            indices.dedup();
            let label_length = u32::try_from(label.chars().count()).unwrap_or(u32::MAX);
            let description_offset = label_length.saturating_add(1);
            let description_matches = indices
                .into_iter()
                .filter_map(|index| index.checked_sub(description_offset))
                .collect();
            Some(NavigationCandidate {
                route,
                label: label.to_owned(),
                description: description.to_owned(),
                description_matches,
                score,
            })
        })
        .collect::<Vec<_>>();
    if pattern.is_some() {
        candidates.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| navigation_rank(left.route).cmp(&navigation_rank(right.route)))
        });
    }
    candidates
}

/// Local services for `--mock`. Without these the route is a blank screen, so
/// it cannot be demonstrated or snapshot-tested. Two of the mappings are public
/// so the exposure column has something to say.
fn mock_services_snapshot() -> LocalServicesSnapshot {
    /// `tcp` selects a raw TCP listener; otherwise the listener is HTTPS.
    fn mapping(
        exposure: Exposure,
        port: u16,
        tcp: bool,
        mount: &str,
        backend: &str,
    ) -> Option<ServiceMapping> {
        let port = Port::new(port).ok()?;
        Some(ServiceMapping {
            exposure,
            listener: if tcp {
                Listener::Tcp(port)
            } else {
                Listener::Https(port)
            },
            mount: PathMount::parse(mount).ok()?,
            backend: Backend::parse(backend).ok()?,
            proxy_protocol: ProxyProtocol::None,
            hostname: None,
        })
    }
    let observed_at = 1_785_751_200;
    let mut snapshot = LocalServicesSnapshot::new();
    let serve = [
        mapping(Exposure::Tailnet, 443, false, "/", "3000"),
        mapping(
            Exposure::Tailnet,
            443,
            false,
            "/metrics",
            "http://127.0.0.1:9090",
        ),
        mapping(Exposure::Tailnet, 22, true, "/", "22"),
    ];
    let funnel = [
        mapping(Exposure::Public, 8443, false, "/", "8080"),
        mapping(Exposure::Public, 10000, false, "/share", "/srv/public"),
    ];
    snapshot.serve.succeed(
        1,
        observed_at,
        ServeStatus {
            mappings: serve.into_iter().flatten().collect(),
        },
    );
    snapshot.funnel.succeed(
        1,
        observed_at,
        FunnelStatus {
            mappings: funnel.into_iter().flatten().collect(),
        },
    );
    snapshot.taildrop_targets.succeed(
        1,
        observed_at,
        vec![
            TaildropTarget {
                command_target: "alpha".to_owned(),
                display_name: "alpha".to_owned(),
                device_name: Some("alpha.example.ts.net".to_owned()),
                online: Some(true),
                capability_reason: None,
            },
            TaildropTarget {
                command_target: "beta".to_owned(),
                display_name: "beta".to_owned(),
                device_name: Some("beta.example.ts.net".to_owned()),
                online: Some(false),
                capability_reason: None,
            },
        ],
    );
    snapshot.taildrive.succeed(
        1,
        observed_at,
        vec![TaildriveShare {
            name: "documents".to_owned(),
            path: std::path::PathBuf::from("/Users/example/Documents"),
            as_user: None,
        }],
    );
    snapshot.certificate_domains.succeed(
        1,
        observed_at,
        vec!["mock-machine.example.ts.net".to_owned()],
    );
    snapshot.observed_at = Some(observed_at);
    snapshot.command_version = Some("1.98.9".to_owned());
    snapshot
}

fn navigation_catalog() -> [(Route, &'static str, &'static str); 14] {
    [
        (Route::Devices, "devices", "machines & status"),
        (Route::Local, "local", "this machine"),
        (Route::Profiles, "profiles", "which source is active"),
        (Route::Services, "services", "serve, funnel & files"),
        (
            Route::Diagnostics,
            "diagnostics",
            "client metrics & bug report",
        ),
        (Route::Users, "users", "members"),
        (Route::Routes, "routes", "network routes"),
        (Route::Dns, "dns", "name resolution"),
        (Route::Access, "access", "policies"),
        (Route::Credentials, "credentials", "keys & tokens"),
        (Route::Tasks, "tasks", "what this client did"),
        (Route::Audit, "audit", "tailnet log & streams"),
        (Route::Overview, "overview", "fleet summary"),
        (Route::Config, "config", "how this client is set up"),
    ]
}

fn navigation_rank(route: Route) -> usize {
    navigation_catalog()
        .iter()
        .position(|(candidate, _, _)| *candidate == route)
        .unwrap_or(usize::MAX)
}

const SNAPSHOT_VALUE_LIMIT: usize = 100;
const DURATION_SUGGESTIONS: &[&str] = &["<1h", "<24h", "<7d", ">7d", ">30d"];

/// What the token under the cursor is asking for.
enum FilterStage<'a> {
    Field {
        prefix: &'a str,
        fragment: &'a str,
    },
    Value {
        spec: &'static FilterFieldSpec,
        prefix: String,
        fragment: &'a str,
    },
}

/// Byte span of the whitespace-separated token the cursor sits in.
fn active_token(input: &str, cursor: usize) -> (usize, usize) {
    filter::token_spans(input)
        .into_iter()
        .find(|(start, end)| cursor >= *start && cursor <= *end)
        .unwrap_or((cursor, cursor))
}

fn filter_stage<'a>(token: &'a str, schema: &FilterSchema) -> FilterStage<'a> {
    let (prefix, body) = token
        .strip_prefix('!')
        .map_or(("", token), |body| ("!", body));
    let Some(colon) = body.find(':') else {
        return FilterStage::Field {
            prefix,
            fragment: body,
        };
    };
    let name = body.get(..colon).map_or("", |value| value);
    let Some(spec) = schema.field(name) else {
        // An unrecognised head is still a field being typed, not a value.
        return FilterStage::Field {
            prefix,
            fragment: name,
        };
    };
    let rest = body
        .get(colon.saturating_add(1)..)
        .map_or("", |value| value);
    // Only the segment after the last unquoted comma is being completed.
    let split = rest
        .rfind(',')
        .map_or(0, |position| position.saturating_add(1));
    let fragment = rest.get(split..).map_or("", |value| value);
    // A half-typed quote is punctuation, not part of the value being matched;
    // the completion re-quotes whatever it inserts.
    let fragment = fragment
        .strip_prefix('"')
        .map_or(fragment, |value| value.strip_suffix('"').unwrap_or(value));
    let committed = rest.get(..split).map_or("", |value| value);
    FilterStage::Value {
        spec,
        prefix: format!("{prefix}{}:{committed}", spec.name),
        fragment,
    }
}

fn field_sections(
    schema: &FilterSchema,
    prefix: &str,
    fragment: &str,
) -> Vec<FilterSuggestionSection> {
    let suggestion = |spec: &'static FilterFieldSpec| FilterSuggestion {
        kind: FilterSuggestionKind::Field,
        text: format!("{}:", spec.name),
        insertion: format!("{prefix}{}:", spec.name),
        note: spec.description.to_owned(),
        matches: Vec::new(),
        score: 0,
    };
    if fragment.is_empty() {
        // Opening the prompt shows the whole vocabulary, grouped as declared.
        return schema
            .groups
            .iter()
            .map(|group| FilterSuggestionSection {
                label: group.label.to_owned(),
                suggestions: group.fields.iter().map(suggestion).collect(),
            })
            .filter(|section| !section.suggestions.is_empty())
            .collect();
    }
    let suggestions = rank(schema.fields().map(suggestion), fragment);
    if suggestions.is_empty() {
        return Vec::new();
    }
    vec![FilterSuggestionSection {
        label: "Matches".to_owned(),
        suggestions,
    }]
}

/// Fuzzy-rank suggestions against `fragment`, recording the matched offsets so the
/// tray can highlight exactly the characters that earned the match.
fn rank(
    suggestions: impl Iterator<Item = FilterSuggestion>,
    fragment: &str,
) -> Vec<FilterSuggestion> {
    if fragment.is_empty() {
        return suggestions.collect();
    }
    let pattern = Pattern::new(
        fragment,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut matcher = Matcher::new(MatcherConfig::DEFAULT);
    let mut ranked = suggestions
        .enumerate()
        .filter_map(|(order, mut suggestion)| {
            let haystack = format!("{} {}", suggestion.text, suggestion.note);
            let mut characters = Vec::new();
            let haystack = Utf32Str::new(&haystack, &mut characters);
            let mut indices = Vec::new();
            let score = pattern.indices(haystack, &mut matcher, &mut indices)?;
            indices.sort_unstable();
            indices.dedup();
            let length = u32::try_from(suggestion.text.chars().count()).unwrap_or(u32::MAX);
            suggestion.matches = indices
                .into_iter()
                .filter(|index| *index < length)
                .collect();
            suggestion.score = score;
            Some((order, suggestion))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|(left_order, left), (right_order, right)| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left_order.cmp(right_order))
    });
    ranked
        .into_iter()
        .map(|(_, suggestion)| suggestion)
        .collect()
}

/// Values with whitespace only parse back when quoted.
fn quote_value(value: &str) -> String {
    if value.contains(char::is_whitespace) {
        format!("\"{value}\"")
    } else {
        value.to_owned()
    }
}

/// Whether this key is someone typing a character rather than pressing a
/// command. Shift is how a capital arrives, so it cannot be what disqualifies a
/// key; Control and Alt are what turn a letter into a command. Every text input
/// in the app asks this one question, so they all answer it the same way.
fn is_typed_text(key: KeyEvent) -> bool {
    !key.modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

fn edit_line(editor: &mut LineEditorState, key: KeyEvent) -> bool {
    match (key.code, key.modifiers) {
        (KeyCode::Char(character), _) if is_typed_text(key) => {
            let mut encoded = [0_u8; 4];
            insert_text(editor, character.encode_utf8(&mut encoded));
            true
        }
        (KeyCode::Left, modifiers) => {
            editor.cursor = if modifiers.contains(KeyModifiers::ALT) {
                previous_word_boundary(&editor.input, editor.cursor)
            } else {
                previous_scalar_boundary(&editor.input, editor.cursor)
            };
            true
        }
        (KeyCode::Right, modifiers) => {
            editor.cursor = if modifiers.contains(KeyModifiers::ALT) {
                next_word_boundary(&editor.input, editor.cursor)
            } else {
                next_scalar_boundary(&editor.input, editor.cursor)
            };
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

fn previous_word_boundary(value: &str, cursor: usize) -> usize {
    let before = &value[..cursor];
    let end = before.trim_end_matches(char::is_whitespace).len();
    value[..end]
        .char_indices()
        .rev()
        .find(|(_, character)| character.is_whitespace())
        .map_or(0, |(index, character)| {
            index.saturating_add(character.len_utf8())
        })
}

fn next_word_boundary(value: &str, cursor: usize) -> usize {
    let after = &value[cursor..];
    let word = after.trim_start_matches(char::is_whitespace);
    let word_start = value.len().saturating_sub(word.len());
    word.char_indices()
        .find(|(_, character)| character.is_whitespace())
        .map_or(value.len(), |(index, _)| word_start.saturating_add(index))
}

/// The key that copies one field. One table: the menu drew its own copy of
/// this list and the two had to be kept in step by hand.
pub const fn copy_field_key(field: CopyField) -> char {
    match field {
        CopyField::DeviceId => 'i',
        CopyField::DisplayName => 'n',
        CopyField::Hostname => 'h',
        CopyField::DnsName => 'd',
        CopyField::Owner => 'o',
        CopyField::Addresses => 'a',
        CopyField::Tags => 't',
        CopyField::PublicKey => 'p',
        CopyField::Endpoint => 'e',
        CopyField::DiagnosticSummary => 'd',
        CopyField::Metrics => 'm',
        CopyField::ServiceUrl => 'u',
        CopyField::ServiceListener => 'l',
        CopyField::ServiceBackend => 'b',
        CopyField::UserId => 'i',
        CopyField::UserName => 'n',
        CopyField::UserLogin => 'l',
        CopyField::TaskId => 'i',
        CopyField::TaskResult => 'r',
        CopyField::TaskCommand => 'c',
        CopyField::TaskOutput => 'o',
        CopyField::ProfileName => 'n',
        CopyField::ProfileTailnet => 't',
        CopyField::ProfileAccount => 'a',
        CopyField::ProfileCredential => 'c',
        CopyField::ProfileBackend => 'b',
        CopyField::ConfigSetting => 'n',
        CopyField::ConfigValue => 'v',
        CopyField::ConfigSource => 's',
    }
}

fn required_field<'a>(fields: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, String> {
    fields
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

fn expand_form_path(path: &Path) -> Result<PathBuf, String> {
    paths::expand_process_home(path).map_err(|error| error.to_string())
}

fn parse_form_backend(value: &str) -> Result<Backend, String> {
    if let Some(socket) = value.strip_prefix("unix:")
        && Path::new(socket).strip_prefix("~").is_ok()
    {
        return expand_form_path(Path::new(socket)).map(Backend::UnixSocket);
    }
    if Path::new(value).strip_prefix("~").is_ok() {
        return expand_form_path(Path::new(value)).map(Backend::FileSystemPath);
    }
    Backend::parse(value).map_err(|error| error.to_string())
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

/// The fields a mapping is made of. Funnel is not offered HTTP, so its choice
/// list simply does not contain it rather than rejecting it after the fact.
const fn reachability(exposure: &Exposure) -> &'static str {
    match exposure {
        Exposure::Public => "anyone on the internet",
        Exposure::Tailnet => "this tailnet only",
    }
}

fn mapping_fields(public: bool, existing: Option<&ServiceMapping>) -> Vec<FormField> {
    const PUBLIC_LISTENERS: &[&str] = &["https", "tcp", "tls-terminated-tcp"];
    const TAILNET_LISTENERS: &[&str] = &["https", "http", "tcp", "tls-terminated-tcp"];
    let listeners = if public {
        PUBLIC_LISTENERS
    } else {
        TAILNET_LISTENERS
    };
    vec![
        FormField::options(
            "listener",
            "Protocol",
            "How clients connect",
            listeners,
            existing.map_or("https", |mapping| mapping.listener.label()),
        ),
        FormField::text(
            "port",
            "Port",
            "The port this machine listens on",
            "443",
            existing.map_or_else(
                || "443".to_owned(),
                |mapping| mapping.listener.port().to_string(),
            ),
        ),
        FormField::text(
            "path",
            "Path",
            "The URL path to serve at; / for everything",
            "/",
            existing.map_or("/", |mapping| mapping.mount.as_path()),
        ),
        FormField::text(
            "backend",
            "Serve",
            "A local port, an http:// URL, or a folder path; ~/ is supported",
            "3000",
            existing.map_or_else(String::new, |mapping| mapping.backend.argument()),
        ),
        FormField::options(
            "proxy",
            "PROXY protocol",
            "Only used by TCP listeners; leave off unless the backend expects it",
            &["none", "1", "2"],
            existing.map_or("none", |mapping| {
                mapping.proxy_protocol.cli_value().unwrap_or("none")
            }),
        ),
    ]
}

/// One sentence saying what this request does to the machine, so the reader
/// does not have to decode an argument list to find out.
fn service_effect_sentence(request: &ServiceActionRequest) -> String {
    match request {
        ServiceActionRequest::Serve { mapping, edit } => format!(
            "{} {}:{}{} to serve {}, reachable by this tailnet.",
            if *edit { "Change" } else { "Add" },
            mapping.listener.label(),
            mapping.listener.port(),
            mapping.mount.as_path(),
            mapping.backend.argument()
        ),
        ServiceActionRequest::Funnel { mapping, edit } => format!(
            "{} {}:{}{} to serve {}, reachable by anyone on the internet.",
            if *edit { "Change" } else { "Add" },
            mapping.listener.label(),
            mapping.listener.port(),
            mapping.mount.as_path(),
            mapping.backend.argument()
        ),
        ServiceActionRequest::ServeReset => {
            "Remove every tailnet mapping on this machine.".to_owned()
        }
        ServiceActionRequest::FunnelReset => {
            "Remove every public mapping on this machine.".to_owned()
        }
        ServiceActionRequest::MappingRemove { mapping } => format!(
            "Remove {}:{}{}, and leave every other mapping in place.",
            mapping.listener.label(),
            mapping.listener.port(),
            mapping.mount.as_path()
        ),
        ServiceActionRequest::FunnelUnpublish { mapping } => format!(
            "Keep {}:{}{} serving {}, but reachable by this tailnet only.",
            mapping.listener.label(),
            mapping.listener.port(),
            mapping.mount.as_path(),
            mapping.backend.argument()
        ),
        ServiceActionRequest::TaildropSend(request) => format!(
            "Send {} file{} to {}.",
            request.files.len(),
            if request.files.len() == 1 { "" } else { "s" },
            request.target.display_name
        ),
        ServiceActionRequest::TaildropReceive(request) => {
            format!(
                "Save incoming files into {}, and {} when a name is already taken.",
                request.directory.display(),
                match request.conflict {
                    TaildropConflict::Skip => "leave the existing file alone",
                    TaildropConflict::Overwrite => "replace the existing file",
                    TaildropConflict::Rename => "give the new file another name",
                }
            )
        }
        ServiceActionRequest::TaildriveShare {
            normalized_name,
            path,
            ..
        } => format!(
            "Share {} with the tailnet as \"{normalized_name}\".",
            path.display()
        ),
        ServiceActionRequest::TaildriveRename {
            old_name,
            normalized_name,
            ..
        } => format!("Rename the share \"{old_name}\" to \"{normalized_name}\"."),
        ServiceActionRequest::TaildriveUnshare { name } => {
            format!("Stop sharing \"{name}\" with the tailnet.")
        }
        ServiceActionRequest::Certificate(request) => {
            format!("Get a certificate for {}.", request.domain)
        }
        ServiceActionRequest::Metrics => "Read this client's metrics.".to_owned(),
        ServiceActionRequest::BugReport(_) => {
            "Send a diagnostic report to Tailscale and show the identifier.".to_owned()
        }
    }
}

/// The warning worth reading twice, and the phrase that has to be typed. The
/// effect itself is stated once, under "What will happen"; anything benign has
/// no warning at all rather than a restatement of it.
fn service_confirmation_text(request: &ServiceActionRequest) -> (String, Option<String>) {
    match request {
        ServiceActionRequest::Funnel { .. } => (
            "This makes the mapping reachable from the public internet.".to_owned(),
            Some("PUBLIC".to_owned()),
        ),
        ServiceActionRequest::FunnelReset => (
            "Everything this machine serves publicly stops being reachable.".to_owned(),
            Some("RESET-PUBLIC".to_owned()),
        ),
        // Funnel is held per listener rather than per path, so a port that
        // serves several paths loses public reach on all of them at once.
        ServiceActionRequest::FunnelUnpublish { mapping } => (
            format!(
                "This mapping stays served to your tailnet but stops being reachable from the \
                 public internet. Funnel is set per listener, so everything on {}:{} stops being \
                 public.",
                mapping.listener.label(),
                mapping.listener.port()
            ),
            Some("UNPUBLISH".to_owned()),
        ),
        ServiceActionRequest::MappingRemove { mapping } if mapping.exposure == Exposure::Public => {
            (
                "This public mapping is removed. Nothing on the internet or your tailnet reaches \
                 it afterwards."
                    .to_owned(),
                Some("REMOVE-PUBLIC".to_owned()),
            )
        }
        ServiceActionRequest::MappingRemove { .. } => (
            "This mapping stops being reachable from your tailnet. Other mappings are left alone."
                .to_owned(),
            Some("REMOVE".to_owned()),
        ),
        ServiceActionRequest::ServeReset => (
            "Everything this machine serves to the tailnet stops being reachable.".to_owned(),
            Some("RESET".to_owned()),
        ),
        ServiceActionRequest::TaildropReceive(request)
            if request.conflict == TaildropConflict::Overwrite =>
        {
            (
                "Existing files of the same name are replaced and cannot be recovered.".to_owned(),
                Some("OVERWRITE".to_owned()),
            )
        }
        ServiceActionRequest::TaildriveUnshare { .. } => (
            "Anyone currently using this share loses access.".to_owned(),
            Some("UNSHARE".to_owned()),
        ),
        ServiceActionRequest::Certificate(request) if request.overwrites_existing => (
            "The existing certificate and key files are replaced.".to_owned(),
            Some("OVERWRITE".to_owned()),
        ),
        ServiceActionRequest::BugReport(_) => (
            "The report is sent to Tailscale. Only the identifier is shown here.".to_owned(),
            None,
        ),
        ServiceActionRequest::Serve { .. }
        | ServiceActionRequest::TaildropSend(_)
        | ServiceActionRequest::TaildropReceive(_)
        | ServiceActionRequest::TaildriveShare { .. }
        | ServiceActionRequest::TaildriveRename { .. }
        | ServiceActionRequest::Certificate(_)
        | ServiceActionRequest::Metrics => (String::new(), None),
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

/// The value a filter field holds when it is not narrowing anything.
const ANY: &str = "any";

/// The collections an export can be asked for, offered rather than spelled out.
const EXPORT_COLLECTIONS: &[&str] = &[
    "devices",
    "users",
    "routes",
    "dns",
    "credentials_metadata",
    "audit",
    "health_findings",
    "flow_logs",
];

/// The target kinds an audit entry can name, offered rather than spelled out.
const AUDIT_TARGET_KINDS: &[&str] = &[
    ANY,
    "device",
    "user",
    "route",
    "dns",
    "credential",
    "policy",
];

fn audit_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn audit_time(value: &str) -> Result<Option<Timestamp>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
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

fn admin_change_from_form(state: &FormState) -> Result<AdminChange, String> {
    match state.action_id {
        ActionId::AdminDeviceRename => Ok(AdminChange::DeviceRename {
            name: crate::admin::device_mutations::validate_machine_name(state.value("name"))?,
        }),
        ActionId::AdminDeviceTagsReplace => Ok(AdminChange::DeviceTags {
            tags: crate::admin::device_mutations::canonical_tags(&state.entries("tags"))?,
        }),
        ActionId::AdminDeviceApprove => Ok(AdminChange::DeviceApproval { authorized: true }),
        ActionId::AdminDeviceRevokeApproval => {
            Ok(AdminChange::DeviceApproval { authorized: false })
        }
        ActionId::AdminDeviceKeyExpiryConfigure => Ok(AdminChange::DeviceKeyExpiry {
            disabled: !state.is_yes("expiry"),
        }),
        ActionId::AdminDeviceKeyExpireNow => Ok(AdminChange::DeviceExpireNow),
        ActionId::AdminDeviceDelete => Ok(AdminChange::DeviceDelete),
        ActionId::AdminRoutesReplaceApprovals => Ok(AdminChange::DeviceRoutes {
            routes: crate::admin::route_mutations::canonical_enabled_routes(
                &state.entries("routes"),
            )?,
        }),
        ActionId::AdminDnsPreferencesEdit => Ok(AdminChange::DnsPreferences {
            magic_dns: state.is_yes("magic-dns"),
        }),
        ActionId::AdminDnsNameserversReplace => Ok(AdminChange::DnsNameservers {
            values: crate::admin::dns_mutations::canonical_resolvers(
                &state.entries("nameservers"),
                "nameserver",
            )?,
        }),
        ActionId::AdminDnsSearchPathsReplace => Ok(AdminChange::DnsSearchPaths {
            values: crate::admin::dns_mutations::canonical_ordered_values(
                &state.entries("search-paths"),
                "search path",
            )?,
        }),
        ActionId::AdminDnsSplitCreate | ActionId::AdminDnsSplitEdit => {
            let resolvers = state.entries("resolvers");
            if resolvers.is_empty() {
                return Err("a split-DNS mapping needs at least one resolver".to_owned());
            }
            Ok(AdminChange::DnsSplitMapping {
                domain: crate::admin::dns_mutations::validate_domain(state.value("domain"))?,
                resolvers: Some(crate::admin::dns_mutations::canonical_resolvers(
                    &resolvers,
                    "split-DNS resolver",
                )?),
                create: state.action_id == ActionId::AdminDnsSplitCreate,
            })
        }
        ActionId::AdminDnsSplitRemove => Ok(AdminChange::DnsSplitMapping {
            domain: crate::admin::dns_mutations::validate_domain(state.value("domain"))?,
            resolvers: None,
            create: false,
        }),
        ActionId::AdminUserApprove => Ok(AdminChange::UserApproval),
        ActionId::AdminUserRoleChange => Ok(AdminChange::UserRole {
            role: crate::admin::user_mutations::validate_role(state.value("role").trim())?,
        }),
        ActionId::AdminUserSuspend => Ok(AdminChange::UserSuspend),
        ActionId::AdminUserRestore => Ok(AdminChange::UserRestore),
        ActionId::AdminUserDelete => Ok(AdminChange::UserDelete),
        _ => Err("this form is not an admin mutation form".to_owned()),
    }
}

/// What a change asked of one form field, so a reopened form still shows it.
fn admin_change_value(change: &AdminChange, key: &str) -> Option<String> {
    match (change, key) {
        (AdminChange::DeviceRename { name }, "name") => Some(name.clone()),
        (AdminChange::DeviceTags { tags }, "tags") => Some(tags.join(",")),
        (AdminChange::DeviceKeyExpiry { disabled }, "expiry") => {
            Some(if *disabled { "no" } else { "yes" }.to_owned())
        }
        (AdminChange::DeviceRoutes { routes }, "routes") => Some(routes.join(",")),
        (AdminChange::DnsPreferences { magic_dns }, "magic-dns") => {
            Some(if *magic_dns { "yes" } else { "no" }.to_owned())
        }
        (AdminChange::DnsNameservers { values }, "nameservers")
        | (AdminChange::DnsSearchPaths { values }, "search-paths") => Some(values.join(",")),
        (AdminChange::DnsSplitMapping { domain, .. }, "domain") => Some(domain.clone()),
        (AdminChange::DnsSplitMapping { resolvers, .. }, "resolvers") => {
            resolvers.as_ref().map(|values| values.join(","))
        }
        (AdminChange::UserRole { role }, "role") => Some(role.clone()),
        _ => None,
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
        | AdminChange::DeviceDelete => device_confirmation_context(request, fields),
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
        | AdminChange::UserDelete => user_confirmation_context(request, fields),
        AdminChange::DnsNameservers { .. }
        | AdminChange::DnsPreferences { .. }
        | AdminChange::DnsSearchPaths { .. }
        | AdminChange::DnsSplitMapping { .. } => {
            vec!["configuration changes are not claimed to have reached every client".to_owned()]
        }
    }
}

fn device_confirmation_context(
    request: &AdminMutationRequest,
    fields: &AdminSnapshotFields,
) -> Vec<String> {
    let name = fields
        .values
        .get("name")
        .or_else(|| fields.values.get("hostname"))
        .filter(|value| !value.is_empty())
        .map_or(request.target_id.as_str(), String::as_str);
    let mut lines = vec![format!("Device: {name}")];
    match &request.change {
        AdminChange::DeviceTags { .. } | AdminChange::DeviceDelete => {
            push_present_field(&mut lines, fields, "owner", "Owner");
            push_present_field(&mut lines, fields, "tags", "Tags");
        }
        AdminChange::DeviceKeyExpiry { .. } | AdminChange::DeviceExpireNow => {
            if let Some(expires) = fields
                .values
                .get("expires")
                .and_then(|value| value.parse::<Timestamp>().ok())
            {
                lines.push(format!(
                    "Current key expiry: {}",
                    format_audit_timestamp(expires)
                ));
            }
        }
        AdminChange::DeviceRename { .. } | AdminChange::DeviceApproval { .. } => {}
        _ => {}
    }
    lines
}

fn user_confirmation_context(
    request: &AdminMutationRequest,
    fields: &AdminSnapshotFields,
) -> Vec<String> {
    let login = fields
        .values
        .get("loginName")
        .filter(|value| !value.is_empty())
        .map_or(request.target_id.as_str(), String::as_str);
    let mut lines = vec![format!("User: {login}")];
    if matches!(
        &request.change,
        AdminChange::UserSuspend | AdminChange::UserDelete
    ) {
        push_present_field(&mut lines, fields, "role", "Role");
        push_present_field(&mut lines, fields, "deviceCount", "Owned devices");
    }
    lines
}

fn push_present_field(
    lines: &mut Vec<String>,
    fields: &AdminSnapshotFields,
    key: &str,
    label: &str,
) {
    if let Some(value) = fields.values.get(key).filter(|value| !value.is_empty()) {
        lines.push(format!("{label}: {value}"));
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

/// The one element of an iterator, or nothing when it is empty or holds more
/// than one. Used where picking the first of several would be a guess.
fn only<'a, T>(mut candidates: impl Iterator<Item = &'a T>) -> Option<&'a T> {
    let first = candidates.next()?;
    candidates.next().is_none().then_some(first)
}

/// Whether a Taildrop target names this device. The two come from different
/// commands — `file cp --targets` reports an address and the name it knows,
/// `status` reports the device — so they are compared on the names both carry.
fn taildrop_target_names_device(target: &TaildropTarget, device: &Device) -> bool {
    [target.command_target.as_str(), target.display_name.as_str()]
        .into_iter()
        .chain(target.device_name.as_deref())
        .any(|name| {
            device_name_matches(name, &device.display_name)
                || device_name_matches(name, &device.hostname)
        })
}

/// `status` reports the short name where `file cp --targets` may report the
/// full MagicDNS name. Addresses are never shortened: `100.64.0.2` and
/// `100.64.0.3` would otherwise share a first label.
fn device_name_matches(left: &str, right: &str) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if left.eq_ignore_ascii_case(right) {
        return true;
    }
    if left.parse::<std::net::IpAddr>().is_ok() || right.parse::<std::net::IpAddr>().is_ok() {
        return false;
    }
    let short = |value: &str| {
        value
            .split('.')
            .next()
            .unwrap_or(value)
            .to_ascii_lowercase()
    };
    short(left) == short(right)
}

/// The value a preference field holds when the form is not changing it.
const UNCHANGED: &str = "unchanged";

/// A yes/no preference, offered as a third state so leaving a field alone and
/// setting it to its current value stay different answers.
fn preference_choice(
    key: &'static str,
    label: &'static str,
    help: &'static str,
    preference: &crate::domain::preference::ObservedPreference<bool>,
) -> FormField {
    let current = preference.value.map_or_else(
        || "not reported".to_owned(),
        |value| if value { "yes" } else { "no" }.to_owned(),
    );
    let field = FormField::choice(
        key,
        label,
        help,
        [
            FormChoice::new(UNCHANGED, format!("{UNCHANGED} ({current})")),
            FormChoice::plain("yes"),
            FormChoice::plain("no"),
        ],
        UNCHANGED,
    );
    lock_unless_editable(field, preference.editability)
}

fn preference_text(
    key: &'static str,
    label: &'static str,
    help: &'static str,
    hint: &'static str,
    preference: &crate::domain::preference::ObservedPreference<String>,
) -> FormField {
    let field = FormField::text(key, label, help, hint, String::new());
    lock_unless_editable(field, preference.editability)
}

fn lock_unless_editable(
    field: FormField,
    editability: crate::domain::preference::PreferenceEditability,
) -> FormField {
    if editability.can_edit() {
        field
    } else {
        field.locked(format!("{} here", editability.label()))
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

fn next_search_match(matches: &[usize], current: Option<usize>, backwards: bool) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    let current_position =
        current.and_then(|current| matches.iter().position(|candidate| *candidate == current));
    let position = if backwards {
        current_position.map_or(matches.len().saturating_sub(1), |position| {
            match position.checked_sub(1) {
                Some(position) => position,
                None => matches.len().saturating_sub(1),
            }
        })
    } else {
        current_position.map_or(0, |position| position.saturating_add(1) % matches.len())
    };
    matches.get(position).copied()
}

fn route_role_label(route: &crate::admin::routes::AdminRouteObservation) -> &'static str {
    if route.advertised_exit_node() {
        "exit advertisement"
    } else if !route.advertised.is_empty() {
        "subnet advertisement"
    } else if route.enabled_exit_node() {
        "exit approval"
    } else if !route.enabled.is_empty() {
        "subnet approval"
    } else if route.complete {
        "none"
    } else {
        "details incomplete"
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

#[cfg(test)]
mod admin_confirmation_context_tests {
    use super::*;
    use crate::action::Risk;

    #[test]
    fn device_approval_confirmation_keeps_only_decision_relevant_context() {
        let fields = AdminSnapshotFields::with([
            ("name".to_owned(), "vault.example.ts.net".to_owned()),
            ("owner".to_owned(), String::new()),
            ("tags".to_owned(), "tag:k8s".to_owned()),
            ("authorized".to_owned(), "true".to_owned()),
            ("connectedToControl".to_owned(), "true".to_owned()),
            ("keyExpiryDisabled".to_owned(), "true".to_owned()),
            ("expires".to_owned(), "1786791665".to_owned()),
            ("advertisedRoutes".to_owned(), String::new()),
            ("enabledRoutes".to_owned(), String::new()),
        ]);
        let request = AdminMutationRequest::new(
            1,
            "audit",
            "device-id",
            fields.clone(),
            AdminChange::DeviceApproval { authorized: false },
            ActionId::AdminDeviceRevokeApproval,
            Risk::DestructiveOrSecret,
        );

        assert_eq!(
            admin_preview_context(&request, &fields),
            vec!["Device: vault.example.ts.net"]
        );
        assert_eq!(
            crate::admin::mutation::preview_lines(&request.base_snapshot, &fields, &request.change),
            vec!["Approval: approved -> revoked"]
        );
    }

    #[test]
    fn key_expiry_confirmation_formats_the_timestamp_for_people() {
        let fields = AdminSnapshotFields::with([
            ("name".to_owned(), "vault.example.ts.net".to_owned()),
            ("expires".to_owned(), "1786791665".to_owned()),
        ]);
        let request = AdminMutationRequest::new(
            1,
            "audit",
            "device-id",
            fields.clone(),
            AdminChange::DeviceExpireNow,
            ActionId::AdminDeviceKeyExpireNow,
            Risk::DestructiveOrSecret,
        );

        let context = admin_preview_context(&request, &fields);
        assert_eq!(context.len(), 2);
        assert_eq!(context[0], "Device: vault.example.ts.net");
        assert!(context[1].starts_with("Current key expiry: 2026-"));
        assert!(!context[1].contains("1786791665"));
    }
}
