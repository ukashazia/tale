use super::*;

impl App {
    pub(super) fn open_admin_operational_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        match action_id {
            ActionId::AdminWebhookCreate
            | ActionId::AdminWebhookEdit
            | ActionId::AdminLogStreamReplace
            | ActionId::AdminNetworkLogsSettings => self.open_admin_form(action_id),
            ActionId::AdminWebhookTest => {
                let Some(webhook) = self.selected_webhook() else {
                    self.runtime_error = Some("no observed webhook is available".to_owned());
                    return Vec::new();
                };
                self.open_operational_confirmation(
                    action_id,
                    OperationalMutation::Webhook(WebhookMutation::Test {
                        endpoint_id: webhook.stable_id.clone(),
                    }),
                )
            }
            ActionId::AdminWebhookRotateSecret => {
                let Some(webhook) = self.selected_webhook() else {
                    self.runtime_error = Some("no observed webhook is available".to_owned());
                    return Vec::new();
                };
                self.open_operational_confirmation(
                    action_id,
                    OperationalMutation::Webhook(WebhookMutation::RotateSecret {
                        endpoint_id: webhook.stable_id.clone(),
                    }),
                )
            }
            ActionId::AdminWebhookDelete => {
                let Some(webhook) = self.selected_webhook() else {
                    self.runtime_error = Some("no observed webhook is available".to_owned());
                    return Vec::new();
                };
                self.open_operational_confirmation(
                    action_id,
                    OperationalMutation::Webhook(WebhookMutation::Delete {
                        endpoint_id: webhook.stable_id.clone(),
                        endpoint_label: webhook.endpoint_url.clone(),
                    }),
                )
            }
            ActionId::AdminLogStreamDelete => {
                let log_type = self
                    .log_stream_configurations
                    .keys()
                    .next()
                    .copied()
                    .unwrap_or(LogType::Network);
                self.open_operational_confirmation(
                    action_id,
                    OperationalMutation::LogStreamDelete(log_type),
                )
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn open_operational_confirmation(
        &mut self,
        action_id: ActionId,
        mutation: OperationalMutation,
    ) -> Vec<Effect> {
        self.pending_export_fingerprint = match &mutation {
            OperationalMutation::Export(request) => match self.export_fingerprint(request) {
                Ok(fingerprint) => Some(fingerprint),
                Err(error) => {
                    self.runtime_error = Some(format!("export preview unavailable: {error}"));
                    return Vec::new();
                }
            },
            _ => None,
        };
        let required_phrase = match &mutation {
            OperationalMutation::Webhook(WebhookMutation::Test { .. }) => None,
            OperationalMutation::Webhook(WebhookMutation::RotateSecret { .. }) => {
                Some("ROTATE WEBHOOK SECRET".to_owned())
            }
            OperationalMutation::Webhook(WebhookMutation::Delete { .. }) => {
                Some("DELETE WEBHOOK".to_owned())
            }
            OperationalMutation::LogStreamDelete(_) => Some("DELETE LOG STREAM".to_owned()),
            OperationalMutation::Webhook(_)
            | OperationalMutation::LogStreamReplace(_)
            | OperationalMutation::NetworkLogSetting { .. } => {
                Some("APPLY OPERATIONAL CHANGE".to_owned())
            }
            OperationalMutation::SavedView(SavedViewMutation::Replace { .. })
            | OperationalMutation::SavedView(SavedViewMutation::Delete { .. }) => None,
            OperationalMutation::SavedView(
                SavedViewMutation::Create(_)
                | SavedViewMutation::Rename { .. }
                | SavedViewMutation::Apply { .. },
            ) => None,
            OperationalMutation::Export(request) if request.path.exists() => {
                Some("OVERWRITE EXPORT".to_owned())
            }
            OperationalMutation::Export(_) => None,
        };
        let prompt = match &mutation {
            OperationalMutation::Webhook(WebhookMutation::Test { .. }) => {
                "Queue a server-side webhook test? Tale will report acknowledgement only.".to_owned()
            }
            OperationalMutation::Webhook(WebhookMutation::RotateSecret { .. }) => {
                "Rotate this webhook's write-only signing secret? The new secret is shown once.".to_owned()
            }
            OperationalMutation::Webhook(WebhookMutation::Delete { .. }) => {
                "Delete this webhook after a final typed confirmation?".to_owned()
            }
            OperationalMutation::LogStreamDelete(_) => {
                "Delete this log-stream configuration?".to_owned()
            }
            OperationalMutation::Webhook(_)
            | OperationalMutation::LogStreamReplace(_)
            | OperationalMutation::NetworkLogSetting { .. } => {
                "Apply this change?".to_owned()
            }
            OperationalMutation::SavedView(_) => {
                "Apply this saved-view operation? The document stores only query and presentation state.".to_owned()
            }
            OperationalMutation::Export(_) => {
                "Write this allowlisted deterministic export?".to_owned()
            }
        };
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id,
                admin_generation: self.admin_generation,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: Some(mutation.clone()),
                handoff: None,
                prompt,
                required_phrase,
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines: vec![
                    mutation.preview(),
                    "Tale sends this request once, then refreshes the setting to confirm it."
                        .to_owned(),
                ],
                redacted_argv: Vec::new(),
                error: None,
            })));
        Vec::new()
    }

    pub(super) fn dispatch_health_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        let Some(finding) = self.selected_overview_finding().cloned() else {
            self.runtime_error = Some("no derived health finding is available".to_owned());
            return Vec::new();
        };
        if action_id == ActionId::OverviewHealthOpenResource {
            return self.open_health_finding_resource(&finding);
        }
        let Some(suggested) = finding.suggested_action_ids.first() else {
            self.runtime_error = Some(
                "this derived finding has no suggested action; inspect its observed facts"
                    .to_owned(),
            );
            return Vec::new();
        };
        let action = match suggested.as_str() {
            "admin.device.approve" => ActionId::AdminDeviceApprove,
            "admin.device.key_expire_now" => ActionId::AdminDeviceKeyExpireNow,
            "admin.routes.replace_approvals" => ActionId::AdminRoutesReplaceApprovals,
            "admin.user.approve" => ActionId::AdminUserApprove,
            _ => {
                self.runtime_error = Some(format!(
                    "suggested action {suggested} is not registered in the current action catalog"
                ));
                return Vec::new();
            }
        };
        if !self.action_available_for_id(action) {
            self.runtime_error = self.action_unavailable_reason(action);
            return Vec::new();
        }
        self.dispatch_action(action)
    }

    pub(super) fn open_health_finding_resource(&mut self, finding: &Finding) -> Vec<Effect> {
        let Some(affected_id) = finding.affected_resource_ids.first().cloned() else {
            self.runtime_error = Some("the selected finding names no affected resource".to_owned());
            return Vec::new();
        };
        match finding.rule_id.as_str() {
            "device-key-expired"
            | "device-key-expiring"
            | "device-approval-pending"
            | "posture-observation-missing"
            | "relay-heavy-local-peer" => {
                let node_id = self
                    .admin
                    .devices
                    .snapshot
                    .as_ref()
                    .and_then(|devices| {
                        devices
                            .iter()
                            .find(|device| device.stable_id == affected_id)
                    })
                    .and_then(AdminDevice::exact_node_id);
                let selected = self
                    .devices_resource
                    .snapshot
                    .iter()
                    .find(|device| {
                        device.id.0 == affected_id
                            || node_id.is_some_and(|node_id| device.id.0 == node_id)
                    })
                    .map(|device| device.id.clone());
                let Some(selected) = selected else {
                    self.runtime_error = Some(
                        "the affected device is no longer in the current device snapshot"
                            .to_owned(),
                    );
                    return Vec::new();
                };
                self.views.devices.filter_draft.clear();
                self.views.devices.applied_filter = FilterExpression::empty();
                self.navigate(Route::Devices);
                self.views.devices.selected_id = Some(selected);
                self.reconcile_selection(None);
                self.reset_device_detail_state();
                self.focus = Focus::Inspector;
                return self
                    .start_admin_device_enrichment(Some(affected_id))
                    .into_iter()
                    .collect();
            }
            "user-approval-pending" => {
                self.views.users.filter.clear();
                let selected = self
                    .admin
                    .users
                    .snapshot
                    .as_ref()
                    .and_then(|users| users.iter().position(|user| user.id == affected_id));
                self.navigate(Route::Users);
                if let Some(selected) = selected {
                    self.admin_user_selected = selected;
                    self.focus = Focus::Inspector;
                } else {
                    self.runtime_error =
                        Some("the affected user is no longer in the current snapshot".to_owned());
                }
            }
            "route-overlap-review" => {
                self.views.routes.filter.clear();
                let selected = self.admin.route_observations().iter().position(|route| {
                    route
                        .advertised
                        .iter()
                        .any(|cidr| format!("{}:{cidr}", route.device_id) == affected_id)
                });
                self.navigate(Route::Routes);
                if let Some(selected) = selected {
                    self.admin_route_selected = selected;
                } else {
                    self.runtime_error =
                        Some("the affected route is no longer in the current snapshot".to_owned());
                }
            }
            _ => {
                self.runtime_error = Some(
                    "the selected finding has evidence but no resource route to open".to_owned(),
                );
            }
        }
        Vec::new()
    }

    pub(super) fn open_local_operational_action(&mut self, action_id: ActionId) -> Vec<Effect> {
        match action_id {
            ActionId::AccessExplorerOpenRule => {
                if let Some(result) = self.access_explorer_result.as_ref() {
                    self.runtime_error = Some(format!(
                        "Matching policy rule locations: {}",
                        result
                            .rule_locations
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                } else {
                    self.runtime_error = Some("No access preview is available".to_owned());
                }
                Vec::new()
            }
            ActionId::SavedViewCreate | ActionId::SavedViewReplace => {
                self.open_saved_view_form(action_id)
            }
            ActionId::SavedViewRename => {
                let names = self.saved_view_names();
                let current = names.first().cloned().unwrap_or_default();
                self.push_form(
                    action_id,
                    "Rename a saved view",
                    Vec::new(),
                    vec![
                        FormField::choice(
                            "name",
                            "View",
                            "Which saved view is renamed",
                            names.into_iter().map(FormChoice::plain),
                            current,
                        ),
                        FormField::text(
                            "new",
                            "New name",
                            "What the view is called from now on",
                            "new name",
                            String::new(),
                        ),
                    ],
                );
                Vec::new()
            }
            ActionId::SavedViewDelete | ActionId::SavedViewApply => {
                let names = self.saved_view_names();
                let current = names.first().cloned().unwrap_or_default();
                let (title, help) = if action_id == ActionId::SavedViewDelete {
                    ("Delete a saved view", "The saved view to remove")
                } else {
                    ("Open a saved view", "The saved view to switch to")
                };
                self.push_form(
                    action_id,
                    title,
                    Vec::new(),
                    vec![FormField::choice(
                        "name",
                        "View",
                        help,
                        names.into_iter().map(FormChoice::plain),
                        current,
                    )],
                );
                Vec::new()
            }
            ActionId::CollectionExport => {
                self.push_form(
                    action_id,
                    "Export a collection to a file",
                    Vec::new(),
                    vec![
                        FormField::options(
                            "collection",
                            "Collection",
                            "Which set of records is written out",
                            EXPORT_COLLECTIONS,
                            "devices",
                        ),
                        FormField::options(
                            "format",
                            "Format",
                            "How the records are written",
                            &["json", "csv"],
                            "json",
                        ),
                        FormField::text(
                            "path",
                            "Path",
                            "Where the file is written; ~/ is supported and an existing file is replaced",
                            "~/export.json",
                            String::new(),
                        ),
                    ],
                );
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// A form whose later questions depend on an earlier answer rebuilds them
    /// when that answer changes, so it never asks for a field the choice made
    /// meaningless.
    pub(super) fn refresh_form_fields(&mut self) {
        let Some(Overlay::Form(state)) = self.overlays.last_mut() else {
            return;
        };
        if state.action_id != ActionId::AdminLogStreamReplace {
            return;
        }
        let destination = state.value("destination").to_owned();
        let kept = state
            .fields
            .iter()
            .map(|field| (field.key, field.value.clone()))
            .collect::<BTreeMap<_, _>>();
        state.fields = log_stream_fields(&destination, &kept);
        state.selected = state.selected.min(state.fields.len());
    }

    pub(super) fn open_log_stream_form(&mut self) -> Vec<Effect> {
        let configuration = self
            .log_stream_configurations
            .get(&LogType::Network)
            .or_else(|| self.log_stream_configurations.get(&LogType::Configuration));
        let (log_type, destination, url) = configuration.map_or_else(
            || ("network".to_owned(), "splunk".to_owned(), String::new()),
            |configuration| {
                (
                    configuration.log_type.wire_value().to_owned(),
                    configuration.destination.kind.clone(),
                    configuration.destination.identity.clone(),
                )
            },
        );
        let seed = BTreeMap::from([
            ("type", log_type),
            ("destination", destination.clone()),
            ("url", url),
        ]);
        self.push_form(
            ActionId::AdminLogStreamReplace,
            "Replace a log stream",
            Vec::new(),
            log_stream_fields(&destination, &seed),
        );
        Vec::new()
    }

    pub(super) fn open_webhook_form(&mut self, action_id: ActionId) -> Vec<Effect> {
        if action_id == ActionId::AdminWebhookEdit {
            let Some(webhook) = self.selected_webhook() else {
                self.runtime_error = Some("no observed webhook is available".to_owned());
                return Vec::new();
            };
            let subject = vec![("endpoint", webhook.endpoint_url.clone())];
            let categories = webhook.subscriptions.wire_categories();
            let events = webhook.subscriptions.wire_events();
            self.push_form(
                action_id,
                "Edit what a webhook is told about",
                subject,
                vec![
                    FormField::list(
                        "categories",
                        "Categories",
                        "Whole categories this endpoint is subscribed to",
                        "none",
                        categories,
                    ),
                    FormField::list(
                        "events",
                        "Events",
                        "Individual events on top of the categories; unknown ones are kept",
                        "none",
                        events,
                    ),
                ],
            );
            return Vec::new();
        }
        self.push_form(
            action_id,
            "Add a webhook endpoint",
            Vec::new(),
            vec![
                FormField::text(
                    "url",
                    "Endpoint",
                    "Where the tailnet posts each notification",
                    "https://host.example/path",
                    String::new(),
                ),
                FormField::options(
                    "provider",
                    "Provider",
                    "How the payload is shaped for the receiving service",
                    WEBHOOK_PROVIDERS,
                    "none",
                ),
                FormField::list(
                    "categories",
                    "Categories",
                    "Whole categories this endpoint is subscribed to",
                    "none",
                    Vec::<String>::new(),
                ),
                FormField::list(
                    "events",
                    "Events",
                    "Individual events on top of the categories",
                    "none",
                    Vec::<String>::new(),
                ),
            ],
        );
        Vec::new()
    }

    pub(super) fn open_network_log_form(&mut self) -> Vec<Effect> {
        let enabled = self
            .admin
            .settings
            .snapshot
            .as_ref()
            .and_then(|settings| settings.network_flow_logging_on)
            .unwrap_or(true);
        self.push_form(
            ActionId::AdminNetworkLogsSettings,
            "Configure network flow logging",
            Vec::new(),
            vec![FormField::toggle(
                "enabled",
                "Flow logging",
                "Whether devices record and report their network flows",
                enabled,
            )],
        );
        Vec::new()
    }

    pub(super) fn open_auth_key_form(&mut self) -> Vec<Effect> {
        self.push_form(
            ActionId::AdminCredentialAuthKeyCreate,
            "Create an auth key",
            Vec::new(),
            vec![
                FormField::text(
                    "description",
                    "Description",
                    "What this key is for, shown in the credential list",
                    "tale-generated",
                    "tale-generated",
                ),
                FormField::text(
                    "expiry",
                    "Valid for",
                    "Whole days before the key stops working",
                    "days",
                    "7",
                ),
                FormField::toggle(
                    "reusable",
                    "Reusable",
                    "Let the key register more than one device",
                    false,
                ),
                FormField::toggle(
                    "ephemeral",
                    "Ephemeral",
                    "Remove devices registered with this key when they go offline",
                    true,
                ),
                FormField::toggle(
                    "preauthorized",
                    "Pre-approved",
                    "Devices registered with this key need no separate approval",
                    false,
                ),
                FormField::list(
                    "tags",
                    "Tags",
                    "The tags every device registered with this key receives",
                    "no tags",
                    Vec::<String>::new(),
                ),
            ],
        );
        Vec::new()
    }

    pub(super) fn saved_view_names(&self) -> Vec<String> {
        self.saved_views
            .as_ref()
            .map(|state| state.names())
            .unwrap_or_default()
    }

    /// A saved view captures the screen that is already visible. The user names
    /// it; columns, filters, and sorting are UI state, not a serialization
    /// format the form asks them to type.
    pub(super) fn open_saved_view_form(&mut self, action_id: ActionId) -> Vec<Effect> {
        let route = self.current_route().label();
        let title = if action_id == ActionId::SavedViewCreate {
            "Save this view"
        } else {
            "Replace a saved view"
        };
        self.push_form(
            action_id,
            title,
            vec![("route", route.to_owned())],
            vec![FormField::text(
                "name",
                "Name",
                "What this view is called; the current columns, filter, and sort are captured",
                "view name",
                String::new(),
            )],
        );
        Vec::new()
    }

    /// The window comes first and the rest narrows it, so each filter is its
    /// own field holding the value the current view is already using.
    pub(super) fn open_flow_window_form(&mut self) -> Vec<Effect> {
        let now = time::OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(self.now as i64);
        let window = crate::domain::flow::FlowWindow::previous_hour(now);
        let (start, end) = window
            .query_values()
            .unwrap_or_else(|_| (String::new(), String::new()));
        let filter = self.flow_filter.clone();
        self.push_form(
            ActionId::ActivityFlowsSelectWindow,
            "Choose which flows to read",
            Vec::new(),
            vec![
                FormField::text(
                    "start",
                    "From",
                    "Inclusive UTC start; the window is at most 24 hours and within retention",
                    "RFC3339 UTC",
                    start,
                ),
                FormField::text(
                    "end",
                    "To",
                    "Inclusive UTC end; the window is at most 24 hours and within retention",
                    "RFC3339 UTC",
                    end,
                ),
                FormField::text(
                    "reporting",
                    "Reported by",
                    "Only flows the named device reported",
                    "any device",
                    filter.reporting_node_id.unwrap_or_default(),
                ),
                FormField::text(
                    "source",
                    "From device",
                    "Only flows that started at this device",
                    "any device",
                    filter.source_node_id.unwrap_or_default(),
                ),
                FormField::text(
                    "destination",
                    "To device",
                    "Only flows that ended at this device",
                    "any device",
                    filter.destination_node_id.unwrap_or_default(),
                ),
                FormField::text(
                    "source-address",
                    "From address",
                    "Only flows that started at this address",
                    "any address",
                    filter.source_address.unwrap_or_default(),
                ),
                FormField::text(
                    "destination-address",
                    "To address",
                    "Only flows that ended at this address",
                    "any address",
                    filter.destination_address.unwrap_or_default(),
                ),
                FormField::text(
                    "protocol",
                    "Protocol",
                    "Only flows carried over this protocol",
                    "any protocol",
                    filter.protocol.unwrap_or_default(),
                ),
                FormField::options(
                    "class",
                    "Traffic",
                    "Which sort of traffic the flow carried",
                    TRAFFIC_CLASSES,
                    filter
                        .traffic_class
                        .map_or_else(|| ANY.to_owned(), |class| class.label().to_owned()),
                ),
                FormField::text(
                    "source-port",
                    "From port",
                    "Only flows that started at this port",
                    "any port",
                    filter
                        .source_port
                        .map_or_else(String::new, |value| value.to_string()),
                ),
                FormField::text(
                    "destination-port",
                    "To port",
                    "Only flows that ended at this port",
                    "any port",
                    filter
                        .destination_port
                        .map_or_else(String::new, |value| value.to_string()),
                ),
                FormField::text(
                    "min-bytes",
                    "At least",
                    "Only flows that carried at least this many bytes",
                    "any size",
                    filter
                        .minimum_bytes
                        .map_or_else(String::new, |value| value.to_string()),
                ),
            ],
        );
        Vec::new()
    }

    pub(super) fn accept_flow_window_form(&mut self, state: &FormState) -> Vec<Effect> {
        let (window, mut filter) = match flow_window_from_form(state, self.now) {
            Ok(value) => value,
            Err(error) => return self.set_form_error(error),
        };
        if let Err(error) = self.resolve_flow_filter_labels(&mut filter) {
            return self.set_form_error(error);
        }
        self.overlays.pop();
        self.cancel_flow_aggregation();
        self.flow_aggregation_generation = self.flow_aggregation_generation.saturating_add(1);
        self.flow_filter = filter;
        self.flow_snapshot = None;
        self.flow_generation.begin();
        self.start_admin_resource_refresh(vec![AdminRefreshResource::FlowLogs(window)])
    }

    /// The explorer asks the server one question, so the form asks for the two
    /// ends of it and which policy to ask against.
    pub(super) fn open_access_explorer_form(&mut self) -> Vec<Effect> {
        self.push_form(
            ActionId::AccessExplorerAsk,
            "Ask whether one device can reach another",
            Vec::new(),
            vec![
                FormField::text(
                    "source",
                    "From",
                    "The device, user, or tag the connection starts at",
                    "user:someone@example.com",
                    String::new(),
                ),
                FormField::text(
                    "destination",
                    "To",
                    "The device, address, or tag the connection is made to",
                    "100.64.0.1",
                    String::new(),
                ),
                FormField::text(
                    "port",
                    "Port",
                    "A port number or protocol name; empty asks about any port",
                    "any",
                    String::new(),
                ),
                FormField::options(
                    "policy",
                    "Policy",
                    "Whether the question is asked of the live policy or the candidate",
                    &["current", "candidate"],
                    "current",
                ),
            ],
        );
        Vec::new()
    }
}

impl App {
    pub(super) fn accept_admin_operational_form(&mut self, state: &FormState) -> Vec<Effect> {
        let result = match state.action_id {
            ActionId::AdminWebhookCreate => webhook_create_from_form(state),
            ActionId::AdminWebhookEdit => self.webhook_edit_from_form(state),
            ActionId::AdminLogStreamReplace => log_stream_from_form(state),
            ActionId::AdminNetworkLogsSettings => Ok(OperationalMutation::NetworkLogSetting {
                enabled: state.is_yes("enabled"),
            }),
            _ => Err("this is not an admin operational form".to_owned()),
        };
        match result {
            Ok(mutation) => {
                self.overlays.pop();
                self.open_operational_confirmation(state.action_id, mutation)
            }
            Err(error) => self.set_form_error(error),
        }
    }

    pub(super) fn accept_auth_key_form(&mut self, state: &FormState) -> Vec<Effect> {
        let days = match state.value("expiry").trim().parse::<u64>() {
            Ok(days) => days,
            Err(_) => {
                return self.set_form_error("the key must be valid for a whole number of days");
            }
        };
        let Some(expiry_seconds) = days.checked_mul(24 * 60 * 60) else {
            return self.set_form_error("that many days is too long");
        };
        let description = state.value("description").trim();
        let request = crate::admin::key_mutations::AuthKeyCreateRequest {
            description: (!description.is_empty()).then(|| description.to_owned()),
            expiry_seconds,
            reusable: state.is_yes("reusable"),
            ephemeral: state.is_yes("ephemeral"),
            preauthorized: state.is_yes("preauthorized"),
            tags: state.entries("tags"),
        };
        if let Err(error) = request.validate() {
            return self.set_form_error(error.to_string());
        }
        self.overlays.pop();
        self.open_auth_key_form_with_request(request)
    }

    pub(super) fn webhook_edit_from_form(
        &self,
        state: &FormState,
    ) -> Result<OperationalMutation, String> {
        let endpoint = self
            .selected_webhook()
            .ok_or_else(|| "no observed webhook is available".to_owned())?;
        let after = endpoint
            .subscriptions
            .edit_known(state.entries("categories"), state.entries("events"))
            .map_err(|error| error.to_string())?;
        Ok(OperationalMutation::Webhook(
            WebhookMutation::EditSubscriptions {
                endpoint_id: endpoint.stable_id.clone(),
                endpoint_url: endpoint.endpoint_url.clone(),
                destination_type: endpoint.destination_type.clone(),
                before: endpoint.subscriptions.clone(),
                after,
            },
        ))
    }

    pub(super) fn resolve_flow_filter_labels(&self, filter: &mut FlowFilter) -> Result<(), String> {
        let Some(devices) = self.admin.devices.snapshot.as_ref() else {
            if filter.reporting_node_label.is_some()
                || filter.source_node_label.is_some()
                || filter.destination_node_label.is_some()
            {
                return Err(
                    "flow label filters require an observed device snapshot for exact ID resolution"
                        .to_owned(),
                );
            }
            return Ok(());
        };
        let resolve = |label: &mut Option<String>| -> Result<Option<String>, String> {
            let Some(label) = label.as_deref() else {
                return Ok(None);
            };
            let matches = devices
                .iter()
                .filter(|device| device.display_name() == label)
                .map(|device| device.stable_id.clone())
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [stable_id] => Ok(Some(stable_id.clone())),
                [] => Err(format!(
                    "flow label {label} was not returned by the device source"
                )),
                _ => Err(format!(
                    "flow label {label} is ambiguous; use a stable node ID"
                )),
            }
        };
        if filter.reporting_node_id.is_none() {
            filter.reporting_node_id = resolve(&mut filter.reporting_node_label)?;
        }
        if filter.source_node_id.is_none() {
            filter.source_node_id = resolve(&mut filter.source_node_label)?;
        }
        if filter.destination_node_id.is_none() {
            filter.destination_node_id = resolve(&mut filter.destination_node_label)?;
        }
        Ok(())
    }

    pub(super) fn cancel_flow_aggregation(&mut self) {
        if let Some(cancellation) = self.flow_aggregation_cancellation.take() {
            cancellation.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn accept_local_operational_form(&mut self, state: &FormState) -> Vec<Effect> {
        let result = match state.action_id {
            ActionId::SavedViewCreate | ActionId::SavedViewReplace => {
                self.saved_view_from_form(state).map(|view| {
                    if state.action_id == ActionId::SavedViewCreate {
                        OperationalMutation::SavedView(SavedViewMutation::Create(view))
                    } else {
                        OperationalMutation::SavedView(SavedViewMutation::Replace {
                            name: view.name.clone(),
                            view,
                        })
                    }
                })
            }
            ActionId::SavedViewRename => {
                let name = required_form_value(state, "name", "a view to rename");
                let replacement = required_form_value(state, "new", "a new name");
                name.and_then(|name| {
                    replacement.map(|replacement| {
                        OperationalMutation::SavedView(SavedViewMutation::Rename {
                            name,
                            replacement,
                        })
                    })
                })
            }
            ActionId::SavedViewDelete => required_form_value(state, "name", "a view to delete")
                .map(|name| OperationalMutation::SavedView(SavedViewMutation::Delete { name })),
            ActionId::SavedViewApply => required_form_value(state, "name", "a view to open")
                .map(|name| OperationalMutation::SavedView(SavedViewMutation::Apply { name })),
            ActionId::CollectionExport => export_from_form(state),
            _ => Err("this is not a local operational form".to_owned()),
        };
        match result {
            Ok(mutation) => {
                self.overlays.pop();
                self.open_operational_confirmation(state.action_id, mutation)
            }
            Err(error) => self.set_form_error(error),
        }
    }

    pub(super) fn saved_view_from_form(&self, state: &FormState) -> Result<SavedView, String> {
        let name = required_form_value(state, "name", "a name for this view")?;
        let route = state
            .subject
            .iter()
            .find(|(label, _)| *label == "route")
            .map(|(_, value)| value.clone())
            .ok_or_else(|| "the view has no route to save".to_owned())?;
        if route != Route::Devices.label() {
            return Ok(SavedView {
                name,
                route,
                wide_columns: false,
                columns: Vec::new(),
                filters: Vec::new(),
                sort: Vec::new(),
            });
        }
        let filters = self
            .views
            .devices
            .applied_filter
            .terms
            .iter()
            .map(saved_filter_from_term)
            .collect::<Result<Vec<_>, _>>()?;
        let sort = self
            .views
            .devices
            .sort_terms
            .iter()
            .copied()
            .map(saved_sort_from_device)
            .collect();
        Ok(SavedView {
            name,
            route,
            wide_columns: self.views.devices.wide_columns,
            columns: self.views.devices.columns.clone(),
            filters,
            sort,
        })
    }

    pub(super) fn accept_access_explorer_form(&mut self, state: &FormState) -> Vec<Effect> {
        let result = access_question_from_form(state).and_then(|question| {
            let policy = match question.policy_source {
                PolicySource::CurrentRemote => self
                    .admin
                    .policy
                    .snapshot
                    .as_ref()
                    .ok_or_else(|| "current remote policy is not observed".to_owned())
                    .and_then(|snapshot| {
                        crate::domain::policy_workflow::PolicyDocument::from_bytes_with_content_type(
                            snapshot.source_bytes.clone(),
                            snapshot.content_type.clone(),
                            snapshot.fetched_at,
                        )
                        .map_err(|error| error.to_string())
                    })?,
                PolicySource::ActiveCandidate => self
                    .policy_workflow
                    .as_ref()
                    .and_then(|workflow| workflow.candidate().cloned())
                    .ok_or_else(|| "an active policy candidate is not available".to_owned())?,
            };
            let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
                return Err("an authenticated admin profile is required".to_owned());
            };
            Ok(Effect::StartAccessExplorer {
                question,
                policy,
                profile,
                tailnet,
                credential,
                timeout: self.resolved_config.admin.request_timeout,
            })
        });
        match result {
            Ok(effect) => {
                self.overlays.pop();
                vec![effect]
            }
            Err(error) => self.set_form_error(error),
        }
    }

    /// The preview asks the server about one selector, so the form asks for
    /// the kind and the value and nothing else.
    pub(super) fn accept_policy_preview_form(&mut self, state: &FormState) -> Vec<Effect> {
        let selector = state.value("for").trim();
        if selector.is_empty() || selector.len() > 256 || selector.chars().any(char::is_control) {
            return self.set_form_error("the selector must be non-empty, bounded, and textual");
        }
        let selector_type = if state.value("type") == "ipport" {
            PolicySelectorType::IpPort
        } else {
            PolicySelectorType::User
        };
        let selector = selector.to_owned();
        self.overlays.pop();
        self.start_policy_preview(selector_type, selector)
    }
}

impl App {
    pub(super) fn accept_operational_mutation(
        &mut self,
        action_id: ActionId,
        mutation: OperationalMutation,
        overwrite_confirmed: bool,
    ) -> Vec<Effect> {
        if matches!(
            mutation,
            OperationalMutation::SavedView(_) | OperationalMutation::Export(_)
        ) {
            self.overlays.pop();
            return self.apply_local_operational_mutation(mutation, overwrite_confirmed);
        }
        if !self.operational_mutation_available(action_id) {
            self.set_confirmation_error(
                "the operational mutation is no longer permitted by profile, scope, or read-only mode",
            );
            return Vec::new();
        }
        let Some(profile) = self.admin.profile.clone() else {
            self.set_confirmation_error("an authenticated admin profile is required");
            return Vec::new();
        };
        let Some(profile_config) = self.resolved_config.profiles.get(&profile) else {
            self.set_confirmation_error("admin profile configuration is unavailable");
            return Vec::new();
        };
        let Some(tailnet) = self.admin.tailnet.clone() else {
            self.set_confirmation_error("admin tailnet is no longer selected");
            return Vec::new();
        };
        let operation_id = self.next_operational_mutation_id;
        self.next_operational_mutation_id = self.next_operational_mutation_id.saturating_add(1);
        self.pending_operational_mutation = Some(operation_id);
        self.overlays.pop();
        vec![Effect::StartOperationalMutation {
            operation_id,
            admin_generation: self.admin_generation,
            action_id,
            mutation,
            profile,
            tailnet,
            credential: profile_config.credential.clone(),
            timeout: self.resolved_config.admin.request_timeout,
        }]
    }

    pub(super) fn apply_local_operational_mutation(
        &mut self,
        mutation: OperationalMutation,
        overwrite_confirmed: bool,
    ) -> Vec<Effect> {
        match mutation {
            OperationalMutation::SavedView(operation) => self.apply_saved_view_operation(operation),
            OperationalMutation::Export(request) => match self.build_export_document(&request) {
                Ok(document) => {
                    let format = if request.format == "csv" {
                        crate::export::ExportFormat::Csv
                    } else {
                        crate::export::ExportFormat::Json
                    };
                    match crate::export::write_atomic(
                        &document,
                        &request.path,
                        format,
                        overwrite_confirmed,
                    ) {
                        Ok(path) => {
                            self.runtime_error = Some(format!(
                                "deterministic {} export written to {}",
                                request.format,
                                path.display()
                            ));
                        }
                        Err(error) => self.runtime_error = Some(error.to_string()),
                    }
                    Vec::new()
                }
                Err(error) => {
                    self.runtime_error = Some(error);
                    Vec::new()
                }
            },
            _ => Vec::new(),
        }
    }

    pub(super) fn apply_saved_view_operation(
        &mut self,
        operation: SavedViewMutation,
    ) -> Vec<Effect> {
        let Some(saved_views) = self.saved_views.as_mut() else {
            self.runtime_error = Some("saved-view state is unavailable".to_owned());
            return Vec::new();
        };
        let result = match operation {
            SavedViewMutation::Create(view) => {
                saved_views.store.create(view, &saved_views.registry)
            }
            SavedViewMutation::Replace { name, view } => {
                saved_views
                    .store
                    .replace(&name, view, &saved_views.registry)
            }
            SavedViewMutation::Rename { name, replacement } => {
                saved_views.store.rename(&name, replacement)
            }
            SavedViewMutation::Delete { name } => saved_views.store.delete(&name),
            SavedViewMutation::Apply { name } => {
                let view = match saved_views.store.apply(&name) {
                    Ok(view) => view.clone(),
                    Err(error) => {
                        self.runtime_error = Some(error.to_string());
                        return Vec::new();
                    }
                };
                match self.apply_saved_view_to_ui(&view) {
                    Ok(()) => {
                        self.runtime_error = Some(format!("saved view {name} applied"));
                        return Vec::new();
                    }
                    Err(error) => {
                        self.runtime_error = Some(error);
                        return Vec::new();
                    }
                }
            }
        };
        match result {
            Ok(()) => self.runtime_error = Some("saved-view file updated atomically".to_owned()),
            Err(error) => self.runtime_error = Some(error.to_string()),
        }
        Vec::new()
    }

    pub(super) fn apply_saved_view_to_ui(&mut self, view: &SavedView) -> Result<(), String> {
        let route = Route::parse(&view.route)
            .filter(|route| route.label() == view.route)
            .ok_or_else(|| format!("saved view route is not canonical: {}", view.route))?;
        if route != Route::Devices
            && (view.wide_columns
                || !view.columns.is_empty()
                || !view.filters.is_empty()
                || !view.sort.is_empty())
        {
            return Err(format!(
                "saved view route {} has no active structured-view adapter",
                view.route
            ));
        }
        let same_route = self.current_route() == route;
        if same_route {
            self.capture_current_frame();
        }
        self.navigate(route);
        if route == Route::Devices {
            let terms = view
                .filters
                .iter()
                .map(saved_filter_to_term)
                .collect::<Result<Vec<_>, _>>()?;
            let filter_text = view
                .filters
                .iter()
                .map(saved_filter_to_cli)
                .collect::<Result<Vec<_>, _>>()?
                .join(" ");
            let expression = FilterExpression { terms };
            self.views.devices.filter_draft = filter_text;
            self.views.devices.applied_filter = expression;
            let sort_terms = view
                .sort
                .iter()
                .map(saved_sort_to_device)
                .collect::<Result<Vec<_>, _>>()?;
            self.views.devices.sort_terms = if sort_terms.is_empty() {
                vec![SortSpec::default()]
            } else {
                sort_terms
            };
            self.views.devices.sort = self
                .views
                .devices
                .sort_terms
                .first()
                .copied()
                .unwrap_or(SortSpec::default());
            self.views.devices.wide_columns =
                view.wide_columns || view.columns.iter().any(|column| column == "version");
            self.views.devices.columns = view.columns.clone();
            self.reconcile_selection(None);
        }
        let mut frame = self.current_view_frame();
        frame.saved_view = Some(view.name.clone());
        if same_route {
            let _ = self.view_history.append(frame);
        } else {
            self.view_history.replace_current(frame);
        }
        Ok(())
    }

    pub(super) fn export_fingerprint(&self, request: &ExportRequest) -> Result<[u8; 32], String> {
        let mut document = self.build_export_document(request)?;
        document.metadata.export_timestamp = None;
        let bytes = document
            .json_bytes_in_order()
            .map_err(|error| error.to_string())?;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hasher.finalize();
        let mut fingerprint = [0_u8; 32];
        fingerprint.copy_from_slice(&digest);
        Ok(fingerprint)
    }

    pub(super) fn build_export_document(
        &self,
        request: &ExportRequest,
    ) -> Result<crate::domain::export::ExportDocument, String> {
        use crate::domain::export::{ExportCollection, ExportMetadata, ExportRow, ExportSource};
        let source_id = self.admin.profile.as_ref().map_or_else(
            || "admin:unselected".to_owned(),
            |value| format!("admin:{value}"),
        );
        let active_filter = if request.collection == ExportCollection::Devices {
            canonical_device_filter(&self.views.devices.applied_filter)
        } else {
            "none".to_owned()
        };
        let active_sort = if request.collection == ExportCollection::Devices {
            canonical_device_sort(&self.device_sort_terms())
        } else {
            "stable_key".to_owned()
        };
        let export_route = match request.collection {
            ExportCollection::Devices => "devices",
            ExportCollection::Users => "users",
            ExportCollection::Routes => "routes",
            ExportCollection::Dns => "dns",
            ExportCollection::CredentialMetadata => "credentials",
            ExportCollection::Audit => "activity",
            ExportCollection::HealthFindings => "overview",
            ExportCollection::FlowLogs => "activity",
        };
        let metadata = |observed_at: Timestamp, complete: bool| ExportMetadata {
            schema: request.collection,
            schema_version: 1,
            tale_version: env!("CARGO_PKG_VERSION").to_owned(),
            sources: vec![ExportSource {
                id: source_id.clone(),
                observed_at,
            }],
            observed_at,
            route: export_route.to_owned(),
            active_filter: active_filter.clone(),
            active_sort: active_sort.clone(),
            truncated: false,
            complete,
            export_timestamp: format_export_timestamp(self.now),
        };
        let (observed_at, complete, rows) =
            match request.collection {
                ExportCollection::Devices => {
                    if self.devices_resource.observed_at.is_none()
                        && self.devices_resource.snapshot.is_empty()
                        && self.devices_resource.health != SourceHealth::Healthy
                    {
                        return Err("device collection is not currently observed".to_owned());
                    }
                    let observed_at = self.devices_resource.observed_at.unwrap_or(self.now);
                    let rows = self
                        .visible_indices()
                        .into_iter()
                        .filter_map(|index| self.devices_resource.snapshot.get(index))
                        .map(|device| ExportRow::Device {
                            id: device.id.0.clone(),
                            name: device.display_name.clone(),
                            addresses: sorted_strings(&device.addresses),
                            source: source_id.clone(),
                            observed_at,
                        })
                        .collect();
                    (
                        observed_at,
                        self.devices_resource.health == SourceHealth::Healthy,
                        rows,
                    )
                }
                ExportCollection::Users => {
                    let values =
                        self.admin.users.snapshot.as_ref().ok_or_else(|| {
                            "user collection is not currently observed".to_owned()
                        })?;
                    let observed_at = self.admin.users.observed_at.unwrap_or(self.now);
                    let rows = values
                        .iter()
                        .map(|user| ExportRow::User {
                            id: user.id.clone(),
                            name: user.label().to_owned(),
                            role: user
                                .role
                                .clone()
                                .unwrap_or_else(|| "not returned".to_owned()),
                            source: source_id.clone(),
                            observed_at,
                        })
                        .collect();
                    (
                        observed_at,
                        self.admin.users.state == AdminResourceState::Ready,
                        rows,
                    )
                }
                ExportCollection::Routes => {
                    let observations = self.admin.route_observations();
                    let observed_at = self.admin.routes.observed_at.unwrap_or(self.now);
                    let rows = observations
                        .iter()
                        .flat_map(|observation| {
                            observation.advertised.iter().map(|cidr| ExportRow::Route {
                                id: format!("{}:{cidr}", observation.device_id),
                                cidr: cidr
                                    .parse::<crate::domain::route::IpNet>()
                                    .map_or_else(|_| cidr.clone(), |value| value.to_string()),
                                advertiser: observation.device_id.clone(),
                                approval: if observation.enabled.iter().any(|value| value == cidr) {
                                    "approved".to_owned()
                                } else {
                                    "not approved".to_owned()
                                },
                                source: source_id.clone(),
                                observed_at: observation.observed_at,
                            })
                        })
                        .collect();
                    (
                        observed_at,
                        self.admin.routes.state == AdminResourceState::Ready,
                        rows,
                    )
                }
                ExportCollection::Dns => {
                    let values = self
                        .admin
                        .nameservers
                        .snapshot
                        .as_ref()
                        .ok_or_else(|| "DNS collection is not currently observed".to_owned())?;
                    let observed_at = self.admin.nameservers.observed_at.unwrap_or(self.now);
                    let sorted_values = sorted_strings(&values.values);
                    let rows = sorted_values
                        .iter()
                        .enumerate()
                        .map(|(index, value)| ExportRow::Dns {
                            name: format!("nameserver-{index}"),
                            value: value.clone(),
                            source: source_id.clone(),
                            observed_at,
                        })
                        .collect();
                    (
                        observed_at,
                        self.admin.nameservers.state == AdminResourceState::Ready,
                        rows,
                    )
                }
                ExportCollection::CredentialMetadata => {
                    let values = self.admin.credentials.snapshot.as_ref().ok_or_else(|| {
                        "credential metadata is not currently observed".to_owned()
                    })?;
                    let rows = values
                        .records
                        .iter()
                        .map(|record| ExportRow::CredentialMetadata {
                            id: record.id.clone(),
                            credential_type: record.key_type.clone(),
                            status: credential_status(record, self.now),
                            created_at: record.created_at,
                            expires_at: record.expires_at,
                            source: source_id.clone(),
                            observed_at: values.observed_at,
                        })
                        .collect();
                    (values.observed_at, !values.partial, rows)
                }
                ExportCollection::Audit => {
                    let values =
                        self.admin.activity.snapshot.as_ref().ok_or_else(|| {
                            "audit collection is not currently observed".to_owned()
                        })?;
                    let rows = values
                        .events
                        .iter()
                        .filter(|event| self.audit_filters.matches(event))
                        .map(|event| ExportRow::Audit {
                            event_id: audit_export_id(event),
                            event_time: format_export_timestamp(event.event_time)
                                .unwrap_or_else(|| event.event_time_text.clone()),
                            action: event
                                .action
                                .clone()
                                .unwrap_or_else(|| "not returned".to_owned()),
                            actor: event.actor.as_ref().map_or_else(
                                || "not returned".to_owned(),
                                |actor| {
                                    actor
                                        .id
                                        .clone()
                                        .or(actor.display.clone())
                                        .unwrap_or_else(|| "not returned".to_owned())
                                },
                            ),
                            target: event.target.as_ref().map_or_else(
                                || "not returned".to_owned(),
                                |target| {
                                    target
                                        .id
                                        .clone()
                                        .or(target.display.clone())
                                        .unwrap_or_else(|| "not returned".to_owned())
                                },
                            ),
                            source: source_id.clone(),
                            observed_at: values.observed_at,
                        })
                        .collect();
                    (values.observed_at, !values.delayed, rows)
                }
                ExportCollection::HealthFindings => {
                    let observed_at = self
                        .health
                        .snapshot
                        .as_ref()
                        .map_or(0, |snapshot| snapshot.now);
                    let rows = self
                        .health_findings
                        .iter()
                        .map(|finding| ExportRow::HealthFinding {
                            id: finding.id.clone(),
                            rule_id: finding.rule_id.clone(),
                            severity: finding.severity.label().to_owned(),
                            title: finding.title.clone(),
                            affected_resource_ids: finding.affected_resource_ids.clone(),
                            source_ids: finding.source_ids.clone(),
                            derived: finding.derived,
                            observed_at: finding.observed_at,
                        })
                        .collect();
                    (observed_at, self.health.snapshot.is_some(), rows)
                }
                ExportCollection::FlowLogs => {
                    let snapshot = self
                        .flow_snapshot
                        .as_ref()
                        .ok_or_else(|| "no bounded flow window is currently observed".to_owned())?;
                    let rows = snapshot
                        .messages
                        .iter()
                        .flat_map(|message| {
                            message
                                .records()
                                .filter(|record| snapshot.filter.matches(record))
                                .map(|record| {
                                    let source = record.connection.canonical_src();
                                    let destination = record.connection.canonical_dst();
                                    ExportRow::FlowLog {
                                        reporting_node: record.node_id.to_owned(),
                                        logged: canonical_wire_timestamp(record.logged),
                                        start: canonical_wire_timestamp(record.start),
                                        end: canonical_wire_timestamp(record.end),
                                        traffic_class: record.class.label().to_owned(),
                                        protocol: record.connection.proto.clone(),
                                        source,
                                        destination,
                                        tx_packets: record.connection.tx_packets,
                                        tx_bytes: record.connection.tx_bytes,
                                        rx_packets: record.connection.rx_packets,
                                        rx_bytes: record.connection.rx_bytes,
                                    }
                                })
                        })
                        .collect();
                    (snapshot.observed_at, snapshot.complete, rows)
                }
            };
        let mut document = crate::domain::export::ExportDocument {
            metadata: metadata(observed_at, complete),
            rows,
        };
        if request.collection != ExportCollection::Devices {
            document.sort_rows();
        }
        Ok(document)
    }
}

impl App {}
