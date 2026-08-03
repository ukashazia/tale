use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;

use crate::domain::Timestamp;
use crate::domain::preference::{LocalPreferences, PreferenceRequest};
use crate::domain::route::{AdvertisementRequest, ExitNodeRequest};
use crate::domain::source::LocalSnapshot;
use crate::domain::source::{
    ExecutableSource, LocalCapabilities, LocalExecutable, LocalFailure, LocalFailureKind,
    LocalState,
};

use super::accounts::AccountError;
use super::dto;
use super::policy::PolicyError;
use super::preferences::{
    PreferenceClient, PreferenceCommandError, PreferenceError, PreferencePlatform,
    advertisement_command, exit_node_command, set_command,
};
use super::process::{self, Cancellation, LocalCommand, LocalOperation, LocalProcessError};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum HostPlatform {
    Unix,
    Windows,
}

#[derive(Debug, Clone)]
pub struct ExecutableResolution {
    pub cli_path: Option<PathBuf>,
    pub environment_path: Option<OsString>,
    pub config_path: Option<PathBuf>,
    pub path: Option<OsString>,
    pub platform: HostPlatform,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResolvedExecutable {
    pub path: PathBuf,
    pub source: ExecutableSource,
}

#[derive(Error, Debug, Clone, Eq, PartialEq)]
pub enum ExecutableError {
    #[error("tailscale executable was not found")]
    NotFound,
    #[error("tailscale executable is not executable")]
    PermissionDenied,
    #[error("tailscale executable path is invalid")]
    InvalidPath,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VersionInfo {
    pub version: String,
    pub daemon_version: Option<String>,
    pub build: Option<String>,
}

#[derive(Error, Debug, Clone, Eq, PartialEq)]
pub enum ClientError {
    #[error("{0}")]
    Process(LocalProcessError),
    #[error("unsupported local command: {0}")]
    UnsupportedCommand(String),
    #[error("unsupported local output for {operation}: {detail}")]
    UnsupportedOutput { operation: String, detail: String },
    #[error("local command {operation} exited with status {status:?}: {detail}")]
    NonZero {
        operation: String,
        status: Option<i32>,
        detail: String,
    },
    #[error("{0}")]
    Preferences(PreferenceError),
    #[error("{0}")]
    PreferenceCommand(PreferenceCommandError),
    #[error("{0}")]
    Accounts(AccountError),
    #[error("{0}")]
    Policy(PolicyError),
}

impl ClientError {
    pub fn operation(&self) -> String {
        match self {
            Self::Process(_) => "local command".to_owned(),
            Self::UnsupportedCommand(operation)
            | Self::UnsupportedOutput { operation, .. }
            | Self::NonZero { operation, .. } => operation.clone(),
            Self::Preferences(error) => error.operation().to_owned(),
            Self::PreferenceCommand(_) => "preferences".to_owned(),
            Self::Accounts(_) => "accounts".to_owned(),
            Self::Policy(_) => "system policy".to_owned(),
        }
    }

