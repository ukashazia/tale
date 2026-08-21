use std::collections::HashMap;
use std::sync::LazyLock;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Route;

macro_rules! define_action_ids {
    (
        listed_before { $( $before:ident => $before_name:literal; )* }
        hidden { $( $hidden:ident => $hidden_name:literal; )* }
        listed_after { $( $after:ident => $after_name:literal; )* }
    ) => {
        #[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
        pub enum ActionId {
            $( $before, )*
            $( $hidden, )*
            $( $after, )*
        }

        impl ActionId {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$before => $before_name, )*
                    $( Self::$hidden => $hidden_name, )*
                    $( Self::$after => $after_name, )*
                }
            }

            pub const fn all() -> &'static [Self] {
                &[ $( Self::$before, )* $( Self::$after, )* ]
            }

            pub fn parse(value: &str) -> Option<Self> {
                match value {
                    $( $before_name => Some(Self::$before), )*
                    $( $hidden_name => Some(Self::$hidden), )*
                    $( $after_name => Some(Self::$after), )*
                    _ => None,
                }
            }
        }
    };
}

define_action_ids! {
    listed_before {
        AppQuit => "app.quit";
        ViewCommandLine => "view.command_line";
        ViewFilter => "view.filter";
    }
    hidden {
        DetailSearch => "detail.search";
        DeviceDetailNextMatch => "device_detail.next_match";
        DeviceDetailPreviousMatch => "device_detail.previous_match";
    }
    listed_after {
        ViewRefresh => "view.refresh";
        ViewRefreshAll => "view.refresh_all";
        ViewHelp => "view.help";
        ViewTasks => "view.tasks";
        ViewHistoryBack => "view.history.back";
        ViewHistoryForward => "view.history.forward";
        CollectionMoveUp => "collection.move_up";
        CollectionMoveDown => "collection.move_down";
        CollectionFirst => "collection.first";
        CollectionLast => "collection.last";
        CollectionPageUp => "collection.page_up";
        CollectionPageDown => "collection.page_down";
        CollectionOpen => "collection.open";
        CollectionBack => "collection.back";
        CollectionSort => "collection.sort";
        CollectionWideColumns => "collection.wide_columns";
        CollectionInspect => "collection.inspect";
        ResourceActions => "resource.actions";
        ResourceCopy => "resource.copy";
        TaskCancel => "task.cancel";
        MockSuccess => "mock.task.success";
        MockFailure => "mock.task.failure";
        MockCancellable => "mock.task.cancellable";
        MockNonCancellable => "mock.task.non_cancellable";
        LocalDiagnostics => "local.diagnostics";
        LocalProbeConnection => "local.probe_connection";
        LocalNetcheck => "local.netcheck";
        LocalNetcheckLive => "local.netcheck_live";
        LocalDnsStatus => "local.dns_status";
        LocalDnsQuery => "local.dns_query";
        LocalWhois => "local.whois";
        DiagnosticCopy => "diagnostic.copy";
        LocalConnect => "local.connect";
        LocalDisconnect => "local.disconnect";
        LocalPreferencesEdit => "local.preferences.edit";
        LocalExitNodeSelect => "local.exit_node.select";
        LocalRoutesEditAdvertisements => "local.routes.edit_advertisements";
        LocalAccountSwitch => "local.account.switch";
        LocalAccountLogin => "local.account.login";
        LocalAccountLogout => "local.account.logout";
        LocalAccountRemove => "local.account.remove";
        LocalSshOpen => "local.ssh.open";
        LocalNcOpen => "local.nc.open";
        LocalSyspolicyReload => "local.syspolicy.reload";
        ViewServices => "view.services";
        ViewDiagnostics => "view.diagnostics";
        SectionNext => "section.next";
        SectionPrevious => "section.previous";
        ServicesServeRefresh => "services.serve.refresh";
        ServicesServeCreate => "services.serve.create";
        ServicesServeEdit => "services.serve.edit";
        ServicesServeRemove => "services.serve.remove";
        ServicesServeReset => "services.serve.reset";
        ServicesFunnelCreate => "services.funnel.create";
        ServicesFunnelEdit => "services.funnel.edit";
        ServicesFunnelUnpublish => "services.funnel.unpublish";
        ServicesFunnelReset => "services.funnel.reset";
        DevicesTaildropSend => "devices.taildrop.send";
        DevicesTaildropReceive => "devices.taildrop.receive";
        ServicesDriveRefresh => "services.drive.refresh";
        ServicesDriveShare => "services.drive.share";
        ServicesDriveRename => "services.drive.rename";
        ServicesDriveUnshare => "services.drive.unshare";
        ServicesCertificateObtain => "services.certificate.obtain";
        ServicesMetricsRefresh => "services.metrics.refresh";
        ServicesBugReportCreate => "services.bugreport.create";
        ServicesDriveEnableAlpha => "services.drive.enable_alpha";
        ProfileActivate => "profile.activate";
        AdminRefreshCurrent => "admin.refresh.current";
        AdminRefreshAll => "admin.refresh.all";
        ViewProfiles => "view.profiles";
        ViewUsers => "view.users";
        ViewRoutes => "view.routes";
        ViewDns => "view.dns";
        ViewAccess => "view.access";
        ViewCredentials => "view.credentials";
        UsersOpenDevices => "users.open.devices";
        RoutesOpenDevice => "routes.open.device";
        DnsOpenLocalDiagnostics => "dns.open.local_diagnostics";
        AccessCopySource => "access.copy_source";
        ActivitySelectWindow => "activity.select_window";
        ActivityOpenActor => "activity.open_actor";
        ActivityOpenTarget => "activity.open_target";
        SettingsInspectCapabilities => "settings.inspect_capabilities";
        AdminDeviceRename => "admin.device.rename";
        AdminDeviceTagsReplace => "admin.device.tags.replace";
        AdminDeviceApprove => "admin.device.approve";
        AdminDeviceRevokeApproval => "admin.device.revoke_approval";
        AdminDeviceKeyExpiryConfigure => "admin.device.key_expiry.configure";
        AdminDeviceKeyExpireNow => "admin.device.key_expire_now";
        AdminDeviceDelete => "admin.device.delete";
        AdminRoutesReplaceApprovals => "admin.routes.replace_approvals";
        AdminDnsPreferencesEdit => "admin.dns.preferences.edit";
        AdminDnsNameserversReplace => "admin.dns.nameservers.replace";
        AdminDnsSearchPathsReplace => "admin.dns.search_paths.replace";
        AdminDnsSplitCreate => "admin.dns.split.create";
        AdminDnsSplitEdit => "admin.dns.split.edit";
        AdminDnsSplitRemove => "admin.dns.split.remove";
        AdminUserApprove => "admin.user.approve";
        AdminUserRoleChange => "admin.user.role.change";
        AdminUserSuspend => "admin.user.suspend";
        AdminUserRestore => "admin.user.restore";
        AdminUserDelete => "admin.user.delete";
        AdminPolicyEdit => "admin.policy.edit";
        AdminPolicyEditorReopen => "admin.policy.editor.reopen";
        AdminPolicyCandidateDiscard => "admin.policy.candidate.discard";
        AdminPolicyRemoteRefresh => "admin.policy.remote.refresh";
        AdminPolicyValidate => "admin.policy.validate";
        AdminPolicyPreview => "admin.policy.preview";
        AdminPolicyDiff => "admin.policy.diff";
        AdminPolicyApply => "admin.policy.apply";
        AdminPolicyWorkflowClose => "admin.policy.workflow.close";
        AdminCredentialAuthKeyCreate => "admin.credential.auth_key.create";
        SecretResultCopy => "secret_result.copy";
        SecretResultClose => "secret_result.close";
        AdminCredentialRevoke => "admin.credential.revoke";
        ProfileCredentialRemove => "profile.credential.remove";
        AuditFilterTime => "audit.filter.time";
        AuditFilterActor => "audit.filter.actor";
        AuditFilterAction => "audit.filter.action";
        AuditFilterTarget => "audit.filter.target";
        AuditOpenTarget => "audit.open.target";
        AuditOpenPolicyDiff => "audit.open.policy_diff";
        BatchReviewOutcomes => "batch.review_outcomes";
        BatchRetrySelected => "batch.retry_selected";
        OverviewHealthOpenResource => "overview.health.open_resource";
        OverviewHealthRunSuggestedAction => "overview.health.run_suggested_action";
        ActivityFlowsSelectWindow => "activity.flows.select_window";
        ActivityFlowsAggregate => "activity.flows.aggregate";
        ActivityFlowsOpenDevice => "activity.flows.open_device";
        AdminWebhookCreate => "admin.webhook.create";
        AdminWebhookEdit => "admin.webhook.edit";
        AdminWebhookTest => "admin.webhook.test";
        AdminWebhookRotateSecret => "admin.webhook.rotate_secret";
        AdminWebhookDelete => "admin.webhook.delete";
        AdminLogStreamReplace => "admin.log_stream.replace";
        AdminLogStreamDelete => "admin.log_stream.delete";
        AdminNetworkLogsSettings => "admin.network_logs.settings";
        SavedViewCreate => "saved_view.create";
        SavedViewReplace => "saved_view.replace";
        SavedViewRename => "saved_view.rename";
        SavedViewDelete => "saved_view.delete";
        SavedViewApply => "saved_view.apply";
        CollectionExport => "collection.export";
        AccessExplorerAsk => "access_explorer.ask";
        AccessExplorerOpenRule => "access_explorer.open_rule";
    }
}

