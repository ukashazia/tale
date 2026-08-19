use super::*;

impl App {
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
        self.status_notice = None;
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
            ActionId::ViewFilter | ActionId::DetailSearch => {
                self.reduce_collection_action(action_id)
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
                self.load_visible_diagnostics()
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
                            admin_generation: self.admin_generation,
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
            ActionId::CollectionMoveUp
            | ActionId::CollectionMoveDown
            | ActionId::CollectionFirst
            | ActionId::CollectionLast
            | ActionId::CollectionPageUp
            | ActionId::CollectionPageDown
            | ActionId::CollectionBack
            | ActionId::CollectionOpen
            | ActionId::CollectionSort
            | ActionId::CollectionWideColumns
            | ActionId::CollectionInspect => self.reduce_collection_action(action_id),
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
            ActionId::LocalDiagnostics
            | ActionId::LocalProbeConnection
            | ActionId::LocalNetcheck
            | ActionId::LocalNetcheckLive
            | ActionId::LocalDnsStatus
            | ActionId::LocalDnsQuery
            | ActionId::LocalWhois
            | ActionId::DiagnosticCopy
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
            | ActionId::LocalSyspolicyReload => self.reduce_local_action(action_id),
            ActionId::SectionNext
            | ActionId::SectionPrevious
            | ActionId::ServicesServeRefresh
            | ActionId::ServicesDriveRefresh
            | ActionId::ServicesMetricsRefresh
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
            | ActionId::ServicesDriveEnableAlpha => self.reduce_service_action(action_id),
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
            | ActionId::SecretResultCopy
            | ActionId::SecretResultClose
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
            | ActionId::ActivityFlowsSelectWindow
            | ActionId::ActivityFlowsAggregate
            | ActionId::ActivityFlowsOpenDevice
            | ActionId::OverviewHealthOpenResource
            | ActionId::OverviewHealthRunSuggestedAction
            | ActionId::AdminWebhookCreate
            | ActionId::AdminWebhookEdit
            | ActionId::AdminWebhookTest
            | ActionId::AdminWebhookRotateSecret
            | ActionId::AdminWebhookDelete
            | ActionId::AdminLogStreamReplace
            | ActionId::AdminLogStreamDelete
            | ActionId::AdminNetworkLogsSettings
            | ActionId::AccessExplorerAsk
            | ActionId::AccessExplorerOpenRule => self.reduce_admin_action(action_id),
            ActionId::SavedViewCreate
            | ActionId::SavedViewReplace
            | ActionId::SavedViewRename
            | ActionId::SavedViewDelete
            | ActionId::SavedViewApply
            | ActionId::CollectionExport => self.open_local_operational_action(action_id),
        }
    }

    pub(super) fn action_available(&self, action_id: ActionId, capability: Capability) -> bool {
        if action_id == ActionId::ResourceCopy {
            return !self.contextual_copy_fields().is_empty();
        }
        match capability {
            Capability::Available if action_id.is_admin() => self.admin_action_available(action_id),
            Capability::Available => self.local_action_available(action_id),
            Capability::MockOnly => self.source_mode == SourceMode::Mock,
            Capability::Disabled(_) => false,
        }
    }

    pub(super) fn action_available_for_id(&self, action_id: ActionId) -> bool {
        action::find_action(action_id)
            .is_some_and(|spec| self.action_available(action_id, spec.capability))
    }

    pub(super) fn admin_action_available(&self, action_id: ActionId) -> bool {
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
            action_id if action_id.is_admin_mutation() => self.admin_mutation_available(action_id),
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

    pub(super) fn admin_mutation_available(&self, action_id: ActionId) -> bool {
        if self.admin.profile.is_none()
            || self.admin.profile_read_only
            || self.resolved_config.read_only
        {
            return false;
        }
        let scope = match action_id {
            ActionId::AdminRoutesReplaceApprovals => "devices:routes",
            action_id if action_id.is_admin_dns_mutation() => "dns",
            action_id if action_id.is_admin_user_mutation() => "users",
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
            action_id if action_id.is_admin_device_mutation() => {
                self.admin.devices.state == AdminResourceState::Ready
                    && self.selected_admin_device().is_some()
            }
            action_id if action_id.is_admin_user_mutation() => {
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

    pub(super) fn policy_credential_admin_available(&self, action_id: ActionId) -> bool {
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
            ActionId::AdminCredentialAuthKeyCreate => {
                self.pending_auth_key_result.is_none()
                    && self.pending_operational_mutation.is_none()
                    && self.secret_result.is_none()
            }
            ActionId::AdminCredentialRevoke => self.selected_credential().is_some(),
            _ => false,
        }
    }

    pub(super) fn operational_read_available(&self, action_id: ActionId) -> bool {
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

    pub(super) fn operational_mutation_available(&self, action_id: ActionId) -> bool {
        if self.admin.profile.is_none()
            || self.admin.profile_read_only
            || self.resolved_config.read_only
            || self.pending_operational_mutation.is_some()
        {
            return false;
        }
        if matches!(
            action_id,
            ActionId::AdminWebhookCreate | ActionId::AdminWebhookRotateSecret
        ) && (self.pending_auth_key_result.is_some() || self.secret_result.is_some())
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

    pub(super) fn admin_scope_allowed(&self, scope: &str) -> bool {
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
        if self.source_mode != SourceMode::Local && action_id.is_mutating() {
            return Some("local operator is disabled".to_owned());
        }
        if self.resolved_config.read_only && action_id.is_mutating() {
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
        if action_id.is_admin_mutation()
            && (self.resolved_config.read_only || self.admin.profile_read_only)
        {
            return Some("read-only mode blocks admin mutations".to_owned());
        }
        if action_id.is_admin_mutation() && self.admin.profile.is_none() {
            return Some("an authenticated admin profile is required".to_owned());
        }
        if action_id.is_service_write() && self.resolved_config.read_only {
            return Some("read-only mode blocks local service mutations".to_owned());
        }
        if action_id.needs_local_verification() && !self.local_daemon_state.is_live() {
            return Some(
                "local daemon observation is not live; mutation verification is unavailable"
                    .to_owned(),
            );
        }
        if action_id.is_taildrive()
            && action_id != ActionId::ServicesDriveEnableAlpha
            && !self.alpha_local_features
        {
            return Some("Taildrive is alpha and disabled until enabled for this run".to_owned());
        }
        if action_id.is_local_service() && self.local_executable.is_none() {
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
            action_id if action_id.is_admin_mutation() => {
                "the selected admin resource or mutation scope is unavailable"
            }
            _ => "capability unavailable",
        };
        Some(reason.to_owned())
    }

    pub(super) fn local_action_available(&self, action_id: ActionId) -> bool {
        if matches!(
            action_id,
            ActionId::SavedViewCreate
                | ActionId::SavedViewReplace
                | ActionId::SavedViewRename
                | ActionId::SavedViewDelete
                | ActionId::SavedViewApply
        ) && !self.resolved_config.experimental_features.saved_views
        {
            return false;
        }
        if action_id == ActionId::ViewServices {
            return self.route_unavailable_reason(Route::Services).is_none();
        }
        if action_id == ActionId::ViewDiagnostics {
            return self.route_unavailable_reason(Route::Diagnostics).is_none();
        }
        if action_id.is_local_service() && self.source_mode != SourceMode::Local {
            return false;
        }
        if action_id.is_local_operator() && self.source_mode != SourceMode::Local {
            return false;
        }
        if action_id.is_mutating() && self.resolved_config.read_only {
            return false;
        }
        if action_id.is_service_write() && self.resolved_config.read_only {
            return false;
        }
        if action_id.needs_local_verification() && !self.local_daemon_state.is_live() {
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

    pub(super) fn local_observer_action_available(&self, action_id: ActionId) -> bool {
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

    pub(super) fn local_preferences_ready(&self) -> bool {
        self.local_preferences.want_running.observed_at != 0
            && self.local_preferences.accept_dns.value.is_some()
    }

    pub fn preferences_ready(&self) -> bool {
        self.local_preferences_ready()
    }

    pub(super) fn open_operator_form(&mut self, action_id: ActionId) -> Vec<Effect> {
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
    pub(super) fn open_preferences_form(&mut self) -> Vec<Effect> {
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
    pub(super) fn open_exit_node_form(&mut self) -> Vec<Effect> {
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

    pub(super) fn open_advertisement_form(&mut self) -> Vec<Effect> {
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

    pub(super) fn accept_preferences_form(&mut self, state: &FormState) -> Vec<Effect> {
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

    pub(super) fn accept_exit_node_form(&mut self, state: &FormState) -> Vec<Effect> {
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

    pub(super) fn accept_advertisement_form(&mut self, state: &FormState) -> Vec<Effect> {
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
}
