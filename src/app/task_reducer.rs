use super::*;

impl App {
    pub(super) fn add_task_started_notification(&mut self, task_id: TaskId) {
        let Some(task) = self.tasks.get(task_id) else {
            return;
        };
        self.notifications.push(Notification {
            task_id,
            message: format!("{} running · @ view task", task.target_label),
            kind: crate::task::TaskNotificationKind::Running,
            expires_at: self.now.saturating_add(5),
        });
    }

    pub(super) fn start_task(
        &mut self,
        action_id: ActionId,
        behavior: MockTaskBehavior,
        cancellable: bool,
    ) -> Vec<Effect> {
        let id = self
            .tasks
            .create(action_id, "mock simulation", self.now, cancellable);
        vec![Effect::StartMockTask {
            task_id: id,
            behavior,
            started_at: self.now,
        }]
    }
}

impl App {
    pub(super) fn cancel_focused_task(&mut self) -> Vec<Effect> {
        let Some(id) = self.tasks.selected else {
            return Vec::new();
        };
        self.cancel_task(id)
    }

    pub(super) fn retry_focused_task(&mut self) -> Vec<Effect> {
        let Some(task_id) = self.tasks.selected else {
            self.runtime_error = Some("select a task to retry".to_owned());
            return Vec::new();
        };
        if let Some(reason) = self.task_retry_unavailable_reason(task_id) {
            self.runtime_error = Some(reason);
            return Vec::new();
        }
        if self.admin_batch_results.contains_key(&task_id) {
            return self.retry_selected_batch();
        }
        let Some(task) = self.tasks.get(task_id) else {
            return Vec::new();
        };
        let action_id = task.action_id;
        if let Some(replay) = self.task_replays.get(&task_id).cloned() {
            return match replay {
                TaskReplay::AdminMutation(request) => self.retry_admin_mutation(request),
                TaskReplay::Diagnostic(request) => self.start_local_diagnostic(request),
                TaskReplay::LocalMutation(mutation) => self.open_mutation_confirmation(mutation),
                TaskReplay::Service(request) => self.open_service_confirmation(request),
                TaskReplay::TerminalHandoff(command) => {
                    self.open_task_handoff_confirmation(action_id, command)
                }
            };
        }
        match action_id {
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
            _ => Vec::new(),
        }
    }

    fn retry_admin_mutation(&mut self, request: AdminMutationRequest) -> Vec<Effect> {
        let mutation_id = self.next_mutation_id;
        self.next_mutation_id = self.next_mutation_id.saturating_add(1);
        self.start_admin_preflight(AdminMutationRequest::new(
            mutation_id,
            request.profile,
            request.target_id,
            request.base_snapshot,
            request.change,
            request.action_id,
            request.risk,
        ))
    }

    pub(super) fn task_retry_unavailable_reason(&self, task_id: TaskId) -> Option<String> {
        let Some(task) = self.tasks.get(task_id) else {
            return Some("the selected task is unavailable".to_owned());
        };
        if !matches!(
            task.state,
            TaskState::Failed | TaskState::Cancelled | TaskState::Interrupted
        ) {
            return Some(match task.state {
                TaskState::Succeeded => "the selected task already succeeded".to_owned(),
                _ => "stop the selected task before retrying it".to_owned(),
            });
        }
        if self.admin_batch_results.contains_key(&task_id) {
            return None;
        }
        if self.task_replays.contains_key(&task_id) {
            return None;
        }
        if self.source_mode == SourceMode::Mock
            && matches!(
                task.action_id,
                ActionId::MockSuccess
                    | ActionId::MockFailure
                    | ActionId::MockCancellable
                    | ActionId::MockNonCancellable
            )
        {
            return None;
        }
        Some("the original request is not retained in task history; run this action again from its source page".to_owned())
    }

    fn open_task_handoff_confirmation(
        &mut self,
        action_id: ActionId,
        command: HandoffCommand,
    ) -> Vec<Effect> {
        let redacted_argv = redacted_argv(&command.args());
        let required_phrase =
            (action_id == ActionId::LocalAccountLogout).then(|| "LOGOUT".to_owned());
        self.overlays
            .push(Overlay::Confirmation(Box::new(ConfirmationState {
                action_id,
                admin_generation: self.admin_generation,
                mutation: None,
                admin_mutation: None,
                admin_batch: None,
                service_request: None,
                operational_mutation: None,
                handoff: Some(command),
                prompt: "Retry this interactive command in the inherited terminal?".to_owned(),
                required_phrase,
                input: String::new(),
                lose_ssh_checked: false,
                preview_lines: vec![
                    "Tale will pause while the command owns the terminal".to_owned(),
                ],
                redacted_argv,
                error: None,
            })));
        Vec::new()
    }

