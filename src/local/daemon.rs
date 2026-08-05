use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Limited};
use hyper::body::Incoming;
use hyper::header::{HOST, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::task::JoinHandle;

use crate::domain::Timestamp;
use crate::domain::device::{ConnectionPath, DeviceId, LocalDevice, OperatingSystem};
use crate::domain::preference::{LocalPreferences, ObservedPreference};
use crate::domain::route::{parse_route_set, parse_static_endpoints};
use crate::domain::source::{LocalFailure, LocalFailureKind, LocalSnapshot, LocalState};
use crate::local::process::Cancellation;

pub const LOCAL_API_HOST: &str = "local-tailscaled.sock";
pub const LOCAL_API_CAPABILITY: &str = "138";
pub const LOCAL_API_STATUS_PATH: &str = "/localapi/v0/status";
pub const LOCAL_API_PREFS_PATH: &str = "/localapi/v0/prefs";
pub const LOCAL_API_WATCH_PATH: &str = "/localapi/v0/watch-ipn-bus";
pub const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_NOTIFICATION_BYTES: usize = 32 * 1024 * 1024;

const CANCEL_POLL: Duration = Duration::from_millis(10);
const EXPECTED_CLIENT_FAMILY: &str = "1.98.9";

pub fn documented_socket_path() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/var/run/tailscaled.socket")
    } else {
        PathBuf::from("/var/run/tailscale/tailscaled.sock")
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LocalEndpointKind {
    UnixSocket,
    WindowsNamedPipe,
}

impl LocalEndpointKind {
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::WindowsNamedPipe
        } else {
            Self::UnixSocket
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::UnixSocket => "unix socket",
            Self::WindowsNamedPipe => "Windows named pipe",
        }
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum LocalDaemonError {
    #[error("LocalAPI {operation} transport failed: {detail}")]
    Connection { operation: String, detail: String },
    #[error("LocalAPI {operation} request timed out")]
    TimedOut { operation: String },
    #[error("LocalAPI request cancelled")]
    Cancelled,
    #[error("LocalAPI {operation} returned HTTP status {status}: {detail}")]
    HttpStatus {
        operation: String,
        status: u16,
        detail: String,
    },
    #[error("LocalAPI {operation} protocol failed: {detail}")]
    Protocol { operation: String, detail: String },
    #[error("LocalAPI {operation} JSON was invalid: {detail}")]
    Decode { operation: String, detail: String },
    #[error("LocalAPI server version is outside the pinned {expected} family: {actual}")]
    UnsupportedVersion { expected: String, actual: String },
    #[error("LocalAPI is unsupported on this platform")]
    UnsupportedPlatform,
}

impl LocalDaemonError {
    pub fn operation(&self) -> String {
        match self {
            Self::Connection { operation, .. }
            | Self::TimedOut { operation }
            | Self::HttpStatus { operation, .. }
            | Self::Protocol { operation, .. }
            | Self::Decode { operation, .. } => operation.clone(),
            Self::UnsupportedVersion { .. } => "version".to_owned(),
            Self::Cancelled => "cancellation".to_owned(),
            Self::UnsupportedPlatform => "platform".to_owned(),
        }
    }

    pub fn failure(&self) -> LocalFailure {
        let operation = self.operation();
        let (kind, summary, detail, retryable) = match self {
            Self::Connection { detail, .. } => (
                LocalFailureKind::DaemonUnavailable,
                "local daemon unavailable",
                detail.clone(),
                true,
            ),
            Self::TimedOut { .. } => (
                LocalFailureKind::TimedOut,
                "local daemon request timed out",
                "the LocalAPI request exceeded the configured deadline".to_owned(),
                true,
            ),
            Self::Cancelled => (
                LocalFailureKind::Cancelled,
                "local daemon request cancelled",
                "the LocalAPI request was cancelled".to_owned(),
                false,
            ),
            Self::HttpStatus { status, detail, .. } => {
                let kind = if *status == 401 || *status == 403 {
                    LocalFailureKind::PermissionDenied
                } else if *status == 404 || *status == 405 || *status == 501 {
                    LocalFailureKind::UnsupportedClient
                } else {
                    LocalFailureKind::DaemonUnavailable
                };
                (
                    kind,
                    "local daemon returned an HTTP error",
                    detail.clone(),
                    true,
                )
            }
            Self::Protocol { detail, .. } | Self::Decode { detail, .. } => (
                LocalFailureKind::InvalidOutput,
                "local daemon response was unsupported",
                detail.clone(),
                false,
            ),
            Self::UnsupportedVersion { actual, .. } => (
                LocalFailureKind::UnsupportedClient,
                "local daemon client family is unsupported",
                format!("server version {actual} is outside the pinned contract"),
                false,
            ),
            Self::UnsupportedPlatform => (
                LocalFailureKind::UnsupportedClient,
                "local daemon transport is unsupported",
                "the current platform has no approved LocalAPI endpoint".to_owned(),
                false,
            ),
        };
        LocalFailure::new(kind, operation, summary, detail, retryable)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct NotifyWatchMask(u64);

impl NotifyWatchMask {
    pub const fn tale() -> Self {
        Self(4495)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalStatusSnapshot {
    pub snapshot: LocalSnapshot,
    pub server_version: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalPreferenceSnapshot {
    pub preferences: LocalPreferences,
}

pub fn decode_status(
    input: &str,
    client_version: String,
    daemon_version: Option<String>,
    observed_at: Timestamp,
) -> Result<LocalSnapshot, String> {
    let value: Value =
        serde_json::from_str(input).map_err(|error| format!("invalid JSON: {error}"))?;
    let users = parse_users(value.get("User").or_else(|| value.get("Users")));
    let self_value = get(&value, &["Self", "self"])
        .ok_or_else(|| "required self node was not returned".to_owned())?;
    let self_node = parse_node(self_value, &users)?;
    let mut peers = Vec::new();
    if let Some(peer_value) = get(&value, &["Peer", "Peers", "peer", "peers"]) {
        match peer_value {
            Value::Object(map) => {
                for value in map.values() {
                    if let Ok(device) = parse_node(value, &users) {
                        peers.push(device);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    if let Ok(device) = parse_node(value, &users) {
                        peers.push(device);
                    }
                }
            }
            _ => return Err("peer collection was not an object or array".to_owned()),
        }
    }
    let health_messages = parse_strings(get(&value, &["Health", "health"]));
    let backend_state = backend_state(
        first_string(&value, &["BackendState", "backendState"]).as_deref(),
        &health_messages,
        first_string(&value, &["AuthURL", "AuthUrl", "LoginURL", "loginUrl"]),
    );
    Ok(LocalSnapshot {
        observed_at,
        client_version,
        daemon_version,
        backend_state,
        health_messages,
        current_tailnet: parse_name(get(&value, &["CurrentTailnet", "Tailnet"])),
        magic_dns_suffix: first_string(&value, &["MagicDNSSuffix", "magicDnsSuffix"]),
        cert_domains: parse_strings(get(&value, &["CertDomains", "CertificateDomains"])),
        self_node,
        peers,
    })
}

fn parse_node(value: &Value, users: &BTreeMap<String, String>) -> Result<LocalDevice, String> {
    if value.as_object().is_none() {
        return Err("local node was not an object".to_owned());
    }
    let id = first_string(value, &["ID", "Id", "NodeID", "NodeId"])
        .ok_or_else(|| "local node has no stable identity".to_owned())?;
    let display_name = match first_string(value, &["HostName", "Hostname", "Name", "DNSName"]) {
        Some(value) => value,
        None => id.clone(),
    };
    let hostname = match first_string(value, &["HostName", "Hostname", "Name"]) {
        Some(value) => value,
        None => display_name.clone(),
    };
    let user_id = first_string(value, &["UserID", "UserId", "userId"]);
    let owner_label = user_id.as_deref().and_then(|user| users.get(user)).cloned();
    let mut capabilities = parse_capabilities(get(value, &["Capabilities", "capabilities"]));
    add_bool_capability(value, &mut capabilities, "ExitNode", "exit-node");
    add_bool_capability(
        value,
        &mut capabilities,
        "ExitNodeOption",
        "exit-node-option",
    );
    add_bool_capability(value, &mut capabilities, "Shared", "shared");
    add_bool_capability(value, &mut capabilities, "Approved", "approved");
    let ssh_host_keys_present = !parse_strings(get(value, &["SSHHostKeys", "SshHostKeys"]))
        .is_empty()
        || capabilities.get("ssh").copied().is_some_and(|value| value);
    let shared = first_bool(value, &["ShareeNode", "Shared", "shared"]).is_some_and(|value| value)
        || capabilities
            .get("shared")
            .copied()
            .is_some_and(|value| value);
    let endpoint = first_string(value, &["CurAddr", "CurrentEndpoint", "Endpoint"]);
    let relay = first_string(value, &["Relay", "RelayRegion", "DERP"]);
    Ok(LocalDevice {
        id: DeviceId::new(id),
        public_key: first_string(value, &["PublicKey", "publicKey"]),
        display_name,
        hostname,
        dns_name: first_string(value, &["DNSName", "DnsName", "dnsName"]),
        os: parse_os(first_string(value, &["OS", "Os", "os"])),
        version: first_string(value, &["ClientVersion", "Version", "version"]),
        owner_label,
        user_id,
        tags: parse_strings(get(value, &["Tags", "tags"])),
        tailscale_ips: parse_strings(get(
            value,
            &["TailscaleIPs", "TailscaleIps", "Addresses", "IPs"],
        )),
        advertised_routes: parse_strings(get(value, &["AdvertisedRoutes", "Routes"])),
        current_endpoint: endpoint.clone(),
        relay_region: relay.clone(),
        path: parse_path(value, endpoint.as_deref(), relay.as_deref()),
        online: first_bool(value, &["Online", "online"]),
        active: first_bool(value, &["Active", "active"]).is_some_and(|value| value),
        rx_bytes: first_u64(value, &["RxBytes", "RXBytes", "rxBytes"]),
        tx_bytes: first_u64(value, &["TxBytes", "TXBytes", "txBytes"]),
        created_at: first_timestamp(value, &["Created", "CreatedAt", "createdAt"]),
        last_seen: first_timestamp(value, &["LastSeen", "lastSeen"]),
        last_handshake: first_timestamp(value, &["LastHandshake", "lastHandshake"]),
        exit_node: first_bool(value, &["ExitNode", "IsExitNode"]).is_some_and(|value| value),
        exit_node_option: first_bool(value, &["ExitNodeOption", "IsExitNodeOption"])
            .is_some_and(|value| value),
        ssh_host_keys_present,
        shared,
        capabilities,
    })
}

fn parse_users(value: Option<&Value>) -> BTreeMap<String, String> {
    let mut users = BTreeMap::new();
    let Some(Value::Object(map)) = value else {
        return users;
    };
    for (id, value) in map {
        if let Some(label) = value.as_str() {
            users.insert(id.clone(), label.to_owned());
        } else if let Some(label) = first_string(
            value,
            &["DisplayName", "Name", "Email", "LoginName", "name"],
        ) {
            users.insert(id.clone(), label);
        }
    }
    users
}

fn parse_os(value: Option<String>) -> OperatingSystem {
    match value.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("linux") => OperatingSystem::Linux,
        Some("darwin") | Some("mac") | Some("macos") | Some("osx") => OperatingSystem::MacOS,
        Some("windows") | Some("win") => OperatingSystem::Windows,
        Some("ios") => OperatingSystem::IOS,
        Some("android") => OperatingSystem::Android,
        Some(value) => OperatingSystem::Unknown(value.to_owned()),
        None => OperatingSystem::Unknown("unknown".to_owned()),
    }
}

fn parse_path(value: &Value, endpoint: Option<&str>, relay: Option<&str>) -> ConnectionPath {
    if let Some(peer) = first_string(value, &["PeerRelay", "PeerRelayNode", "peerRelay"]) {
        return ConnectionPath::PeerRelay { peer };
    }
    if let Some(relay) =
        relay.filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("false"))
    {
        return ConnectionPath::Derp {
            region: relay.to_owned(),
        };
    }
    if endpoint.is_some_and(|value| !value.is_empty()) {
        return ConnectionPath::Direct { latency_ms: None };
    }
    match first_bool(value, &["Active", "active"]) {
        Some(true) => ConnectionPath::Unknown("active without endpoint".to_owned()),
        Some(false) => ConnectionPath::Idle,
        None => ConnectionPath::Unknown("path not returned".to_owned()),
    }
}

fn parse_capabilities(value: Option<&Value>) -> BTreeMap<String, bool> {
    let mut capabilities = BTreeMap::new();
    match value {
        Some(Value::Array(values)) => {
            for value in values {
                if let Some(name) = value.as_str() {
                    capabilities.insert(normalize(name), true);
                }
            }
        }
        Some(Value::Object(map)) => {
            for (name, value) in map {
                if let Some(value) = value.as_bool() {
                    capabilities.insert(normalize(name), value);
                }
            }
        }
        _ => {}
    }
    capabilities
}

fn add_bool_capability(
    value: &Value,
    capabilities: &mut BTreeMap<String, bool>,
    field: &str,
    name: &str,
) {
    if let Some(value) = first_bool(value, &[field]) {
        capabilities.insert(name.to_owned(), value);
    }
}

fn backend_state(value: Option<&str>, health: &[String], auth_url: Option<String>) -> LocalState {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("needslogin")
        | Some("needs_login")
        | Some("needs machine auth")
        | Some("needsmachineauth") => LocalState::NeedsLogin { auth_url },
        Some("stopped") | Some("nostate") | Some("no_state") => LocalState::Stopped,
        Some("running") | Some("starting") if health.is_empty() => LocalState::Running,
        Some("running") | Some("starting") => LocalState::Degraded {
            health_messages: health.to_vec(),
        },
        _ if health.is_empty() => LocalState::Running,
        _ => LocalState::Degraded {
            health_messages: health.to_vec(),
        },
    }
}

fn parse_name(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) if !value.is_empty() => Some(value.clone()),
        Some(Value::Object(_)) => {
            value.and_then(|value| first_string(value, &["Name", "Domain", "ID", "Id"]))
        }
        _ => None,
    }
}

fn first_string(value: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| get(value, &[*name]).and_then(value_string))
}

fn first_bool(value: &Value, names: &[&str]) -> Option<bool> {
    names
        .iter()
        .find_map(|name| get(value, &[*name]).and_then(value_bool))
}

fn first_u64(value: &Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| get(value, &[*name]).and_then(value_u64))
}

