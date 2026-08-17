use super::*;

impl App {
    pub(super) fn change_diagnostics_section(&mut self, offset: isize) {
        let sections = DiagnosticsSection::ALL;
        let length = sections.len();
        let current = sections
            .iter()
            .position(|section| *section == self.views.diagnostics.section)
            .unwrap_or(0);
        let step = offset.rem_euclid(length as isize).unsigned_abs();
        let next = current.saturating_add(step) % length;
        self.views.diagnostics.section = sections
            .get(next)
            .copied()
            .unwrap_or(DiagnosticsSection::Client);
        self.views.diagnostics.scroll = 0;
        self.focus = Focus::Collection;
    }

    pub(super) fn load_visible_diagnostics(&mut self) -> Vec<Effect> {
        if self.current_route() != Route::Diagnostics || self.local_executable.is_none() {
            return Vec::new();
        }
        match self.views.diagnostics.section {
            DiagnosticsSection::Client
                if self.services_snapshot.metrics.status == ServiceResourceStatus::Idle =>
            {
                self.start_service_request(ServiceActionRequest::Metrics)
            }
            DiagnosticsSection::DnsStatus if self.dns_status_needs_loading() => {
                self.start_local_diagnostic(DiagnosticRequest::DnsStatus)
            }
            _ => Vec::new(),
        }
    }

    fn dns_status_needs_loading(&self) -> bool {
        !self
            .local_diagnostics
            .values()
            .any(|state| matches!(state.result, Some(DiagnosticResult::DnsStatus(_))))
            && !self.dns_status_is_loading()
    }

    pub(crate) fn dns_status_is_loading(&self) -> bool {
        self.local_diagnostics.iter().any(|(task_id, state)| {
            state.kind == "dns status"
                && state.result.is_none()
                && self
                    .tasks
                    .get(*task_id)
                    .is_some_and(|task| !task.state.is_terminal())
        })
    }
}