    pub(super) fn cancel_task(&mut self, id: TaskId) -> Vec<Effect> {
        if !self.tasks.request_cancel(id) {
            return Vec::new();
        }
        let mut effects = vec![Effect::CancelTask { task_id: id }];
        if let Some(batch) = self.admin_batches_in_flight.get_mut(&id.0) {
            let pending = std::mem::take(&mut batch.pending_requests);
            for request in pending {
                batch.batch.record(
                    request.target_id,
                    crate::domain::admin_mutation::BatchChildOutcome::CancelledBeforeDispatch,
                );
                self.admin_resource_locks.release(request.mutation_id);
            }
            let children = batch.child_tasks.values().copied().collect::<Vec<_>>();
            for child in children {
                if self.tasks.request_cancel(child) {
                    effects.push(Effect::CancelTask { task_id: child });
                }
            }
        }
        effects
    }

    pub(super) fn request_shutdown(&mut self, reason: ShutdownReason) -> Vec<Effect> {
        if reason == ShutdownReason::UserQuit {
            self.runtime_error = None;
        }
        if matches!(self.shutdown_state, ShutdownState::Running) {
            self.shutdown_state = ShutdownState::Requested(reason);
            self.close_policy_temp_file();
            self.close_latest_policy_temp_file();
            if let Some(workflow) = self.policy_workflow.as_mut() {
                workflow.close();
            }
            self.policy_workflow = None;
            self.pending_auth_key_result = None;
            if let Some(result) = self.secret_result.as_mut() {
                result.close();
            }
            self.secret_result = None;
            self.overlays.clear();
            self.render_invalidated = true;
        }
        self.tasks
            .active()
            .filter(|task| task.cancellable)
            .map(|task| Effect::CancelTask { task_id: task.id })
            .chain(std::iter::once(Effect::RequestShutdown))
            .collect()
    }

    pub(super) fn update_task(&mut self, event: TaskEvent) -> Vec<Effect> {
        match event {
            TaskEvent::Started { task_id } => {
                let _ = self.tasks.start(task_id);
            }
            TaskEvent::Progress {
                task_id,
                progress,
                detail,
            } => {
                let _ = self.tasks.progress(task_id, progress, &detail);
            }
            TaskEvent::Succeeded {
                task_id,
                finished_at,
                summary,
                detail,
            } => {
                if self.tasks.succeed(task_id, finished_at, &summary, &detail) {
                    self.add_notification(
                        task_id,
                        crate::task::TaskNotificationKind::Success,
                        &summary,
                    );
                    self.tasks
                        .evict_completed(self.resolved_config.history.max_tasks);
                }
            }
            TaskEvent::Failed {
                task_id,
                finished_at,
                summary,
                detail,
            } => {
                if self.tasks.fail(task_id, finished_at, &summary, &detail) {
                    self.add_notification(
                        task_id,
                        crate::task::TaskNotificationKind::Failure,
                        &summary,
                    );
                    self.tasks
                        .evict_completed(self.resolved_config.history.max_tasks);
                }
            }
            TaskEvent::Cancelled {
                task_id,
                finished_at,
                detail,
            } => {
                if self.tasks.cancel(task_id, finished_at, &detail) {
                    self.add_notification(
                        task_id,
                        crate::task::TaskNotificationKind::Cancelled,
                        "cancelled",
                    );
                    self.tasks
                        .evict_completed(self.resolved_config.history.max_tasks);
                }
            }
            TaskEvent::DiagnosticProgress {
                task_id,
                progress,
                detail,
                sample,
                netcheck,
            } => {
                return self.update_local(LocalEvent::DiagnosticProgress {
                    task_id,
                    progress,
                    detail,
                    sample,
                    netcheck,
                });
            }
            TaskEvent::DiagnosticResult { task_id, result } => {
                return self.update_local(LocalEvent::DiagnosticResult { task_id, result });
            }
        }
        Vec::new()
    }

    pub(super) fn add_notification(
        &mut self,
        task_id: TaskId,
        kind: crate::task::TaskNotificationKind,
        message: &str,
    ) {
        let message = self.tasks.get(task_id).map_or_else(
            || format!("{message} · @ view task"),
            |task| format!("{}: {message} · @ view task", task.target_label),
        );
        self.notifications.push(Notification {
            task_id,
            message,
            kind,
            expires_at: self.now.saturating_add(5),
        });
    }
}