fn first_timestamp(value: &Value, names: &[&str]) -> Option<Timestamp> {
    names
        .iter()
        .find_map(|name| get(value, &[*name]).and_then(value_timestamp))
}

fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn value_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::String(value) => match value.to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Some(true),
            "false" | "no" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn value_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn value_timestamp(value: &Value) -> Option<Timestamp> {
    match value {
        Value::Number(_) => value_u64(value),
        Value::String(value) => value.parse().ok().or_else(|| parse_timestamp(value)),
        _ => None,
    }
}

fn parse_timestamp(value: &str) -> Option<Timestamp> {
    let (date, time) = value.split_once('T').or_else(|| value.split_once(' '))?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<i64>().ok()?;
    let day = date_parts.next()?.parse::<i64>().ok()?;
    let time = time.trim_end_matches('Z').split(['+', '-']).next()?;
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u64>().ok()?;
    let minute = time_parts.next()?.parse::<u64>().ok()?;
    let second = time_parts.next()?.split('.').next()?.parse::<u64>().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    let seconds = i128::from(days)
        .checked_mul(86_400)?
        .checked_add(i128::from(hour.saturating_mul(3_600)))?
        .checked_add(i128::from(minute.saturating_mul(60)))?
        .checked_add(i128::from(second))?;
    u64::try_from(seconds).ok()
}

fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let adjusted_year = year.checked_sub(i64::from(month <= 2))?;
    let era = if adjusted_year >= 0 {
        adjusted_year / 400
    } else {
        (adjusted_year - 399) / 400
    };
    let year_of_era = adjusted_year - era * 400;
    let month_index = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era.checked_mul(146_097)
        .and_then(|value| value.checked_add(day_of_era))
        .and_then(|value| value.checked_sub(719_468))
}

fn parse_strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values.iter().filter_map(value_string).collect(),
        Some(Value::String(value)) if !value.is_empty() => vec![value.clone()],
        _ => Vec::new(),
    }
}

fn get<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    let object = value.as_object()?;
    names.iter().find_map(|name| {
        object.get(*name).or_else(|| {
            object
                .iter()
                .find(|(key, _)| normalize(key) == normalize(name))
                .map(|(_, value)| value)
        })
    })
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum WatchInvalidation {
    Status,
    Preferences,
    Both,
    None,
    DaemonError { detail: String },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WatchNotification {
    pub invalidation: WatchInvalidation,
}

#[derive(Debug)]
pub struct LocalDaemonClient {
    socket_path: PathBuf,
    timeout: Duration,
}

impl Clone for LocalDaemonClient {
    fn clone(&self) -> Self {
        Self {
            socket_path: self.socket_path.clone(),
            timeout: self.timeout,
        }
    }
}

impl LocalDaemonClient {
    pub fn new(socket_path: PathBuf, timeout: Duration) -> Self {
        Self {
            socket_path,
            timeout,
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn endpoint_kind(&self) -> LocalEndpointKind {
        LocalEndpointKind::current()
    }

    pub async fn status(
        &self,
        cancellation: &Cancellation,
    ) -> Result<LocalStatusSnapshot, LocalDaemonError> {
        let (response, connection) = self
            .snapshot_response("status", LOCAL_API_STATUS_PATH, cancellation)
            .await?;
        let server_version = server_version(&response, "status")?;
        let body = self
            .read_snapshot_body(
                "status",
                response,
                connection,
                cancellation,
                self.endpoint_kind(),
            )
            .await?;
        let observed_at = crate::local::now();
        let snapshot = decode_status(
            std::str::from_utf8(&body).map_err(|error| LocalDaemonError::Decode {
                operation: "status".to_owned(),
                detail: bounded_detail(&format!("response was not UTF-8: {error}")),
            })?,
            match server_version.clone() {
                Some(version) => version,
                None => "unknown".to_owned(),
            },
            None,
            observed_at,
        )
        .map_err(|detail| LocalDaemonError::Decode {
            operation: "status".to_owned(),
            detail: bounded_detail(&detail),
        })?;
        Ok(LocalStatusSnapshot {
            snapshot,
            server_version,
        })
    }

    pub async fn preferences(
        &self,
        cancellation: &Cancellation,
    ) -> Result<LocalPreferenceSnapshot, LocalDaemonError> {
        let (response, connection) = self
            .snapshot_response("preferences", LOCAL_API_PREFS_PATH, cancellation)
            .await?;
        let body = self
            .read_snapshot_body(
                "preferences",
                response,
                connection,
                cancellation,
                self.endpoint_kind(),
            )
            .await?;
        let preferences = decode_preferences(&body, crate::local::now())?;
        Ok(LocalPreferenceSnapshot { preferences })
    }

    pub async fn watch(
        &self,
        mask: NotifyWatchMask,
        cancellation: &Cancellation,
    ) -> Result<LocalWatchStream, LocalDaemonError> {
        let operation = "watch-ipn-bus";
        let path = format!("{LOCAL_API_WATCH_PATH}?mask={}", mask.value());
        let (response, connection) = with_deadline(
            self.timeout,
            operation,
            cancellation,
            self.open_response(operation, &path, false),
        )
        .await?;
        let version = server_version(&response, operation)?;
        validate_server_version(version.as_deref())?;
        if response.status() != StatusCode::OK {
            return Err(http_status_error(
                operation,
                response.status(),
                self.endpoint_kind(),
            ));
        }
        Ok(LocalWatchStream::new(response.into_body(), connection))
    }

    async fn snapshot_response(
        &self,
        operation: &'static str,
        path: &'static str,
        cancellation: &Cancellation,
    ) -> Result<(Response<Incoming>, ConnectionTask), LocalDaemonError> {
        let (response, connection) = with_deadline(
            self.timeout,
            operation,
            cancellation,
            self.open_response(operation, path, true),
        )
        .await?;
        server_version(&response, operation)?;
        Ok((response, connection))
    }

    async fn read_snapshot_body(
        &self,
        operation: &'static str,
        response: Response<Incoming>,
        mut connection: ConnectionTask,
        cancellation: &Cancellation,
        endpoint_kind: LocalEndpointKind,
    ) -> Result<Vec<u8>, LocalDaemonError> {
        let status = response.status();
        if status != StatusCode::OK {
            connection.abort();
            return Err(http_status_error(operation, status, endpoint_kind));
        }
        let body = Limited::new(response.into_body(), MAX_BODY_BYTES);
        let collected = with_deadline(self.timeout, operation, cancellation, async move {
            body.collect()
                .await
                .map_err(|error| LocalDaemonError::Protocol {
                    operation: operation.to_owned(),
                    detail: bounded_detail(&format!("response body failed: {error}")),
                })
        })
        .await?;
        let bytes = collected.to_bytes();
        connection.finish().await;
        Ok(bytes.to_vec())
    }

    async fn open_response(
        &self,
        operation: &'static str,
        path: &str,
        close: bool,
    ) -> Result<(Response<Incoming>, ConnectionTask), LocalDaemonError> {
        let stream = connect_stream(&self.socket_path, operation).await?;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| LocalDaemonError::Connection {
                operation: operation.to_owned(),
                detail: bounded_detail(&error.to_string()),
            })?;
        let task = tokio::spawn(async move {
            let _ = connection.await;
        });
        let request = local_request(path, close).map_err(|error| LocalDaemonError::Protocol {
            operation: operation.to_owned(),
            detail: bounded_detail(&error.to_string()),
        })?;
        match sender.send_request(request).await {
            Ok(response) => Ok((response, ConnectionTask::new(task))),
            Err(error) => {
                task.abort();
                Err(LocalDaemonError::Connection {
                    operation: operation.to_owned(),
                    detail: bounded_detail(&error.to_string()),
                })
            }
        }
    }
}

fn local_request(path: &str, close: bool) -> Result<Request<Empty<Bytes>>, hyper::http::Error> {
    let mut builder = Request::builder()
        .method(Method::GET)
        .uri(format!("http://{LOCAL_API_HOST}{path}"))
        .header(HOST, HeaderValue::from_static(LOCAL_API_HOST))
        .header(
            "Tailscale-Cap",
            HeaderValue::from_static(LOCAL_API_CAPABILITY),
        );
    if close {
        builder = builder.header("Connection", HeaderValue::from_static("close"));
    }
    builder.body(Empty::new())
}

fn server_version(
    response: &Response<Incoming>,
    operation: &str,
) -> Result<Option<String>, LocalDaemonError> {
    let version = response
        .headers()
        .get("Tailscale-Version")
        .map(|value| {
            value
                .to_str()
                .map(str::to_owned)
                .map_err(|error| LocalDaemonError::Protocol {
                    operation: operation.to_owned(),
                    detail: bounded_detail(&format!("server version header was invalid: {error}")),
                })
        })
        .transpose()?;
    validate_server_version(version.as_deref())?;
    Ok(version)
}

fn validate_server_version(version: Option<&str>) -> Result<(), LocalDaemonError> {
    if let Some(version) = version
        && !version.eq(EXPECTED_CLIENT_FAMILY)
        && !version
            .strip_prefix(EXPECTED_CLIENT_FAMILY)
            .is_some_and(|suffix| suffix.starts_with([' ', '-', '+']))
    {
        return Err(LocalDaemonError::UnsupportedVersion {
            expected: EXPECTED_CLIENT_FAMILY.to_owned(),
            actual: bounded_detail(version),
        });
    }
    Ok(())
}

fn http_status_error(
    operation: &str,
    status: StatusCode,
    endpoint_kind: LocalEndpointKind,
) -> LocalDaemonError {
    LocalDaemonError::HttpStatus {
        operation: operation.to_owned(),
        status: status.as_u16(),
        detail: if status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED {
            format!("GET {operation} over {endpoint_kind}: LocalAPI permission was denied")
        } else {
            format!("GET {operation} over {endpoint_kind}: LocalAPI returned a non-success status")
        },
    }
}

async fn with_deadline<T, F>(
    timeout: Duration,
    operation: &str,
    cancellation: &Cancellation,
    future: F,
) -> Result<T, LocalDaemonError>
where
    F: Future<Output = Result<T, LocalDaemonError>>,
{
    let guarded = async {
        tokio::select! {
            result = future => result,
            () = cancellation_wait(cancellation) => Err(LocalDaemonError::Cancelled),
        }
    };
    match tokio::time::timeout(timeout, guarded).await {
        Ok(result) => result,
        Err(_) => Err(LocalDaemonError::TimedOut {
            operation: operation.to_owned(),
        }),
    }
}

async fn cancellation_wait(cancellation: &Cancellation) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}

#[cfg(unix)]
enum LocalStream {
    Unix(tokio::net::UnixStream),
}

#[cfg(windows)]
enum LocalStream {
    Pipe(tokio::net::windows::named_pipe::NamedPipeClient),
}

impl AsyncRead for LocalStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(stream) => Pin::new(stream).poll_read(context, buffer),
            #[cfg(windows)]
            Self::Pipe(stream) => Pin::new(stream).poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for LocalStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(stream) => Pin::new(stream).poll_write(context, bytes),
            #[cfg(windows)]
            Self::Pipe(stream) => Pin::new(stream).poll_write(context, bytes),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(stream) => Pin::new(stream).poll_flush(context),
            #[cfg(windows)]
            Self::Pipe(stream) => Pin::new(stream).poll_flush(context),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(stream) => Pin::new(stream).poll_shutdown(context),
            #[cfg(windows)]
            Self::Pipe(stream) => Pin::new(stream).poll_shutdown(context),
        }
    }
}

