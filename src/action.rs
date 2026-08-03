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
    LocalDiagnostics,
    LocalProbeConnection,
    LocalNetcheck,
    LocalNetcheckLive,
    LocalDnsStatus,
    LocalDnsQuery,
    LocalWhois,
    DiagnosticCopy,
    LocalConnect,
    LocalDisconnect,
    LocalPreferencesEdit,
    LocalExitNodeSelect,
    LocalRoutesEditAdvertisements,
    LocalAccountSwitch,
    LocalAccountLogin,
    LocalAccountLogout,
    LocalAccountRemove,
    LocalSshOpen,
    LocalNcOpen,
    LocalSyspolicyReload,
    ViewServices,
    ServicesSectionNext,
    ServicesSectionPrevious,
    ServicesServeRefresh,
    ServicesServeCreate,
    ServicesServeEdit,
    ServicesServeReset,
    ServicesFunnelRefresh,
    ServicesFunnelCreate,
    ServicesFunnelEdit,
    ServicesFunnelReset,
    ServicesTaildropSend,
    ServicesTaildropReceive,
    ServicesDriveRefresh,
    ServicesDriveShare,
    ServicesDriveRename,
    ServicesDriveUnshare,
    ServicesCertificateObtain,
    ServicesMetricsRefresh,
    ServicesBugReportCreate,
    ServicesDriveEnableAlpha,
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
            Self::LocalDiagnostics => "local.diagnostics",
            Self::LocalProbeConnection => "local.probe_connection",
            Self::LocalNetcheck => "local.netcheck",
            Self::LocalNetcheckLive => "local.netcheck_live",
            Self::LocalDnsStatus => "local.dns_status",
            Self::LocalDnsQuery => "local.dns_query",
            Self::LocalWhois => "local.whois",
            Self::DiagnosticCopy => "diagnostic.copy",
            Self::LocalConnect => "local.connect",
            Self::LocalDisconnect => "local.disconnect",
            Self::LocalPreferencesEdit => "local.preferences.edit",
            Self::LocalExitNodeSelect => "local.exit_node.select",
            Self::LocalRoutesEditAdvertisements => "local.routes.edit_advertisements",
            Self::LocalAccountSwitch => "local.account.switch",
            Self::LocalAccountLogin => "local.account.login",
            Self::LocalAccountLogout => "local.account.logout",
            Self::LocalAccountRemove => "local.account.remove",
            Self::LocalSshOpen => "local.ssh.open",
            Self::LocalNcOpen => "local.nc.open",
            Self::LocalSyspolicyReload => "local.syspolicy.reload",
            Self::ViewServices => "view.services",
            Self::ServicesSectionNext => "services.section.next",
            Self::ServicesSectionPrevious => "services.section.previous",
            Self::ServicesServeRefresh => "services.serve.refresh",
            Self::ServicesServeCreate => "services.serve.create",
            Self::ServicesServeEdit => "services.serve.edit",
            Self::ServicesServeReset => "services.serve.reset",
            Self::ServicesFunnelRefresh => "services.funnel.refresh",
            Self::ServicesFunnelCreate => "services.funnel.create",
            Self::ServicesFunnelEdit => "services.funnel.edit",
            Self::ServicesFunnelReset => "services.funnel.reset",
            Self::ServicesTaildropSend => "services.taildrop.send",
            Self::ServicesTaildropReceive => "services.taildrop.receive",
            Self::ServicesDriveRefresh => "services.drive.refresh",
            Self::ServicesDriveShare => "services.drive.share",
            Self::ServicesDriveRename => "services.drive.rename",
            Self::ServicesDriveUnshare => "services.drive.unshare",
            Self::ServicesCertificateObtain => "services.certificate.obtain",
            Self::ServicesMetricsRefresh => "services.metrics.refresh",
            Self::ServicesBugReportCreate => "services.bugreport.create",
            Self::ServicesDriveEnableAlpha => "services.drive.enable_alpha",
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
            Self::LocalDiagnostics,
            Self::LocalProbeConnection,
            Self::LocalNetcheck,
            Self::LocalNetcheckLive,
            Self::LocalDnsStatus,
            Self::LocalDnsQuery,
            Self::LocalWhois,
            Self::DiagnosticCopy,
            Self::LocalConnect,
            Self::LocalDisconnect,
            Self::LocalPreferencesEdit,
            Self::LocalExitNodeSelect,
            Self::LocalRoutesEditAdvertisements,
            Self::LocalAccountSwitch,
            Self::LocalAccountLogin,
            Self::LocalAccountLogout,
            Self::LocalAccountRemove,
            Self::LocalSshOpen,
            Self::LocalNcOpen,
            Self::LocalSyspolicyReload,
            Self::ViewServices,
            Self::ServicesSectionNext,
            Self::ServicesSectionPrevious,
            Self::ServicesServeRefresh,
            Self::ServicesServeCreate,
            Self::ServicesServeEdit,
            Self::ServicesServeReset,
            Self::ServicesFunnelRefresh,
            Self::ServicesFunnelCreate,
            Self::ServicesFunnelEdit,
            Self::ServicesFunnelReset,
            Self::ServicesTaildropSend,
            Self::ServicesTaildropReceive,
            Self::ServicesDriveRefresh,
            Self::ServicesDriveShare,
            Self::ServicesDriveRename,
            Self::ServicesDriveUnshare,
            Self::ServicesCertificateObtain,
            Self::ServicesMetricsRefresh,
            Self::ServicesBugReportCreate,
            Self::ServicesDriveEnableAlpha,
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

