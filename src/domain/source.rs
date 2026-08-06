use std::path::PathBuf;

use super::Timestamp;
use super::device::LocalDevice;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ExecutableSource {
    Cli,
    Environment,
    Config,
    Path,
}

impl ExecutableSource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Environment => "environment",
            Self::Config => "config",
            Self::Path => "PATH",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct LocalCapabilities {
    pub ping: bool,
    pub netcheck_json: bool,
    pub netcheck_json_line: bool,
    pub dns_status_json: bool,
    pub dns_query_json: bool,
    pub whois_json: bool,
    pub connect: bool,
    pub disconnect: bool,
    pub set: bool,
    pub accounts: bool,
    pub account_login: bool,
    pub account_logout: bool,
    pub account_remove: bool,
    pub syspolicy: bool,
    pub ssh: bool,
    pub nc: bool,
    pub serve: bool,
    pub serve_https: bool,
    pub serve_http: bool,
    pub serve_tcp: bool,
    pub serve_tls_terminated_tcp: bool,
    pub funnel: bool,
    pub funnel_https: bool,
    pub funnel_tcp: bool,
    pub funnel_tls_terminated_tcp: bool,
    pub taildrop: bool,
    pub drive: bool,
    pub certificate: bool,
    pub metrics: bool,
    pub bugreport: bool,
}

impl LocalCapabilities {
    pub const fn all_supported() -> Self {
        Self {
            ping: true,
            netcheck_json: true,
            netcheck_json_line: true,
            dns_status_json: true,
            dns_query_json: true,
            whois_json: true,
            connect: true,
            disconnect: true,
            set: true,
            accounts: true,
            account_login: true,
            account_logout: true,
            account_remove: true,
            syspolicy: true,
            ssh: true,
            nc: true,
            serve: true,
            serve_https: true,
            serve_http: true,
            serve_tcp: true,
            serve_tls_terminated_tcp: true,
            funnel: true,
            funnel_https: true,
            funnel_tcp: true,
            funnel_tls_terminated_tcp: true,
            taildrop: true,
            drive: true,
            certificate: true,
            metrics: true,
            bugreport: true,
        }
    }

