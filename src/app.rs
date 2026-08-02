use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::{self, ActionContext, ActionId, Capability};
use crate::config::ResolvedConfig;
use crate::domain::device::{
    Device, DeviceId, SortDirection, SortField, SortSpec, compare_devices,
};
use crate::domain::filter::{self, FilterExpression};
use crate::domain::{SourceHealth, Timestamp};
use crate::effect::{Effect, Resource};
use crate::event::{Event, InputEvent, ShutdownReason, SourceEvent, TaskEvent};
use crate::mock::{MOCK_NOW, MockLoadScenario, MockTaskBehavior};
use crate::task::{Notification, TaskId, TaskState, TaskStore};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SourceMode {
    Mock,
    Unavailable,
}

impl SourceMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Unavailable => "local unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Route {
    Overview,
    Devices,
    Activity,
    Settings,
}

impl Route {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Devices => "devices",
            Self::Activity => "activity",
            Self::Settings => "settings",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "overview" | "ov" | "home" => Some(Self::Overview),
            "devices" | "device" | "dev" | "nodes" => Some(Self::Devices),
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CopyField {
    DeviceId,
    DisplayName,
    Hostname,
    Owner,
    Addresses,
    Tags,
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
}

impl App {
    pub fn new(config: ResolvedConfig) -> Self {
        let source_mode = if config.mock {
            SourceMode::Mock
        } else {
            SourceMode::Unavailable
        };
        Self {
            route_stack: vec![Route::Overview],
            focus: Focus::Collection,
            overlays: Vec::new(),
            views: Views {
                devices: DeviceViewState::default(),
            },
            devices_resource: DeviceResource::empty(source_mode),
            tasks: TaskStore::new(),
            notifications: Vec::new(),
            resolved_config: config,
            shutdown_state: ShutdownState::Running,
            source_mode,
            terminal_width: 80,
            terminal_height: 24,
            now: MOCK_NOW,
            tick_count: 0,
            runtime_error: None,
            copied_value: None,
            render_invalidated: true,
        }
    }

    pub fn bootstrap_effects(&mut self) -> Vec<Effect> {
        if self.source_mode != SourceMode::Mock {
            return Vec::new();
        }
        self.devices_resource.generation = 1;
        vec![Effect::StartMockLoad {
            resource: Resource::Devices,
            generation: 1,
            scenario: MockLoadScenario::Initial,
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
            Event::Tick(_) => self.update_tick(),
            Event::Task(task) => self.update_task(task),
            Event::Source(source) => self.update_source(source),
            Event::ShutdownRequested(reason) => self.request_shutdown(reason),
        }
    }

    fn update_tick(&mut self) -> Vec<Effect> {
        self.tick_count = self.tick_count.saturating_add(1);
        self.now = MOCK_NOW.saturating_add(self.tick_count);
        self.notifications
            .retain(|notification| notification.expires_at > self.now);
        if self.tasks.has_active() {
            self.render_invalidated = true;
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
                    KeyCode::Char('j') | KeyCode::Down => selected = (selected + 1).min(13),
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
        if !self.action_available(spec.capability) {
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
                self.overlays.push(Overlay::CopyPicker(CopyPickerState {
                    fields: vec![
                        CopyField::DeviceId,
                        CopyField::DisplayName,
                        CopyField::Hostname,
                        CopyField::Owner,
                        CopyField::Addresses,
                        CopyField::Tags,
                    ],
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
        }
    }

    fn action_available(&self, capability: Capability) -> bool {
        match capability {
            Capability::Available => true,
            Capability::MockOnly => self.source_mode == SourceMode::Mock,
            Capability::Disabled(_) => false,
        }
    }

    fn start_refresh(&mut self, _all: bool) -> Vec<Effect> {
        if self.source_mode != SourceMode::Mock {
            self.runtime_error = Some("local integration is unavailable in this build".to_owned());
            return Vec::new();
        }
        self.devices_resource.generation = self.devices_resource.generation.saturating_add(1);
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
            .filter(|(_, device)| self.views.devices.applied_filter.matches(device, self.now))
            .map(|(index, _)| index)
            .collect();
        indices.sort_by(|left, right| {
            let left_device = devices.get(*left);
            let right_device = devices.get(*right);
            match (left_device, right_device) {
                (Some(left), Some(right)) => compare_devices(left, right, self.views.devices.sort),
                _ => left.cmp(right),
            }
        });
        indices
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

fn route_candidates(input: &str) -> Vec<Route> {
    let value = input.to_ascii_lowercase();
    [
        Route::Overview,
        Route::Devices,
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
        Route::Devices => &["device", "dev", "nodes"],
        Route::Activity => &["tasks"],
        Route::Settings => &["config"],
    }
}
