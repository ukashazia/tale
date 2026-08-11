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
    pub const UNAVAILABLE_FALLBACK: Self = Self::Overview;

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
        matches!(self, Self::Local | Self::Services | Self::Diagnostics)
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
            .any(|value| filter::fuzzy_matches(value, needle))
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

/// What `:tasks` remembers between frames. The selection itself lives in the
/// task store, beside the history it indexes into.
#[derive(Debug, Clone, Default)]
pub struct TaskViewState {
    /// Whether the side inspector shares the pane with the table. Off by
    /// default, the way devices and users are: the table is what the route is
    /// for, and a task's output is long enough to want the full width when you
    /// do ask for it.
    pub inspector: bool,
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
    pub scroll: usize,
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
    pub policy_workflow: Option<PolicyWorkflow>,
    pub policy_workflow_view: PolicyWorkflowView,
    policy_temp_file: Option<Arc<Mutex<crate::temporary::TemporaryPolicyFile>>>,
    latest_policy_temp_file: Option<Arc<Mutex<crate::temporary::TemporaryPolicyFile>>>,
    pub secret_result: Option<SecretResult>,
    next_policy_workflow_id: u64,
    next_secret_result_id: u64,
    pending_auth_key_request: Option<crate::admin::key_mutations::AuthKeyCreateRequest>,
    pending_auth_key_result: Option<u64>,
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
        let saved_views_load = crate::saved_views::SavedViewsState::load(&config.paths.state_dir);
        let (saved_views, saved_views_error) = match saved_views_load {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(format!("saved-view state is invalid: {error}"))),
        };
        let initial_route = if source_mode == SourceMode::Unavailable && admin.profile.is_none() {
            Route::UNAVAILABLE_FALLBACK
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
            policy_workflow: None,
            policy_workflow_view: PolicyWorkflowView::Actions,
            policy_temp_file: None,
            latest_policy_temp_file: None,
            secret_result: None,
            next_policy_workflow_id: 1,
            next_secret_result_id: 1,
            pending_auth_key_request: None,
            pending_auth_key_result: None,
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

    fn recompute_health(&mut self) -> Vec<Effect> {
        self.health_evaluation_generation = self.health_evaluation_generation.saturating_add(1);
        if self.admin.profile.is_none() {
            self.health.clear();
            self.health_findings.clear();
            self.views.overview.selected_id = None;
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
                        let unchanged = self
                            .policy_workflow
                            .as_ref()
                            .and_then(PolicyWorkflow::base)
                            .is_some_and(|base| base.hash() == candidate.hash());
                        if unchanged {
                            effects.extend(self.close_policy_workflow());
                            if !editor_success {
                                self.runtime_error = Some(format!(
                                    "external editor returned {}; policy was unchanged",
                                    editor_code.map_or_else(
                                        || "signal".to_owned(),
                                        |value| value.to_string()
                                    )
                                ));
                            }
                            return effects;
                        }
                        if let Some(workflow) = self.policy_workflow.as_mut() {
                            workflow.set_candidate(candidate, path.clone());
                        }
                        self.policy_workflow_view = PolicyWorkflowView::Actions;
                        if !editor_success {
                            self.runtime_error = Some(format!(
                                "external editor returned {}; candidate retained",
                                editor_code
                                    .map_or_else(|| "signal".to_owned(), |value| value.to_string())
                            ));
                        }
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
                self.policy_workflow_view = PolicyWorkflowView::Validation;
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
                self.policy_workflow_view = PolicyWorkflowView::Preview;
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
                self.policy_workflow_view = PolicyWorkflowView::Diff;
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
            CredentialEvent::ProfilesInspected { presences } => {
                for (profile, presence) in presences {
                    self.profile_statuses.entry(profile).or_default().presence = Some(presence);
                }
            }
            CredentialEvent::ProfileProbed { profile, result } => {
                return self.finish_profile_probe(&profile, result);
            }
            CredentialEvent::LocalRemoved {
                profile, result, ..
            } => {
                // The page reports what the store holds for every profile, not
                // only the active one, so the removal is recorded either way.
                if matches!(result, Ok(true)) {
                    let status = self.profile_statuses.entry(profile.clone()).or_default();
                    status.presence = Some(CredentialPresence::Missing);
                    status.probe = ProbeState::NotProbed;
                }
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
            CredentialEvent::ClipboardTextCopied { text, result } => match result {
                Ok(()) => self.copied_value = Some(text),
                Err(detail) => self.runtime_error = Some(detail),
            },
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

    fn update_input(&mut self, input: InputEvent) -> Vec<Effect> {
        if self.interactive_handoff_active {
            return Vec::new();
        }
        match input {
            InputEvent::Resize { width, height } => {
                self.set_terminal_size(width, height);
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
                let mut x = layout.footer.x;
                for hint in self.footer_actions(layout.footer.width) {
                    let end = x.saturating_add(
                        u16::try_from(hint.width()).map_or(u16::MAX, |value| value),
                    );
                    if mouse.column >= x && mouse.column < end {
                        return self.dispatch_action(hint.action_id);
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
        if let InteractionMode::Transient(state) = &self.interaction
            && state.kind == TransientKind::Action
        {
            let action_id = crate::ui::components::interaction_shell::action_menu_action_at(
                self,
                state,
                area,
                mouse.column,
                mouse.row,
            );
            let Some(action_id) = action_id else {
                return Vec::new();
            };
            if let Some(reason) = self.action_unavailable_reason(action_id) {
                if let InteractionMode::Transient(state) = &mut self.interaction {
                    state.message = Some(reason);
                }
                return Vec::new();
            }
            self.interaction = InteractionMode::Normal;
            return self.dispatch_action(action_id);
        }
        if let InteractionMode::FilterLine(state) = &self.interaction {
            let insertion = crate::ui::components::interaction_shell::filter_suggestion_at(
                self,
                state,
                area,
                mouse.column,
                mouse.row,
            )
            .and_then(|index| {
                state
                    .suggestions()
                    .nth(index)
                    .map(|suggestion| suggestion.insertion.clone())
            });
            if let (Some(insertion), InteractionMode::FilterLine(state)) =
                (insertion, &mut self.interaction)
            {
                let (start, end) = active_token(&state.editor.input, state.editor.cursor);
                state.editor.input.replace_range(start..end, &insertion);
                state.editor.cursor = start.saturating_add(insertion.len());
                state.selected_completion = None;
            }
            return self.update_live_filter();
        }
        let clicked_route = match &mut self.interaction {
            InteractionMode::CommandLine(state) => {
                crate::ui::components::interaction_shell::navigation_route_at(
                    state,
                    area,
                    mouse.column,
                    mouse.row,
                )
            }
            InteractionMode::Transient(state) => {
                if !matches!(state.kind, TransientKind::Copy | TransientKind::Choice) {
                    return Vec::new();
                }
                let key = if state.kind == TransientKind::Choice {
                    crate::ui::components::interaction_shell::choice_menu_key_at(
                        state,
                        area,
                        mouse.column,
                        mouse.row,
                    )
                } else {
                    crate::ui::components::interaction_shell::copy_menu_field_at(
                        state,
                        area,
                        mouse.column,
                        mouse.row,
                    )
                };
                if let Some(key) = key {
                    return self.handle_transient_key(KeyEvent::new(
                        KeyCode::Char(key),
                        KeyModifiers::NONE,
                    ));
                }
                None
            }
            InteractionMode::Normal
            | InteractionMode::HelpSheet
            | InteractionMode::FilterLine(_) => None,
        };
        if let Some(route) = clicked_route {
            return self.open_navigation_route(route);
        }
        Vec::new()
    }

    /// Which keys are live right now. This is the single answer: key dispatch,
    /// the footer, and contextual help all read it, so they cannot disagree.
    pub fn action_context(&self) -> ActionContext {
        match self.current_route() {
            Route::Local if self.views.local.section == LocalSection::Accounts => {
                ActionContext::Collection
            }
            Route::Local | Route::Dns | Route::Access | Route::Diagnostics => ActionContext::Detail,
            Route::Audit if self.focus != Focus::Inspector => ActionContext::Audit,
            Route::Overview
            | Route::Devices
            | Route::Services
            | Route::Tasks
            | Route::Profiles
            | Route::Users
            | Route::Routes
            | Route::Credentials
            | Route::Audit
                if matches!(self.focus, Focus::Inspector) =>
            {
                ActionContext::Detail
            }
            Route::Overview
            | Route::Devices
            | Route::Users
            | Route::Routes
            | Route::Credentials
            | Route::Profiles
            | Route::Config
            | Route::Services
            | Route::Tasks => ActionContext::Collection,
            _ => ActionContext::Root,
        }
    }

    fn focus_mouse_region(&mut self, column: u16, row: u16) {
        if self.current_route() == Route::Overview {
            self.focus_overview_mouse_region(column, row);
            return;
        }
        if self.current_route() == Route::Audit {
            let frame = crate::ui::layout::compute(
                ratatui::layout::Rect {
                    x: 0,
                    y: 0,
                    width: self.terminal_width,
                    height: self.terminal_height,
                },
                self,
            );
            if frame
                .inspector
                .is_some_and(|inspector| contains_point(inspector, column, row))
            {
                self.focus = Focus::Inspector;
                return;
            }
            self.focus = Focus::Collection;
            let Some(collection) = self.audit_event_area() else {
                return;
            };
            if !contains_point(collection, column, row) {
                return;
            }
            let first_row = collection.y.saturating_add(2);
            let row_count = usize::from(collection.height.saturating_sub(3));
            if row >= first_row && usize::from(row.saturating_sub(first_row)) < row_count {
                let position = usize::from(row.saturating_sub(first_row));
                if position < self.audit_event_count() {
                    self.admin_activity_selected = position;
                }
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
            if matches!(
                self.current_route(),
                Route::Routes | Route::Credentials | Route::Services | Route::Tasks
            ) && frame
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
                Route::Users | Route::Profiles => area.y.saturating_add(1),
                // A border and a heading row sit above the first task.
                Route::Routes | Route::Credentials | Route::Tasks | Route::Config => {
                    area.y.saturating_add(2)
                }
                Route::Local if self.views.local.section == LocalSection::Accounts => {
                    area.y.saturating_add(3)
                }
                Route::Services => area.y.saturating_add(3),
                _ => return,
            };
            if row < first_row {
                return;
            }
            let position = row.saturating_sub(first_row);
            match self.current_route() {
                Route::Users => {
                    let length = self.filtered_admin_users().len();
                    if usize::from(position) < length {
                        self.admin_user_selected = usize::from(position);
                    }
                }
                Route::Routes => {
                    let length = self.filtered_admin_routes().len();
                    if usize::from(position) < length {
                        self.admin_route_selected = usize::from(position);
                    }
                }
                Route::Profiles => {
                    let length = self.profile_rows().len();
                    if usize::from(position) < length {
                        self.views.profiles.selected = usize::from(position);
                    }
                }
                Route::Config => {
                    let length = self.config_rows().len();
                    if usize::from(position) < length {
                        self.views.config.selected = usize::from(position);
                    }
                }
                Route::Credentials => {
                    let length = self.filtered_admin_credentials().len();
                    if usize::from(position) < length {
                        self.admin_credential_selected = usize::from(position);
                    }
                }
                Route::Local if usize::from(position) < self.local_accounts.len() => {
                    self.views.local.selected = usize::from(position);
                    self.views.local.scroll = self.views.local.selected;
                }
                Route::Services if usize::from(position) < self.service_row_count() => {
                    self.views.services.selected = usize::from(position);
                }
                // The table shows a window over the history, so the row under
                // the pointer is an offset from wherever that window starts.
                Route::Tasks => {
                    let count = self.filtered_task_count();
                    let visible = usize::from(area.height.saturating_sub(3)).max(1);
                    let start =
                        crate::ui::views::tasks::window_start(self.task_cursor(), count, visible);
                    let index = start.saturating_add(usize::from(position));
                    if index < count {
                        self.tasks
                            .select_filtered_position(&self.task_filter, index);
                    }
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
            self.reset_device_detail_state();
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
        if self.current_route() == Route::Overview {
            return self
                .overview_collection_area()
                .is_some_and(|area| contains_point(area, column, row));
        }
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
        if self.current_route() == Route::Audit {
            return self
                .audit_event_area()
                .is_some_and(|area| contains_point(area, column, row));
        }
        if !matches!(
            self.current_route(),
            Route::Users
                | Route::Routes
                | Route::Credentials
                | Route::Profiles
                | Route::Services
                | Route::Tasks
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

    fn focus_overview_mouse_region(&mut self, column: u16, row: u16) {
        let Some(collection) = self.overview_collection_area() else {
            return;
        };
        if collection.width < self.terminal_width
            && row >= collection.y
            && column >= collection.x.saturating_add(collection.width)
        {
            self.focus = Focus::Inspector;
            return;
        }
        self.focus = Focus::Collection;
        if !contains_point(collection, column, row) {
            return;
        }
        let first_row = collection.y.saturating_add(2);
        if row < first_row {
            return;
        }
        let viewport = usize::from(collection.height.saturating_sub(3)).max(1);
        let selected = self
            .selected_overview_finding()
            .and_then(|selected| {
                self.health_findings
                    .iter()
                    .position(|finding| finding.id == selected.id)
            })
            .map_or(0, |position| position);
        let start = selected
            .saturating_add(1)
            .saturating_sub(viewport)
            .min(self.health_findings.len().saturating_sub(1));
        let position = start.saturating_add(usize::from(row.saturating_sub(first_row)));
        if position < self.health_findings.len() {
            self.select_overview_position(position);
        }
    }

    fn overview_collection_area(&self) -> Option<ratatui::layout::Rect> {
        if self.focus == Focus::Inspector {
            return None;
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
            return None;
        }
        let source_height = if frame.content.width >= 110 { 5 } else { 6 }
            .min(frame.content.height.saturating_sub(3));
        let mut collection = ratatui::layout::Rect {
            x: frame.content.x,
            y: frame.content.y.saturating_add(source_height),
            width: frame.content.width,
            height: frame.content.height.saturating_sub(source_height),
        };
        if frame.content.width >= 110 {
            collection.width = collection.width.saturating_mul(60) / 100;
        }
        Some(collection)
    }

    /// The audit collection uses the whole pane on compact terminals. On a
    /// wide terminal it yields the right side either to delivery status or to
    /// the selected event's inspector.
    fn audit_event_area(&self) -> Option<ratatui::layout::Rect> {
        if self.focus == Focus::Inspector {
            return None;
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
            return None;
        }
        if let Some(inspector) = frame.inspector {
            return Some(ratatui::layout::Rect {
                x: frame.content.x,
                y: frame.content.y,
                width: inspector.x.saturating_sub(frame.content.x),
                height: frame.content.height,
            });
        }
        if frame.content.width < 110 {
            return Some(frame.content);
        }
        Some(ratatui::layout::Rect {
            width: frame.content.width.saturating_mul(60) / 100,
            ..frame.content
        })
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
                let detail_search = matches!(state.purpose, FilterLinePurpose::DetailSearch { .. });
                insert_text(&mut state.editor, text);
                return if detail_search {
                    self.update_detail_search_preview();
                    Vec::new()
                } else {
                    self.update_live_filter()
                };
            }
            _ => {}
        }
        let Some(overlay) = self.overlays.last_mut() else {
            return Vec::new();
        };
        match overlay {
            Overlay::Form(state) => {
                state.error = None;
                if !state.is_editing() {
                    return Vec::new();
                }
                if let Some(list) = state.list.as_mut() {
                    list.edit(|entry| entry.push_str(text));
                } else if state.selected_field().is_some_and(FormField::is_secret) {
                    if let Some(secret) = state.secret.as_mut() {
                        secret.push_str(text);
                    }
                } else if let cursor = state.cursor
                    && let Some(field) = state.selected_field_mut()
                    && field.is_text()
                {
                    field.value.insert_str(cursor, text);
                    state.cursor = cursor.saturating_add(text.len());
                }
            }
            Overlay::Confirmation(state) => {
                state.input.push_str(text);
                state.error = None;
            }
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
            // The detail pane is a state the user opened, so Esc leaves it.
            if self.focus == Focus::Inspector {
                self.focus = Focus::Collection;
            }
            return Vec::new();
        }
        let context = self.action_context();
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
            InteractionMode::HelpSheet => self.handle_help_sheet_key(key),
            InteractionMode::Normal => Vec::new(),
        }
    }

    fn handle_command_line_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Esc {
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if key.code == KeyCode::Enter {
            return self.accept_navigation();
        }
        let edited = if let InteractionMode::CommandLine(state) = &mut self.interaction {
            edit_line(&mut state.editor, key)
        } else {
            false
        };
        if edited {
            if let InteractionMode::CommandLine(state) = &mut self.interaction {
                state.error = None;
            }
            self.refresh_command_completions();
        }
        Vec::new()
    }

    fn handle_filter_line_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Esc {
            let detail_restoration = match &self.interaction {
                InteractionMode::FilterLine(FilterLineState {
                    purpose:
                        FilterLinePurpose::DetailSearch {
                            route,
                            scroll,
                            query,
                            match_line,
                        },
                    ..
                }) => Some((*route, *scroll, query.clone(), *match_line)),
                _ => None,
            };
            if let Some((route, scroll, query, match_line)) = detail_restoration {
                if route == Route::Devices {
                    self.views.devices.detail_scroll = scroll;
                    self.views.devices.detail_search = query;
                    self.views.devices.detail_search_match = match_line;
                } else {
                    self.detail_scroll = scroll;
                    self.detail_search = query;
                    self.detail_search_match = match_line;
                }
                self.interaction = InteractionMode::Normal;
                return Vec::new();
            }
            let restoration = match &self.interaction {
                InteractionMode::FilterLine(state) => Some(state.restoration.clone()),
                _ => None,
            };
            if let Some(restoration) = restoration {
                if self.current_route() == Route::Tasks {
                    self.task_filter = restoration.task_filter;
                    self.tasks.selected = restoration.task_selection;
                } else if matches!(
                    self.current_route(),
                    Route::Users | Route::Routes | Route::Credentials | Route::Audit
                ) {
                    self.set_simple_collection_filter(
                        restoration.input,
                        restoration.collection_selection,
                    );
                } else if self.current_route() == Route::Profiles {
                    self.views.profiles.filter = restoration.input;
                    self.views.profiles.selected = restoration.profile_selection;
                } else if self.current_route() == Route::Config {
                    self.views.config.filter = restoration.input;
                    self.views.config.selected = restoration.config_selection;
                } else if self.current_route() == Route::Services {
                    self.views.services.filter_draft = restoration.input;
                    self.views.services.applied_filter = restoration.expression;
                    self.views.services.selected = 0;
                    self.views.services.scroll = 0;
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
            if matches!(
                self.interaction,
                InteractionMode::FilterLine(FilterLineState {
                    purpose: FilterLinePurpose::DetailSearch { .. },
                    ..
                })
            ) {
                let valid = matches!(
                    &self.interaction,
                    InteractionMode::FilterLine(state) if state.error.is_none()
                );
                if valid {
                    self.interaction = InteractionMode::Normal;
                    self.clamp_device_detail_scroll();
                }
                return Vec::new();
            }
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
            if matches!(
                self.interaction,
                InteractionMode::FilterLine(FilterLineState {
                    purpose: FilterLinePurpose::DetailSearch { .. },
                    ..
                })
            ) {
                self.update_detail_search_preview();
                return Vec::new();
            }
            return self.update_live_filter();
        }
        Vec::new()
    }

    fn handle_transient_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Esc {
            if let InteractionMode::Transient(state) = &mut self.interaction
                && state.prefix.is_some()
            {
                state.prefix = None;
                state.message = None;
                return Vec::new();
            }
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if key.code == KeyCode::Char('?') && key.modifiers.is_empty() {
            self.interaction = InteractionMode::HelpSheet;
            return Vec::new();
        }
        let KeyCode::Char(character) = key.code else {
            return Vec::new();
        };
        if !key.modifiers.is_empty() {
            return Vec::new();
        }
        let (kind, actions, fields, addresses, choices, prefix) = match &self.interaction {
            InteractionMode::Transient(state) => (
                state.kind.clone(),
                state.actions.clone(),
                state.fields.clone(),
                state.addresses.clone(),
                state.choices.clone(),
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
            TransientKind::Choice => {
                if let Some(choice) = choices
                    .iter()
                    .find(|choice| choice.sequence == sequence)
                    .cloned()
                {
                    self.interaction = InteractionMode::Normal;
                    return self.apply_choice(choice.outcome);
                }
                if prefix.is_none()
                    && choices.iter().any(|choice| {
                        choice.sequence.chars().count() == 2
                            && choice.sequence.starts_with(character)
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
                // Inside the address level a digit picks one address.
                if prefix == Some(ADDRESS_PREFIX) {
                    if character == ADDRESS_PREFIX {
                        let effects = self.copy_text(addresses.join("\n"));
                        self.interaction = InteractionMode::Normal;
                        return effects;
                    }
                    if let Some(index) = character
                        .to_digit(10)
                        .and_then(|digit| usize::try_from(digit).ok())
                        .and_then(|digit| digit.checked_sub(1))
                        && let Some(address) = addresses.get(index)
                    {
                        let address = address.clone();
                        let effects = self.copy_text(address);
                        self.interaction = InteractionMode::Normal;
                        return effects;
                    }
                } else {
                    if character == copy_field_key(CopyField::Addresses) && addresses.len() > 1 {
                        // More than one address is a choice, not a single value.
                        if let InteractionMode::Transient(state) = &mut self.interaction {
                            state.prefix = Some(ADDRESS_PREFIX);
                            state.message = None;
                        }
                        return Vec::new();
                    }
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
        }
        if let InteractionMode::Transient(state) = &mut self.interaction {
            state.message = Some(format!("unknown key: {sequence}"));
            if state.kind == TransientKind::Action {
                state.prefix = None;
            }
        }
        Vec::new()
    }

    fn handle_help_sheet_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        self.interaction = InteractionMode::Normal;
        self.handle_key(key)
    }

    fn update_live_filter(&mut self) -> Vec<Effect> {
        let (input, cursor, anchored) = match &self.interaction {
            InteractionMode::FilterLine(state) => (
                state.editor.input.clone(),
                state.editor.cursor,
                state.selected_completion.is_some(),
            ),
            _ => return Vec::new(),
        };
        let generation = self.advance_completion_generation();
        // While `Tab` walks the tray the offered set stays anchored, so the row the
        // user is cycling through does not move underneath them.
        let sections = (!anchored).then(|| self.filter_suggestions(&input, cursor));
        if matches!(
            self.current_route(),
            Route::Users | Route::Routes | Route::Credentials | Route::Audit
        ) {
            self.set_simple_collection_filter(input, 0);
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
                state.generation = generation;
                if let Some(sections) = sections {
                    state.sections = sections;
                }
            }
            return Vec::new();
        }
        if self.current_route() == Route::Services
            && self.views.services.section != ServiceSection::Serve
        {
            self.views.services.filter_draft = input;
            self.views.services.applied_filter = FilterExpression::empty();
            self.views.services.selected = 0;
            self.views.services.scroll = 0;
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
                state.generation = generation;
                if let Some(sections) = sections {
                    state.sections = sections;
                }
            }
            return Vec::new();
        }
        if self.current_route() == Route::Profiles {
            self.views.profiles.filter = input;
            self.views.profiles.selected = 0;
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
                state.generation = generation;
                if let Some(sections) = sections {
                    state.sections = sections;
                }
            }
            return Vec::new();
        }
        if self.current_route() == Route::Config {
            self.views.config.filter = input;
            self.views.config.selected = 0;
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
                state.generation = generation;
                if let Some(sections) = sections {
                    state.sections = sections;
                }
            }
            return Vec::new();
        }
        if self.current_route() == Route::Tasks {
            self.task_filter = input;
            self.tasks.select_filtered_first(&self.task_filter);
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
                state.generation = generation;
                if let Some(sections) = sections {
                    state.sections = sections;
                }
            }
            return Vec::new();
        }
        let parsed = filter::parse(&input, &self.filter_schema());
        match parsed {
            Ok(expression) => {
                if self.current_route() == Route::Services {
                    self.views.services.filter_draft = input;
                    self.views.services.applied_filter = expression;
                    self.views.services.selected = 0;
                    self.views.services.scroll = 0;
                } else {
                    self.views.devices.filter_draft = input;
                    self.views.devices.applied_filter = expression;
                    self.reconcile_selection(None);
                }
                if let InteractionMode::FilterLine(state) = &mut self.interaction {
                    state.error = None;
                    state.generation = generation;
                    if let Some(sections) = sections {
                        state.sections = sections;
                    }
                }
            }
            Err(error) => {
                // The last valid expression stays applied, so the rows behind the
                // prompt keep showing a real result while the term is repaired.
                if let InteractionMode::FilterLine(state) = &mut self.interaction {
                    state.error = Some(FilterErrorReport {
                        message: error.to_string(),
                        expected: error.expected,
                    });
                    state.generation = generation;
                    if let Some(sections) = sections {
                        state.sections = sections;
                    }
                }
            }
        }
        Vec::new()
    }

    fn command_candidates(&self, input: &str) -> Vec<NavigationCandidate> {
        navigation_candidates(input.trim())
    }

    /// The filter vocabulary of the route the shell is currently showing.
    pub fn filter_schema(&self) -> FilterSchema {
        match self.current_route() {
            Route::Tasks => filter::tasks_schema(),
            Route::Users | Route::Routes | Route::Credentials | Route::Audit => {
                filter::collection_schema()
            }
            Route::Services => match self.views.services.section {
                ServiceSection::Serve => filter::service_schema(),
                ServiceSection::Taildrive | ServiceSection::Certificates => {
                    filter::collection_schema()
                }
            },
            Route::Diagnostics => filter::empty_schema(),
            Route::Profiles => filter::profiles_schema(),
            Route::Config => filter::config_schema(),
            _ => filter::device_schema(),
        }
    }

    fn filter_suggestions(&self, input: &str, cursor: usize) -> Vec<FilterSuggestionSection> {
        let schema = self.filter_schema();
        let (start, end) = active_token(input, cursor);
        let token = input.get(start..end).map_or("", |value| value);
        match filter_stage(token, &schema) {
            FilterStage::Field { prefix, fragment } => field_sections(&schema, prefix, fragment),
            FilterStage::Value {
                spec,
                prefix,
                fragment,
            } => self.value_sections(spec, &prefix, fragment),
        }
    }

    fn value_sections(
        &self,
        spec: &'static FilterFieldSpec,
        prefix: &str,
        fragment: &str,
    ) -> Vec<FilterSuggestionSection> {
        let values = match spec.value_kind {
            FilterValueKind::Enumeration(values) => {
                values.iter().map(|value| (*value).to_owned()).collect()
            }
            FilterValueKind::Duration => DURATION_SUGGESTIONS
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            FilterValueKind::Snapshot => self.snapshot_values(spec.field),
        };
        let mut suggestions = rank(
            values.into_iter().map(|value| {
                let text = quote_value(&value);
                FilterSuggestion {
                    kind: FilterSuggestionKind::Value,
                    insertion: format!("{prefix}{text}"),
                    note: String::new(),
                    matches: Vec::new(),
                    score: 0,
                    text,
                }
            }),
            fragment,
        );
        suggestions.truncate(SNAPSHOT_VALUE_LIMIT);
        // Values lead: `Tab` should land on one, not on a match-mode refinement.
        let mut sections = Vec::new();
        if !suggestions.is_empty() {
            sections.push(FilterSuggestionSection {
                label: format!("{} values", spec.name),
                suggestions,
            });
        }
        let operators = spec
            .operators
            .iter()
            .copied()
            // Comparisons are offered as whole operands beside the values, so
            // only the match-mode refinements belong in their own section.
            .filter(|operator| *operator == filter::FilterOperator::StartsWith)
            .map(|operator| FilterSuggestion {
                kind: FilterSuggestionKind::Operator,
                text: operator.syntax().to_owned(),
                insertion: format!("{prefix}{}", operator.syntax()),
                note: operator.description().to_owned(),
                matches: Vec::new(),
                score: 0,
            })
            .collect::<Vec<_>>();
        if fragment.is_empty() && !operators.is_empty() {
            sections.push(FilterSuggestionSection {
                label: format!("{} operators", spec.name),
                suggestions: operators,
            });
        }
        sections
    }

    /// Deduplicated, deterministically ordered values already present in the snapshot.
    fn snapshot_values(&self, field: FilterField) -> Vec<String> {
        let mut values = BTreeSet::new();
        // Mapping fields draw their suggestions from the mappings on screen,
        // not from the device list.
        if matches!(
            field,
            FilterField::Port | FilterField::Mount | FilterField::Backend
        ) {
            for mapping in self.services_snapshot.mappings() {
                let _ = match field {
                    FilterField::Port => values.insert(mapping.listener.port().to_string()),
                    FilterField::Mount => values.insert(mapping.mount.as_path().to_owned()),
                    _ => values.insert(mapping.backend.argument()),
                };
            }
            return values
                .into_iter()
                .filter(|value| !value.is_empty())
                .take(SNAPSHOT_VALUE_LIMIT)
                .collect();
        }
        for device in &self.devices_resource.snapshot {
            match field {
                FilterField::Id => {
                    let _ = values.insert(device.id.0.clone());
                }
                FilterField::Name => {
                    let _ = values.insert(device.display_name.clone());
                    let _ = values.insert(device.hostname.clone());
                }
                FilterField::Owner => {
                    if let Some(owner) = device.owner.clone() {
                        let _ = values.insert(owner);
                    }
                }
                FilterField::Os => {
                    let _ = values.insert(device.os.label().to_owned());
                }
                FilterField::Tag => values.extend(device.tags.iter().cloned()),
                FilterField::ClientVersion => {
                    if let Some(version) = device.version.clone() {
                        let _ = values.insert(version);
                    }
                }
                _ => {}
            }
        }
        values
            .into_iter()
            .filter(|value| !value.is_empty())
            .take(SNAPSHOT_VALUE_LIMIT)
            .collect()
    }

    fn refresh_command_completions(&mut self) {
        let input = match &self.interaction {
            InteractionMode::CommandLine(state) => state.editor.input.clone(),
            _ => return,
        };
        let generation = self.advance_completion_generation();
        let candidates = self.command_candidates(&input);
        if let InteractionMode::CommandLine(state) = &mut self.interaction {
            state.candidates = candidates;
            state.generation = generation;
            state.error = state
                .candidates
                .is_empty()
                .then(|| "No matching view".to_owned());
        }
    }

    fn advance_completion_generation(&mut self) -> u64 {
        self.next_completion_generation = self.next_completion_generation.saturating_add(1);
        self.next_completion_generation
    }

    /// `Tab` takes the best offer, then walks forward; `Shift+Tab` walks backward.
    /// A lone offer is accepted outright so the tray can move on to the next stage.
    fn complete_filter(&mut self, reverse: bool) {
        let InteractionMode::FilterLine(state) = &mut self.interaction else {
            return;
        };
        let count = state.suggestion_count();
        if count == 0 {
            return;
        }
        let index = match (state.selected_completion, reverse) {
            (None, false) => 0,
            (None, true) => count.saturating_sub(1),
            (Some(current), false) => current.saturating_add(1) % count,
            (Some(0), true) => count.saturating_sub(1),
            (Some(current), true) => current.saturating_sub(1),
        };
        let Some(insertion) = state
            .suggestions()
            .nth(index)
            .map(|suggestion| suggestion.insertion.clone())
        else {
            return;
        };
        let (start, end) = active_token(&state.editor.input, state.editor.cursor);
        state.editor.input.replace_range(start..end, &insertion);
        state.editor.cursor = start.saturating_add(insertion.len());
        state.selected_completion = (count > 1).then_some(index);
    }

    fn handle_text_key(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        let overlay = self.overlays.last_mut()?;
        match overlay {
            Overlay::Form(state) => {
                // Two modes, and the same rule in both: Enter acts on what is
                // selected. Browsing, that means edit this field or submit;
                // editing, it means keep the value and stop editing.
                if state.is_editing() {
                    // An open list is a form of its own: entries are selected,
                    // reordered and typed into without leaving the field.
                    //
                    // Every binding here is one a terminal actually sends under
                    // the encoding this app asks for. Ctrl+I and Tab are the
                    // same byte, so Tab adds an entry; Ctrl with an arrow is not
                    // encoded at all, so the moves are plain control characters.
                    if let Some(list) = state.list.as_mut() {
                        let control = key.modifiers.contains(KeyModifiers::CONTROL);
                        match key.code {
                            KeyCode::Enter => state.commit_edit(),
                            KeyCode::Esc => state.abandon_edit(),
                            KeyCode::Up => list.select(-1),
                            KeyCode::Down => list.select(1),
                            KeyCode::Tab => list.insert(),
                            KeyCode::Char('p') if control => list.move_entry(-1),
                            KeyCode::Char('n') if control => list.move_entry(1),
                            KeyCode::Char('x') if control => list.remove(),
                            KeyCode::Backspace => list.edit(|entry| {
                                let _ = entry.pop();
                            }),
                            KeyCode::Char(character) if is_typed_text(key) => {
                                list.edit(|entry| entry.push(character));
                            }
                            _ => return None,
                        }
                        state.error = None;
                        return Some(Vec::new());
                    }
                    match key.code {
                        KeyCode::Enter => {
                            state.commit_edit();
                            state.error = None;
                            self.refresh_form_fields();
                            return Some(Vec::new());
                        }
                        KeyCode::Esc => state.abandon_edit(),
                        KeyCode::Left if state.selected_field().is_some_and(FormField::is_text) => {
                            if let Some(field) = state.selected_field() {
                                state.cursor = if key.modifiers.contains(KeyModifiers::ALT) {
                                    previous_word_boundary(&field.value, state.cursor)
                                } else {
                                    previous_scalar_boundary(&field.value, state.cursor)
                                };
                            }
                        }
                        KeyCode::Right
                            if state.selected_field().is_some_and(FormField::is_text) =>
                        {
                            if let Some(field) = state.selected_field() {
                                state.cursor = if key.modifiers.contains(KeyModifiers::ALT) {
                                    next_word_boundary(&field.value, state.cursor)
                                } else {
                                    next_scalar_boundary(&field.value, state.cursor)
                                };
                            }
                        }
                        KeyCode::Home if state.selected_field().is_some_and(FormField::is_text) => {
                            state.cursor = 0;
                        }
                        KeyCode::End if state.selected_field().is_some_and(FormField::is_text) => {
                            if let Some(field) = state.selected_field() {
                                state.cursor = field.value.len();
                            }
                        }
                        KeyCode::Left => {
                            if let Some(field) = state.selected_field_mut() {
                                field.cycle(false);
                            }
                        }
                        KeyCode::Right => {
                            if let Some(field) = state.selected_field_mut() {
                                field.cycle(true);
                            }
                        }
                        KeyCode::Backspace => {
                            if state.selected_field().is_some_and(FormField::is_secret) {
                                if let Some(secret) = state.secret.as_mut() {
                                    secret.pop();
                                }
                            } else if let cursor = state.cursor
                                && let Some(field) = state.selected_field_mut()
                                && field.is_text()
                            {
                                let previous = previous_scalar_boundary(&field.value, cursor);
                                field.value.replace_range(previous..cursor, "");
                                state.cursor = previous;
                            }
                        }
                        KeyCode::Char(character) if is_typed_text(key) => {
                            if state.selected_field().is_some_and(FormField::is_secret) {
                                if let Some(secret) = state.secret.as_mut() {
                                    secret.push(character);
                                }
                            } else if let cursor = state.cursor
                                && let Some(field) = state.selected_field_mut()
                            {
                                if field.is_text() {
                                    field.value.insert(cursor, character);
                                    state.cursor = cursor.saturating_add(character.len_utf8());
                                } else if character == ' ' {
                                    field.cycle(true);
                                }
                            }
                        }
                        _ => return None,
                    }
                    state.error = None;
                    return Some(Vec::new());
                }
                match key.code {
                    KeyCode::Enter => {
                        if state.on_submit_row() {
                            let state = state.clone();
                            return Some(self.accept_form(state));
                        }
                        // A field something else decides says so instead of
                        // opening an editor that could not change anything.
                        if let Some(reason) = state.locked_reason() {
                            state.error = Some(reason.to_owned());
                            return Some(Vec::new());
                        }
                        state.begin_edit();
                    }
                    KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                        state.move_selection(-1);
                    }
                    KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                        state.move_selection(1);
                    }
                    _ => return None,
                }
                state.error = None;
                Some(Vec::new())
            }
            Overlay::Confirmation(state) => {
                match key.code {
                    KeyCode::Char(character) if is_typed_text(key) => {
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
            Overlay::Form(state) => {
                self.overlays.push(Overlay::Form(state));
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

    fn accept_navigation(&mut self) -> Vec<Effect> {
        let route = match &self.interaction {
            InteractionMode::CommandLine(state) => {
                state.candidates.first().map(|candidate| candidate.route)
            }
            _ => None,
        };
        let Some(route) = route else {
            if let InteractionMode::CommandLine(state) = &mut self.interaction {
                state.error = Some("No matching view".to_owned());
            }
            return Vec::new();
        };
        self.open_navigation_route(route)
    }

    fn open_navigation_route(&mut self, route: Route) -> Vec<Effect> {
        if let Some(reason) = self.route_unavailable_reason(route) {
            if let InteractionMode::CommandLine(state) = &mut self.interaction {
                state.error = Some(reason.to_owned());
            }
            return Vec::new();
        }
        self.interaction = InteractionMode::Normal;
        if self.current_route() == route {
            self.focus = Focus::Collection;
            return Vec::new();
        }
        self.navigate(route);
        Vec::new()
    }

    pub fn route_unavailable_reason(&self, route: Route) -> Option<&'static str> {
        if route.requires_admin_profile() && self.admin.profile.is_none() {
            return Some("Select an administration profile to open this view");
        }
        if route.requires_local_daemon() && !self.local_routes_available() {
            return Some("Connect to the local daemon to open this view");
        }
        if route.requires_observation_source()
            && !self.local_routes_available()
            && self.admin.profile.is_none()
        {
            return Some("Connect to the local daemon or select an administration profile");
        }
        None
    }

    fn local_routes_available(&self) -> bool {
        matches!(
            self.local_daemon_state,
            LocalDaemonState::Mock | LocalDaemonState::Connecting | LocalDaemonState::Live
        )
    }

    fn leave_unavailable_route(&mut self) {
        if self
            .route_unavailable_reason(self.current_route())
            .is_some()
        {
            self.set_route(Route::UNAVAILABLE_FALLBACK);
        }
    }

    fn accept_filter(&mut self, input: &str) -> Vec<Effect> {
        if matches!(
            self.current_route(),
            Route::Users | Route::Routes | Route::Credentials | Route::Audit
        ) {
            self.set_simple_collection_filter(input.trim().to_owned(), 0);
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if self.current_route() == Route::Services
            && self.views.services.section != ServiceSection::Serve
        {
            self.views.services.filter_draft = input.trim().to_owned();
            self.views.services.applied_filter = FilterExpression::empty();
            self.views.services.selected = 0;
            self.views.services.scroll = 0;
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if self.current_route() == Route::Profiles {
            self.views.profiles.filter = input.trim().to_owned();
            self.views.profiles.selected = 0;
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if self.current_route() == Route::Config {
            self.views.config.filter = input.trim().to_owned();
            self.views.config.selected = 0;
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        if self.current_route() == Route::Tasks {
            self.task_filter = input.trim().to_owned();
            self.tasks.select_filtered_first(&self.task_filter);
            self.interaction = InteractionMode::Normal;
            return Vec::new();
        }
        match filter::parse(input, &self.filter_schema()) {
            Ok(expression) => {
                if self.current_route() == Route::Services {
                    self.views.services.filter_draft = input.to_owned();
                    self.views.services.applied_filter = expression;
                    self.views.services.selected = 0;
                    self.views.services.scroll = 0;
                } else {
                    self.views.devices.filter_draft = input.to_owned();
                    self.views.devices.applied_filter = expression;
                    self.reconcile_selection(None);
                }
                self.interaction = InteractionMode::Normal;
            }
            Err(error) => {
                if let InteractionMode::FilterLine(state) = &mut self.interaction {
                    state.error = Some(FilterErrorReport {
                        message: error.to_string(),
                        expected: error.expected,
                    });
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
        let selection = match route {
            Route::Overview => self
                .views
                .overview
                .selected_id
                .clone()
                .map(ResourceIdentity::Opaque),
            Route::Devices => self
                .views
                .devices
                .selected_id
                .clone()
                .map(ResourceIdentity::Device),
            _ => None,
        };
        let section = (route == Route::Services).then_some(self.views.services.section);
        let local_section = (route == Route::Local).then_some(self.views.local.section);
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
            task_filter: if route == Route::Tasks {
                self.task_filter.clone()
            } else {
                String::new()
            },
            sort: self.views.devices.sort,
            section,
            local_section,
            saved_view: None,
        }
    }

    fn restore_view_frame(&mut self, frame: &ViewFrame) {
        self.detail_scroll = 0;
        self.detail_search.clear();
        self.detail_search_match = None;
        self.focus = frame.focus;
        if frame.route == Route::Overview {
            self.views.overview.selected_id = match &frame.selection {
                Some(ResourceIdentity::Opaque(id)) => Some(id.clone()),
                _ => None,
            };
            self.reconcile_overview_selection();
        }
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
        if frame.route == Route::Local {
            self.views.local.section = frame.local_section.unwrap_or(LocalSection::Client);
            self.views.local.selected = 0;
            self.views.local.scroll = 0;
        }
        if frame.route == Route::Tasks {
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
        if matches!(spec.selection_rule, action::SelectionRule::One)
            && ((self.current_route() == Route::Devices && self.selected_device().is_none())
                || (self.current_route() == Route::Tasks && self.tasks.selected.is_none())
                || (self.current_route() == Route::Audit
                    && self.selected_admin_activity().is_none())
                || (self.current_route() == Route::Users && self.selected_admin_user().is_none())
                || (self.current_route() == Route::Routes && self.selected_admin_route().is_none())
                || (self.current_route() == Route::Profiles
                    && self.selected_profile_row().is_none())
                || (self.current_route() == Route::Config && self.selected_config_row().is_none())
                || (self.current_route() == Route::Local
                    && self.views.local.section == LocalSection::Accounts
                    && self.selected_local_account().is_none()))
        {
            self.runtime_error = Some("select a resource before running this action".to_owned());
            return Vec::new();
        }
        // Recoverable interaction errors describe the last failed attempt.
        // Once another action is valid they are no longer current, and must
        // not turn a later normal shutdown into an application failure.
        self.runtime_error = None;
        match action_id {
            ActionId::AppQuit => self.handle_quit_key(),
            ActionId::ViewCommandLine => {
                let candidates = self.command_candidates("");
                let generation = self.advance_completion_generation();
                self.interaction = InteractionMode::CommandLine(CommandLineState {
                    editor: LineEditorState::new(String::new()),
                    generation,
                    candidates,
                    error: None,
                });
                Vec::new()
            }
            ActionId::ViewFilter => {
                if self.filter_schema().is_empty()
                    && !matches!(
                        self.current_route(),
                        Route::Tasks | Route::Profiles | Route::Config
                    )
                {
                    let subject = if self.current_route() == Route::Services {
                        self.views.services.section.label()
                    } else {
                        self.current_route().label()
                    };
                    self.runtime_error = Some(format!("{subject} has nothing to filter on"));
                    return Vec::new();
                }
                let input = match self.current_route() {
                    Route::Tasks => self.task_filter.clone(),
                    Route::Users => self.views.users.filter.clone(),
                    Route::Routes => self.views.routes.filter.clone(),
                    Route::Credentials => self.views.credentials.filter.clone(),
                    Route::Audit => self.views.audit.filter.clone(),
                    Route::Profiles => self.views.profiles.filter.clone(),
                    Route::Config => self.views.config.filter.clone(),
                    Route::Services => self.views.services.filter_draft.clone(),
                    _ => self.views.devices.filter_draft.clone(),
                };
                let restoration = FilterRestoration {
                    input: input.clone(),
                    expression: if self.current_route() == Route::Services {
                        self.views.services.applied_filter.clone()
                    } else {
                        self.views.devices.applied_filter.clone()
                    },
                    selection: self.views.devices.selected_id.clone(),
                    scroll: self.views.devices.scroll,
                    task_filter: self.task_filter.clone(),
                    task_selection: self.tasks.selected,
                    profile_selection: self.views.profiles.selected,
                    config_selection: self.views.config.selected,
                    collection_selection: self.current_collection_selection(),
                };
                let cursor = input.len();
                let sections = self.filter_suggestions(&input, cursor);
                let generation = self.advance_completion_generation();
                self.interaction = InteractionMode::FilterLine(FilterLineState {
                    editor: LineEditorState::new(input.clone()),
                    generation,
                    sections,
                    selected_completion: None,
                    error: None,
                    restoration,
                    purpose: FilterLinePurpose::Collection,
                });
                Vec::new()
            }
            ActionId::DetailSearch => {
                let route = self.current_route();
                let input = if route == Route::Devices {
                    self.views.devices.detail_search.clone()
                } else {
                    self.detail_search.clone()
                };
                let restoration = FilterRestoration {
                    input: self.views.devices.filter_draft.clone(),
                    expression: self.views.devices.applied_filter.clone(),
                    selection: self.views.devices.selected_id.clone(),
                    scroll: self.views.devices.scroll,
                    task_filter: self.task_filter.clone(),
                    task_selection: self.tasks.selected,
                    profile_selection: self.views.profiles.selected,
                    config_selection: self.views.config.selected,
                    collection_selection: self.current_collection_selection(),
                };
                let generation = self.advance_completion_generation();
                self.interaction = InteractionMode::FilterLine(FilterLineState {
                    editor: LineEditorState::new(input.clone()),
                    generation,
                    sections: Vec::new(),
                    selected_completion: None,
                    error: None,
                    restoration,
                    purpose: FilterLinePurpose::DetailSearch {
                        route,
                        scroll: if route == Route::Devices {
                            self.views.devices.detail_scroll
                        } else {
                            self.detail_scroll
                        },
                        query: input,
                        match_line: if route == Route::Devices {
                            self.views.devices.detail_search_match
                        } else {
                            self.detail_search_match
                        },
                    },
                });
                Vec::new()
            }
            ActionId::DeviceDetailNextMatch => {
                self.move_detail_search_match(false);
                Vec::new()
            }
            ActionId::DeviceDetailPreviousMatch => {
                self.move_detail_search_match(true);
                Vec::new()
            }
            ActionId::ViewRefresh => self.start_refresh(false),
            ActionId::ViewRefreshAll => self.start_refresh(true),
            ActionId::ViewHelp => {
                self.interaction = InteractionMode::HelpSheet;
                Vec::new()
            }
            ActionId::ViewTasks => {
                self.navigate(Route::Tasks);
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
            ActionId::ViewDiagnostics => {
                self.navigate(Route::Diagnostics);
                Vec::new()
            }
            ActionId::ProfileActivate => self.activate_selected_profile(),
            ActionId::AdminRefreshCurrent => self.start_admin_current_view_refresh(),
            ActionId::AdminRefreshAll => self.start_admin_refresh(),
            ActionId::ViewProfiles => {
                self.navigate(Route::Profiles);
                Vec::new()
            }
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
            ActionId::CollectionMoveUp => {
                if self.current_route() == Route::Devices && self.focus == Focus::Inspector {
                    self.move_device_detail_scroll(-1);
                } else if self.current_route() == Route::Access {
                    self.move_access_scroll(-1);
                } else if self.current_route() == Route::Overview {
                    self.move_overview_selection(-1);
                } else if self.current_route() == Route::Tasks {
                    self.tasks.select_next_filtered(&self.task_filter, -1);
                } else if self.current_route() == Route::Audit {
                    self.move_admin_activity_selection(-1);
                } else if self.current_route() == Route::Local {
                    self.move_local_account_selection(-1);
                } else if self.current_route() == Route::Services {
                    self.move_service_selection(-1);
                } else if self.current_route() == Route::Diagnostics {
                    self.move_diagnostics_scroll(-1);
                } else if self.current_route() == Route::Users {
                    self.move_admin_user_selection(-1);
                } else if self.current_route() == Route::Routes {
                    self.move_admin_route_selection(-1);
                } else if self.current_route() == Route::Credentials {
                    self.move_admin_credential_selection(-1);
                } else if self.current_route() == Route::Profiles {
                    self.move_profile_selection(-1);
                } else if self.current_route() == Route::Config {
                    self.move_config_selection(-1);
                } else {
                    self.move_selection(-1);
                }
                Vec::new()
            }
            ActionId::CollectionMoveDown => {
                if self.current_route() == Route::Devices && self.focus == Focus::Inspector {
                    self.move_device_detail_scroll(1);
                } else if self.current_route() == Route::Access {
                    self.move_access_scroll(1);
                } else if self.current_route() == Route::Overview {
                    self.move_overview_selection(1);
                } else if self.current_route() == Route::Tasks {
                    self.tasks.select_next_filtered(&self.task_filter, 1);
                } else if self.current_route() == Route::Audit {
                    self.move_admin_activity_selection(1);
                } else if self.current_route() == Route::Local {
                    self.move_local_account_selection(1);
                } else if self.current_route() == Route::Services {
                    self.move_service_selection(1);
                } else if self.current_route() == Route::Diagnostics {
                    self.move_diagnostics_scroll(1);
                } else if self.current_route() == Route::Users {
                    self.move_admin_user_selection(1);
                } else if self.current_route() == Route::Routes {
                    self.move_admin_route_selection(1);
                } else if self.current_route() == Route::Credentials {
                    self.move_admin_credential_selection(1);
                } else if self.current_route() == Route::Profiles {
                    self.move_profile_selection(1);
                } else if self.current_route() == Route::Config {
                    self.move_config_selection(1);
                } else {
                    self.move_selection(1);
                }
                Vec::new()
            }
            ActionId::CollectionFirst => {
                if self.current_route() == Route::Devices && self.focus == Focus::Inspector {
                    self.views.devices.detail_scroll = 0;
                } else if self.current_route() == Route::Access {
                    self.detail_scroll = 0;
                } else if self.current_route() == Route::Overview {
                    self.select_overview_position(0);
                } else if self.current_route() == Route::Tasks {
                    self.tasks.select_filtered_first(&self.task_filter);
                } else if self.current_route() == Route::Audit {
                    self.admin_activity_selected = 0;
                } else if self.current_route() == Route::Local {
                    self.views.local.selected = 0;
                    self.views.local.scroll = 0;
                } else if self.current_route() == Route::Services {
                    self.views.services.selected = 0;
                    self.views.services.scroll = 0;
                } else if self.current_route() == Route::Users {
                    self.admin_user_selected = 0;
                } else if self.current_route() == Route::Routes {
                    self.admin_route_selected = 0;
                } else if self.current_route() == Route::Credentials {
                    self.admin_credential_selected = 0;
                } else if self.current_route() == Route::Profiles {
                    self.views.profiles.selected = 0;
                } else if self.current_route() == Route::Config {
                    self.views.config.selected = 0;
                } else {
                    self.move_selection_to(0);
                }
                Vec::new()
            }
            ActionId::CollectionLast => {
                if self.current_route() == Route::Devices && self.focus == Focus::Inspector {
                    self.views.devices.detail_scroll = self.device_detail_max_scroll();
                } else if self.current_route() == Route::Access {
                    self.detail_scroll = self.access_max_scroll();
                } else if self.current_route() == Route::Overview {
                    self.select_overview_position(usize::MAX);
                } else if self.current_route() == Route::Tasks {
                    self.tasks.select_filtered_last(&self.task_filter);
                } else if self.current_route() == Route::Audit {
                    self.admin_activity_selected = self.audit_event_count().saturating_sub(1);
                } else if self.current_route() == Route::Local {
                    self.views.local.selected = self.local_accounts.len().saturating_sub(1);
                    self.views.local.scroll = self.views.local.selected;
                } else if self.current_route() == Route::Services {
                    self.views.services.selected = self.service_row_count().saturating_sub(1);
                    self.views.services.scroll = self.views.services.selected;
                } else if self.current_route() == Route::Diagnostics {
                    self.views.diagnostics.scroll = self.metrics_max_scroll();
                } else if self.current_route() == Route::Users {
                    self.admin_user_selected = self.filtered_admin_users().len().saturating_sub(1);
                } else if self.current_route() == Route::Routes {
                    self.admin_route_selected =
                        self.filtered_admin_routes().len().saturating_sub(1);
                } else if self.current_route() == Route::Credentials {
                    self.admin_credential_selected =
                        self.filtered_admin_credentials().len().saturating_sub(1);
                } else if self.current_route() == Route::Profiles {
                    self.views.profiles.selected = self.profile_rows().len().saturating_sub(1);
                } else if self.current_route() == Route::Config {
                    self.views.config.selected = self.config_rows().len().saturating_sub(1);
                } else {
                    self.move_selection_to(usize::MAX);
                }
                Vec::new()
            }
            ActionId::CollectionPageUp => {
                if self.current_route() == Route::Devices && self.focus == Focus::Inspector {
                    self.move_device_detail_scroll(-5);
                } else if self.current_route() == Route::Access {
                    self.move_access_scroll(-5);
                } else if self.current_route() == Route::Overview {
                    self.move_overview_selection(-5);
                } else if self.current_route() == Route::Tasks {
                    self.tasks.select_next_filtered(&self.task_filter, -5);
                } else if self.current_route() == Route::Audit {
                    self.move_admin_activity_selection(-5);
                } else if self.current_route() == Route::Local {
                    self.move_local_account_selection(-5);
                } else if self.current_route() == Route::Services {
                    self.move_service_selection(-5);
                } else if self.current_route() == Route::Diagnostics {
                    self.move_diagnostics_scroll(-5);
                } else if self.current_route() == Route::Users {
                    self.move_admin_user_selection(-5);
                } else if self.current_route() == Route::Routes {
                    self.move_admin_route_selection(-5);
                } else if self.current_route() == Route::Credentials {
                    self.move_admin_credential_selection(-5);
                } else if self.current_route() == Route::Profiles {
                    self.move_profile_selection(-5);
                } else if self.current_route() == Route::Config {
                    self.move_config_selection(-5);
                } else {
                    self.move_selection(-5);
                }
                Vec::new()
            }
            ActionId::CollectionPageDown => {
                if self.current_route() == Route::Devices && self.focus == Focus::Inspector {
                    self.move_device_detail_scroll(5);
                } else if self.current_route() == Route::Access {
                    self.move_access_scroll(5);
                } else if self.current_route() == Route::Overview {
                    self.move_overview_selection(5);
                } else if self.current_route() == Route::Tasks {
                    self.tasks.select_next_filtered(&self.task_filter, 5);
                } else if self.current_route() == Route::Audit {
                    self.move_admin_activity_selection(5);
                } else if self.current_route() == Route::Local {
                    self.move_local_account_selection(5);
                } else if self.current_route() == Route::Services {
                    self.move_service_selection(5);
                } else if self.current_route() == Route::Diagnostics {
                    self.move_diagnostics_scroll(5);
                } else if self.current_route() == Route::Users {
                    self.move_admin_user_selection(5);
                } else if self.current_route() == Route::Routes {
                    self.move_admin_route_selection(5);
                } else if self.current_route() == Route::Credentials {
                    self.move_admin_credential_selection(5);
                } else if self.current_route() == Route::Profiles {
                    self.move_profile_selection(5);
                } else if self.current_route() == Route::Config {
                    self.move_config_selection(5);
                } else {
                    self.move_selection(5);
                }
                Vec::new()
            }
            ActionId::CollectionBack => {
                self.focus = Focus::Collection;
                Vec::new()
            }
            ActionId::CollectionOpen => {
                self.detail_search.clear();
                self.detail_search_match = None;
                if self.current_route() == Route::Overview {
                    if self.selected_overview_finding().is_some() {
                        self.focus = Focus::Inspector;
                    }
                } else if self.current_route() == Route::Tasks {
                    // A batch mutation is a table of its own, so it keeps the
                    // full-screen overlay. Everything else opens in the pane
                    // the route already has.
                    if let Some(task_id) = self.tasks.selected {
                        if self.admin_batch_results.contains_key(&task_id) {
                            self.overlays.push(Overlay::TaskInspector(task_id));
                        } else {
                            self.focus = Focus::Inspector;
                        }
                    }
                } else if self.current_route() == Route::Users {
                    // Enter replaces the table with the detail view; there is
                    // nothing to open into when no row is selected.
                    if self.selected_admin_user().is_some() {
                        self.focus = Focus::Inspector;
                    }
                } else if self.current_route() == Route::Profiles {
                    if self.selected_profile_row().is_some() {
                        self.focus = Focus::Inspector;
                    }
                } else if (self.current_route() == Route::Routes
                    && self.selected_admin_route().is_some())
                    || (self.current_route() == Route::Credentials
                        && self.selected_credential().is_some())
                    || (self.current_route() == Route::Audit
                        && self.selected_admin_activity().is_some())
                {
                    self.focus = Focus::Inspector;
                } else if self.current_route() == Route::Services {
                    if self.service_inspector_available() {
                        self.focus = Focus::Inspector;
                    }
                } else if self.selected_device().is_some() {
                    let selected_id = self.selected_device().map(|device| device.id.0.clone());
                    if self.current_route() == Route::Devices {
                        self.reset_device_detail_state();
                    }
                    self.focus = Focus::Inspector;
                    if let Some(effect) = self.start_admin_device_enrichment(selected_id) {
                        return vec![effect];
                    }
                }
                Vec::new()
            }
            ActionId::CollectionSort => {
                self.interaction = InteractionMode::Transient(TransientMenuState {
                    kind: TransientKind::Choice,
                    title: "Sort",
                    actions: Vec::new(),
                    choices: self.sort_choices(),
                    fields: Vec::new(),
                    addresses: Vec::new(),
                    prefix: None,
                    message: None,
                });
                Vec::new()
            }
            ActionId::CollectionWideColumns => {
                if self.current_route() == Route::Devices {
                    self.views.devices.wide_columns = !self.views.devices.wide_columns;
                }
                Vec::new()
            }
            ActionId::CollectionInspect => {
                let shown = match self.current_route() {
                    Route::Devices => {
                        self.views.devices.inspector = !self.views.devices.inspector;
                        self.views.devices.inspector
                    }
                    Route::Users => {
                        self.views.users.inspector = !self.views.users.inspector;
                        self.views.users.inspector
                    }
                    Route::Tasks => {
                        self.views.tasks.inspector = !self.views.tasks.inspector;
                        self.views.tasks.inspector
                    }
                    Route::Profiles => {
                        self.views.profiles.inspector = !self.views.profiles.inspector;
                        self.views.profiles.inspector
                    }
                    Route::Routes => {
                        self.views.routes.inspector = !self.views.routes.inspector;
                        self.views.routes.inspector
                    }
                    Route::Credentials => {
                        self.views.credentials.inspector = !self.views.credentials.inspector;
                        self.views.credentials.inspector
                    }
                    Route::Audit => {
                        self.views.audit.inspector = !self.views.audit.inspector;
                        self.views.audit.inspector
                    }
                    Route::Services => {
                        self.views.services.inspector = !self.views.services.inspector;
                        self.views.services.inspector
                    }
                    _ => return Vec::new(),
                };
                // Hiding the pane cannot leave the keys pointed at it.
                if !shown {
                    self.focus = Focus::Collection;
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
                    title: "Actions",
                    actions,
                    choices: Vec::new(),
                    fields: Vec::new(),
                    addresses: Vec::new(),
                    prefix: None,
                    message: None,
                });
                Vec::new()
            }
            ActionId::ResourceCopy => {
                let fields = self.contextual_copy_fields();
                if fields.is_empty() {
                    self.runtime_error = Some("nothing here to copy".to_owned());
                    return Vec::new();
                }
                let addresses = self
                    .selected_device()
                    .map(|device| device.addresses.clone())
                    .unwrap_or_default();
                self.interaction = InteractionMode::Transient(TransientMenuState {
                    kind: TransientKind::Copy,
                    title: "Copy",
                    actions: Vec::new(),
                    choices: Vec::new(),
                    fields,
                    addresses,
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
            ActionId::LocalDnsQuery => self.open_dns_query_form(),
            ActionId::LocalWhois => self.open_whois_form(),
            ActionId::DiagnosticCopy => {
                let value = self.diagnostic_summary();
                self.copy_text(value)
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
            ActionId::LocalAccountSwitch => self.open_selected_account_confirmation(false),
            ActionId::LocalAccountLogin => self.open_login_confirmation(),
            ActionId::LocalAccountLogout => self.open_logout_confirmation(),
            ActionId::LocalAccountRemove => self.open_selected_account_confirmation(true),
            ActionId::LocalSshOpen => self.open_handoff_form(ActionId::LocalSshOpen),
            ActionId::LocalNcOpen => self.open_handoff_form(ActionId::LocalNcOpen),
            ActionId::LocalSyspolicyReload => {
                self.open_mutation_confirmation(LocalMutation::SyspolicyReload)
            }
            ActionId::SectionNext => {
                self.change_route_section(1);
                Vec::new()
            }
            ActionId::SectionPrevious => {
                self.change_route_section(-1);
                Vec::new()
            }
            ActionId::ServicesServeRefresh
            | ActionId::ServicesDriveRefresh
            | ActionId::ServicesMetricsRefresh => self.start_services_action(action_id),
            ActionId::ServicesServeCreate
            | ActionId::ServicesServeEdit
            | ActionId::ServicesServeRemove
            | ActionId::ServicesServeReset
            | ActionId::ServicesFunnelCreate
            | ActionId::ServicesFunnelEdit
            | ActionId::ServicesFunnelUnpublish
            | ActionId::ServicesFunnelReset
            | ActionId::DevicesTaildropSend
            | ActionId::DevicesTaildropReceive
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
            ActionId::AdminCredentialAuthKeyCreate => self.open_auth_key_form(),
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
            ActionId::ActivityFlowsSelectWindow => self.open_flow_window_form(),
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
            | ActionId::AdminNetworkLogsSettings => self.open_admin_operational_action(action_id),
            ActionId::SavedViewCreate
            | ActionId::SavedViewReplace
            | ActionId::SavedViewRename
            | ActionId::SavedViewDelete
            | ActionId::SavedViewApply
            | ActionId::CollectionExport
            | ActionId::AccessExplorerOpenRule => self.open_local_operational_action(action_id),
            ActionId::AccessExplorerAsk => self.open_access_explorer_form(),
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
            ActionId::ViewDns | ActionId::ViewProfiles => true,
            // Always offered: the page always has the local row to fall back to,
            // and a probe already in flight is superseded rather than refused.
            ActionId::ProfileActivate => self.selected_profile_row().is_some(),
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
                self.operational_read_available(action_id)
            }
            ActionId::AdminWebhookCreate
            | ActionId::AdminWebhookEdit
            | ActionId::AdminWebhookTest
            | ActionId::AdminWebhookRotateSecret
            | ActionId::AdminWebhookDelete
            | ActionId::AdminLogStreamReplace
            | ActionId::AdminLogStreamDelete
            | ActionId::AdminNetworkLogsSettings => self.operational_mutation_available(action_id),
            ActionId::SettingsInspectCapabilities => self.admin.profile.is_some(),
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
            | ActionId::ProfileCredentialRemove => {
                self.policy_credential_admin_available(action_id)
            }
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

    fn policy_credential_admin_available(&self, action_id: ActionId) -> bool {
        if self.source_mode == SourceMode::Mock
            && matches!(
                action_id,
                ActionId::AdminPolicyEdit
                    | ActionId::AdminPolicyEditorReopen
                    | ActionId::AdminPolicyCandidateDiscard
                    | ActionId::AdminPolicyRemoteRefresh
                    | ActionId::AdminPolicyValidate
                    | ActionId::AdminPolicyPreview
                    | ActionId::AdminPolicyDiff
                    | ActionId::AdminPolicyApply
                    | ActionId::AdminPolicyWorkflowClose
            )
        {
            return match action_id {
                ActionId::AdminPolicyEdit => self.policy_workflow.is_none(),
                ActionId::AdminPolicyApply => self
                    .policy_workflow
                    .as_ref()
                    .is_some_and(|workflow| workflow.state() == PolicyState::ReadyToApply),
                _ => self.policy_workflow.is_some(),
            };
        }
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

    fn operational_read_available(&self, action_id: ActionId) -> bool {
        if self.admin.profile.is_none() {
            return false;
        }
        if action_id == ActionId::OverviewHealthOpenResource {
            return self.selected_overview_finding().is_some();
        }
        if action_id == ActionId::OverviewHealthRunSuggestedAction {
            return self
                .selected_overview_finding()
                .is_some_and(|finding| !finding.suggested_action_ids.is_empty());
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

    fn operational_mutation_available(&self, action_id: ActionId) -> bool {
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
        if action_id == ActionId::OverviewHealthOpenResource
            && self.selected_overview_finding().is_none()
        {
            return Some("no derived health finding is selected".to_owned());
        }
        if action_id == ActionId::OverviewHealthRunSuggestedAction
            && self
                .selected_overview_finding()
                .is_some_and(|finding| finding.suggested_action_ids.is_empty())
        {
            return Some("the selected finding has no suggested action".to_owned());
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
        if is_local_service_action(action_id) && self.local_executable.is_none() {
            return Some(self.missing_executable_reason());
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
            return Some(self.missing_executable_reason());
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
            return self.route_unavailable_reason(Route::Services).is_none();
        }
        if action_id == ActionId::ViewDiagnostics {
            return self.route_unavailable_reason(Route::Diagnostics).is_none();
        }
        if is_local_service_action(action_id) && self.source_mode != SourceMode::Local {
            return false;
        }
        if is_local_operator_action(action_id) && self.source_mode != SourceMode::Local {
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
            // Removing and unpublishing both run `tailscale serve`, so they
            // survive a node that has lost Funnel: the way out of a public
            // mapping must never depend on the capability that created it.
            ActionId::ServicesServeRefresh
            | ActionId::ServicesServeCreate
            | ActionId::ServicesServeEdit
            | ActionId::ServicesServeRemove
            | ActionId::ServicesFunnelUnpublish
            | ActionId::ServicesServeReset => capabilities.serve,
            ActionId::ServicesFunnelCreate
            | ActionId::ServicesFunnelEdit
            | ActionId::ServicesFunnelReset => capabilities.funnel,
            ActionId::DevicesTaildropSend | ActionId::DevicesTaildropReceive => {
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
        match action_id {
            ActionId::LocalPreferencesEdit => self.open_preferences_form(),
            ActionId::LocalExitNodeSelect => self.open_exit_node_form(),
            ActionId::LocalRoutesEditAdvertisements => self.open_advertisement_form(),
            _ => Vec::new(),
        }
    }

    /// Every preference is shown holding what the daemon reports, so a change
    /// is a change to something visible rather than a field named from memory.
    fn open_preferences_form(&mut self) -> Vec<Effect> {
        let preferences = &self.local_preferences;
        let fields = vec![
            preference_choice(
                "accept-dns",
                "Accept DNS",
                "Use the tailnet DNS configuration on this machine",
                &preferences.accept_dns,
            ),
            preference_choice(
                "accept-routes",
                "Accept routes",
                "Use subnet routes other devices advertise",
                &preferences.accept_routes,
            ),
            preference_choice(
                "shields-up",
                "Shields up",
                "Refuse all incoming connections from the tailnet",
                &preferences.shields_up,
            ),
            preference_choice(
                "ssh",
                "Tailscale SSH",
                "Accept Tailscale SSH connections on this machine",
                &preferences.ssh,
            ),
            preference_choice(
                "auto-update",
                "Automatic updates",
                "Install client updates without being asked",
                &preferences.automatic_update,
            ),
            preference_choice(
                "update-check",
                "Update checks",
                "Check whether a newer client is available",
                &preferences.update_check,
            ),
            preference_choice(
                "report-posture",
                "Report posture",
                "Send device posture data to the tailnet",
                &preferences.report_posture,
            ),
            preference_choice(
                "webclient",
                "Web client",
                "Serve the local web interface on this machine",
                &preferences.web_client,
            ),
            preference_text(
                "hostname",
                "Hostname",
                "The name this machine reports to the tailnet",
                "unchanged",
                &preferences.hostname,
            ),
            preference_text(
                "nickname",
                "Nickname",
                "The name this machine is shown under",
                "unchanged",
                &preferences.nickname,
            ),
        ];
        self.push_form(
            ActionId::LocalPreferencesEdit,
            "Edit local preferences",
            Vec::new(),
            fields,
        );
        Vec::new()
    }

    /// The candidates are the list, so an exit node is picked by the name the
    /// rest of the screen shows rather than typed as an identifier.
    fn open_exit_node_form(&mut self) -> Vec<Effect> {
        let mut options = vec![
            FormChoice::new("none", "none"),
            FormChoice::new("auto:any", "automatic"),
        ];
        options.extend(self.exit_node_candidates().into_iter().map(|candidate| {
            let state = match candidate.online {
                Some(true) => "online",
                Some(false) => "offline",
                None => "unknown",
            };
            let latency = candidate
                .last_probe_ms
                .map_or_else(|| "not probed".to_owned(), |value| format!("{value}ms"));
            FormChoice::new(
                candidate.device_id.0.clone(),
                format!("{} · {state} · {latency}", candidate.display_name),
            )
        }));
        let selected = self
            .local_preferences
            .exit_node_id
            .value
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "none".to_owned());
        let allow_lan = self
            .local_preferences
            .exit_node_allow_lan_access
            .value
            .unwrap_or(false);
        self.push_form(
            ActionId::LocalExitNodeSelect,
            "Route traffic through an exit node",
            Vec::new(),
            vec![
                FormField::choice(
                    "target",
                    "Exit node",
                    "Which device this machine sends its traffic through",
                    options,
                    selected,
                ),
                FormField::toggle(
                    "lan",
                    "Keep LAN access",
                    "Reach the local network directly while an exit node is in use",
                    allow_lan,
                ),
            ],
        );
        Vec::new()
    }

    fn open_advertisement_form(&mut self) -> Vec<Effect> {
        let preferences = &self.local_preferences;
        let routes = preferences
            .advertised_routes
            .value
            .clone()
            .unwrap_or_default();
        let endpoints = preferences
            .relay_server_static_endpoints
            .value
            .clone()
            .unwrap_or_default();
        let port = preferences
            .relay_server_port
            .value
            .filter(|_| preferences.relay_server_port_disabled.value != Some(true))
            .map_or_else(String::new, |value| value.to_string());
        self.push_form(
            ActionId::LocalRoutesEditAdvertisements,
            "Advertise routes from this machine",
            Vec::new(),
            vec![
                FormField::list(
                    "routes",
                    "Subnet routes",
                    "The complete set of CIDRs this machine offers to the tailnet",
                    "none advertised",
                    routes,
                ),
                FormField::toggle(
                    "exit",
                    "Offer as exit node",
                    "Let other devices send their internet traffic through this machine",
                    preferences.advertised_exit_node.value.unwrap_or(false),
                ),
                FormField::toggle(
                    "connector",
                    "App connector",
                    "Route a named application's traffic through this machine",
                    preferences.app_connector.value.unwrap_or(false),
                ),
                FormField::toggle(
                    "accept-risk",
                    "Accept connector risk",
                    "Required before the app connector can be turned on",
                    false,
                ),
                FormField::text(
                    "relay-port",
                    "Relay port",
                    "The port this machine relays on; empty turns relaying off",
                    "off",
                    port,
                ),
                FormField::list(
                    "relay-endpoints",
                    "Relay endpoints",
                    "The complete set of address:port pairs the relay is reachable on",
                    "none",
                    endpoints,
                ),
            ],
        );
        Vec::new()
    }

    fn accept_preferences_form(&mut self, state: &FormState) -> Vec<Effect> {
        let mut request = PreferenceRequest::default();
        for field in &state.fields {
            if field.locked.is_some() {
                continue;
            }
            let value = field.value.trim();
            if value == UNCHANGED {
                continue;
            }
            let flag = value == "yes";
            match field.key {
                "accept-dns" => request.accept_dns = Some(flag),
                "accept-routes" => request.accept_routes = Some(flag),
                "shields-up" => request.shields_up = Some(flag),
                "ssh" => request.ssh = Some(flag),
                "auto-update" => request.automatic_update = Some(flag),
                "update-check" => request.update_check = Some(flag),
                "report-posture" => request.report_posture = Some(flag),
                "webclient" => request.web_client = Some(flag),
                "hostname" if !value.is_empty() => request.hostname = Some(value.to_owned()),
                "nickname" if !value.is_empty() => request.nickname = Some(value.to_owned()),
                _ => {}
            }
        }
        if request == PreferenceRequest::default() {
            return self.set_form_error("no preference was changed");
        }
        self.overlays.pop();
        self.open_mutation_confirmation(LocalMutation::Preferences(request))
    }

    fn accept_exit_node_form(&mut self, state: &FormState) -> Vec<Effect> {
        let target = state.value("target");
        let allow_lan_access = state.is_yes("lan");
        let selection = match target {
            "" | "none" => ExitNodeSelection::None,
            "auto:any" => ExitNodeSelection::AutoAny,
            device_id => {
                let Some(candidate) = self
                    .exit_node_candidates()
                    .into_iter()
                    .find(|candidate| candidate.device_id.0 == device_id)
                else {
                    return self.set_form_error("the chosen exit node is no longer a candidate");
                };
                let Some(target) = candidate.stable_target() else {
                    return self.set_form_error("the chosen exit node has no stable target");
                };
                ExitNodeSelection::Device {
                    device_id: candidate.device_id,
                    target,
                }
            }
        };
        if matches!(selection, ExitNodeSelection::None) && allow_lan_access {
            return self.set_form_error("LAN access cannot be enabled when no exit node is chosen");
        }
        self.overlays.pop();
        self.open_mutation_confirmation(LocalMutation::ExitNode(ExitNodeRequest {
            selection,
            allow_lan_access,
        }))
    }

    fn accept_advertisement_form(&mut self, state: &FormState) -> Vec<Effect> {
        let routes = if state.entries("routes").is_empty() {
            Vec::new()
        } else {
            match parse_route_set(state.value("routes")) {
                Ok(routes) => routes,
                Err(error) => return self.set_form_error(error.to_string()),
            }
        };
        let endpoints = if state.entries("relay-endpoints").is_empty() {
            Vec::new()
        } else {
            match parse_static_endpoints(state.value("relay-endpoints")) {
                Ok(endpoints) => endpoints,
                Err(error) => return self.set_form_error(error.to_string()),
            }
        };
        let port = state.value("relay-port").trim();
        let relay_server_port = if port.is_empty() {
            None
        } else {
            match port.parse::<u16>() {
                Ok(value) => Some(value),
                Err(_) => {
                    return self.set_form_error("relay port must be empty, 0, or 1-65535");
                }
            }
        };
        let request = AdvertisementRequest {
            routes: Some(routes),
            advertise_exit_node: Some(state.is_yes("exit")),
            advertise_connector: Some(state.is_yes("connector")),
            relay_server_port: Some(relay_server_port),
            relay_server_static_endpoints: Some(endpoints),
            accept_mac_app_connector_risk: state.is_yes("accept-risk"),
        };
        if request.advertise_connector == Some(true) && !request.accept_mac_app_connector_risk {
            return self.set_form_error("turning on the app connector requires accepting its risk");
        }
        if request.accept_mac_app_connector_risk && request.advertise_connector != Some(true) {
            return self.set_form_error("accepting the connector risk requires turning it on");
        }
        self.overlays.pop();
        self.open_mutation_confirmation(LocalMutation::Advertisements(request))
    }

    fn open_admin_form(&mut self, action_id: ActionId) -> Vec<Effect> {
        match action_id {
            ActionId::AdminWebhookCreate | ActionId::AdminWebhookEdit => {
                self.open_webhook_form(action_id)
            }
            ActionId::AdminLogStreamReplace => self.open_log_stream_form(),
            ActionId::AdminNetworkLogsSettings => self.open_network_log_form(),
            _ => {
                let Some(shape) = self.admin_form_shape(action_id) else {
                    self.runtime_error = Some("this action has no admin form".to_owned());
                    return Vec::new();
                };
                self.push_form(action_id, shape.title, shape.subject, shape.fields);
                Vec::new()
            }
        }
    }

    /// Every admin form states the resource it acts on and then asks only for
    /// the values that change, each one seeded with what the tailnet reports.
    fn admin_form_shape(&self, action_id: ActionId) -> Option<FormShape> {
        match action_id {
            ActionId::AdminDeviceRename => {
                let device = self.selected_admin_device();
                let current =
                    device.map_or_else(String::new, |device| device.display_name().to_owned());
                Some(FormShape::new(
                    "Rename a device",
                    self.admin_device_subject(),
                    vec![FormField::text(
                        "name",
                        "Machine name",
                        "The name this device is known by across the tailnet",
                        "machine name",
                        current,
                    )],
                ))
            }
            ActionId::AdminDeviceTagsReplace => {
                let tags = self
                    .selected_admin_device()
                    .map(|device| device.tags.clone())
                    .unwrap_or_default();
                Some(FormShape::new(
                    "Replace device tags",
                    self.admin_device_subject(),
                    vec![FormField::list(
                        "tags",
                        "Tags",
                        "The complete tag set for this device; an empty list clears them",
                        "no tags",
                        tags,
                    )],
                ))
            }
            ActionId::AdminDeviceKeyExpiryConfigure => {
                let disabled = self
                    .selected_admin_device()
                    .and_then(|device| device.key_expiry_disabled)
                    .unwrap_or(false);
                Some(FormShape::new(
                    "Configure key expiry",
                    self.admin_device_subject(),
                    vec![FormField::toggle(
                        "expiry",
                        "Key expires",
                        "Turning this off keeps the device key valid indefinitely",
                        !disabled,
                    )],
                ))
            }
            ActionId::AdminRoutesReplaceApprovals => {
                let route = self.selected_admin_route().or_else(|| {
                    self.admin
                        .route_observations()
                        .into_iter()
                        .find(|route| route.complete)
                });
                let (subject, enabled) = route.map_or_else(
                    || (Vec::new(), Vec::new()),
                    |route| {
                        (
                            vec![("advertiser", route.device_id.clone())],
                            route.enabled.clone(),
                        )
                    },
                );
                Some(FormShape::new(
                    "Replace approved routes",
                    subject,
                    vec![FormField::list(
                        "routes",
                        "Approved",
                        "The complete set of approved CIDRs; an empty list approves none",
                        "none approved",
                        enabled,
                    )],
                ))
            }
            ActionId::AdminDnsPreferencesEdit => {
                let magic_dns = self
                    .admin
                    .dns_preferences
                    .snapshot
                    .as_ref()
                    .and_then(|value| value.magic_dns)
                    .unwrap_or(false);
                Some(FormShape::new(
                    "Edit tailnet DNS preferences",
                    Vec::new(),
                    vec![FormField::toggle(
                        "magic-dns",
                        "MagicDNS",
                        "Resolve tailnet names automatically on every device",
                        magic_dns,
                    )],
                ))
            }
            ActionId::AdminDnsNameserversReplace => {
                let values = self
                    .admin
                    .nameservers
                    .snapshot
                    .as_ref()
                    .map(|value| value.values.clone())
                    .unwrap_or_default();
                Some(FormShape::new(
                    "Replace tailnet nameservers",
                    Vec::new(),
                    vec![FormField::list(
                        "nameservers",
                        "Nameservers",
                        "The complete resolver list, asked in the order shown",
                        "none",
                        values,
                    )],
                ))
            }
            ActionId::AdminDnsSearchPathsReplace => {
                let values = self
                    .admin
                    .search_paths
                    .snapshot
                    .as_ref()
                    .map(|value| value.values.clone())
                    .unwrap_or_default();
                Some(FormShape::new(
                    "Replace DNS search paths",
                    Vec::new(),
                    vec![FormField::list(
                        "search-paths",
                        "Search paths",
                        "The complete suffix list, tried in the order shown",
                        "none",
                        values,
                    )],
                ))
            }
            ActionId::AdminDnsSplitCreate => Some(FormShape::new(
                "Add a split-DNS mapping",
                Vec::new(),
                vec![
                    FormField::text(
                        "domain",
                        "Suffix",
                        "The domain whose queries go to their own resolvers",
                        "corp.example.com",
                        String::new(),
                    ),
                    FormField::list(
                        "resolvers",
                        "Resolvers",
                        "The resolvers for this suffix, asked in the order shown",
                        "none",
                        Vec::<String>::new(),
                    ),
                ],
            )),
            ActionId::AdminDnsSplitEdit => {
                let (domain, resolvers) = self.selected_split_dns_entry();
                Some(FormShape::new(
                    "Edit a split-DNS mapping",
                    Vec::new(),
                    vec![
                        FormField::text(
                            "domain",
                            "Suffix",
                            "The domain whose queries go to their own resolvers",
                            "corp.example.com",
                            domain,
                        ),
                        FormField::list(
                            "resolvers",
                            "Resolvers",
                            "The resolvers for this suffix, asked in the order shown",
                            "none",
                            resolvers,
                        ),
                    ],
                ))
            }
            ActionId::AdminDnsSplitRemove => {
                let (domain, _) = self.selected_split_dns_entry();
                Some(FormShape::new(
                    "Remove a split-DNS mapping",
                    Vec::new(),
                    vec![FormField::text(
                        "domain",
                        "Suffix",
                        "The mapping to remove; its queries return to the default resolvers",
                        "corp.example.com",
                        domain,
                    )],
                ))
            }
            ActionId::AdminUserRoleChange => {
                let user = self.selected_admin_user();
                let current = user
                    .and_then(|user| user.role.clone())
                    .unwrap_or_else(|| "member".to_owned());
                let subject = user.map_or_else(Vec::new, |user| {
                    vec![(
                        "user",
                        user.login_name.clone().unwrap_or_else(|| user.id.clone()),
                    )]
                });
                Some(FormShape::new(
                    "Change a user role",
                    subject,
                    vec![FormField::options(
                        "role",
                        "Role",
                        "What this user is allowed to do across the tailnet",
                        crate::admin::user_mutations::DOCUMENTED_ROLES,
                        current,
                    )],
                ))
            }
            _ => None,
        }
    }

    /// Reopens an admin form still holding what the user asked for, so a
    /// preflight conflict is answered rather than retyped.
    fn reopen_admin_form(&mut self, action_id: ActionId, change: &AdminChange, error: String) {
        let Some(mut shape) = self.admin_form_shape(action_id) else {
            return;
        };
        for field in &mut shape.fields {
            if let Some(value) = admin_change_value(change, field.key) {
                field.value = value;
            }
        }
        self.overlays.push(Overlay::Form(FormState {
            action_id,
            title: shape.title,
            subject: shape.subject,
            fields: shape.fields,
            selected: 0,
            cursor: 0,
            draft: None,
            list: None,
            secret: None,
            error: Some(error),
        }));
    }

    fn admin_device_subject(&self) -> Vec<(&'static str, String)> {
        self.selected_admin_device()
            .map_or_else(Vec::new, |device| {
                vec![(
                    "device",
                    device
                        .name
                        .clone()
                        .or_else(|| device.hostname.clone())
                        .unwrap_or_else(|| device.stable_id.clone()),
                )]
            })
    }

    fn selected_split_dns_entry(&self) -> (String, Vec<String>) {
        self.admin
            .split_dns
            .snapshot
            .as_ref()
            .and_then(|value| value.entries.first())
            .map_or_else(
                || (String::new(), Vec::new()),
                |(domain, resolvers)| (domain.clone(), resolvers.clone().unwrap_or_default()),
            )
    }

    fn selected_webhook(&self) -> Option<&WebhookEndpoint> {
        self.webhooks.first()
    }

    fn open_admin_operational_action(&mut self, action_id: ActionId) -> Vec<Effect> {
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
        let Some(finding) = self.selected_overview_finding().cloned() else {
            self.runtime_error = Some("no derived health finding is available".to_owned());
            return Vec::new();
        };
        if action_id == ActionId::OverviewHealthOpenResource {
            return self.open_health_finding_resource(&finding);
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

    fn open_health_finding_resource(&mut self, finding: &Finding) -> Vec<Effect> {
        let Some(affected_id) = finding.affected_resource_ids.first().cloned() else {
            self.runtime_error = Some("the selected finding names no affected resource".to_owned());
            return Vec::new();
        };
        match finding.rule_id.as_str() {
            "device-key-expired"
            | "device-key-expiring"
            | "device-approval-pending"
            | "posture-observation-missing"
            | "relay-heavy-local-peer" => {
                let node_id = self
                    .admin
                    .devices
                    .snapshot
                    .as_ref()
                    .and_then(|devices| {
                        devices
                            .iter()
                            .find(|device| device.stable_id == affected_id)
                    })
                    .and_then(AdminDevice::exact_node_id);
                let selected = self
                    .devices_resource
                    .snapshot
                    .iter()
                    .find(|device| {
                        device.id.0 == affected_id
                            || node_id.is_some_and(|node_id| device.id.0 == node_id)
                    })
                    .map(|device| device.id.clone());
                let Some(selected) = selected else {
                    self.runtime_error = Some(
                        "the affected device is no longer in the current device snapshot"
                            .to_owned(),
                    );
                    return Vec::new();
                };
                self.views.devices.filter_draft.clear();
                self.views.devices.applied_filter = FilterExpression::empty();
                self.navigate(Route::Devices);
                self.views.devices.selected_id = Some(selected);
                self.reconcile_selection(None);
                self.reset_device_detail_state();
                self.focus = Focus::Inspector;
                return self
                    .start_admin_device_enrichment(Some(affected_id))
                    .into_iter()
                    .collect();
            }
            "user-approval-pending" => {
                self.views.users.filter.clear();
                let selected = self
                    .admin
                    .users
                    .snapshot
                    .as_ref()
                    .and_then(|users| users.iter().position(|user| user.id == affected_id));
                self.navigate(Route::Users);
                if let Some(selected) = selected {
                    self.admin_user_selected = selected;
                    self.focus = Focus::Inspector;
                } else {
                    self.runtime_error =
                        Some("the affected user is no longer in the current snapshot".to_owned());
                }
            }
            "route-overlap-review" => {
                self.views.routes.filter.clear();
                let selected = self.admin.route_observations().iter().position(|route| {
                    route
                        .advertised
                        .iter()
                        .any(|cidr| format!("{}:{cidr}", route.device_id) == affected_id)
                });
                self.navigate(Route::Routes);
                if let Some(selected) = selected {
                    self.admin_route_selected = selected;
                } else {
                    self.runtime_error =
                        Some("the affected route is no longer in the current snapshot".to_owned());
                }
            }
            _ => {
                self.runtime_error = Some(
                    "the selected finding has evidence but no resource route to open".to_owned(),
                );
            }
        }
        Vec::new()
    }

    fn open_local_operational_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        match action_id {
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
                Vec::new()
            }
            ActionId::SavedViewCreate | ActionId::SavedViewReplace => {
                self.open_saved_view_form(action_id)
            }
            ActionId::SavedViewRename => {
                let names = self.saved_view_names();
                let current = names.first().cloned().unwrap_or_default();
                self.push_form(
                    action_id,
                    "Rename a saved view",
                    Vec::new(),
                    vec![
                        FormField::choice(
                            "name",
                            "View",
                            "Which saved view is renamed",
                            names.into_iter().map(FormChoice::plain),
                            current,
                        ),
                        FormField::text(
                            "new",
                            "New name",
                            "What the view is called from now on",
                            "new name",
                            String::new(),
                        ),
                    ],
                );
                Vec::new()
            }
            ActionId::SavedViewDelete | ActionId::SavedViewApply => {
                let names = self.saved_view_names();
                let current = names.first().cloned().unwrap_or_default();
                let (title, help) = if action_id == ActionId::SavedViewDelete {
                    ("Delete a saved view", "The saved view to remove")
                } else {
                    ("Open a saved view", "The saved view to switch to")
                };
                self.push_form(
                    action_id,
                    title,
                    Vec::new(),
                    vec![FormField::choice(
                        "name",
                        "View",
                        help,
                        names.into_iter().map(FormChoice::plain),
                        current,
                    )],
                );
                Vec::new()
            }
            ActionId::CollectionExport => {
                self.push_form(
                    action_id,
                    "Export a collection to a file",
                    Vec::new(),
                    vec![
                        FormField::options(
                            "collection",
                            "Collection",
                            "Which set of records is written out",
                            EXPORT_COLLECTIONS,
                            "devices",
                        ),
                        FormField::options(
                            "format",
                            "Format",
                            "How the records are written",
                            &["json", "csv"],
                            "json",
                        ),
                        FormField::text(
                            "path",
                            "Path",
                            "Where the file is written; ~/ is supported and an existing file is replaced",
                            "~/export.json",
                            String::new(),
                        ),
                    ],
                );
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// A form whose later questions depend on an earlier answer rebuilds them
    /// when that answer changes, so it never asks for a field the choice made
    /// meaningless.
    fn refresh_form_fields(&mut self) {
        let Some(Overlay::Form(state)) = self.overlays.last_mut() else {
            return;
        };
        if state.action_id != ActionId::AdminLogStreamReplace {
            return;
        }
        let destination = state.value("destination").to_owned();
        let kept = state
            .fields
            .iter()
            .map(|field| (field.key, field.value.clone()))
            .collect::<BTreeMap<_, _>>();
        state.fields = log_stream_fields(&destination, &kept);
        state.selected = state.selected.min(state.fields.len());
    }

    fn open_log_stream_form(&mut self) -> Vec<Effect> {
        let configuration = self
            .log_stream_configurations
            .get(&LogType::Network)
            .or_else(|| self.log_stream_configurations.get(&LogType::Configuration));
        let (log_type, destination, url) = configuration.map_or_else(
            || ("network".to_owned(), "splunk".to_owned(), String::new()),
            |configuration| {
                (
                    configuration.log_type.wire_value().to_owned(),
                    configuration.destination.kind.clone(),
                    configuration.destination.identity.clone(),
                )
            },
        );
        let seed = BTreeMap::from([
            ("type", log_type),
            ("destination", destination.clone()),
            ("url", url),
        ]);
        self.push_form(
            ActionId::AdminLogStreamReplace,
            "Replace a log stream",
            Vec::new(),
            log_stream_fields(&destination, &seed),
        );
        Vec::new()
    }

    fn open_webhook_form(&mut self, action_id: ActionId) -> Vec<Effect> {
        if action_id == ActionId::AdminWebhookEdit {
            let Some(webhook) = self.selected_webhook() else {
                self.runtime_error = Some("no observed webhook is available".to_owned());
                return Vec::new();
            };
            let subject = vec![("endpoint", webhook.endpoint_url.clone())];
            let categories = webhook.subscriptions.wire_categories();
            let events = webhook.subscriptions.wire_events();
            self.push_form(
                action_id,
                "Edit what a webhook is told about",
                subject,
                vec![
                    FormField::list(
                        "categories",
                        "Categories",
                        "Whole categories this endpoint is subscribed to",
                        "none",
                        categories,
                    ),
                    FormField::list(
                        "events",
                        "Events",
                        "Individual events on top of the categories; unknown ones are kept",
                        "none",
                        events,
                    ),
                ],
            );
            return Vec::new();
        }
        self.push_form(
            action_id,
            "Add a webhook endpoint",
            Vec::new(),
            vec![
                FormField::text(
                    "url",
                    "Endpoint",
                    "Where the tailnet posts each notification",
                    "https://host.example/path",
                    String::new(),
                ),
                FormField::options(
                    "provider",
                    "Provider",
                    "How the payload is shaped for the receiving service",
                    WEBHOOK_PROVIDERS,
                    "none",
                ),
                FormField::list(
                    "categories",
                    "Categories",
                    "Whole categories this endpoint is subscribed to",
                    "none",
                    Vec::<String>::new(),
                ),
                FormField::list(
                    "events",
                    "Events",
                    "Individual events on top of the categories",
                    "none",
                    Vec::<String>::new(),
                ),
            ],
        );
        Vec::new()
    }

    fn open_network_log_form(&mut self) -> Vec<Effect> {
        let enabled = self
            .admin
            .settings
            .snapshot
            .as_ref()
            .and_then(|settings| settings.network_flow_logging_on)
            .unwrap_or(true);
        self.push_form(
            ActionId::AdminNetworkLogsSettings,
            "Configure network flow logging",
            Vec::new(),
            vec![FormField::toggle(
                "enabled",
                "Flow logging",
                "Whether devices record and report their network flows",
                enabled,
            )],
        );
        Vec::new()
    }

    fn open_auth_key_form(&mut self) -> Vec<Effect> {
        self.push_form(
            ActionId::AdminCredentialAuthKeyCreate,
            "Create an auth key",
            Vec::new(),
            vec![
                FormField::text(
                    "description",
                    "Description",
                    "What this key is for, shown in the credential list",
                    "tale-generated",
                    "tale-generated",
                ),
                FormField::text(
                    "expiry",
                    "Valid for",
                    "Whole days before the key stops working",
                    "days",
                    "7",
                ),
                FormField::toggle(
                    "reusable",
                    "Reusable",
                    "Let the key register more than one device",
                    false,
                ),
                FormField::toggle(
                    "ephemeral",
                    "Ephemeral",
                    "Remove devices registered with this key when they go offline",
                    true,
                ),
                FormField::toggle(
                    "preauthorized",
                    "Pre-approved",
                    "Devices registered with this key need no separate approval",
                    false,
                ),
                FormField::list(
                    "tags",
                    "Tags",
                    "The tags every device registered with this key receives",
                    "no tags",
                    Vec::<String>::new(),
                ),
            ],
        );
        Vec::new()
    }

    fn saved_view_names(&self) -> Vec<String> {
        self.saved_views
            .as_ref()
            .map(|state| state.names())
            .unwrap_or_default()
    }

    /// A saved view captures the screen that is already visible. The user names
    /// it; columns, filters, and sorting are UI state, not a serialization
    /// format the form asks them to type.
    fn open_saved_view_form(&mut self, action_id: ActionId) -> Vec<Effect> {
        let route = self.current_route().label();
        let title = if action_id == ActionId::SavedViewCreate {
            "Save this view"
        } else {
            "Replace a saved view"
        };
        self.push_form(
            action_id,
            title,
            vec![("route", route.to_owned())],
            vec![FormField::text(
                "name",
                "Name",
                "What this view is called; the current columns, filter, and sort are captured",
                "view name",
                String::new(),
            )],
        );
        Vec::new()
    }

    /// The window comes first and the rest narrows it, so each filter is its
    /// own field holding the value the current view is already using.
    fn open_flow_window_form(&mut self) -> Vec<Effect> {
        let now = time::OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(self.now as i64);
        let window = crate::domain::flow::FlowWindow::previous_hour(now);
        let (start, end) = window
            .query_values()
            .unwrap_or_else(|_| (String::new(), String::new()));
        let filter = self.flow_filter.clone();
        self.push_form(
            ActionId::ActivityFlowsSelectWindow,
            "Choose which flows to read",
            Vec::new(),
            vec![
                FormField::text(
                    "start",
                    "From",
                    "Inclusive UTC start; the window is at most 24 hours and within retention",
                    "RFC3339 UTC",
                    start,
                ),
                FormField::text(
                    "end",
                    "To",
                    "Inclusive UTC end; the window is at most 24 hours and within retention",
                    "RFC3339 UTC",
                    end,
                ),
                FormField::text(
                    "reporting",
                    "Reported by",
                    "Only flows the named device reported",
                    "any device",
                    filter.reporting_node_id.unwrap_or_default(),
                ),
                FormField::text(
                    "source",
                    "From device",
                    "Only flows that started at this device",
                    "any device",
                    filter.source_node_id.unwrap_or_default(),
                ),
                FormField::text(
                    "destination",
                    "To device",
                    "Only flows that ended at this device",
                    "any device",
                    filter.destination_node_id.unwrap_or_default(),
                ),
                FormField::text(
                    "source-address",
                    "From address",
                    "Only flows that started at this address",
                    "any address",
                    filter.source_address.unwrap_or_default(),
                ),
                FormField::text(
                    "destination-address",
                    "To address",
                    "Only flows that ended at this address",
                    "any address",
                    filter.destination_address.unwrap_or_default(),
                ),
                FormField::text(
                    "protocol",
                    "Protocol",
                    "Only flows carried over this protocol",
                    "any protocol",
                    filter.protocol.unwrap_or_default(),
                ),
                FormField::options(
                    "class",
                    "Traffic",
                    "Which sort of traffic the flow carried",
                    TRAFFIC_CLASSES,
                    filter
                        .traffic_class
                        .map_or_else(|| ANY.to_owned(), |class| class.label().to_owned()),
                ),
                FormField::text(
                    "source-port",
                    "From port",
                    "Only flows that started at this port",
                    "any port",
                    filter
                        .source_port
                        .map_or_else(String::new, |value| value.to_string()),
                ),
                FormField::text(
                    "destination-port",
                    "To port",
                    "Only flows that ended at this port",
                    "any port",
                    filter
                        .destination_port
                        .map_or_else(String::new, |value| value.to_string()),
                ),
                FormField::text(
                    "min-bytes",
                    "At least",
                    "Only flows that carried at least this many bytes",
                    "any size",
                    filter
                        .minimum_bytes
                        .map_or_else(String::new, |value| value.to_string()),
                ),
            ],
        );
        Vec::new()
    }

    fn accept_flow_window_form(&mut self, state: &FormState) -> Vec<Effect> {
        let (window, mut filter) = match flow_window_from_form(state, self.now) {
            Ok(value) => value,
            Err(error) => return self.set_form_error(error),
        };
        if let Err(error) = self.resolve_flow_filter_labels(&mut filter) {
            return self.set_form_error(error);
        }
        self.overlays.pop();
        self.cancel_flow_aggregation();
        self.flow_aggregation_generation = self.flow_aggregation_generation.saturating_add(1);
        self.flow_filter = filter;
        self.flow_snapshot = None;
        self.flow_generation.begin();
        self.start_admin_resource_refresh(vec![AdminRefreshResource::FlowLogs(window)])
    }

    /// The explorer asks the server one question, so the form asks for the two
    /// ends of it and which policy to ask against.
    fn open_access_explorer_form(&mut self) -> Vec<Effect> {
        self.push_form(
            ActionId::AccessExplorerAsk,
            "Ask whether one device can reach another",
            Vec::new(),
            vec![
                FormField::text(
                    "source",
                    "From",
                    "The device, user, or tag the connection starts at",
                    "user:someone@example.com",
                    String::new(),
                ),
                FormField::text(
                    "destination",
                    "To",
                    "The device, address, or tag the connection is made to",
                    "100.64.0.1",
                    String::new(),
                ),
                FormField::text(
                    "port",
                    "Port",
                    "A port number or protocol name; empty asks about any port",
                    "any",
                    String::new(),
                ),
                FormField::options(
                    "policy",
                    "Policy",
                    "Whether the question is asked of the live policy or the candidate",
                    &["current", "candidate"],
                    "current",
                ),
            ],
        );
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

    fn open_selected_account_confirmation(&mut self, remove: bool) -> Vec<Effect> {
        let Some(account_id) = self
            .selected_local_account()
            .map(|account| account.id.clone())
        else {
            self.runtime_error = Some("select an account before running this action".to_owned());
            return Vec::new();
        };
        let mutation = if remove {
            LocalMutation::AccountRemove { account_id }
        } else {
            LocalMutation::AccountSwitch { account_id }
        };
        self.open_mutation_confirmation(mutation)
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
            return self.reopen_policy_editor();
        }
        if let Err(error) = crate::terminal::EditorCommand::from_environment() {
            self.runtime_error = Some(error.to_string());
            return Vec::new();
        }
        if self.source_mode == SourceMode::Mock {
            let Some(snapshot) = self.admin.policy.snapshot.as_ref() else {
                self.runtime_error = Some("the mock policy source is unavailable".to_owned());
                return Vec::new();
            };
            let document =
                match crate::domain::policy_workflow::PolicyDocument::from_bytes_with_content_type(
                    snapshot.source_bytes.clone(),
                    snapshot.content_type.clone(),
                    self.now,
                ) {
                    Ok(document) => document,
                    Err(error) => {
                        self.runtime_error = Some(error.to_string());
                        return Vec::new();
                    }
                };
            let file = match crate::temporary::TemporaryPolicyFile::create(document.bytes()) {
                Ok(file) => file,
                Err(error) => {
                    self.runtime_error = Some(error.to_string());
                    return Vec::new();
                }
            };
            let workflow_id = self.next_policy_workflow_id;
            self.next_policy_workflow_id = self.next_policy_workflow_id.saturating_add(1);
            let path = file.path().to_path_buf();
            self.policy_temp_file = Some(Arc::new(Mutex::new(file)));
            let mut workflow = PolicyWorkflow::opening(
                workflow_id,
                "mock".to_owned(),
                "example.test".to_owned(),
                self.now,
            );
            workflow.set_base(document.clone());
            workflow.set_candidate(document, path);
            self.policy_workflow = Some(workflow);
            return self.start_policy_editor();
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
        vec![Effect::StartPolicyRemoteFetch {
            workflow_id,
            profile,
            tailnet,
            credential,
            timeout: self.resolved_config.admin.request_timeout,
        }]
    }

    fn refresh_policy_workflow(&mut self) -> Vec<Effect> {
        self.policy_workflow_view = PolicyWorkflowView::Actions;
        if self.source_mode == SourceMode::Mock {
            let latest = self.admin.policy.snapshot.as_ref().and_then(|snapshot| {
                crate::domain::policy_workflow::PolicyDocument::from_bytes_with_content_type(
                    snapshot.source_bytes.clone(),
                    snapshot.content_type.clone(),
                    self.now,
                )
                .ok()
            });
            if let Some(latest) = latest
                && let Some(workflow) = self.policy_workflow.as_mut()
            {
                workflow.set_latest_remote(latest);
            }
            self.runtime_error = Some("mock remote policy refreshed".to_owned());
            return Vec::new();
        }
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
        if self
            .policy_workflow
            .as_ref()
            .is_some_and(|workflow| workflow.state() == PolicyState::Opening)
        {
            self.runtime_error = Some("the policy source is still loading".to_owned());
            return Vec::new();
        }
        self.start_policy_editor()
    }

    fn discard_policy_candidate(&mut self) -> Vec<Effect> {
        self.close_policy_workflow()
    }

    fn validate_policy_candidate(&mut self) -> Vec<Effect> {
        self.policy_workflow_view = PolicyWorkflowView::Validation;
        if self.source_mode == SourceMode::Mock {
            let Some(workflow) = self.policy_workflow.as_mut() else {
                return Vec::new();
            };
            let Some(candidate_hash) = workflow.candidate().map(|value| value.hash().to_owned())
            else {
                return Vec::new();
            };
            let _ = workflow.set_validation(crate::domain::policy_workflow::PolicyValidation {
                candidate_hash,
                validated_at: self.now,
                valid: true,
                message: Some("mock server validation passed".to_owned()),
                bounded_safe_detail: None,
                diagnostics: Vec::new(),
                server_tests: Vec::new(),
                observed_at: self.now,
            });
            return Vec::new();
        }
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
            timeout: self.resolved_config.admin.request_timeout,
            path,
        }]
    }

    fn preview_policy_candidate(&mut self) -> Vec<Effect> {
        self.policy_workflow_view = PolicyWorkflowView::Preview;
        let selector = self
            .selected_admin_user()
            .map_or_else(|| "autogroup:members".to_owned(), |user| user.id.clone());
        self.push_form(
            ActionId::AdminPolicyPreview,
            "Preview the policy for one selector",
            Vec::new(),
            vec![
                FormField::options(
                    "type",
                    "Selector",
                    "Whether the preview is asked for a user or an address and port",
                    &["user", "ipport"],
                    "user",
                ),
                FormField::text(
                    "for",
                    "Preview for",
                    "The user selector, or address:port, the server previews access for",
                    "autogroup:members",
                    selector,
                ),
            ],
        );
        Vec::new()
    }

    fn start_policy_preview(
        &mut self,
        selector_type: PolicySelectorType,
        selector: String,
    ) -> Vec<Effect> {
        if self.source_mode == SourceMode::Mock {
            let Some(workflow) = self.policy_workflow.as_mut() else {
                return Vec::new();
            };
            let Some(candidate_hash) = workflow.candidate().map(|value| value.hash().to_owned())
            else {
                return Vec::new();
            };
            let _ = workflow.set_preview(crate::domain::policy_workflow::PolicyPreview {
                candidate_hash,
                selector_type,
                selector,
                matches: vec![crate::domain::policy_workflow::PolicyPreviewMatch {
                    users: vec!["alice@example.test".to_owned()],
                    ports: vec!["tag:server:22".to_owned()],
                    line_number: Some(4),
                }],
                observed_at: self.now,
            });
            self.policy_workflow_view = PolicyWorkflowView::Preview;
            return Vec::new();
        }
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
            timeout: self.resolved_config.admin.request_timeout,
            path,
            selector_type,
            selector,
        }]
    }

    fn diff_policy_candidate(&mut self) -> Vec<Effect> {
        self.policy_workflow_view = PolicyWorkflowView::Diff;
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
        self.policy_workflow_view = PolicyWorkflowView::Actions;
        self.pending_auth_key_request = None;
        self.pending_credential_revoke = None;
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
        self.filtered_admin_credentials()
            .get(self.admin_credential_selected)
            .copied()
    }

    fn open_credential_revoke_confirmation(&mut self) -> Vec<Effect> {
        let Some(credential) = self.selected_credential() else {
            self.runtime_error = Some("select a credential before revoking it".to_owned());
            return Vec::new();
        };
        let credential_type = crate::admin::key_mutations::remote_credential_type(credential);
        if !credential_type.supported_for_revoke() {
            self.runtime_error = Some(
                "the selected credential type has no documented revocation contract".to_owned(),
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
                "the selected credential type has no documented revocation contract".to_owned(),
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
        let filters = &self.audit_filters;
        let (title, fields) = match action_id {
            ActionId::AuditFilterTime => (
                "Limit the audit log to a time range",
                vec![
                    FormField::text(
                        "start",
                        "From",
                        "Inclusive UTC start, as 2026-08-03T00:00:00Z; empty removes the bound",
                        "any time",
                        filters.start.map_or(String::new(), format_audit_timestamp),
                    ),
                    FormField::text(
                        "end",
                        "To",
                        "Inclusive UTC end, as 2026-08-04T00:00:00Z; empty removes the bound",
                        "any time",
                        filters.end.map_or(String::new(), format_audit_timestamp),
                    ),
                ],
            ),
            ActionId::AuditFilterActor => (
                "Limit the audit log to one actor",
                vec![
                    FormField::text(
                        "id",
                        "Actor id",
                        "The exact user or principal id recorded on the entry",
                        "any actor",
                        filters.actor_id.clone().unwrap_or_default(),
                    ),
                    FormField::text(
                        "display",
                        "Shown as",
                        "The exact display value the entry resolved to",
                        "any name",
                        filters.actor_display.clone().unwrap_or_default(),
                    ),
                ],
            ),
            ActionId::AuditFilterAction => (
                "Limit the audit log to one action",
                vec![FormField::text(
                    "action",
                    "Action",
                    "The exact action value, such as device.view",
                    "any action",
                    filters.action.clone().unwrap_or_default(),
                )],
            ),
            ActionId::AuditFilterTarget => (
                "Limit the audit log to one target",
                vec![
                    FormField::options(
                        "type",
                        "Kind",
                        "What sort of thing the entry acted on",
                        AUDIT_TARGET_KINDS,
                        filters
                            .target_type
                            .clone()
                            .unwrap_or_else(|| ANY.to_owned()),
                    ),
                    FormField::text(
                        "id",
                        "Target id",
                        "The exact stable id the entry recorded",
                        "any id",
                        filters.target_id.clone().unwrap_or_default(),
                    ),
                    FormField::text(
                        "text",
                        "Summary contains",
                        "Matches entries whose summary contains this text",
                        "anything",
                        filters.text.clone().unwrap_or_default(),
                    ),
                ],
            ),
            _ => return Vec::new(),
        };
        self.push_form(action_id, title, Vec::new(), fields);
        Vec::new()
    }

    fn accept_audit_filter(&mut self, state: &FormState) -> Vec<Effect> {
        match state.action_id {
            ActionId::AuditFilterTime => {
                let start = match audit_time(state.value("start")) {
                    Ok(value) => value,
                    Err(error) => return self.set_form_error(error),
                };
                let end = match audit_time(state.value("end")) {
                    Ok(value) => value,
                    Err(error) => return self.set_form_error(error),
                };
                if start.zip(end).is_some_and(|(start, end)| start > end) {
                    return self.set_form_error("the start must not be after the end");
                }
                self.audit_filters.start = start;
                self.audit_filters.end = end;
            }
            ActionId::AuditFilterActor => {
                self.audit_filters.actor_id = audit_text(state.value("id"));
                self.audit_filters.actor_display = audit_text(state.value("display"));
            }
            ActionId::AuditFilterAction => {
                self.audit_filters.action = audit_text(state.value("action"));
            }
            ActionId::AuditFilterTarget => {
                self.audit_filters.target_type = match state.value("type") {
                    ANY => None,
                    value => Some(value.to_owned()),
                };
                self.audit_filters.target_id = audit_text(state.value("id"));
                self.audit_filters.text = audit_text(state.value("text"));
            }
            _ => return Vec::new(),
        }
        self.admin_activity_selected = 0;
        self.overlays.pop();
        self.open_audit_investigation()
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
            self.runtime_error = Some(self.missing_executable_reason());
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
            self.runtime_error = Some(self.missing_executable_reason());
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

    /// The handoff forms name the host they act on rather than asking for it:
    /// the form is modal, so the selected row is still the row underneath it.
    fn open_handoff_form(&mut self, action_id: ActionId) -> Vec<Effect> {
        let Some(host) = self
            .selected_local_device()
            .and_then(LocalDevice::preferred_target)
            .map(str::to_owned)
        else {
            self.runtime_error = Some("selected device has no DNS name or Tailscale IP".to_owned());
            return Vec::new();
        };
        let (title, field) = if action_id == ActionId::LocalNcOpen {
            (
                "Open a netcat session",
                FormField::text(
                    "port",
                    "Port",
                    "TCP port 1-65535 on the selected host",
                    "443",
                    "443",
                ),
            )
        } else {
            (
                "Open an SSH session",
                FormField::text(
                    "user",
                    "Username",
                    "Leave empty to let the client pick the remote username",
                    "remote default",
                    String::new(),
                ),
            )
        };
        self.push_form(action_id, title, vec![("host", host)], vec![field]);
        Vec::new()
    }

    fn accept_handoff_form(&mut self, state: &FormState) -> Vec<Effect> {
        let Some(executable) = self.local_executable.as_ref() else {
            self.runtime_error = Some(self.missing_executable_reason());
            return Vec::new();
        };
        let Some(host) = self
            .selected_local_device()
            .and_then(LocalDevice::preferred_target)
        else {
            return self.set_form_error("selected device has no DNS name or Tailscale IP");
        };
        let command = if state.action_id == ActionId::LocalNcOpen {
            handoff::nc_command(&executable.path, host, state.value("port").trim())
        } else {
            let user = state.value("user").trim();
            handoff::ssh_command(&executable.path, (!user.is_empty()).then_some(user), host)
        };
        match command {
            Ok(command) => {
                let command = local_handoff_command(command, executable.socket_path.as_deref());
                let redacted_argv = redacted_argv(&command.args());
                self.overlays.pop();
                self.overlays
                    .push(Overlay::Confirmation(Box::new(ConfirmationState {
                        action_id: state.action_id,
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
            Err(error) => self.set_form_error(error.to_string()),
        }
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

    fn accept_admin_operational_form(&mut self, state: &FormState) -> Vec<Effect> {
        let result = match state.action_id {
            ActionId::AdminWebhookCreate => webhook_create_from_form(state),
            ActionId::AdminWebhookEdit => self.webhook_edit_from_form(state),
            ActionId::AdminLogStreamReplace => log_stream_from_form(state),
            ActionId::AdminNetworkLogsSettings => Ok(OperationalMutation::NetworkLogSetting {
                enabled: state.is_yes("enabled"),
            }),
            _ => Err("this is not an admin operational form".to_owned()),
        };
        match result {
            Ok(mutation) => {
                self.overlays.pop();
                self.open_operational_confirmation(state.action_id, mutation)
            }
            Err(error) => self.set_form_error(error),
        }
    }

    fn accept_auth_key_form(&mut self, state: &FormState) -> Vec<Effect> {
        let days = match state.value("expiry").trim().parse::<u64>() {
            Ok(days) => days,
            Err(_) => {
                return self.set_form_error("the key must be valid for a whole number of days");
            }
        };
        let Some(expiry_seconds) = days.checked_mul(24 * 60 * 60) else {
            return self.set_form_error("that many days is too long");
        };
        let description = state.value("description").trim();
        let request = crate::admin::key_mutations::AuthKeyCreateRequest {
            description: (!description.is_empty()).then(|| description.to_owned()),
            expiry_seconds,
            reusable: state.is_yes("reusable"),
            ephemeral: state.is_yes("ephemeral"),
            preauthorized: state.is_yes("preauthorized"),
            tags: state.entries("tags"),
        };
        if let Err(error) = request.validate() {
            return self.set_form_error(error.to_string());
        }
        self.overlays.pop();
        self.open_auth_key_form_with_request(request)
    }

    fn webhook_edit_from_form(&self, state: &FormState) -> Result<OperationalMutation, String> {
        let endpoint = self
            .selected_webhook()
            .ok_or_else(|| "no observed webhook is available".to_owned())?;
        let after = endpoint
            .subscriptions
            .edit_known(state.entries("categories"), state.entries("events"))
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

    fn accept_local_operational_form(&mut self, state: &FormState) -> Vec<Effect> {
        let result = match state.action_id {
            ActionId::SavedViewCreate | ActionId::SavedViewReplace => {
                self.saved_view_from_form(state).map(|view| {
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
                let name = required_form_value(state, "name", "a view to rename");
                let replacement = required_form_value(state, "new", "a new name");
                name.and_then(|name| {
                    replacement.map(|replacement| {
                        OperationalMutation::SavedView(SavedViewMutation::Rename {
                            name,
                            replacement,
                        })
                    })
                })
            }
            ActionId::SavedViewDelete => required_form_value(state, "name", "a view to delete")
                .map(|name| OperationalMutation::SavedView(SavedViewMutation::Delete { name })),
            ActionId::SavedViewApply => required_form_value(state, "name", "a view to open")
                .map(|name| OperationalMutation::SavedView(SavedViewMutation::Apply { name })),
            ActionId::CollectionExport => export_from_form(state),
            _ => Err("this is not a local operational form".to_owned()),
        };
        match result {
            Ok(mutation) => {
                self.overlays.pop();
                self.open_operational_confirmation(state.action_id, mutation)
            }
            Err(error) => self.set_form_error(error),
        }
    }

    fn saved_view_from_form(&self, state: &FormState) -> Result<SavedView, String> {
        let name = required_form_value(state, "name", "a name for this view")?;
        let route = state
            .subject
            .iter()
            .find(|(label, _)| *label == "route")
            .map(|(_, value)| value.clone())
            .ok_or_else(|| "the view has no route to save".to_owned())?;
        if route != Route::Devices.label() {
            return Ok(SavedView {
                name,
                route,
                wide_columns: false,
                columns: Vec::new(),
                filters: Vec::new(),
                sort: Vec::new(),
            });
        }
        let filters = self
            .views
            .devices
            .applied_filter
            .terms
            .iter()
            .map(saved_filter_from_term)
            .collect::<Result<Vec<_>, _>>()?;
        let sort = self
            .views
            .devices
            .sort_terms
            .iter()
            .copied()
            .map(saved_sort_from_device)
            .collect();
        Ok(SavedView {
            name,
            route,
            wide_columns: self.views.devices.wide_columns,
            columns: self.views.devices.columns.clone(),
            filters,
            sort,
        })
    }

    fn accept_access_explorer_form(&mut self, state: &FormState) -> Vec<Effect> {
        let result = access_question_from_form(state).and_then(|question| {
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
                timeout: self.resolved_config.admin.request_timeout,
            })
        });
        match result {
            Ok(effect) => {
                self.overlays.pop();
                vec![effect]
            }
            Err(error) => self.set_form_error(error),
        }
    }

    /// The preview asks the server about one selector, so the form asks for
    /// the kind and the value and nothing else.
    fn accept_policy_preview_form(&mut self, state: &FormState) -> Vec<Effect> {
        let selector = state.value("for").trim();
        if selector.is_empty() || selector.len() > 256 || selector.chars().any(char::is_control) {
            return self.set_form_error("the selector must be non-empty, bounded, and textual");
        }
        let selector_type = if state.value("type") == "ipport" {
            PolicySelectorType::IpPort
        } else {
            PolicySelectorType::User
        };
        let selector = selector.to_owned();
        self.overlays.pop();
        self.start_policy_preview(selector_type, selector)
    }

    fn accept_admin_form(&mut self, state: &FormState) -> Vec<Effect> {
        let change = match admin_change_from_form(state) {
            Ok(change) => change,
            Err(error) => return self.set_form_error(error),
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
                return self.set_form_error(error);
            }
        }
        if !self.admin_mutation_available(state.action_id) {
            let reason = self
                .action_unavailable_reason(state.action_id)
                .unwrap_or_else(|| "admin mutation is unavailable".to_owned());
            return self.set_form_error(reason);
        }
        if state.action_id == ActionId::AdminRoutesReplaceApprovals {
            return self.accept_admin_batch_form(state, change);
        }
        let Some(profile) = self.admin.profile.clone() else {
            return Vec::new();
        };
        let (target_id, base_snapshot) = match self.admin_base_snapshot(&change) {
            Ok(value) => value,
            Err(error) => return self.set_form_error(error),
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
            return self
                .set_form_error("a conflicting admin mutation or read is running; preview again");
        }
        self.overlays.pop();
        effects
    }

    fn accept_admin_batch_form(&mut self, state: &FormState, change: AdminChange) -> Vec<Effect> {
        let AdminChange::DeviceRoutes { routes } = change else {
            return self.set_form_error("this batch action only supports route approvals");
        };
        let Some(profile) = self.admin.profile.clone() else {
            return self.set_form_error("an authenticated admin profile is required");
        };
        if !self.resolved_config.profiles.contains_key(&profile) {
            return self.set_form_error("admin profile configuration is unavailable");
        }
        if self.admin.tailnet.is_none() {
            return self.set_form_error("admin tailnet is not selected");
        }
        let observations = self
            .admin
            .route_observations()
            .into_iter()
            .filter(|route| route.complete)
            .collect::<Vec<_>>();
        if observations.is_empty() {
            return self.set_form_error("no complete route advertisers are available");
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
                    return self.set_form_error(format!(
                        "{} cannot receive the same replacement: {error}",
                        observation.device_id
                    ));
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
                return self.set_form_error(
                    "a route advertiser is already being read or changed; preview again",
                );
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
                            "latency: not probed; run the ping action before relying on this choice"
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
            if self.source_mode == SourceMode::Mock {
                self.overlays.pop();
                if let Some(workflow) = self.policy_workflow.as_mut() {
                    if let Err(error) = workflow.apply_guard(self.now) {
                        self.runtime_error = Some(error.to_string());
                        return Vec::new();
                    }
                    workflow.mark_applying();
                    workflow.mark_verifying();
                    workflow.mark_succeeded();
                    self.runtime_error = Some("mock policy applied and verified".to_owned());
                }
                return Vec::new();
            }
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
                self.runtime_error = Some(self.missing_executable_reason());
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
        if !self.operational_mutation_available(action_id) {
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
        if self.current_route() == Route::Local
            && self.views.local.section == LocalSection::Accounts
            && self.local_capabilities.accounts
            && let Some(executable) = self.local_executable.as_ref()
        {
            effects.push(Effect::StartLocalAccounts {
                executable: executable.clone(),
                timeout: self.resolved_config.local.command_timeout,
            });
        }
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
                    self.local_discovery_generation =
                        self.local_discovery_generation.saturating_add(1);
                    effects.push(Effect::StartLocalDiscovery {
                        generation: self.local_discovery_generation,
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

    /// The rows of `:profiles`, in the order they are shown. The local client is
    /// pinned first because it is where Tale starts and what it falls back to;
    /// only the admin profiles answer to the sort.
    pub fn profile_rows(&self) -> Vec<ProfileRow<'_>> {
        let filter = self.views.profiles.filter.trim().to_ascii_lowercase();
        let mut rows = self.all_profile_rows();
        if !filter.is_empty() {
            // The local row is pinned, not exempt: a filter that excludes it has
            // to exclude it, or the count in the border would be a lie.
            rows.retain(|row| row.matches(&filter));
        }
        rows
    }

    /// Every row, before the filter. The border reports both counts, so the
    /// total has to survive being narrowed.
    pub fn all_profile_rows(&self) -> Vec<ProfileRow<'_>> {
        let mut rows = Vec::with_capacity(self.resolved_config.profiles.len().saturating_add(1));
        rows.push(ProfileRow::Local {
            tailnet: self
                .local_resource
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.current_tailnet.as_deref()),
            account: self
                .local_accounts
                .iter()
                .find(|account| account.active)
                .map(LocalAccount::display_label),
            state: self.local_state.label(),
            active: self.admin.profile.is_none(),
        });
        let mut profiles = self
            .resolved_config
            .profiles
            .iter()
            .map(|(name, config)| ProfileRow::Admin {
                name: name.as_str(),
                config,
                status: self.profile_statuses.get(name),
                active: self.admin.profile.as_deref() == Some(name.as_str()),
            })
            .collect::<Vec<_>>();
        let sort = self.views.profiles.sort;
        profiles.sort_by(|left, right| {
            let ordering = left
                .ordering_key(sort.field)
                .cmp(&right.ordering_key(sort.field));
            if sort.direction.is_ascending() {
                ordering
            } else {
                ordering.reverse()
            }
        });
        rows.extend(profiles);
        rows
    }

    pub fn selected_profile_row(&self) -> Option<ProfileRow<'_>> {
        self.profile_rows()
            .get(self.views.profiles.selected)
            .copied()
    }

    fn move_profile_selection(&mut self, offset: isize) {
        let length = self.profile_rows().len();
        self.views.profiles.selected =
            move_bounded_index(self.views.profiles.selected, length, offset);
    }

    /// Resolved settings projected as the collection shown by `:config`.
    pub fn config_rows(&self) -> Vec<SettingDisplay> {
        let mut rows = self.all_config_rows();
        let filter = self.views.config.filter.trim().to_ascii_lowercase();
        if !filter.is_empty() {
            rows.retain(|row| {
                filter::fuzzy_matches(row.name, &filter)
                    || filter::fuzzy_matches(&row.value, &filter)
                    || filter::fuzzy_matches(row.source.label(), &filter)
            });
        }
        let sort = self.views.config.sort;
        rows.sort_by(|left, right| {
            let ordering = match sort.field {
                SettingSortField::Name => left.name.cmp(right.name),
                SettingSortField::Value => left.value.cmp(&right.value),
                SettingSortField::Source => left.source.label().cmp(right.source.label()),
            };
            if sort.direction.is_ascending() {
                ordering
            } else {
                ordering.reverse()
            }
        });
        rows
    }

    pub fn all_config_rows(&self) -> Vec<SettingDisplay> {
        let mut rows = self.resolved_config.settings();
        rows.push(SettingDisplay {
            name: "ui.theme.session",
            value: self.theme.id().as_str().to_owned(),
            source: ValueSource::Default,
        });
        rows.push(SettingDisplay {
            name: "ui.color.resolved",
            value: format!(
                "{} ({})",
                self.theme.capability().as_str(),
                match self.resolved_config.ui.color {
                    crate::config::ColorMode::Auto => "auto policy",
                    crate::config::ColorMode::None => "NO_COLOR or configured",
                    _ => "configured",
                }
            ),
            source: self.resolved_config.ui.color_source,
        });
        rows
    }

    pub fn selected_config_row(&self) -> Option<SettingDisplay> {
        self.config_rows()
            .into_iter()
            .nth(self.views.config.selected)
    }

    fn move_config_selection(&mut self, offset: isize) {
        let length = self.config_rows().len();
        self.views.config.selected = move_bounded_index(self.views.config.selected, length, offset);
    }

    /// Read what every configured profile's store holds. Local reads only, so
    /// this is cheap enough to repeat whenever the answer could have changed.
    fn inspect_profile_credentials(&self) -> Option<Effect> {
        if self.resolved_config.profiles.is_empty() {
            return None;
        }
        Some(Effect::InspectProfileCredentials {
            profiles: self
                .resolved_config
                .profiles
                .iter()
                .map(|(name, profile)| crate::effect::ProfileCredentialRef {
                    profile: name.clone(),
                    credential: profile.credential.clone(),
                })
                .collect(),
        })
    }

    /// Activation is the only thing on this page that costs a request, and it
    /// only ever costs one: the selected profile has to answer the control plane
    /// before the rest of the app is pointed at it.
    fn activate_selected_profile(&mut self) -> Vec<Effect> {
        let Some(row) = self.selected_profile_row() else {
            self.runtime_error = Some("no profile row is selected".to_owned());
            return Vec::new();
        };
        let Some(name) = row.name().map(str::to_owned) else {
            // The local client needs no credential and no verification: it is
            // the daemon on this machine, reachable or not on its own terms.
            self.profile_probe_in_flight = None;
            return self.switch_profile(None);
        };
        if self.admin.profile.as_deref() == Some(name.as_str()) {
            self.runtime_error = Some(format!("profile {name} is already active"));
            return Vec::new();
        }
        let Some(profile) = self.resolved_config.profiles.get(&name) else {
            self.runtime_error = Some(format!("profile {name} is no longer configured"));
            return Vec::new();
        };
        let status = self.profile_statuses.entry(name.clone()).or_default();
        match status.presence.as_ref() {
            None => {
                self.runtime_error = Some("the credential store has not been read yet".to_owned());
                return Vec::new();
            }
            Some(CredentialPresence::Missing) => {
                self.runtime_error = Some(format!(
                    "profile {name} has no stored credential; run `tale auth add {name}`"
                ));
                return Vec::new();
            }
            Some(CredentialPresence::Unreadable { detail }) => {
                self.runtime_error =
                    Some(format!("profile {name} credential is unreadable: {detail}"));
                return Vec::new();
            }
            Some(CredentialPresence::Stored { .. }) => {}
        }
        status.probe = ProbeState::InFlight;
        self.profile_probe_in_flight = Some(name.clone());
        vec![Effect::StartProfileProbe {
            profile: name,
            tailnet: profile.tailnet.clone(),
            credential: profile.credential.clone(),
            timeout: self.resolved_config.admin.request_timeout,
        }]
    }

    /// A verdict only counts for the attempt that is still outstanding, so a
    /// superseded probe cannot activate a profile the user has moved on from.
    fn finish_profile_probe(
        &mut self,
        profile: &str,
        result: Result<crate::secrets::CredentialKind, String>,
    ) -> Vec<Effect> {
        if self.profile_probe_in_flight.as_deref() != Some(profile) {
            return Vec::new();
        }
        self.profile_probe_in_flight = None;
        let status = self.profile_statuses.entry(profile.to_owned()).or_default();
        match result {
            Ok(kind) => {
                status.probe = ProbeState::Reachable { kind, at: self.now };
                self.switch_profile(Some(profile.to_owned()))
            }
            Err(detail) => {
                status.probe = ProbeState::Rejected {
                    detail: detail.clone(),
                    at: self.now,
                };
                self.runtime_error = Some(format!("profile {profile} was not activated: {detail}"));
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
            .retain(|overlay| !matches!(overlay, Overlay::SecretResult));
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
        self.views.overview.selected_id = None;
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
        self.refresh_device_view();
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
            generation,
            timeout: self.resolved_config.admin.request_timeout,
            audit_window_days: self.admin_audit_window_days,
        });
        effects
    }

    fn start_admin_current_view_refresh(&mut self) -> Vec<Effect> {
        // Refreshing `:profiles` re-reads the credential stores. It deliberately
        // does not re-probe: a probe is what activation is for.
        if self.current_route() == Route::Profiles {
            return self.inspect_profile_credentials().into_iter().collect();
        }
        let resources = match self.current_route() {
            Route::Overview | Route::Services | Route::Diagnostics => vec![
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
            // Task history is this client's own record: there is no server to
            // ask for it, so `r` has nothing to fetch.
            Route::Tasks => Vec::new(),
            Route::Audit => vec![
                AdminRefreshResource::Activity,
                AdminRefreshResource::Webhooks,
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
            // The rows come from stores, but the inspector states how the
            // active credential's tailnet is configured, and only the control
            // plane knows that.
            Route::Profiles => vec![
                AdminRefreshResource::Settings,
                AdminRefreshResource::Contacts,
            ],
            // Resolved from files and flags at startup: there is nothing to
            // re-read, so `r` has nothing to fetch.
            Route::Config => Vec::new(),
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
            Route::Audit => self.start_admin_resource_refresh(vec![AdminRefreshResource::Activity]),
            Route::Overview
            | Route::Local
            | Route::Profiles
            | Route::Services
            | Route::Diagnostics
            | Route::Config
            | Route::Tasks => self.start_admin_current_view_refresh(),
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
            generation: self.admin_generation,
            device_id: stable_id,
            timeout: self.resolved_config.admin.request_timeout,
        })
    }

    pub fn admin_device_enrichment_in_flight(&self, stable_id: &str) -> bool {
        self.admin_read_locks.contains_key(stable_id)
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
                self.refresh_device_view();
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
                self.refresh_device_view();
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
                self.refresh_device_view();
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
                self.refresh_device_view();
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
                        self.reopen_admin_form(
                            request.action_id,
                            &request.change,
                            error.to_string(),
                        );
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
                    self.reopen_admin_form(request.action_id, &request.change, detail);
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
                    self.reconcile_overview_selection();
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
                    self.refresh_device_view();
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
                self.reopen_admin_form(pending.action_id, &request.change, error.to_string());
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
            self.reopen_admin_form(pending.action_id, &request.change, detail);
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

    /// The tailnet the local client is on, as its MagicDNS suffix.
    pub fn local_tailnet_suffix(&self) -> Option<&str> {
        self.local_resource
            .snapshot
            .as_ref()?
            .magic_dns_suffix
            .as_deref()
            .filter(|value| !value.is_empty())
    }

    /// The tailnet the active profile reads, as its MagicDNS suffix. Taken from
    /// the devices the API returned rather than from `profiles.*.tailnet`,
    /// because that field is a request parameter — `-` is legal and common —
    /// and so cannot identify anything.
    pub fn admin_tailnet_suffix(&self) -> Option<&str> {
        self.admin
            .devices
            .snapshot
            .as_ref()?
            .iter()
            .find_map(AdminDevice::tailnet_suffix)
    }

    /// Whether the two sources are describing the same tailnet. Nothing may be
    /// composed until this says so: a node ID from one tailnet never matches a
    /// node ID from another, so composing them yields a union of two fleets
    /// wearing one heading.
    pub fn source_alignment(&self) -> SourceAlignment {
        if self.admin.profile.is_none() || self.source_mode != SourceMode::Local {
            return SourceAlignment::Single;
        }
        match (self.local_tailnet_suffix(), self.admin_tailnet_suffix()) {
            (Some(local), Some(admin)) if same_tailnet(local, admin) => {
                SourceAlignment::SameTailnet
            }
            (Some(local), Some(admin)) => SourceAlignment::Divergent {
                local: local.to_owned(),
                admin: admin.to_owned(),
            },
            _ => SourceAlignment::Undetermined,
        }
    }

    /// Which source owns `:devices`. What the user activated decides it. An
    /// unproven match is not a match: until both sources have named their
    /// tailnet, the active profile is shown alone rather than merged on a guess.
    pub fn device_view_source(&self) -> DeviceViewSource {
        if self.admin.profile.is_none() {
            return DeviceViewSource::Local;
        }
        match self.source_alignment() {
            SourceAlignment::SameTailnet => DeviceViewSource::Composed,
            _ => DeviceViewSource::Admin,
        }
    }

    fn local_devices(&self) -> Option<Vec<LocalDevice>> {
        self.local_resource.snapshot.as_ref().map(|snapshot| {
            let mut devices = Vec::with_capacity(snapshot.peers.len().saturating_add(1));
            devices.push(snapshot.self_node.clone());
            devices.extend(snapshot.peers.clone());
            devices
        })
    }

    fn recompute_composed_devices(&mut self) {
        let source = self.device_view_source();
        let local = self.local_devices();
        let admin = self.admin.devices.snapshot.clone();
        self.composed_devices = match source {
            DeviceViewSource::Composed => match (local.as_deref(), admin.as_deref()) {
                (Some(local), Some(admin)) => compose_exact_id(local, admin),
                _ => Vec::new(),
            },
            DeviceViewSource::Local => local
                .unwrap_or_default()
                .into_iter()
                .map(|device| ComposedDevice {
                    id: device.id.0.clone(),
                    local: Some(device),
                    admin: None,
                })
                .collect(),
            DeviceViewSource::Admin => admin
                .unwrap_or_default()
                .into_iter()
                .map(|device| ComposedDevice {
                    id: device.stable_id.clone(),
                    local: None,
                    admin: Some(device),
                })
                .collect(),
        };
    }

    /// The one writer of `devices_resource`. It used to be three — a local poll,
    /// an admin refresh, and the composer — each overwriting the list on
    /// arrival, so which tailnet `:devices` showed depended on whichever
    /// answered last. Now the owning source is decided first and written once.
    ///
    /// Public because it is the invariant, not an event handler: anything that
    /// changes either source restores the view by calling it.
    pub fn refresh_device_view(&mut self) {
        self.recompute_composed_devices();
        // Mock data has no local client and no profile behind it; it writes its
        // own list through the source events and owns it end to end.
        if self.source_mode == SourceMode::Mock {
            return;
        }
        let display = self
            .composed_devices
            .iter()
            .map(Self::display_device_from_composed)
            .collect::<Vec<_>>();
        let (observed_at, health, error) = match self.device_view_source() {
            DeviceViewSource::Admin => (
                self.admin.devices.observed_at,
                SourceHealth::from_admin_state(self.admin.devices.state),
                self.admin.devices.error.clone(),
            ),
            DeviceViewSource::Local | DeviceViewSource::Composed => (
                self.local_resource.last_success_at,
                match self.local_resource.status {
                    LocalResourceStatus::NeverLoaded => SourceHealth::Unavailable,
                    LocalResourceStatus::Loading => SourceHealth::Loading,
                    LocalResourceStatus::Fresh => SourceHealth::Healthy,
                    LocalResourceStatus::Stale => SourceHealth::Stale,
                    LocalResourceStatus::Failed => SourceHealth::Error,
                },
                self.local_resource
                    .failure
                    .as_ref()
                    .map(|failure| failure.detail.clone()),
            ),
        };
        self.reconcile_selection(Some(&display));
        self.devices_resource.snapshot = display;
        // One counter for one list. Two sources stamping their own generations
        // on a shared field is what let the visible-row cache serve indexes
        // computed against a list that is no longer on screen.
        self.devices_resource.generation = self.devices_resource.generation.saturating_add(1);
        self.devices_resource.observed_at = observed_at;
        self.devices_resource.health = health;
        self.devices_resource.error = error;
        self.reconcile_selection(None);
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
                version: None,
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

    /// Tab moves to the next tab and wraps, which is what a tab strip implies.
    fn change_route_section(&mut self, offset: isize) {
        match self.current_route() {
            Route::Local => self.change_local_section(offset),
            Route::Services => self.change_service_section(offset),
            _ => {}
        }
    }

    fn change_local_section(&mut self, offset: isize) {
        let sections = LocalSection::ALL;
        let length = sections.len();
        let current = sections
            .iter()
            .position(|section| *section == self.views.local.section)
            .unwrap_or(0);
        let step = offset.rem_euclid(length as isize).unsigned_abs();
        let next = current.saturating_add(step) % length;
        self.views.local.section = sections.get(next).copied().unwrap_or(LocalSection::Client);
        self.views.local.selected = 0;
        self.views.local.scroll = 0;
        self.detail_search.clear();
        self.detail_search_match = None;
        self.focus = Focus::Collection;
    }

    fn change_service_section(&mut self, offset: isize) {
        let sections = ServiceSection::ALL;
        let length = sections.len();
        let current = sections
            .iter()
            .position(|section| *section == self.views.services.section)
            .unwrap_or(0);
        let step = offset.rem_euclid(length as isize).unsigned_abs();
        let next = current.saturating_add(step) % length;
        self.views.services.section = sections.get(next).copied().unwrap_or(ServiceSection::Serve);
        self.views.services.selected = 0;
        self.views.services.scroll = 0;
        self.views.services.filter_draft.clear();
        self.views.services.applied_filter = FilterExpression::empty();
        self.focus = Focus::Collection;
    }

    fn move_diagnostics_scroll(&mut self, offset: isize) {
        let current = self.views.diagnostics.scroll;
        let next = if offset.is_negative() {
            current.saturating_sub(offset.unsigned_abs())
        } else {
            current.saturating_add(offset.unsigned_abs())
        };
        self.views.diagnostics.scroll = next.min(self.metrics_max_scroll());
    }

    fn move_service_selection(&mut self, offset: isize) {
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

    fn move_local_account_selection(&mut self, offset: isize) {
        let count = self.local_accounts.len();
        if count == 0 {
            self.views.local.selected = 0;
            self.views.local.scroll = 0;
            return;
        }
        self.views.local.selected = move_bounded_index(self.views.local.selected, count, offset);
        self.views.local.scroll = self.views.local.selected;
    }

    pub fn selected_local_account(&self) -> Option<&LocalAccount> {
        if self.views.local.section != LocalSection::Accounts {
            return None;
        }
        self.local_accounts.get(self.views.local.selected)
    }

    fn reconcile_local_account_selection(&mut self) {
        self.views.local.selected = self
            .views
            .local
            .selected
            .min(self.local_accounts.len().saturating_sub(1));
        self.views.local.scroll = self.views.local.selected;
    }

    /// Serve and Funnel as one table: filtered, then ordered by the chosen
    /// column. Public rows are mappings whose exposure is public, nothing more.
    pub fn visible_service_mappings(&self) -> Vec<&ServiceMapping> {
        let filter = &self.views.services.applied_filter;
        let mut mappings = self
            .services_snapshot
            .mappings()
            .filter(|mapping| filter.matches_mapping(mapping))
            .collect::<Vec<_>>();
        let sort = self.views.services.sort;
        mappings.sort_by(|left, right| {
            let ordering = sort
                .field
                .ordering_key(left)
                .cmp(&sort.field.ordering_key(right));
            match sort.direction {
                SortDirection::Ascending => ordering,
                SortDirection::Descending => ordering.reverse(),
            }
        });
        mappings
    }

    pub fn service_mapping_total(&self) -> usize {
        self.services_snapshot.mappings().count()
    }

    pub fn visible_taildrive_shares(&self) -> Vec<&TaildriveShare> {
        let query = self.views.services.filter_draft.trim();
        self.services_snapshot
            .taildrive
            .value
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|share| {
                query.is_empty()
                    || filter::fuzzy_matches(&share.name, query)
                    || filter::fuzzy_matches(&share.path.display().to_string(), query)
                    || share
                        .as_user
                        .as_deref()
                        .is_some_and(|user| filter::fuzzy_matches(user, query))
            })
            .collect()
    }

    pub fn visible_certificate_domains(&self) -> Vec<&str> {
        let query = self.views.services.filter_draft.trim();
        self.services_snapshot
            .certificate_domains
            .value
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(String::as_str)
            .filter(|domain| query.is_empty() || filter::fuzzy_matches(domain, query))
            .collect()
    }

    fn service_row_count(&self) -> usize {
        match self.views.services.section {
            ServiceSection::Serve => self.visible_service_mappings().len(),
            // With the alpha feature off nothing is listed, so nothing counts.
            ServiceSection::Taildrive if !self.alpha_local_features => 0,
            ServiceSection::Taildrive => self.visible_taildrive_shares().len(),
            ServiceSection::Certificates => self.visible_certificate_domains().len(),
        }
    }

    pub fn selected_service_mapping(&self) -> Option<ServiceMapping> {
        if self.views.services.section != ServiceSection::Serve {
            return None;
        }
        self.visible_service_mappings()
            .get(self.views.services.selected)
            .map(|mapping| (*mapping).clone())
    }

    /// The discovered Taildrop target for the selected device row. An address
    /// is exact, so it decides on its own; a display name is not, so it is used
    /// only when it picks out exactly one target and no address matched. A
    /// destination is never inferred from a name several devices share.
    pub fn selected_taildrop_target(&self) -> Option<TaildropTarget> {
        let device = self.selected_device()?;
        let targets = self.services_snapshot.taildrop_targets.value.as_ref()?;
        only(targets.iter().filter(|target| {
            device
                .addresses
                .iter()
                .any(|address| address.eq_ignore_ascii_case(&target.command_target))
        }))
        .or_else(|| {
            only(
                targets
                    .iter()
                    .filter(|target| taildrop_target_names_device(target, device)),
            )
        })
        .cloned()
    }

    pub fn selected_taildrive_share(&self) -> Option<TaildriveShare> {
        self.visible_taildrive_shares()
            .get(self.views.services.selected)
            .copied()
            .cloned()
    }

    pub fn selected_certificate_domain(&self) -> Option<&str> {
        self.visible_certificate_domains()
            .get(self.views.services.selected)
            .copied()
    }

    pub fn service_inspector_available(&self) -> bool {
        match self.views.services.section {
            ServiceSection::Serve => self.selected_service_mapping().is_some(),
            ServiceSection::Taildrive => {
                self.alpha_local_features && self.selected_taildrive_share().is_some()
            }
            ServiceSection::Certificates => self.selected_certificate_domain().is_some(),
        }
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
        } else {
            Vec::new()
        };
        actions.extend(self.local_actions_for_route());
        actions.extend(self.operational_resource_actions());
        actions
    }

    /// The local client's actions, offered where their subject is on screen.
    /// These used to be one list handed to every route that had no list of its
    /// own, which is how `:credentials` came to offer `remove local account`
    /// and how `open tailscale ssh` — which acts on the selected device — was
    /// missing from `:devices` whenever an admin profile was configured.
    fn local_actions_for_route(&self) -> Vec<ActionId> {
        if self.source_mode != SourceMode::Local {
            return Vec::new();
        }
        match self.current_route() {
            // This machine: connecting it, its preferences, and local policy.
            Route::Local if self.views.local.section == LocalSection::Client => vec![
                ActionId::LocalConnect,
                ActionId::LocalDisconnect,
                ActionId::LocalPreferencesEdit,
                ActionId::LocalExitNodeSelect,
                ActionId::LocalRoutesEditAdvertisements,
                ActionId::LocalSyspolicyReload,
            ],
            // Account actions live with the account rows they act on.
            Route::Local => {
                let mut actions = Vec::new();
                let has_selection = self.selected_local_account().is_some();
                if has_selection {
                    actions.push(ActionId::LocalAccountSwitch);
                }
                actions.push(ActionId::LocalAccountLogin);
                actions.push(ActionId::LocalAccountLogout);
                if has_selection {
                    actions.push(ActionId::LocalAccountRemove);
                }
                actions
            }
            // Every one of these acts on the selected row: it pings it, looks
            // it up, opens a session to it, or sends it a file. All of them go
            // through the local daemon, so they are withheld when the rows on
            // screen belong to a tailnet this machine is not on — offering to
            // SSH to an unreachable node is worse than not offering at all.
            Route::Devices if self.device_view_source().is_locally_reachable() => vec![
                ActionId::LocalProbeConnection,
                ActionId::LocalWhois,
                ActionId::LocalSshOpen,
                ActionId::LocalNcOpen,
                ActionId::DevicesTaildropSend,
                ActionId::DevicesTaildropReceive,
            ],
            // The summary this route is showing is the thing being copied.
            Route::Diagnostics => vec![ActionId::DiagnosticCopy],
            _ => Vec::new(),
        }
    }

    pub fn contextual_copy_fields(&self) -> Vec<CopyField> {
        if self.current_route() == Route::Diagnostics {
            return vec![CopyField::Metrics];
        }
        if self.current_route() == Route::Services {
            // Only the mapping table has a row worth copying; the other
            // sections are a name or a path already visible in full.
            return if self.selected_service_mapping().is_some() {
                vec![
                    CopyField::ServiceUrl,
                    CopyField::ServiceListener,
                    CopyField::ServiceBackend,
                ]
            } else {
                Vec::new()
            };
        }
        if self.current_route() == Route::Tasks {
            // The row is already readable; what anyone pastes into a bug report
            // is the command that ran and what it printed.
            let Some(task) = self.focused_task() else {
                return Vec::new();
            };
            let mut fields = vec![CopyField::TaskId, CopyField::TaskResult];
            if !task.redacted_argv.is_empty() {
                fields.push(CopyField::TaskCommand);
            }
            if !task.detail.is_empty() {
                fields.push(CopyField::TaskOutput);
            }
            return fields;
        }
        if self.current_route() == Route::Config {
            return self.selected_config_row().map_or_else(Vec::new, |_| {
                vec![
                    CopyField::ConfigSetting,
                    CopyField::ConfigValue,
                    CopyField::ConfigSource,
                ]
            });
        }
        if self.current_route() == Route::Profiles {
            // The row is mostly words already on screen; what is worth pasting
            // is what you would type somewhere else — into a config file, a
            // shell, or a message asking someone why a credential was refused.
            let Some(row) = self.selected_profile_row() else {
                return Vec::new();
            };
            let mut fields = vec![CopyField::ProfileName];
            if row.tailnet().is_some_and(|value| !value.is_empty()) {
                fields.push(CopyField::ProfileTailnet);
            }
            match row {
                ProfileRow::Local { account, .. } => {
                    if account.is_some() {
                        fields.push(CopyField::ProfileAccount);
                    }
                }
                ProfileRow::Admin { .. } => {
                    fields.push(CopyField::ProfileCredential);
                    fields.push(CopyField::ProfileBackend);
                }
            }
            return fields;
        }
        if self.current_route() == Route::Users {
            // The row is three facts and two of them are words already on
            // screen; the ones worth pasting are the identifiers.
            let Some(user) = self.selected_admin_user() else {
                return Vec::new();
            };
            let mut fields = vec![CopyField::UserId];
            if user.display_name.is_some() {
                fields.push(CopyField::UserName);
            }
            if user.login_name.is_some() {
                fields.push(CopyField::UserLogin);
            }
            return fields;
        }
        if self.current_route() != Route::Devices {
            return Vec::new();
        }
        let mut fields = vec![
            CopyField::DeviceId,
            CopyField::DisplayName,
            CopyField::Hostname,
        ];
        // Offered only when a name was actually reported: a key that copies
        // "not returned" is worse than a key that is not there.
        if self.selected_dns_name().is_some() {
            fields.push(CopyField::DnsName);
        }
        fields.extend([CopyField::Owner, CopyField::Addresses, CopyField::Tags]);
        if self.source_mode == SourceMode::Local {
            fields.push(CopyField::PublicKey);
            fields.push(CopyField::Endpoint);
        }
        fields
    }

    /// The selected device's full MagicDNS name. The local client reports it
    /// with a trailing dot, which is correct in a zone file and wrong in every
    /// place this value gets pasted.
    pub fn selected_dns_name(&self) -> Option<String> {
        let id = self.views.devices.selected_id.as_ref()?;
        let name = self.local_dns_name(id).map(str::to_owned).or_else(|| {
            self.admin
                .devices
                .snapshot
                .as_ref()?
                .iter()
                .find(|device| device.stable_id == id.0)
                .and_then(|device| device.name.clone())
        })?;
        let name = name.trim_end_matches('.');
        (!name.is_empty()).then(|| name.to_owned())
    }

    fn service_actions_for_section(&self) -> Vec<ActionId> {
        match self.views.services.section {
            // One table, so both sets of actions belong to it. Which command
            // runs is decided by the exposure of the row, not by a tab.
            ServiceSection::Serve => vec![
                ActionId::ServicesServeRefresh,
                ActionId::ServicesServeCreate,
                ActionId::ServicesFunnelCreate,
                ActionId::ServicesServeEdit,
                ActionId::ServicesFunnelUnpublish,
                ActionId::ServicesServeRemove,
                ActionId::ServicesServeReset,
                ActionId::ServicesFunnelReset,
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
        }
    }

    fn operational_resource_actions(&self) -> Vec<ActionId> {
        let mut actions = Vec::new();
        // Saved views and exports are for collections Tale fetched. `:profiles`
        // lists this machine's own configuration, which is already a file the
        // user owns, so offering to export it or to name a view of it would be
        // offering something with no subject.
        if !matches!(self.current_route(), Route::Profiles | Route::Config) {
            actions.extend([
                ActionId::SavedViewCreate,
                ActionId::SavedViewReplace,
                ActionId::SavedViewRename,
                ActionId::SavedViewDelete,
                ActionId::SavedViewApply,
                ActionId::CollectionExport,
            ]);
        }
        match self.current_route() {
            Route::Overview => actions.extend([
                ActionId::OverviewHealthOpenResource,
                ActionId::OverviewHealthRunSuggestedAction,
            ]),
            Route::Access => {
                if self.policy_workflow.is_some() {
                    actions.extend([
                        ActionId::AdminPolicyEditorReopen,
                        ActionId::AdminPolicyRemoteRefresh,
                        ActionId::AdminPolicyValidate,
                        ActionId::AdminPolicyPreview,
                        ActionId::AdminPolicyDiff,
                        ActionId::AdminPolicyApply,
                        ActionId::AdminPolicyCandidateDiscard,
                        ActionId::AdminPolicyWorkflowClose,
                    ]);
                } else {
                    actions.push(ActionId::AdminPolicyEdit);
                }
                actions.extend([
                    ActionId::AccessExplorerAsk,
                    ActionId::AccessExplorerOpenRule,
                ]);
            }
            Route::Audit => actions.extend([
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
            Route::Diagnostics => actions.extend([
                ActionId::ServicesMetricsRefresh,
                ActionId::ServicesBugReportCreate,
            ]),
            // The one thing a row on this page can be asked to do.
            Route::Profiles => actions.push(ActionId::ProfileActivate),
            Route::Devices
            | Route::Users
            | Route::Routes
            | Route::Dns
            | Route::Credentials
            | Route::Local
            | Route::Tasks
            | Route::Config
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
            // Neither of these asks anything the row does not already answer,
            // so they go straight to the confirmation with no form in between.
            ActionId::ServicesServeRemove => {
                let Some(mapping) = self.selected_service_mapping() else {
                    self.runtime_error = Some("select a mapping to remove".to_owned());
                    return Vec::new();
                };
                self.open_service_confirmation(ServiceActionRequest::MappingRemove { mapping })
            }
            ActionId::ServicesFunnelUnpublish => {
                let Some(mapping) = self.selected_service_mapping() else {
                    self.runtime_error = Some("select a public mapping to unpublish".to_owned());
                    return Vec::new();
                };
                if mapping.exposure != Exposure::Public {
                    self.runtime_error =
                        Some("the selected mapping is already tailnet-only".to_owned());
                    return Vec::new();
                }
                self.open_service_confirmation(ServiceActionRequest::FunnelUnpublish { mapping })
            }
            ActionId::ServicesServeCreate | ActionId::ServicesFunnelCreate => {
                let public = action_id == ActionId::ServicesFunnelCreate;
                self.push_form(
                    action_id,
                    if public {
                        "New public mapping"
                    } else {
                        "New tailnet mapping"
                    },
                    vec![(
                        "reachable by",
                        reachability(&if public {
                            Exposure::Public
                        } else {
                            Exposure::Tailnet
                        })
                        .to_owned(),
                    )],
                    mapping_fields(public, None),
                );
                Vec::new()
            }
            ActionId::ServicesServeEdit => {
                // The selected row already knows its exposure, and Tailscale
                // replaces a mapping by listener and path, so those are stated
                // rather than offered: changing them is a new mapping.
                let Some(mapping) = self.selected_service_mapping() else {
                    self.runtime_error = Some("select a mapping to edit".to_owned());
                    return Vec::new();
                };
                self.push_form(
                    action_id,
                    "Edit mapping",
                    vec![
                        ("reachable by", reachability(&mapping.exposure).to_owned()),
                        (
                            "listener",
                            format!("{}:{}", mapping.listener.label(), mapping.listener.port()),
                        ),
                        ("path", mapping.mount.as_path().to_owned()),
                    ],
                    vec![
                        FormField::text(
                            "backend",
                            "Serve",
                            "A local port, an http:// URL, or a folder to serve files from",
                            "3000",
                            mapping.backend.argument(),
                        ),
                        FormField::options(
                            "proxy",
                            "PROXY protocol",
                            "Only used by TCP listeners; leave off unless the backend expects it",
                            &["none", "1", "2"],
                            mapping.proxy_protocol.cli_value().unwrap_or("none"),
                        ),
                    ],
                );
                Vec::new()
            }
            ActionId::DevicesTaildropSend => {
                // The selected row is the target, so the form asks only what
                // it cannot already know.
                let Some(device) = self.selected_device() else {
                    self.runtime_error = Some("select a device to send files to".to_owned());
                    return Vec::new();
                };
                let name = device.display_name.clone();
                let Some(target) = self.selected_taildrop_target() else {
                    self.runtime_error = Some(format!(
                        "{name} was not offered as a Taildrop target by this client"
                    ));
                    return Vec::new();
                };
                if !target.available() {
                    self.runtime_error = Some(match target.capability_reason.as_deref() {
                        Some(reason) => format!("{name} cannot receive files: {reason}"),
                        None => format!("{name} is offline"),
                    });
                    return Vec::new();
                }
                self.push_form(
                    action_id,
                    "Send files",
                    vec![("to", target.display_name.clone())],
                    vec![FormField::text(
                        "files",
                        "Files",
                        "Full paths or ~/ paths, separated by commas",
                        "~/path/to/file",
                        String::new(),
                    )],
                );
                Vec::new()
            }
            ActionId::DevicesTaildropReceive => {
                self.push_form(
                    action_id,
                    "Receive files",
                    Vec::new(),
                    vec![
                        FormField::text(
                            "directory",
                            "Save to",
                            "An existing directory on this machine; ~/ is supported",
                            "~/Downloads",
                            String::new(),
                        ),
                        FormField::options(
                            "conflict",
                            "If a name is taken",
                            "What to do when a file of that name already exists",
                            &["rename", "skip", "overwrite"],
                            "rename",
                        ),
                        FormField::toggle(
                            "wait",
                            "Keep waiting",
                            "Stay open for files that arrive later",
                            false,
                        ),
                    ],
                );
                Vec::new()
            }
            ActionId::ServicesDriveShare => {
                self.push_form(
                    action_id,
                    "Share a folder",
                    Vec::new(),
                    vec![
                        FormField::text(
                            "name",
                            "Share name",
                            "What the tailnet will see; letters, digits and dashes",
                            "documents",
                            String::new(),
                        ),
                        FormField::text(
                            "path",
                            "Folder",
                            "An existing directory on this machine; ~/ is supported",
                            "~/Documents",
                            String::new(),
                        ),
                    ],
                );
                Vec::new()
            }
            ActionId::ServicesDriveRename => {
                let Some(share) = self.selected_taildrive_share() else {
                    self.runtime_error = Some("select a share to rename".to_owned());
                    return Vec::new();
                };
                self.push_form(
                    action_id,
                    "Rename share",
                    vec![("current name", share.name.clone())],
                    vec![FormField::text(
                        "new",
                        "New name",
                        "Letters, digits and dashes",
                        "documents",
                        share.name,
                    )],
                );
                Vec::new()
            }
            ActionId::ServicesDriveUnshare => {
                let Some(share) = self.selected_taildrive_share() else {
                    self.runtime_error = Some("select a share to stop sharing".to_owned());
                    return Vec::new();
                };
                self.open_service_confirmation(ServiceActionRequest::TaildriveUnshare {
                    name: share.name,
                })
            }
            ActionId::ServicesCertificateObtain => {
                let Some(domain) = self.selected_certificate_domain().map(str::to_owned) else {
                    self.runtime_error = Some("select a domain".to_owned());
                    return Vec::new();
                };
                self.push_form(
                    action_id,
                    "Get a certificate",
                    vec![("domain", domain)],
                    vec![
                        FormField::text(
                            "cert",
                            "Certificate file",
                            "Where to write the certificate; ~/ is supported",
                            "~/certificate.crt",
                            String::new(),
                        ),
                        FormField::text(
                            "key",
                            "Key file",
                            "Where to write the private key; ~/ is supported",
                            "~/certificate.key",
                            String::new(),
                        ),
                        FormField::text(
                            "min-validity",
                            "Renew if under",
                            "Renew when less than this remains, such as 30d; blank never forces",
                            "30d",
                            String::new(),
                        ),
                    ],
                );
                Vec::new()
            }
            ActionId::ServicesMetricsRefresh => {
                self.start_service_request(ServiceActionRequest::Metrics)
            }
            ActionId::ServicesBugReportCreate => {
                self.push_form(
                    action_id,
                    "Create a bug report",
                    Vec::new(),
                    vec![
                        FormField::text(
                            "note",
                            "Note",
                            "What went wrong, in your own words",
                            "optional",
                            String::new(),
                        ),
                        FormField::toggle(
                            "diagnose",
                            "Run diagnostics",
                            "Collect extra network checks; takes longer",
                            false,
                        ),
                    ],
                );
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn push_form(
        &mut self,
        action_id: ActionId,
        title: &'static str,
        subject: Vec<(&'static str, String)>,
        fields: Vec<FormField>,
    ) {
        self.overlays.push(Overlay::Form(FormState {
            action_id,
            title,
            subject,
            fields,
            selected: 0,
            cursor: 0,
            draft: None,
            list: None,
            secret: None,
            error: None,
        }));
    }

    /// Reports why a form cannot be submitted on the form itself, so the user
    /// answers the question where they were asked it.
    fn set_form_error(&mut self, error: impl Into<String>) -> Vec<Effect> {
        if let Some(Overlay::Form(current)) = self.overlays.last_mut() {
            current.error = Some(error.into());
        }
        Vec::new()
    }

    fn accept_form(&mut self, state: FormState) -> Vec<Effect> {
        match state.action_id {
            ActionId::LocalSshOpen | ActionId::LocalNcOpen => {
                return self.accept_handoff_form(&state);
            }
            ActionId::LocalDnsQuery => return self.accept_dns_query_form(&state),
            ActionId::LocalWhois => return self.accept_whois_form(&state),
            ActionId::LocalPreferencesEdit => return self.accept_preferences_form(&state),
            ActionId::LocalExitNodeSelect => return self.accept_exit_node_form(&state),
            ActionId::LocalRoutesEditAdvertisements => {
                return self.accept_advertisement_form(&state);
            }
            ActionId::SavedViewCreate
            | ActionId::SavedViewReplace
            | ActionId::SavedViewRename
            | ActionId::SavedViewDelete
            | ActionId::SavedViewApply
            | ActionId::CollectionExport => {
                return self.accept_local_operational_form(&state);
            }
            ActionId::ActivityFlowsSelectWindow => return self.accept_flow_window_form(&state),
            ActionId::AdminCredentialAuthKeyCreate => return self.accept_auth_key_form(&state),
            ActionId::AdminWebhookCreate
            | ActionId::AdminWebhookEdit
            | ActionId::AdminLogStreamReplace
            | ActionId::AdminNetworkLogsSettings => {
                return self.accept_admin_operational_form(&state);
            }
            ActionId::AdminPolicyPreview => return self.accept_policy_preview_form(&state),
            ActionId::AccessExplorerAsk => return self.accept_access_explorer_form(&state),
            ActionId::AuditFilterTime
            | ActionId::AuditFilterActor
            | ActionId::AuditFilterAction
            | ActionId::AuditFilterTarget => return self.accept_audit_filter(&state),
            action_id if is_admin_mutation_action(action_id) => {
                return self.accept_admin_form(&state);
            }
            _ => {}
        }
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
                if let Some(Overlay::Form(current)) = self.overlays.last_mut() {
                    current.error = Some(error);
                }
                Vec::new()
            }
        }
    }

    fn parse_service_form(&self, state: &FormState) -> Result<ServiceActionRequest, String> {
        let fields = state
            .fields
            .iter()
            .map(|field| (field.key.to_owned(), field.value.trim().to_owned()))
            .collect::<BTreeMap<_, _>>();
        match state.action_id {
            ActionId::ServicesServeCreate | ActionId::ServicesFunnelCreate => {
                let exposure = if state.action_id == ActionId::ServicesFunnelCreate {
                    Exposure::Public
                } else {
                    Exposure::Tailnet
                };
                let mapping = self.parse_mapping_form(&fields, exposure.clone())?;
                Ok(if exposure == Exposure::Public {
                    ServiceActionRequest::Funnel {
                        mapping,
                        edit: false,
                    }
                } else {
                    ServiceActionRequest::Serve {
                        mapping,
                        edit: false,
                    }
                })
            }
            // One edit action: the selected row decides which command runs, and
            // its listener and path are not editable, so identity always holds.
            ActionId::ServicesServeEdit => {
                let Some(selected) = self.selected_service_mapping() else {
                    return Err("select a mapping to edit".to_owned());
                };
                let backend = parse_form_backend(required_field(&fields, "backend")?)?;
                let proxy_protocol =
                    ProxyProtocol::parse(optional_field(&fields, "proxy").unwrap_or("none"))
                        .map_err(|error| error.to_string())?;
                let mapping = ServiceMapping {
                    backend,
                    proxy_protocol,
                    ..selected
                };
                mapping.validate().map_err(|error| error.to_string())?;
                Ok(if mapping.exposure == Exposure::Public {
                    ServiceActionRequest::Funnel {
                        mapping,
                        edit: true,
                    }
                } else {
                    ServiceActionRequest::Serve {
                        mapping,
                        edit: true,
                    }
                })
            }
            // The target is the selected device, never typed: the form is
            // modal, so the row it names is still the row underneath it.
            ActionId::DevicesTaildropSend => {
                let target = self.selected_taildrop_target().ok_or_else(|| {
                    "the selected device is no longer a Taildrop target".to_owned()
                })?;
                if !target.available() {
                    return Err("the selected Taildrop target is unavailable".to_owned());
                }
                let files = required_field(&fields, "files")?
                    .split(',')
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .map(std::path::PathBuf::from)
                    .map(|path| expand_form_path(&path))
                    .map(|path| path.and_then(|path| validate_regular_file(&path)))
                    .collect::<Result<Vec<_>, _>>()?;
                if files.is_empty() {
                    return Err("select at least one existing regular file".to_owned());
                }
                Ok(ServiceActionRequest::TaildropSend(TaildropSendRequest {
                    files,
                    target,
                }))
            }
            ActionId::DevicesTaildropReceive => {
                let directory = expand_form_path(Path::new(required_field(&fields, "directory")?))?;
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
                let path = expand_form_path(Path::new(required_field(&fields, "path")?))?;
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
                let domain = state
                    .subject
                    .iter()
                    .find_map(|(label, value)| (*label == "domain").then(|| value.clone()))
                    .ok_or_else(|| "the selected certificate domain is unavailable".to_owned())?;
                let certificate_path =
                    expand_form_path(Path::new(required_field(&fields, "cert")?))?;
                let key_path = expand_form_path(Path::new(required_field(&fields, "key")?))?;
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
        let mount = PathMount::parse(optional_field(fields, "path").unwrap_or("/"))
            .map_err(|error| error.to_string())?;
        let backend = parse_form_backend(required_field(fields, "backend")?)?;
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
            ServiceActionRequest::MappingRemove { mapping } => {
                services::mapping_off_command(command_path, timeout, mapping, true).ok()?
            }
            ServiceActionRequest::FunnelUnpublish { mapping } => {
                services::mapping_unpublish_command(command_path, timeout, mapping, true).ok()?
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
        // The argv already appears under Command; the preview says what the
        // change means, in the reader's terms.
        let mut preview = vec![service_effect_sentence(request)];
        if let ServiceActionRequest::TaildriveShare {
            input_name,
            normalized_name,
            ..
        } = request
            && input_name != normalized_name
        {
            preview.push(format!(
                "\"{input_name}\" is not a usable share name; it will be shared as \"{normalized_name}\"."
            ));
        }
        if let ServiceActionRequest::TaildropSend(request) = request {
            preview.push("Resolved files:".to_owned());
            preview.extend(
                request
                    .files
                    .iter()
                    .map(|file| format!("  {}", file.path.display())),
            );
        }
        if let ServiceActionRequest::Certificate(request) = request {
            preview.push(format!(
                "Certificate file: {}",
                request.certificate_path.display()
            ));
            preview.push(format!("Key file: {}", request.key_path.display()));
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

    /// Whether the last status read still shows this exact mapping, in the list
    /// its exposure puts it in. Identity is listener and path, so the backend is
    /// compared too: the same address serving something else is a different row.
    fn service_mapping_is_current(&self, mapping: &ServiceMapping) -> bool {
        let listed: Option<&[ServiceMapping]> = match mapping.exposure {
            Exposure::Public => self
                .services_snapshot
                .funnel
                .value
                .as_ref()
                .map(|status| status.mappings.as_slice()),
            Exposure::Tailnet => self
                .services_snapshot
                .serve
                .value
                .as_ref()
                .map(|status| status.mappings.as_slice()),
        };
        listed.is_some_and(|mappings| {
            mappings.iter().any(|actual| {
                actual.exact_identity_matches(mapping) && actual.backend == mapping.backend
            })
        })
    }

    fn revalidate_service_request(&self, request: &ServiceActionRequest) -> Result<(), String> {
        match request {
            ServiceActionRequest::Serve { mapping, edit } => {
                mapping.validate().map_err(|error| error.to_string())?;
                if mapping.exposure != Exposure::Tailnet {
                    return Err("Serve requests must remain tailnet-only".to_owned());
                }
                if !self.local_capabilities.serve {
                    return Err("Serve is unsupported by this CLI".to_owned());
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
                if !self.local_capabilities.funnel {
                    return Err("Funnel is unsupported by this CLI".to_owned());
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
            // A stale row is the whole hazard here: removing by listener and
            // path would happily take down whatever now sits at that address.
            ServiceActionRequest::MappingRemove { mapping } => {
                mapping.validate().map_err(|error| error.to_string())?;
                if !self.service_mapping_is_current(mapping) {
                    return Err("the selected mapping changed; refresh and remove again".to_owned());
                }
                Ok(())
            }
            ServiceActionRequest::FunnelUnpublish { mapping } => {
                mapping.validate().map_err(|error| error.to_string())?;
                if mapping.exposure != Exposure::Public {
                    return Err("only a public mapping can stop being published".to_owned());
                }
                if !self.local_capabilities.serve {
                    return Err("Serve is unsupported by this CLI".to_owned());
                }
                if !self.service_mapping_is_current(mapping) {
                    return Err(
                        "the selected PUBLIC mapping changed; refresh and unpublish again"
                            .to_owned(),
                    );
                }
                // The mapping is re-served verbatim, so the backend has to be
                // as usable now as it was when it was first accepted.
                validate_mapping_backend(mapping)
            }
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
            | ServiceActionRequest::Funnel { mapping, .. }
            | ServiceActionRequest::FunnelUnpublish { mapping } => {
                vec![
                    "listener".to_owned(),
                    "mount".to_owned(),
                    mapping.backend.label().to_owned(),
                ]
            }
            ServiceActionRequest::MappingRemove { .. } => {
                vec!["listener".to_owned(), "mount".to_owned()]
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
            started_at: self.now,
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
            title: "Actions",
            actions,
            choices: Vec::new(),
            fields: Vec::new(),
            addresses: Vec::new(),
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
            self.runtime_error = Some(self.missing_executable_reason());
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

    fn open_dns_query_form(&mut self) -> Vec<Effect> {
        self.push_form(
            ActionId::LocalDnsQuery,
            "Query the tailnet resolver",
            Vec::new(),
            vec![
                FormField::text(
                    "name",
                    "Name",
                    "The DNS name to resolve through the local daemon",
                    "host.example.com",
                    String::new(),
                ),
                FormField::options(
                    "type",
                    "Record",
                    "Which record the resolver is asked for",
                    diagnostics::DnsRecordType::LABELS,
                    "A",
                ),
            ],
        );
        Vec::new()
    }

    fn open_whois_form(&mut self) -> Vec<Effect> {
        let seed = self
            .selected_local_device()
            .and_then(|device| device.tailscale_ips.first())
            .cloned()
            .unwrap_or_default();
        self.push_form(
            ActionId::LocalWhois,
            "Identify a tailnet address",
            Vec::new(),
            vec![
                FormField::text(
                    "target",
                    "Address",
                    "A Tailscale IP, optionally with a port",
                    "100.64.0.1 or 100.64.0.1:443",
                    seed,
                ),
                FormField::options(
                    "protocol",
                    "Protocol",
                    "Narrows the lookup to one transport; any leaves it unset",
                    &["any", "tcp", "udp"],
                    "any",
                ),
            ],
        );
        Vec::new()
    }

    fn accept_dns_query_form(&mut self, state: &FormState) -> Vec<Effect> {
        let name = state.value("name").trim();
        if name.is_empty() {
            return self.set_form_error("enter a DNS name");
        }
        match diagnostics::validate_dns_query(name, state.value("type")) {
            Ok(record_type) => {
                self.overlays.pop();
                self.start_local_diagnostic(DiagnosticRequest::DnsQuery {
                    name: name.to_owned(),
                    record_type,
                })
            }
            Err(error) => self.set_form_error(error),
        }
    }

    fn accept_whois_form(&mut self, state: &FormState) -> Vec<Effect> {
        let target = state.value("target").trim();
        if target.is_empty() {
            return self.set_form_error("enter an IP address or IP:port");
        }
        let protocol = match state.value("protocol") {
            "tcp" => Some(diagnostics::WhoisProtocol::Tcp),
            "udp" => Some(diagnostics::WhoisProtocol::Udp),
            _ => None,
        };
        match diagnostics::validate_whois_target(target) {
            Ok(_) => {
                self.overlays.pop();
                self.start_local_diagnostic(DiagnosticRequest::Whois {
                    target: target.to_owned(),
                    protocol,
                })
            }
            Err(error) => self.set_form_error(error),
        }
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
        if reason == ShutdownReason::UserQuit {
            self.runtime_error = None;
        }
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
                // The loading frame may already have cached an empty visible
                // list under this same request generation. The completed
                // snapshot changes the cache's subject even when the request
                // generation does not change.
                let _ = self.device_visible_cache.get_mut().take();
                self.devices_resource.observed_at = Some(observed_at);
                self.devices_resource.health = if self.now.saturating_sub(observed_at) > 60 {
                    SourceHealth::Stale
                } else {
                    SourceHealth::Healthy
                };
                self.devices_resource.error = None;
                self.reconcile_selection(None);
                self.refresh_device_view();
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
                if generation >= self.local_discovery_generation {
                    self.local_discovery_generation = generation;
                    self.local_discovery_in_flight = true;
                }
            }
            LocalEvent::DiscoverySucceeded {
                generation,
                executable,
            } => {
                if generation < self.local_discovery_generation {
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
                if generation < self.local_discovery_generation {
                    return Vec::new();
                }
                self.local_discovery_in_flight = false;
                self.local_cli_state = match failure.kind {
                    LocalFailureKind::ExecutableMissing => LocalCliState::Missing {
                        detail: format!("{}. {}", failure.summary, failure.detail),
                    },
                    LocalFailureKind::ExecutableDenied | LocalFailureKind::PermissionDenied => {
                        LocalCliState::PermissionDenied {
                            detail: format!("{}. {}", failure.summary, failure.detail),
                        }
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
                self.services_snapshot.command_version = Some(snapshot.client_version.clone());
                self.services_snapshot.certificate_domains.succeed(
                    self.services_snapshot.generation,
                    snapshot.observed_at,
                    snapshot.cert_domains.clone(),
                );
                self.local_resource.succeed(generation, snapshot);
                self.refresh_device_view();
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
                self.refresh_device_view();
                self.services_snapshot
                    .certificate_domains
                    .fail(self.services_snapshot.generation, service_failure);
                self.leave_unavailable_route();
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
            LocalEvent::WatcherConnected { generation } => {
                if generation != self.local_observer_generation {
                    return Vec::new();
                }
                self.local_watcher_connected = true;
                self.local_daemon_state = LocalDaemonState::Connecting;
            }
            LocalEvent::WatcherDisconnected {
                generation,
                failure,
            } => {
                if generation != self.local_observer_generation {
                    return Vec::new();
                }
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
                self.refresh_device_view();
                // The list itself is unchanged by a dropped watcher; only the
                // reason it has stopped moving is new.
                self.devices_resource.error = Some(failure.detail);
                self.leave_unavailable_route();
            }
            LocalEvent::AccountsSucceeded { accounts } => {
                self.local_accounts = accounts;
                self.local_accounts_failure = None;
                self.reconcile_local_account_selection();
            }
            LocalEvent::AccountsFailed { failure } => {
                self.local_accounts_failure = Some(failure);
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
                    self.local_accounts_failure = None;
                    self.reconcile_local_account_selection();
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
                        | ServiceActionRequest::MappingRemove { .. }
                        | ServiceActionRequest::Funnel { .. }
                        | ServiceActionRequest::FunnelUnpublish { .. }
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

    fn apply_fresh_snapshot(&mut self, snapshot: LocalSnapshot) {
        let generation = self.local_resource.generation.saturating_add(1);
        self.local_resource.generation = generation;
        self.local_state = snapshot.backend_state.clone();
        let _ = self.local_resource.succeed(generation, snapshot);
        self.refresh_device_view();
    }

    fn invalidate_local_state(&mut self) {
        self.local_resource.snapshot = None;
        self.local_resource.status = LocalResourceStatus::NeverLoaded;
        self.local_resource.generation = self.local_resource.generation.saturating_add(1);
        self.views.devices.selected_id = None;
        self.views.devices.scroll = 0;
        self.local_capabilities = LocalCapabilities::default();
        self.services_snapshot = LocalServicesSnapshot::new();
        self.alpha_local_features = false;
        self.local_diagnostics.clear();
        self.local_preferences = LocalPreferences::empty(self.now);
        self.system_policy.clear();
        self.system_policy_failure = None;
        self.refresh_device_view();
    }

    fn start_account_rediscovery(&mut self) -> Vec<Effect> {
        if self.local_executable.is_none() {
            return Vec::new();
        }
        self.local_discovery_generation = self.local_discovery_generation.saturating_add(1);
        self.local_discovery_in_flight = true;
        vec![Effect::StartLocalDiscovery {
            generation: self.local_discovery_generation,
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
                (Some(left), Some(right)) => {
                    compare_devices_by_specs(left, right, &sort_terms, self.now)
                }
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

    fn device_sort_terms(&self) -> Vec<SortSpec> {
        if self.views.devices.sort_terms.is_empty()
            || self.views.devices.sort_terms.first() != Some(&self.views.devices.sort)
        {
            vec![self.views.devices.sort]
        } else {
            self.views.devices.sort_terms.clone()
        }
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
        let length = self.filtered_admin_users().len();
        self.admin_user_selected = move_bounded_index(self.admin_user_selected, length, offset);
    }

    fn current_collection_selection(&self) -> usize {
        match self.current_route() {
            Route::Users => self.admin_user_selected,
            Route::Routes => self.admin_route_selected,
            Route::Credentials => self.admin_credential_selected,
            Route::Audit => self.admin_activity_selected,
            _ => 0,
        }
    }

    fn set_simple_collection_filter(&mut self, filter: String, selection: usize) {
        match self.current_route() {
            Route::Users => {
                self.views.users.filter = filter;
                self.admin_user_selected = selection;
            }
            Route::Routes => {
                self.views.routes.filter = filter;
                self.admin_route_selected = selection;
            }
            Route::Credentials => {
                self.views.credentials.filter = filter;
                self.admin_credential_selected = selection;
            }
            Route::Audit => {
                self.views.audit.filter = filter;
                self.admin_activity_selected = selection;
            }
            _ => {}
        }
    }

    fn move_device_detail_scroll(&mut self, offset: isize) {
        let length = self.device_detail_max_scroll().saturating_add(1);
        let current = self
            .views
            .devices
            .detail_scroll
            .min(length.saturating_sub(1));
        self.views.devices.detail_scroll = move_bounded_index(current, length, offset);
    }

    fn move_access_scroll(&mut self, offset: isize) {
        let length = self.access_max_scroll().saturating_add(1);
        let current = self.detail_scroll.min(length.saturating_sub(1));
        self.detail_scroll = move_bounded_index(current, length, offset);
    }

    fn access_max_scroll(&self) -> usize {
        let frame = crate::ui::layout::compute(
            ratatui::layout::Rect {
                x: 0,
                y: 0,
                width: self.terminal_width,
                height: self.terminal_height,
            },
            self,
        );
        crate::ui::views::access::max_scroll(self, frame.content.height)
    }

    fn device_detail_max_scroll(&self) -> usize {
        let frame = crate::ui::layout::compute(
            ratatui::layout::Rect {
                x: 0,
                y: 0,
                width: self.terminal_width,
                height: self.terminal_height,
            },
            self,
        );
        crate::ui::components::inspector::device_detail_max_scroll(self, frame.content.height)
    }

    fn clamp_device_detail_scroll(&mut self) {
        if self.current_route() != Route::Devices || self.focus != Focus::Inspector {
            return;
        }
        self.views.devices.detail_scroll = self
            .views
            .devices
            .detail_scroll
            .min(self.device_detail_max_scroll());
    }

    fn reset_device_detail_state(&mut self) {
        self.views.devices.detail_scroll = 0;
        self.views.devices.detail_search.clear();
        self.views.devices.detail_search_match = None;
    }

    fn update_detail_search_preview(&mut self) {
        let (route, input, initial_scroll) = match &self.interaction {
            InteractionMode::FilterLine(FilterLineState {
                editor,
                purpose: FilterLinePurpose::DetailSearch { route, scroll, .. },
                ..
            }) => (*route, editor.input.trim().to_owned(), *scroll),
            _ => return,
        };
        if route != Route::Devices {
            self.detail_search = input;
            if route == Route::Access {
                let matches = crate::ui::views::access::search_matches(self, &self.detail_search);
                if self.detail_search.is_empty() {
                    self.detail_search_match = None;
                    self.detail_scroll = initial_scroll.min(self.access_max_scroll());
                } else {
                    let matched = matches
                        .iter()
                        .copied()
                        .find(|line| *line >= initial_scroll)
                        .or_else(|| matches.first().copied());
                    self.detail_search_match = matched;
                    if let Some(line) = matched {
                        self.detail_scroll = line.min(self.access_max_scroll());
                    }
                    if let InteractionMode::FilterLine(state) = &mut self.interaction {
                        state.error = matched.is_none().then(|| FilterErrorReport {
                            message: "No matches in this policy".to_owned(),
                            expected: "plain text".to_owned(),
                        });
                    }
                    return;
                }
            }
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
            }
            return;
        }
        self.views.devices.detail_search = input;
        let matches = crate::ui::components::inspector::device_detail_search_matches(
            self,
            &self.views.devices.detail_search,
        );
        if self.views.devices.detail_search.is_empty() {
            self.views.devices.detail_search_match = None;
            self.views.devices.detail_scroll = initial_scroll.min(self.device_detail_max_scroll());
            if let InteractionMode::FilterLine(state) = &mut self.interaction {
                state.error = None;
            }
            return;
        }
        let matched = matches
            .iter()
            .copied()
            .find(|line| *line >= initial_scroll)
            .or_else(|| matches.first().copied());
        self.views.devices.detail_search_match = matched;
        if let Some(line) = matched {
            self.views.devices.detail_scroll = line.min(self.device_detail_max_scroll());
        }
        if let InteractionMode::FilterLine(state) = &mut self.interaction {
            state.error = matched.is_none().then(|| FilterErrorReport {
                message: "No matches in this device record".to_owned(),
                expected: "plain text".to_owned(),
            });
        }
    }

    fn move_detail_search_match(&mut self, backwards: bool) {
        if self.current_route() == Route::Access {
            let matches = crate::ui::views::access::search_matches(self, &self.detail_search);
            let Some(next) = next_search_match(&matches, self.detail_search_match, backwards)
            else {
                self.runtime_error = Some("search the policy with / first".to_owned());
                return;
            };
            self.detail_search_match = Some(next);
            self.detail_scroll = next.min(self.access_max_scroll());
            return;
        }
        let matches = crate::ui::components::inspector::device_detail_search_matches(
            self,
            &self.views.devices.detail_search,
        );
        let Some(next) =
            next_search_match(&matches, self.views.devices.detail_search_match, backwards)
        else {
            self.runtime_error = Some("search device details with / first".to_owned());
            return;
        };
        self.views.devices.detail_search_match = Some(next);
        self.views.devices.detail_scroll = next.min(self.device_detail_max_scroll());
    }

    fn move_admin_route_selection(&mut self, offset: isize) {
        let length = self.filtered_admin_routes().len();
        self.admin_route_selected = move_bounded_index(self.admin_route_selected, length, offset);
    }

    fn move_admin_credential_selection(&mut self, offset: isize) {
        let length = self.filtered_admin_credentials().len();
        self.admin_credential_selected =
            move_bounded_index(self.admin_credential_selected, length, offset);
    }

    pub fn selected_admin_user(&self) -> Option<&crate::domain::user::AdminUser> {
        self.filtered_admin_users()
            .get(self.admin_user_selected)
            .copied()
    }

    pub fn filtered_admin_users(&self) -> Vec<&crate::domain::user::AdminUser> {
        let query = self.views.users.filter.trim();
        self.admin
            .users
            .snapshot
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|user| {
                query.is_empty()
                    || [
                        Some(user.id.as_str()),
                        user.display_name.as_deref(),
                        user.login_name.as_deref(),
                        user.role.as_deref(),
                        user.status.as_deref(),
                        user.relation_type.as_deref(),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|value| filter::fuzzy_matches(value, query))
            })
            .collect()
    }

    pub fn filtered_admin_routes(&self) -> Vec<crate::admin::routes::AdminRouteObservation> {
        let query = self.views.routes.filter.trim();
        self.admin
            .route_observations()
            .into_iter()
            .filter(|route| {
                query.is_empty()
                    || [route.device_id.as_str(), route_role_label(route)]
                        .into_iter()
                        .chain(route.advertised.iter().map(String::as_str))
                        .chain(route.enabled.iter().map(String::as_str))
                        .any(|value| filter::fuzzy_matches(value, query))
            })
            .collect()
    }

    pub fn filtered_admin_credentials(
        &self,
    ) -> Vec<&crate::domain::credential::CredentialMetadata> {
        let query = self.views.credentials.filter.trim();
        self.admin
            .credentials
            .snapshot
            .as_ref()
            .map_or(&[][..], |snapshot| snapshot.records.as_slice())
            .iter()
            .filter(|credential| {
                query.is_empty()
                    || [
                        Some(credential.id.as_str()),
                        Some(credential.key_type.as_str()),
                        credential.description.as_deref(),
                        credential.user_id.as_deref(),
                    ]
                    .into_iter()
                    .flatten()
                    .chain(credential.scopes.iter().map(String::as_str))
                    .chain(credential.tags.iter().map(String::as_str))
                    .any(|value| filter::fuzzy_matches(value, query))
            })
            .collect()
    }

    pub fn selected_admin_credential_for_view(
        &self,
    ) -> Option<&crate::domain::credential::CredentialMetadata> {
        self.filtered_admin_credentials()
            .get(self.admin_credential_selected)
            .copied()
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
        self.filtered_admin_routes()
            .into_iter()
            .nth(self.admin_route_selected)
    }

    pub fn selected_admin_route_for_view(
        &self,
    ) -> Option<crate::admin::routes::AdminRouteObservation> {
        self.selected_admin_route()
    }

    fn move_admin_activity_selection(&mut self, offset: isize) {
        let length = self.audit_event_count();
        self.admin_activity_selected =
            move_bounded_index(self.admin_activity_selected, length, offset);
    }

    pub fn audit_event_count(&self) -> usize {
        self.filtered_audit_events().len()
    }

    fn selected_admin_activity(&self) -> Option<&crate::domain::activity::AuditEvent> {
        self.filtered_audit_events()
            .into_iter()
            .nth(self.admin_activity_selected)
    }

    pub fn filtered_audit_events(&self) -> Vec<&crate::domain::activity::AuditEvent> {
        let query = self.views.audit.filter.trim();
        self.admin
            .activity
            .snapshot
            .as_ref()
            .map_or_else(Vec::new, |snapshot| {
                snapshot.filtered_events(&self.audit_filters)
            })
            .into_iter()
            .filter(|event| {
                query.is_empty()
                    || [
                        event.event_type.as_deref(),
                        event.action.as_deref(),
                        event.origin.as_deref(),
                        event.action_details.as_deref(),
                        event.error.as_deref(),
                        event
                            .actor
                            .as_ref()
                            .and_then(|actor| actor.display.as_deref()),
                        event.actor.as_ref().and_then(|actor| actor.id.as_deref()),
                        event
                            .target
                            .as_ref()
                            .and_then(|target| target.display.as_deref()),
                        event
                            .target
                            .as_ref()
                            .and_then(|target| target.id.as_deref()),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|value| filter::fuzzy_matches(value, query))
            })
            .collect()
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
                    self.views.credentials.filter.clear();
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
                self.views.users.filter.clear();
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
                    self.reset_device_detail_state();
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
                self.views.credentials.filter.clear();
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
        self.reset_device_detail_state();
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

    /// Sort as two independent one-key decisions rather than a list of every
    /// field-and-direction pair.
    /// Field and direction in one mnemonic: the field key names the column, the
    /// second key names the order.
    /// The mapping table's own columns. Ascending and descending live behind
    /// the field key, the same two-key shape the device sort uses.
    fn service_sort_choices(&self) -> Vec<MenuChoice> {
        let current = self.views.services.sort;
        ServiceSortField::ALL
            .into_iter()
            .flat_map(|field| {
                [
                    (SortDirection::Ascending, 'a', "ascending"),
                    (SortDirection::Descending, 'd', "descending"),
                ]
                .into_iter()
                .map(move |(direction, order, label)| MenuChoice {
                    sequence: format!("{}{order}", field.key()),
                    group: "Column".to_owned(),
                    subject: field.label().to_owned(),
                    label: label.to_owned(),
                    active: current.field == field && current.direction == direction,
                    outcome: ChoiceOutcome::ServiceSort(ServiceSortSpec { field, direction }),
                })
            })
            .collect()
    }

    fn profile_sort_choices(&self) -> Vec<MenuChoice> {
        let current = self.views.profiles.sort;
        ProfileSortField::ALL
            .into_iter()
            .flat_map(|field| {
                [
                    (SortDirection::Ascending, 'a', "ascending"),
                    (SortDirection::Descending, 'd', "descending"),
                ]
                .into_iter()
                .map(move |(direction, order, label)| MenuChoice {
                    sequence: format!("{}{order}", field.key()),
                    group: "Column".to_owned(),
                    subject: field.label().to_owned(),
                    label: label.to_owned(),
                    active: current.field == field && current.direction == direction,
                    outcome: ChoiceOutcome::ProfileSort(ProfileSortSpec { field, direction }),
                })
            })
            .collect()
    }

    fn config_sort_choices(&self) -> Vec<MenuChoice> {
        let current = self.views.config.sort;
        SettingSortField::ALL
            .into_iter()
            .flat_map(|field| {
                [
                    (SortDirection::Ascending, 'a', "ascending"),
                    (SortDirection::Descending, 'd', "descending"),
                ]
                .into_iter()
                .map(move |(direction, order, label)| MenuChoice {
                    sequence: format!("{}{order}", field.key()),
                    group: "Column".to_owned(),
                    subject: field.label().to_owned(),
                    label: label.to_owned(),
                    active: current.field == field && current.direction == direction,
                    outcome: ChoiceOutcome::ConfigSort(SettingSortSpec { field, direction }),
                })
            })
            .collect()
    }

    fn sort_choices(&self) -> Vec<MenuChoice> {
        if self.current_route() == Route::Services {
            return self.service_sort_choices();
        }
        if self.current_route() == Route::Profiles {
            return self.profile_sort_choices();
        }
        if self.current_route() == Route::Config {
            return self.config_sort_choices();
        }
        const FIELDS: [(char, SortField, &str, &str); 10] = [
            ('n', SortField::Name, "Identity", "name"),
            ('i', SortField::DeviceId, "Identity", "id"),
            ('w', SortField::Owner, "Identity", "owner"),
            ('s', SortField::Liveness, "Connection", "state"),
            ('p', SortField::Path, "Connection", "path"),
            ('t', SortField::LastSeen, "Connection", "last seen"),
            ('o', SortField::Os, "Platform", "os"),
            ('v', SortField::Version, "Platform", "version"),
            ('c', SortField::Rx, "Traffic", "received"),
            ('m', SortField::Tx, "Traffic", "transmitted"),
        ];
        let current = self.views.devices.sort;
        FIELDS
            .into_iter()
            .flat_map(|(key, field, group, subject)| {
                [
                    (SortDirection::Ascending, 'a', "ascending"),
                    (SortDirection::Descending, 'd', "descending"),
                ]
                .into_iter()
                .map(move |(direction, order, label)| MenuChoice {
                    sequence: format!("{key}{order}"),
                    group: group.to_owned(),
                    subject: subject.to_owned(),
                    label: label.to_owned(),
                    active: current.field == field && current.direction == direction,
                    outcome: ChoiceOutcome::Sort(SortSpec { field, direction }),
                })
            })
            .collect()
    }

    fn apply_choice(&mut self, outcome: ChoiceOutcome) -> Vec<Effect> {
        match outcome {
            ChoiceOutcome::Sort(sort) => {
                self.set_sort(sort);
                Vec::new()
            }
            ChoiceOutcome::ServiceSort(sort) => {
                self.views.services.sort = sort;
                self.views.services.selected = 0;
                self.views.services.scroll = 0;
                Vec::new()
            }
            ChoiceOutcome::ProfileSort(sort) => {
                self.views.profiles.sort = sort;
                self.views.profiles.selected = 0;
                Vec::new()
            }
            ChoiceOutcome::ConfigSort(sort) => {
                self.views.config.sort = sort;
                self.views.config.selected = 0;
                Vec::new()
            }
        }
    }

    fn set_sort(&mut self, sort: SortSpec) {
        self.views.devices.sort = sort;
        self.views.devices.sort_terms = vec![sort];
        self.reconcile_selection(None);
    }

    fn copy_field(&mut self, field: CopyField) -> Vec<Effect> {
        if field == CopyField::DiagnosticSummary {
            let value = self.diagnostic_summary();
            return self.copy_text(value);
        }
        if field == CopyField::Metrics {
            let value = self
                .services_snapshot
                .metrics
                .value
                .as_ref()
                .map_or_else(String::new, |metrics| metrics.text.clone());
            return self.copy_text(value);
        }
        if matches!(
            field,
            CopyField::ServiceUrl | CopyField::ServiceListener | CopyField::ServiceBackend
        ) {
            let Some(mapping) = self.selected_service_mapping() else {
                return Vec::new();
            };
            let value = match field {
                CopyField::ServiceListener => {
                    format!("{}:{}", mapping.listener.label(), mapping.listener.port())
                }
                CopyField::ServiceBackend => mapping.backend.argument(),
                _ => self.service_url(&mapping),
            };
            return self.copy_text(value);
        }
        if matches!(
            field,
            CopyField::ConfigSetting | CopyField::ConfigValue | CopyField::ConfigSource
        ) {
            let Some(row) = self.selected_config_row() else {
                return Vec::new();
            };
            let value = match field {
                CopyField::ConfigSetting => row.name.to_owned(),
                CopyField::ConfigValue => row.value,
                CopyField::ConfigSource => row.source.label().to_owned(),
                _ => return Vec::new(),
            };
            return self.copy_text(value);
        }
        if matches!(
            field,
            CopyField::UserId | CopyField::UserName | CopyField::UserLogin
        ) {
            let Some(user) = self.selected_admin_user() else {
                return Vec::new();
            };
            let value = match field {
                CopyField::UserName => user.display_name.clone(),
                CopyField::UserLogin => user.login_name.clone(),
                _ => Some(user.id.clone()),
            };
            // The menu only offers a field the API reported, so a missing one
            // here means the selection moved: copy nothing rather than a lie.
            let Some(value) = value else {
                return Vec::new();
            };
            return self.copy_text(value);
        }
        if matches!(
            field,
            CopyField::TaskId
                | CopyField::TaskResult
                | CopyField::TaskCommand
                | CopyField::TaskOutput
        ) {
            let Some(task) = self.focused_task() else {
                return Vec::new();
            };
            let value = match field {
                CopyField::TaskResult => task.summary.clone(),
                CopyField::TaskCommand => task.redacted_argv.join(" "),
                CopyField::TaskOutput => task.detail.clone(),
                _ => task.id.to_string(),
            };
            return self.copy_text(value);
        }
        if matches!(
            field,
            CopyField::ProfileName
                | CopyField::ProfileTailnet
                | CopyField::ProfileAccount
                | CopyField::ProfileCredential
                | CopyField::ProfileBackend
        ) {
            let Some(row) = self.selected_profile_row() else {
                return Vec::new();
            };
            let value = match (field, row) {
                (CopyField::ProfileName, row) => Some(row.label().to_owned()),
                (CopyField::ProfileTailnet, row) => row.tailnet().map(str::to_owned),
                (CopyField::ProfileAccount, ProfileRow::Local { account, .. }) => {
                    account.map(str::to_owned)
                }
                (CopyField::ProfileCredential, ProfileRow::Admin { config, .. }) => {
                    Some(config.credential.clone())
                }
                (CopyField::ProfileBackend, ProfileRow::Admin { config, .. }) => {
                    Some(config.credential_backend.location().display().to_string())
                }
                _ => None,
            };
            // The menu only offers a field the row has, so a missing one here
            // means the selection moved: copy nothing rather than a lie.
            let Some(value) = value else {
                return Vec::new();
            };
            return self.copy_text(value);
        }
        if field == CopyField::DnsName {
            let Some(value) = self.selected_dns_name() else {
                return Vec::new();
            };
            return self.copy_text(value);
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
            return self.copy_text(value);
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
            CopyField::DnsName
            | CopyField::PublicKey
            | CopyField::Endpoint
            | CopyField::DiagnosticSummary
            | CopyField::Metrics => "not returned".to_owned(),
            CopyField::ServiceUrl
            | CopyField::ServiceListener
            | CopyField::ServiceBackend
            | CopyField::UserId
            | CopyField::UserName
            | CopyField::UserLogin
            | CopyField::TaskId
            | CopyField::TaskResult
            | CopyField::TaskCommand
            | CopyField::TaskOutput
            | CopyField::ProfileName
            | CopyField::ProfileTailnet
            | CopyField::ProfileAccount
            | CopyField::ProfileCredential
            | CopyField::ProfileBackend
            | CopyField::ConfigSetting
            | CopyField::ConfigValue
            | CopyField::ConfigSource => "not returned".to_owned(),
        };
        self.copy_text(value)
    }

    /// What a mapping is reachable at: this machine's DNS name, the listener's
    /// scheme and port, and the mount path. This is the thing worth pasting
    /// somewhere, which is why it is the first entry in the copy menu.
    fn service_url(&self, mapping: &ServiceMapping) -> String {
        // The first eligible certificate domain is exactly the name a Serve
        // mapping answers on, which is why the client offers it at all.
        let host = self
            .services_snapshot
            .certificate_domains
            .value
            .as_ref()
            .and_then(|domains| domains.first())
            .map(String::as_str)
            .unwrap_or("this-machine");
        let host = host.trim_end_matches('.');
        let scheme = match mapping.listener {
            Listener::Https(_) | Listener::TlsTerminatedTcp(_) => "https",
            Listener::Http(_) => "http",
            Listener::Tcp(_) => "tcp",
        };
        let port = mapping.listener.port();
        let path = mapping.mount.as_path();
        let path = if path == "/" { "" } else { path };
        format!("{scheme}://{host}:{port}{path}")
    }

    /// Says which command is missing, where Tale looked, and what to do. The
    /// discovery failure already knows all three; only the wording is new here.
    fn missing_executable_reason(&self) -> String {
        use crate::domain::source::LocalCliState;
        match &self.local_cli_state {
            LocalCliState::Discovering => "still looking for the tailscale command".to_owned(),
            LocalCliState::Disabled => {
                "local access is off for this run; restart without --no-local".to_owned()
            }
            LocalCliState::Mock => "simulated data has no local command".to_owned(),
            LocalCliState::Available => {
                "the tailscale command is available but this action could not use it".to_owned()
            }
            LocalCliState::Unsupported { detail }
            | LocalCliState::Unavailable { detail }
            | LocalCliState::Missing { detail }
            | LocalCliState::PermissionDenied { detail } => detail.clone(),
        }
    }

    /// The status bar reports the text, so simulated copies report it too:
    /// `--mock` must show the same sentence the real clipboard produces.
    fn copy_text(&mut self, text: String) -> Vec<Effect> {
        if self.source_mode == SourceMode::Mock {
            self.copied_value = Some(text);
            Vec::new()
        } else {
            vec![Effect::CopyText { text }]
        }
    }

    /// Whether the side inspector shares the content pane. The two table
    /// routes hold it behind `i` — the table is what they are for; everywhere
    /// else the inspector is the route.
    pub fn inspector_pane_visible(&self) -> bool {
        match self.current_route() {
            Route::Devices => self.views.devices.inspector,
            Route::Users => self.views.users.inspector,
            Route::Tasks => self.views.tasks.inspector,
            Route::Profiles => self.views.profiles.inspector,
            Route::Routes => self.views.routes.inspector,
            Route::Credentials => self.views.credentials.inspector,
            Route::Audit => self.views.audit.inspector,
            Route::Services => self.views.services.inspector,
            _ => true,
        }
    }

    pub fn footer_actions(&self, width: u16) -> Vec<action::FooterHint> {
        action::footer_actions_filtered(self.action_context(), self.current_route(), width, |id| {
            self.footer_action_is_relevant(id)
        })
    }

    pub fn active_detail_search(&self) -> &str {
        if self.current_route() == Route::Devices {
            &self.views.devices.detail_search
        } else {
            &self.detail_search
        }
    }

    pub fn footer_action_is_relevant(&self, id: ActionId) -> bool {
        if !action::applies_to_route(id, self.current_route())
            || self.action_unavailable_reason(id).is_some()
        {
            return false;
        }
        match id {
            ActionId::CollectionMoveUp
            | ActionId::CollectionMoveDown
            | ActionId::CollectionFirst
            | ActionId::CollectionLast
            | ActionId::CollectionPageUp
            | ActionId::CollectionPageDown
            | ActionId::CollectionOpen
            | ActionId::CollectionSort
            | ActionId::CollectionInspect => self.collection_subject_available(),
            ActionId::CollectionBack => self.focus == Focus::Inspector,
            ActionId::TaskCancel => self.tasks.selected_can_cancel(),
            _ => true,
        }
    }

    fn collection_subject_available(&self) -> bool {
        match self.current_route() {
            Route::Overview => self.selected_overview_finding().is_some(),
            Route::Local => self.selected_local_account().is_some(),
            Route::Devices => self.selected_device().is_some(),
            Route::Users => self.selected_admin_user().is_some(),
            Route::Routes => self.selected_admin_route().is_some(),
            Route::Credentials => self.selected_credential().is_some(),
            Route::Profiles => self.selected_profile_row().is_some(),
            Route::Config => self.selected_config_row().is_some(),
            Route::Tasks => self.tasks.selected.is_some(),
            Route::Audit => self.selected_admin_activity().is_some(),
            Route::Services => self.service_inspector_available(),
            _ => true,
        }
    }

    pub fn selected_device(&self) -> Option<&Device> {
        let id = self.views.devices.selected_id.as_ref()?;
        self.devices_resource
            .snapshot
            .iter()
            .find(|device| &device.id == id)
    }

    pub fn selected_overview_finding(&self) -> Option<&Finding> {
        let selected = self.views.overview.selected_id.as_deref();
        selected
            .and_then(|id| self.health_findings.iter().find(|finding| finding.id == id))
            .or_else(|| self.health_findings.first())
    }

    fn move_overview_selection(&mut self, offset: isize) {
        if self.health_findings.is_empty() {
            self.views.overview.selected_id = None;
            return;
        }
        let current = self
            .views
            .overview
            .selected_id
            .as_deref()
            .and_then(|id| {
                self.health_findings
                    .iter()
                    .position(|finding| finding.id == id)
            })
            .map_or(0, |position| position);
        let next = move_bounded_index(current, self.health_findings.len(), offset);
        self.views.overview.selected_id = self
            .health_findings
            .get(next)
            .map(|finding| finding.id.clone());
    }

    fn select_overview_position(&mut self, position: usize) {
        let index = if position == usize::MAX {
            self.health_findings.len().saturating_sub(1)
        } else {
            position.min(self.health_findings.len().saturating_sub(1))
        };
        self.views.overview.selected_id = self
            .health_findings
            .get(index)
            .map(|finding| finding.id.clone());
    }

    fn reconcile_overview_selection(&mut self) {
        if self
            .views
            .overview
            .selected_id
            .as_deref()
            .is_some_and(|id| self.health_findings.iter().any(|finding| finding.id == id))
        {
            return;
        }
        self.views.overview.selected_id = self
            .health_findings
            .first()
            .map(|finding| finding.id.clone());
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

    pub fn filtered_task_count(&self) -> usize {
        self.tasks.filtered(&self.task_filter).count()
    }

    /// Where the selection sits in the filtered history. The table and the
    /// mouse both size their window from this, so a click lands on the row the
    /// pointer is over rather than on the one the list happens to start with.
    pub fn task_cursor(&self) -> usize {
        let Some(selected) = self.tasks.selected else {
            return 0;
        };
        self.tasks
            .filtered(&self.task_filter)
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
            let label_length = u32::try_from(label.chars().count()).map_or(u32::MAX, |value| value);
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
        .map_or(usize::MAX, |index| index)
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
        .map_or((cursor, cursor), |span| span)
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
            let length = u32::try_from(suggestion.text.chars().count()).map_or(u32::MAX, |v| v);
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
            | ActionId::ServicesServeRemove
            | ActionId::ServicesServeReset
            | ActionId::ServicesFunnelCreate
            | ActionId::ServicesFunnelEdit
            | ActionId::ServicesFunnelUnpublish
            | ActionId::ServicesFunnelReset
            | ActionId::DevicesTaildropSend
            | ActionId::DevicesTaildropReceive
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
        ActionId::ProfileActivate
            | ActionId::AdminRefreshCurrent
            | ActionId::AdminRefreshAll
            | ActionId::ViewProfiles
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

fn is_service_write_action(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::ServicesServeCreate
            | ActionId::ServicesServeEdit
            | ActionId::ServicesServeRemove
            | ActionId::ServicesServeReset
            | ActionId::ServicesFunnelCreate
            | ActionId::ServicesFunnelEdit
            | ActionId::ServicesFunnelUnpublish
            | ActionId::ServicesFunnelReset
            | ActionId::DevicesTaildropSend
            | ActionId::DevicesTaildropReceive
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

fn is_local_service_action(action_id: ActionId) -> bool {
    matches!(
        action_id,
        ActionId::ServicesServeRefresh
            | ActionId::ServicesServeCreate
            | ActionId::ServicesServeEdit
            | ActionId::ServicesServeRemove
            | ActionId::ServicesServeReset
            | ActionId::ServicesFunnelCreate
            | ActionId::ServicesFunnelEdit
            | ActionId::ServicesFunnelUnpublish
            | ActionId::ServicesFunnelReset
            | ActionId::DevicesTaildropSend
            | ActionId::DevicesTaildropReceive
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

fn is_local_operator_action(action_id: ActionId) -> bool {
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
