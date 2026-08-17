use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::process::Command;

use crate::local::process::LocalOperation;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HandoffCommand {
    Login {
        executable: OsString,
        socket_path: Option<OsString>,
    },
    Logout {
        executable: OsString,
        socket_path: Option<OsString>,
    },
    Ssh {
        executable: OsString,
        socket_path: Option<OsString>,
        username: Option<String>,
        host: String,
    },
    Nc {
        executable: OsString,
        socket_path: Option<OsString>,
        host: String,
        port: u16,
    },
}

impl HandoffCommand {
    pub fn operation(&self) -> LocalOperation {
        match self {
            Self::Login { .. } => LocalOperation::Login,
            Self::Logout { .. } => LocalOperation::Logout,
            Self::Ssh { .. } => LocalOperation::Ssh,
            Self::Nc { .. } => LocalOperation::Nc,
        }
    }

    pub fn executable(&self) -> &OsString {
        match self {
            Self::Login { executable, .. }
            | Self::Logout { executable, .. }
            | Self::Ssh { executable, .. }
            | Self::Nc { executable, .. } => executable,
        }
    }

    pub fn args(&self) -> Vec<OsString> {
        let (socket_path, mut args) = match self {
            Self::Login { socket_path, .. } => (socket_path, vec![OsString::from("login")]),
            Self::Logout { socket_path, .. } => (socket_path, vec![OsString::from("logout")]),
            Self::Ssh {
                socket_path,
                username,
                host,
                ..
            } => {
                let target = username
                    .as_deref()
                    .map_or_else(|| host.clone(), |username| format!("{username}@{host}"));
                (
                    socket_path,
                    vec![OsString::from("ssh"), OsString::from(target)],
                )
            }
            Self::Nc {
                socket_path,
                host,
                port,
                ..
            } => (
                socket_path,
                vec![
                    OsString::from("nc"),
                    OsString::from(host),
                    OsString::from(port.to_string()),
                ],
            ),
        };
        if let Some(socket_path) = socket_path {
            args.insert(0, socket_path.clone());
            args.insert(0, OsString::from("--socket"));
        }
        args
    }

    pub fn with_socket_path(mut self, path: &Path) -> Self {
        let socket_path = Some(path.as_os_str().to_os_string());
        match &mut self {
            Self::Login {
                socket_path: target,
                ..
            }
            | Self::Logout {
                socket_path: target,
                ..
            }
            | Self::Ssh {
                socket_path: target,
                ..
            }
            | Self::Nc {
                socket_path: target,
                ..
            } => *target = socket_path,
        }
        self
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum HandoffError {
    #[error("interactive child argument is invalid: {0}")]
    InvalidArgument(String),
    #[error("interactive child could not be started: {0}")]
    Spawn(String),
    #[error("interactive child could not be waited on: {0}")]
    Wait(String),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HandoffResult {
    pub operation: LocalOperation,
    pub exit_status: Option<i32>,
    pub started_at: Instant,
    pub finished_at: Instant,
}

pub fn validate_ssh_username(value: &str) -> Result<(), HandoffError> {
    if value.is_empty() || value.contains('@') || value.chars().any(char::is_whitespace) {
        return Err(HandoffError::InvalidArgument(
            "SSH username must be non-empty and cannot contain @ or whitespace".to_owned(),
        ));
    }
    Ok(())
}

pub fn validate_ssh_host(value: &str) -> Result<(), HandoffError> {
    if value.is_empty() || value.chars().any(char::is_whitespace) || value.contains('@') {
        return Err(HandoffError::InvalidArgument(
            "SSH host must be a selected host without @ or whitespace".to_owned(),
        ));
    }
    Ok(())
}

pub fn parse_nc_port(value: &str) -> Result<u16, HandoffError> {
    let port = value
        .parse::<u16>()
        .map_err(|_| HandoffError::InvalidArgument("netcat port must be an integer".to_owned()))?;
    if port == 0 {
        return Err(HandoffError::InvalidArgument(
            "netcat port must be between 1 and 65535".to_owned(),
        ));
    }
    Ok(port)
}

pub fn ssh_command(
    executable: &Path,
    username: Option<&str>,
    host: &str,
) -> Result<HandoffCommand, HandoffError> {
    validate_ssh_host(host)?;
    if let Some(username) = username {
        validate_ssh_username(username)?;
    }
    Ok(HandoffCommand::Ssh {
        executable: executable.as_os_str().to_os_string(),
        socket_path: None,
        username: username.map(ToOwned::to_owned),
        host: host.to_owned(),
    })
}

pub fn nc_command(
    executable: &Path,
    host: &str,
    port: &str,
) -> Result<HandoffCommand, HandoffError> {
    validate_ssh_host(host)?;
    let port = parse_nc_port(port)?;
    Ok(HandoffCommand::Nc {
        executable: executable.as_os_str().to_os_string(),
        socket_path: None,
        host: host.to_owned(),
        port,
    })
}

pub fn login_command(executable: &Path) -> HandoffCommand {
    HandoffCommand::Login {
        executable: executable.as_os_str().to_os_string(),
        socket_path: None,
    }
}

pub fn logout_command(executable: &Path) -> HandoffCommand {
    HandoffCommand::Logout {
        executable: executable.as_os_str().to_os_string(),
        socket_path: None,
    }
}

pub async fn run(command: HandoffCommand) -> Result<HandoffResult, HandoffError> {
    let started_at = Instant::now();
    let operation = command.operation();
    let mut child = Command::new(command.executable());
    child
        .args(command.args())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    let mut child = child
        .spawn()
        .map_err(|error| HandoffError::Spawn(error.to_string()))?;
    let status = child
        .wait()
        .await
        .map_err(|error| HandoffError::Wait(error.to_string()))?;
    Ok(HandoffResult {
        operation,
        exit_status: status.code(),
        started_at,
        finished_at: Instant::now(),
    })
}

pub fn bounded_handoff_duration(result: &HandoffResult) -> Duration {
    result
        .finished_at
        .checked_duration_since(result.started_at)
        .unwrap_or(Duration::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_and_nc_argv_are_exact_and_single_value_targets() {
        let executable = Path::new("/tmp/tailscale with spaces");
        let ssh = ssh_command(executable, Some("alice"), "host.example");
        assert!(ssh.is_ok());
        if let Ok(ssh) = ssh {
            assert_eq!(
                ssh.args(),
                vec![OsString::from("ssh"), OsString::from("alice@host.example")]
            );
        }
        let nc = nc_command(executable, "host.example", "443");
        assert!(nc.is_ok());
        if let Ok(nc) = nc {
            assert_eq!(
                nc.args(),
                vec![
                    OsString::from("nc"),
                    OsString::from("host.example"),
                    OsString::from("443")
                ]
            );
        }
    }

    #[test]
    fn unsafe_ssh_forms_and_zero_port_are_rejected() {
        let executable = Path::new("tailscale");
        assert!(ssh_command(executable, Some("a@b"), "host").is_err());
        assert!(ssh_command(executable, None, "host name").is_err());
        assert!(nc_command(executable, "host", "0").is_err());
    }
}
