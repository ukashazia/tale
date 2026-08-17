use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;

use crate::domain::preference::PreferenceRequest;
use crate::domain::route::{
    AdvertisementRequest, ExitNodeRequest, ExitNodeSelection, format_route_set,
    format_static_endpoints,
};
use crate::domain::source::{
    ExecutableSource, LocalCapabilities, LocalExecutable, LocalFailure, LocalFailureKind,
    LocalState,
};

use super::accounts::AccountError;
use super::dto;
use super::policy::PolicyError;
use super::process::{self, Cancellation, LocalCommand, LocalOperation, LocalProcessError};

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum PreferenceCommandError {
    #[error("no preference fields were changed")]
    EmptyRequest,
    #[error("enabling the app connector requires explicit mac-app-connector risk acceptance")]
    MissingMacAppConnectorRisk,
    #[error("mac-app-connector risk acceptance requires connector=true")]
    UnexpectedMacAppConnectorRisk,
    #[error("{field} must be non-empty")]
    EmptyText { field: String },
}

pub fn set_command(
    path: &Path,
    timeout: Duration,
    request: &PreferenceRequest,
) -> Result<LocalCommand, PreferenceCommandError> {
    if request.is_empty() {
        return Err(PreferenceCommandError::EmptyRequest);
    }
    let mut args = vec![OsString::from("set")];
    if let Some(value) = request.accept_dns {
        args.push(OsString::from(format!("--accept-dns={value}")));
    }
    if let Some(value) = request.accept_routes {
        args.push(OsString::from(format!("--accept-routes={value}")));
    }
    if let Some(value) = request.shields_up {
        args.push(OsString::from(format!("--shields-up={value}")));
    }
    if let Some(value) = request.ssh {
        args.push(OsString::from(format!("--ssh={value}")));
    }
    if let Some(value) = request.automatic_update {
        args.push(OsString::from(format!("--auto-update={value}")));
    }
    if let Some(value) = request.update_check {
        args.push(OsString::from(format!("--update-check={value}")));
    }
    if let Some(value) = request.report_posture {
        args.push(OsString::from(format!("--report-posture={value}")));
    }
    if let Some(value) = request.hostname.as_deref() {
        validate_text(value, "hostname")?;
        args.push(OsString::from(format!("--hostname={value}")));
    }
    if let Some(value) = request.nickname.as_deref() {
        validate_text(value, "nickname")?;
        args.push(OsString::from(format!("--nickname={value}")));
    }
    if let Some(value) = request.web_client {
        args.push(OsString::from(format!("--webclient={value}")));
    }
    Ok(
        LocalCommand::new(path.as_os_str().to_os_string(), LocalOperation::Set, args)
            .with_timeout(timeout),
    )
}

pub fn exit_node_command(
    path: &Path,
    timeout: Duration,
    request: &ExitNodeRequest,
) -> LocalCommand {
    let target = match &request.selection {
        ExitNodeSelection::None => String::new(),
        ExitNodeSelection::Device { target, .. } => target.clone(),
        ExitNodeSelection::AutoAny => "auto:any".to_owned(),
    };
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::Set,
        vec![
            OsString::from("set"),
            OsString::from(format!("--exit-node={target}")),
            OsString::from(format!(
                "--exit-node-allow-lan-access={}",
                request.allow_lan_access && !matches!(&request.selection, ExitNodeSelection::None)
            )),
        ],
    )
    .with_timeout(timeout)
}

pub fn advertisement_command(
    path: &Path,
    timeout: Duration,
    request: &AdvertisementRequest,
) -> Result<LocalCommand, PreferenceCommandError> {
    if request.is_empty() {
        return Err(PreferenceCommandError::EmptyRequest);
    }
    if request.advertise_connector == Some(true) && !request.accept_mac_app_connector_risk {
        return Err(PreferenceCommandError::MissingMacAppConnectorRisk);
    }
    if request.accept_mac_app_connector_risk && request.advertise_connector != Some(true) {
        return Err(PreferenceCommandError::UnexpectedMacAppConnectorRisk);
    }
    let mut args = vec![OsString::from("set")];
    if let Some(routes) = request.canonical_routes() {
        args.push(OsString::from(format!(
            "--advertise-routes={}",
            format_route_set(&routes)
        )));
    }
    if let Some(value) = request.advertise_exit_node {
        args.push(OsString::from(format!("--advertise-exit-node={value}")));
    }
    if let Some(value) = request.advertise_connector {
        args.push(OsString::from(format!("--advertise-connector={value}")));
        if value && request.accept_mac_app_connector_risk {
            args.push(OsString::from("--accept-risk=mac-app-connector"));
        }
    }
    if let Some(port) = request.relay_server_port {
        let value = port.map_or_else(String::new, |port| port.to_string());
        args.push(OsString::from(format!("--relay-server-port={value}")));
    }
    if let Some(endpoints) = request.relay_server_static_endpoints.as_deref() {
        args.push(OsString::from(format!(
            "--relay-server-static-endpoints={}",
            format_static_endpoints(endpoints)
        )));
    }
    Ok(
        LocalCommand::new(path.as_os_str().to_os_string(), LocalOperation::Set, args)
            .with_timeout(timeout),
    )
}

