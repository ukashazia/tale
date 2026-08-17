use super::*;

impl App {
    pub(super) fn reduce_service_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        match action_id {
            ActionId::SectionNext => {
                self.change_route_section(1);
                Vec::new()
            }
            ActionId::SectionPrevious => {
                self.change_route_section(-1);
                Vec::new()
            }
            ActionId::ServicesServeRefresh
            | ActionId::ServicesDriveRefresh
            | ActionId::ServicesMetricsRefresh => self.start_services_action(action_id),
            ActionId::ServicesServeCreate
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
            | ActionId::ServicesBugReportCreate => self.open_service_action(action_id),
            ActionId::ServicesDriveEnableAlpha => {
                self.alpha_local_features = true;
                self.start_services_refresh()
            }
            _ => Vec::new(),
        }
    }
}

impl App {
    /// Tab moves to the next tab and wraps, which is what a tab strip implies.
    pub(super) fn change_route_section(&mut self, offset: isize) {
        match self.current_route() {
            Route::Local => self.change_local_section(offset),
            Route::Services => self.change_service_section(offset),
            _ => {}
        }
    }

    pub(super) fn change_local_section(&mut self, offset: isize) {
        let sections = LocalSection::ALL;
        let length = sections.len();
        let current = sections
            .iter()
            .position(|section| *section == self.views.local.section)
            .unwrap_or(0);
        let step = offset.rem_euclid(length as isize).unsigned_abs();
        let next = current.saturating_add(step) % length;
        self.views.local.section = sections.get(next).copied().unwrap_or(LocalSection::Client);
        self.views.local.selected = 0;
        self.views.local.scroll = 0;
        self.detail_search.clear();
        self.detail_search_match = None;
        self.focus = Focus::Collection;
    }

    pub(super) fn change_service_section(&mut self, offset: isize) {
        let sections = ServiceSection::ALL;
        let length = sections.len();
        let current = sections
            .iter()
            .position(|section| *section == self.views.services.section)
            .unwrap_or(0);
        let step = offset.rem_euclid(length as isize).unsigned_abs();
        let next = current.saturating_add(step) % length;
        self.views.services.section = sections.get(next).copied().unwrap_or(ServiceSection::Serve);
        self.views.services.selected = 0;
        self.views.services.scroll = 0;
        self.views.services.filter_draft.clear();
        self.views.services.applied_filter = FilterExpression::empty();
        self.focus = Focus::Collection;
    }

    pub(super) fn move_diagnostics_scroll(&mut self, offset: isize) {
        let current = self.views.diagnostics.scroll;
        let next = if offset.is_negative() {
            current.saturating_sub(offset.unsigned_abs())
        } else {
            current.saturating_add(offset.unsigned_abs())
        };
        self.views.diagnostics.scroll = next.min(self.metrics_max_scroll());
    }

    pub(super) fn move_service_selection(&mut self, offset: isize) {
        let count = self.service_row_count();
        self.views.services.selected =
            move_bounded_index(self.views.services.selected, count, offset);
        self.views.services.scroll = self.views.services.selected;
    }

    /// Serve and Funnel as one table: filtered, then ordered by the chosen
    /// column. Public rows are mappings whose exposure is public, nothing more.
    pub fn visible_service_mappings(&self) -> Vec<&ServiceMapping> {
        let filter = &self.views.services.applied_filter;
        let mut mappings = self
            .services_snapshot
            .mappings()
            .filter(|mapping| filter.matches_mapping(mapping))
            .collect::<Vec<_>>();
        let sort = self.views.services.sort;
        mappings.sort_by(|left, right| {
            let ordering = sort
                .field
                .ordering_key(left)
                .cmp(&sort.field.ordering_key(right));
            match sort.direction {
                SortDirection::Ascending => ordering,
                SortDirection::Descending => ordering.reverse(),
            }
        });
        mappings
    }

    pub fn service_mapping_total(&self) -> usize {
        self.services_snapshot.mappings().count()
    }