    pub fn failure(&self) -> LocalFailure {
        let operation = self.operation();
        match self {
            Self::Process(LocalProcessError::NotFound) => LocalFailure::new(
                LocalFailureKind::ExecutableMissing,
                operation,
                "tailscale executable missing",
                "install or expose tailscale outside Tale, then retry",
                false,
            ),
            Self::Process(LocalProcessError::PermissionDenied) => LocalFailure::new(
                LocalFailureKind::ExecutableDenied,
                operation,
                "tailscale executable permission denied",
                "check the executable permissions outside Tale",
                false,
            ),
            Self::Process(LocalProcessError::TimedOut) => LocalFailure::new(
                LocalFailureKind::TimedOut,
                operation,
                "local command timed out",
                "the command exceeded the configured timeout",
                true,
            ),
            Self::Process(LocalProcessError::Cancelled) => LocalFailure::new(
                LocalFailureKind::Cancelled,
                operation,
                "local command cancelled",
                "the command was cancelled",
                false,
            ),
            Self::Process(error) => LocalFailure::new(
                LocalFailureKind::Transport,
                operation,
                "local command failed",
                bounded_detail(&error.to_string()),
                true,
            ),
            Self::UnsupportedCommand(_) => LocalFailure::new(
                LocalFailureKind::UnsupportedClient,
                operation,
                "local command unavailable",
                "the installed client did not advertise this command",
                false,
            ),
            Self::UnsupportedOutput {
                operation, detail, ..
            } => LocalFailure::new(
                if operation == "version" {
                    LocalFailureKind::UnsupportedClient
                } else {
                    LocalFailureKind::InvalidOutput
                },
                operation,
                "local output is unsupported",
                bounded_detail(detail),
                false,
            ),
            Self::NonZero { detail, .. } => {
                let kind = classify_stderr(detail);
                let safe_detail = match kind {
                    LocalFailureKind::UnsupportedClient => {
                        "the client did not accept the requested command or flag"
                    }
                    LocalFailureKind::PermissionDenied => {
                        "the operating system denied the operation"
                    }
                    LocalFailureKind::NeedsLogin => "the client requires authentication",
                    LocalFailureKind::DaemonUnavailable => "the local daemon could not be reached",
                    _ => "the local command returned a non-zero status",
                };
                LocalFailure::new(
                    kind,
                    operation,
                    "local command returned an error",
                    safe_detail,
                    true,
                )
            }
            Self::Preferences(error) => LocalFailure::new(
                match error {
                    PreferenceError::PermissionDenied => LocalFailureKind::PermissionDenied,
                    PreferenceError::UnsupportedPlatform { .. }
                    | PreferenceError::UnsupportedVersion { .. }
                    | PreferenceError::HttpStatus {
                        status: 404 | 405 | 501,
                    } => LocalFailureKind::UnsupportedClient,
                    PreferenceError::TimedOut => LocalFailureKind::TimedOut,
                    PreferenceError::Cancelled => LocalFailureKind::Cancelled,
                    PreferenceError::Connection { .. } => LocalFailureKind::DaemonUnavailable,
                    PreferenceError::HttpStatus { .. }
                    | PreferenceError::InvalidResponse { .. }
                    | PreferenceError::InvalidJson { .. } => LocalFailureKind::InvalidOutput,
                },
                operation,
                "local preferences could not be read",
                bounded_detail(&error.to_string()),
                true,
            ),
            Self::PreferenceCommand(error) => LocalFailure::new(
                LocalFailureKind::InvalidOutput,
                operation,
                "local preference request was invalid",
                bounded_detail(&error.to_string()),
                false,
            ),
            Self::Accounts(error) => LocalFailure::new(
                error.failure_kind(),
                operation,
                "local account data was unavailable",
                bounded_detail(&error.to_string()),
                error.retryable(),
            ),
            Self::Policy(error) => LocalFailure::new(
                error.failure_kind(),
                operation,
                "system policy data was unavailable",
                bounded_detail(&error.to_string()),
                error.retryable(),
            ),
        }
    }