fn validate_text(value: &str, field: &str) -> Result<(), PreferenceCommandError> {
    if value.is_empty() {
        Err(PreferenceCommandError::EmptyText {
            field: field.to_owned(),
        })
    } else {
        Ok(())
    }
}

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
    pub socket_path: Option<PathBuf>,
    pub path: Option<OsString>,
    pub platform: HostPlatform,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResolvedExecutable {
    pub path: PathBuf,
    pub socket_path: Option<PathBuf>,
    pub source: ExecutableSource,
}

#[derive(Error, Debug, Clone, Eq, PartialEq)]
pub enum ExecutableError {
    /// Carries every location that was checked, so the failure can say where
    /// Tale looked instead of only that it looked.
    #[error("tailscale executable was not found")]
    NotFound { searched: Vec<PathBuf> },
    #[error("tailscale executable is not executable")]
    PermissionDenied { path: PathBuf },
    #[error("tailscale executable path is invalid")]
    InvalidPath,
}

impl ExecutableError {
    /// Where Tale looked, in the order it looked.
    pub fn searched(&self) -> Vec<String> {
        match self {
            Self::NotFound { searched } => searched
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            Self::PermissionDenied { path } => vec![path.display().to_string()],
            Self::InvalidPath => Vec::new(),
        }
    }
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
            Self::Accounts(_) | Self::Policy(_) => LocalState::DaemonUnavailable {
                detail: self.to_string(),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalCliClient {
    pub executable: LocalExecutable,
    pub timeout: Duration,
}

impl LocalCliClient {
    pub fn new(executable: LocalExecutable, timeout: Duration) -> Self {
        Self {
            executable,
            timeout,
        }
    }

    pub async fn version(
        path: &Path,
        timeout: Duration,
        cancellation: &Cancellation,
        socket_path: Option<&Path>,
    ) -> Result<VersionInfo, ClientError> {
        let command = apply_socket_path(version_command(path, timeout), socket_path);
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
        let stdout = process::decode_utf8(&result.stdout).map_err(ClientError::Process)?;
        dto::decode_version(stdout).map_err(|detail| ClientError::UnsupportedOutput {
            operation: result.operation.label(),
            detail,
        })
    }

    pub async fn run_command(
        &self,
        command: LocalCommand,
        cancellation: &Cancellation,
    ) -> Result<process::LocalCommandResult, ClientError> {
        let command = apply_socket_path(command, self.executable.socket_path.as_deref());
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
        let command =
            set_command(&self.executable.path, self.timeout, request).map_err(|error| {
                ClientError::UnsupportedOutput {
                    operation: "preferences".to_owned(),
                    detail: error.to_string(),
                }
            })?;
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
        let command = advertisement_command(&self.executable.path, self.timeout, request).map_err(
            |error| ClientError::UnsupportedOutput {
                operation: "advertisements".to_owned(),
                detail: error.to_string(),
            },
        )?;
        self.run_command(command, cancellation).await
    }

    pub async fn discover(
        resolution: ResolvedExecutable,
        timeout: Duration,
        cancellation: &Cancellation,
    ) -> Result<LocalExecutable, ClientError> {
        let version = Self::version(
            &resolution.path,
            timeout,
            cancellation,
            resolution.socket_path.as_deref(),
        )
        .await?;
        let capabilities = probe_capabilities(
            &resolution.path,
            timeout,
            cancellation,
            resolution.socket_path.as_deref(),
        )
        .await;
        Ok(LocalExecutable {
            path: resolution.path,
            socket_path: resolution.socket_path,
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

fn apply_socket_path(mut command: LocalCommand, socket_path: Option<&Path>) -> LocalCommand {
    if let Some(socket_path) = socket_path {
        command = command.with_socket_path(socket_path.as_os_str().to_os_string());
    }
    command
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
    let result = if let Some(path) = input.cli_path.as_deref() {
        resolve_explicit(path, ExecutableSource::Cli, input.platform)
    } else if let Some(path) = input.environment_path.as_deref() {
        let path = PathBuf::from(path);
        resolve_explicit_or_path(&path, ExecutableSource::Environment, input)
    } else if let Some(path) = input.config_path.as_deref() {
        resolve_explicit_or_path(path, ExecutableSource::Config, input)
    } else {
        search_path(
            OsStr::new("tailscale"),
            input.path.as_deref(),
            input.platform,
        )
    }?;
    Ok(ResolvedExecutable {
        socket_path: input.socket_path.clone(),
        ..result
    })
}

pub fn process_resolution(
    cli_path: Option<PathBuf>,
    environment_path: Option<OsString>,
    config_path: Option<PathBuf>,
    socket_path: Option<PathBuf>,
) -> ExecutableResolution {
    ExecutableResolution {
        cli_path,
        environment_path,
        config_path,
        socket_path,
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
    socket_path: Option<&Path>,
) -> LocalCapabilities {
    let ping = help_available(path, "ping", timeout, cancellation, socket_path).await;
    let netcheck = help_available(path, "netcheck", timeout, cancellation, socket_path).await;
    let dns_status = help_available(path, "dns status", timeout, cancellation, socket_path).await;
    let dns_query = help_available(path, "dns query", timeout, cancellation, socket_path).await;
    let whois = help_available(path, "whois", timeout, cancellation, socket_path).await;
    let connect = help_available(path, "up", timeout, cancellation, socket_path).await;
    let disconnect = help_available(path, "down", timeout, cancellation, socket_path).await;
    let set = help_available(path, "set", timeout, cancellation, socket_path).await;
    let accounts = help_available(path, "switch", timeout, cancellation, socket_path).await;
    let account_login = help_available(path, "login", timeout, cancellation, socket_path).await;
    let account_logout = help_available(path, "logout", timeout, cancellation, socket_path).await;
    let account_remove =
        help_available(path, "switch remove", timeout, cancellation, socket_path).await;
    let syspolicy = help_available(path, "syspolicy", timeout, cancellation, socket_path).await;
    let ssh = help_available(path, "ssh", timeout, cancellation, socket_path).await;
    let nc = help_available(path, "nc", timeout, cancellation, socket_path).await;
    let serve = help_available(path, "serve", timeout, cancellation, socket_path).await
        && help_available(path, "serve status", timeout, cancellation, socket_path).await;
    let funnel = help_available(path, "funnel", timeout, cancellation, socket_path).await
        && help_available(path, "funnel status", timeout, cancellation, socket_path).await;
    let taildrop = help_available(path, "file cp", timeout, cancellation, socket_path).await
        && help_available(path, "file get", timeout, cancellation, socket_path).await;
    let drive = help_available(path, "drive list", timeout, cancellation, socket_path).await
        && help_available(path, "drive share", timeout, cancellation, socket_path).await
        && help_available(path, "drive rename", timeout, cancellation, socket_path).await
        && help_available(path, "drive unshare", timeout, cancellation, socket_path).await;
    let certificate = help_available(path, "cert", timeout, cancellation, socket_path).await;
    let metrics = help_available(path, "metrics", timeout, cancellation, socket_path).await;
    let bugreport = help_available(path, "bugreport", timeout, cancellation, socket_path).await;
    LocalCapabilities {
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
        serve,
        funnel,
        taildrop,
        drive,
        certificate,
        metrics,
        bugreport,
    }
}

async fn help_available(
    path: &Path,
    command: &str,
    timeout: Duration,
    cancellation: &Cancellation,
    socket_path: Option<&Path>,
) -> bool {
    help_output(path, command, timeout, cancellation, socket_path)
        .await
        .is_some()
}

async fn help_output(
    path: &Path,
    command: &str,
    timeout: Duration,
    cancellation: &Cancellation,
    socket_path: Option<&Path>,
) -> Option<String> {
    let mut args = command
        .split_ascii_whitespace()
        .map(OsString::from)
        .collect::<Vec<_>>();
    args.push(OsString::from("--help"));
    let operation = LocalOperation::Help(command.to_owned());
    let command = apply_socket_path(
        LocalCommand::new(path.as_os_str().to_os_string(), operation, args)
            .with_timeout(timeout)
            .with_limits(64 * 1024, 64 * 1024),
        socket_path,
    );
    match process::run(command, cancellation).await {
        Ok(result) if result.exit_status == Some(0) => {
            Some(process::decode_utf8(&result.stdout).map_or_else(|_| String::new(), str::to_owned))
        }
        _ => None,
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
    let candidate =
        executable_candidate(path, platform).ok_or_else(|| ExecutableError::NotFound {
            searched: vec![path.to_path_buf()],
        })?;
    if !is_executable(&candidate, platform) {
        return Err(ExecutableError::PermissionDenied { path: candidate });
    }
    Ok(ResolvedExecutable {
        path: candidate,
        socket_path: None,
        source,
    })
}

fn search_path(
    name: &OsStr,
    path_value: Option<&OsStr>,
    platform: HostPlatform,
) -> Result<ResolvedExecutable, ExecutableError> {
    let Some(path_value) = path_value else {
        return Err(ExecutableError::NotFound {
            searched: Vec::new(),
        });
    };
    let mut denied = None;
    let mut searched = Vec::new();
    for directory in std::env::split_paths(path_value) {
        let target = directory.join(name);
        searched.push(target.clone());
        let Some(candidate) = executable_candidate(&target, platform) else {
            continue;
        };
        if is_executable(&candidate, platform) {
            return Ok(ResolvedExecutable {
                path: candidate,
                socket_path: None,
                source: ExecutableSource::Path,
            });
        }
        denied = Some(candidate);
    }
    denied.map_or(Err(ExecutableError::NotFound { searched }), |path| {
        Err(ExecutableError::PermissionDenied { path })
    })
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
    crate::detail::bounded_prefix_bytes(value, 4096)
}
