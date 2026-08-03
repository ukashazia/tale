use serde_json::Value;

use crate::admin::dto::{
    AuditEventDto, DtoError, object_kind, object_string, parse_timestamp, required_collection,
};
use crate::domain::Timestamp;
use crate::domain::activity::{AuditEvent, AuditPrincipal, AuditSnapshot, AuditTarget};

pub fn decode_audit(
    events: Option<Vec<AuditEventDto>>,
    observed_at: Timestamp,
) -> Result<AuditSnapshot, DtoError> {
    decode_audit_with_token(events, observed_at, None)
}

pub fn decode_audit_with_token(
    events: Option<Vec<AuditEventDto>>,
    observed_at: Timestamp,
    token: Option<&str>,
) -> Result<AuditSnapshot, DtoError> {
    let events = required_collection(events, "logs")?;
    if events.len() > 50_000 {
        return Err(DtoError::AuditLimit);
    }
    let values = events
        .into_iter()
        .map(|event| decode_event(event, token))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AuditSnapshot {
        version: None,
        tailnet: None,
        events: values,
        start: String::new(),
        end: String::new(),
        observed_at,
        delayed: true,
    })
}

fn decode_event(event: AuditEventDto, token: Option<&str>) -> Result<AuditEvent, DtoError> {
    let event_time = parse_timestamp(event.event_time.as_deref(), "audit.eventTime")?.ok_or(
        DtoError::InvalidTimestamp {
            field: "audit.eventTime",
        },
    )?;
    Ok(AuditEvent {
        event_time,
        event_time_text: match event.event_time {
            Some(event_time) => event_time,
            None => "not returned".to_owned(),
        },
        event_type: event.event_type.map(|value| redact_text(&value, token)),
        deferred_at: parse_timestamp(event.deferred_at.as_deref(), "audit.deferredAt")?,
        event_group_id: event.event_group_id.map(|value| redact_text(&value, token)),
        origin: event.origin.map(|value| redact_text(&value, token)),
        actor: principal(event.actor.as_ref(), token),
        target: target(event.target.as_ref(), token),
        action: event.action.map(|value| redact_text(&value, token)),
        old: safe_value(event.old, token),
        new: safe_value(event.new, token),
        action_details: event.action_details.map(|value| redact_text(&value, token)),
        error: event.error.map(|value| redact_text(&value, token)),
    })
}

fn principal(value: Option<&Value>, token: Option<&str>) -> Option<AuditPrincipal> {
    let value = value?;
    Some(AuditPrincipal {
        id: redact_optional(object_string(value, &["id", "userId", "user_id"]), token),
        display: redact_optional(
            object_string(value, &["name", "displayName", "loginName", "login_name"]),
            token,
        ),
        kind: object_kind(value),
    })
}

fn target(value: Option<&Value>, token: Option<&str>) -> Option<AuditTarget> {
    let value = value?;
    Some(AuditTarget {
        id: redact_optional(
            object_string(value, &["id", "deviceId", "userId", "keyId"]),
            token,
        ),
        display: redact_optional(
            object_string(value, &["name", "displayName", "loginName"]),
            token,
        ),
        kind: object_kind(value),
    })
}

fn safe_value(value: Option<Value>, token: Option<&str>) -> Option<Value> {
    value.map(|value| {
        let value = crate::admin::dto::redact_json_value(&value);
        redact_token_value(value, token)
    })
}

fn redact_token_value(value: Value, token: Option<&str>) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, redact_token_value(value, token)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| redact_token_value(value, token))
                .collect(),
        ),
        Value::String(value) => match token {
            Some(token) => Value::String(value.replace(token, "<redacted>")),
            None => Value::String(value),
        },
        other => other,
    }
}

fn redact_text(value: &str, token: Option<&str>) -> String {
    let value = crate::admin::client::redact_text(value);
    match token {
        Some(token) => value.replace(token, "<redacted>"),
        None => value,
    }
}

fn redact_optional(value: Option<String>, token: Option<&str>) -> Option<String> {
    value.map(|value| redact_text(&value, token))
}
