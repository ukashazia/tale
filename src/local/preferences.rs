use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::domain::Timestamp;
use crate::domain::preference::{LocalPreferences, ObservedPreference, PreferenceRequest};
use crate::domain::route::{
    AdvertisementRequest, ExitNodeRequest, ExitNodeSelection, format_route_set,
    format_static_endpoints, parse_route_set, parse_static_endpoints,
};
use crate::local::client::HostPlatform;
use crate::local::process::Cancellation;

const LOCAL_API_PATH: &str = "/localapi/v0/prefs";
const LOCAL_API_HOST: &str = "local-tailscaled.sock";
const COMPATIBILITY_VERSION: &str = "1.98.9";
const COMPATIBILITY_CAPABILITY: &str = "138";
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PreferencePlatform {
    Linux,
    MacOS,
    Windows,
}

impl PreferencePlatform {
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOS
        } else {
            Self::Linux
        }
    }

    pub const fn from_host(platform: HostPlatform) -> Self {
        match platform {
            HostPlatform::Windows => Self::Windows,
            HostPlatform::Unix => {
                if cfg!(target_os = "macos") {
                    Self::MacOS
                } else {
                    Self::Linux
                }
            }
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::MacOS => "macOS standalone",
            Self::Windows => "windows",
        }
    }
}

pub fn documented_socket_path(platform: PreferencePlatform) -> PathBuf {
    match platform {
        PreferencePlatform::Linux => PathBuf::from("/var/run/tailscale/tailscaled.sock"),
        PreferencePlatform::MacOS => PathBuf::from("/var/run/tailscaled.socket"),
        PreferencePlatform::Windows => {
            PathBuf::from(r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled")
        }
    }
}

pub fn local_api_path() -> &'static str {
    LOCAL_API_PATH
}

pub fn local_api_host() -> &'static str {
    LOCAL_API_HOST
}

pub fn set_command(
    path: &Path,
    timeout: Duration,
    request: &PreferenceRequest,
) -> Result<crate::local::process::LocalCommand, PreferenceCommandError> {
    if request.is_empty() {
        return Err(PreferenceCommandError::EmptyRequest);
    }
    let mut args = vec![std::ffi::OsString::from("set")];
    if let Some(value) = request.accept_dns {
        args.push(std::ffi::OsString::from(format!("--accept-dns={value}")));
    }
    if let Some(value) = request.accept_routes {
        args.push(std::ffi::OsString::from(format!("--accept-routes={value}")));
    }
    if let Some(value) = request.shields_up {
        args.push(std::ffi::OsString::from(format!("--shields-up={value}")));
    }
    if let Some(value) = request.ssh {
        args.push(std::ffi::OsString::from(format!("--ssh={value}")));
    }
    if let Some(value) = request.automatic_update {
        args.push(std::ffi::OsString::from(format!("--auto-update={value}")));
    }
    if let Some(value) = request.update_check {
        args.push(std::ffi::OsString::from(format!("--update-check={value}")));
    }
    if let Some(value) = request.report_posture {
        args.push(std::ffi::OsString::from(format!(
            "--report-posture={value}"
        )));
    }
    if let Some(value) = request.hostname.as_deref() {
        validate_text(value, "hostname")?;
        args.push(std::ffi::OsString::from(format!("--hostname={value}")));
    }
    if let Some(value) = request.nickname.as_deref() {
        validate_text(value, "nickname")?;
        args.push(std::ffi::OsString::from(format!("--nickname={value}")));
    }
    if let Some(value) = request.web_client {
        args.push(std::ffi::OsString::from(format!("--webclient={value}")));
    }
    Ok(crate::local::process::LocalCommand::new(
        path.as_os_str().to_os_string(),
        crate::local::process::LocalOperation::Set,
        args,
    )
    .with_timeout(timeout))
}