#[cfg(unix)]
async fn connect_stream(path: &Path, operation: &str) -> Result<LocalStream, LocalDaemonError> {
    tokio::net::UnixStream::connect(path)
        .await
        .map(LocalStream::Unix)
        .map_err(|error| LocalDaemonError::Connection {
            operation: operation.to_owned(),
            detail: bounded_detail(&error.to_string()),
        })
}

#[cfg(windows)]
async fn connect_stream(path: &Path, operation: &str) -> Result<LocalStream, LocalDaemonError> {
    use tokio::net::windows::named_pipe::ClientOptions;

    let mut stream =
        ClientOptions::new()
            .open(path)
            .map_err(|error| LocalDaemonError::Connection {
                operation: operation.to_owned(),
                detail: bounded_detail(&error.to_string()),
            })?;
    stream
        .connect()
        .await
        .map_err(|error| LocalDaemonError::Connection {
            operation: operation.to_owned(),
            detail: bounded_detail(&error.to_string()),
        })?;
    Ok(LocalStream::Pipe(stream))
}

#[cfg(not(any(unix, windows)))]
async fn connect_stream(_path: &Path, _operation: &str) -> Result<LocalStream, LocalDaemonError> {
    Err(LocalDaemonError::UnsupportedPlatform)
}

#[derive(Debug)]
struct ConnectionTask {
    handle: Option<JoinHandle<()>>,
}

