use super::*;

impl App {
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
                    self.add_notification(task_id, crate::task::TaskResultKind::Success, &summary);
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
                    self.add_notification(task_id, crate::task::TaskResultKind::Failure, &summary);
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
                        crate::task::TaskResultKind::Cancelled,
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
        kind: crate::task::TaskResultKind,
        message: &str,
    ) {
        self.notifications.push(Notification {
            task_id,
            message: message.to_owned(),
            kind,
            expires_at: self.now.saturating_add(5),
        });
    }
}