pub fn exit_node_command(
    path: &Path,
    timeout: Duration,
    request: &ExitNodeRequest,
) -> crate::local::process::LocalCommand {
    let target = match &request.selection {
        ExitNodeSelection::None => "".to_owned(),
        ExitNodeSelection::Device { target, .. } => target.clone(),
        ExitNodeSelection::AutoAny => "auto:any".to_owned(),
    };
    crate::local::process::LocalCommand::new(
        path.as_os_str().to_os_string(),
        crate::local::process::LocalOperation::Set,
        vec![
            std::ffi::OsString::from("set"),
            std::ffi::OsString::from(format!("--exit-node={target}")),
            std::ffi::OsString::from(format!(
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
) -> Result<crate::local::process::LocalCommand, PreferenceCommandError> {
    if request.is_empty() {
        return Err(PreferenceCommandError::EmptyRequest);
    }
    if request.advertise_connector == Some(true) && !request.accept_mac_app_connector_risk {
        return Err(PreferenceCommandError::MissingMacAppConnectorRisk);
    }
    if request.accept_mac_app_connector_risk && request.advertise_connector != Some(true) {
        return Err(PreferenceCommandError::UnexpectedMacAppConnectorRisk);
    }
    let mut args = vec![std::ffi::OsString::from("set")];
    if let Some(routes) = request.canonical_routes() {
        args.push(std::ffi::OsString::from(format!(
            "--advertise-routes={}",
            format_route_set(&routes)
        )));
    }
    if let Some(value) = request.advertise_exit_node {
        args.push(std::ffi::OsString::from(format!(
            "--advertise-exit-node={value}"
        )));
    }
    if let Some(value) = request.advertise_connector {
        args.push(std::ffi::OsString::from(format!(
            "--advertise-connector={value}"
        )));
        if value && request.accept_mac_app_connector_risk {
            args.push(std::ffi::OsString::from("--accept-risk=mac-app-connector"));
        }
    }
    if let Some(port) = request.relay_server_port {
        let value = port.map_or_else(String::new, |port| port.to_string());
        args.push(std::ffi::OsString::from(format!(
            "--relay-server-port={value}"
        )));
    }
    if let Some(endpoints) = request.relay_server_static_endpoints.as_deref() {
        args.push(std::ffi::OsString::from(format!(
            "--relay-server-static-endpoints={}",
            format_static_endpoints(endpoints)
        )));
    }
    Ok(crate::local::process::LocalCommand::new(
        path.as_os_str().to_os_string(),
        crate::local::process::LocalOperation::Set,
        args,
    )
    .with_timeout(timeout))
}

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

fn validate_text(value: &str, field: &str) -> Result<(), PreferenceCommandError> {
    if value.is_empty() {
        Err(PreferenceCommandError::EmptyText {
            field: field.to_owned(),
        })
    } else {
        Ok(())
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum PreferenceError {
    #[error("preference transport is unsupported on {platform}")]
    UnsupportedPlatform { platform: String },
    #[error("preference transport requires Tailscale {expected}, got {actual}")]
    UnsupportedVersion { expected: String, actual: String },
    #[error("preference LocalAPI connection failed: {detail}")]
    Connection { detail: String },
    #[error("preference LocalAPI request timed out")]
    TimedOut,
    #[error("preference LocalAPI request cancelled")]
    Cancelled,
    #[error("preference LocalAPI permission denied")]
    PermissionDenied,
    #[error("preference LocalAPI returned HTTP status {status}")]
    HttpStatus { status: u16 },
    #[error("preference LocalAPI response was invalid: {detail}")]
    InvalidResponse { detail: String },
    #[error("preference JSON was invalid: {detail}")]
    InvalidJson { detail: String },
}

impl PreferenceError {
    pub const fn operation(&self) -> &'static str {
        match self {
            Self::UnsupportedPlatform { .. } | Self::UnsupportedVersion { .. } => "preferences",
            Self::Connection { .. } => "preferences transport",
            Self::TimedOut => "preferences timeout",
            Self::Cancelled => "preferences cancellation",
            Self::PermissionDenied => "preferences permission",
            Self::HttpStatus { .. } => "preferences HTTP",
            Self::InvalidResponse { .. } => "preferences response",
            Self::InvalidJson { .. } => "preferences JSON",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreferenceClient {
    pub version: String,
    pub platform: PreferencePlatform,
    pub timeout: Duration,
    socket_path: Option<PathBuf>,
}

impl PreferenceClient {
    pub fn new(
        version: impl Into<String>,
        platform: PreferencePlatform,
        timeout: Duration,
    ) -> Self {
        Self {
            version: version.into(),
            platform,
            timeout,
            socket_path: None,
        }
    }

    pub fn with_socket_path(mut self, socket_path: impl Into<PathBuf>) -> Self {
        self.socket_path = Some(socket_path.into());
        self
    }

    pub fn socket_path(&self) -> PathBuf {
        match self.socket_path.clone() {
            Some(path) => path,
            None => documented_socket_path(self.platform),
        }
    }

    pub async fn get_prefs(
        &self,
        observed_at: Timestamp,
        cancellation: &Cancellation,
    ) -> Result<LocalPreferences, PreferenceError> {
        if self.version != COMPATIBILITY_VERSION {
            return Err(PreferenceError::UnsupportedVersion {
                expected: COMPATIBILITY_VERSION.to_owned(),
                actual: self.version.clone(),
            });
        }
        let response = self.request(cancellation).await?;
        decode_preferences(&response, observed_at)
    }

    async fn request(&self, cancellation: &Cancellation) -> Result<Vec<u8>, PreferenceError> {
        let path = self.socket_path();
        let operation = async {
            #[cfg(unix)]
            {
                let stream = tokio::net::UnixStream::connect(&path)
                    .await
                    .map_err(io_error)?;
                http_get(stream, cancellation).await
            }
            #[cfg(windows)]
            {
                use tokio::net::windows::named_pipe::ClientOptions;
                let mut stream = ClientOptions::new().open(&path).map_err(io_error)?;
                stream.connect().await.map_err(io_error)?;
                http_get(stream, cancellation).await
            }
            #[cfg(not(any(unix, windows)))]
            {
                let _ = path;
                let _ = cancellation;
                Err(PreferenceError::UnsupportedPlatform {
                    platform: std::env::consts::OS.to_owned(),
                })
            }
        };
        match tokio::time::timeout(self.timeout, operation).await {
            Ok(result) => result,
            Err(_) => Err(PreferenceError::TimedOut),
        }
    }
}

pub fn decode_preferences(
    input: &[u8],
    observed_at: Timestamp,
) -> Result<LocalPreferences, PreferenceError> {
    let text = std::str::from_utf8(input).map_err(|error| PreferenceError::InvalidJson {
        detail: format!("response was not UTF-8: {error}"),
    })?;
    let value: Value =
        serde_json::from_str(text).map_err(|error| PreferenceError::InvalidJson {
            detail: error.to_string(),
        })?;
    let object = value
        .as_object()
        .ok_or_else(|| PreferenceError::InvalidJson {
            detail: "preference response was not a JSON object".to_owned(),
        })?;
    let _ = object;
    let auto_update = get(&value, "AutoUpdate");
    let app_connector = get(&value, "AppConnector");
    let routes = optional_strings(get(&value, "AdvertiseRoutes"))?;
    let routes: Option<Vec<String>> = routes
        .map(|routes| {
            parse_route_set(&routes.join(","))
                .map(|routes| routes.into_iter().map(|route| route.to_string()).collect())
                .map_err(|error| PreferenceError::InvalidResponse {
                    detail: format!("advertised routes were invalid: {error}"),
                })
        })
        .transpose()?;
    let advertised_exit_node = routes.as_ref().map(|routes| {
        routes.iter().any(|route| route == "0.0.0.0/0")
            && routes.iter().any(|route| route == "::/0")
    });
    let relay_port_returned = get(&value, "RelayServerPort");
    let relay_port = match relay_port_returned {
        None | Some(Value::Null) => None,
        Some(value) => {
            Some(
                optional_u16(Some(value)).ok_or_else(|| PreferenceError::InvalidResponse {
                    detail: "relay server port was not an unsigned 16-bit integer".to_owned(),
                })?,
            )
        }
    };
    let relay_port_pref = match relay_port {
        Some(value) => ObservedPreference::known(value, observed_at),
        None => ObservedPreference::unknown(observed_at),
    };
    let relay_port_disabled = match relay_port_returned {
        Some(Value::Null) => ObservedPreference::known(true, observed_at),
        Some(_) => ObservedPreference::known(false, observed_at),
        None => ObservedPreference::unknown(observed_at),
    };
    let mut preferences = LocalPreferences {
        want_running: observed_bool(&value, "WantRunning", observed_at),
        logged_out: observed_bool(&value, "LoggedOut", observed_at),
        accept_dns: observed_bool(&value, "CorpDNS", observed_at),
        accept_routes: observed_bool(&value, "RouteAll", observed_at),
        shields_up: observed_bool(&value, "ShieldsUp", observed_at),
        ssh: observed_bool(&value, "RunSSH", observed_at),
        update_check: observed_nested_bool(auto_update, "Check", observed_at),
        automatic_update: observed_nested_bool(auto_update, "Apply", observed_at),
        report_posture: observed_bool(&value, "PostureChecking", observed_at),
        hostname: observed_string(&value, "Hostname", observed_at),
        nickname: observed_string(&value, "ProfileName", observed_at),
        web_client: observed_bool(&value, "RunWebClient", observed_at),
        exit_node_id: observed_string(&value, "ExitNodeID", observed_at),
        exit_node_ip: observed_string(&value, "ExitNodeIP", observed_at),
        auto_exit_node: observed_auto_exit_node(&value, observed_at),
        exit_node_allow_lan_access: observed_bool(&value, "ExitNodeAllowLANAccess", observed_at),
        advertised_routes: match routes {
            Some(value) => ObservedPreference::known(value, observed_at),
            None => ObservedPreference::unknown(observed_at),
        },
        advertised_exit_node: match advertised_exit_node {
            Some(value) => ObservedPreference::known(value, observed_at),
            None => ObservedPreference::unknown(observed_at),
        },
        app_connector: observed_nested_bool(app_connector, "Advertise", observed_at),
        relay_server_port: relay_port_pref,
        relay_server_port_disabled: relay_port_disabled,
        relay_server_static_endpoints: match optional_strings(get(
            &value,
            "RelayServerStaticEndpoints",
        ))? {
            Some(value) => {
                let endpoints = parse_static_endpoints(&value.join(",")).map_err(|error| {
                    PreferenceError::InvalidResponse {
                        detail: format!("relay static endpoints were invalid: {error}"),
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

pub fn request_bytes() -> Vec<u8> {
    format!(
        "GET {LOCAL_API_PATH} HTTP/1.1\r\nHost: {LOCAL_API_HOST}\r\nTailscale-Cap: {COMPATIBILITY_CAPABILITY}\r\nConnection: close\r\n\r\n"
    )
    .into_bytes()
}

async fn http_get<S>(mut stream: S, cancellation: &Cancellation) -> Result<Vec<u8>, PreferenceError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = request_bytes();
    write_with_cancellation(&mut stream, &request, cancellation).await?;
    let mut response = Vec::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        if cancellation.is_cancelled() {
            return Err(PreferenceError::Cancelled);
        }
        let count = tokio::select! {
            count = stream.read(&mut buffer) => count.map_err(io_error)?,
            () = tokio::time::sleep(Duration::from_millis(10)) => {
                if cancellation.is_cancelled() { return Err(PreferenceError::Cancelled); }
                continue;
            }
        };
        if count == 0 {
            if response.is_empty() {
                return Err(PreferenceError::InvalidResponse {
                    detail: "local API closed without a response".to_owned(),
                });
            }
            break;
        }
        if response.len().saturating_add(count) > MAX_RESPONSE_BYTES {
            return Err(PreferenceError::InvalidResponse {
                detail: "response exceeded the bounded capture size".to_owned(),
            });
        }
        response.extend_from_slice(&buffer[..count]);
    }
    parse_http_response(&response)
}

async fn write_with_cancellation<S>(
    stream: &mut S,
    bytes: &[u8],
    cancellation: &Cancellation,
) -> Result<(), PreferenceError>
where
    S: AsyncWrite + Unpin,
{
    let mut offset = 0usize;
    while offset < bytes.len() {
        if cancellation.is_cancelled() {
            return Err(PreferenceError::Cancelled);
        }
        let written = stream.write(&bytes[offset..]).await.map_err(io_error)?;
        if written == 0 {
            return Err(PreferenceError::Connection {
                detail: "local API closed during request".to_owned(),
            });
        }
        offset = offset.saturating_add(written);
    }
    stream.flush().await.map_err(io_error)
}

fn parse_http_response(response: &[u8]) -> Result<Vec<u8>, PreferenceError> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position.saturating_add(4))
        .ok_or_else(|| PreferenceError::InvalidResponse {
            detail: "HTTP headers were incomplete".to_owned(),
        })?;
    let headers = std::str::from_utf8(&response[..header_end]).map_err(|error| {
        PreferenceError::InvalidResponse {
            detail: format!("HTTP headers were not UTF-8: {error}"),
        }
    })?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| PreferenceError::InvalidResponse {
            detail: "HTTP status was not returned".to_owned(),
        })?;
    if status == 401 || status == 403 {
        return Err(PreferenceError::PermissionDenied);
    }
    if status != 200 {
        return Err(PreferenceError::HttpStatus { status });
    }
    let body = response.get(header_end..).map_or(&[][..], |value| value);
    if body.is_empty() {
        return Err(PreferenceError::InvalidResponse {
            detail: "HTTP body was empty".to_owned(),
        });
    }
    Ok(body.to_vec())
}

fn io_error(error: io::Error) -> PreferenceError {
    if error.kind() == io::ErrorKind::PermissionDenied {
        PreferenceError::PermissionDenied
    } else {
        PreferenceError::Connection {
            detail: error.to_string(),
        }
    }
}

fn observed_bool(value: &Value, name: &str, observed_at: Timestamp) -> ObservedPreference<bool> {
    match get(value, name).and_then(Value::as_bool) {
        Some(value) => ObservedPreference::known(value, observed_at),
        None => ObservedPreference::unknown(observed_at),
    }
}

fn observed_nested_bool(
    value: Option<&Value>,
    name: &str,
    observed_at: Timestamp,
) -> ObservedPreference<bool> {
    match value
        .and_then(|value| get(value, name))
        .and_then(Value::as_bool)
    {
        Some(value) => ObservedPreference::known(value, observed_at),
        None => ObservedPreference::unknown(observed_at),
    }
}

fn observed_auto_exit_node(value: &Value, observed_at: Timestamp) -> ObservedPreference<bool> {
    match get(value, "AutoExitNode") {
        Some(Value::String(value)) if value.is_empty() => {
            ObservedPreference::known(false, observed_at)
        }
        Some(Value::String(value)) if value == "any" => {
            ObservedPreference::known(true, observed_at)
        }
        Some(Value::String(_)) => ObservedPreference::unknown(observed_at),
        Some(Value::Null) | None => ObservedPreference::unknown(observed_at),
        Some(_) => ObservedPreference::unknown(observed_at),
    }
}

fn observed_string(
    value: &Value,
    name: &str,
    observed_at: Timestamp,
) -> ObservedPreference<String> {
    match get(value, name).and_then(Value::as_str) {
        Some(value) => ObservedPreference::known(value.to_owned(), observed_at),
        None => ObservedPreference::unknown(observed_at),
    }
}

fn optional_strings(value: Option<&Value>) -> Result<Option<Vec<String>>, PreferenceError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let values = match value {
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                    PreferenceError::InvalidResponse {
                        detail: "preference list contained a non-string value".to_owned(),
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        Value::Null => return Ok(None),
        _ => {
            return Err(PreferenceError::InvalidResponse {
                detail: "preference list was not an array".to_owned(),
            });
        }
    };
    Ok(Some(values))
}

fn optional_u16(value: Option<&Value>) -> Option<u16> {
    let value = value?.as_u64()?;
    u16::try_from(value).ok()
}

fn get<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.as_object()?.get(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_is_fixed_and_structured() {
        let request = String::from_utf8(request_bytes());
        assert!(request.is_ok());
        if let Ok(request) = request {
            assert!(request.contains("GET /localapi/v0/prefs HTTP/1.1"));
            assert!(request.contains("Host: local-tailscaled.sock"));
            assert!(request.contains("Tailscale-Cap: 138"));
        }
    }

    #[test]
    fn fixture_preserves_unknown_and_optional_values() {
        let fixture = br#"{
            "WantRunning": true,
            "LoggedOut": false,
            "CorpDNS": true,
            "RouteAll": false,
            "ShieldsUp": false,
            "RunSSH": true,
            "AutoUpdate": {"Check": true},
            "PostureChecking": true,
            "Hostname": "build-01",
            "ProfileName": "work",
            "ExitNodeID": "nodekey:exit",
            "ExitNodeAllowLANAccess": true,
            "AdvertiseRoutes": ["10.0.0.0/8", "0.0.0.0/0", "::/0"],
            "AppConnector": {"Advertise": false},
            "RelayServerPort": 0,
            "RelayServerStaticEndpoints": ["203.0.113.10:443"],
            "NewField": "ignored"
        }"#;
        let decoded = decode_preferences(fixture, 7);
        assert!(decoded.is_ok());
        if let Ok(decoded) = decoded {
            assert_eq!(decoded.accept_dns.value, Some(true));
            assert_eq!(decoded.automatic_update.value, None);
            assert_eq!(decoded.advertised_exit_node.value, Some(true));
            assert_eq!(decoded.relay_server_port.value, Some(0));
        }
    }

    #[test]
    fn exit_node_command_includes_set_and_preserves_explicit_empty_target() {
        let request = ExitNodeRequest {
            selection: ExitNodeSelection::None,
            allow_lan_access: false,
        };
        let command = exit_node_command(Path::new("tailscale"), Duration::from_secs(1), &request);
        assert_eq!(
            command.args,
            vec![
                std::ffi::OsString::from("set"),
                std::ffi::OsString::from("--exit-node="),
                std::ffi::OsString::from("--exit-node-allow-lan-access=false"),
            ]
        );
    }

    #[test]
    fn malformed_route_or_endpoint_data_is_rejected() {
        let route = decode_preferences(br#"{"AdvertiseRoutes":["not-a-route"]}"#, 1);
        assert!(matches!(
            route,
            Err(PreferenceError::InvalidResponse { .. })
        ));
        let endpoint =
            decode_preferences(br#"{"RelayServerStaticEndpoints":["not-an-endpoint"]}"#, 1);
        assert!(matches!(
            endpoint,
            Err(PreferenceError::InvalidResponse { .. })
        ));
    }

    #[test]
    fn auto_exit_expression_is_observed_without_treating_missing_as_false() {
        let automatic = decode_preferences(br#"{"AutoExitNode":"any"}"#, 1);
        assert!(automatic.is_ok());
        if let Ok(automatic) = automatic {
            assert_eq!(automatic.auto_exit_node.value, Some(true));
        }
        let unknown = decode_preferences(br#"{"AutoExitNode":"unexpected"}"#, 1);
        assert!(unknown.is_ok());
        if let Ok(unknown) = unknown {
            assert_eq!(unknown.auto_exit_node.value, None);
        }
        let missing = decode_preferences(br#"{}"#, 1);
        assert!(missing.is_ok());
        if let Ok(missing) = missing {
            assert_eq!(missing.auto_exit_node.value, None);
            assert!(!missing.auto_exit_node.can_edit());
        }
    }

    #[test]
    fn http_errors_are_classified_without_body_scraping() {
        let result = parse_http_response(b"HTTP/1.1 403 Forbidden\r\n\r\n{}");
        assert_eq!(result, Err(PreferenceError::PermissionDenied));
        let result = parse_http_response(b"HTTP/1.1 404 Not Found\r\n\r\n{}");
        assert_eq!(result, Err(PreferenceError::HttpStatus { status: 404 }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn get_prefs_uses_the_documented_socket_and_http_contract() {
        use std::fs;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::UnixListener;

        let socket = std::env::temp_dir().join(format!("tale-prefs-{}.sock", std::process::id()));
        let _ = fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket);
        assert!(listener.is_ok());
        let Some(listener) = listener.ok() else {
            return;
        };
        let server = tokio::spawn(async move {
            let accepted = listener.accept().await;
            let Ok((mut stream, _)) = accepted else {
                return Err("accept failed".to_owned());
            };
            let mut request = Vec::new();
            let mut buffer = [0_u8; 256];
            loop {
                let count = stream
                    .read(&mut buffer)
                    .await
                    .map_err(|error| error.to_string())?;
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"WantRunning\":true,\"CorpDNS\":true}")
                .await
                .map_err(|error| error.to_string())?;
            Ok::<Vec<u8>, String>(request)
        });
        let client =
            PreferenceClient::new("1.98.9", PreferencePlatform::Linux, Duration::from_secs(2))
                .with_socket_path(&socket);
        let preferences = client.get_prefs(42, &Cancellation::new()).await;
        let request = server.await;
        let _ = fs::remove_file(&socket);
        assert!(preferences.is_ok());
        assert!(request.is_ok());
        if let (Ok(preferences), Ok(Ok(request))) = (preferences, request) {
            assert_eq!(preferences.want_running.value, Some(true));
            assert_eq!(preferences.accept_dns.value, Some(true));
            let request = String::from_utf8(request);
            assert!(request.is_ok());
            if let Ok(request) = request {
                assert!(request.starts_with("GET /localapi/v0/prefs HTTP/1.1\r\n"));
                assert!(request.contains("Host: local-tailscaled.sock\r\n"));
                assert!(request.contains("Tailscale-Cap: 138\r\n"));
            }
        }
    }
}