impl Risk {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Reversible => "reversible",
            Self::Disruptive => "disruptive",
            Self::DestructiveOrSecret => "destructive or secret",
        }
    }
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
            Self::Char('[') => "[",
            Self::Char(']') => "]",
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

const BIND_ACTIONS_ROOT: &[Binding] = &[Binding::Char('a')];

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
    all_actions().into_iter().find(|spec| spec.id == id)
}

pub fn action_for_key(key: KeyEvent, context: ActionContext) -> Option<ActionId> {
    all_actions().into_iter().find_map(|spec| {
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
    for spec in all_actions() {
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

pub fn phase_two_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec {
            id: ActionId::LocalDiagnostics,
            label: "Local diagnostics",
            description: "Open read-only local diagnostics",
            contexts: ROOT,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_ACTIONS_ROOT,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::LocalProbeConnection,
            label: "Probe connection",
            description: "Run a Tailscale ping against the selected peer",
            contexts: &[
                ActionContext::Collection,
                ActionContext::Detail,
                ActionContext::Overlay,
            ],
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::LocalNetcheck,
            label: "Netcheck",
            description: "Run one-shot local network diagnostics",
            contexts: &[ActionContext::Root, ActionContext::Overlay],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::LocalNetcheckLive,
            label: "Live netcheck",
            description: "Stream local network diagnostics until cancelled",
            contexts: &[ActionContext::Root, ActionContext::Overlay],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::LocalDnsStatus,
            label: "DNS status",
            description: "Inspect local DNS configuration",
            contexts: &[ActionContext::Root, ActionContext::Overlay],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::LocalDnsQuery,
            label: "DNS query",
            description: "Query a DNS record through Tailscale",
            contexts: &[ActionContext::Root, ActionContext::Overlay],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::LocalWhois,
            label: "Whois",
            description: "Identify a Tailscale IP address",
            contexts: &[
                ActionContext::Collection,
                ActionContext::Detail,
                ActionContext::Root,
                ActionContext::Overlay,
            ],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::DiagnosticCopy,
            label: "Copy redacted diagnostic",
            description: "Render a redacted diagnostic summary",
            contexts: &[
                ActionContext::Root,
                ActionContext::Collection,
                ActionContext::Detail,
                ActionContext::Overlay,
            ],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
    ]
}

pub fn phase_three_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec {
            id: ActionId::LocalConnect,
            label: "Connect local node",
            description: "Connect this node without changing preferences",
            contexts: &[
                ActionContext::Root,
                ActionContext::Collection,
                ActionContext::Detail,
            ],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::LocalDisconnect,
            label: "Disconnect local node",
            description: "Disconnect this node after explicit confirmation",
            contexts: &[
                ActionContext::Root,
                ActionContext::Collection,
                ActionContext::Detail,
            ],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::LocalPreferencesEdit,
            label: "Edit local preferences",
            description: "Edit supported preferences with preview and verification",
            contexts: &[
                ActionContext::Root,
                ActionContext::Collection,
                ActionContext::Detail,
            ],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::LocalExitNodeSelect,
            label: "Select exit node",
            description: "Choose or clear this node's exit node",
            contexts: &[
                ActionContext::Collection,
                ActionContext::Detail,
                ActionContext::Root,
            ],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::LocalRoutesEditAdvertisements,
            label: "Edit advertisements",
            description: "Edit this node's route, connector, and relay advertisements",
            contexts: &[
                ActionContext::Root,
                ActionContext::Collection,
                ActionContext::Detail,
            ],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::LocalAccountSwitch,
            label: "Switch account",
            description: "Switch this local client to another account profile",
            contexts: &[
                ActionContext::Root,
                ActionContext::Collection,
                ActionContext::Detail,
            ],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::LocalAccountLogin,
            label: "Add account",
            description: "Open Tailscale login in the terminal",
            contexts: &[
                ActionContext::Root,
                ActionContext::Collection,
                ActionContext::Detail,
            ],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::LocalAccountLogout,
            label: "Log out account",
            description: "Log out this local account after confirmation",
            contexts: &[
                ActionContext::Root,
                ActionContext::Collection,
                ActionContext::Detail,
            ],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::LocalAccountRemove,
            label: "Remove local account",
            description: "Remove a local account profile after typed confirmation",
            contexts: &[
                ActionContext::Root,
                ActionContext::Collection,
                ActionContext::Detail,
            ],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::LocalSshOpen,
            label: "Open Tailscale SSH",
            description: "Open a terminal session to the selected device",
            contexts: &[ActionContext::Collection, ActionContext::Detail],
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::LocalNcOpen,
            label: "Open Tailscale netcat",
            description: "Open a terminal netcat session to the selected device",
            contexts: &[ActionContext::Collection, ActionContext::Detail],
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::LocalSyspolicyReload,
            label: "Reload system policy",
            description: "Reload local policy and verify it with a fresh list",
            contexts: &[
                ActionContext::Root,
                ActionContext::Collection,
                ActionContext::Detail,
            ],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
    ]
}

const SERVICES: &[ActionContext] = &[
    ActionContext::Root,
    ActionContext::Collection,
    ActionContext::Detail,
];
const SERVICES_NAVIGATION: &[ActionContext] = &[ActionContext::Collection, ActionContext::Detail];

pub fn phase_four_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec {
            id: ActionId::ViewServices,
            label: "Services",
            description: "Open local services",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ServicesSectionNext,
            label: "Next service section",
            description: "Select the next local services subsection",
            contexts: SERVICES_NAVIGATION,
            selection_rule: SelectionRule::None,
            default_bindings: &[Binding::Char(']')],
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ServicesSectionPrevious,
            label: "Previous service section",
            description: "Select the previous local services subsection",
            contexts: SERVICES_NAVIGATION,
            selection_rule: SelectionRule::None,
            default_bindings: &[Binding::Char('[')],
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ServicesServeRefresh,
            label: "Refresh Serve",
            description: "Inspect local Serve mappings",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ServicesServeCreate,
            label: "Create Serve mapping",
            description: "Create a tailnet-only Serve mapping",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::ServicesServeEdit,
            label: "Edit Serve mapping",
            description: "Replace one local Serve mapping",
            contexts: SERVICES,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::ServicesServeReset,
            label: "Reset Serve",
            description: "Remove every local Serve mapping",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::ServicesFunnelRefresh,
            label: "Refresh Funnel",
            description: "Inspect public Funnel mappings",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ServicesFunnelCreate,
            label: "Create public Funnel",
            description: "Expose one mapping publicly through Funnel",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::ServicesFunnelEdit,
            label: "Edit public Funnel",
            description: "Replace one public Funnel mapping",
            contexts: SERVICES,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::ServicesFunnelReset,
            label: "Reset Funnel",
            description: "Remove every public Funnel mapping",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::ServicesTaildropSend,
            label: "Send with Taildrop",
            description: "Send selected regular files to a visible target",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::ServicesTaildropReceive,
            label: "Receive with Taildrop",
            description: "Receive one Taildrop batch into a directory",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::ServicesDriveRefresh,
            label: "Refresh Taildrive",
            description: "Inspect alpha Taildrive shares",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ServicesDriveShare,
            label: "Share directory",
            description: "Create an alpha Taildrive share",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::ServicesDriveRename,
            label: "Rename share",
            description: "Rename an alpha Taildrive share",
            contexts: SERVICES,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::ServicesDriveUnshare,
            label: "Unshare directory",
            description: "Remove an alpha Taildrive share",
            contexts: SERVICES,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::ServicesCertificateObtain,
            label: "Obtain certificate",
            description: "Request a certificate for an eligible local domain",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::ServicesMetricsRefresh,
            label: "Refresh metrics",
            description: "Capture bounded local Prometheus metrics",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ServicesBugReportCreate,
            label: "Create bug report",
            description: "Ask Tailscale to collect a diagnostic report",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::ServicesDriveEnableAlpha,
            label: "Enable Taildrive alpha",
            description: "Enable alpha Taildrive controls for this run",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
    ]
}

pub fn all_actions() -> Vec<ActionSpec> {
    let mut actions = phase_one_actions();
    actions.extend(phase_two_actions());
    actions.extend(phase_three_actions());
    actions.extend(phase_four_actions());
    actions
}
