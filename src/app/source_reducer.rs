use super::*;

impl App {
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

    pub(super) fn start_refresh(&mut self, all: bool) -> Vec<Effect> {
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

    pub(super) fn move_profile_selection(&mut self, offset: isize) {
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
                filter::contains_matches(row.name, &filter)
                    || filter::contains_matches(&row.value, &filter)
                    || filter::contains_matches(row.source.label(), &filter)
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

    pub(super) fn move_config_selection(&mut self, offset: isize) {
        let length = self.config_rows().len();
        self.views.config.selected = move_bounded_index(self.views.config.selected, length, offset);
    }

    /// Read what every configured profile's store holds. Local reads only, so
    /// this is cheap enough to repeat whenever the answer could have changed.
    pub(super) fn inspect_profile_credentials(&self) -> Option<Effect> {
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
    pub(super) fn activate_selected_profile(&mut self) -> Vec<Effect> {
        let Some(row) = self.selected_profile_row() else {
            self.runtime_error = Some("no profile row is selected".to_owned());
            return Vec::new();
        };
        let Some(name) = row.name().map(str::to_owned) else {
            if self.pending_navigation_route.is_some() {
                self.runtime_error = Some(
                    "Choose an administration profile to continue to the requested view".to_owned(),
                );
                return Vec::new();
            }
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
    pub(super) fn finish_profile_probe(
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

    pub(super) fn clear_admin_profile(&mut self) -> Vec<Effect> {
        self.switch_profile(None)
    }

    pub fn switch_profile(&mut self, profile: Option<String>) -> Vec<Effect> {
        if self.resolved_config.profile == profile {
            return Vec::new();
        }
        if !self.admin_mutations_in_flight.is_empty()
            || !self.admin_batches_in_flight.is_empty()
            || !self.admin_batch_preflights.is_empty()
            || self.pending_auth_key_result.is_some()
            || self.pending_operational_mutation.is_some()
            || self.pending_credential_revoke.is_some()
            || self.secret_result.is_some()
            || self
                .policy_workflow
                .as_ref()
                .is_some_and(|workflow| workflow.state() == PolicyState::Applying)
        {
            self.runtime_error = Some(
                "finish the active control-plane write and preserve or close any view-once secret before switching profiles"
                    .to_owned(),
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
        if profile.is_some()
            && let Some(route) = self.pending_navigation_route.take()
        {
            self.navigate(route);
        }
        effects
    }

    pub(super) fn start_admin_refresh(&mut self) -> Vec<Effect> {
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

    pub(super) fn start_admin_current_view_refresh(&mut self) -> Vec<Effect> {
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

    pub(super) fn start_admin_selected_refresh(&mut self) -> Vec<Effect> {
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

    pub(super) fn start_admin_resource_refresh(
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

    pub(super) fn start_admin_device_enrichment(
        &mut self,
        selected_id: Option<String>,
    ) -> Option<Effect> {
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

    pub(super) fn update_admin(&mut self, event: AdminEvent) -> Vec<Effect> {
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
                            "could not load the latest details needed for this change: {error}"
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
                    self.runtime_error = Some(format!(
                        "the item changed while you were reviewing it:\n{detail}"
                    ));
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
                        admin_generation: self.admin_generation,
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
                let _ = self
                    .tasks
                    .set_verification(task_id, outcome.verification.clone());
                let task_succeeded = self
                    .tasks
                    .get(task_id)
                    .is_some_and(|task| task.state == TaskState::Succeeded);
                if task_succeeded {
                    self.add_notification(
                        task_id,
                        crate::task::TaskResultKind::Success,
                        request.change.completed_message(),
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
                operation_id,
                admin_generation,
                profile,
                tailnet,
                action_id,
                mutation,
                result,
                secret,
            } => {
                let mutation = *mutation;
                let expected = self.pending_operational_mutation == Some(operation_id);
                if expected {
                    self.pending_operational_mutation = None;
                }
                let context_current = admin_generation == self.admin_generation
                    && self.admin.profile.as_deref() == Some(profile.as_str())
                    && self.admin.tailnet.as_deref() == Some(tailnet.as_str());
                if expected && context_current {
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
                        Ok(OperationalResult::SucceededUnverified { detail }) => {
                            self.runtime_error = Some(format!(
                                "the change may have succeeded, but Tale could not refresh it: {detail}"
                            ));
                        }
                        Ok(OperationalResult::OutcomeUnknown { detail }) => {
                            self.runtime_error = Some(format!(
                                "Tale could not tell whether the change completed: {detail}"
                            ));
                        }
                        Err(error) => {
                            self.runtime_error = Some(error.to_string());
                        }
                    }
                } else {
                    self.runtime_error = Some(format!(
                        "a change completed for {profile} / {tailnet} after that administration view was closed; refresh to see it"
                    ));
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
                    | ActionId::AdminNetworkLogsSettings
                        if expected && context_current =>
                    {
                        self.start_admin_current_view_refresh()
                    }
                    _ => Vec::new(),
                };
                return refresh;
            }
            AdminEvent::AccessExplorerFinished { result } => match result {
                Ok(result) => {
                    self.access_explorer_result = Some(result);
                    self.runtime_error =
                        Some("Access preview loaded for the edited policy".to_owned());
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
                            self.runtime_error = Some("Flow counters updated".to_owned());
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
                    "No matching activity entry was found".to_owned()
                } else if correlation.is_ambiguous() {
                    "Several possible activity entries were found".to_owned()
                } else {
                    "Matching activity entry found".to_owned()
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

    pub(super) fn finish_admin_batch_preflight(
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
                    "could not load current route details for {}: {error}",
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
                "route details changed for {} while you were reviewing them:\n{detail}",
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
            "devices to update: {} route advertisers",
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
                admin_generation: self.admin_generation,
                mutation: None,
                admin_mutation: None,
                admin_batch: Some(AdminBatchConfirmation { batch, requests }),
                service_request: None,
                operational_mutation: None,
                handoff: None,
                prompt: "Apply these route approvals? Each device is checked separately, and one failure will not undo the others."
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

    pub(super) fn finish_admin_batch_child(
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
            "Route updates cancelled; review the device results"
        } else if has_failure && in_flight.batch.verified_count() > 0 {
            "Some route approvals were updated; review the device results"
        } else if has_failure {
            "Route approvals could not be updated; review the device results"
        } else {
            "Route approvals updated for every device"
        };
        let detail = format!(
            "{} of {} devices updated",
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

    pub(super) fn refresh_admin_capabilities(&mut self) {
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

    pub(super) fn local_devices(&self) -> Option<Vec<LocalDevice>> {
        self.local_resource.snapshot.as_ref().map(|snapshot| {
            let mut devices = Vec::with_capacity(snapshot.peers.len().saturating_add(1));
            devices.push(snapshot.self_node.clone());
            devices.extend(snapshot.peers.clone());
            devices
        })
    }

    pub(super) fn recompute_composed_devices(&mut self) {
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

    pub(super) fn display_device_from_composed(composed: &ComposedDevice) -> Device {
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
}

impl App {
    pub(super) fn recompute_health(&mut self) -> Vec<Effect> {
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
}

impl App {
    pub(super) fn update_source(&mut self, event: SourceEvent) -> Vec<Effect> {
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
}
