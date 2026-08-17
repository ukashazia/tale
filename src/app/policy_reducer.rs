use super::*;

impl App {
    pub(super) fn admin_policy_context(&self) -> Option<(String, String, String)> {
        let profile = self.admin.profile.clone()?;
        let tailnet = self.admin.tailnet.clone()?;
        let credential = self
            .resolved_config
            .profiles
            .get(&profile)?
            .credential
            .clone();
        Some((profile, tailnet, credential))
    }

    pub(super) fn open_policy_workflow(&mut self) -> Vec<Effect> {
        if self.policy_workflow.is_some() {
            return self.reopen_policy_editor();
        }
        if let Err(error) = crate::terminal::EditorCommand::from_environment() {
            self.runtime_error = Some(error.to_string());
            return Vec::new();
        }
        if self.source_mode == SourceMode::Mock {
            let Some(snapshot) = self.admin.policy.snapshot.as_ref() else {
                self.runtime_error = Some("the mock policy source is unavailable".to_owned());
                return Vec::new();
            };
            let document =
                match crate::domain::policy_workflow::PolicyDocument::from_bytes_with_content_type(
                    snapshot.source_bytes.clone(),
                    snapshot.content_type.clone(),
                    self.now,
                ) {
                    Ok(document) => document,
                    Err(error) => {
                        self.runtime_error = Some(error.to_string());
                        return Vec::new();
                    }
                };
            let file = match crate::temporary::TemporaryPolicyFile::create(document.bytes()) {
                Ok(file) => file,
                Err(error) => {
                    self.runtime_error = Some(error.to_string());
                    return Vec::new();
                }
            };
            let workflow_id = self.next_policy_workflow_id;
            self.next_policy_workflow_id = self.next_policy_workflow_id.saturating_add(1);
            let path = file.path().to_path_buf();
            self.policy_temp_file = Some(Arc::new(Mutex::new(file)));
            let mut workflow = PolicyWorkflow::opening(
                workflow_id,
                "mock".to_owned(),
                "example.test".to_owned(),
                self.now,
            );
            workflow.set_base(document.clone());
            workflow.set_candidate(document, path);
            self.policy_workflow = Some(workflow);
            return self.start_policy_editor();
        }
        let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
            self.runtime_error = Some("an authenticated admin profile is required".to_owned());
            return Vec::new();
        };
        let workflow_id = self.next_policy_workflow_id;
        self.next_policy_workflow_id = self.next_policy_workflow_id.saturating_add(1);
        self.policy_workflow = Some(PolicyWorkflow::opening(
            workflow_id,
            profile.clone(),
            tailnet.clone(),
            self.now,
        ));
        vec![Effect::StartPolicyRemoteFetch {
            workflow_id,
            profile,
            tailnet,
            credential,
            timeout: self.resolved_config.admin.request_timeout,
        }]
    }

    pub(super) fn refresh_policy_workflow(&mut self) -> Vec<Effect> {
        self.policy_workflow_view = PolicyWorkflowView::Actions;
        if self.source_mode == SourceMode::Mock {
            let latest = self.admin.policy.snapshot.as_ref().and_then(|snapshot| {
                crate::domain::policy_workflow::PolicyDocument::from_bytes_with_content_type(
                    snapshot.source_bytes.clone(),
                    snapshot.content_type.clone(),
                    self.now,
                )
                .ok()
            });
            if let Some(latest) = latest
                && let Some(workflow) = self.policy_workflow.as_mut()
            {
                workflow.set_latest_remote(latest);
            }
            self.runtime_error = Some("mock remote policy refreshed".to_owned());
            return Vec::new();
        }
        let Some(workflow) = self.policy_workflow.as_ref() else {
            return self.open_policy_workflow();
        };
        let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
            self.runtime_error = Some("an authenticated admin profile is required".to_owned());
            return Vec::new();
        };
        vec![Effect::StartPolicyRemoteFetch {
            workflow_id: workflow.workflow_id(),
            profile,
            tailnet,
            credential,
            timeout: self.resolved_config.admin.request_timeout,
        }]
    }

    pub(super) fn start_policy_editor(&mut self) -> Vec<Effect> {
        let Some(workflow) = self.policy_workflow.as_ref() else {
            return Vec::new();
        };
        let Some(path) = workflow.candidate_path().map(PathBuf::from) else {
            self.runtime_error = Some("the policy temporary file is unavailable".to_owned());
            return Vec::new();
        };
        let command = match crate::terminal::EditorCommand::from_environment() {
            Ok(command) => command,
            Err(error) => {
                self.runtime_error = Some(error.to_string());
                if let Some(workflow) = self.policy_workflow.as_mut() {
                    workflow.retain_failure();
                }
                return Vec::new();
            }
        };
        let workflow_id = workflow.workflow_id();
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.mark_editing_externally();
        }
        self.interactive_handoff_active = true;
        vec![Effect::StartPolicyEditor {
            workflow_id,
            command,
            path,
        }]
    }

    pub(super) fn reopen_policy_editor(&mut self) -> Vec<Effect> {
        if self.policy_workflow.is_none() {
            return self.open_policy_workflow();
        }
        if self
            .policy_workflow
            .as_ref()
            .is_some_and(|workflow| workflow.state() == PolicyState::Opening)
        {
            self.runtime_error = Some("the policy source is still loading".to_owned());
            return Vec::new();
        }
        self.start_policy_editor()
    }

    pub(super) fn discard_policy_candidate(&mut self) -> Vec<Effect> {
        self.close_policy_workflow()
    }

    pub(super) fn validate_policy_candidate(&mut self) -> Vec<Effect> {
        self.policy_workflow_view = PolicyWorkflowView::Validation;
        if self.source_mode == SourceMode::Mock {
            let Some(workflow) = self.policy_workflow.as_mut() else {
                return Vec::new();
            };
            let Some(candidate_hash) = workflow.candidate().map(|value| value.hash().to_owned())
            else {
                return Vec::new();
            };
            let _ = workflow.set_validation(crate::domain::policy_workflow::PolicyValidation {
                candidate_hash,
                validated_at: self.now,
                valid: true,
                message: Some("mock server validation passed".to_owned()),
                bounded_safe_detail: None,
                diagnostics: Vec::new(),
                server_tests: Vec::new(),
                observed_at: self.now,
            });
            return Vec::new();
        }
        let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
            self.runtime_error = Some("an authenticated admin profile is required".to_owned());
            return Vec::new();
        };
        if !self.sync_policy_candidate_file() {
            return Vec::new();
        }
        let Some(workflow) = self.policy_workflow.as_ref() else {
            return Vec::new();
        };
        let Some(path) = workflow.candidate_path().map(PathBuf::from) else {
            self.runtime_error = Some("the policy candidate is unavailable".to_owned());
            return Vec::new();
        };
        let workflow_id = workflow.workflow_id();
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.mark_validating();
        }
        vec![Effect::StartPolicyValidate {
            workflow_id,
            profile,
            tailnet,
            credential,
            timeout: self.resolved_config.admin.request_timeout,
            path,
        }]
    }

    pub(super) fn preview_policy_candidate(&mut self) -> Vec<Effect> {
        self.policy_workflow_view = PolicyWorkflowView::Preview;
        let selector = self
            .selected_admin_user()
            .map_or_else(|| "autogroup:members".to_owned(), |user| user.id.clone());
        self.push_form(
            ActionId::AdminPolicyPreview,
            "Preview the policy for one selector",
            Vec::new(),
            vec![
                FormField::options(
                    "type",
                    "Selector",
                    "Whether the preview is asked for a user or an address and port",
                    &["user", "ipport"],
                    "user",
                ),
                FormField::text(
                    "for",
                    "Preview for",
                    "The user selector, or address:port, the server previews access for",
                    "autogroup:members",
                    selector,
                ),
            ],
        );
        Vec::new()
    }

    pub(super) fn start_policy_preview(
        &mut self,
        selector_type: PolicySelectorType,
        selector: String,
    ) -> Vec<Effect> {
        if self.source_mode == SourceMode::Mock {
            let Some(workflow) = self.policy_workflow.as_mut() else {
                return Vec::new();
            };
            let Some(candidate_hash) = workflow.candidate().map(|value| value.hash().to_owned())
            else {
                return Vec::new();
            };
            let _ = workflow.set_preview(crate::domain::policy_workflow::PolicyPreview {
                candidate_hash,
                selector_type,
                selector,
                matches: vec![crate::domain::policy_workflow::PolicyPreviewMatch {
                    users: vec!["alice@example.test".to_owned()],
                    ports: vec!["tag:server:22".to_owned()],
                    line_number: Some(4),
                }],
                observed_at: self.now,
            });
            self.policy_workflow_view = PolicyWorkflowView::Preview;
            return Vec::new();
        }
        let Some((profile, tailnet, credential)) = self.admin_policy_context() else {
            self.runtime_error = Some("an authenticated admin profile is required".to_owned());
            return Vec::new();
        };
        if !self.sync_policy_candidate_file() {
            return Vec::new();
        }
        let Some(workflow) = self.policy_workflow.as_ref() else {
            return Vec::new();
        };
        let Some(path) = workflow.candidate_path().map(PathBuf::from) else {
            self.runtime_error = Some("the policy candidate is unavailable".to_owned());
            return Vec::new();
        };
        let workflow_id = workflow.workflow_id();
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.mark_previewing();
        }
        vec![Effect::StartPolicyPreview {
            workflow_id,
            profile,
            tailnet,
            credential,
            timeout: self.resolved_config.admin.request_timeout,
            path,
            selector_type,
            selector,
        }]
    }

    pub(super) fn diff_policy_candidate(&mut self) -> Vec<Effect> {
        self.policy_workflow_view = PolicyWorkflowView::Diff;
        if !self.sync_policy_candidate_file() {
            return Vec::new();
        }
        let Some(workflow) = self.policy_workflow.as_mut() else {
            return Vec::new();
        };
        let Some((base, candidate)) = workflow.base().zip(workflow.candidate()) else {
            self.runtime_error = Some("both policy base and candidate are required".to_owned());
            return Vec::new();
        };
        match crate::admin::policy_mutations::build_policy_diff(base, candidate) {
            Ok(diff) => {
                let _ = workflow.set_diff(diff);
            }
            Err(error) => self.runtime_error = Some(error.to_string()),
        }
        Vec::new()
    }

    pub(super) fn open_policy_apply_confirmation(&mut self) -> Vec<Effect> {
        if !self.sync_policy_candidate_file() {
            return Vec::new();
        }
        let Some(workflow) = self.policy_workflow.as_ref() else {
            return Vec::new();
        };
        if let Err(error) = workflow.apply_guard(self.now) {
            self.runtime_error = Some(error.to_string());
            return Vec::new();
        }
        let Some(candidate) = workflow.candidate() else {
            return Vec::new();
        };
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: ActionId::AdminPolicyApply,
                admin_generation: self.admin_generation,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: None,
                handoff: None,
                prompt: "Apply this exact policy candidate to the remote tailnet?".to_owned(),
                required_phrase: Some("APPLY POLICY".to_owned()),
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines: vec![
                    format!(
                        "base hash: {}",
                        workflow.base().map_or("not returned", |value| value.hash())
                    ),
                    format!("candidate hash: {}", candidate.hash()),
                    format!(
                        "base observed: {}",
                        workflow
                            .base()
                            .map_or("not returned".to_owned(), |value| value
                                .observed_at()
                                .to_string())
                    ),
                    format!("candidate observed: {}", candidate.observed_at()),
                    format!("candidate bytes: {}", candidate.len()),
                    format!("validation bound: {}", workflow.validation().is_some()),
                    format!(
                        "validation/tests: {}",
                        workflow.validation().map_or_else(
                            || "not returned".to_owned(),
                            |value| if value.valid {
                                "server passed".to_owned()
                            } else {
                                "server failed".to_owned()
                            }
                        )
                    ),
                    format!("permission preview bound: {}", workflow.preview().is_some()),
                    format!(
                        "diff: {}",
                        workflow.diff().map_or_else(
                            || "not computed; press d for the complete textual diff".to_owned(),
                            |value| format!(
                                "+{} -{}; press d for the complete textual diff",
                                value.additions, value.removals
                            )
                        )
                    ),
                    "final server validation runs immediately before one save request".to_owned(),
                    "remote bytes are fetched and compared after save".to_owned(),
                    "the final hash check is not a server-atomic compare-and-swap".to_owned(),
                ],
                redacted_argv: Vec::new(),
                error: None,
            })));
        Vec::new()
    }

    pub(super) fn sync_policy_candidate_file(&mut self) -> bool {
        let Some((path, expected_hash, content_type)) =
            self.policy_workflow.as_ref().and_then(|workflow| {
                workflow
                    .candidate()
                    .zip(workflow.candidate_path())
                    .map(|(candidate, path)| {
                        (
                            path.to_path_buf(),
                            candidate.hash().to_owned(),
                            candidate.content_type().to_owned(),
                        )
                    })
            })
        else {
            return true;
        };
        let bytes = match crate::temporary::TemporaryPolicyFile::read_candidate_path(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                if let Some(workflow) = self.policy_workflow.as_mut() {
                    workflow.retain_failure();
                }
                self.runtime_error = Some(error.to_string());
                return false;
            }
        };
        if crate::domain::policy_workflow::hash_bytes(&bytes) == expected_hash {
            return true;
        }
        let document =
            match crate::domain::policy_workflow::PolicyDocument::from_bytes_with_content_type(
                bytes,
                content_type,
                self.now,
            ) {
                Ok(document) => document,
                Err(error) => {
                    if let Some(workflow) = self.policy_workflow.as_mut() {
                        workflow.retain_failure();
                    }
                    self.runtime_error = Some(error.to_string());
                    return false;
                }
            };
        self.access_explorer_result = None;
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.set_candidate(document, path);
        }
        self.runtime_error = Some(
            "the temporary candidate changed; validation, preview, and diff were invalidated"
                .to_owned(),
        );
        false
    }

    pub(super) fn open_policy_discard_confirmation(&mut self) -> Vec<Effect> {
        let Some(workflow) = self.policy_workflow.as_ref() else {
            self.runtime_error = Some("the policy workflow is not open".to_owned());
            return Vec::new();
        };
        let replacing_remote =
            workflow.latest_remote().is_some() && workflow.state() == PolicyState::RemoteConflict;
        let phrase = if replacing_remote {
            "REPLACE POLICY CANDIDATE"
        } else {
            "DISCARD POLICY CANDIDATE"
        };
        let mut preview_lines = vec![
            format!(
                "base hash: {}",
                workflow.base().map_or("not returned", |value| value.hash())
            ),
            format!(
                "candidate hash: {}",
                workflow
                    .candidate()
                    .map_or("not returned", |value| value.hash())
            ),
            format!(
                "candidate path: {}",
                workflow
                    .candidate_path()
                    .map_or("not retained".to_owned(), |value| value
                        .display()
                        .to_string())
            ),
        ];
        if replacing_remote {
            preview_lines.extend([
                format!(
                    "latest remote hash: {}",
                    workflow
                        .latest_remote()
                        .map_or("not returned", |value| value.hash())
                ),
                format!(
                    "latest remote path: {}",
                    workflow
                        .latest_remote_path()
                        .map_or("not retained".to_owned(), |value| value
                            .display()
                            .to_string())
                ),
                "replace candidate with latest remote bytes; no merge will be attempted".to_owned(),
            ]);
        } else {
            preview_lines
                .push("the candidate will be replaced with the unchanged base bytes".to_owned());
        }
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: ActionId::AdminPolicyCandidateDiscard,
                admin_generation: self.admin_generation,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: None,
                handoff: None,
                prompt: if replacing_remote {
                    "Replace the retained candidate with the latest remote policy?".to_owned()
                } else {
                    "Discard the retained policy candidate?".to_owned()
                },
                required_phrase: Some(phrase.to_owned()),
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines,
                redacted_argv: Vec::new(),
                error: None,
            })));
        Vec::new()
    }

    pub(super) fn open_policy_close_confirmation(&mut self) -> Vec<Effect> {
        let Some(workflow) = self.policy_workflow.as_ref() else {
            return Vec::new();
        };
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id: ActionId::AdminPolicyWorkflowClose,
                admin_generation: self.admin_generation,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: None,
                handoff: None,
                prompt: "Close the policy workflow and remove its temporary files?".to_owned(),
                required_phrase: Some("CLOSE POLICY WORKFLOW".to_owned()),
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines: vec![
                    format!("state: {}", workflow.state().label()),
                    format!(
                        "candidate path: {}",
                        workflow
                            .candidate_path()
                            .map_or("not retained".to_owned(), |value| value
                                .display()
                                .to_string())
                    ),
                    "closing destroys the candidate and any retained latest-remote copy".to_owned(),
                ],
                redacted_argv: Vec::new(),
                error: None,
            })));
        Vec::new()
    }

    pub(super) fn replace_policy_candidate_with_latest(&mut self) -> Vec<Effect> {
        let Some(latest) = self
            .policy_workflow
            .as_ref()
            .and_then(PolicyWorkflow::latest_remote)
            .cloned()
        else {
            self.runtime_error = Some("the latest remote policy is unavailable".to_owned());
            return Vec::new();
        };
        self.close_policy_temp_file();
        let file = match crate::temporary::TemporaryPolicyFile::create(latest.bytes()) {
            Ok(file) => file,
            Err(error) => {
                self.runtime_error = Some(error.to_string());
                return Vec::new();
            }
        };
        let path = file.path().to_path_buf();
        self.policy_temp_file = Some(Arc::new(Mutex::new(file)));
        self.close_latest_policy_temp_file();
        self.access_explorer_result = None;
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.set_base(latest.clone());
            workflow.set_candidate(latest, path);
        }
        Vec::new()
    }

    pub(super) fn close_policy_workflow(&mut self) -> Vec<Effect> {
        self.close_policy_temp_file();
        self.close_latest_policy_temp_file();
        if let Some(workflow) = self.policy_workflow.as_mut() {
            workflow.close();
        }
        self.policy_workflow = None;
        self.policy_workflow_view = PolicyWorkflowView::Actions;
        self.pending_auth_key_request = None;
        self.pending_credential_revoke = None;
        Vec::new()
    }

    pub(super) fn close_policy_temp_file(&mut self) {
        if let Some(file) = self.policy_temp_file.take() {
            match file.lock() {
                Ok(mut file) => {
                    if let Err(error) = file.close() {
                        self.runtime_error = Some(error.to_string());
                    }
                }
                Err(_) => {
                    self.runtime_error =
                        Some("policy temporary storage could not be locked".to_owned())
                }
            }
        }
    }

    pub(super) fn close_latest_policy_temp_file(&mut self) {
        if let Some(file) = self.latest_policy_temp_file.take() {
            match file.lock() {
                Ok(mut file) => {
                    if let Err(error) = file.close() {
                        self.runtime_error = Some(error.to_string());
                    }
                }
                Err(_) => {
                    self.runtime_error =
                        Some("latest remote policy storage could not be locked".to_owned());
                }
            }
        }
    }
}