    pub fn state(&self, version: &str) -> LocalState {
        match self {
            Self::Process(LocalProcessError::NotFound) => LocalState::ExecutableMissing,
            Self::Process(LocalProcessError::PermissionDenied) => LocalState::ExecutableDenied,
            Self::UnsupportedCommand(_) => LocalState::UnsupportedClient {
                version: version.to_owned(),
                reason: self.to_string(),
            },
            Self::UnsupportedOutput { operation, .. } if operation == "version" => {
                LocalState::UnsupportedClient {
                    version: version.to_owned(),
                    reason: self.to_string(),
                }
            }
            Self::UnsupportedOutput { .. } => LocalState::DaemonUnavailable {
                detail: self.to_string(),
            },
            Self::NonZero { detail, .. } => match classify_stderr(detail) {
                LocalFailureKind::PermissionDenied => LocalState::PermissionDenied {
                    operation: self.operation(),
                    detail: "the operating system denied the operation".to_owned(),
                },
                LocalFailureKind::NeedsLogin => LocalState::NeedsLogin { auth_url: None },
                LocalFailureKind::DaemonUnavailable => LocalState::DaemonUnavailable {
                    detail: "the local daemon could not be reached".to_owned(),
                },
                LocalFailureKind::UnsupportedClient => LocalState::UnsupportedClient {
                    version: version.to_owned(),
                    reason: "the client did not accept the requested command or flag".to_owned(),
                },
                LocalFailureKind::Transport => LocalState::DaemonUnavailable {
                    detail: "the local command returned a transport error".to_owned(),
                },
                _ => LocalState::DaemonUnavailable {
                    detail: "the local command returned an error".to_owned(),
                },
            },
            Self::Process(LocalProcessError::TimedOut) => LocalState::DaemonUnavailable {
                detail: "local command timed out".to_owned(),
            },
            Self::Process(error) => LocalState::DaemonUnavailable {
                detail: bounded_detail(&error.to_string()),
            },
            Self::Preferences(error) => match error {
                PreferenceError::PermissionDenied => LocalState::PermissionDenied {
                    operation: self.operation(),
                    detail: "the LocalAPI denied preference access".to_owned(),
                },
                PreferenceError::UnsupportedPlatform { .. }
                | PreferenceError::UnsupportedVersion { .. }
                | PreferenceError::HttpStatus {
                    status: 404 | 405 | 501,
                } => LocalState::UnsupportedClient {
                    version: version.to_owned(),
                    reason: self.to_string(),
                },
                PreferenceError::InvalidJson { .. } | PreferenceError::InvalidResponse { .. } => {
                    LocalState::DaemonUnavailable {
                        detail: self.to_string(),
                    }
                }
                _ => LocalState::DaemonUnavailable {
                    detail: self.to_string(),
                },
            },
            Self::PreferenceCommand(_) | Self::Accounts(_) | Self::Policy(_) => {
                LocalState::DaemonUnavailable {
                    detail: self.to_string(),
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalClient {
    pub executable: LocalExecutable,
    pub timeout: Duration,
}

impl LocalClient {
    pub fn new(executable: LocalExecutable, timeout: Duration) -> Self {
        Self {
            executable,
            timeout,
        }
    }

    pub async fn status(
        &self,
        observed_at: Timestamp,
        cancellation: &Cancellation,
    ) -> Result<LocalSnapshot, ClientError> {
        if !self.executable.capabilities.status_json {
            return Err(ClientError::UnsupportedCommand("status --json".to_owned()));
        }
        let result = process::run(
            status_command(&self.executable.path, self.timeout),
            cancellation,
        )
        .await
        .map_err(ClientError::Process)?;
        if result.exit_status != Some(0) {
            if !result.stdout.is_empty()
                && let Ok(snapshot) = dto::decode_status(
                    process::decode_utf8(&result.stdout).map_err(ClientError::Process)?,
                    self.executable.version.clone(),
                    self.executable.daemon_version.clone(),
                    observed_at,
                )
            {
                return Ok(snapshot);
            }
            return Err(ClientError::NonZero {
                operation: result.operation.label(),
                status: result.exit_status,
                detail: bounded_output(&result.stderr),
            });
        }
        let stdout = process::decode_utf8(&result.stdout).map_err(ClientError::Process)?;
        dto::decode_status(
            stdout,
            self.executable.version.clone(),
            self.executable.daemon_version.clone(),
            observed_at,
        )
        .map_err(|detail| ClientError::UnsupportedOutput {
            operation: result.operation.label(),
            detail,
        })
    }

    pub async fn version(
        path: &Path,
        timeout: Duration,
        cancellation: &Cancellation,
    ) -> Result<VersionInfo, ClientError> {
        let result = process::run(version_command(path, timeout), cancellation)
            .await
            .map_err(ClientError::Process)?;
        if result.exit_status != Some(0) {
            return Err(ClientError::NonZero {
                operation: result.operation.label(),
                status: result.exit_status,
                detail: bounded_output(&result.stderr),
            });
        }
        let stdout = process::decode_utf8(&result.stdout).map_err(ClientError::Process)?;
        dto::decode_version(stdout).map_err(|detail| ClientError::UnsupportedOutput {
            operation: result.operation.label(),
            detail,
        })
    }

    pub async fn preferences(
        &self,
        observed_at: Timestamp,
        cancellation: &Cancellation,
    ) -> Result<LocalPreferences, ClientError> {
        PreferenceClient::new(
            self.executable.version.clone(),
            PreferencePlatform::current(),
            self.timeout,
        )
        .get_prefs(observed_at, cancellation)
        .await
        .map_err(ClientError::Preferences)
    }

    pub async fn run_command(
        &self,
        command: LocalCommand,
        cancellation: &Cancellation,
    ) -> Result<process::LocalCommandResult, ClientError> {
        let result = process::run(command, cancellation)
            .await
            .map_err(ClientError::Process)?;
        if result.exit_status != Some(0) {
            return Err(ClientError::NonZero {
                operation: result.operation.label(),
                status: result.exit_status,
                detail: bounded_output(&result.stderr),
            });
        }
        Ok(result)
    }

    pub async fn set_preferences(
        &self,
        request: &PreferenceRequest,
        cancellation: &Cancellation,
    ) -> Result<process::LocalCommandResult, ClientError> {
        let command = set_command(&self.executable.path, self.timeout, request)
            .map_err(ClientError::PreferenceCommand)?;
        self.run_command(command, cancellation).await
    }

    pub async fn set_exit_node(
        &self,
        request: &ExitNodeRequest,
        cancellation: &Cancellation,
    ) -> Result<process::LocalCommandResult, ClientError> {
        self.run_command(
            exit_node_command(&self.executable.path, self.timeout, request),
            cancellation,
        )
        .await
    }

    pub async fn set_advertisements(
        &self,
        request: &AdvertisementRequest,
        cancellation: &Cancellation,
    ) -> Result<process::LocalCommandResult, ClientError> {
        let command = advertisement_command(&self.executable.path, self.timeout, request)
            .map_err(ClientError::PreferenceCommand)?;
        self.run_command(command, cancellation).await
    }

    pub async fn discover(
        resolution: ResolvedExecutable,
        timeout: Duration,
        cancellation: &Cancellation,
    ) -> Result<LocalExecutable, ClientError> {
        let version = Self::version(&resolution.path, timeout, cancellation).await?;
        let capabilities = probe_capabilities(&resolution.path, timeout, cancellation).await;
        Ok(LocalExecutable {
            path: resolution.path,
            source: resolution.source,
            version: version.version,
            daemon_version: version.daemon_version,
            build: version.build,
            capabilities,
        })
    }
}

pub fn version_command(path: &Path, timeout: Duration) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::Version,
        vec![
            OsString::from("version"),
            OsString::from("--json"),
            OsString::from("--daemon"),
        ],
    )
    .with_timeout(timeout)
}

pub fn status_command(path: &Path, timeout: Duration) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::Status,
        vec![OsString::from("status"), OsString::from("--json")],
    )
    .with_timeout(timeout)
}

pub fn up_command(path: &Path, timeout: Duration) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::Up,
        vec![OsString::from("up")],
    )
    .with_timeout(timeout)
}

pub fn down_command(path: &Path, timeout: Duration, accept_lose_ssh: bool) -> LocalCommand {
    let mut args = vec![OsString::from("down")];
    if accept_lose_ssh {
        args.push(OsString::from("--accept-risk=lose-ssh"));
    }
    LocalCommand::new(path.as_os_str().to_os_string(), LocalOperation::Down, args)
        .with_timeout(timeout)
}

pub fn resolve_executable(
    input: &ExecutableResolution,
) -> Result<ResolvedExecutable, ExecutableError> {
    if let Some(path) = input.cli_path.as_deref() {
        return resolve_explicit(path, ExecutableSource::Cli, input.platform);
    }
    if let Some(path) = input.environment_path.as_deref() {
        let path = PathBuf::from(path);
        return resolve_explicit_or_path(&path, ExecutableSource::Environment, input);
    }
    if let Some(path) = input.config_path.as_deref() {
        return resolve_explicit_or_path(path, ExecutableSource::Config, input);
    }
    search_path(
        OsStr::new("tailscale"),
        input.path.as_deref(),
        input.platform,
    )
}

pub fn process_resolution(
    cli_path: Option<PathBuf>,
    environment_path: Option<OsString>,
    config_path: Option<PathBuf>,
) -> ExecutableResolution {
    ExecutableResolution {
        cli_path,
        environment_path,
        config_path,
        path: std::env::var_os("PATH"),
        platform: if cfg!(windows) {
            HostPlatform::Windows
        } else {
            HostPlatform::Unix
        },
    }
}

async fn probe_capabilities(
    path: &Path,
    timeout: Duration,
    cancellation: &Cancellation,
) -> LocalCapabilities {
    let status = help_available(path, "status", timeout, cancellation).await;
    let ping = help_available(path, "ping", timeout, cancellation).await;
    let netcheck = help_available(path, "netcheck", timeout, cancellation).await;
    let dns_status = help_available(path, "dns status", timeout, cancellation).await;
    let dns_query = help_available(path, "dns query", timeout, cancellation).await;
    let whois = help_available(path, "whois", timeout, cancellation).await;
    let connect = help_available(path, "up", timeout, cancellation).await;
    let disconnect = help_available(path, "down", timeout, cancellation).await;
    let set = help_available(path, "set", timeout, cancellation).await;
    let accounts = help_available(path, "switch", timeout, cancellation).await;
    let account_login = help_available(path, "login", timeout, cancellation).await;
    let account_logout = help_available(path, "logout", timeout, cancellation).await;
    let account_remove = help_available(path, "switch remove", timeout, cancellation).await;
    let syspolicy = help_available(path, "syspolicy", timeout, cancellation).await;
    let ssh = help_available(path, "ssh", timeout, cancellation).await;
    let nc = help_available(path, "nc", timeout, cancellation).await;
    LocalCapabilities {
        status_json: status,
        ping,
        netcheck_json: netcheck,
        netcheck_json_line: netcheck,
        dns_status_json: dns_status,
        dns_query_json: dns_query,
        whois_json: whois,
        connect,
        disconnect,
        set,
        accounts,
        account_login,
        account_logout,
        account_remove,
        syspolicy,
        ssh,
        nc,
    }
}

async fn help_available(
    path: &Path,
    command: &str,
    timeout: Duration,
    cancellation: &Cancellation,
) -> bool {
    let mut args = command
        .split_ascii_whitespace()
        .map(OsString::from)
        .collect::<Vec<_>>();
    args.push(OsString::from("--help"));
    let operation = LocalOperation::Help(command.to_owned());
    let command = LocalCommand::new(path.as_os_str().to_os_string(), operation, args)
        .with_timeout(timeout)
        .with_limits(64 * 1024, 64 * 1024);
    match process::run(command, cancellation).await {
        Ok(result) => result.exit_status == Some(0),
        Err(_) => false,
    }
}

fn resolve_explicit_or_path(
    path: &Path,
    source: ExecutableSource,
    input: &ExecutableResolution,
) -> Result<ResolvedExecutable, ExecutableError> {
    if path.is_absolute() || path.components().count() > 1 {
        return resolve_explicit(path, source, input.platform);
    }
    search_path(path.as_os_str(), input.path.as_deref(), input.platform)
        .map(|resolved| ResolvedExecutable { source, ..resolved })
}

fn resolve_explicit(
    path: &Path,
    source: ExecutableSource,
    platform: HostPlatform,
) -> Result<ResolvedExecutable, ExecutableError> {
    if path.as_os_str().is_empty() {
        return Err(ExecutableError::InvalidPath);
    }
    let candidate = executable_candidate(path, platform).ok_or(ExecutableError::NotFound)?;
    if !is_executable(&candidate, platform) {
        return Err(ExecutableError::PermissionDenied);
    }
    Ok(ResolvedExecutable {
        path: candidate,
        source,
    })
}

fn search_path(
    name: &OsStr,
    path_value: Option<&OsStr>,
    platform: HostPlatform,
) -> Result<ResolvedExecutable, ExecutableError> {
    let Some(path_value) = path_value else {
        return Err(ExecutableError::NotFound);
    };
    let mut denied = false;
    for directory in std::env::split_paths(path_value) {
        let Some(candidate) = executable_candidate(&directory.join(name), platform) else {
            continue;
        };
        if is_executable(&candidate, platform) {
            return Ok(ResolvedExecutable {
                path: candidate,
                source: ExecutableSource::Path,
            });
        }
        denied = true;
    }
    if denied {
        Err(ExecutableError::PermissionDenied)
    } else {
        Err(ExecutableError::NotFound)
    }
}

fn executable_candidate(path: &Path, platform: HostPlatform) -> Option<PathBuf> {
    if fs::metadata(path).is_ok() {
        return Some(path.to_path_buf());
    }
    if platform == HostPlatform::Windows && path.extension().is_none() {
        let exe = path.with_extension("exe");
        if fs::metadata(&exe).is_ok() {
            return Some(exe);
        }
    }
    None
}

fn is_executable(path: &Path, platform: HostPlatform) -> bool {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return false,
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    if platform == HostPlatform::Unix {
        use std::os::unix::fs::PermissionsExt;
        return metadata.permissions().mode() & 0o111 != 0;
    }
    let _ = platform;
    true
}

fn classify_stderr(detail: &str) -> LocalFailureKind {
    let normalized = detail.trim().to_ascii_lowercase();
    if normalized.contains("unknown flag")
        || normalized.contains("flag provided but not defined")
        || normalized.contains("unknown command")
    {
        LocalFailureKind::UnsupportedClient
    } else if normalized.contains("permission denied") || normalized.contains("not permitted") {
        LocalFailureKind::PermissionDenied
    } else if normalized.contains("not logged in")
        || normalized.contains("logged out")
        || normalized.contains("login required")
    {
        LocalFailureKind::NeedsLogin
    } else if normalized.contains("cannot connect")
        || normalized.contains("daemon unavailable")
        || normalized.contains("daemon is not running")
        || normalized.contains("failed to connect")
    {
        LocalFailureKind::DaemonUnavailable
    } else {
        LocalFailureKind::Transport
    }
}

fn bounded_output(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    bounded_detail(&text)
}

fn bounded_detail(value: &str) -> String {
    const LIMIT: usize = 4096;
    if value.len() <= LIMIT {
        value.to_owned()
    } else {
        let mut end = 0;
        for (index, character) in value.char_indices() {
            if index.saturating_add(character.len_utf8()) > LIMIT {
                break;
            }
            end = index.saturating_add(character.len_utf8());
        }
        format!("{}\n...[detail truncated]", &value[..end])
    }
}
