use std::ffi::OsString;
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;

use crate::domain::source::LocalFailureKind;
use crate::local::process::{self, Cancellation, LocalCommand, LocalOperation};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SystemPolicyEntry {
    pub name: String,
    pub source: Option<String>,
    pub value: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum PolicyError {
    #[error("system policy command permission denied")]
    PermissionDenied,
    #[error("system policy command timed out")]
    TimedOut,
    #[error("system policy command cancelled")]
    Cancelled,
    #[error("system policy command failed: {0}")]
    Command(String),
    #[error("system policy JSON was invalid: {0}")]
    InvalidJson(String),
}

impl PolicyError {
    pub fn failure_kind(&self) -> LocalFailureKind {
        match self {
            Self::PermissionDenied => LocalFailureKind::PermissionDenied,
            Self::TimedOut => LocalFailureKind::TimedOut,
            Self::Cancelled => LocalFailureKind::Cancelled,
            Self::InvalidJson(_) => LocalFailureKind::InvalidOutput,
            Self::Command(detail) => classify_command_detail(detail),
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::Command(_) | Self::TimedOut)
    }
}

pub fn list_command(path: &std::path::Path, timeout: Duration) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::SyspolicyList,
        vec![
            OsString::from("syspolicy"),
            OsString::from("list"),
            OsString::from("--json"),
        ],
    )
    .with_timeout(timeout)
}

pub fn reload_command(path: &std::path::Path, timeout: Duration) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::SyspolicyReload,
        vec![
            OsString::from("syspolicy"),
            OsString::from("reload"),
            OsString::from("--json"),
        ],
    )
    .with_timeout(timeout)
}

pub async fn list(
    path: &std::path::Path,
    timeout: Duration,
    cancellation: &Cancellation,
    socket_path: Option<&std::path::Path>,
) -> Result<Vec<SystemPolicyEntry>, PolicyError> {
    let command = match socket_path {
        Some(socket_path) => {
            list_command(path, timeout).with_socket_path(socket_path.as_os_str().to_os_string())
        }
        None => list_command(path, timeout),
    };
    let result = process::run(command, cancellation)
        .await
        .map_err(process_error)?;
    if result.exit_status != Some(0) {
        return Err(PolicyError::Command(
            String::from_utf8_lossy(&result.stderr).into_owned(),
        ));
    }
    let value = process::decode_utf8(&result.stdout)
        .map_err(|error| PolicyError::InvalidJson(error.to_string()))?;
    decode_policy(value)
}

fn process_error(error: crate::local::process::LocalProcessError) -> PolicyError {
    match error {
        crate::local::process::LocalProcessError::PermissionDenied => PolicyError::PermissionDenied,
        crate::local::process::LocalProcessError::TimedOut => PolicyError::TimedOut,
        crate::local::process::LocalProcessError::Cancelled => PolicyError::Cancelled,
        error => PolicyError::Command(error.to_string()),
    }
}

fn classify_command_detail(detail: &str) -> LocalFailureKind {
    let detail = detail.to_ascii_lowercase();
    if detail.contains("permission denied") || detail.contains("access is denied") {
        LocalFailureKind::PermissionDenied
    } else if detail.contains("unknown command")
        || detail.contains("unrecognized option")
        || detail.contains("not supported")
    {
        LocalFailureKind::UnsupportedClient
    } else if detail.contains("timed out") || detail.contains("timeout") {
        LocalFailureKind::TimedOut
    } else {
        LocalFailureKind::Transport
    }
}

pub fn decode_policy(input: &str) -> Result<Vec<SystemPolicyEntry>, PolicyError> {
    let value: Value =
        serde_json::from_str(input).map_err(|error| PolicyError::InvalidJson(error.to_string()))?;
    let object = value.as_object().ok_or_else(|| {
        PolicyError::InvalidJson("policy response was not a JSON object".to_owned())
    })?;
    let settings = object
        .get("Settings")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            PolicyError::InvalidJson("policy response did not contain a Settings object".to_owned())
        })?;
    let mut entries = settings
        .iter()
        .map(|(name, value)| entry_from_value(name.clone(), value))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

fn entry_from_value(name: String, value: &Value) -> Result<SystemPolicyEntry, PolicyError> {
    let object = value.as_object().ok_or_else(|| {
        PolicyError::InvalidJson(format!("policy setting {name} was not an object"))
    })?;
    let setting_value = object.get("Value").and_then(value_to_string);
    Ok(SystemPolicyEntry {
        name: name.clone(),
        source: object.get("Origin").and_then(origin_to_string),
        value: setting_value.map(|value| policy_value(&name, value)),
        error: object.get("Error").and_then(value_to_string),
    })
}

fn origin_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Object(object) => {
            let name = object
                .get("Name")
                .and_then(Value::as_str)
                .map_or("", |value| value);
            let scope = object.get("Scope").and_then(value_to_string);
            match (name.is_empty(), scope) {
                (true, Some(scope)) if !scope.is_empty() => Some(scope),
                (false, Some(scope)) if !scope.is_empty() => Some(format!("{name} ({scope})")),
                (false, _) => Some(name.to_owned()),
                (true, _) => None,
            }
        }
        _ => value_to_string(value),
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    let value = match value {
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Null => None,
        _ => Some(value.to_string()),
    }?;
    Some(bound_text(&value))
}

fn policy_value(name: &str, value: String) -> String {
    if name.eq_ignore_ascii_case("AuthKey") {
        "[redacted]".to_owned()
    } else {
        value
    }
}

fn bound_text(value: &str) -> String {
    const LIMIT: usize = 4096;
    if value.len() <= LIMIT {
        return value.to_owned();
    }
    let mut end = 0;
    for (index, character) in value.char_indices() {
        if index.saturating_add(character.len_utf8()) > LIMIT {
            break;
        }
        end = index.saturating_add(character.len_utf8());
    }
    format!("{}\n...[policy value truncated]", &value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_commands_are_exact() {
        let path = std::path::Path::new("tailscale");
        assert_eq!(
            list_command(path, Duration::from_secs(1)).args,
            vec![
                OsString::from("syspolicy"),
                OsString::from("list"),
                OsString::from("--json")
            ]
        );
        assert_eq!(
            reload_command(path, Duration::from_secs(1)).args,
            vec![
                OsString::from("syspolicy"),
                OsString::from("reload"),
                OsString::from("--json")
            ]
        );
    }

    #[test]
    fn policy_fixture_keeps_source_and_effective_value() {
        let result = decode_policy(
            r#"{
                "Summary":{"Scope":"Device"},
                "Settings":{
                    "ssh":{"Origin":{"Name":"mdm","Scope":"Device"},"Value":false}
                }
            }"#,
        );
        assert!(result.is_ok());
        if let Ok(result) = result {
            assert_eq!(result[0].source.as_deref(), Some("mdm (Device)"));
            assert_eq!(result[0].value.as_deref(), Some("false"));
            assert_eq!(result[0].error, None);
        }
    }
}
