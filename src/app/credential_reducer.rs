use super::*;

impl App {
    pub(super) fn open_auth_key_form_with_request(
        &mut self,
        request: crate::admin::key_mutations::AuthKeyCreateRequest,
    ) -> Vec<Effect> {
        if let Err(error) = request.validate() {
            self.runtime_error = Some(error.to_string());
            return Vec::new();
        }
        self.pending_auth_key_request = Some(request.clone());
        let expiry_days = request.expiry_seconds / (24 * 60 * 60);
        let tags = request.tags.join(",");
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: ActionId::AdminCredentialAuthKeyCreate,
                admin_generation: self.admin_generation,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: None,
                handoff: None,
                prompt:
                    "Create this auth key? The secret will be shown once and cannot be recovered."
                        .to_owned(),
                required_phrase: Some("CREATE AUTH KEY".to_owned()),
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines: vec![
                    format!(
                        "profile: {}",
                        self.admin.profile.as_deref().unwrap_or("not selected")
                    ),
                    format!(
                        "tailnet: {}",
                        self.admin.tailnet.as_deref().unwrap_or("not selected")
                    ),
                    "endpoint: POST /tailnet/{tailnet}/keys".to_owned(),
                    "scope: auth_keys".to_owned(),
                    "type: auth".to_owned(),
                    format!(
                        "description: {}",
                        request.description.as_deref().unwrap_or("none")
                    ),
                    format!("expiry: {expiry_days} days"),
                    format!("reusable: {}", request.reusable),
                    format!("ephemeral: {}", request.ephemeral),
                    format!("preauthorized: {}", request.preauthorized),
                    format!(
                        "tags: {}",
                        if tags.is_empty() {
                            "none"
                        } else {
                            tags.as_str()
                        }
                    ),
                    format!(
                        "expires at: {}",
                        self.now.saturating_add(request.expiry_seconds)
                    ),
                ],
                redacted_argv: Vec::new(),
                error: None,
            })));
        Vec::new()
    }

    pub(super) fn selected_credential(
        &self,
    ) -> Option<&crate::domain::credential::CredentialMetadata> {
        self.filtered_admin_credentials()
            .get(self.admin_credential_selected)
            .copied()
    }

    pub(super) fn open_credential_revoke_confirmation(&mut self) -> Vec<Effect> {
        let Some(credential) = self.selected_credential() else {
            self.runtime_error = Some("select a credential before revoking it".to_owned());
            return Vec::new();
        };
        let credential_type = crate::admin::key_mutations::remote_credential_type(credential);
        if !credential_type.supported_for_revoke() {
            self.runtime_error = Some("Tale cannot revoke this type of credential".to_owned());
            return Vec::new();
        }
        let Some(read_scope) = credential_type.read_scope() else {
            self.runtime_error = Some("the selected credential read scope is unknown".to_owned());
            return Vec::new();
        };
        let Some(write_scope) = credential_type.write_scope() else {
            self.runtime_error = Some("the selected credential write scope is unknown".to_owned());
            return Vec::new();
        };
        if !self.admin_scope_allowed(read_scope) || !self.admin_scope_allowed(write_scope) {
            self.runtime_error = Some(format!(
                "revocation requires the selected credential's {read_scope} and {write_scope} scopes"
            ));
            return Vec::new();
        }
        let key_id = credential.id.clone();
        let Some((profile, tailnet, credential_reference)) = self.admin_policy_context() else {
            self.runtime_error = Some("an authenticated admin profile is required".to_owned());
            return Vec::new();
        };
        self.pending_credential_revoke = Some(key_id.clone());
        vec![Effect::StartCredentialDetail {
            key_id,
            profile,
            tailnet,
            credential: credential_reference,
            timeout: self.resolved_config.admin.request_timeout,
        }]
    }

    pub(super) fn open_credential_revoke_with_metadata(
        &mut self,
        credential: crate::domain::credential::CredentialMetadata,
    ) -> Vec<Effect> {
        let credential_type = crate::admin::key_mutations::remote_credential_type(&credential);
        if !credential_type.supported_for_revoke() {
            self.runtime_error = Some("Tale cannot revoke this type of credential".to_owned());
            return Vec::new();
        }
        let Some(read_scope) = credential_type.read_scope() else {
            self.runtime_error = Some("the selected credential read scope is unknown".to_owned());
            return Vec::new();
        };
        let Some(write_scope) = credential_type.write_scope() else {
            self.runtime_error = Some("the selected credential write scope is unknown".to_owned());
            return Vec::new();
        };
        if !self.admin_scope_allowed(read_scope) || !self.admin_scope_allowed(write_scope) {
            self.runtime_error = Some(format!(
                "revocation requires the selected credential's {read_scope} and {write_scope} scopes"
            ));
            return Vec::new();
        }
        if credential.invalid == Some(true) || credential.revoked_at.is_some() {
            self.runtime_error = Some("the credential is already invalid or revoked".to_owned());
            return Vec::new();
        }
        let phrase = format!("REVOKE {}", credential.id);
        self.pending_credential_revoke = Some(credential.id.clone());
        let display_list = |values: &[String]| {
            if values.is_empty() {
                "none returned".to_owned()
            } else {
                values.join(",")
            }
        };
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
            action_id: ActionId::AdminCredentialRevoke,
            admin_generation: self.admin_generation,
            mutation: None,
            admin_mutation: None,
            admin_batch: None,
            service_request: None,
            operational_mutation: None,
            handoff: None,
            prompt: "Revoke this credential? Tale will send the request once, then check that it is gone."
                .to_owned(),
            required_phrase: Some(phrase),
            input: String::new(),
            lose_ssh_checked: false,
            preview_lines: vec![
                format!("id: {}", credential.id),
                format!("type: {}", credential.key_type),
                format!(
                    "description: {}",
                    credential
                        .description
                        .as_deref()
                        .map_or("not returned", |value| value)
                ),
                format!(
                    "owner: {}",
                    credential
                        .user_id
                        .as_deref()
                        .map_or("not returned", |value| value)
                ),
                format!(
                    "created: {}",
                    credential
                        .created_at
                        .map_or_else(|| "not returned".to_owned(), |value| value.to_string())
                ),
                format!(
                    "expires: {}",
                    credential
                        .expires_at
                        .map_or_else(|| "not returned".to_owned(), |value| value.to_string())
                ),
                format!(
                    "last used: {}",
                    credential
                        .last_used_at
                        .map_or_else(|| "not returned".to_owned(), |value| value.to_string())
                ),
                format!("scopes: {}", display_list(&credential.scopes)),
                format!("tags: {}", display_list(&credential.tags)),
                format!(
                    "known dependents: {}",
                    display_list(&credential.known_dependents)
                ),
                "Saved credential references cannot be matched reliably. If this credential belongs to the active profile, Tale will disconnect it after revocation."
                    .to_owned(),
                "Revoking this credential does not remove a saved credential from the OS keyring."
                    .to_owned(),
            ],
            redacted_argv: Vec::new(),
            error: None,
        })));
        Vec::new()
    }

    pub(super) fn open_profile_credential_confirmation(&mut self) -> Vec<Effect> {
        let Some(profile) = self.admin.profile.clone() else {
            self.runtime_error = Some("an active profile is required".to_owned());
            return Vec::new();
        };
        let Some(configuration) = self.resolved_config.profiles.get(&profile) else {
            self.runtime_error = Some("the active profile configuration is unavailable".to_owned());
            return Vec::new();
        };
        self.overlays.push(Overlay::Confirmation(Box::new(ConfirmationState {
            action_id: ActionId::ProfileCredentialRemove,
            admin_generation: self.admin_generation,
            mutation: None,
            admin_mutation: None,
            admin_batch: None,
            service_request: None,
            operational_mutation: None,
            handoff: None,
            prompt: "Remove this local Tale credential from the OS keyring? This does not revoke any remote credential.".to_owned(),
            required_phrase: Some("REMOVE LOCAL CREDENTIAL".to_owned()),
            input: String::new(),
            lose_ssh_checked: false,
            preview_lines: vec![format!("profile: {profile}"), format!("keyring reference: {}", configuration.credential)],
            redacted_argv: Vec::new(),
            error: None,
        })));
        Vec::new()
    }

    pub(super) fn open_audit_investigation(&mut self) -> Vec<Effect> {
        self.overlays.push(Overlay::AuditInvestigation);
        Vec::new()
    }

    pub(super) fn open_audit_filter(&mut self, action_id: ActionId) -> Vec<Effect> {
        let filters = &self.audit_filters;
        let (title, fields) = match action_id {
            ActionId::AuditFilterTime => (
                "Limit the audit log to a time range",
                vec![
                    FormField::text(
                        "start",
                        "From",
                        "Inclusive UTC start, as 2026-08-03T00:00:00Z; empty removes the bound",
                        "any time",
                        filters.start.map_or(String::new(), format_audit_timestamp),
                    ),
                    FormField::text(
                        "end",
                        "To",
                        "Inclusive UTC end, as 2026-08-04T00:00:00Z; empty removes the bound",
                        "any time",
                        filters.end.map_or(String::new(), format_audit_timestamp),
                    ),
                ],
            ),
            ActionId::AuditFilterActor => (
                "Limit the audit log to one actor",
                vec![
                    FormField::text(
                        "id",
                        "Actor id",
                        "The exact user or principal id recorded on the entry",
                        "any actor",
                        filters.actor_id.clone().unwrap_or_default(),
                    ),
                    FormField::text(
                        "display",
                        "Shown as",
                        "The exact display value the entry resolved to",
                        "any name",
                        filters.actor_display.clone().unwrap_or_default(),
                    ),
                ],
            ),
            ActionId::AuditFilterAction => (
                "Limit the audit log to one action",
                vec![FormField::text(
                    "action",
                    "Action",
                    "The exact action value, such as device.view",
                    "any action",
                    filters.action.clone().unwrap_or_default(),
                )],
            ),
            ActionId::AuditFilterTarget => (
                "Limit the audit log to one target",
                vec![
                    FormField::options(
                        "type",
                        "Kind",
                        "What sort of thing the entry acted on",
                        AUDIT_TARGET_KINDS,
                        filters
                            .target_type
                            .clone()
                            .unwrap_or_else(|| ANY.to_owned()),
                    ),
                    FormField::text(
                        "id",
                        "Target id",
                        "The exact stable id the entry recorded",
                        "any id",
                        filters.target_id.clone().unwrap_or_default(),
                    ),
                    FormField::text(
                        "text",
                        "Summary contains",
                        "Matches entries whose summary contains this text",
                        "anything",
                        filters.text.clone().unwrap_or_default(),
                    ),
                ],
            ),
            _ => return Vec::new(),
        };
        self.push_form(action_id, title, Vec::new(), fields);
        Vec::new()
    }

    pub(super) fn accept_audit_filter(&mut self, state: &FormState) -> Vec<Effect> {
        match state.action_id {
            ActionId::AuditFilterTime => {
                let start = match audit_time(state.value("start")) {
                    Ok(value) => value,
                    Err(error) => return self.set_form_error(error),
                };
                let end = match audit_time(state.value("end")) {
                    Ok(value) => value,
                    Err(error) => return self.set_form_error(error),
                };
                if start.zip(end).is_some_and(|(start, end)| start > end) {
                    return self.set_form_error("the start must not be after the end");
                }
                self.audit_filters.start = start;
                self.audit_filters.end = end;
            }
            ActionId::AuditFilterActor => {
                self.audit_filters.actor_id = audit_text(state.value("id"));
                self.audit_filters.actor_display = audit_text(state.value("display"));
            }
            ActionId::AuditFilterAction => {
                self.audit_filters.action = audit_text(state.value("action"));
            }
            ActionId::AuditFilterTarget => {
                self.audit_filters.target_type = match state.value("type") {
                    ANY => None,
                    value => Some(value.to_owned()),
                };
                self.audit_filters.target_id = audit_text(state.value("id"));
                self.audit_filters.text = audit_text(state.value("text"));
            }
            _ => return Vec::new(),
        }
        self.admin_activity_selected = 0;
        self.overlays.pop();
        self.open_audit_investigation()
    }

    pub(super) fn copy_secret_result(&mut self) -> Vec<Effect> {
        let Some(result) = self.secret_result.as_mut() else {
            self.runtime_error = Some("no one-time secret is open".to_owned());
            return Vec::new();
        };
        let result_id = result.metadata().result_id;
        let Some(secret) = result.mark_copy_requested() else {
            self.runtime_error = Some("the one-time secret has already been closed".to_owned());
            return Vec::new();
        };
        vec![Effect::CopySecret { result_id, secret }]
    }

    pub(super) fn close_secret_result(&mut self) -> Vec<Effect> {
        if let Some(result) = self.secret_result.as_mut() {
            result.close();
        }
        self.secret_result = None;
        self.overlays
            .retain(|overlay| !matches!(overlay, Overlay::SecretResult));
        if self.admin.profile.is_some() {
            self.start_admin_resource_refresh(vec![AdminRefreshResource::Credentials])
        } else {
            Vec::new()
        }
    }
}
