use std::path::PathBuf;
use std::str::FromStr;

use crate::action::{ActionId, Risk};

use super::Timestamp;
use super::certificate::CertificateRequest;
use super::transfer::{TaildropReceiveRequest, TaildropSendRequest};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServiceValueError(pub String);

impl std::fmt::Display for ServiceValueError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ServiceValueError {}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Port(u16);

impl Port {
    pub fn new(value: u16) -> Result<Self, ServiceValueError> {
        if value == 0 {
            return Err(ServiceValueError(
                "port must be between 1 and 65535".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for Port {
    type Error = ServiceValueError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for Port {
    type Err = ServiceValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.parse::<u16>().map_err(|_| {
            ServiceValueError("port must be an integer between 1 and 65535".to_owned())
        })?;
        Self::new(value)
    }
}

impl std::fmt::Display for Port {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AbsoluteUrlPath(String);

impl AbsoluteUrlPath {
    pub fn new(value: &str) -> Result<Self, ServiceValueError> {
        if value.chars().any(char::is_control) || value.contains('?') || value.contains('#') {
            return Err(ServiceValueError(
                "URL path cannot contain control characters, '?' or '#'".to_owned(),
            ));
        }
        let value = if value.is_empty() {
            "/".to_owned()
        } else if value.starts_with('/') {
            value.to_owned()
        } else {
            format!("/{value}")
        };
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AbsoluteUrlPath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Exposure {
    Tailnet,
    Public,
}

impl Exposure {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Tailnet => "tailnet",
            Self::Public => "public",
        }
    }

    pub const fn is_public(&self) -> bool {
        matches!(self, Self::Public)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Listener {
    Https(Port),
    Http(Port),
    Tcp(Port),
    TlsTerminatedTcp(Port),
}

impl Listener {
    pub const fn port(&self) -> Port {
        match self {
            Self::Https(port)
            | Self::Http(port)
            | Self::Tcp(port)
            | Self::TlsTerminatedTcp(port) => *port,
        }
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::Https(_) => "https",
            Self::Http(_) => "http",
            Self::Tcp(_) => "tcp",
            Self::TlsTerminatedTcp(_) => "tls-terminated-tcp",
        }
    }

    pub const fn allows_path(&self) -> bool {
        matches!(self, Self::Https(_) | Self::Http(_))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Backend {
    Port(Port),
    HttpUrl(String),
    HttpsInsecureUrl(String),
    UnixSocket(PathBuf),
    FileSystemPath(PathBuf),
}

impl Backend {
    pub fn parse(value: &str) -> Result<Self, ServiceValueError> {
        if value.is_empty() {
            return Err(ServiceValueError("backend must not be empty".to_owned()));
        }
        if let Ok(port) = value.parse::<Port>() {
            return Ok(Self::Port(port));
        }
        if value.starts_with("http://") && value.len() > "http://".len() {
            return Ok(Self::HttpUrl(value.to_owned()));
        }
        if value.starts_with("https+insecure://") && value.len() > "https+insecure://".len() {
            return Ok(Self::HttpsInsecureUrl(value.to_owned()));
        }
        if value.starts_with("unix:") && value.len() > "unix:".len() {
            return Ok(Self::UnixSocket(PathBuf::from(&value["unix:".len()..])));
        }
        if value.starts_with('/') {
            return Ok(Self::FileSystemPath(PathBuf::from(value)));
        }
        Err(ServiceValueError(
            "backend must be a port, http:// URL, https+insecure:// URL, unix: socket, or absolute filesystem path".to_owned(),
        ))
    }

    pub fn argument(&self) -> String {
        match self {
            Self::Port(port) => port.to_string(),
            Self::HttpUrl(value) | Self::HttpsInsecureUrl(value) => value.clone(),
            Self::UnixSocket(path) => format!("unix:{}", path.display()),
            Self::FileSystemPath(path) => path.display().to_string(),
        }
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::Port(_) => "port",
            Self::HttpUrl(_) => "http URL",
            Self::HttpsInsecureUrl(_) => "https+insecure URL",
            Self::UnixSocket(_) => "Unix socket",
            Self::FileSystemPath(_) => "filesystem path",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum PathMount {
    Root,
    Path(AbsoluteUrlPath),
}

impl PathMount {
    pub fn parse(value: &str) -> Result<Self, ServiceValueError> {
        let path = AbsoluteUrlPath::new(value)?;
        if path.as_str() == "/" {
            Ok(Self::Root)
        } else {
            Ok(Self::Path(path))
        }
    }

    pub fn as_path(&self) -> &str {
        match self {
            Self::Root => "/",
            Self::Path(path) => path.as_str(),
        }
    }
}

impl std::fmt::Display for PathMount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_path())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ProxyProtocol {
    None,
    Version1,
    Version2,
}

impl ProxyProtocol {
    pub fn parse(value: &str) -> Result<Self, ServiceValueError> {
        match value.to_ascii_lowercase().as_str() {
            "none" | "" => Ok(Self::None),
            "1" | "v1" | "version1" => Ok(Self::Version1),
            "2" | "v2" | "version2" => Ok(Self::Version2),
            _ => Err(ServiceValueError(
                "proxy protocol must be none, 1, or 2".to_owned(),
            )),
        }
    }

    pub const fn cli_value(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Version1 => Some("1"),
            Self::Version2 => Some("2"),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServiceMapping {
    pub exposure: Exposure,
    pub listener: Listener,
    pub mount: PathMount,
    pub backend: Backend,
    pub proxy_protocol: ProxyProtocol,
    pub hostname: Option<String>,
}

impl ServiceMapping {
    pub fn key(&self) -> String {
        format!(
            "{}:{}{}",
            self.listener.label(),
            self.listener.port(),
            self.mount.as_path()
        )
    }

    pub fn exact_identity_matches(&self, other: &Self) -> bool {
        self.listener == other.listener && self.mount == other.mount
    }

    pub fn validate(&self) -> Result<(), ServiceValueError> {
        if !self.listener.allows_path() && !matches!(self.mount, PathMount::Root) {
            return Err(ServiceValueError(
                "a mount path is only valid for HTTP or HTTPS listeners".to_owned(),
            ));
        }
        if !matches!(self.listener, Listener::Tcp(_)) && self.proxy_protocol != ProxyProtocol::None
        {
            return Err(ServiceValueError(
                "proxy protocol is only valid for TCP listeners".to_owned(),
            ));
        }
        if matches!(self.exposure, Exposure::Public) && matches!(self.listener, Listener::Http(_)) {
            return Err(ServiceValueError(
                "HTTP is not offered as a public Funnel listener".to_owned(),
            ));
        }
        Ok(())
    }
}

/// What the mapping table can be ordered by. Every one of these is a column the
/// table actually shows.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ServiceSortField {
    Exposure,
    Listener,
    Port,
    Mount,
    Backend,
}

impl ServiceSortField {
    pub const ALL: [Self; 5] = [
        Self::Exposure,
        Self::Listener,
        Self::Port,
        Self::Mount,
        Self::Backend,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Exposure => "exposure",
            Self::Listener => "listener",
            Self::Port => "port",
            Self::Mount => "path",
            Self::Backend => "backend",
        }
    }

    pub const fn key(self) -> char {
        match self {
            Self::Exposure => 'e',
            Self::Listener => 'l',
            Self::Port => 'p',
            Self::Mount => 'm',
            Self::Backend => 'b',
        }
    }

    /// A comparable key for one mapping. The port is always the tiebreak so the
    /// order is total and stable across refreshes.
    pub fn ordering_key(self, mapping: &ServiceMapping) -> (String, u16) {
        let port = mapping.listener.port().get();
        let text = match self {
            Self::Exposure => mapping.exposure.label().to_owned(),
            Self::Listener => mapping.listener.label().to_owned(),
            Self::Port => String::new(),
            Self::Mount => mapping.mount.as_path().to_owned(),
            Self::Backend => mapping.backend.argument(),
        };
        (text, port)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct ServeStatus {
    pub mappings: Vec<ServiceMapping>,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct FunnelStatus {
    pub mappings: Vec<ServiceMapping>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ServiceResourceStatus {
    /// Nothing has been asked of the local client yet. Distinct from `Loading`,
    /// which claims a request is outstanding.
    Idle,
    Loading,
    Ready,
    Stale,
    Unsupported,
    Failed,
}

impl ServiceResourceStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "not requested",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Stale => "stale",
            Self::Unsupported => "unsupported",
            Self::Failed => "failed",
        }
    }

    /// How badly a status reflects on the data, used to combine two of them.
    const fn severity(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::Loading => 1,
            Self::Idle => 2,
            Self::Stale => 3,
            Self::Unsupported => 4,
            Self::Failed => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ServiceFailureKind {
    NotInstalled,
    DaemonUnavailable,
    Unsupported,
    PermissionDenied,
    PolicyDenied,
    TimedOut,
    Cancelled,
    DecodeFailed,
    CommandFailed,
}

impl ServiceFailureKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NotInstalled => "not installed",
            Self::DaemonUnavailable => "daemon unavailable",
            Self::Unsupported => "unsupported",
            Self::PermissionDenied => "permission denied",
            Self::PolicyDenied => "policy denied",
            Self::TimedOut => "timed out",
            Self::Cancelled => "cancelled",
            Self::DecodeFailed => "decode failed",
            Self::CommandFailed => "command failed",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServiceFailure {
    pub kind: ServiceFailureKind,
    pub operation: String,
    pub summary: String,
    pub detail: String,
    pub exit_status: Option<i32>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl ServiceFailure {
    pub fn new(
        kind: ServiceFailureKind,
        operation: impl Into<String>,
        summary: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            operation: operation.into(),
            summary: summary.into(),
            detail: detail.into(),
            exit_status: None,
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServiceResource<T> {
    pub status: ServiceResourceStatus,
    pub value: Option<T>,
    pub observed_at: Option<Timestamp>,
    pub generation: u64,
    pub failure: Option<ServiceFailure>,
}

impl<T> ServiceResource<T> {
    pub const fn new() -> Self {
        Self {
            status: ServiceResourceStatus::Idle,
            value: None,
            observed_at: None,
            generation: 0,
            failure: None,
        }
    }

    pub fn begin(&mut self, generation: u64) {
        self.generation = generation;
        self.status = ServiceResourceStatus::Loading;
        self.failure = None;
    }

    pub fn succeed(&mut self, generation: u64, observed_at: Timestamp, value: T) {
        if generation < self.generation {
            return;
        }
        self.generation = generation;
        self.status = ServiceResourceStatus::Ready;
        self.observed_at = Some(observed_at);
        self.value = Some(value);
        self.failure = None;
    }

    pub fn fail(&mut self, generation: u64, failure: ServiceFailure) {
        if generation < self.generation {
            return;
        }
        self.generation = generation;
        self.status = match failure.kind {
            ServiceFailureKind::Unsupported => ServiceResourceStatus::Unsupported,
            _ if self.value.is_some() => ServiceResourceStatus::Stale,
            _ => ServiceResourceStatus::Failed,
        };
        self.failure = Some(failure);
    }
}

impl<T> Default for ServiceResource<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CapabilityState {
    pub status: ServiceResourceStatus,
    pub reason: Option<String>,
}

impl CapabilityState {
    pub fn available() -> Self {
        Self {
            status: ServiceResourceStatus::Ready,
            reason: None,
        }
    }

    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self {
            status: ServiceResourceStatus::Unsupported,
            reason: Some(reason.into()),
        }
    }

    pub fn loading() -> Self {
        Self {
            status: ServiceResourceStatus::Loading,
            reason: None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServiceCapabilities {
    pub serve: CapabilityState,
    pub funnel: CapabilityState,
    pub taildrop: CapabilityState,
    pub taildrive: CapabilityState,
    pub certificates: CapabilityState,
    pub metrics: CapabilityState,
    pub bug_report: CapabilityState,
}

impl ServiceCapabilities {
    pub fn loading() -> Self {
        Self {
            serve: CapabilityState::loading(),
            funnel: CapabilityState::loading(),
            taildrop: CapabilityState::loading(),
            taildrive: CapabilityState::loading(),
            certificates: CapabilityState::loading(),
            metrics: CapabilityState::loading(),
            bug_report: CapabilityState::loading(),
        }
    }
}

/// The three things this route actually offers. Serve and Funnel are one
/// section: a Funnel mapping is a Serve mapping whose exposure is public, and
/// the local client already partitions them by `AllowFunnel`. Taildrop is not
/// here: its rows were the tailnet's devices, so it lives on `:devices`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ServiceSection {
    Serve,
    Taildrive,
    Certificates,
}

impl ServiceSection {
    pub const ALL: [Self; 3] = [Self::Serve, Self::Taildrive, Self::Certificates];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Serve => "serve",
            Self::Taildrive => "taildrive",
            Self::Certificates => "certificates",
        }
    }

    /// What a row of this section is, for counts and empty states.
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Serve => "mappings",
            Self::Taildrive => "shares",
            Self::Certificates => "domains",
        }
    }

    /// The key that selects this section in the section menu.
    pub const fn key(self) -> char {
        match self {
            Self::Serve => 's',
            Self::Taildrive => 'v',
            Self::Certificates => 'c',
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalServicesSnapshot {
    pub serve: ServiceResource<ServeStatus>,
    pub funnel: ServiceResource<FunnelStatus>,
    pub taildrop_targets: ServiceResource<Vec<super::transfer::TaildropTarget>>,
    pub taildrive: ServiceResource<Vec<super::transfer::TaildriveShare>>,
    pub certificate_domains: ServiceResource<Vec<String>>,
    pub metrics: ServiceResource<MetricsOutput>,
    pub bug_report: ServiceResource<BugReportResult>,
    pub capabilities: ServiceCapabilities,
    pub observed_at: Option<Timestamp>,
    pub generation: u64,
    pub command_version: Option<String>,
}

impl LocalServicesSnapshot {
    pub fn new() -> Self {
        Self {
            serve: ServiceResource::new(),
            funnel: ServiceResource::new(),
            taildrop_targets: ServiceResource::new(),
            taildrive: ServiceResource::new(),
            certificate_domains: ServiceResource::new(),
            metrics: ServiceResource::new(),
            bug_report: ServiceResource::new(),
            capabilities: ServiceCapabilities::loading(),
            observed_at: None,
            generation: 0,
            command_version: None,
        }
    }

    pub fn begin(&mut self, generation: u64) {
        self.generation = generation;
        self.serve.begin(generation);
        self.funnel.begin(generation);
        self.taildrop_targets.begin(generation);
        self.taildrive.begin(generation);
        self.certificate_domains.begin(generation);
    }

    /// Serve and Funnel as one list. `serve status` and `funnel status` read the
    /// same configuration and partition it by `AllowFunnel`, so the two are
    /// disjoint and their concatenation is the whole set.
    pub fn mappings(&self) -> impl Iterator<Item = &ServiceMapping> {
        self.serve
            .value
            .iter()
            .flat_map(|status| status.mappings.iter())
            .chain(
                self.funnel
                    .value
                    .iter()
                    .flat_map(|status| status.mappings.iter()),
            )
    }

    /// The worse of the two mapping statuses, since the table shows both.
    pub fn mapping_status(&self) -> ServiceResourceStatus {
        let (serve, funnel) = (self.serve.status, self.funnel.status);
        if serve == funnel {
            return serve;
        }
        [serve, funnel]
            .into_iter()
            .max_by_key(|status| status.severity())
            .unwrap_or(serve)
    }

    pub fn mapping_failure(&self) -> Option<&ServiceFailure> {
        self.serve.failure.as_ref().or(self.funnel.failure.as_ref())
    }
}

impl Default for LocalServicesSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ServiceActionRequest {
    Serve {
        mapping: ServiceMapping,
        edit: bool,
    },
    ServeReset,
    /// Take down exactly one mapping. `tailscale serve … off` is the same code
    /// path for both exposures, so the selected row decides nothing here except
    /// how loudly the confirmation has to shout.
    MappingRemove {
        mapping: ServiceMapping,
    },
    Funnel {
        mapping: ServiceMapping,
        edit: bool,
    },
    /// Demote one public mapping to tailnet-only. There is no `funnel off` that
    /// keeps the handler, so this re-serves the same mapping without Funnel.
    FunnelUnpublish {
        mapping: ServiceMapping,
    },
    FunnelReset,
    TaildropSend(TaildropSendRequest),
    TaildropReceive(TaildropReceiveRequest),
    TaildriveShare {
        input_name: String,
        normalized_name: String,
        path: PathBuf,
    },
    TaildriveRename {
        old_name: String,
        input_name: String,
        normalized_name: String,
    },
    TaildriveUnshare {
        name: String,
    },
    Certificate(CertificateRequest),
    Metrics,
    BugReport(super::certificate::BugReportRequest),
}

impl ServiceActionRequest {
    pub const fn action_id(&self) -> ActionId {
        match self {
            Self::Serve { edit: true, .. } => ActionId::ServicesServeEdit,
            Self::Serve { edit: false, .. } => ActionId::ServicesServeCreate,
            Self::ServeReset => ActionId::ServicesServeReset,
            Self::MappingRemove { .. } => ActionId::ServicesServeRemove,
            Self::Funnel { edit: true, .. } => ActionId::ServicesFunnelEdit,
            Self::Funnel { edit: false, .. } => ActionId::ServicesFunnelCreate,
            Self::FunnelUnpublish { .. } => ActionId::ServicesFunnelUnpublish,
            Self::FunnelReset => ActionId::ServicesFunnelReset,
            Self::TaildropSend(_) => ActionId::DevicesTaildropSend,
            Self::TaildropReceive(_) => ActionId::DevicesTaildropReceive,
            Self::TaildriveShare { .. } => ActionId::ServicesDriveShare,
            Self::TaildriveRename { .. } => ActionId::ServicesDriveRename,
            Self::TaildriveUnshare { .. } => ActionId::ServicesDriveUnshare,
            Self::Certificate(_) => ActionId::ServicesCertificateObtain,
            Self::Metrics => ActionId::ServicesMetricsRefresh,
            Self::BugReport(_) => ActionId::ServicesBugReportCreate,
        }
    }

    pub const fn risk(&self) -> Risk {
        match self {
            Self::ServeReset
            | Self::MappingRemove { .. }
            | Self::Funnel { .. }
            | Self::FunnelUnpublish { .. }
            | Self::FunnelReset => Risk::Disruptive,
            Self::TaildriveUnshare { .. } => Risk::Disruptive,
            Self::Certificate(request) if request.overwrites_existing => Risk::Disruptive,
            Self::TaildropReceive(request) if request.conflict.is_overwrite() => Risk::Disruptive,
            Self::Serve { .. }
            | Self::TaildropSend(_)
            | Self::TaildropReceive(_)
            | Self::TaildriveShare { .. }
            | Self::TaildriveRename { .. }
            | Self::Certificate(_)
            | Self::Metrics
            | Self::BugReport(_) => Risk::Reversible,
        }
    }

    pub fn target_label(&self) -> String {
        match self {
            Self::Serve { mapping, .. }
            | Self::MappingRemove { mapping }
            | Self::Funnel { mapping, .. }
            | Self::FunnelUnpublish { mapping } => mapping.key(),
            Self::ServeReset => "all Serve mappings".to_owned(),
            Self::FunnelReset => "all Funnel mappings".to_owned(),
            Self::TaildropSend(request) => request.target.command_target.clone(),
            Self::TaildropReceive(request) => request.directory.display().to_string(),
            Self::TaildriveShare {
                normalized_name, ..
            } => normalized_name.clone(),
            Self::TaildriveRename { old_name, .. } => old_name.clone(),
            Self::TaildriveUnshare { name } => name.clone(),
            Self::Certificate(request) => request.domain.clone(),
            Self::Metrics => "local metrics".to_owned(),
            Self::BugReport(_) => "Tailscale diagnostic report".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ServiceConflictKey {
    Serve,
    Funnel,
    TaildropReceive,
    TaildropTarget(String),
    Taildrive,
    Certificate,
}

impl ServiceActionRequest {
    pub fn conflict_key(&self) -> Option<ServiceConflictKey> {
        match self {
            Self::Serve { .. } | Self::ServeReset => Some(ServiceConflictKey::Serve),
            Self::Funnel { .. } | Self::FunnelReset => Some(ServiceConflictKey::Funnel),
            // Both run `tailscale serve`, but the resource the user is changing
            // is the one the row is currently listed under.
            Self::MappingRemove { mapping } => Some(match mapping.exposure {
                Exposure::Public => ServiceConflictKey::Funnel,
                Exposure::Tailnet => ServiceConflictKey::Serve,
            }),
            Self::FunnelUnpublish { .. } => Some(ServiceConflictKey::Funnel),
            Self::TaildropSend(request) => Some(ServiceConflictKey::TaildropTarget(
                request.target.command_target.clone(),
            )),
            Self::TaildropReceive(_) => Some(ServiceConflictKey::TaildropReceive),
            Self::TaildriveShare { .. }
            | Self::TaildriveRename { .. }
            | Self::TaildriveUnshare { .. } => Some(ServiceConflictKey::Taildrive),
            Self::Certificate(_) => Some(ServiceConflictKey::Certificate),
            Self::Metrics | Self::BugReport(_) => None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MetricsOutput {
    pub text: String,
    pub captured_at: Timestamp,
    pub truncated: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BugReportResult {
    pub identifier: String,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CertificateVerification {
    pub domain: String,
    pub certificate_path: PathBuf,
    pub key_path: PathBuf,
    pub certificate_size: u64,
    pub key_size: u64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ServiceTaskData {
    Serve {
        status: ServeStatus,
        verified: bool,
        summary: String,
    },
    Funnel {
        status: FunnelStatus,
        verified: bool,
        summary: String,
    },
    Taildrive {
        shares: Vec<super::transfer::TaildriveShare>,
        verified: bool,
        summary: String,
    },
    TaildropTargets(Vec<super::transfer::TaildropTarget>),
    Transfer {
        summary: String,
        filenames: Vec<String>,
    },
    Certificate(CertificateVerification),
    Metrics(MetricsOutput),
    BugReport(BugReportResult),
}