impl ConnectionTask {
    fn new(handle: JoinHandle<()>) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    async fn finish(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }

    fn abort(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

impl Drop for ConnectionTask {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

#[derive(Debug)]
pub struct LocalWatchStream {
    body: Incoming,
    connection: ConnectionTask,
    decoder: NewlineJsonDecoder,
    pending: VecDeque<WatchNotification>,
}

impl LocalWatchStream {
    fn new(body: Incoming, connection: ConnectionTask) -> Self {
        Self {
            body,
            connection,
            decoder: NewlineJsonDecoder::new(MAX_NOTIFICATION_BYTES),
            pending: VecDeque::new(),
        }
    }

    pub async fn next(
        &mut self,
        cancellation: &Cancellation,
    ) -> Result<Option<WatchNotification>, LocalDaemonError> {
        if let Some(notification) = self.pending.pop_front() {
            return Ok(Some(notification));
        }
        loop {
            let frame = tokio::select! {
                frame = self.body.frame() => frame,
                () = cancellation_wait(cancellation) => {
                    self.connection.abort();
                    return Err(LocalDaemonError::Cancelled);
                }
            };
            let Some(frame) = frame else {
                self.decoder.finish("watch-ipn-bus")?;
                self.connection.finish().await;
                return Ok(None);
            };
            let frame = frame.map_err(|error| LocalDaemonError::Connection {
                operation: "watch-ipn-bus".to_owned(),
                detail: bounded_detail(&error.to_string()),
            })?;
            let Ok(data) = frame.into_data() else {
                continue;
            };
            for value in self.decoder.push(&data, "watch-ipn-bus")? {
                if self.pending.len() >= 256 {
                    return Err(LocalDaemonError::Protocol {
                        operation: "watch-ipn-bus".to_owned(),
                        detail: "too many notifications were buffered in one body frame".to_owned(),
                    });
                }
                self.pending.push_back(value);
            }
            if let Some(notification) = self.pending.pop_front() {
                return Ok(Some(notification));
            }
        }
    }
}

#[derive(Debug)]
pub struct NewlineJsonDecoder {
    buffer: Vec<u8>,
    limit: usize,
}

impl NewlineJsonDecoder {
    pub fn new(limit: usize) -> Self {
        Self {
            buffer: Vec::new(),
            limit,
        }
    }

    pub fn push(
        &mut self,
        bytes: &[u8],
        operation: &str,
    ) -> Result<Vec<WatchNotification>, LocalDaemonError> {
        let mut values = Vec::new();
        for byte in bytes {
            if *byte == b'\n' {
                if self.buffer.is_empty() {
                    return Err(LocalDaemonError::Protocol {
                        operation: operation.to_owned(),
                        detail: "empty notification frame".to_owned(),
                    });
                }
                let line = std::mem::take(&mut self.buffer);
                let value = decode_notification(&line, operation)?;
                values.push(value);
            } else {
                if *byte == b'\r' {
                    return Err(LocalDaemonError::Protocol {
                        operation: operation.to_owned(),
                        detail: "CRLF framing is not part of the pinned watch contract".to_owned(),
                    });
                }
                if self.buffer.len() >= self.limit {
                    return Err(LocalDaemonError::Protocol {
                        operation: operation.to_owned(),
                        detail: "notification exceeded the bounded frame size".to_owned(),
                    });
                }
                self.buffer.push(*byte);
            }
        }
        Ok(values)
    }

    pub fn finish(&mut self, operation: &str) -> Result<(), LocalDaemonError> {
        if self.buffer.is_empty() {
            Ok(())
        } else {
            Err(LocalDaemonError::Protocol {
                operation: operation.to_owned(),
                detail: "watch stream closed with an unterminated notification".to_owned(),
            })
        }
    }
}

fn decode_notification(
    bytes: &[u8],
    operation: &str,
) -> Result<WatchNotification, LocalDaemonError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| LocalDaemonError::Protocol {
            operation: operation.to_owned(),
            detail: bounded_detail(&format!("notification JSON was invalid: {error}")),
        })?;
    let object = value
        .as_object()
        .ok_or_else(|| LocalDaemonError::Protocol {
            operation: operation.to_owned(),
            detail: "notification envelope was not a JSON object".to_owned(),
        })?;
    let status = [
        "State",
        "NetMap",
        "SelfChange",
        "PeerChanges",
        "Engine",
        "Health",
        "LoginFinished",
        "ClientVersion",
    ]
    .iter()
    .any(|field| object.get(*field).is_some_and(|value| !value.is_null()));
    let preferences = match object.get("Prefs") {
        None | Some(Value::Null) => false,
        Some(Value::Object(_)) => true,
        Some(_) => {
            return Err(LocalDaemonError::Protocol {
                operation: operation.to_owned(),
                detail: "notification Prefs field was not an object or null".to_owned(),
            });
        }
    };
    if let Some(error_value) = object.get("ErrMessage")
        && !error_value.is_null()
    {
        let error = error_value
            .as_str()
            .ok_or_else(|| LocalDaemonError::Protocol {
                operation: operation.to_owned(),
                detail: "notification ErrMessage field was not a string or null".to_owned(),
            })?;
        if !error.is_empty() {
            return Ok(WatchNotification {
                invalidation: WatchInvalidation::DaemonError {
                    detail: bounded_detail(error),
                },
            });
        }
    }
    let invalidation = match (status, preferences) {
        (true, true) => WatchInvalidation::Both,
        (true, false) => WatchInvalidation::Status,
        (false, true) => WatchInvalidation::Preferences,
        (false, false) => WatchInvalidation::None,
    };
    Ok(WatchNotification { invalidation })
}

pub fn decode_preferences(
    input: &[u8],
    observed_at: Timestamp,
) -> Result<LocalPreferences, LocalDaemonError> {
    let value: Value = serde_json::from_slice(input).map_err(|error| LocalDaemonError::Decode {
        operation: "preferences".to_owned(),
        detail: bounded_detail(&error.to_string()),
    })?;
    let object = value.as_object().ok_or_else(|| LocalDaemonError::Decode {
        operation: "preferences".to_owned(),
        detail: "preference response was not a JSON object".to_owned(),
    })?;
    let auto_update = object.get("AutoUpdate");
    let app_connector = object.get("AppConnector");
    let routes = optional_strings(object.get("AdvertiseRoutes"))?;
    let routes = routes
        .map(|values| {
            parse_route_set(&values.join(","))
                .map(|routes| routes.into_iter().map(|route| route.to_string()).collect())
                .map_err(|error| LocalDaemonError::Decode {
                    operation: "preferences".to_owned(),
                    detail: bounded_detail(&format!("advertised routes were invalid: {error}")),
                })
        })
        .transpose()?;
    let advertised_exit_node = routes.as_ref().map(|routes: &Vec<String>| {
        routes.iter().any(|route| route == "0.0.0.0/0")
            && routes.iter().any(|route| route == "::/0")
    });
    let relay_port_returned = object.get("RelayServerPort");
    let relay_port = match relay_port_returned {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|value| u16::try_from(value).ok())
                .ok_or_else(|| LocalDaemonError::Decode {
                    operation: "preferences".to_owned(),
                    detail: "relay server port was not an unsigned 16-bit integer".to_owned(),
                })?,
        ),
    };
    let mut preferences = LocalPreferences {
        want_running: observed_bool(object.get("WantRunning"), observed_at),
        logged_out: observed_bool(object.get("LoggedOut"), observed_at),
        accept_dns: observed_bool(object.get("CorpDNS"), observed_at),
        accept_routes: observed_bool(object.get("RouteAll"), observed_at),
        shields_up: observed_bool(object.get("ShieldsUp"), observed_at),
        ssh: observed_bool(object.get("RunSSH"), observed_at),
        update_check: observed_nested_bool(auto_update, "Check", observed_at),
        automatic_update: observed_nested_bool(auto_update, "Apply", observed_at),
        report_posture: observed_bool(object.get("PostureChecking"), observed_at),
        hostname: observed_string(object.get("Hostname"), observed_at),
        nickname: observed_string(object.get("ProfileName"), observed_at),
        web_client: observed_bool(object.get("RunWebClient"), observed_at),
        exit_node_id: observed_string(object.get("ExitNodeID"), observed_at),
        exit_node_ip: observed_string(object.get("ExitNodeIP"), observed_at),
        auto_exit_node: observed_auto_exit_node(object.get("AutoExitNode"), observed_at),
        exit_node_allow_lan_access: observed_bool(
            object.get("ExitNodeAllowLANAccess"),
            observed_at,
        ),
        advertised_routes: match routes {
            Some(value) => ObservedPreference::known(value, observed_at),
            None => ObservedPreference::unknown(observed_at),
        },
        advertised_exit_node: match advertised_exit_node {
            Some(value) => ObservedPreference::known(value, observed_at),
            None => ObservedPreference::unknown(observed_at),
        },
        app_connector: observed_nested_bool(app_connector, "Advertise", observed_at),
        relay_server_port: match relay_port {
            Some(value) => ObservedPreference::known(value, observed_at),
            None => ObservedPreference::unknown(observed_at),
        },
        relay_server_port_disabled: match relay_port_returned {
            Some(Value::Null) => ObservedPreference::known(true, observed_at),
            Some(_) => ObservedPreference::known(false, observed_at),
            None => ObservedPreference::unknown(observed_at),
        },
        relay_server_static_endpoints: match optional_strings(
            object.get("RelayServerStaticEndpoints"),
        )? {
            Some(value) => {
                let endpoints = parse_static_endpoints(&value.join(",")).map_err(|error| {
                    LocalDaemonError::Decode {
                        operation: "preferences".to_owned(),
                        detail: bounded_detail(&format!(
                            "relay static endpoints were invalid: {error}"
                        )),
                    }
                })?;
                ObservedPreference::known(
                    endpoints.iter().map(ToString::to_string).collect(),
                    observed_at,
                )
            }
            None => ObservedPreference::unknown(observed_at),
        },
    };
    if preferences.logged_out.value == Some(true) {
        preferences.want_running.editability =
            crate::domain::preference::PreferenceEditability::Unsupported;
    }
    Ok(preferences)
}