    pub fn visible_taildrive_shares(&self) -> Vec<&TaildriveShare> {
        let query = self.views.services.filter_draft.trim();
        self.services_snapshot
            .taildrive
            .value
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|share| {
                query.is_empty()
                    || filter::contains_matches(&share.name, query)
                    || filter::contains_matches(&share.path.display().to_string(), query)
                    || share
                        .as_user
                        .as_deref()
                        .is_some_and(|user| filter::contains_matches(user, query))
            })
            .collect()
    }

    pub fn visible_certificate_domains(&self) -> Vec<&str> {
        let query = self.views.services.filter_draft.trim();
        self.services_snapshot
            .certificate_domains
            .value
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(String::as_str)
            .filter(|domain| query.is_empty() || filter::contains_matches(domain, query))
            .collect()
    }

    pub(super) fn service_row_count(&self) -> usize {
        match self.views.services.section {
            ServiceSection::Serve => self.visible_service_mappings().len(),
            // With the alpha feature off nothing is listed, so nothing counts.
            ServiceSection::Taildrive if !self.alpha_local_features => 0,
            ServiceSection::Taildrive => self.visible_taildrive_shares().len(),
            ServiceSection::Certificates => self.visible_certificate_domains().len(),
        }
    }

    pub fn selected_service_mapping(&self) -> Option<ServiceMapping> {
        if self.views.services.section != ServiceSection::Serve {
            return None;
        }
        self.visible_service_mappings()
            .get(self.views.services.selected)
            .map(|mapping| (*mapping).clone())
    }

    /// The discovered Taildrop target for the selected device row. An address
    /// is exact, so it decides on its own; a display name is not, so it is used
    /// only when it picks out exactly one target and no address matched. A
    /// destination is never inferred from a name several devices share.
    pub fn selected_taildrop_target(&self) -> Option<TaildropTarget> {
        let device = self.selected_device()?;
        let targets = self.services_snapshot.taildrop_targets.value.as_ref()?;
        only(targets.iter().filter(|target| {
            device
                .addresses
                .iter()
                .any(|address| address.eq_ignore_ascii_case(&target.command_target))
        }))
        .or_else(|| {
            only(
                targets
                    .iter()
                    .filter(|target| taildrop_target_names_device(target, device)),
            )
        })
        .cloned()
    }

    pub fn selected_taildrive_share(&self) -> Option<TaildriveShare> {
        self.visible_taildrive_shares()
            .get(self.views.services.selected)
            .copied()
            .cloned()
    }

    pub fn selected_certificate_domain(&self) -> Option<&str> {
        self.visible_certificate_domains()
            .get(self.views.services.selected)
            .copied()
    }

    pub fn service_inspector_available(&self) -> bool {
        match self.views.services.section {
            ServiceSection::Serve => self.selected_service_mapping().is_some(),
            ServiceSection::Taildrive => {
                self.alpha_local_features && self.selected_taildrive_share().is_some()
            }
            ServiceSection::Certificates => self.selected_certificate_domain().is_some(),
        }
    }

    pub(super) fn metrics_max_scroll(&self) -> usize {
        let line_count = self
            .services_snapshot
            .metrics
            .value
            .as_ref()
            .map_or(0, |metrics| metrics.text.lines().count());
        let viewport = usize::from(self.terminal_height.saturating_sub(8)).max(1);
        line_count.saturating_sub(viewport)
    }

    pub fn contextual_actions(&self) -> Vec<ActionId> {
        let mut actions = if self.current_route() == Route::Services {
            self.service_actions_for_section()
        } else if self.admin.profile.is_some() && self.current_route() == Route::Devices {
            vec![
                ActionId::AdminDeviceRename,
                ActionId::AdminDeviceTagsReplace,
                ActionId::AdminDeviceApprove,
                ActionId::AdminDeviceRevokeApproval,
                ActionId::AdminDeviceKeyExpiryConfigure,
                ActionId::AdminDeviceKeyExpireNow,
                ActionId::AdminDeviceDelete,
            ]
        } else if self.admin.profile.is_some() && self.current_route() == Route::Users {
            vec![
                ActionId::AdminUserApprove,
                ActionId::AdminUserRoleChange,
                ActionId::AdminUserSuspend,
                ActionId::AdminUserRestore,
                ActionId::AdminUserDelete,
            ]
        } else if self.admin.profile.is_some() && self.current_route() == Route::Routes {
            vec![ActionId::AdminRoutesReplaceApprovals]
        } else if self.admin.profile.is_some() && self.current_route() == Route::Dns {
            vec![
                ActionId::AdminDnsPreferencesEdit,
                ActionId::AdminDnsNameserversReplace,
                ActionId::AdminDnsSearchPathsReplace,
                ActionId::AdminDnsSplitCreate,
                ActionId::AdminDnsSplitEdit,
                ActionId::AdminDnsSplitRemove,
            ]
        } else if self.source_mode == SourceMode::Mock {
            vec![
                ActionId::MockSuccess,
                ActionId::MockFailure,
                ActionId::MockCancellable,
                ActionId::MockNonCancellable,
            ]
        } else {
            Vec::new()
        };
        actions.extend(self.local_actions_for_route());
        actions.extend(self.operational_resource_actions());
        actions
    }

    /// The local client's actions, offered where their subject is on screen.
    /// These used to be one list handed to every route that had no list of its
    /// own, which is how `:credentials` came to offer `remove local account`
    /// and how `open tailscale ssh` — which acts on the selected device — was
    /// missing from `:devices` whenever an admin profile was configured.
    pub(super) fn local_actions_for_route(&self) -> Vec<ActionId> {
        if self.source_mode != SourceMode::Local {
            return Vec::new();
        }
        match self.current_route() {
            // This machine: connecting it, its preferences, and local policy.
            Route::Local if self.views.local.section == LocalSection::Client => vec![
                ActionId::LocalConnect,
                ActionId::LocalDisconnect,
                ActionId::LocalPreferencesEdit,
                ActionId::LocalExitNodeSelect,
                ActionId::LocalRoutesEditAdvertisements,
                ActionId::LocalSyspolicyReload,
            ],
            // Account actions live with the account rows they act on.
            Route::Local => {
                let mut actions = Vec::new();
                let has_selection = self.selected_local_account().is_some();
                if has_selection {
                    actions.push(ActionId::LocalAccountSwitch);
                }
                actions.push(ActionId::LocalAccountLogin);
                actions.push(ActionId::LocalAccountLogout);
                if has_selection {
                    actions.push(ActionId::LocalAccountRemove);
                }
                actions
            }
            // Every one of these acts on the selected row: it pings it, looks
            // it up, opens a session to it, or sends it a file. All of them go
            // through the local daemon, so they are withheld when the rows on
            // screen belong to a tailnet this machine is not on — offering to
            // SSH to an unreachable node is worse than not offering at all.
            Route::Devices if self.device_view_source().is_locally_reachable() => vec![
                ActionId::LocalProbeConnection,
                ActionId::LocalWhois,
                ActionId::LocalSshOpen,
                ActionId::LocalNcOpen,
                ActionId::DevicesTaildropSend,
                ActionId::DevicesTaildropReceive,
            ],
            // The summary this route is showing is the thing being copied.
            Route::Diagnostics => vec![ActionId::DiagnosticCopy],
            _ => Vec::new(),
        }
    }

    pub fn contextual_copy_fields(&self) -> Vec<CopyField> {
        if self.current_route() == Route::Diagnostics {
            return vec![CopyField::Metrics];
        }
        if self.current_route() == Route::Services {
            // Only the mapping table has a row worth copying; the other
            // sections are a name or a path already visible in full.
            return if self.selected_service_mapping().is_some() {
                vec![
                    CopyField::ServiceUrl,
                    CopyField::ServiceListener,
                    CopyField::ServiceBackend,
                ]
            } else {
                Vec::new()
            };
        }
        if self.current_route() == Route::Tasks {
            // The row is already readable; what anyone pastes into a bug report
            // is the command that ran and what it printed.
            let Some(task) = self.focused_task() else {
                return Vec::new();
            };
            let mut fields = vec![CopyField::TaskId, CopyField::TaskResult];
            if !task.redacted_argv.is_empty() {
                fields.push(CopyField::TaskCommand);
            }
            if !task.detail.is_empty() {
                fields.push(CopyField::TaskOutput);
            }
            return fields;
        }
        if self.current_route() == Route::Config {
            return self.selected_config_row().map_or_else(Vec::new, |_| {
                vec![
                    CopyField::ConfigSetting,
                    CopyField::ConfigValue,
                    CopyField::ConfigSource,
                ]
            });
        }
        if self.current_route() == Route::Profiles {
            // The row is mostly words already on screen; what is worth pasting
            // is what you would type somewhere else — into a config file, a
            // shell, or a message asking someone why a credential was refused.
            let Some(row) = self.selected_profile_row() else {
                return Vec::new();
            };
            let mut fields = vec![CopyField::ProfileName];
            if row.tailnet().is_some_and(|value| !value.is_empty()) {
                fields.push(CopyField::ProfileTailnet);
            }
            match row {
                ProfileRow::Local { account, .. } => {
                    if account.is_some() {
                        fields.push(CopyField::ProfileAccount);
                    }
                }
                ProfileRow::Admin { .. } => {
                    fields.push(CopyField::ProfileCredential);
                    fields.push(CopyField::ProfileBackend);
                }
            }
            return fields;
        }
        if self.current_route() == Route::Users {
            // The row is three facts and two of them are words already on
            // screen; the ones worth pasting are the identifiers.
            let Some(user) = self.selected_admin_user() else {
                return Vec::new();
            };
            let mut fields = vec![CopyField::UserId];
            if user.display_name.is_some() {
                fields.push(CopyField::UserName);
            }
            if user.login_name.is_some() {
                fields.push(CopyField::UserLogin);
            }
            return fields;
        }
        if self.current_route() != Route::Devices {
            return Vec::new();
        }
        let mut fields = vec![
            CopyField::DeviceId,
            CopyField::DisplayName,
            CopyField::Hostname,
        ];
        // Offered only when a name was actually reported: a key that copies
        // "not returned" is worse than a key that is not there.
        if self.selected_dns_name().is_some() {
            fields.push(CopyField::DnsName);
        }
        fields.extend([CopyField::Owner, CopyField::Addresses, CopyField::Tags]);
        if self.source_mode == SourceMode::Local {
            fields.push(CopyField::PublicKey);
            fields.push(CopyField::Endpoint);
        }
        fields
    }

    /// The selected device's full MagicDNS name. The local client reports it
    /// with a trailing dot, which is correct in a zone file and wrong in every
    /// place this value gets pasted.
    pub fn selected_dns_name(&self) -> Option<String> {
        let id = self.views.devices.selected_id.as_ref()?;
        let name = self.local_dns_name(id).map(str::to_owned).or_else(|| {
            self.admin
                .devices
                .snapshot
                .as_ref()?
                .iter()
                .find(|device| device.stable_id == id.0)
                .and_then(|device| device.name.clone())
        })?;
        let name = name.trim_end_matches('.');
        (!name.is_empty()).then(|| name.to_owned())
    }

    pub(super) fn service_actions_for_section(&self) -> Vec<ActionId> {
        match self.views.services.section {
            // One table, so both sets of actions belong to it. Which command
            // runs is decided by the exposure of the row, not by a tab.
            ServiceSection::Serve => vec![
                ActionId::ServicesServeRefresh,
                ActionId::ServicesServeCreate,
                ActionId::ServicesFunnelCreate,
                ActionId::ServicesServeEdit,
                ActionId::ServicesFunnelUnpublish,
                ActionId::ServicesServeRemove,
                ActionId::ServicesServeReset,
                ActionId::ServicesFunnelReset,
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
        }
    }

    pub(super) fn operational_resource_actions(&self) -> Vec<ActionId> {
        let mut actions = Vec::new();
        // Saved views and exports are for collections Tale fetched. `:profiles`
        // lists this machine's own configuration, which is already a file the
        // user owns, so offering to export it or to name a view of it would be
        // offering something with no subject.
        if !matches!(self.current_route(), Route::Profiles | Route::Config) {
            actions.extend([
                ActionId::SavedViewCreate,
                ActionId::SavedViewReplace,
                ActionId::SavedViewRename,
                ActionId::SavedViewDelete,
                ActionId::SavedViewApply,
                ActionId::CollectionExport,
            ]);
        }
        match self.current_route() {
            Route::Overview => actions.extend([
                ActionId::OverviewHealthOpenResource,
                ActionId::OverviewHealthRunSuggestedAction,
            ]),
            Route::Access => {
                if self.policy_workflow.is_some() {
                    actions.extend([
                        ActionId::AdminPolicyEditorReopen,
                        ActionId::AdminPolicyRemoteRefresh,
                        ActionId::AdminPolicyValidate,
                        ActionId::AdminPolicyPreview,
                        ActionId::AdminPolicyDiff,
                        ActionId::AdminPolicyApply,
                        ActionId::AdminPolicyCandidateDiscard,
                        ActionId::AdminPolicyWorkflowClose,
                    ]);
                } else {
                    actions.push(ActionId::AdminPolicyEdit);
                }
                actions.extend([
                    ActionId::AccessExplorerAsk,
                    ActionId::AccessExplorerOpenRule,
                ]);
            }
            Route::Audit => actions.extend([
                ActionId::ActivityFlowsSelectWindow,
                ActionId::ActivityFlowsAggregate,
                ActionId::ActivityFlowsOpenDevice,
                ActionId::AdminWebhookCreate,
                ActionId::AdminWebhookEdit,
                ActionId::AdminWebhookTest,
                ActionId::AdminWebhookRotateSecret,
                ActionId::AdminWebhookDelete,
                ActionId::AdminLogStreamReplace,
                ActionId::AdminLogStreamDelete,
                ActionId::AdminNetworkLogsSettings,
            ]),
            Route::Diagnostics => actions.extend([
                ActionId::ServicesMetricsRefresh,
                ActionId::ServicesBugReportCreate,
            ]),
            // The one thing a row on this page can be asked to do.
            Route::Profiles => actions.push(ActionId::ProfileActivate),
            Route::Devices
            | Route::Users
            | Route::Routes
            | Route::Dns
            | Route::Credentials
            | Route::Local
            | Route::Tasks
            | Route::Config
            | Route::Services => {}
        }
        actions
    }

    pub(super) fn open_service_action(&mut self, action_id: ActionId) -> Vec<Effect> {
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
            // Neither of these asks anything the row does not already answer,
            // so they go straight to the confirmation with no form in between.
            ActionId::ServicesServeRemove => {
                let Some(mapping) = self.selected_service_mapping() else {
                    self.runtime_error = Some("select a mapping to remove".to_owned());
                    return Vec::new();
                };
                self.open_service_confirmation(ServiceActionRequest::MappingRemove { mapping })
            }
            ActionId::ServicesFunnelUnpublish => {
                let Some(mapping) = self.selected_service_mapping() else {
                    self.runtime_error = Some("select a public mapping to unpublish".to_owned());
                    return Vec::new();
                };
                if mapping.exposure != Exposure::Public {
                    self.runtime_error =
                        Some("the selected mapping is already tailnet-only".to_owned());
                    return Vec::new();
                }
                self.open_service_confirmation(ServiceActionRequest::FunnelUnpublish { mapping })
            }
            ActionId::ServicesServeCreate | ActionId::ServicesFunnelCreate => {
                let public = action_id == ActionId::ServicesFunnelCreate;
                self.push_form(
                    action_id,
                    if public {
                        "New public mapping"
                    } else {
                        "New tailnet mapping"
                    },
                    vec![(
                        "reachable by",
                        reachability(&if public {
                            Exposure::Public
                        } else {
                            Exposure::Tailnet
                        })
                        .to_owned(),
                    )],
                    mapping_fields(public, None),
                );
                Vec::new()
            }
            ActionId::ServicesServeEdit => {
                // The selected row already knows its exposure, and Tailscale
                // replaces a mapping by listener and path, so those are stated
                // rather than offered: changing them is a new mapping.
                let Some(mapping) = self.selected_service_mapping() else {
                    self.runtime_error = Some("select a mapping to edit".to_owned());
                    return Vec::new();
                };
                self.push_form(
                    action_id,
                    "Edit mapping",
                    vec![
                        ("reachable by", reachability(&mapping.exposure).to_owned()),
                        (
                            "listener",
                            format!("{}:{}", mapping.listener.label(), mapping.listener.port()),
                        ),
                        ("path", mapping.mount.as_path().to_owned()),
                    ],
                    vec![
                        FormField::text(
                            "backend",
                            "Serve",
                            "A local port, an http:// URL, or a folder to serve files from",
                            "3000",
                            mapping.backend.argument(),
                        ),
                        FormField::options(
                            "proxy",
                            "PROXY protocol",
                            "Only used by TCP listeners; leave off unless the backend expects it",
                            &["none", "1", "2"],
                            mapping.proxy_protocol.cli_value().unwrap_or("none"),
                        ),
                    ],
                );
                Vec::new()
            }
            ActionId::DevicesTaildropSend => {
                // The selected row is the target, so the form asks only what
                // it cannot already know.
                let Some(device) = self.selected_device() else {
                    self.runtime_error = Some("select a device to send files to".to_owned());
                    return Vec::new();
                };
                let name = device.display_name.clone();
                let Some(target) = self.selected_taildrop_target() else {
                    self.runtime_error = Some(format!(
                        "{name} was not offered as a Taildrop target by this client"
                    ));
                    return Vec::new();
                };
                if !target.available() {
                    self.runtime_error = Some(match target.capability_reason.as_deref() {
                        Some(reason) => format!("{name} cannot receive files: {reason}"),
                        None => format!("{name} is offline"),
                    });
                    return Vec::new();
                }
                self.push_form(
                    action_id,
                    "Send files",
                    vec![("to", target.display_name.clone())],
                    vec![FormField::text(
                        "files",
                        "Files",
                        "Full paths or ~/ paths, separated by commas",
                        "~/path/to/file",
                        String::new(),
                    )],
                );
                Vec::new()
            }
            ActionId::DevicesTaildropReceive => {
                self.push_form(
                    action_id,
                    "Receive files",
                    Vec::new(),
                    vec![
                        FormField::text(
                            "directory",
                            "Save to",
                            "An existing directory on this machine; ~/ is supported",
                            "~/Downloads",
                            String::new(),
                        ),
                        FormField::options(
                            "conflict",
                            "If a name is taken",
                            "What to do when a file of that name already exists",
                            &["rename", "skip", "overwrite"],
                            "rename",
                        ),
                        FormField::toggle(
                            "wait",
                            "Keep waiting",
                            "Stay open for files that arrive later",
                            false,
                        ),
                    ],
                );
                Vec::new()
            }
            ActionId::ServicesDriveShare => {
                self.push_form(
                    action_id,
                    "Share a folder",
                    Vec::new(),
                    vec![
                        FormField::text(
                            "name",
                            "Share name",
                            "What the tailnet will see; letters, digits and dashes",
                            "documents",
                            String::new(),
                        ),
                        FormField::text(
                            "path",
                            "Folder",
                            "An existing directory on this machine; ~/ is supported",
                            "~/Documents",
                            String::new(),
                        ),
                    ],
                );
                Vec::new()
            }
            ActionId::ServicesDriveRename => {
                let Some(share) = self.selected_taildrive_share() else {
                    self.runtime_error = Some("select a share to rename".to_owned());
                    return Vec::new();
                };
                self.push_form(
                    action_id,
                    "Rename share",
                    vec![("current name", share.name.clone())],
                    vec![FormField::text(
                        "new",
                        "New name",
                        "Letters, digits and dashes",
                        "documents",
                        share.name,
                    )],
                );
                Vec::new()
            }
            ActionId::ServicesDriveUnshare => {
                let Some(share) = self.selected_taildrive_share() else {
                    self.runtime_error = Some("select a share to stop sharing".to_owned());
                    return Vec::new();
                };
                self.open_service_confirmation(ServiceActionRequest::TaildriveUnshare {
                    name: share.name,
                })
            }
            ActionId::ServicesCertificateObtain => {
                let Some(domain) = self.selected_certificate_domain().map(str::to_owned) else {
                    self.runtime_error = Some("select a domain".to_owned());
                    return Vec::new();
                };
                self.push_form(
                    action_id,
                    "Get a certificate",
                    vec![("domain", domain)],
                    vec![
                        FormField::text(
                            "cert",
                            "Certificate file",
                            "Where to write the certificate; ~/ is supported",
                            "~/certificate.crt",
                            String::new(),
                        ),
                        FormField::text(
                            "key",
                            "Key file",
                            "Where to write the private key; ~/ is supported",
                            "~/certificate.key",
                            String::new(),
                        ),
                        FormField::text(
                            "min-validity",
                            "Renew if under",
                            "Renew when less than this remains, such as 30d; blank never forces",
                            "30d",
                            String::new(),
                        ),
                    ],
                );
                Vec::new()
            }
            ActionId::ServicesMetricsRefresh => {
                self.start_service_request(ServiceActionRequest::Metrics)
            }
            ActionId::ServicesBugReportCreate => {
                self.push_form(
                    action_id,
                    "Create a bug report",
                    Vec::new(),
                    vec![
                        FormField::text(
                            "note",
                            "Note",
                            "What went wrong, in your own words",
                            "optional",
                            String::new(),
                        ),
                        FormField::toggle(
                            "diagnose",
                            "Run diagnostics",
                            "Collect extra network checks; takes longer",
                            false,
                        ),
                    ],
                );
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn push_form(
        &mut self,
        action_id: ActionId,
        title: &'static str,
        subject: Vec<(&'static str, String)>,
        fields: Vec<FormField>,
    ) {
        self.overlays.push(Overlay::Form(FormState {
            action_id,
            title,
            subject,
            fields,
            selected: 0,
            cursor: 0,
            draft: None,
            list: None,
            secret: None,
            error: None,
        }));
    }

    /// Reports why a form cannot be submitted on the form itself, so the user
    /// answers the question where they were asked it.
    pub(super) fn set_form_error(&mut self, error: impl Into<String>) -> Vec<Effect> {
        if let Some(Overlay::Form(current)) = self.overlays.last_mut() {
            current.error = Some(error.into());
        }
        Vec::new()
    }

    pub(super) fn accept_form(&mut self, state: FormState) -> Vec<Effect> {
        match state.action_id {
            ActionId::LocalSshOpen | ActionId::LocalNcOpen => {
                return self.accept_handoff_form(&state);
            }
            ActionId::LocalDnsQuery => return self.accept_dns_query_form(&state),
            ActionId::LocalWhois => return self.accept_whois_form(&state),
            ActionId::LocalPreferencesEdit => return self.accept_preferences_form(&state),
            ActionId::LocalExitNodeSelect => return self.accept_exit_node_form(&state),
            ActionId::LocalRoutesEditAdvertisements => {
                return self.accept_advertisement_form(&state);
            }
            ActionId::SavedViewCreate
            | ActionId::SavedViewReplace
            | ActionId::SavedViewRename
            | ActionId::SavedViewDelete
            | ActionId::SavedViewApply
            | ActionId::CollectionExport => {
                return self.accept_local_operational_form(&state);
            }
            ActionId::ActivityFlowsSelectWindow => return self.accept_flow_window_form(&state),
            ActionId::AdminCredentialAuthKeyCreate => return self.accept_auth_key_form(&state),
            ActionId::AdminWebhookCreate
            | ActionId::AdminWebhookEdit
            | ActionId::AdminLogStreamReplace
            | ActionId::AdminNetworkLogsSettings => {
                return self.accept_admin_operational_form(&state);
            }
            ActionId::AdminPolicyPreview => return self.accept_policy_preview_form(&state),
            ActionId::AccessExplorerAsk => return self.accept_access_explorer_form(&state),
            ActionId::AuditFilterTime
            | ActionId::AuditFilterActor
            | ActionId::AuditFilterAction
            | ActionId::AuditFilterTarget => return self.accept_audit_filter(&state),
            action_id if action_id.is_admin_mutation() => {
                return self.accept_admin_form(&state);
            }
            _ => {}
        }
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
                if let Some(Overlay::Form(current)) = self.overlays.last_mut() {
                    current.error = Some(error);
                }
                Vec::new()
            }
        }
    }

    pub(super) fn parse_service_form(
        &self,
        state: &FormState,
    ) -> Result<ServiceActionRequest, String> {
        let fields = state
            .fields
            .iter()
            .map(|field| (field.key.to_owned(), field.value.trim().to_owned()))
            .collect::<BTreeMap<_, _>>();
        match state.action_id {
            ActionId::ServicesServeCreate | ActionId::ServicesFunnelCreate => {
                let exposure = if state.action_id == ActionId::ServicesFunnelCreate {
                    Exposure::Public
                } else {
                    Exposure::Tailnet
                };
                let mapping = self.parse_mapping_form(&fields, exposure.clone())?;
                Ok(if exposure == Exposure::Public {
                    ServiceActionRequest::Funnel {
                        mapping,
                        edit: false,
                    }
                } else {
                    ServiceActionRequest::Serve {
                        mapping,
                        edit: false,
                    }
                })
            }
            // One edit action: the selected row decides which command runs, and
            // its listener and path are not editable, so identity always holds.
            ActionId::ServicesServeEdit => {
                let Some(selected) = self.selected_service_mapping() else {
                    return Err("select a mapping to edit".to_owned());
                };
                let backend = parse_form_backend(required_field(&fields, "backend")?)?;
                let proxy_protocol =
                    ProxyProtocol::parse(optional_field(&fields, "proxy").unwrap_or("none"))
                        .map_err(|error| error.to_string())?;
                let mapping = ServiceMapping {
                    backend,
                    proxy_protocol,
                    ..selected
                };
                mapping.validate().map_err(|error| error.to_string())?;
                Ok(if mapping.exposure == Exposure::Public {
                    ServiceActionRequest::Funnel {
                        mapping,
                        edit: true,
                    }
                } else {
                    ServiceActionRequest::Serve {
                        mapping,
                        edit: true,
                    }
                })
            }
            // The target is the selected device, never typed: the form is
            // modal, so the row it names is still the row underneath it.
            ActionId::DevicesTaildropSend => {
                let target = self.selected_taildrop_target().ok_or_else(|| {
                    "the selected device is no longer a Taildrop target".to_owned()
                })?;
                if !target.available() {
                    return Err("the selected Taildrop target is unavailable".to_owned());
                }
                let files = required_field(&fields, "files")?
                    .split(',')
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .map(std::path::PathBuf::from)
                    .map(|path| expand_form_path(&path))
                    .map(|path| path.and_then(|path| validate_regular_file(&path)))
                    .collect::<Result<Vec<_>, _>>()?;
                if files.is_empty() {
                    return Err("select at least one existing regular file".to_owned());
                }
                Ok(ServiceActionRequest::TaildropSend(TaildropSendRequest {
                    files,
                    target,
                }))
            }
            ActionId::DevicesTaildropReceive => {
                let directory = expand_form_path(Path::new(required_field(&fields, "directory")?))?;
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
                let path = expand_form_path(Path::new(required_field(&fields, "path")?))?;
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
                let domain = state
                    .subject
                    .iter()
                    .find_map(|(label, value)| (*label == "domain").then(|| value.clone()))
                    .ok_or_else(|| "the selected certificate domain is unavailable".to_owned())?;
                let certificate_path =
                    expand_form_path(Path::new(required_field(&fields, "cert")?))?;
                let key_path = expand_form_path(Path::new(required_field(&fields, "key")?))?;
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

    pub(super) fn parse_mapping_form(
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
        let mount = PathMount::parse(optional_field(fields, "path").unwrap_or("/"))
            .map_err(|error| error.to_string())?;
        let backend = parse_form_backend(required_field(fields, "backend")?)?;
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
        mapping.validate().map_err(|error| error.to_string())?;
        Ok(mapping)
    }

    pub(super) fn open_service_confirmation(
        &mut self,
        request: ServiceActionRequest,
    ) -> Vec<Effect> {
        let Some((preview_lines, redacted_argv)) = self.service_preview(&request) else {
            self.runtime_error = Some("service command preview is unavailable".to_owned());
            return Vec::new();
        };
        let (prompt, required_phrase) = service_confirmation_text(&request);
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: request.action_id(),
                admin_generation: self.admin_generation,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: Some(request),
                operational_mutation: None,
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

    pub(super) fn service_preview(
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
            ServiceActionRequest::MappingRemove { mapping } => {
                services::mapping_off_command(command_path, timeout, mapping, true).ok()?
            }
            ServiceActionRequest::FunnelUnpublish { mapping } => {
                services::mapping_unpublish_command(command_path, timeout, mapping, true).ok()?
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
        // The argv already appears under Command; the preview says what the
        // change means, in the reader's terms.
        let mut preview = vec![service_effect_sentence(request)];
        if let ServiceActionRequest::TaildriveShare {
            input_name,
            normalized_name,
            ..
        } = request
            && input_name != normalized_name
        {
            preview.push(format!(
                "\"{input_name}\" is not a usable share name; it will be shared as \"{normalized_name}\"."
            ));
        }
        if let ServiceActionRequest::TaildropSend(request) = request {
            preview.push("Resolved files:".to_owned());
            preview.extend(
                request
                    .files
                    .iter()
                    .map(|file| format!("  {}", file.path.display())),
            );
        }
        if let ServiceActionRequest::Certificate(request) = request {
            preview.push(format!(
                "Certificate file: {}",
                request.certificate_path.display()
            ));
            preview.push(format!("Key file: {}", request.key_path.display()));
        }
        Some((preview, argv))
    }

    pub(super) fn start_service_request(&mut self, request: ServiceActionRequest) -> Vec<Effect> {
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

    /// Whether the last status read still shows this exact mapping, in the list
    /// its exposure puts it in. Identity is listener and path, so the backend is
    /// compared too: the same address serving something else is a different row.
    pub(super) fn service_mapping_is_current(&self, mapping: &ServiceMapping) -> bool {
        let listed: Option<&[ServiceMapping]> = match mapping.exposure {
            Exposure::Public => self
                .services_snapshot
                .funnel
                .value
                .as_ref()
                .map(|status| status.mappings.as_slice()),
            Exposure::Tailnet => self
                .services_snapshot
                .serve
                .value
                .as_ref()
                .map(|status| status.mappings.as_slice()),
        };
        listed.is_some_and(|mappings| {
            mappings.iter().any(|actual| {
                actual.exact_identity_matches(mapping) && actual.backend == mapping.backend
            })
        })
    }

    pub(super) fn revalidate_service_request(
        &self,
        request: &ServiceActionRequest,
    ) -> Result<(), String> {
        match request {
            ServiceActionRequest::Serve { mapping, edit } => {
                mapping.validate().map_err(|error| error.to_string())?;
                if mapping.exposure != Exposure::Tailnet {
                    return Err("Serve requests must remain tailnet-only".to_owned());
                }
                if !self.local_capabilities.serve {
                    return Err("Serve is unsupported by this CLI".to_owned());
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
                if !self.local_capabilities.funnel {
                    return Err("Funnel is unsupported by this CLI".to_owned());
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
            // A stale row is the whole hazard here: removing by listener and
            // path would happily take down whatever now sits at that address.
            ServiceActionRequest::MappingRemove { mapping } => {
                mapping.validate().map_err(|error| error.to_string())?;
                if !self.service_mapping_is_current(mapping) {
                    return Err("the selected mapping changed; refresh and remove again".to_owned());
                }
                Ok(())
            }
            ServiceActionRequest::FunnelUnpublish { mapping } => {
                mapping.validate().map_err(|error| error.to_string())?;
                if mapping.exposure != Exposure::Public {
                    return Err("only a public mapping can stop being published".to_owned());
                }
                if !self.local_capabilities.serve {
                    return Err("Serve is unsupported by this CLI".to_owned());
                }
                if !self.service_mapping_is_current(mapping) {
                    return Err(
                        "the selected PUBLIC mapping changed; refresh and unpublish again"
                            .to_owned(),
                    );
                }
                // The mapping is re-served verbatim, so the backend has to be
                // as usable now as it was when it was first accepted.
                validate_mapping_backend(mapping)
            }
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

    pub(super) fn service_task_metadata(
        &self,
        request: &ServiceActionRequest,
    ) -> Option<(Vec<String>, Vec<String>)> {
        let (_, argv) = self.service_preview(request)?;
        let fields = match request {
            ServiceActionRequest::Serve { mapping, .. }
            | ServiceActionRequest::Funnel { mapping, .. }
            | ServiceActionRequest::FunnelUnpublish { mapping } => {
                vec![
                    "listener".to_owned(),
                    "mount".to_owned(),
                    mapping.backend.label().to_owned(),
                ]
            }
            ServiceActionRequest::MappingRemove { .. } => {
                vec!["listener".to_owned(), "mount".to_owned()]
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

    pub(super) fn start_services_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        match action_id {
            ActionId::ServicesMetricsRefresh => {
                self.open_service_action(ActionId::ServicesMetricsRefresh)
            }
            _ => self.start_services_refresh(),
        }
    }

    pub(super) fn start_services_refresh(&mut self) -> Vec<Effect> {
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

    pub(super) fn update_services(&mut self, event: ServicesEvent) -> Vec<Effect> {
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
                        | ServiceActionRequest::MappingRemove { .. }
                        | ServiceActionRequest::Funnel { .. }
                        | ServiceActionRequest::FunnelUnpublish { .. }
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

    pub(super) fn update_service_capabilities(&mut self) {
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
}
