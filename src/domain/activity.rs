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
