use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::{self, ActionContext, ActionId, Capability};
use crate::config::ResolvedConfig;
use crate::domain::device::{
    Device, DeviceId, LocalDevice, SortDirection, SortField, SortSpec, compare_devices,
};
use crate::domain::diagnostic::{DiagnosticResult, DiagnosticState};
use crate::domain::filter::{self, FilterExpression};
use crate::domain::redaction::{DiagnosticReportInput, redact_diagnostic_report};
use crate::domain::source::{
    LocalCapabilities, LocalExecutable, LocalFailure, LocalFailureKind, LocalResource,
    LocalResourceStatus, LocalSnapshot, LocalState,
};
use crate::domain::{SourceHealth, Timestamp};
use crate::effect::{Effect, Resource};
use crate::event::{Event, InputEvent, LocalEvent, ShutdownReason, SourceEvent, TaskEvent};
use crate::local::client::{ExecutableResolution, HostPlatform};
use crate::local::diagnostics::{self, DiagnosticRequest};
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
    render_invalidated: bool,
    local_discovery_in_flight: bool,
    local_status_in_flight: bool,
    local_next_refresh: Option<Instant>,
    local_last_tick: Option<Instant>,
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
            },
            devices_resource: DeviceResource::empty(source_mode),
            local_resource: LocalResource::new(),
            local_state,
            local_executable: None,
            local_capabilities: LocalCapabilities::default(),
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
            render_invalidated: true,
            local_discovery_in_flight: false,
            local_status_in_flight: false,
            local_next_refresh: None,
            local_last_tick: None,
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
                state.error = Some("unknown Phase-1 route".to_owned());
            }
            return Vec::new();
        };
        if !filter_text.trim().is_empty() && route != Route::Devices {
            if let Some(Overlay::CommandPalette(state)) = self.overlays.last_mut() {
                state.error = Some("filters are available for devices in Phase 1".to_owned());
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
            ActionId::CollectionMoveUp => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_next(-1);
                } else {
                    self.move_selection(-1);
                }
                Vec::new()
            }
            ActionId::CollectionMoveDown => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_next(1);
                } else {
                    self.move_selection(1);
                }
                Vec::new()
            }
            ActionId::CollectionFirst => {
                if self.current_route() == Route::Activity {
                    self.tasks.selected = self.tasks.all().first().map(|task| task.id);
                } else {
                    self.move_selection_to(0);
                }
                Vec::new()
            }
            ActionId::CollectionLast => {
                if self.current_route() == Route::Activity {
                    self.tasks.selected = self.tasks.all().last().map(|task| task.id);
                } else {
                    self.move_selection_to(usize::MAX);
                }
                Vec::new()
            }
            ActionId::CollectionPageUp => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_next(-5);
                } else {
                    self.move_selection(-5);
                }
                Vec::new()
            }
            ActionId::CollectionPageDown => {
                if self.current_route() == Route::Activity {
                    self.tasks.select_next(5);
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
                } else if self.selected_device().is_some() {
                    self.focus = Focus::Inspector;
                }
                Vec::new()
            }
            ActionId::CollectionSort => {
                self.overlays.push(Overlay::SortPicker { selected: 0 });
                Vec::new()
            }
            ActionId::CollectionWideColumns => {
                self.views.devices.wide_columns = !self.views.devices.wide_columns;
                Vec::new()
            }
            ActionId::ResourceActions => {
                let actions = if self.source_mode == SourceMode::Mock {
                    vec![
                        ActionId::MockSuccess,
                        ActionId::MockFailure,
                        ActionId::MockCancellable,
                        ActionId::MockNonCancellable,
                    ]
                } else if self.source_mode == SourceMode::Local {
                    vec![
                        ActionId::LocalProbeConnection,
                        ActionId::LocalWhois,
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
            )
        {
            return Some("tailscale executable has not been discovered".to_owned());
        }
        let reason = match action_id {
            ActionId::LocalProbeConnection => "ping is unavailable for this client",
            ActionId::LocalNetcheck => "one-shot netcheck is unavailable for this client",
            ActionId::LocalNetcheckLive => "live netcheck is unavailable for this client",
            ActionId::LocalDnsStatus => "DNS status is unavailable for this client",
            ActionId::LocalDnsQuery => "DNS query is unavailable for this client",
            ActionId::LocalWhois => "whois is unavailable for this client",
            _ => "capability unavailable",
        };
        Some(reason.to_owned())
    }

    fn local_action_available(&self, action_id: ActionId) -> bool {
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

    fn start_refresh(&mut self, _all: bool) -> Vec<Effect> {
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
                self.devices_resource.health = if self.local_resource.snapshot.is_some() {
                    SourceHealth::Stale
                } else {
                    SourceHealth::Error
                };
                self.devices_resource.error = Some(failure.detail);
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
                self.devices_resource.health = if self.local_resource.snapshot.is_some() {
                    SourceHealth::Stale
                } else {
                    SourceHealth::Error
                };
                self.devices_resource.error = Some(failure.detail);
                self.schedule_failure_backoff();
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
            CopyField::DiagnosticSummary => "not returned".to_owned(),
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

fn route_candidates(input: &str) -> Vec<Route> {
    let value = input.to_ascii_lowercase();
    [
        Route::Overview,
        Route::Local,
        Route::Devices,
        Route::Dns,
        Route::Activity,
        Route::Settings,
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

fn route_aliases(route: Route) -> &'static [&'static str] {
    match route {
        Route::Overview => &["ov", "home"],
        Route::Local => &["self"],
        Route::Devices => &["device", "dev", "nodes"],
        Route::Dns => &[],
        Route::Activity => &["tasks"],
        Route::Settings => &["config"],
    }
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