fn observed_bool(value: Option<&Value>, observed_at: Timestamp) -> ObservedPreference<bool> {
    match value.and_then(Value::as_bool) {
        Some(value) => ObservedPreference::known(value, observed_at),
        None => ObservedPreference::unknown(observed_at),
    }
}

fn observed_nested_bool(
    value: Option<&Value>,
    name: &str,
    observed_at: Timestamp,
) -> ObservedPreference<bool> {
    observed_bool(value.and_then(|value| value.get(name)), observed_at)
}

fn observed_auto_exit_node(
    value: Option<&Value>,
    observed_at: Timestamp,
) -> ObservedPreference<bool> {
    match value {
        Some(Value::String(value)) if value.is_empty() => {
            ObservedPreference::known(false, observed_at)
        }
        Some(Value::String(value)) if value == "any" => {
            ObservedPreference::known(true, observed_at)
        }
        _ => ObservedPreference::unknown(observed_at),
    }
}

fn observed_string(value: Option<&Value>, observed_at: Timestamp) -> ObservedPreference<String> {
    match value.and_then(Value::as_str) {
        Some(value) => ObservedPreference::known(value.to_owned(), observed_at),
        None => ObservedPreference::unknown(observed_at),
    }
}

fn optional_strings(value: Option<&Value>) -> Result<Option<Vec<String>>, LocalDaemonError> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| LocalDaemonError::Decode {
                        operation: "preferences".to_owned(),
                        detail: "preference list contained a non-string value".to_owned(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Value::Null => Ok(None),
        _ => Err(LocalDaemonError::Decode {
            operation: "preferences".to_owned(),
            detail: "preference list was not an array".to_owned(),
        }),
    }
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

impl fmt::Display for LocalEndpointKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}
