use super::*;

impl App {
    pub(super) fn reduce_local_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        match action_id {
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
            ActionId::LocalPreferencesEdit
            | ActionId::LocalExitNodeSelect
            | ActionId::LocalRoutesEditAdvertisements => self.open_operator_form(action_id),
            ActionId::LocalAccountSwitch => self.open_selected_account_confirmation(false),
            ActionId::LocalAccountLogin => self.open_login_confirmation(),
            ActionId::LocalAccountLogout => self.open_logout_confirmation(),
            ActionId::LocalAccountRemove => self.open_selected_account_confirmation(true),
            ActionId::LocalSshOpen | ActionId::LocalNcOpen => self.open_handoff_form(action_id),
            ActionId::LocalSyspolicyReload => {
                self.open_mutation_confirmation(LocalMutation::SyspolicyReload)
            }
            _ => Vec::new(),
        }
    }
}

impl App {
    pub(super) fn move_local_account_selection(&mut self, offset: isize) {
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

    pub(super) fn reconcile_local_account_selection(&mut self) {
        self.views.local.selected = self
            .views
            .local
            .selected
            .min(self.local_accounts.len().saturating_sub(1));
        self.views.local.scroll = self.views.local.selected;
    }
}

impl App {
    pub(super) fn open_local_diagnostics(&mut self) -> Vec<Effect> {
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

    pub(super) fn start_probe_connection(&mut self) -> Vec<Effect> {
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

    pub(super) fn start_local_diagnostic(&mut self, request: DiagnosticRequest) -> Vec<Effect> {
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

    pub(super) fn request_capability_available(&self, request: &DiagnosticRequest) -> bool {
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

    pub(super) fn open_dns_query_form(&mut self) -> Vec<Effect> {
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

    pub(super) fn open_whois_form(&mut self) -> Vec<Effect> {
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

    pub(super) fn accept_dns_query_form(&mut self, state: &FormState) -> Vec<Effect> {
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

    pub(super) fn accept_whois_form(&mut self, state: &FormState) -> Vec<Effect> {
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

    pub(super) fn diagnostic_summary(&self) -> String {
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
}

impl App {
    pub(super) fn update_local(&mut self, event: LocalEvent) -> Vec<Effect> {
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
                effects.extend(self.load_visible_diagnostics());
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
                let needs_login = matches!(&snapshot.backend_state, LocalState::NeedsLogin { .. });
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
                if needs_login
                    && self.current_route() == Route::DEFAULT
                    && self.admin.profile.is_none()
                {
                    self.open_local_account_recovery();
                }
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
}

impl App {
    pub(super) fn apply_fresh_snapshot(&mut self, snapshot: LocalSnapshot) {
        let generation = self.local_resource.generation.saturating_add(1);
        self.local_resource.generation = generation;
        self.local_state = snapshot.backend_state.clone();
        let _ = self.local_resource.succeed(generation, snapshot);
        self.refresh_device_view();
    }

    pub(super) fn invalidate_local_state(&mut self) {
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

    pub(super) fn start_account_rediscovery(&mut self) -> Vec<Effect> {
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
}