    pub const fn supports_service_listener(
        &self,
        listener: &super::service::Listener,
        public: bool,
    ) -> bool {
        match (public, listener) {
            (false, super::service::Listener::Https(_)) => self.serve_https,
            (false, super::service::Listener::Http(_)) => self.serve_http,
            (false, super::service::Listener::Tcp(_)) => self.serve_tcp,
            (false, super::service::Listener::TlsTerminatedTcp(_)) => self.serve_tls_terminated_tcp,
            (true, super::service::Listener::Https(_)) => self.funnel_https,
            (true, super::service::Listener::Tcp(_)) => self.funnel_tcp,
            (true, super::service::Listener::TlsTerminatedTcp(_)) => self.funnel_tls_terminated_tcp,
            (true, super::service::Listener::Http(_)) => false,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalExecutable {
    pub path: PathBuf,
    pub socket_path: Option<PathBuf>,
    pub source: ExecutableSource,
    pub version: String,
    pub daemon_version: Option<String>,
    pub build: Option<String>,
    pub capabilities: LocalCapabilities,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LocalDaemonState {
    Disabled,
    Mock,
    Connecting,
    Live,
    Reconnecting,
    Unavailable { detail: String },
    PermissionDenied { detail: String },
    Unsupported { detail: String },
}

impl LocalDaemonState {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Mock => "mock",
            Self::Connecting => "connecting",
            Self::Live => "live",
            Self::Reconnecting => "reconnecting",
            Self::Unavailable { .. } => "unavailable",
            Self::PermissionDenied { .. } => "permission denied",
            Self::Unsupported { .. } => "unsupported",
        }
    }

    pub const fn is_live(&self) -> bool {
        matches!(self, Self::Live)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LocalCliState {
    Disabled,
    Mock,
    Discovering,
    Available,
    /// Carries the same explanation the other failure states do: what was
    /// looked for, where, and what to do about it.
    Missing {
        detail: String,
    },
    PermissionDenied {
        detail: String,
    },
    Unsupported {
        detail: String,
    },
    Unavailable {
        detail: String,
    },
}

impl LocalCliState {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Mock => "mock",
            Self::Discovering => "discovering",
            Self::Available => "available",
            Self::Missing { .. } => "missing",
            Self::PermissionDenied { .. } => "permission denied",
            Self::Unsupported { .. } => "unsupported",
            Self::Unavailable { .. } => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LocalState {
    Disabled,
    Mock,
    ExecutableMissing,
    ExecutableDenied,
    UnsupportedClient { version: String, reason: String },
    DaemonUnavailable { detail: String },
    PermissionDenied { operation: String, detail: String },
    NeedsLogin { auth_url: Option<String> },
    Stopped,
    Running,
    Degraded { health_messages: Vec<String> },
}

impl LocalState {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Mock => "mock",
            Self::ExecutableMissing => "executable missing",
            Self::ExecutableDenied => "executable denied",
            Self::UnsupportedClient { .. } => "unsupported client",
            Self::DaemonUnavailable { .. } => "daemon unavailable",
            Self::PermissionDenied { .. } => "permission denied",
            Self::NeedsLogin { .. } => "logged out",
            Self::Stopped => "stopped",
            Self::Running => "running",
            Self::Degraded { .. } => "degraded",
        }
    }

    pub const fn is_usable(&self) -> bool {
        matches!(self, Self::Running | Self::Degraded { .. })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LocalFailureKind {
    ExecutableMissing,
    ExecutableDenied,
    UnsupportedClient,
    DaemonUnavailable,
    PermissionDenied,
    NeedsLogin,
    InvalidOutput,
    TimedOut,
    Cancelled,
    Transport,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalFailure {
    pub kind: LocalFailureKind,
    pub operation: String,
    pub summary: String,
    pub detail: String,
    pub retryable: bool,
}

impl LocalFailure {
    pub fn new(
        kind: LocalFailureKind,
        operation: impl Into<String>,
        summary: impl Into<String>,
        detail: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            kind,
            operation: operation.into(),
            summary: summary.into(),
            detail: detail.into(),
            retryable,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LocalResourceStatus {
    NeverLoaded,
    Loading,
    Fresh,
    Stale,
    Failed,
}

impl LocalResourceStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NeverLoaded => "never loaded",
            Self::Loading => "loading",
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalSnapshot {
    pub observed_at: Timestamp,
    pub client_version: String,
    pub daemon_version: Option<String>,
    pub backend_state: LocalState,
    pub health_messages: Vec<String>,
    pub current_tailnet: Option<String>,
    pub magic_dns_suffix: Option<String>,
    pub cert_domains: Vec<String>,
    pub self_node: LocalDevice,
    pub peers: Vec<LocalDevice>,
}

#[derive(Debug, Clone)]
pub struct LocalResource {
    pub snapshot: Option<LocalSnapshot>,
    pub status: LocalResourceStatus,
    pub last_attempt_at: Option<Timestamp>,
    pub last_success_at: Option<Timestamp>,
    pub failure: Option<LocalFailure>,
    pub generation: u64,
    pub consecutive_failures: u32,
}

impl LocalResource {
    pub const fn new() -> Self {
        Self {
            snapshot: None,
            status: LocalResourceStatus::NeverLoaded,
            last_attempt_at: None,
            last_success_at: None,
            failure: None,
            generation: 0,
            consecutive_failures: 0,
        }
    }

    pub fn begin(&mut self, generation: u64, attempted_at: Timestamp) {
        self.generation = generation;
        self.last_attempt_at = Some(attempted_at);
        self.status = LocalResourceStatus::Loading;
        self.failure = None;
    }

    pub fn succeed(&mut self, generation: u64, snapshot: LocalSnapshot) -> bool {
        if generation < self.generation {
            return false;
        }
        self.generation = generation;
        self.last_success_at = Some(snapshot.observed_at);
        self.snapshot = Some(snapshot);
        self.status = LocalResourceStatus::Fresh;
        self.failure = None;
        self.consecutive_failures = 0;
        true
    }

    pub fn fail(&mut self, generation: u64, failure: LocalFailure) -> bool {
        if generation < self.generation {
            return false;
        }
        self.generation = generation;
        self.status = if self.snapshot.is_some() {
            LocalResourceStatus::Stale
        } else {
            LocalResourceStatus::Failed
        };
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.failure = Some(failure);
        true
    }

    pub fn mark_stale(&mut self) {
        if self.snapshot.is_some() && self.status == LocalResourceStatus::Fresh {
            self.status = LocalResourceStatus::Stale;
        }
    }
}

impl Default for LocalResource {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct LocalPreferencesResource {
    pub snapshot: Option<super::preference::LocalPreferences>,
    pub status: LocalResourceStatus,
    pub last_attempt_at: Option<Timestamp>,
    pub last_success_at: Option<Timestamp>,
    pub failure: Option<LocalFailure>,
    pub generation: u64,
    pub consecutive_failures: u32,
}

impl LocalPreferencesResource {
    pub const fn new() -> Self {
        Self {
            snapshot: None,
            status: LocalResourceStatus::NeverLoaded,
            last_attempt_at: None,
            last_success_at: None,
            failure: None,
            generation: 0,
            consecutive_failures: 0,
        }
    }

    pub fn begin(&mut self, generation: u64, attempted_at: Timestamp) {
        self.generation = generation;
        self.last_attempt_at = Some(attempted_at);
        self.status = LocalResourceStatus::Loading;
        self.failure = None;
    }

    pub fn succeed(
        &mut self,
        generation: u64,
        snapshot: super::preference::LocalPreferences,
    ) -> bool {
        if generation < self.generation {
            return false;
        }
        self.generation = generation;
        self.last_success_at = Some(snapshot.want_running.observed_at);
        self.snapshot = Some(snapshot);
        self.status = LocalResourceStatus::Fresh;
        self.failure = None;
        self.consecutive_failures = 0;
        true
    }

    pub fn fail(&mut self, generation: u64, failure: LocalFailure) -> bool {
        if generation < self.generation {
            return false;
        }
        self.generation = generation;
        self.status = if self.snapshot.is_some() {
            LocalResourceStatus::Stale
        } else {
            LocalResourceStatus::Failed
        };
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.failure = Some(failure);
        true
    }

    pub fn mark_stale(&mut self) {
        if self.snapshot.is_some() && self.status == LocalResourceStatus::Fresh {
            self.status = LocalResourceStatus::Stale;
        }
    }
}

impl Default for LocalPreferencesResource {
    fn default() -> Self {
        Self::new()
    }
}
