use std::ffi::OsString;
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;

use crate::domain::account::{LocalAccount, deduplicate_accounts};
use crate::domain::source::LocalFailureKind;
use crate::local::process::{self, Cancellation, LocalCommand, LocalOperation};

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum AccountError {
    #[error("account command is unavailable for this client")]
    Unsupported,
    #[error("account command permission denied")]
    PermissionDenied,
    #[error("account command timed out")]
    TimedOut,
    #[error("account command cancelled")]
    Cancelled,
    #[error("account JSON was invalid: {0}")]
    InvalidJson(String),
    #[error("account command failed: {0}")]
    Command(String),
    #[error("account ID cannot be empty")]
    EmptyId,
}

impl AccountError {
    pub fn failure_kind(&self) -> LocalFailureKind {
        match self {
            Self::Unsupported => LocalFailureKind::UnsupportedClient,
            Self::PermissionDenied => LocalFailureKind::PermissionDenied,
            Self::TimedOut => LocalFailureKind::TimedOut,
            Self::Cancelled => LocalFailureKind::Cancelled,
            Self::EmptyId | Self::InvalidJson(_) => LocalFailureKind::InvalidOutput,
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
        LocalOperation::SwitchList,
        vec![
            OsString::from("switch"),
            OsString::from("--list"),
            OsString::from("--json"),
        ],
    )
    .with_timeout(timeout)
}

pub fn switch_command(
    path: &std::path::Path,
    timeout: Duration,
    account_id: &str,
) -> Result<LocalCommand, AccountError> {
    let account_id = valid_id(account_id)?;
    Ok(LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::Switch,
        vec![OsString::from("switch"), OsString::from(account_id)],
    )
    .with_timeout(timeout))
}

pub fn remove_command(
    path: &std::path::Path,
    timeout: Duration,
    account_id: &str,
) -> Result<LocalCommand, AccountError> {
    let account_id = valid_id(account_id)?;
    Ok(LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::SwitchRemove,
        vec![
            OsString::from("switch"),
            OsString::from("remove"),
            OsString::from(account_id),
        ],
    )
    .with_timeout(timeout))
}

pub fn login_command(path: &std::path::Path) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::Login,
        vec![OsString::from("login")],
    )
    .without_timeout()
}

pub fn logout_command(path: &std::path::Path) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::Logout,
        vec![OsString::from("logout")],
    )
    .without_timeout()
}

pub async fn list(
    path: &std::path::Path,
    timeout: Duration,
    cancellation: &Cancellation,
    socket_path: Option<&std::path::Path>,
) -> Result<Vec<LocalAccount>, AccountError> {
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
        return Err(AccountError::Command(
            String::from_utf8_lossy(&result.stderr).into_owned(),
        ));
    }
    let text = process::decode_utf8(&result.stdout)
        .map_err(|error| AccountError::InvalidJson(error.to_string()))?;
    decode_accounts(text)
}

fn process_error(error: crate::local::process::LocalProcessError) -> AccountError {
    match error {
        crate::local::process::LocalProcessError::PermissionDenied => {
            AccountError::PermissionDenied
        }
        crate::local::process::LocalProcessError::TimedOut => AccountError::TimedOut,
        crate::local::process::LocalProcessError::Cancelled => AccountError::Cancelled,
        error => AccountError::Command(error.to_string()),
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

pub fn decode_accounts(input: &str) -> Result<Vec<LocalAccount>, AccountError> {
    let value: Value = serde_json::from_str(input)
        .map_err(|error| AccountError::InvalidJson(error.to_string()))?;
    let values = match value {
        Value::Array(values) => values,
        _ => {
            return Err(AccountError::InvalidJson(
                "account list was not an array".to_owned(),
            ));
        }
    };
    parse_account_values(values)
}

fn parse_account_values(values: Vec<Value>) -> Result<Vec<LocalAccount>, AccountError> {
    let mut accounts = Vec::with_capacity(values.len());
    for value in values {
        let Some(object) = value.as_object() else {
            return Err(AccountError::InvalidJson(
                "account entry was not an object".to_owned(),
            ));
        };
        let id = string(object, "id")
            .ok_or_else(|| AccountError::InvalidJson("account ID was not returned".to_owned()))?;
        if id.is_empty() {
            return Err(AccountError::InvalidJson("account ID was empty".to_owned()));
        }
        accounts.push(LocalAccount {
            id,
            tailnet_name: string(object, "tailnet"),
            account_name: string(object, "account"),
            display_name: None,
            profile_name: string(object, "nickname"),
            active: bool_value(object, "selected").is_some_and(|value| value),
        });
    }
    deduplicate_accounts(&mut accounts);
    Ok(accounts)
}

fn valid_id(value: &str) -> Result<&str, AccountError> {
    if value.trim().is_empty() {
        Err(AccountError::EmptyId)
    } else {
        Ok(value)
    }
}

fn string(object: &serde_json::Map<String, Value>, name: &str) -> Option<String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn bool_value(object: &serde_json::Map<String, Value>, name: &str) -> Option<bool> {
    object.get(name).and_then(Value::as_bool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_commands_use_opaque_ids_without_shell_parsing() {
        let path = std::path::Path::new("/tmp/tailscale with spaces");
        let command = switch_command(path, Duration::from_secs(2), "profile id").ok();
        assert!(command.is_some());
        if let Some(command) = command {
            assert_eq!(
                command.args,
                vec![OsString::from("switch"), OsString::from("profile id")]
            );
        }
        let remove = remove_command(path, Duration::from_secs(2), "opaque").ok();
        assert!(remove.is_some());
        if let Some(remove) = remove {
            assert_eq!(
                remove.args,
                vec![
                    OsString::from("switch"),
                    OsString::from("remove"),
                    OsString::from("opaque")
                ]
            );
        }
    }

    #[test]
    fn list_fixture_accepts_unknown_fields_and_falls_back_to_id() {
        let decoded = decode_accounts(
            r#"[
                {"id":"a","nickname":"Work","selected":true,"new":1},
                {"id":"b","nickname":"Personal"},
                {"id":"c"}
            ]"#,
        );
        assert!(decoded.is_ok());
        if let Ok(decoded) = decoded {
            assert_eq!(decoded[0].display_label(), "Work");
            assert_eq!(decoded[1].display_label(), "Personal");
            assert_eq!(decoded[2].display_label(), "c");
        }
    }
}
