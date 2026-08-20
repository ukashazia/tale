use super::*;

impl App {
    pub(super) fn reduce_admin_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        match action_id {
            ActionId::AdminDeviceRename
            | ActionId::AdminDeviceTagsReplace
            | ActionId::AdminDeviceKeyExpiryConfigure
            | ActionId::AdminRoutesReplaceApprovals
            | ActionId::AdminDnsPreferencesEdit
            | ActionId::AdminDnsNameserversReplace
            | ActionId::AdminDnsSearchPathsReplace
            | ActionId::AdminDnsSplitCreate
            | ActionId::AdminDnsSplitEdit
            | ActionId::AdminDnsSplitRemove
            | ActionId::AdminUserRoleChange => self.open_admin_form(action_id),
            ActionId::AdminDeviceApprove => {
                self.open_admin_change(action_id, AdminChange::DeviceApproval { authorized: true })
            }
            ActionId::AdminDeviceRevokeApproval => {
                self.open_admin_change(action_id, AdminChange::DeviceApproval { authorized: false })
            }
            ActionId::AdminDeviceKeyExpireNow => {
                self.open_admin_change(action_id, AdminChange::DeviceExpireNow)
            }
            ActionId::AdminDeviceDelete => {
                self.open_admin_change(action_id, AdminChange::DeviceDelete)
            }
            ActionId::AdminUserApprove => {
                self.open_admin_change(action_id, AdminChange::UserApproval)
            }
            ActionId::AdminUserSuspend => {
                self.open_admin_change(action_id, AdminChange::UserSuspend)
            }
            ActionId::AdminUserRestore => {
                self.open_admin_change(action_id, AdminChange::UserRestore)
            }
            ActionId::AdminUserDelete => self.open_admin_change(action_id, AdminChange::UserDelete),
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
            ActionId::ActivityFlowsAggregate => self.start_flow_aggregation(),
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
            ActionId::AccessExplorerAsk => self.open_access_explorer_form(),
            ActionId::AccessExplorerOpenRule => self.open_local_operational_action(action_id),
            _ => Vec::new(),
        }
    }

    pub(super) fn start_flow_aggregation(&mut self) -> Vec<Effect> {
        let Some(snapshot) = self.flow_snapshot.as_ref() else {
            self.runtime_error =
                Some("flow aggregation requires a completed bounded flow window".to_owned());
            return Vec::new();
        };
        if !snapshot.complete {
            self.runtime_error = Some(
                "flow aggregation is disabled for a partial bounded response; choose a narrower window"
                    .to_owned(),
            );
            return Vec::new();
        }
        let messages = snapshot.messages.clone();
        self.cancel_flow_aggregation();
        self.flow_aggregation_generation = self.flow_aggregation_generation.saturating_add(1);
        let generation = self.flow_aggregation_generation;
        let cancellation = Arc::new(AtomicBool::new(false));
        self.flow_aggregation_cancellation = Some(Arc::clone(&cancellation));
        self.runtime_error = Some("aggregating the bounded flow window".to_owned());
        vec![Effect::StartFlowAggregation {
            generation,
            messages,
            filter: self.flow_filter.clone(),
            dimensions: vec![
                AggregateDimension::ReportingNode,
                AggregateDimension::TrafficClass,
                AggregateDimension::Protocol,
            ],
            cancellation,
        }]
    }
}

impl App {
    /// Starts a mutation that has no values to ask for. The verified remote
    /// preflight supplies the preview and confirmation context.
    pub(super) fn open_admin_change(
        &mut self,
        action_id: ActionId,
        change: AdminChange,
    ) -> Vec<Effect> {
        match self.start_admin_change(action_id, change) {
            Ok(effects) => effects,
            Err(error) => {
                self.runtime_error = Some(error);
                Vec::new()
            }
        }
    }

