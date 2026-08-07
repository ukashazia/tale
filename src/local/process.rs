use std::ffi::OsString;
use std::fmt;
use std::io;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

const READ_CHUNK: usize = 8 * 1024;
const CANCEL_POLL: Duration = Duration::from_millis(10);
const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LocalOperation {
    Version,
    Help(String),
    Up,
    Down,
    Set,
    SwitchList,
    Switch,
    SwitchRemove,
    Login,
    Logout,
    Ssh,
    Nc,
    SyspolicyList,
    SyspolicyReload,
    Ping,
    Netcheck,
    DnsStatus,
    DnsQuery,
    Whois,
    ServeStatus,
    Serve,
    ServeOff,
    ServeReset,
    FunnelStatus,
    Funnel,
    FunnelReset,
    TaildropTargets,
    TaildropSend,
    TaildropReceive,
    TaildriveList,
    TaildriveShare,
    TaildriveRename,
    TaildriveUnshare,
    Certificate,
    Metrics,
    BugReport,
}

impl LocalOperation {
    pub fn label(&self) -> String {
        match self {
            Self::Version => "version".to_owned(),
            Self::Help(command) => format!("{command} help"),
            Self::Up => "up".to_owned(),
            Self::Down => "down".to_owned(),
            Self::Set => "set".to_owned(),
            Self::SwitchList => "switch --list".to_owned(),
            Self::Switch => "switch".to_owned(),
            Self::SwitchRemove => "switch remove".to_owned(),
            Self::Login => "login".to_owned(),
            Self::Logout => "logout".to_owned(),
            Self::Ssh => "ssh".to_owned(),
            Self::Nc => "nc".to_owned(),
            Self::SyspolicyList => "syspolicy list".to_owned(),
            Self::SyspolicyReload => "syspolicy reload".to_owned(),
            Self::Ping => "ping".to_owned(),
            Self::Netcheck => "netcheck".to_owned(),
            Self::DnsStatus => "dns status".to_owned(),
            Self::DnsQuery => "dns query".to_owned(),
            Self::Whois => "whois".to_owned(),
            Self::ServeStatus => "serve status".to_owned(),
            Self::Serve => "serve".to_owned(),
            Self::ServeOff => "serve off".to_owned(),
            Self::ServeReset => "serve reset".to_owned(),
            Self::FunnelStatus => "funnel status".to_owned(),
            Self::Funnel => "funnel".to_owned(),
            Self::FunnelReset => "funnel reset".to_owned(),
            Self::TaildropTargets => "file cp --targets".to_owned(),
            Self::TaildropSend => "file cp".to_owned(),
            Self::TaildropReceive => "file get".to_owned(),
            Self::TaildriveList => "drive list".to_owned(),
            Self::TaildriveShare => "drive share".to_owned(),
            Self::TaildriveRename => "drive rename".to_owned(),
            Self::TaildriveUnshare => "drive unshare".to_owned(),
            Self::Certificate => "cert".to_owned(),
            Self::Metrics => "metrics print".to_owned(),
            Self::BugReport => "bugreport".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StdinMode {
    Closed,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum OutputMode {
    Collect,
    Lines,
}

#[derive(Clone)]
pub struct LocalCommand {
    pub executable: OsString,
    pub operation: LocalOperation,
    pub args: Vec<OsString>,
    pub socket_path: Option<OsString>,
    pub stdin: StdinMode,
    pub stdout_mode: OutputMode,
    pub stderr_mode: OutputMode,
    pub timeout: Option<Duration>,
    pub stdout_limit: usize,
    pub stderr_limit: usize,
    pub redactions: Vec<usize>,
}

impl fmt::Debug for LocalCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalCommand")
            .field("operation", &self.operation)
            .field("arg_count", &self.args.len())
            .field("has_socket_path", &self.socket_path.is_some())
            .field("stdin", &self.stdin)
            .field("stdout_mode", &self.stdout_mode)
            .field("stderr_mode", &self.stderr_mode)
            .field("timeout", &self.timeout)
            .field("stdout_limit", &self.stdout_limit)
            .field("stderr_limit", &self.stderr_limit)
            .field("redacted_arg_count", &self.redactions.len())
            .finish()
    }
}

impl LocalCommand {
    pub fn new(
        executable: impl Into<OsString>,
        operation: LocalOperation,
        args: Vec<OsString>,
    ) -> Self {
        Self {
            executable: executable.into(),
            operation,
            args,
            socket_path: None,
            stdin: StdinMode::Closed,
            stdout_mode: OutputMode::Collect,
            stderr_mode: OutputMode::Collect,
            timeout: Some(Duration::from_secs(10)),
            stdout_limit: 4 * 1024 * 1024,
            stderr_limit: 256 * 1024,
            redactions: Vec::new(),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn without_timeout(mut self) -> Self {
        self.timeout = None;
        self
    }

    pub fn with_modes(mut self, stdout_mode: OutputMode, stderr_mode: OutputMode) -> Self {
        self.stdout_mode = stdout_mode;
        self.stderr_mode = stderr_mode;
        self
    }

    pub fn with_limits(mut self, stdout_limit: usize, stderr_limit: usize) -> Self {
        self.stdout_limit = stdout_limit;
        self.stderr_limit = stderr_limit;
        self
    }

    pub fn redact_arg(mut self, index: usize) -> Self {
        self.redactions.push(index);
        self
    }

    pub fn with_socket_path(mut self, path: impl Into<OsString>) -> Self {
        self.socket_path = Some(path.into());
        self
    }
}

#[derive(Clone, Default)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
}

impl Cancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Error, Debug, Clone, Eq, PartialEq)]
pub enum LocalProcessError {
    #[error("executable not found")]
    NotFound,
    #[error("permission denied while starting executable")]
    PermissionDenied,
    #[error("could not start local command: {0}")]
    Spawn(String),
    #[error("local command timed out")]
    TimedOut,
    #[error("local command cancelled")]
    Cancelled,
    #[error("local command I/O failed: {0}")]
    Io(String),
    #[error("local command output was not UTF-8: {0}")]
    OutputNotUtf8(String),
}

#[derive(Clone, Eq, PartialEq)]
pub struct LocalCommandResult {
    pub operation: LocalOperation,
    pub exit_status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub started_at: Instant,
    pub finished_at: Instant,
    pub truncated_stdout: bool,
    pub truncated_stderr: bool,
}

impl fmt::Debug for LocalCommandResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalCommandResult")
            .field("operation", &self.operation)
            .field("exit_status", &self.exit_status)
            .field("stdout_len", &self.stdout.len())
            .field("stderr_len", &self.stderr.len())
            .field("started_at", &self.started_at)
            .field("finished_at", &self.finished_at)
            .field("truncated_stdout", &self.truncated_stdout)
            .field("truncated_stderr", &self.truncated_stderr)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProcessLine {
    pub stream: OutputStream,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProcessLinesResult {
    pub result: LocalCommandResult,
    pub invalid_utf8: bool,
}

pub async fn run(
    command: LocalCommand,
    cancellation: &Cancellation,
) -> Result<LocalCommandResult, LocalProcessError> {
    let started_at = Instant::now();
    let mut child = spawn(&command)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LocalProcessError::Io("stdout pipe was not available".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| LocalProcessError::Io("stderr pipe was not available".to_owned()))?;
    let stdout_task = tokio::spawn(read_limited(stdout, command.stdout_limit));
    let stderr_task = tokio::spawn(read_limited(stderr, command.stderr_limit));
    let status = wait_for_child(&mut child, command.timeout, cancellation).await;
    let stdout = join_capture(stdout_task).await;
    let stderr = join_capture(stderr_task).await;
    let (status, stdout, stderr) = match (status, stdout, stderr) {
        (Ok(status), Ok(stdout), Ok(stderr)) => (status, stdout, stderr),
        (Err(error), _, _) => return Err(error),
        (_, Err(error), _) => return Err(error),
        (_, _, Err(error)) => return Err(error),
    };
    let finished_at = Instant::now();
    Ok(LocalCommandResult {
        operation: command.operation,
        exit_status: status.code(),
        stdout: stdout.bytes,
        stderr: stderr.bytes,
        started_at,
        finished_at,
        truncated_stdout: stdout.truncated,
        truncated_stderr: stderr.truncated,
    })
}

pub async fn run_lines(
    command: LocalCommand,
    cancellation: &Cancellation,
    sender: mpsc::Sender<ProcessLine>,
) -> Result<ProcessLinesResult, LocalProcessError> {
    let started_at = Instant::now();
    let mut child = spawn(&command)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LocalProcessError::Io("stdout pipe was not available".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| LocalProcessError::Io("stderr pipe was not available".to_owned()))?;
    let stdout_task = tokio::spawn(read_lines(
        stdout,
        OutputStream::Stdout,
        command.stdout_limit,
        sender.clone(),
    ));
    let stderr_task = tokio::spawn(read_lines(
        stderr,
        OutputStream::Stderr,
        command.stderr_limit,
        sender,
    ));
    let status = wait_for_child(&mut child, command.timeout, cancellation).await;
    let stdout = join_capture(stdout_task).await;
    let stderr = join_capture(stderr_task).await;
    let (status, stdout, stderr) = match (status, stdout, stderr) {
        (Ok(status), Ok(stdout), Ok(stderr)) => (status, stdout, stderr),
        (Err(error), _, _) => return Err(error),
        (_, Err(error), _) => return Err(error),
        (_, _, Err(error)) => return Err(error),
    };
    let finished_at = Instant::now();
    Ok(ProcessLinesResult {
        result: LocalCommandResult {
            operation: command.operation,
            exit_status: status.code(),
            stdout: stdout.bytes,
            stderr: stderr.bytes,
            started_at,
            finished_at,
            truncated_stdout: stdout.truncated,
            truncated_stderr: stderr.truncated,
        },
        invalid_utf8: stdout.invalid_utf8 || stderr.invalid_utf8,
    })
}

pub fn decode_utf8(bytes: &[u8]) -> Result<&str, LocalProcessError> {
    std::str::from_utf8(bytes).map_err(|error| {
        let start = error.valid_up_to();
        let context =
            bytes
                .get(start..start.saturating_add(16))
                .map_or_else(String::new, |value| {
                    value
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<Vec<_>>()
                        .join("")
                });
        LocalProcessError::OutputNotUtf8(context)
    })
}

fn spawn(command: &LocalCommand) -> Result<Child, LocalProcessError> {
    let mut process = Command::new(&command.executable);
    if let Some(socket_path) = command.socket_path.as_ref() {
        process.arg("--socket").arg(socket_path);
    }
    process
        .args(&command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process.spawn().map_err(classify_spawn_error)
}

fn classify_spawn_error(error: io::Error) -> LocalProcessError {
    match error.kind() {
        io::ErrorKind::NotFound => LocalProcessError::NotFound,
        io::ErrorKind::PermissionDenied => LocalProcessError::PermissionDenied,
        _ => LocalProcessError::Spawn(error.to_string()),
    }
}

async fn wait_for_child(
    child: &mut Child,
    timeout: Option<Duration>,
    cancellation: &Cancellation,
) -> Result<ExitStatus, LocalProcessError> {
    let deadline = timeout.map(tokio::time::sleep);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            status = child.wait() => {
                return status.map_err(|error| LocalProcessError::Io(error.to_string()));
            }
            () = async {
                if let Some(deadline) = deadline.as_mut().as_pin_mut() {
                    deadline.await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(LocalProcessError::TimedOut);
            }
            () = tokio::time::sleep(CANCEL_POLL) => {
                if cancellation.is_cancelled() {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return Err(LocalProcessError::Cancelled);
                }
            }
        }
    }
}

#[derive(Debug)]
struct Capture {
    bytes: Vec<u8>,
    truncated: bool,
    invalid_utf8: bool,
}

async fn read_limited<R>(mut reader: R, limit: usize) -> io::Result<Capture>
where
    R: AsyncRead + Unpin,
{
    let limit = limit.min(MAX_CAPTURE_BYTES);
    let mut bytes = Vec::with_capacity(limit.min(READ_CHUNK));
    let mut buffer = [0_u8; READ_CHUNK];
    let mut truncated = false;
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        if bytes.len() < limit {
            let remaining = limit.saturating_sub(bytes.len());
            let retained = count.min(remaining);
            bytes.extend_from_slice(&buffer[..retained]);
            if retained < count {
                truncated = true;
            }
        } else {
            truncated = true;
        }
    }
    let invalid_utf8 = std::str::from_utf8(&bytes).is_err();
    Ok(Capture {
        bytes,
        truncated,
        invalid_utf8,
    })
}

async fn read_lines<R>(
    mut reader: R,
    stream: OutputStream,
    limit: usize,
    sender: mpsc::Sender<ProcessLine>,
) -> io::Result<Capture>
where
    R: AsyncRead + Unpin,
{
    let limit = limit.min(MAX_CAPTURE_BYTES);
    let mut bytes = Vec::with_capacity(limit.min(READ_CHUNK));
    let mut line = Vec::with_capacity(limit.min(READ_CHUNK));
    let mut buffer = [0_u8; READ_CHUNK];
    let mut truncated = false;
    let mut invalid_utf8 = false;
    'read: loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        for byte in &buffer[..count] {
            if bytes.len() < limit {
                bytes.push(*byte);
            } else {
                truncated = true;
            }
            if line.len() < limit {
                line.push(*byte);
            } else {
                truncated = true;
            }
            if *byte == b'\n' {
                if std::str::from_utf8(&line).is_err() {
                    invalid_utf8 = true;
                }
                if sender
                    .send(ProcessLine {
                        stream,
                        bytes: line.clone(),
                    })
                    .await
                    .is_err()
                {
                    break 'read;
                }
                line.clear();
            }
        }
    }
    if !line.is_empty()
        && sender
            .send(ProcessLine {
                stream,
                bytes: line,
            })
            .await
            .is_err()
    {
        truncated = true;
    }
    if std::str::from_utf8(&bytes).is_err() {
        invalid_utf8 = true;
    }
    Ok(Capture {
        bytes,
        truncated,
        invalid_utf8,
    })
}

async fn join_capture(
    handle: tokio::task::JoinHandle<io::Result<Capture>>,
) -> Result<Capture, LocalProcessError> {
    match handle.await {
        Ok(Ok(capture)) => Ok(capture),
        Ok(Err(error)) => Err(LocalProcessError::Io(error.to_string())),
        Err(error) => Err(LocalProcessError::Io(error.to_string())),
    }
}