impl ActionId {
    pub(crate) const fn is_mutating(self) -> bool {
        self.needs_local_verification()
            || matches!(self, Self::LocalAccountLogin | Self::LocalAccountLogout)
            || self.is_service_write()
    }

    pub(crate) const fn needs_local_verification(self) -> bool {
        matches!(
            self,
            Self::LocalConnect
                | Self::LocalDisconnect
                | Self::LocalPreferencesEdit
                | Self::LocalExitNodeSelect
                | Self::LocalRoutesEditAdvertisements
                | Self::LocalAccountSwitch
                | Self::LocalAccountRemove
                | Self::LocalSyspolicyReload
        )
    }

    pub(crate) const fn is_admin(self) -> bool {
        self.is_admin_mutation()
            || matches!(
                self,
                Self::ProfileActivate
                    | Self::AdminRefreshCurrent
                    | Self::AdminRefreshAll
                    | Self::ViewProfiles
                    | Self::ViewUsers
                    | Self::ViewRoutes
                    | Self::ViewDns
                    | Self::ViewAccess
                    | Self::ViewCredentials
                    | Self::UsersOpenDevices
                    | Self::RoutesOpenDevice
                    | Self::DnsOpenLocalDiagnostics
                    | Self::AccessCopySource
                    | Self::ActivitySelectWindow
                    | Self::ActivityOpenActor
                    | Self::ActivityOpenTarget
                    | Self::SettingsInspectCapabilities
                    | Self::AdminPolicyEdit
                    | Self::AdminPolicyEditorReopen
                    | Self::AdminPolicyRemoteRefresh
                    | Self::AdminPolicyValidate
                    | Self::AdminPolicyPreview
                    | Self::AdminPolicyDiff
                    | Self::AdminPolicyWorkflowClose
                    | Self::AuditFilterTime
                    | Self::AuditFilterActor
                    | Self::AuditFilterAction
                    | Self::AuditFilterTarget
                    | Self::AuditOpenTarget
                    | Self::AuditOpenPolicyDiff
                    | Self::BatchReviewOutcomes
                    | Self::BatchRetrySelected
                    | Self::OverviewHealthOpenResource
                    | Self::OverviewHealthRunSuggestedAction
                    | Self::ActivityFlowsSelectWindow
                    | Self::ActivityFlowsAggregate
                    | Self::ActivityFlowsOpenDevice
                    | Self::AdminWebhookCreate
                    | Self::AdminWebhookEdit
                    | Self::AdminWebhookTest
                    | Self::AdminWebhookRotateSecret
                    | Self::AdminWebhookDelete
                    | Self::AdminLogStreamReplace
                    | Self::AdminLogStreamDelete
                    | Self::AdminNetworkLogsSettings
                    | Self::AccessExplorerAsk
                    | Self::AccessExplorerOpenRule
            )
    }

    pub(crate) const fn is_admin_mutation(self) -> bool {
        self.is_admin_device_mutation()
            || self.is_admin_dns_mutation()
            || self.is_admin_user_mutation()
            || matches!(
                self,
                Self::AdminRoutesReplaceApprovals
                    | Self::AdminPolicyCandidateDiscard
                    | Self::AdminPolicyApply
                    | Self::AdminCredentialAuthKeyCreate
                    | Self::AdminCredentialRevoke
                    | Self::ProfileCredentialRemove
            )
    }

    pub(crate) const fn is_admin_device_mutation(self) -> bool {
        matches!(
            self,
            Self::AdminDeviceRename
                | Self::AdminDeviceTagsReplace
                | Self::AdminDeviceApprove
                | Self::AdminDeviceRevokeApproval
                | Self::AdminDeviceKeyExpiryConfigure
                | Self::AdminDeviceKeyExpireNow
                | Self::AdminDeviceDelete
        )
    }

    pub(crate) const fn is_admin_dns_mutation(self) -> bool {
        matches!(
            self,
            Self::AdminDnsPreferencesEdit
                | Self::AdminDnsNameserversReplace
                | Self::AdminDnsSearchPathsReplace
                | Self::AdminDnsSplitCreate
                | Self::AdminDnsSplitEdit
                | Self::AdminDnsSplitRemove
        )
    }

    pub(crate) const fn is_admin_user_mutation(self) -> bool {
        matches!(
            self,
            Self::AdminUserApprove
                | Self::AdminUserRoleChange
                | Self::AdminUserSuspend
                | Self::AdminUserRestore
                | Self::AdminUserDelete
        )
    }

    pub(crate) const fn is_service_write(self) -> bool {
        matches!(
            self,
            Self::ServicesServeCreate
                | Self::ServicesServeEdit
                | Self::ServicesServeRemove
                | Self::ServicesServeReset
                | Self::ServicesFunnelCreate
                | Self::ServicesFunnelEdit
                | Self::ServicesFunnelUnpublish
                | Self::ServicesFunnelReset
                | Self::DevicesTaildropSend
                | Self::DevicesTaildropReceive
                | Self::ServicesDriveShare
                | Self::ServicesDriveRename
                | Self::ServicesDriveUnshare
                | Self::ServicesCertificateObtain
                | Self::ServicesBugReportCreate
        )
    }

    pub(crate) const fn is_taildrive(self) -> bool {
        matches!(
            self,
            Self::ServicesDriveRefresh
                | Self::ServicesDriveShare
                | Self::ServicesDriveRename
                | Self::ServicesDriveUnshare
                | Self::ServicesDriveEnableAlpha
        )
    }

    pub(crate) const fn is_local_service(self) -> bool {
        self.is_service_write()
            || matches!(
                self,
                Self::ServicesServeRefresh
                    | Self::ServicesDriveRefresh
                    | Self::ServicesMetricsRefresh
                    | Self::ServicesDriveEnableAlpha
            )
    }