    pub(super) fn open_admin_form(&mut self, action_id: ActionId) -> Vec<Effect> {
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
    pub(super) fn admin_form_shape(&self, action_id: ActionId) -> Option<FormShape> {
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
    pub(super) fn reopen_admin_form(
        &mut self,
        action_id: ActionId,
        change: &AdminChange,
        error: String,
    ) {
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

    pub(super) fn admin_device_subject(&self) -> Vec<(&'static str, String)> {
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

    pub(super) fn selected_split_dns_entry(&self) -> (String, Vec<String>) {
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

    pub(super) fn selected_webhook(&self) -> Option<&WebhookEndpoint> {
        self.webhooks.first()
    }

    pub(super) fn admin_base_snapshot(
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
                    .ok_or_else(|| "select a loaded device before editing it".to_owned())?;
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
                    .ok_or_else(|| "select a loaded user before editing it".to_owned())?;
                Ok((user.id.clone(), crate::admin::mutation::user_fields(user)))
            }
            AdminChange::DnsNameservers { .. } => Ok((
                "tailnet".to_owned(),
                crate::admin::mutation::nameserver_fields(
                    self.admin
                        .nameservers
                        .snapshot
                        .as_ref()
                        .ok_or_else(|| "DNS nameservers have not loaded yet".to_owned())?,
                ),
            )),
            AdminChange::DnsPreferences { .. } => Ok((
                "tailnet".to_owned(),
                crate::admin::mutation::dns_preferences_fields(
                    self.admin
                        .dns_preferences
                        .snapshot
                        .as_ref()
                        .ok_or_else(|| "DNS preferences have not loaded yet".to_owned())?,
                ),
            )),
            AdminChange::DnsSearchPaths { .. } => Ok((
                "tailnet".to_owned(),
                crate::admin::mutation::search_path_fields(
                    self.admin
                        .search_paths
                        .snapshot
                        .as_ref()
                        .ok_or_else(|| "DNS search paths have not loaded yet".to_owned())?,
                ),
            )),
            AdminChange::DnsSplitMapping { .. } => Ok((
                "tailnet".to_owned(),
                crate::admin::mutation::split_dns_fields(
                    self.admin
                        .split_dns
                        .snapshot
                        .as_ref()
                        .ok_or_else(|| "split DNS rules have not loaded yet".to_owned())?,
                ),
            )),
        }
    }

    pub(super) fn start_admin_preflight(&mut self, request: AdminMutationRequest) -> Vec<Effect> {
        if !self.admin_resource_locks.try_hold(
            request.mutation_id,
            request
                .change
                .lock_keys(&request.profile, &request.target_id),
        ) {
            self.runtime_error =
                Some("another change or refresh is already running for this item".to_owned());
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

    pub(super) fn release_admin_preflight_lock(&mut self, mutation_id: u64) {
        if self.admin_preflight_locks.remove(&mutation_id) {
            self.admin_resource_locks.release(mutation_id);
        }
    }

    pub(super) fn release_admin_read_lock(&mut self, device_id: &str) {
        if let Some(owner) = self.admin_read_locks.remove(device_id) {
            self.admin_resource_locks.release(owner);
        }
    }

    pub(super) fn release_all_admin_read_locks(&mut self) {
        let owners = self.admin_read_locks.values().copied().collect::<Vec<_>>();
        self.admin_read_locks.clear();
        for owner in owners {
            self.admin_resource_locks.release(owner);
        }
    }

    pub(super) fn open_selected_account_confirmation(&mut self, remove: bool) -> Vec<Effect> {
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

    pub(super) fn open_login_confirmation(&mut self) -> Vec<Effect> {
        let Some(executable) = self.local_executable.as_ref() else {
            self.runtime_error = Some(self.missing_executable_reason());
            return Vec::new();
        };
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: ActionId::LocalAccountLogin,
                admin_generation: self.admin_generation,
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

    pub(super) fn open_logout_confirmation(&mut self) -> Vec<Effect> {
        let Some(executable) = self.local_executable.as_ref() else {
            self.runtime_error = Some(self.missing_executable_reason());
            return Vec::new();
        };
        self.overlays.push(Overlay::Confirmation(Box::new(ConfirmationState {
            action_id: ActionId::LocalAccountLogout,
            admin_generation: self.admin_generation,
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
    pub(super) fn open_handoff_form(&mut self, action_id: ActionId) -> Vec<Effect> {
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

    pub(super) fn accept_handoff_form(&mut self, state: &FormState) -> Vec<Effect> {
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
                        admin_generation: self.admin_generation,
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

    pub(super) fn open_mutation_confirmation(&mut self, mutation: LocalMutation) -> Vec<Effect> {
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
                admin_generation: self.admin_generation,
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

    pub(super) fn validate_mutation_request(&self, mutation: &LocalMutation) -> Result<(), String> {
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

    pub(super) fn accept_admin_form(&mut self, state: &FormState) -> Vec<Effect> {
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
        if state.action_id == ActionId::AdminRoutesReplaceApprovals {
            return self.accept_admin_batch_form(state, change);
        }
        let effects = match self.start_admin_change(state.action_id, change) {
            Ok(effects) => effects,
            Err(error) => return self.set_form_error(error),
        };
        self.overlays.pop();
        effects
    }

    pub(super) fn start_admin_change(
        &mut self,
        action_id: ActionId,
        change: AdminChange,
    ) -> Result<Vec<Effect>, String> {
        if !self.admin_mutation_available(action_id) {
            return Err(self
                .action_unavailable_reason(action_id)
                .unwrap_or_else(|| "this change is unavailable".to_owned()));
        }
        let Some(profile) = self.admin.profile.clone() else {
            return Err("an authenticated admin profile is required".to_owned());
        };
        let (target_id, base_snapshot) = self.admin_base_snapshot(&change)?;
        let mutation_id = self.next_mutation_id;
        self.next_mutation_id = self.next_mutation_id.saturating_add(1);
        let risk = change.risk();
        let mut request = crate::domain::admin_mutation::AdminMutation::new(
            mutation_id,
            profile,
            target_id,
            base_snapshot,
            change,
            action_id,
            risk,
        );
        request
            .begin_preflight()
            .map_err(|error| error.to_string())?;
        let effects = self.start_admin_preflight(request);
        if effects.is_empty() {
            return Err("this item is already being changed or refreshed; review it again when that finishes".to_owned());
        }
        Ok(effects)
    }

    pub(super) fn accept_admin_batch_form(
        &mut self,
        state: &FormState,
        change: AdminChange,
    ) -> Vec<Effect> {
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

    pub(super) fn begin_retry_batch_preflight(&mut self, targets: Vec<BatchTarget>) -> Vec<Effect> {
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
                        "could not refresh route details for {}: {error}",
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
                self.runtime_error = Some("could not refresh the selected devices".to_owned());
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

    pub(super) fn mutation_preview_lines(&self, mutation: &LocalMutation) -> Vec<String> {
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

    pub(super) fn accept_admin_batch_confirmation(
        &mut self,
        confirmation: AdminBatchConfirmation,
    ) -> Vec<Effect> {
        let Some(active_profile) = self.admin.profile.as_ref() else {
            self.set_confirmation_error("an authenticated admin profile is required");
            return Vec::new();
        };
        let Some(profile_config) = self.resolved_config.profiles.get(active_profile) else {
            self.set_confirmation_error("admin profile configuration is unavailable");
            return Vec::new();
        };
        if confirmation
            .requests
            .iter()
            .any(|request| request.profile != *active_profile)
        {
            self.set_confirmation_error(
                "the active administration profile changed; review the route changes again",
            );
            return Vec::new();
        }
        let tailnet = profile_config.tailnet.clone();
        if self.admin.tailnet.as_deref() != Some(tailnet.as_str()) {
            self.set_confirmation_error("admin profile and tailnet context are inconsistent");
            return Vec::new();
        }
        if !self.admin_mutation_available(confirmation.batch.action_id) {
            let reason = self
                .action_unavailable_reason(confirmation.batch.action_id)
                .unwrap_or_else(|| "these route changes are no longer available".to_owned());
            self.set_confirmation_error(&reason);
            return Vec::new();
        }
        let target_ids = confirmation
            .requests
            .iter()
            .map(|request| request.target_id.clone())
            .collect::<Vec<_>>();
        if !confirmation.batch.target_list_is_unchanged(&target_ids) {
            self.set_confirmation_error(
                "the selected devices changed; review the route changes again",
            );
            return Vec::new();
        }
        for request in &confirmation.requests {
            let Some(preflight) = request.preflight.as_ref() else {
                self.set_confirmation_error(
                    "current details are required for every selected device",
                );
                return Vec::new();
            };
            if !preflight.is_fresh_at(self.now) {
                self.set_confirmation_error(
                    "the device details are out of date; review the route changes again",
                );
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
                    "a selected device is already being changed or refreshed",
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
            let _ = self
                .tasks
                .set_changes(task_id, crate::admin::mutation::task_changes(&request));
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

    pub(super) fn set_confirmation_error(&mut self, error: &str) {
        if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
            current.error = Some(error.to_owned());
        }
    }

    pub(super) fn extend_admin_refresh_for_owned_devices(
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

    pub(super) fn admin_mutation_target_is_current(&self, request: &AdminMutationRequest) -> bool {
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

    pub(super) fn open_selected_batch_result(&mut self) -> Vec<Effect> {
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

    pub(super) fn retry_selected_batch(&mut self) -> Vec<Effect> {
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

    pub(super) fn accept_confirmation(&mut self, state: ConfirmationState) -> Vec<Effect> {
        if state.admin_generation != self.admin_generation {
            self.set_confirmation_error(
                "the active administration profile changed after this preview; discard it and review the operation again",
            );
            return Vec::new();
        }
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
                    self.runtime_error = Some("Policy applied".to_owned());
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
                self.runtime_error = Some("the policy draft is unavailable".to_owned());
                return Vec::new();
            };
            let Some(base_hash) = workflow.base().map(|value| value.hash().to_owned()) else {
                self.runtime_error = Some("the policy base is unavailable".to_owned());
                return Vec::new();
            };
            let Some(candidate_hash) = workflow.candidate().map(|value| value.hash().to_owned())
            else {
                self.runtime_error = Some("the policy draft is unavailable".to_owned());
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
                admin_generation: self.admin_generation,
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
            if self.admin.profile.as_deref() != Some(request.profile.as_str()) {
                self.set_confirmation_error(
                    "the active administration profile changed; review the change again",
                );
                return Vec::new();
            }
            if !self.admin_mutation_available(request.action_id) {
                let reason = self
                    .action_unavailable_reason(request.action_id)
                    .unwrap_or_else(|| "this change is no longer available".to_owned());
                if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
                    current.error = Some(reason);
                }
                return Vec::new();
            }
            if !self.admin_mutation_target_is_current(&request) {
                if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
                    current.error = Some(
                        "the selected item changed; discard this review and start again".to_owned(),
                    );
                }
                return Vec::new();
            }
            let Some(preflight) = request.preflight.as_ref() else {
                if let Some(Overlay::Confirmation(current)) = self.overlays.last_mut() {
                    current.error =
                        Some("review the latest details before applying this change".to_owned());
                }
                return Vec::new();
            };
            if !preflight.is_fresh_at(self.now) {
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
                        Some("this item is already being changed or refreshed".to_owned());
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
            let tailnet = profile_config.tailnet.clone();
            if self.admin.tailnet.as_deref() != Some(tailnet.as_str()) {
                self.admin_resource_locks.release(request.mutation_id);
                self.runtime_error =
                    Some("admin profile and tailnet context are inconsistent".to_owned());
                return Vec::new();
            }
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
            let _ = self
                .tasks
                .set_changes(task_id, crate::admin::mutation::task_changes(&request));
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
            if self.resolved_config.read_only && request.action_id().is_service_write() {
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

    pub(super) fn update_policy(&mut self, event: PolicyEvent) -> Vec<Effect> {
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
                        "The tailnet policy changed; the draft and latest policy were kept separately"
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
                        "The tailnet policy is unchanged; the edited draft was kept".to_owned(),
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
                                "The editor exited with {}; the draft was kept",
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
                                    "The policy changed before the validation result arrived; validate it again"
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
                                    "The policy changed before the access preview arrived; preview it again"
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
                                    "The policy changed before the comparison finished; compare it again".to_owned(),
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
                        "The tailnet policy changed; the draft and latest policy were kept for review"
                            .to_owned(),
                    );
                    return Vec::new();
                }
                let mut refresh_audit = false;
                if let Some(workflow) = self.policy_workflow.as_mut()
                    && workflow.workflow_id() == workflow_id
                {
                    match result {
                        PolicyApplyResult::Succeeded { saved_hash: _ } => {
                            workflow.mark_verifying();
                            workflow.mark_succeeded();
                            self.runtime_error = Some("Policy applied".to_owned());
                            refresh_audit = true;
                        }
                        PolicyApplyResult::SucceededUnverified { saved_hash } => {
                            workflow.mark_succeeded_unverified();
                            self.runtime_error = Some(format!(
                                "Policy saved, but Tale could not refresh it ({saved_hash})"
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

    pub(super) fn update_credential(&mut self, event: CredentialEvent) -> Vec<Effect> {
        match event {
            CredentialEvent::AuthKeyCreated {
                result_id,
                admin_generation,
                profile,
                tailnet,
                metadata,
                secret,
                observed_at,
            } => {
                if self.pending_auth_key_result != Some(result_id) {
                    return Vec::new();
                }
                self.pending_auth_key_result = None;
                if admin_generation != self.admin_generation
                    || self.admin.profile.as_deref() != Some(profile.as_str())
                    || self.admin.tailnet.as_deref() != Some(tailnet.as_str())
                {
                    self.runtime_error = Some(format!(
                        "auth key creation completed for {profile} / {tailnet} after the active admin context changed; preserve the displayed secret and review that tailnet"
                    ));
                }
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
            CredentialEvent::AuthKeyCreateFailed {
                result_id,
                admin_generation,
                profile,
                tailnet,
                detail,
            } => {
                if self.pending_auth_key_result == Some(result_id) {
                    self.pending_auth_key_result = None;
                    let context = if admin_generation == self.admin_generation
                        && self.admin.profile.as_deref() == Some(profile.as_str())
                        && self.admin.tailnet.as_deref() == Some(tailnet.as_str())
                    {
                        String::new()
                    } else {
                        format!(" for {profile} / {tailnet}")
                    };
                    self.runtime_error =
                        Some(format!("auth key creation{context} failed: {detail}"));
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
                        self.runtime_error = Some(
                            "Credential revoked. The active profile was disconnected because Tale cannot safely match the saved credential to the revoked one."
                                .to_owned(),
                        );
                        return self.clear_admin_profile();
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
}
