use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::{self, ActionContext, ActionId, Capability};
use crate::config::ResolvedConfig;
use crate::domain::account::LocalAccount;
use crate::domain::certificate::{BugReportRequest, CertificateRequest};
use crate::domain::device::{
    Device, DeviceId, LocalDevice, SortDirection, SortField, SortSpec, compare_devices,
};
use crate::domain::diagnostic::{DiagnosticResult, DiagnosticState};
use crate::domain::filter::{self, FilterExpression};
use crate::domain::mutation::{LocalMutation, MutationLock};
use crate::domain::preference::{
    LocalPreferences, ObservedPreference, PreferenceEditability, PreferenceField, PreferenceRequest,
};
use crate::domain::redaction::{DiagnosticReportInput, redact_diagnostic_report};
use crate::domain::route::{
    AdvertisementRequest, ExitNodeCandidate, ExitNodeRequest, ExitNodeSelection,
    overlapping_routes, parse_route_set, parse_static_endpoints,
};
use crate::domain::service::{
    Backend, CertificateVerification, Exposure, Listener, LocalServicesSnapshot, PathMount, Port,
    ProxyProtocol, ServiceActionRequest, ServiceCapabilities, ServiceConflictKey,
    ServiceFailureKind, ServiceMapping, ServiceResourceStatus, ServiceSection, ServiceTaskData,
};
use crate::domain::source::{
    LocalCapabilities, LocalExecutable, LocalFailure, LocalFailureKind, LocalResource,
    LocalResourceStatus, LocalSnapshot, LocalState,
};
use crate::domain::transfer::{
    TaildriveShare, TaildropConflict, TaildropReceiveRequest, TaildropSendRequest, TaildropTarget,
    normalize_share_name, validate_receive_directory, validate_regular_file,
};
use crate::domain::{SourceHealth, Timestamp};
use crate::effect::{Effect, Resource};
use crate::event::{
    Event, InputEvent, LocalEvent, ServicesEvent, ShutdownReason, SourceEvent, TaskEvent,
};
use crate::local::client::{ExecutableResolution, HostPlatform};
use crate::local::diagnostics::{self, DiagnosticRequest};
use crate::local::handoff::{self, HandoffCommand};
use crate::local::policy::SystemPolicyEntry;
use crate::local::{certificates, services, transfers};
use crate::mock::{MOCK_NOW, MockLoadScenario, MockTaskBehavior};
use crate::task::{Notification, TaskId, TaskState, TaskStore};

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
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Route {
    Overview,
    Local,
    Devices,
    Dns,
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
            Self::Dns => "dns",
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
            "dns" => Some(Self::Dns),
            "activity" | "tasks" => Some(Self::Activity),
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
pub struct CommandPaletteState {
    pub input: String,
    pub candidates: Vec<Route>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilterEditorState {
    pub input: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HelpState {
    pub searchable: bool,
    pub query: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ActionPickerState {
    pub actions: Vec<ActionId>,
    pub selected: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CopyPickerState {
    pub fields: Vec<CopyField>,
    pub selected: usize,
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
    pub service_request: Option<ServiceActionRequest>,
    pub handoff: Option<HandoffCommand>,
    pub prompt: String,
    pub required_phrase: Option<String>,
    pub input: String,
    pub lose_ssh_checked: bool,
    pub preview_lines: Vec<String>,
    pub redacted_argv: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OperatorFormState {
    pub action_id: ActionId,
    pub input: String,
    pub error: Option<String>,
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
    CommandPalette(CommandPaletteState),
    FilterEditor(FilterEditorState),
    Help(HelpState),
    ActionPicker(ActionPickerState),
    CopyPicker(CopyPickerState),
    QuitConfirmation,
    TaskInspector(TaskId),
    SortPicker { selected: usize },
    DiagnosticInput(DiagnosticInputState),
    Confirmation(Box<ConfirmationState>),
    OperatorForm(OperatorFormState),
    ServiceForm(ServiceFormState),
    ServiceSectionPicker(ServiceSectionPickerState),
    AccountPicker(AccountPickerState),
    HandoffInput(HandoffInputState),
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
    pub wide_columns: bool,
}

impl Default for DeviceViewState {
    fn default() -> Self {
        Self {
            selected_id: None,
            scroll: 0,
            filter_draft: String::new(),
            applied_filter: FilterExpression::empty(),
            sort: SortSpec::default(),
            wide_columns: false,
        }
    }
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

#[derive(Debug, Clone)]
pub struct App {
    pub route_stack: Vec<Route>,
    pub focus: Focus,
    pub overlays: Vec<Overlay>,
    pub views: Views,
    pub devices_resource: DeviceResource,
    pub local_resource: LocalResource,
    pub local_state: LocalState,
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
    pub interactive_handoff_active: bool,
    render_invalidated: bool,
    local_discovery_in_flight: bool,
    local_status_in_flight: bool,
    local_services_refresh_in_flight: bool,
    local_next_refresh: Option<Instant>,
    local_last_tick: Option<Instant>,
    next_mutation_id: u64,
}

impl App {
    pub fn new(config: ResolvedConfig) -> Self {
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
        Self {
            route_stack: vec![Route::Overview],
            focus: Focus::Collection,
            overlays: Vec::new(),
            views: Views {
                devices: DeviceViewState::default(),
                services: ServiceViewState::default(),
            },
            devices_resource: DeviceResource::empty(source_mode),
            local_resource: LocalResource::new(),
            local_state,
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
            runtime_error: None,
            copied_value: None,
            mutation_lock: MutationLock::new(),
            mutation_in_flight: None,
            interactive_handoff_active: false,
            render_invalidated: true,
            local_discovery_in_flight: false,
            local_status_in_flight: false,
            local_services_refresh_in_flight: false,
            local_next_refresh: None,
            local_last_tick: None,
            next_mutation_id: 1,
        }
    }

    pub fn bootstrap_effects(&mut self) -> Vec<Effect> {
        match self.source_mode {
            SourceMode::Unavailable => return Vec::new(),
            SourceMode::Mock => {
                self.devices_resource.generation = 1;
                return vec![Effect::StartMockLoad {
                    resource: Resource::Devices,
                    generation: 1,
                    scenario: MockLoadScenario::Initial,
                }];
            }
            SourceMode::Local => {}
        }
        self.local_resource.generation = 1;
        self.local_resource.begin(1, self.now);
        self.local_discovery_in_flight = true;
        self.local_next_refresh = Some(instant_after(
            Instant::now(),
            self.resolved_config.local.refresh_interval,
        ));
        vec![Effect::StartLocalDiscovery {
            generation: 1,
            resolution: local_resolution(&self.resolved_config),
            timeout: self.resolved_config.local.command_timeout,
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
            Event::ShutdownRequested(reason) => self.request_shutdown(reason),
        }
    }

    fn update_tick(&mut self, tick: Instant) -> Vec<Effect> {
        self.tick_count = self.tick_count.saturating_add(1);
        self.now = if self.source_mode == SourceMode::Mock {
            MOCK_NOW.saturating_add(self.tick_count)
        } else {
            crate::local::now()
        };
        self.local_last_tick = Some(tick);
        self.notifications
            .retain(|notification| notification.expires_at > self.now);
        if self.tasks.has_active() {
            self.render_invalidated = true;
        }
        if self.source_mode == SourceMode::Local
            && !self.interactive_handoff_active
            && !self.local_discovery_in_flight
            && !self.local_status_in_flight
            && self.overlays.is_empty()
            && self.local_next_refresh.is_some_and(|due| tick >= due)
        {
            return self.start_refresh(false);
        }
        Vec::new()
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
            InputEvent::Paste(text) => self.handle_paste(&text),
            InputEvent::FocusGained | InputEvent::FocusLost => Vec::new(),
            InputEvent::Key(key) => self.handle_key(key),
        }
    }

    fn handle_paste(&mut self, text: &str) -> Vec<Effect> {
        let Some(overlay) = self.overlays.last_mut() else {
            return Vec::new();
        };
        match overlay {
            Overlay::CommandPalette(state) => {
                state.input.push_str(text);
                state.candidates = route_candidates(&state.input);
            }
            Overlay::FilterEditor(state) => {
                state.input.push_str(text);
                state.error = None;
            }
            Overlay::Help(state) if state.searchable => state.query.push_str(text),
            Overlay::DiagnosticInput(state) => state.input.push_str(text),
            Overlay::OperatorForm(state) => {
                state.input.push_str(text);
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
            _ => {}
        }
        Vec::new()
    }

    fn handle_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if let Some(effect) = self.handle_text_key(key) {
            return effect;
        }
        if key.code == KeyCode::Esc {
            self.pop_overlay_or_back();
            return Vec::new();
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(effect) = self.cancel_focused_task() {
                return vec![effect];
            }
            return self.request_shutdown(ShutdownReason::UserQuit);
        }
        if key.code == KeyCode::Char('q') && key.modifiers.is_empty() {
            return self.handle_quit_key();
        }

        if !self.overlays.is_empty() {
            return self.handle_overlay_key(key);
        }

        let context = match self.current_route() {
            Route::Activity => ActionContext::Activity,
            Route::Devices if self.focus == Focus::Inspector => ActionContext::Detail,
            Route::Devices => ActionContext::Collection,
            Route::Services if self.focus == Focus::Inspector => ActionContext::Detail,
            Route::Services => ActionContext::Collection,
            _ => ActionContext::Root,
        };
        let Some(action_id) = action::action_for_key(key, context) else {
            return Vec::new();
        };
        self.dispatch_action(action_id)
    }

    fn handle_text_key(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        let overlay = self.overlays.last_mut()?;
        match overlay {
            Overlay::CommandPalette(state) => {
                match key.code {
                    KeyCode::Char(character) if key.modifiers.is_empty() => {
                        state.input.push(character);
                        state.candidates = route_candidates(&state.input);
                    }
                    KeyCode::Backspace => {
                        let _ = state.input.pop();
                        state.candidates = route_candidates(&state.input);
                    }
                    KeyCode::Enter => {
                        let input = state.input.clone();
                        return Some(self.accept_command(&input));
                    }
                    _ => return None,
                }
                Some(Vec::new())
            }
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
            Overlay::FilterEditor(state) => {
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
                        return Some(self.accept_filter(&input));
                    }
                    _ => return None,
                }
                Some(Vec::new())
            }
            Overlay::Help(state) if state.searchable => {
                match key.code {
                    KeyCode::Char(character) if key.modifiers.is_empty() => {
                        state.query.push(character)
                    }
                    KeyCode::Backspace => {
                        let _ = state.query.pop();
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
            Overlay::CommandPalette(state) => {
                self.overlays.push(Overlay::CommandPalette(state));
                Vec::new()
            }
            Overlay::FilterEditor(state) => {
                self.overlays.push(Overlay::FilterEditor(state));
                Vec::new()
            }
            Overlay::Help(mut state) => {
                if key.code == KeyCode::Char('?') && key.modifiers.is_empty() {
                    state.searchable = true;
                }
                self.overlays.push(Overlay::Help(state));
                Vec::new()
            }
            Overlay::ActionPicker(mut state) => {
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        state.selected =
                            (state.selected + 1).min(state.actions.len().saturating_sub(1));
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        state.selected = state.selected.saturating_sub(1)
                    }
                    KeyCode::Enter => {
                        let action_id = state.actions.get(state.selected).copied();
                        if let Some(action_id) = action_id {
                            return self.dispatch_action(action_id);
                        }
                    }
                    KeyCode::Char('?') => {
                        self.overlays.push(Overlay::ActionPicker(state.clone()));
                        self.overlays.push(Overlay::Help(HelpState {
                            searchable: false,
                            query: String::new(),
                        }));
                        return Vec::new();
                    }
                    _ => {}
                }
                self.overlays.push(Overlay::ActionPicker(state));
                Vec::new()
            }
            Overlay::CopyPicker(mut state) => {
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        state.selected =
                            (state.selected + 1).min(state.fields.len().saturating_sub(1));
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        state.selected = state.selected.saturating_sub(1)
                    }
                    KeyCode::Enter => {
                        let field = state.fields.get(state.selected).copied();
                        if let Some(field) = field {
                            self.copy_field(field);
                        }
                        return Vec::new();
                    }
                    _ => {}
                }
                self.overlays.push(Overlay::CopyPicker(state));
                Vec::new()
            }
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
                    KeyCode::Char('j') | KeyCode::Down => selected = (selected + 1).min(17),
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
        }
    }

    fn handle_quit_key(&mut self) -> Vec<Effect> {
        if !self.overlays.is_empty() {
            self.pop_overlay_or_back();
            return Vec::new();
        }
        if self.focus == Focus::Inspector {
            self.focus = Focus::Collection;
            return Vec::new();
        }
        if self.route_stack.len() > 1 {
            self.route_stack.pop();
            self.focus = Focus::Collection;
            return Vec::new();
        }
        if self.tasks.has_active() {
            self.overlays.push(Overlay::QuitConfirmation);
            Vec::new()
        } else {
            self.request_shutdown(ShutdownReason::UserQuit)
        }
    }

    fn pop_overlay_or_back(&mut self) {
        if self.overlays.pop().is_some() {
            return;
        }
        if self.focus == Focus::Inspector {
            self.focus = Focus::Collection;
        } else if self.route_stack.len() > 1 {
            self.route_stack.pop();
        }
    }

    fn accept_command(&mut self, input: &str) -> Vec<Effect> {
        let trimmed = input.trim();
        let (route_text, filter_text) =
            trimmed.split_once(' ').map_or((trimmed, ""), |parts| parts);
        let Some(route) = Route::parse(route_text) else {
            if let Some(Overlay::CommandPalette(state)) = self.overlays.last_mut() {
                state.error = Some("unknown route".to_owned());
            }
            return Vec::new();
        };
        if !filter_text.trim().is_empty() && route != Route::Devices {
            if let Some(Overlay::CommandPalette(state)) = self.overlays.last_mut() {
                state.error = Some("filters are available for devices only".to_owned());
            }
            return Vec::new();
        }
        if !filter_text.trim().is_empty() {
            match filter::parse(filter_text) {
                Ok(expression) => {
                    self.views.devices.filter_draft = filter_text.to_owned();
                    self.views.devices.applied_filter = expression;
                    self.reconcile_selection(None);
                }
                Err(error) => {
                    if let Some(Overlay::CommandPalette(state)) = self.overlays.last_mut() {
                        state.error = Some(error.to_string());
                    }
                    return Vec::new();
                }
            }
        }
        self.overlays.pop();
        self.navigate(route);
        Vec::new()
    }

    fn accept_filter(&mut self, input: &str) -> Vec<Effect> {
        match filter::parse(input) {
            Ok(expression) => {
                self.views.devices.filter_draft = input.to_owned();
                self.views.devices.applied_filter = expression;
                self.reconcile_selection(None);
                self.overlays.pop();
            }
            Err(error) => {
                if let Some(Overlay::FilterEditor(state)) = self.overlays.last_mut() {
                    state.error = Some(error.to_string());
                }
            }
        }
        Vec::new()
    }

    fn navigate(&mut self, route: Route) {
        if self.current_route() != route {
            self.route_stack.push(route);
        }
        self.focus = Focus::Collection;
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
                || (self.current_route() == Route::Activity && self.tasks.selected.is_none()))
        {
            self.runtime_error = Some("select a resource before running this action".to_owned());
            return Vec::new();
        }
        match action_id {
            ActionId::AppQuit => self.handle_quit_key(),
            ActionId::ViewCommandPalette => {
                self.overlays
                    .push(Overlay::CommandPalette(CommandPaletteState {
                        input: String::new(),
                        candidates: route_candidates(""),
                        error: None,
                    }));
                Vec::new()
            }
            ActionId::ViewFilter => {
                self.overlays.push(Overlay::FilterEditor(FilterEditorState {
                    input: self.views.devices.filter_draft.clone(),
                    error: None,
                }));
                Vec::new()
            }
            ActionId::ViewRefresh => self.start_refresh(false),
            ActionId::ViewRefreshAll => self.start_refresh(true),
            ActionId::ViewHelp => {
                self.overlays.push(Overlay::Help(HelpState {
                    searchable: false,
                    query: String::new(),
                }));
                Vec::new()
            }
            ActionId::ViewTasks => {
                self.navigate(Route::Activity);
                Vec::new()
            }
            ActionId::ViewServices => {
                self.navigate(Route::Services);
                Vec::new()
            }
            ActionId::CollectionMoveUp => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_next(-1);
                } else if self.current_route() == Route::Services {
                    self.move_service_selection(-1);
                } else {
                    self.move_selection(-1);
                }
                Vec::new()
            }
            ActionId::CollectionMoveDown => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_next(1);
                } else if self.current_route() == Route::Services {
                    self.move_service_selection(1);
                } else {
                    self.move_selection(1);
                }
                Vec::new()
            }
            ActionId::CollectionFirst => {
                if self.current_route() == Route::Activity {
                    self.tasks.selected = self.tasks.all().first().map(|task| task.id);
                } else if self.current_route() == Route::Services {
                    self.views.services.selected = 0;
                    self.views.services.scroll = 0;
                } else {
                    self.move_selection_to(0);
                }
                Vec::new()
            }
            ActionId::CollectionLast => {
                if self.current_route() == Route::Activity {
                    self.tasks.selected = self.tasks.all().last().map(|task| task.id);
                } else if self.current_route() == Route::Services {
                    self.views.services.selected = self.service_row_count().saturating_sub(1);
                    if self.views.services.section == ServiceSection::Metrics {
                        self.views.services.scroll = self.metrics_max_scroll();
                    }
                } else {
                    self.move_selection_to(usize::MAX);
                }
                Vec::new()
            }
            ActionId::CollectionPageUp => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_next(-5);
                } else if self.current_route() == Route::Services {
                    self.move_service_selection(-5);
                } else {
                    self.move_selection(-5);
                }
                Vec::new()
            }
            ActionId::CollectionPageDown => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_next(5);
                } else if self.current_route() == Route::Services {
                    self.move_service_selection(5);
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
                    self.focus = Focus::Inspector;
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
                let actions = if self.current_route() == Route::Services {
                    self.service_actions_for_section()
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
                self.overlays.push(Overlay::ActionPicker(ActionPickerState {
                    actions,
                    selected: 0,
                }));
                Vec::new()
            }
            ActionId::ResourceCopy => {
                if self.current_route() == Route::Services
                    && self.views.services.section == ServiceSection::Metrics
                {
                    self.overlays.push(Overlay::CopyPicker(CopyPickerState {
                        fields: vec![CopyField::Metrics],
                        selected: 0,
                    }));
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
                self.overlays.push(Overlay::CopyPicker(CopyPickerState {
                    fields,
                    selected: 0,
                }));
                Vec::new()
            }
            ActionId::TaskCancel => self.cancel_focused_task().into_iter().collect(),
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
                self.copy_diagnostic_summary();
                Vec::new()
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
        }
    }

    fn action_available(&self, action_id: ActionId, capability: Capability) -> bool {
        match capability {
            Capability::Available => self.local_action_available(action_id),
            Capability::MockOnly => self.source_mode == SourceMode::Mock,
            Capability::Disabled(_) => false,
        }
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
        if is_service_write_action(action_id) && self.resolved_config.read_only {
            return Some("read-only mode blocks local service mutations".to_owned());
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
        self.overlays.push(Overlay::OperatorForm(OperatorFormState {
            action_id,
            input,
            error: None,
        }));
        Vec::new()
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

    fn open_login_confirmation(&mut self) -> Vec<Effect> {
        let Some(executable) = self.local_executable.as_ref() else {
            self.runtime_error = Some("local executable has not been discovered".to_owned());
            return Vec::new();
        };
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: ActionId::LocalAccountLogin,
                mutation: None,
                service_request: None,
                handoff: Some(handoff::login_command(&executable.path)),
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
            service_request: None,
            handoff: Some(handoff::logout_command(&executable.path)),
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
                service_request: None,
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
                let redacted_argv = redacted_argv(&command.args());
                self.overlays.pop();
                self.overlays
                    .push(Overlay::Confirmation(Box::new(ConfirmationState {
                        action_id: match state.kind {
                            HandoffInputKind::Ssh => ActionId::LocalSshOpen,
                            HandoffInputKind::Nc => ActionId::LocalNcOpen,
                        },
                        mutation: None,
                        service_request: None,
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

    fn accept_confirmation(&mut self, state: ConfirmationState) -> Vec<Effect> {
        if let Some(required) = state.required_phrase.as_deref()
            && state.input != required
        {
            if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
                current.error = Some(format!("type {required} exactly to confirm"));
            }
            return Vec::new();
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

    fn start_refresh(&mut self, _all: bool) -> Vec<Effect> {
        if self.current_route() == Route::Services {
            return self.start_services_refresh();
        }
        match self.source_mode {
            SourceMode::Unavailable => {
                self.runtime_error = Some("local integration is disabled".to_owned());
                Vec::new()
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
                vec![Effect::StartMockLoad {
                    resource: Resource::Devices,
                    generation,
                    scenario,
                }]
            }
            SourceMode::Local => {
                let generation = self.local_resource.generation.saturating_add(1);
                let mut effects = Vec::new();
                if self.local_discovery_in_flight {
                    effects.push(Effect::CancelLocalDiscovery);
                }
                if self.local_status_in_flight {
                    effects.push(Effect::CancelLocalStatus);
                }
                self.local_resource.begin(generation, self.now);
                self.local_status_in_flight = false;
                self.local_discovery_in_flight = false;
                self.local_next_refresh = None;
                if let Some(executable) = self.local_executable.clone() {
                    self.local_status_in_flight = true;
                    effects.push(Effect::StartLocalStatus {
                        generation,
                        executable,
                        timeout: self.resolved_config.local.command_timeout,
                    });
                } else {
                    self.local_discovery_in_flight = true;
                    effects.push(Effect::StartLocalDiscovery {
                        generation,
                        resolution: local_resolution(&self.resolved_config),
                        timeout: self.resolved_config.local.command_timeout,
                    });
                }
                effects
            }
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
                service_request: Some(request),
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
        self.overlays.push(Overlay::ActionPicker(ActionPickerState {
            actions,
            selected: 0,
        }));
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

    fn copy_diagnostic_summary(&mut self) {
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
        self.copied_value = Some(redact_diagnostic_report(&input).text);
    }

    fn cancel_focused_task(&mut self) -> Option<Effect> {
        let id = self.tasks.selected?;
        if self.tasks.request_cancel(id) {
            Some(Effect::CancelTask { task_id: id })
        } else {
            None
        }
    }

    fn request_shutdown(&mut self, reason: ShutdownReason) -> Vec<Effect> {
        if matches!(self.shutdown_state, ShutdownState::Running) {
            self.shutdown_state = ShutdownState::Requested(reason);
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
                self.devices_resource.observed_at = Some(observed_at);
                self.devices_resource.health = if self.now.saturating_sub(observed_at) > 60 {
                    SourceHealth::Stale
                } else {
                    SourceHealth::Healthy
                };
                self.devices_resource.error = None;
                self.reconcile_selection(None);
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
                self.local_status_in_flight = true;
                self.local_resource.begin(generation, self.now);
                return vec![Effect::StartLocalStatus {
                    generation,
                    executable,
                    timeout: self.resolved_config.local.command_timeout,
                }];
            }
            LocalEvent::DiscoveryFailed {
                generation,
                failure,
            } => {
                if generation < self.local_resource.generation {
                    return Vec::new();
                }
                self.local_discovery_in_flight = false;
                self.local_status_in_flight = false;
                self.local_state = state_for_failure(&failure, self.local_executable.as_ref());
                self.local_resource.fail(generation, failure.clone());
                let service_failure = service_failure_from_local_failure(&failure);
                self.devices_resource.health = if self.local_resource.snapshot.is_some() {
                    SourceHealth::Stale
                } else {
                    SourceHealth::Error
                };
                self.devices_resource.error = Some(failure.detail);
                self.services_snapshot
                    .certificate_domains
                    .fail(self.services_snapshot.generation, service_failure);
                self.schedule_failure_backoff();
            }
            LocalEvent::StatusStarted {
                generation,
                attempted_at,
            } => {
                if generation >= self.local_resource.generation {
                    self.local_status_in_flight = true;
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
                self.local_status_in_flight = false;
                let snapshot = *snapshot;
                self.local_state = snapshot.backend_state.clone();
                self.apply_local_snapshot(&snapshot);
                self.services_snapshot.command_version = Some(snapshot.client_version.clone());
                self.services_snapshot.certificate_domains.succeed(
                    self.services_snapshot.generation,
                    snapshot.observed_at,
                    snapshot.cert_domains.clone(),
                );
                self.local_resource.succeed(generation, snapshot);
                self.local_capabilities.status_json = true;
                self.local_next_refresh = self
                    .local_last_tick
                    .map(|tick| instant_after(tick, self.resolved_config.local.refresh_interval))
                    .or_else(|| {
                        Some(instant_after(
                            Instant::now(),
                            self.resolved_config.local.refresh_interval,
                        ))
                    });
                let mut effects = Vec::new();
                if self.local_capabilities.set
                    && let Some(executable) = self.local_executable.clone()
                {
                    effects.push(Effect::StartLocalPreferences {
                        executable,
                        timeout: self.resolved_config.local.command_timeout,
                    });
                }
                if self.local_capabilities.accounts
                    && let Some(executable) = self.local_executable.clone()
                {
                    effects.push(Effect::StartLocalAccounts {
                        executable,
                        timeout: self.resolved_config.local.command_timeout,
                    });
                }
                if self.local_capabilities.syspolicy
                    && let Some(executable) = self.local_executable.clone()
                {
                    effects.push(Effect::StartLocalPolicy {
                        executable,
                        timeout: self.resolved_config.local.command_timeout,
                    });
                }
                if self.local_executable.is_some() {
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
                self.local_status_in_flight = false;
                self.local_state = state_for_failure(&failure, self.local_executable.as_ref());
                self.local_resource.fail(generation, failure.clone());
                let service_failure = service_failure_from_local_failure(&failure);
                self.devices_resource.health = if self.local_resource.snapshot.is_some() {
                    SourceHealth::Stale
                } else {
                    SourceHealth::Error
                };
                self.devices_resource.error = Some(failure.detail);
                self.services_snapshot
                    .certificate_domains
                    .fail(self.services_snapshot.generation, service_failure);
                self.schedule_failure_backoff();
            }
            LocalEvent::PreferencesSucceeded { preferences } => {
                self.local_preferences = *preferences;
                apply_system_policy_editability(&mut self.local_preferences, &self.system_policy);
            }
            LocalEvent::PreferencesFailed { failure } => {
                self.local_preferences = match failure.kind {
                    crate::domain::source::LocalFailureKind::UnsupportedClient => {
                        LocalPreferences::unavailable(self.now)
                    }
                    crate::domain::source::LocalFailureKind::PermissionDenied => {
                        LocalPreferences::permission_denied(self.now)
                    }
                    _ => LocalPreferences::empty(self.now),
                };
                self.devices_resource.error = Some(failure.detail.clone());
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
                let preference_read_required = matches!(
                    &mutation,
                    LocalMutation::Preferences(_)
                        | LocalMutation::ExitNode(_)
                        | LocalMutation::Advertisements(_)
                );
                let preference_read_failed = preference_read_required
                    && preferences.is_none()
                    && !matches!(
                        &result,
                        crate::domain::mutation::MutationResult::CancelledBeforeDispatch { .. }
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
                if preference_read_failed {
                    self.local_preferences = LocalPreferences::empty(self.now);
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
    }

    fn apply_fresh_snapshot(&mut self, snapshot: LocalSnapshot) {
        let generation = self.local_resource.generation.saturating_add(1);
        self.local_resource.generation = generation;
        self.local_state = snapshot.backend_state.clone();
        self.apply_local_snapshot(&snapshot);
        let _ = self.local_resource.succeed(generation, snapshot);
        self.local_next_refresh = Some(instant_after(
            Instant::now(),
            self.resolved_config.local.refresh_interval,
        ));
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
    }

    fn start_account_rediscovery(&mut self) -> Vec<Effect> {
        if self.local_executable.is_none() {
            return Vec::new();
        }
        let generation = self.local_resource.generation.saturating_add(1);
        self.local_resource.generation = generation;
        self.local_discovery_in_flight = true;
        self.local_status_in_flight = false;
        self.local_next_refresh = None;
        vec![Effect::StartLocalDiscovery {
            generation,
            resolution: local_resolution(&self.resolved_config),
            timeout: self.resolved_config.local.command_timeout,
        }]
    }

    fn schedule_failure_backoff(&mut self) {
        let failures = self.local_resource.consecutive_failures;
        let interval = self.resolved_config.local.refresh_interval;
        let exponent = failures.saturating_sub(1).min(6);
        let multiplier = 1_u32.checked_shl(exponent).map_or(u32::MAX, |value| value);
        let delay = interval
            .checked_mul(multiplier)
            .map_or(Duration::from_secs(60), |value| {
                value.min(Duration::from_secs(60))
            });
        let base = match self.local_last_tick {
            Some(value) => value,
            None => Instant::now(),
        };
        self.local_next_refresh = Some(instant_after(base, delay));
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
        self.visible_indices_for(&self.devices_resource.snapshot)
    }

    fn visible_indices_for(&self, devices: &[Device]) -> Vec<usize> {
        let mut indices: Vec<usize> = devices
            .iter()
            .enumerate()
            .filter(|(_, device)| {
                let dns_name = if self.source_mode == SourceMode::Local {
                    self.local_dns_name(&device.id)
                } else {
                    None
                };
                self.views
                    .devices
                    .applied_filter
                    .matches_with_dns(device, dns_name, self.now)
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
                (Some(left), Some(right)) => compare_devices(left, right, self.views.devices.sort),
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
        if self.views.devices.sort == SortSpec::default() {
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
        compare_devices(left, right, self.views.devices.sort)
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
        let visible = self.visible_indices();
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
        self.views.devices.scroll = next;
    }

    fn move_selection_to(&mut self, position: usize) {
        let visible = self.visible_indices();
        let index = if position == usize::MAX {
            visible.len().saturating_sub(1)
        } else {
            position.min(visible.len().saturating_sub(1))
        };
        self.views.devices.selected_id = visible
            .get(index)
            .and_then(|value| self.devices_resource.snapshot.get(*value))
            .map(|device| device.id.clone());
        self.views.devices.scroll = index;
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
        self.reconcile_selection(None);
    }

    fn copy_field(&mut self, field: CopyField) {
        if field == CopyField::DiagnosticSummary {
            self.copy_diagnostic_summary();
            return;
        }
        if field == CopyField::Metrics {
            self.copied_value = Some(
                self.services_snapshot
                    .metrics
                    .value
                    .as_ref()
                    .map_or_else(String::new, |metrics| metrics.text.clone()),
            );
            return;
        }
        if matches!(field, CopyField::PublicKey | CopyField::Endpoint) {
            let value = self.selected_local_device().and_then(|device| match field {
                CopyField::PublicKey => device.public_key.clone(),
                CopyField::Endpoint => device.current_endpoint.clone(),
                _ => None,
            });
            self.copied_value = Some(match value {
                Some(value) => value,
                None => "not returned".to_owned(),
            });
            return;
        }
        let Some(device) = self.selected_device() else {
            return;
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
        self.copied_value = Some(value);
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
        self.route_stack
            .last()
            .copied()
            .map_or(Route::Overview, |route| route)
    }

    pub fn overlay_title(&self) -> Option<&'static str> {
        self.overlays.last().map(|overlay| match overlay {
            Overlay::CommandPalette(_) => "command palette",
            Overlay::FilterEditor(_) => "filter",
            Overlay::Help(_) => "help",
            Overlay::ActionPicker(_) => "actions",
            Overlay::CopyPicker(_) => "copy field",
            Overlay::QuitConfirmation => "quit",
            Overlay::TaskInspector(_) => "task",
            Overlay::SortPicker { .. } => "sort",
            Overlay::DiagnosticInput(_) => "diagnostic input",
            Overlay::Confirmation(_) => "confirm local action",
            Overlay::OperatorForm(_) => "local operator form",
            Overlay::ServiceForm(_) => "local service form",
            Overlay::ServiceSectionPicker(_) => "service section",
            Overlay::AccountPicker(_) => "local accounts",
            Overlay::HandoffInput(_) => "terminal handoff",
        })
    }

    pub fn clear_render_invalidated(&mut self) {
        self.render_invalidated = false;
    }

    pub const fn render_invalidated(&self) -> bool {
        self.render_invalidated
    }

    pub const fn local_refresh_due_at(&self) -> Option<Instant> {
        self.local_next_refresh
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

fn route_candidates(input: &str) -> Vec<Route> {
    let value = input.to_ascii_lowercase();
    [
        Route::Overview,
        Route::Local,
        Route::Devices,
        Route::Dns,
        Route::Activity,
        Route::Settings,
        Route::Services,
    ]
    .into_iter()
    .filter(|route| {
        route.label().starts_with(&value)
            || route_aliases(*route)
                .iter()
                .any(|alias| alias.starts_with(&value))
    })
    .collect()
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
            crate::local::preferences::set_command(path, timeout, request).ok()
        }
        LocalMutation::ExitNode(request) => Some(crate::local::preferences::exit_node_command(
            path, timeout, request,
        )),
        LocalMutation::Advertisements(request) => {
            crate::local::preferences::advertisement_command(path, timeout, request).ok()
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
        Route::Dns => &[],
        Route::Activity => &["tasks"],
        Route::Settings => &["config"],
        Route::Services => &["service", "serve", "funnel"],
    }
}

fn online_rank(value: Option<bool>) -> u8 {
    match value {
        Some(true) => 0,
        Some(false) => 1,
        None => 2,
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
        path: std::env::var_os("PATH"),
        platform: if cfg!(windows) {
            HostPlatform::Windows
        } else {
            HostPlatform::Unix
        },
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