    pub(crate) const fn is_local_operator(self) -> bool {
        matches!(
            self,
            Self::LocalConnect
                | Self::LocalDisconnect
                | Self::LocalPreferencesEdit
                | Self::LocalExitNodeSelect
                | Self::LocalRoutesEditAdvertisements
                | Self::LocalAccountSwitch
                | Self::LocalAccountLogin
                | Self::LocalAccountLogout
                | Self::LocalAccountRemove
                | Self::LocalSshOpen
                | Self::LocalNcOpen
                | Self::LocalSyspolicyReload
        )
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ActionContext {
    Root,
    Collection,
    Detail,
    Overlay,
    Audit,
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

    pub const fn style_role(self) -> crate::ui::theme::StyleRole {
        match self {
            Self::Observe => crate::ui::theme::StyleRole::RiskObserve,
            Self::Reversible => crate::ui::theme::StyleRole::RiskReversible,
            Self::Disruptive => crate::ui::theme::StyleRole::RiskDisruptive,
            Self::DestructiveOrSecret => crate::ui::theme::StyleRole::RiskDestructive,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Binding {
    Char(char),
    Ctrl(char),
    Enter,
    Tab,
    BackTab,
}

/// Every printable ASCII character as its own string, indexed by code point.
/// This is a table rather than a match so that binding a key Tale has not used
/// before cannot silently render as a placeholder.
const PRINTABLE_ASCII: [&str; 95] = [
    " ", "!", "\"", "#", "$", "%", "&", "'", "(", ")", "*", "+", ",", "-", ".", "/", "0", "1", "2",
    "3", "4", "5", "6", "7", "8", "9", ":", ";", "<", "=", ">", "?", "@", "A", "B", "C", "D", "E",
    "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X",
    "Y", "Z", "[", "\\", "]", "^", "_", "`", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k",
    "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "{", "|", "}", "~",
];

const fn printable_label(character: char) -> &'static str {
    let index = (character as u32).wrapping_sub(0x20) as usize;
    if index < PRINTABLE_ASCII.len() {
        PRINTABLE_ASCII[index]
    } else {
        "key"
    }
}

impl Binding {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Char(' ') => "Space",
            Self::Char(character) => printable_label(character),
            Self::Ctrl('d') => "C-d",
            Self::Ctrl('u') => "C-u",
            Self::Enter => "Enter",
            Self::Tab => "Tab",
            Self::BackTab => "S-Tab",
            Self::Ctrl(_) => "C-key",
        }
    }

    pub fn matches(self, key: KeyEvent) -> bool {
        match self {
            // Shift is inherent to an uppercase character, not an extra
            // modifier, so `G` must match a key event that reports SHIFT.
            Self::Char(character) => {
                key.code == KeyCode::Char(character)
                    && key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
            }
            Self::Ctrl(character) => {
                key.code == KeyCode::Char(character)
                    && key.modifiers.contains(KeyModifiers::CONTROL)
            }
            Self::Enter => key.code == KeyCode::Enter && key.modifiers.is_empty(),
            Self::Tab => key.code == KeyCode::Tab && key.modifiers.is_empty(),
            // Crossterm reports Shift-Tab as BackTab and still sets SHIFT.
            Self::BackTab => {
                key.code == KeyCode::BackTab
                    && key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
            }
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
    ActionContext::Audit,
];
const NAVIGATION: &[ActionContext] = &[
    ActionContext::Collection,
    ActionContext::Detail,
    ActionContext::Audit,
];
const COLLECTION: &[ActionContext] = &[ActionContext::Collection, ActionContext::Detail];
const COLLECTION_OR_AUDIT: &[ActionContext] = &[
    ActionContext::Collection,
    ActionContext::Detail,
    ActionContext::Audit,
];
const AUDIT: &[ActionContext] = &[ActionContext::Audit];
const OVERLAY: &[ActionContext] = &[ActionContext::Overlay];

const NO_BINDING: &[Binding] = &[];
const BIND_Q: &[Binding] = &[Binding::Char('q')];
const BIND_COLON: &[Binding] = &[Binding::Char(':')];
const BIND_SLASH: &[Binding] = &[Binding::Char('/')];
const BIND_NEXT_MATCH: &[Binding] = &[Binding::Char('n')];
const BIND_PREVIOUS_MATCH: &[Binding] = &[Binding::Char('N')];
const BIND_R: &[Binding] = &[Binding::Char('r')];
const BIND_BIG_R: &[Binding] = &[Binding::Char('R')];
const BIND_HELP: &[Binding] = &[Binding::Char('?')];
const BIND_TASKS: &[Binding] = &[Binding::Char('@')];
const BIND_HISTORY_BACK: &[Binding] = &[Binding::Char('[')];
const BIND_HISTORY_FORWARD: &[Binding] = &[Binding::Char(']')];
const BIND_UP: &[Binding] = &[Binding::Char('k')];
const BIND_DOWN: &[Binding] = &[Binding::Char('j')];
const BIND_FIRST: &[Binding] = &[Binding::Char('g')];
const BIND_LAST: &[Binding] = &[Binding::Char('G')];
const BIND_PAGE_UP: &[Binding] = &[Binding::Ctrl('u')];
const BIND_PAGE_DOWN: &[Binding] = &[Binding::Ctrl('d')];
const BIND_OPEN: &[Binding] = &[Binding::Enter, Binding::Char('l')];
const BIND_BACK: &[Binding] = &[Binding::Char('h')];
const BIND_SORT: &[Binding] = &[Binding::Char('s')];
const BIND_WIDE: &[Binding] = &[Binding::Char('w')];
const BIND_INSPECT: &[Binding] = &[Binding::Char('i')];
const BIND_ACTIONS: &[Binding] = &[Binding::Char('a')];
const BIND_COPY: &[Binding] = &[Binding::Char('y')];
const BIND_CANCEL: &[Binding] = &[Binding::Char('x')];

const BIND_ACTIONS_ROOT: &[Binding] = &[Binding::Char('a')];

pub fn shell_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec {
            id: ActionId::AppQuit,
            label: "Quit",
            description: "Quit Tale",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_Q,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewCommandLine,
            label: "Go to view",
            description: "Open the inline route command line",
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
            contexts: &[ActionContext::Collection, ActionContext::Audit],
            selection_rule: SelectionRule::None,
            default_bindings: BIND_SLASH,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::DetailSearch,
            label: "Search",
            description: "Search within the open detail view",
            contexts: &[ActionContext::Detail],
            selection_rule: SelectionRule::None,
            default_bindings: BIND_SLASH,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::DeviceDetailNextMatch,
            label: "Next match",
            description: "Jump to the next device-detail search match",
            contexts: &[ActionContext::Detail],
            selection_rule: SelectionRule::One,
            default_bindings: BIND_NEXT_MATCH,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::DeviceDetailPreviousMatch,
            label: "Previous match",
            description: "Jump to the previous device-detail search match",
            contexts: &[ActionContext::Detail],
            selection_rule: SelectionRule::One,
            default_bindings: BIND_PREVIOUS_MATCH,
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
            id: ActionId::ViewHistoryBack,
            label: "Back",
            description: "Restore the previous view frame",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_HISTORY_BACK,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewHistoryForward,
            label: "Forward",
            description: "Restore the next view frame",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_HISTORY_FORWARD,
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
            id: ActionId::CollectionBack,
            label: "Back",
            description: "Leave the detail pane and return to the list",
            contexts: &[ActionContext::Detail],
            selection_rule: SelectionRule::None,
            default_bindings: BIND_BACK,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::CollectionOpen,
            label: "Open details",
            description: "Open selected resource details",
            contexts: COLLECTION_OR_AUDIT,
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
        // `Enter` opens the inspector and `h` leaves it; one key that does both
        // is what you want when the pane is a reference you glance at.
        ActionSpec {
            id: ActionId::CollectionInspect,
            label: "Inspector",
            description: "Show or hide the inspector pane",
            contexts: COLLECTION_OR_AUDIT,
            selection_rule: SelectionRule::None,
            default_bindings: BIND_INSPECT,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ResourceActions,
            label: "Actions",
            description: "Open actions for this view and the selected resource",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
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
            contexts: COLLECTION,
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

pub fn find_action(id: ActionId) -> Option<&'static ActionSpec> {
    let catalog = catalog();
    catalog
        .by_id
        .get(&id)
        .and_then(|index| catalog.specs.get(*index))
}

pub fn action_for_key(key: KeyEvent, context: ActionContext) -> Option<ActionId> {
    all_actions().iter().find_map(|spec| {
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

/// Stable keys used after the `a` prefix. They are deliberately explicit: the
/// UI never invents a mnemonic from a label or from the order of a list.
pub const fn transient_sequence(id: ActionId) -> Option<&'static str> {
    match id {
        ActionId::MockSuccess => Some("ms"),
        ActionId::MockFailure => Some("mf"),
        ActionId::MockCancellable => Some("mc"),
        ActionId::MockNonCancellable => Some("mn"),
        ActionId::LocalConnect => Some("c"),
        ActionId::LocalDisconnect => Some("d"),
        ActionId::LocalPreferencesEdit => Some("p"),
        ActionId::LocalExitNodeSelect => Some("e"),
        ActionId::LocalRoutesEditAdvertisements => Some("r"),
        ActionId::LocalAccountSwitch => Some("as"),
        ActionId::LocalAccountLogin => Some("al"),
        ActionId::LocalAccountLogout => Some("ao"),
        ActionId::LocalAccountRemove => Some("ar"),
        ActionId::LocalSyspolicyReload => Some("y"),
        ActionId::LocalProbeConnection => Some("g"),
        ActionId::LocalWhois => Some("w"),
        ActionId::LocalSshOpen => Some("ss"),
        ActionId::LocalNcOpen => Some("nc"),
        ActionId::DiagnosticCopy => Some("ic"),
        ActionId::LocalNetcheck => Some("dn"),
        ActionId::LocalNetcheckLive => Some("dl"),
        ActionId::LocalDnsStatus => Some("ds"),
        ActionId::LocalDnsQuery => Some("dq"),
        ActionId::ServicesServeRefresh => Some("r"),
        ActionId::ServicesServeCreate => Some("ct"),
        ActionId::ServicesFunnelCreate => Some("cp"),
        ActionId::ServicesServeEdit => Some("e"),
        // `d` deletes the one selected row; the `x` prefix stays reserved for
        // the two resets so a single letter never means "all of them".
        ActionId::ServicesServeRemove => Some("d"),
        ActionId::ServicesFunnelUnpublish => Some("u"),
        ActionId::ServicesServeReset => Some("xt"),
        ActionId::ServicesFunnelReset => Some("xp"),
        // `f` for files, drilled into, because the devices menu already spends
        // `s`, `r` and `d` on admin device actions.
        ActionId::DevicesTaildropSend => Some("fs"),
        ActionId::DevicesTaildropReceive => Some("fr"),
        ActionId::ServicesDriveRefresh => Some("r"),
        ActionId::ServicesDriveShare => Some("s"),
        ActionId::ServicesDriveRename => Some("n"),
        ActionId::ServicesDriveUnshare => Some("u"),
        ActionId::ServicesDriveEnableAlpha => Some("e"),
        ActionId::ServicesCertificateObtain => Some("o"),
        ActionId::ServicesMetricsRefresh => Some("r"),
        ActionId::ServicesBugReportCreate => Some("c"),
        ActionId::AdminDeviceRename => Some("r"),
        ActionId::AdminDeviceTagsReplace => Some("t"),
        ActionId::AdminDeviceApprove => Some("a"),
        // Not `v`: the saved-view sequences are `v` followed by a letter, and a
        // leaf that is also a prefix makes the whole devices menu unopenable.
        ActionId::AdminDeviceRevokeApproval => Some("u"),
        ActionId::AdminDeviceKeyExpiryConfigure => Some("k"),
        ActionId::AdminDeviceKeyExpireNow => Some("x"),
        ActionId::AdminDeviceDelete => Some("d"),
        ActionId::AdminUserApprove => Some("a"),
        ActionId::AdminUserRoleChange => Some("r"),
        ActionId::AdminUserSuspend => Some("s"),
        ActionId::AdminUserRestore => Some("u"),
        ActionId::AdminUserDelete => Some("d"),
        ActionId::AdminRoutesReplaceApprovals => Some("r"),
        ActionId::AdminDnsPreferencesEdit => Some("p"),
        ActionId::AdminDnsNameserversReplace => Some("n"),
        ActionId::AdminDnsSearchPathsReplace => Some("h"),
        ActionId::AdminDnsSplitCreate => Some("sc"),
        ActionId::AdminDnsSplitEdit => Some("se"),
        ActionId::AdminDnsSplitRemove => Some("sd"),
        ActionId::AdminPolicyEdit | ActionId::AdminPolicyEditorReopen => Some("pe"),
        ActionId::AdminPolicyCandidateDiscard => Some("px"),
        ActionId::AdminPolicyRemoteRefresh => Some("pr"),
        ActionId::AdminPolicyValidate => Some("pv"),
        ActionId::AdminPolicyPreview => Some("pp"),
        ActionId::AdminPolicyDiff => Some("pd"),
        ActionId::AdminPolicyApply => Some("pa"),
        ActionId::AdminPolicyWorkflowClose => Some("pc"),
        ActionId::ProfileActivate => Some("a"),
        ActionId::SavedViewCreate => Some("vc"),
        ActionId::SavedViewReplace => Some("vr"),
        ActionId::SavedViewRename => Some("vn"),
        ActionId::SavedViewDelete => Some("vd"),
        ActionId::SavedViewApply => Some("va"),
        ActionId::CollectionExport => Some("zx"),
        ActionId::OverviewHealthOpenResource => Some("ho"),
        ActionId::OverviewHealthRunSuggestedAction => Some("hr"),
        ActionId::AccessExplorerAsk => Some("ea"),
        ActionId::AccessExplorerOpenRule => Some("eo"),
        ActionId::ActivityFlowsSelectWindow => Some("fw"),
        ActionId::ActivityFlowsAggregate => Some("fa"),
        ActionId::ActivityFlowsOpenDevice => Some("fd"),
        ActionId::AdminWebhookCreate => Some("wc"),
        ActionId::AdminWebhookEdit => Some("we"),
        ActionId::AdminWebhookTest => Some("wt"),
        ActionId::AdminWebhookRotateSecret => Some("wr"),
        ActionId::AdminWebhookDelete => Some("wd"),
        ActionId::AdminLogStreamReplace => Some("lr"),
        ActionId::AdminLogStreamDelete => Some("ld"),
        ActionId::AdminNetworkLogsSettings => Some("ln"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TransientGroup {
    Simulation,
    Machine,
    Account,
    Diagnostics,
    Handoff,
    Serve,
    Funnel,
    Taildrop,
    Taildrive,
    Certificates,
    Monitoring,
    Device,
    User,
    Routing,
    Dns,
    SplitDns,
    Policy,
    Profile,
    Views,
    Data,
    Health,
    Explorer,
    Flows,
    Webhooks,
    Logging,
    Danger,
}

impl TransientGroup {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Simulation => "Simulation",
            Self::Machine => "Machine",
            Self::Account => "Account",
            Self::Diagnostics => "Diagnostics",
            Self::Handoff => "Remote access",
            Self::Serve => "Serve",
            Self::Funnel => "Funnel",
            Self::Taildrop => "Taildrop",
            Self::Taildrive => "Taildrive",
            Self::Certificates => "Certificates",
            Self::Monitoring => "Monitoring",
            Self::Device => "Device",
            Self::User => "User",
            Self::Routing => "Routing",
            Self::Dns => "DNS",
            Self::SplitDns => "Split DNS",
            Self::Policy => "Policy",
            Self::Profile => "Profile",
            Self::Views => "Views",
            Self::Data => "Data",
            Self::Health => "Health",
            Self::Explorer => "Explorer",
            Self::Flows => "Flows",
            Self::Webhooks => "Webhooks",
            Self::Logging => "Logging",
            Self::Danger => "Danger",
        }
    }
}

pub const fn transient_group(id: ActionId) -> Option<TransientGroup> {
    match id {
        ActionId::MockSuccess
        | ActionId::MockFailure
        | ActionId::MockCancellable
        | ActionId::MockNonCancellable => Some(TransientGroup::Simulation),
        ActionId::LocalConnect
        | ActionId::LocalDisconnect
        | ActionId::LocalPreferencesEdit
        | ActionId::LocalExitNodeSelect
        | ActionId::LocalRoutesEditAdvertisements
        | ActionId::LocalSyspolicyReload => Some(TransientGroup::Machine),
        ActionId::LocalAccountSwitch
        | ActionId::LocalAccountLogin
        | ActionId::LocalAccountLogout => Some(TransientGroup::Account),
        ActionId::LocalAccountRemove
        | ActionId::AdminDeviceRevokeApproval
        | ActionId::AdminDeviceKeyExpireNow
        | ActionId::AdminDeviceDelete
        | ActionId::AdminUserSuspend
        | ActionId::AdminUserDelete
        | ActionId::AdminWebhookRotateSecret
        | ActionId::AdminWebhookDelete
        | ActionId::AdminLogStreamDelete => Some(TransientGroup::Danger),
        ActionId::LocalProbeConnection
        | ActionId::LocalWhois
        | ActionId::DiagnosticCopy
        | ActionId::LocalNetcheck
        | ActionId::LocalNetcheckLive
        | ActionId::LocalDnsStatus
        | ActionId::LocalDnsQuery => Some(TransientGroup::Diagnostics),
        ActionId::LocalSshOpen | ActionId::LocalNcOpen => Some(TransientGroup::Handoff),
        ActionId::ProfileActivate => Some(TransientGroup::Profile),
        ActionId::ServicesServeRefresh
        | ActionId::ServicesServeCreate
        | ActionId::ServicesServeEdit
        | ActionId::ServicesServeRemove
        | ActionId::ServicesServeReset
        | ActionId::ServicesFunnelCreate
        | ActionId::ServicesFunnelEdit
        | ActionId::ServicesFunnelUnpublish
        | ActionId::ServicesFunnelReset => Some(TransientGroup::Serve),
        ActionId::DevicesTaildropSend | ActionId::DevicesTaildropReceive => {
            Some(TransientGroup::Taildrop)
        }
        ActionId::ServicesDriveRefresh
        | ActionId::ServicesDriveShare
        | ActionId::ServicesDriveRename
        | ActionId::ServicesDriveUnshare
        | ActionId::ServicesDriveEnableAlpha => Some(TransientGroup::Taildrive),
        ActionId::ServicesCertificateObtain => Some(TransientGroup::Certificates),
        ActionId::ServicesMetricsRefresh | ActionId::ServicesBugReportCreate => {
            Some(TransientGroup::Monitoring)
        }
        ActionId::AdminDeviceRename
        | ActionId::AdminDeviceTagsReplace
        | ActionId::AdminDeviceApprove
        | ActionId::AdminDeviceKeyExpiryConfigure => Some(TransientGroup::Device),
        ActionId::AdminUserApprove | ActionId::AdminUserRoleChange | ActionId::AdminUserRestore => {
            Some(TransientGroup::User)
        }
        ActionId::AdminRoutesReplaceApprovals => Some(TransientGroup::Routing),
        ActionId::AdminDnsPreferencesEdit
        | ActionId::AdminDnsNameserversReplace
        | ActionId::AdminDnsSearchPathsReplace => Some(TransientGroup::Dns),
        ActionId::AdminDnsSplitCreate
        | ActionId::AdminDnsSplitEdit
        | ActionId::AdminDnsSplitRemove => Some(TransientGroup::SplitDns),
        ActionId::AdminPolicyEdit
        | ActionId::AdminPolicyEditorReopen
        | ActionId::AdminPolicyCandidateDiscard
        | ActionId::AdminPolicyRemoteRefresh
        | ActionId::AdminPolicyValidate
        | ActionId::AdminPolicyPreview
        | ActionId::AdminPolicyDiff
        | ActionId::AdminPolicyApply
        | ActionId::AdminPolicyWorkflowClose => Some(TransientGroup::Policy),
        ActionId::SavedViewCreate
        | ActionId::SavedViewReplace
        | ActionId::SavedViewRename
        | ActionId::SavedViewDelete
        | ActionId::SavedViewApply => Some(TransientGroup::Views),
        ActionId::CollectionExport => Some(TransientGroup::Data),
        ActionId::OverviewHealthOpenResource | ActionId::OverviewHealthRunSuggestedAction => {
            Some(TransientGroup::Health)
        }
        ActionId::AccessExplorerAsk | ActionId::AccessExplorerOpenRule => {
            Some(TransientGroup::Explorer)
        }
        ActionId::ActivityFlowsSelectWindow
        | ActionId::ActivityFlowsAggregate
        | ActionId::ActivityFlowsOpenDevice => Some(TransientGroup::Flows),
        ActionId::AdminWebhookCreate | ActionId::AdminWebhookEdit | ActionId::AdminWebhookTest => {
            Some(TransientGroup::Webhooks)
        }
        ActionId::AdminLogStreamReplace | ActionId::AdminNetworkLogsSettings => {
            Some(TransientGroup::Logging)
        }
        _ => None,
    }
}

pub fn validate_transient_sequences(actions: &[ActionId]) -> Result<(), String> {
    let mut sequences = std::collections::BTreeMap::new();
    for id in actions {
        let sequence = transient_sequence(*id)
            .ok_or_else(|| format!("visible action has no transient sequence: {}", id.as_str()))?;
        if sequence.is_empty() || sequence.chars().count() > 2 {
            return Err(format!("invalid transient depth for {}", id.as_str()));
        }
        if matches!(sequence, "[" | "]" | "q" | ":" | "/" | "?") {
            return Err(format!("reserved transient sequence: {sequence}"));
        }
        if let Some(other) = sequences.insert(sequence, *id) {
            return Err(format!(
                "duplicate transient sequence {sequence}: {} and {}",
                other.as_str(),
                id.as_str()
            ));
        }
    }
    for sequence in sequences.keys() {
        if sequence.chars().count() == 1
            && sequences
                .keys()
                .any(|candidate| candidate.len() == 2 && candidate.starts_with(*sequence))
        {
            return Err(format!("transient leaf is also a prefix: {sequence}"));
        }
    }
    Ok(())
}

pub fn footer_hints(context: ActionContext, route: Route, width: u16) -> Vec<String> {
    footer_actions(context, route, width)
        .into_iter()
        .map(|hint| hint.text())
        .collect()
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct FooterHint {
    pub action_id: ActionId,
    pub key: &'static str,
    pub label: &'static str,
}

pub const FOOTER_MAX_ROWS: usize = 2;

impl FooterHint {
    pub fn text(self) -> String {
        format!("{} {}", self.key, self.label)
    }

    pub fn width(self) -> usize {
        self.key
            .chars()
            .count()
            .saturating_add(1)
            .saturating_add(self.label.chars().count())
    }
}

pub fn footer_rows(hints: &[FooterHint], width: u16) -> Vec<Vec<FooterHint>> {
    let width = usize::from(width);
    let mut rows = vec![Vec::new()];
    let mut used = 0usize;
    for hint in hints {
        let separator = usize::from(!rows.last().is_none_or(Vec::is_empty)) * 2;
        if used.saturating_add(separator).saturating_add(hint.width()) > width
            && rows.last().is_some_and(|row| !row.is_empty())
        {
            rows.push(Vec::new());
            used = 0;
        }
        let separator = usize::from(!rows.last().is_none_or(Vec::is_empty)) * 2;
        used = used.saturating_add(separator).saturating_add(hint.width());
        if let Some(row) = rows.last_mut() {
            row.push(*hint);
        }
    }
    rows
}

pub fn footer_actions(context: ActionContext, route: Route, width: u16) -> Vec<FooterHint> {
    footer_actions_filtered(context, route, width, |_| true)
}

pub fn footer_actions_filtered(
    context: ActionContext,
    route: Route,
    width: u16,
    include: impl Fn(ActionId) -> bool,
) -> Vec<FooterHint> {
    let mut specs = all_actions()
        .iter()
        .filter(|spec| {
            spec.contexts.contains(&context)
                && !spec.default_bindings.is_empty()
                && spec.id != ActionId::ViewHelp
                && applies_to_route(spec.id, route)
                && include(spec.id)
        })
        .collect::<Vec<_>>();
    specs.sort_by_key(|spec| footer_priority(spec.id));
    let available = specs
        .into_iter()
        .filter_map(|spec| {
            Some(FooterHint {
                action_id: spec.id,
                key: spec.default_bindings[0].label(),
                label: compact_help_label(spec.id)?,
            })
        })
        .collect::<Vec<_>>();

    for visible in (0..=available.len()).rev() {
        let hidden = visible < available.len();
        let mut hints = available[..visible].to_vec();
        let help_index = hints
            .iter()
            .take_while(|hint| {
                matches!(
                    hint.action_id,
                    ActionId::ViewCommandLine | ActionId::ViewFilter | ActionId::DetailSearch
                )
            })
            .count();
        hints.insert(
            help_index,
            FooterHint {
                action_id: ActionId::ViewHelp,
                key: "?",
                label: if hidden { "more" } else { "help" },
            },
        );
        let rows = footer_rows(&hints, width);
        if rows.len() <= FOOTER_MAX_ROWS
            && hints.iter().all(|hint| hint.width() <= usize::from(width))
        {
            return hints;
        }
    }

    Vec::new()
}

/// Some keys are bound in a shared context but only mean something on one
/// route. Offering them everywhere is how `w columns` ended up on a screen with
/// no columns and `S section` never appeared on the one screen that has them.
pub const fn applies_to_route(id: ActionId, route: Route) -> bool {
    match id {
        ActionId::SectionNext | ActionId::SectionPrevious => {
            matches!(route, Route::Local | Route::Services | Route::Diagnostics)
        }
        ActionId::CollectionWideColumns => matches!(route, Route::Devices),
        ActionId::DetailSearch => matches!(
            route,
            Route::Overview
                | Route::Local
                | Route::Devices
                | Route::Users
                | Route::Routes
                | Route::Dns
                | Route::Access
                | Route::Credentials
                | Route::Profiles
                | Route::Tasks
                | Route::Audit
                | Route::Services
                | Route::Diagnostics
        ),
        ActionId::DeviceDetailNextMatch | ActionId::DeviceDetailPreviousMatch => {
            matches!(route, Route::Devices | Route::Access)
        }
        // Every route here keeps a table with a row worth describing, and each
        // starts with the pane closed.
        ActionId::CollectionInspect => {
            matches!(
                route,
                Route::Devices
                    | Route::Users
                    | Route::Routes
                    | Route::Credentials
                    | Route::Profiles
                    | Route::Tasks
                    | Route::Audit
                    | Route::Services
            )
        }
        // A task is this client's own record, so cancelling and reviewing one
        // only mean something on the page that lists them.
        ActionId::TaskCancel | ActionId::BatchReviewOutcomes | ActionId::BatchRetrySelected => {
            matches!(route, Route::Tasks)
        }
        // Sorting offers device fields, so it is offered where those fields
        // are. Tasks are already in the order they happened, which is the only
        // order a history reads well in.
        ActionId::CollectionSort => {
            matches!(
                route,
                Route::Devices | Route::Profiles | Route::Config | Route::Services
            )
        }
        // Diagnostics is one scrolling body: nothing to open into, no rows to
        // filter, no columns to order by.
        ActionId::CollectionOpen => {
            !matches!(route, Route::Local | Route::Config | Route::Diagnostics)
        }
        ActionId::ViewFilter => {
            matches!(
                route,
                Route::Devices
                    | Route::Users
                    | Route::Routes
                    | Route::Credentials
                    | Route::Profiles
                    | Route::Config
                    | Route::Services
                    | Route::Tasks
                    | Route::Audit
            )
        }
        ActionId::ResourceCopy => matches!(
            route,
            Route::Devices
                | Route::Users
                | Route::Profiles
                | Route::Config
                | Route::Services
                | Route::Tasks
                | Route::Diagnostics
        ),
        _ => true,
    }
}

const fn footer_priority(id: ActionId) -> u8 {
    match id {
        ActionId::ViewCommandLine => 0,
        ActionId::ViewFilter | ActionId::DetailSearch => 1,
        ActionId::ResourceActions => 3,
        ActionId::ResourceCopy => 4,
        ActionId::ViewTasks => 5,
        ActionId::CollectionMoveUp => 10,
        ActionId::CollectionMoveDown => 11,
        ActionId::CollectionOpen => 12,
        ActionId::CollectionBack | ActionId::TaskCancel => 13,
        ActionId::SectionNext | ActionId::DeviceDetailNextMatch => 14,
        ActionId::SectionPrevious | ActionId::DeviceDetailPreviousMatch => 15,
        ActionId::CollectionSort => 16,
        ActionId::CollectionWideColumns => 17,
        ActionId::CollectionInspect => 18,
        ActionId::ViewRefresh => 20,
        ActionId::ViewRefreshAll => 21,
        ActionId::ViewHistoryBack => 22,
        ActionId::ViewHistoryForward => 23,
        ActionId::CollectionFirst => 30,
        ActionId::CollectionLast => 31,
        ActionId::CollectionPageUp => 32,
        ActionId::CollectionPageDown => 33,
        ActionId::AppQuit => 40,
        _ => u8::MAX,
    }
}

pub const fn compact_help_label(id: ActionId) -> Option<&'static str> {
    match id {
        ActionId::AppQuit => Some("quit"),
        ActionId::ViewCommandLine => Some("command"),
        ActionId::ViewFilter => Some("filter"),
        ActionId::DetailSearch => Some("search"),
        ActionId::DeviceDetailNextMatch => Some("next"),
        ActionId::DeviceDetailPreviousMatch => Some("previous"),
        ActionId::ViewRefresh => Some("refresh"),
        ActionId::ViewRefreshAll => Some("refresh-all"),
        ActionId::ViewHelp => Some("help"),
        ActionId::ViewTasks => Some("tasks"),
        ActionId::ViewHistoryBack => Some("back"),
        ActionId::ViewHistoryForward => Some("forward"),
        ActionId::CollectionMoveUp => Some("up"),
        ActionId::CollectionMoveDown => Some("down"),
        ActionId::CollectionFirst => Some("first"),
        ActionId::CollectionLast => Some("last"),
        ActionId::CollectionPageUp => Some("page-up"),
        ActionId::CollectionPageDown => Some("page-down"),
        ActionId::CollectionOpen => Some("open"),
        ActionId::CollectionBack => Some("back"),
        ActionId::CollectionSort => Some("sort"),
        ActionId::CollectionWideColumns => Some("columns"),
        ActionId::CollectionInspect => Some("inspector"),
        ActionId::ResourceActions => Some("actions"),
        ActionId::ResourceCopy => Some("copy"),
        ActionId::TaskCancel => Some("cancel"),
        ActionId::SectionNext => Some("next tab"),
        ActionId::SectionPrevious => Some("previous tab"),
        _ => None,
    }
}

pub fn local_observer_actions() -> Vec<ActionSpec> {
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
            label: "Ping",
            description: "Ping the selected peer over Tailscale",
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
            label: "Refresh DNS status",
            description: "Read the local Tailscale DNS status",
            contexts: &[ActionContext::Detail, ActionContext::Overlay],
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

pub fn local_operator_actions() -> Vec<ActionSpec> {
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
            selection_rule: SelectionRule::One,
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
            selection_rule: SelectionRule::One,
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
const TABBED_NAVIGATION: &[ActionContext] = &[ActionContext::Collection, ActionContext::Detail];

pub fn local_service_actions() -> Vec<ActionSpec> {
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
            id: ActionId::ViewDiagnostics,
            label: "Diagnostics",
            description: "Open client metrics and the bug report",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::SectionNext,
            label: "Next tab",
            description: "Move to the next tab",
            contexts: TABBED_NAVIGATION,
            selection_rule: SelectionRule::None,
            default_bindings: &[Binding::Tab],
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::SectionPrevious,
            label: "Previous tab",
            description: "Move to the previous tab",
            contexts: TABBED_NAVIGATION,
            selection_rule: SelectionRule::None,
            default_bindings: &[Binding::BackTab],
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ServicesServeRefresh,
            label: "Refresh mappings",
            description: "Re-read Serve and Funnel mappings",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ServicesServeCreate,
            label: "Create tailnet mapping",
            description: "Serve a backend to the tailnet only",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::ServicesServeEdit,
            label: "Edit mapping",
            description: "Change what the selected mapping serves",
            contexts: SERVICES,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::ServicesServeRemove,
            label: "Remove mapping",
            description: "Remove only the selected mapping",
            contexts: SERVICES,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::ServicesServeReset,
            label: "Remove all tailnet mappings",
            description: "Remove every local Serve mapping",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::ServicesFunnelCreate,
            label: "Create public mapping",
            description: "Expose one mapping publicly through Funnel",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::ServicesFunnelEdit,
            label: "Edit public mapping",
            description: "Replace one public Funnel mapping",
            contexts: SERVICES,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::ServicesFunnelUnpublish,
            label: "Stop publishing",
            description: "Keep the selected mapping but make it tailnet-only",
            contexts: SERVICES,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::ServicesFunnelReset,
            label: "Remove all public mappings",
            description: "Remove every public Funnel mapping",
            contexts: SERVICES,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        // Taildrop is device to device, so it lives on `:devices`: the selected
        // row is the target rather than a second list of the same machines.
        ActionSpec {
            id: ActionId::DevicesTaildropSend,
            label: "Send files",
            description: "Send regular files to the selected device",
            contexts: SERVICES,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::DevicesTaildropReceive,
            label: "Receive files",
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

/// The registry every availability check reads. Assembling it allocates every
/// spec in the program, and a single frame consults it once per action per
/// footer row, so it is built once and borrowed from there on.
struct Catalog {
    specs: Vec<ActionSpec>,
    by_id: HashMap<ActionId, usize>,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    let mut specs = shell_actions();
    specs.extend(local_observer_actions());
    specs.extend(local_operator_actions());
    specs.extend(local_service_actions());
    specs.extend(admin_observer_actions());
    specs.extend(admin_operator_actions());
    specs.extend(policy_and_credential_actions());
    specs.extend(operational_actions());
    let by_id = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| (spec.id, index))
        .collect();
    Catalog { specs, by_id }
});

fn catalog() -> &'static Catalog {
    &CATALOG
}

pub fn all_actions() -> &'static [ActionSpec] {
    &catalog().specs
}

pub fn operational_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec {
            id: ActionId::OverviewHealthOpenResource,
            label: "Open affected item",
            description: "Open the item behind an issue found by Tale",
            contexts: GLOBAL,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::OverviewHealthRunSuggestedAction,
            label: "Run suggested action",
            description: "Review and run an action suggested for an issue",
            contexts: GLOBAL,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::ActivityFlowsSelectWindow,
            label: "Select flow window",
            description: "Choose a UTC time range for flow logs",
            contexts: AUDIT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ActivityFlowsAggregate,
            label: "Aggregate flow logs",
            description: "Aggregate the observed flow counters by registered dimensions",
            contexts: AUDIT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ActivityFlowsOpenDevice,
            label: "Open flow device",
            description: "Open the reporting device for a selected flow row",
            contexts: AUDIT,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AdminWebhookCreate,
            label: "Create webhook",
            description: "Create a typed HTTPS webhook endpoint",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::AdminWebhookEdit,
            label: "Edit webhook",
            description: "Replace documented webhook subscriptions while preserving unknown values",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::AdminWebhookTest,
            label: "Test webhook",
            description: "Queue a webhook test and show the refreshed server result",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AdminWebhookRotateSecret,
            label: "Rotate webhook secret",
            description: "Rotate a write-only webhook secret and show it once",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::AdminWebhookDelete,
            label: "Delete webhook",
            description: "Delete an exact webhook endpoint after confirmation",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::AdminLogStreamReplace,
            label: "Replace log stream",
            description: "Replace one documented log-stream configuration",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::AdminLogStreamDelete,
            label: "Delete log stream",
            description: "Delete one documented log-stream configuration",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::AdminNetworkLogsSettings,
            label: "Network log settings",
            description: "Change only the documented network-flow logging setting",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::SavedViewCreate,
            label: "Create saved view",
            description: "Persist the current registered view definition",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::SavedViewReplace,
            label: "Replace saved view",
            description: "Replace a saved view after showing the full definition",
            contexts: GLOBAL,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::SavedViewRename,
            label: "Rename saved view",
            description: "Rename one saved view without changing its definition",
            contexts: GLOBAL,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::SavedViewDelete,
            label: "Delete saved view",
            description: "Delete one saved view after a review",
            contexts: GLOBAL,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::SavedViewApply,
            label: "Apply saved view",
            description: "Apply a strict registered saved view",
            contexts: GLOBAL,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::CollectionExport,
            label: "Export collection",
            description:
                "Export the active filtered and sorted collection using an allowlisted schema",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AccessExplorerAsk,
            label: "Ask Access Explorer",
            description: "Ask the server's documented policy preview for an access result",
            contexts: &[ActionContext::Collection, ActionContext::Detail],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AccessExplorerOpenRule,
            label: "Open matched rule",
            description: "Open the matching policy rule",
            contexts: &[ActionContext::Collection, ActionContext::Detail],
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
    ]
}

pub fn admin_observer_actions() -> Vec<ActionSpec> {
    vec![
        // The subject is the selected row, so this belongs to the collection the
        // way every other row action does. Selecting `local` is the same act as
        // selecting a profile, which is why there is no separate clear.
        ActionSpec {
            id: ActionId::ProfileActivate,
            label: "Activate",
            description: "Verify this credential against the control plane and make it active",
            contexts: COLLECTION,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AdminRefreshCurrent,
            label: "Refresh admin view",
            description: "Refresh the current admin sources",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AdminRefreshAll,
            label: "Refresh all admin",
            description: "Refresh every selected admin source",
            contexts: GLOBAL,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewProfiles,
            label: "Profiles",
            description: "Open the local client and the configured admin profiles",
            contexts: ROOT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewUsers,
            label: "Users",
            description: "Open read-only tailnet users",
            contexts: ROOT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewRoutes,
            label: "Routes",
            description: "Open read-only device route inventory",
            contexts: ROOT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewDns,
            label: "DNS",
            description: "Open read-only DNS configuration",
            contexts: ROOT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewAccess,
            label: "Access",
            description: "Open the preserved read-only policy source",
            contexts: ROOT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ViewCredentials,
            label: "Credentials",
            description: "Open non-secret credential metadata",
            contexts: ROOT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::UsersOpenDevices,
            label: "Open user devices",
            description: "Filter devices by exact owner ID",
            contexts: COLLECTION,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::RoutesOpenDevice,
            label: "Open route device",
            description: "Open a route's exact device",
            contexts: COLLECTION,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::DnsOpenLocalDiagnostics,
            label: "Open local DNS diagnostics",
            description: "Open local DNS observations beside admin DNS",
            contexts: ROOT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AccessCopySource,
            label: "Copy policy source",
            description: "Explicitly copy the preserved policy source",
            contexts: &[ActionContext::Root, ActionContext::Collection],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ActivitySelectWindow,
            label: "Select audit window",
            description: "Choose a time range for activity",
            contexts: AUDIT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ActivityOpenActor,
            label: "Open audit actor",
            description: "Open an audit actor when an exact ID is present",
            contexts: AUDIT,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::ActivityOpenTarget,
            label: "Open audit target",
            description: "Open an audit target when an exact ID is present",
            contexts: AUDIT,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::SettingsInspectCapabilities,
            label: "Inspect capabilities",
            description: "Show which administration features are available",
            contexts: ROOT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
    ]
}

const ADMIN_MUTATIONS: &[ActionContext] = &[
    ActionContext::Root,
    ActionContext::Collection,
    ActionContext::Detail,
];
const ADMIN_BATCH: &[ActionContext] = &[ActionContext::Collection, ActionContext::Detail];

pub fn admin_operator_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec {
            id: ActionId::AdminDeviceRename,
            label: "Rename device",
            description: "Change the device's machine name",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Reversible,
        },
        ActionSpec {
            id: ActionId::AdminDeviceTagsReplace,
            label: "Replace device tags",
            description: "Replace the complete device tag set",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminDeviceApprove,
            label: "Approve device",
            description: "Approve a device without signing Tailnet Lock material",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminDeviceRevokeApproval,
            label: "Revoke device approval",
            description: "Remove this device's approval to connect",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::AdminDeviceKeyExpiryConfigure,
            label: "Configure key expiry",
            description: "Choose whether this device's key expires",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminDeviceKeyExpireNow,
            label: "Expire device key now",
            description: "Expire the current device key and require reauthentication",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::AdminDeviceDelete,
            label: "Delete device",
            description: "Remove a device from the tailnet",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::AdminRoutesReplaceApprovals,
            label: "Replace route approvals",
            description: "Choose which advertised routes each device may use",
            contexts: ADMIN_BATCH,
            selection_rule: SelectionRule::Many,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminDnsPreferencesEdit,
            label: "Edit DNS preferences",
            description: "Set the tailnet MagicDNS preference",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminDnsNameserversReplace,
            label: "Replace nameservers",
            description: "Replace the complete ordered global nameserver list",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminDnsSearchPathsReplace,
            label: "Replace search paths",
            description: "Replace the complete ordered DNS search-path list",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminDnsSplitCreate,
            label: "Create split-DNS mapping",
            description: "Add one split-DNS suffix mapping",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminDnsSplitEdit,
            label: "Edit split-DNS mapping",
            description: "Replace one split-DNS suffix resolver set",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminDnsSplitRemove,
            label: "Remove split-DNS mapping",
            description: "Remove one split-DNS suffix mapping",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminUserApprove,
            label: "Approve user",
            description: "Approve a pending user",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminUserRoleChange,
            label: "Change user role",
            description: "Change the user's role after showing the access difference",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminUserSuspend,
            label: "Suspend user",
            description: "Suspend a user and review their owned devices",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::AdminUserRestore,
            label: "Restore user",
            description: "Restore a suspended user",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminUserDelete,
            label: "Delete user",
            description: "Delete a user after reviewing owned devices",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::BatchReviewOutcomes,
            label: "Review device results",
            description: "See which devices were updated and which failed",
            contexts: COLLECTION,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::BatchRetrySelected,
            label: "Retry selected devices",
            description: "Retry the selected devices using their latest settings",
            contexts: COLLECTION,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
    ]
}

pub fn policy_and_credential_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec {
            id: ActionId::AdminPolicyEdit,
            label: "Edit policy",
            description: "Open the preserved policy in the configured external editor",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::AdminPolicyEditorReopen,
            label: "Reopen policy editor",
            description: "Reopen the saved draft in the external editor",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminPolicyCandidateDiscard,
            label: "Discard policy draft",
            description: "Discard the saved policy draft after confirmation",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::AdminPolicyRemoteRefresh,
            label: "Refresh tailnet policy",
            description: "Fetch the latest tailnet policy before continuing",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AdminPolicyValidate,
            label: "Validate policy",
            description: "Ask Tailscale to validate the edited policy",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AdminPolicyPreview,
            label: "Preview permissions",
            description: "Preview how the edited policy changes access",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AdminPolicyDiff,
            label: "Show policy diff",
            description: "Compare the edited policy with the current tailnet policy",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AdminPolicyApply,
            label: "Apply policy",
            description: "Apply the edited policy after Tailscale validates it",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::AdminPolicyWorkflowClose,
            label: "Close policy editor",
            description: "Close editing and delete its temporary file",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Disruptive,
        },
        ActionSpec {
            id: ActionId::AdminCredentialAuthKeyCreate,
            label: "Create auth key",
            description: "Create a Tailscale auth key and show its secret once",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::SecretResultCopy,
            label: "Copy secret",
            description: "Explicitly copy the visible one-time secret",
            contexts: &[ActionContext::Overlay],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::SecretResultClose,
            label: "Close secret",
            description: "Destroy the one-time secret result",
            contexts: &[ActionContext::Overlay],
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::AdminCredentialRevoke,
            label: "Revoke remote credential",
            description: "Revoke one supported remote credential after exact-ID confirmation",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::ProfileCredentialRemove,
            label: "Remove local credential",
            description: "Remove the selected profile credential from Tale's keyring",
            contexts: ADMIN_MUTATIONS,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::DestructiveOrSecret,
        },
        ActionSpec {
            id: ActionId::AuditFilterTime,
            label: "Filter audit time",
            description: "Filter audit events by UTC time window",
            contexts: AUDIT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AuditFilterActor,
            label: "Filter audit actor",
            description: "Filter audit events by actor",
            contexts: AUDIT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AuditFilterAction,
            label: "Filter audit action",
            description: "Filter audit events by action",
            contexts: AUDIT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AuditFilterTarget,
            label: "Filter audit target",
            description: "Filter audit events by target",
            contexts: AUDIT,
            selection_rule: SelectionRule::None,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AuditOpenTarget,
            label: "Open audit target",
            description: "Open an exact target from an audit event",
            contexts: AUDIT,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
        ActionSpec {
            id: ActionId::AuditOpenPolicyDiff,
            label: "Open policy diff",
            description: "Open the retained policy workflow diff from an audit event",
            contexts: AUDIT,
            selection_rule: SelectionRule::One,
            default_bindings: NO_BINDING,
            capability: Capability::Available,
            risk: Risk::Observe,
        },
    ]
}
