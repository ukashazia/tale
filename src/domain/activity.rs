use serde_json::Value;

use super::Timestamp;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuditPrincipal {
    pub id: Option<String>,
    pub display: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuditTarget {
    pub id: Option<String>,
    pub display: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuditEvent {
    pub event_time: Timestamp,
    pub event_time_text: String,
    pub event_type: Option<String>,
    pub deferred_at: Option<Timestamp>,
    pub event_group_id: Option<String>,
    pub origin: Option<String>,
    pub actor: Option<AuditPrincipal>,
    pub target: Option<AuditTarget>,
    pub action: Option<String>,
    pub old: Option<Value>,
    pub new: Option<Value>,
    pub action_details: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuditSnapshot {
    pub version: Option<String>,
    pub tailnet: Option<String>,
    pub events: Vec<AuditEvent>,
    pub start: String,
    pub end: String,
    pub observed_at: Timestamp,
    pub delayed: bool,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct AuditFilters {
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub actor_id: Option<String>,
    pub actor_display: Option<String>,
    pub action: Option<String>,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub text: Option<String>,
}

impl AuditFilters {
    pub fn matches(&self, event: &AuditEvent) -> bool {
        if self.start.is_some_and(|value| event.event_time < value)
            || self.end.is_some_and(|value| event.event_time > value)
        {
            return false;
        }
        if self.actor_id.as_deref().is_some_and(|value| {
            event.actor.as_ref().and_then(|actor| actor.id.as_deref()) != Some(value)
        }) {
            return false;
        }
        if self.actor_display.as_deref().is_some_and(|value| {
            event
                .actor
                .as_ref()
                .and_then(|actor| actor.display.as_deref())
                != Some(value)
        }) {
            return false;
        }
        if self
            .action
            .as_deref()
            .is_some_and(|value| event.action.as_deref() != Some(value))
        {
            return false;
        }
        if self.target_type.as_deref().is_some_and(|value| {
            event
                .target
                .as_ref()
                .and_then(|target| target.kind.as_deref())
                != Some(value)
        }) {
            return false;
        }
        if self.target_id.as_deref().is_some_and(|value| {
            event
                .target
                .as_ref()
                .and_then(|target| target.id.as_deref())
                != Some(value)
        }) {
            return false;
        }
        self.text.as_deref().is_none_or(|value| {
            let query = value.to_ascii_lowercase();
            [
                event.event_type.as_deref(),
                event.origin.as_deref(),
                event.action.as_deref(),
                event.action_details.as_deref(),
                event.error.as_deref(),
                event.actor.as_ref().and_then(|actor| actor.id.as_deref()),
                event
                    .actor
                    .as_ref()
                    .and_then(|actor| actor.display.as_deref()),
                event
                    .target
                    .as_ref()
                    .and_then(|target| target.id.as_deref()),
                event
                    .target
                    .as_ref()
                    .and_then(|target| target.display.as_deref()),
            ]
            .into_iter()
            .flatten()
            .any(|field| field.to_ascii_lowercase().contains(&query))
        })
    }
}

impl AuditSnapshot {
    pub fn filtered_events(&self, filters: &AuditFilters) -> Vec<&AuditEvent> {
        self.events
            .iter()
            .filter(|event| filters.matches(event))
            .collect()
    }
}
