use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ActionId {
    AppQuit,
    ViewCommandPalette,
    ViewFilter,
    ViewRefresh,
    ViewRefreshAll,
    ViewHelp,
    ViewTasks,
    CollectionMoveUp,
    CollectionMoveDown,
    CollectionFirst,
    CollectionLast,
    CollectionPageUp,
    CollectionPageDown,
    CollectionOpen,
    CollectionSort,
    CollectionWideColumns,
    ResourceActions,
    ResourceCopy,
    TaskCancel,
    MockSuccess,
    MockFailure,
    MockCancellable,
    MockNonCancellable,
}

impl ActionId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AppQuit => "app.quit",
            Self::ViewCommandPalette => "view.command_palette",
            Self::ViewFilter => "view.filter",
            Self::ViewRefresh => "view.refresh",
            Self::ViewRefreshAll => "view.refresh_all",
            Self::ViewHelp => "view.help",
            Self::ViewTasks => "view.tasks",
            Self::CollectionMoveUp => "collection.move_up",
            Self::CollectionMoveDown => "collection.move_down",
            Self::CollectionFirst => "collection.first",
            Self::CollectionLast => "collection.last",
            Self::CollectionPageUp => "collection.page_up",
            Self::CollectionPageDown => "collection.page_down",
            Self::CollectionOpen => "collection.open",
            Self::CollectionSort => "collection.sort",
            Self::CollectionWideColumns => "collection.wide_columns",
            Self::ResourceActions => "resource.actions",
            Self::ResourceCopy => "resource.copy",
            Self::TaskCancel => "task.cancel",
            Self::MockSuccess => "mock.task.success",
            Self::MockFailure => "mock.task.failure",
            Self::MockCancellable => "mock.task.cancellable",
            Self::MockNonCancellable => "mock.task.non_cancellable",
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::AppQuit,
            Self::ViewCommandPalette,
            Self::ViewFilter,
            Self::ViewRefresh,
            Self::ViewRefreshAll,
            Self::ViewHelp,
            Self::ViewTasks,
            Self::CollectionMoveUp,
            Self::CollectionMoveDown,
            Self::CollectionFirst,
            Self::CollectionLast,
            Self::CollectionPageUp,
            Self::CollectionPageDown,
            Self::CollectionOpen,
            Self::CollectionSort,
            Self::CollectionWideColumns,
            Self::ResourceActions,
            Self::ResourceCopy,
            Self::TaskCancel,
            Self::MockSuccess,
            Self::MockFailure,
            Self::MockCancellable,
            Self::MockNonCancellable,
        ]
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ActionContext {
    Root,
    Collection,
    Detail,
    Overlay,
    Activity,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SelectionRule {
    None,
    One,
    Many,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Capability {
    Available,
    MockOnly,
    Disabled(&'static str),
}

impl Capability {
    pub const fn reason(self) -> Option<&'static str> {
        match self {
            Self::Available => None,
            Self::MockOnly => Some("available only in --mock mode"),
            Self::Disabled(reason) => Some(reason),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Risk {
    Observe,
    Reversible,
    Disruptive,
    DestructiveOrSecret,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Binding {
    Char(char),
    Ctrl(char),
    Enter,
}

impl Binding {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Char(' ') => "Space",
            Self::Char(':') => ":",
            Self::Char('/') => "/",
            Self::Char('?') => "?",
            Self::Char('@') => "@",
            Self::Char('R') => "R",
            Self::Char(value) => match value {
                'j' => "j",
                'k' => "k",
                'g' => "g",
                'G' => "G",
                'r' => "r",
                's' => "s",
                'w' => "w",
                'a' => "a",
                'y' => "y",
                'q' => "q",
                'x' => "x",
                'l' => "l",
                'h' => "h",
                _ => "key",
            },
            Self::Ctrl('d') => "Ctrl+d",
            Self::Ctrl('u') => "Ctrl+u",
            Self::Enter => "Enter",
            Self::Ctrl(_) => "Ctrl+key",
        }
    }

    pub fn matches(self, key: KeyEvent) -> bool {
        match self {
            Self::Char(character) => {
                key.code == KeyCode::Char(character) && key.modifiers.is_empty()
            }
            Self::Ctrl(character) => {
                key.code == KeyCode::Char(character)
                    && key.modifiers.contains(KeyModifiers::CONTROL)
            }
            Self::Enter => key.code == KeyCode::Enter && key.modifiers.is_empty(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActionSpec {
    pub id: ActionId,
    pub label: &'static str,
    pub description: &'static str,
    pub contexts: &'static [ActionContext],
    pub selection_rule: SelectionRule,
    pub default_bindings: &'static [Binding],
    pub capability: Capability,
    pub risk: Risk,
}

const ROOT: &[ActionContext] = &[ActionContext::Root];
const GLOBAL: &[ActionContext] = &[
    ActionContext::Root,
    ActionContext::Collection,
    ActionContext::Detail,
    ActionContext::Activity,
];
const NAVIGATION: &[ActionContext] = &[
    ActionContext::Collection,
    ActionContext::Detail,
    ActionContext::Activity,
];
const COLLECTION: &[ActionContext] = &[ActionContext::Collection, ActionContext::Detail];
const ACTIVITY: &[ActionContext] = &[ActionContext::Activity];
const OVERLAY: &[ActionContext] = &[ActionContext::Overlay];

const NO_BINDING: &[Binding] = &[];
const BIND_Q: &[Binding] = &[Binding::Char('q')];
const BIND_COLON: &[Binding] = &[Binding::Char(':')];
const BIND_SLASH: &[Binding] = &[Binding::Char('/')];
const BIND_R: &[Binding] = &[Binding::Char('r')];
const BIND_BIG_R: &[Binding] = &[Binding::Char('R')];
const BIND_HELP: &[Binding] = &[Binding::Char('?')];
const BIND_TASKS: &[Binding] = &[Binding::Char('@')];
const BIND_UP: &[Binding] = &[Binding::Char('k')];
const BIND_DOWN: &[Binding] = &[Binding::Char('j')];
const BIND_FIRST: &[Binding] = &[Binding::Char('g')];
const BIND_LAST: &[Binding] = &[Binding::Char('G')];
const BIND_PAGE_UP: &[Binding] = &[Binding::Ctrl('u')];
const BIND_PAGE_DOWN: &[Binding] = &[Binding::Ctrl('d')];
const BIND_OPEN: &[Binding] = &[Binding::Enter, Binding::Char('l')];
const BIND_SORT: &[Binding] = &[Binding::Char('s')];
const BIND_WIDE: &[Binding] = &[Binding::Char('w')];
const BIND_ACTIONS: &[Binding] = &[Binding::Char('a')];
const BIND_COPY: &[Binding] = &[Binding::Char('y')];
const BIND_CANCEL: &[Binding] = &[Binding::Char('x')];

pub fn phase_one_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec {
            id: ActionId::AppQuit,
            label: "Quit",
            description: "Quit Tale",
            contexts: ROOT,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_Q,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewCommandPalette,
            label: "Command palette",
            description: "Open route command palette",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_COLON,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewFilter,
            label: "Filter",
            description: "Edit the active collection filter",
            contexts: &[ActionContext::Collection],
            selection_rule: SelectionRule::None,
            default_bindings: BIND_SLASH,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewRefresh,
            label: "Refresh",
            description: "Refresh the active mock resource",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_R,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewRefreshAll,
            label: "Refresh all",
            description: "Refresh every mock source",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_BIG_R,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewHelp,
            label: "Help",
            description: "Show contextual help",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_HELP,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewTasks,
            label: "Tasks",
            description: "Open task history",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_TASKS,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::CollectionMoveUp,
            label: "Move up",
            description: "Move selection up",
            contexts: NAVIGATION,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_UP,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::CollectionMoveDown,
            label: "Move down",
            description: "Move selection down",
            contexts: NAVIGATION,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_DOWN,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::CollectionFirst,
            label: "First row",
            description: "Select the first visible row",
            contexts: NAVIGATION,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_FIRST,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::CollectionLast,
            label: "Last row",
            description: "Select the last visible row",
            contexts: NAVIGATION,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_LAST,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::CollectionPageUp,
            label: "Page up",
            description: "Move selection up one half page",
            contexts: NAVIGATION,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_PAGE_UP,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::CollectionPageDown,
            label: "Page down",
            description: "Move selection down one half page",
            contexts: NAVIGATION,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_PAGE_DOWN,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::CollectionOpen,
            label: "Open details",
            description: "Open selected resource details",
            contexts: &[
                ActionContext::Collection,
                ActionContext::Detail,
                ActionContext::Activity,
            ],
            selection_rule: SelectionRule::One,
            default_bindings: BIND_OPEN,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::CollectionSort,
            label: "Sort",
            description: "Choose field and direction",
            contexts: COLLECTION,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_SORT,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::CollectionWideColumns,
            label: "Wide columns",
            description: "Toggle standard and wide columns",
            contexts: COLLECTION,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_WIDE,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ResourceActions,
            label: "Actions",
            description: "Open actions for the selected resource",
            contexts: COLLECTION,
            selection_rule: SelectionRule::One,
            default_bindings: BIND_ACTIONS,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ResourceCopy,
            label: "Copy field",
            description: "Choose a complete field to copy",
            contexts: COLLECTION,
            selection_rule: SelectionRule::One,
            default_bindings: BIND_COPY,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::TaskCancel,
            label: "Cancel task",
            description: "Cancel the focused cancellable task",
            contexts: ACTIVITY,
            selection_rule: SelectionRule::One,
            default_bindings: BIND_CANCEL,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::MockSuccess,
            label: "Simulate success",
            description: "Start a delayed successful simulation",
            contexts: OVERLAY,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::MockOnly,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::MockFailure,
            label: "Simulate failure",
            description: "Start a delayed failed simulation",
            contexts: OVERLAY,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::MockOnly,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::MockCancellable,
            label: "Simulate cancellable task",
            description: "Start a long cancellable simulation",
            contexts: OVERLAY,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::MockOnly,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::MockNonCancellable,
            label: "Simulate non-cancellable task",
            description: "Start a non-cancellable simulation",
            contexts: OVERLAY,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::MockOnly,
            risk: Risk::Observe,
        },
    ]
}

pub fn find_action(id: ActionId) -> Option<ActionSpec> {
    phase_one_actions().into_iter().find(|spec| spec.id == id)
}

pub fn action_for_key(key: KeyEvent, context: ActionContext) -> Option<ActionId> {
    phase_one_actions().into_iter().find_map(|spec| {
        if spec.contexts.contains(&context)
            && spec
                .default_bindings
                .iter()
                .any(|binding| binding.matches(key))
        {
            Some(spec.id)
        } else {
            None
        }
    })
}

pub fn footer_hints(context: ActionContext, width: u16) -> Vec<String> {
    let mut used = 0usize;
    let mut hints = Vec::new();
    for spec in phase_one_actions() {
        if !spec.contexts.contains(&context) || spec.default_bindings.is_empty() {
            continue;
        }
        let binding = spec.default_bindings[0].label();
        let hint = format!("{} {}", binding, spec.label);
        let separator = if hints.is_empty() { 0 } else { 2 };
        if used + separator + hint.len() + 8 > usize::from(width) {
            hints.push("? more".to_owned());
            break;
        }
        used += separator + hint.len();
        hints.push(hint);
    }
    hints
}
