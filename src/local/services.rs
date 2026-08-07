use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::domain::service::{
    Backend, Exposure, FunnelStatus, Listener, PathMount, Port, ProxyProtocol, ServeStatus,
    ServiceMapping, ServiceValueError,
};
use crate::local::process::{LocalCommand, LocalOperation};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServiceDecodeError {
    pub detail: String,
}

impl ServiceDecodeError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for ServiceDecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for ServiceDecodeError {}

#[derive(Debug, Deserialize)]
struct ServeStatusDtoV1989 {
    #[serde(alias = "Mappings")]
    mappings: Option<Value>,
    #[serde(rename = "TCP", alias = "tcp")]
    tcp: Option<Value>,
    #[serde(rename = "Web", alias = "web")]
    web: Option<Value>,
    #[serde(rename = "AllowFunnel", alias = "allowFunnel")]
    allow_funnel: Option<Value>,
    #[serde(flatten)]
    additive: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct FunnelStatusDtoV1989 {
    #[serde(alias = "Mappings")]
    mappings: Option<Value>,
    #[serde(rename = "TCP", alias = "tcp")]
    tcp: Option<Value>,
    #[serde(rename = "Web", alias = "web")]
    web: Option<Value>,
    #[serde(rename = "AllowFunnel", alias = "allowFunnel")]
    allow_funnel: Option<Value>,
    #[serde(flatten)]
    additive: BTreeMap<String, Value>,
}

pub fn serve_status_command(path: &Path, timeout: Duration) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::ServeStatus,
        vec![
            OsString::from("serve"),
            OsString::from("status"),
            OsString::from("--json"),
        ],
    )
    .with_timeout(timeout)
}

pub fn funnel_status_command(path: &Path, timeout: Duration) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::FunnelStatus,
        vec![
            OsString::from("funnel"),
            OsString::from("status"),
            OsString::from("--json"),
        ],
    )
    .with_timeout(timeout)
}

pub fn serve_reset_command(path: &Path, timeout: Duration) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::ServeReset,
        vec![OsString::from("serve"), OsString::from("reset")],
    )
    .with_timeout(timeout)
}

pub fn funnel_reset_command(path: &Path, timeout: Duration) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::FunnelReset,
        vec![OsString::from("funnel"), OsString::from("reset")],
    )
    .with_timeout(timeout)
}

pub fn mapping_command(
    path: &Path,
    timeout: Duration,
    mapping: &ServiceMapping,
    confirmed: bool,
) -> Result<LocalCommand, ServiceValueError> {
    mapping.validate()?;
    if !confirmed {
        return Err(ServiceValueError(
            "service commands require an accepted confirmation".to_owned(),
        ));
    }
    let command = match mapping.exposure {
        Exposure::Tailnet => "serve",
        Exposure::Public => "funnel",
    };
    let operation = if matches!(mapping.exposure, Exposure::Tailnet) {
        LocalOperation::Serve
    } else {
        LocalOperation::Funnel
    };
    if matches!(mapping.exposure, Exposure::Public) && matches!(mapping.listener, Listener::Http(_))
    {
        return Err(ServiceValueError(
            "Funnel does not offer HTTP listeners".to_owned(),
        ));
    }
    let mut args = vec![
        OsString::from(command),
        OsString::from("--bg"),
        OsString::from("--yes"),
        OsString::from(listener_flag(&mapping.listener)),
    ];
    if let PathMount::Path(path) = &mapping.mount {
        args.push(OsString::from(format!("--set-path={path}")));
    }
    if let Some(proxy) = mapping.proxy_protocol.cli_value() {
        args.push(OsString::from(format!("--proxy-protocol={proxy}")));
    }
    args.push(OsString::from(mapping.backend.argument()));
    Ok(LocalCommand::new(path.as_os_str().to_os_string(), operation, args).with_timeout(timeout))
}

/// Take down one mapping. `funnel … off` and `serve … off` are the same
/// operation in the CLI — both delete the handler — but `funnel` first insists
/// the node advertises Funnel, so the narrower command is always the right one.
///
/// `--set-path` is mandatory for HTTP and HTTPS: without it the CLI removes
/// every mount on the port rather than the selected one.
pub fn mapping_off_command(
    path: &Path,
    timeout: Duration,
    mapping: &ServiceMapping,
    confirmed: bool,
) -> Result<LocalCommand, ServiceValueError> {
    mapping.validate()?;
    if !confirmed {
        return Err(ServiceValueError(
            "service commands require an accepted confirmation".to_owned(),
        ));
    }
    let mut args = vec![
        OsString::from("serve"),
        OsString::from("--yes"),
        OsString::from(listener_flag(&mapping.listener)),
    ];
    if mapping.listener.allows_path() {
        args.push(OsString::from(format!(
            "--set-path={}",
            mapping.mount.as_path()
        )));
    }
    args.push(OsString::from("off"));
    Ok(LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::ServeOff,
        args,
    )
    .with_timeout(timeout))
}

/// Stop publishing one mapping without taking it down. Nothing in the CLI turns
/// Funnel off on its own, so the mapping is re-served as a tailnet mapping and
/// the CLI clears the Funnel bit for that port on the way through.
pub fn mapping_unpublish_command(
    path: &Path,
    timeout: Duration,
    mapping: &ServiceMapping,
    confirmed: bool,
) -> Result<LocalCommand, ServiceValueError> {
    if mapping.exposure != Exposure::Public {
        return Err(ServiceValueError(
            "only a public mapping can stop being published".to_owned(),
        ));
    }
    let tailnet = ServiceMapping {
        exposure: Exposure::Tailnet,
        ..mapping.clone()
    };
    mapping_command(path, timeout, &tailnet, confirmed)
}

fn listener_flag(listener: &Listener) -> String {
    match listener {
        Listener::Https(port) => format!("--https={port}"),
        Listener::Http(port) => format!("--http={port}"),
        Listener::Tcp(port) => format!("--tcp={port}"),
        Listener::TlsTerminatedTcp(port) => format!("--tls-terminated-tcp={port}"),
    }
}

pub fn parse_serve_status(input: &str) -> Result<ServeStatus, ServiceDecodeError> {
    let dto = decode_serve_dto(input)?;
    let mappings = parse_mappings(
        dto.mappings.as_ref(),
        dto.tcp.as_ref(),
        dto.web.as_ref(),
        dto.allow_funnel.as_ref(),
        !dto.additive.is_empty(),
        Exposure::Tailnet,
    )?;
    Ok(ServeStatus { mappings })
}

pub fn parse_funnel_status(input: &str) -> Result<FunnelStatus, ServiceDecodeError> {
    let dto = decode_funnel_dto(input)?;
    let mappings = parse_mappings(
        dto.mappings.as_ref(),
        dto.tcp.as_ref(),
        dto.web.as_ref(),
        dto.allow_funnel.as_ref(),
        !dto.additive.is_empty(),
        Exposure::Public,
    )?;
    Ok(FunnelStatus { mappings })
}

fn parse_mappings(
    explicit_mappings: Option<&Value>,
    tcp: Option<&Value>,
    web: Option<&Value>,
    allow_funnel: Option<&Value>,
    has_additive_fields: bool,
    exposure: Exposure,
) -> Result<Vec<ServiceMapping>, ServiceDecodeError> {
    let mut mappings = if let Some(values) = explicit_mappings {
        parse_fixture_mappings(values, exposure.clone())?
    } else {
        let mut parsed = Vec::new();
        let listener_by_port = tcp_listener_modes(tcp)?;
        if let Some(tcp) = tcp {
            parse_tcp_mappings(tcp, exposure.clone(), &mut parsed)?;
        }
        if let Some(web) = web {
            parse_web_mappings(web, exposure.clone(), &listener_by_port, &mut parsed)?;
        }
        let has_supported_root = tcp.is_some() || web.is_some() || allow_funnel.is_some();
        if parsed.is_empty() && has_additive_fields && !has_supported_root {
            return Err(ServiceDecodeError::new(
                "service status did not contain a supported Serve/Funnel mapping document",
            ));
        }
        parsed
    };
    if explicit_mappings.is_none()
        && let Some(public_hosts) = parse_allow_funnel(allow_funnel)
    {
        mappings.retain(|mapping| {
            let public = mapping_is_public(mapping, &public_hosts);
            match exposure {
                Exposure::Public => public,
                Exposure::Tailnet => !public,
            }
        });
    }
    mappings.sort_by_key(ServiceMapping::key);
    Ok(mappings)
}

fn decode_serve_dto(input: &str) -> Result<ServeStatusDtoV1989, ServiceDecodeError> {
    let value: Value = serde_json::from_str(input)
        .map_err(|error| ServiceDecodeError::new(format!("invalid JSON: {error}")))?;
    reject_out_of_scope_root(&value)?;
    serde_json::from_value(value)
        .map_err(|error| ServiceDecodeError::new(format!("Serve status DTO is invalid: {error}")))
}

fn decode_funnel_dto(input: &str) -> Result<FunnelStatusDtoV1989, ServiceDecodeError> {
    let value: Value = serde_json::from_str(input)
        .map_err(|error| ServiceDecodeError::new(format!("invalid JSON: {error}")))?;
    reject_out_of_scope_root(&value)?;
    serde_json::from_value(value)
        .map_err(|error| ServiceDecodeError::new(format!("Funnel status DTO is invalid: {error}")))
}

fn reject_out_of_scope_root(value: &Value) -> Result<(), ServiceDecodeError> {
    let object = value
        .as_object()
        .ok_or_else(|| ServiceDecodeError::new("service status must be a JSON object"))?;
    if object.contains_key("Services") || object.contains_key("services") {
        return Err(ServiceDecodeError::new(
            "tailnet Services output is outside the local service contract",
        ));
    }
    Ok(())
}

fn parse_fixture_mappings(
    value: &Value,
    exposure: Exposure,
) -> Result<Vec<ServiceMapping>, ServiceDecodeError> {
    let values = value
        .as_array()
        .ok_or_else(|| ServiceDecodeError::new("mappings must be an array"))?;
    values
        .iter()
        .map(|value| parse_fixture_mapping(value, exposure.clone()))
        .collect()
}

fn parse_fixture_mapping(
    value: &Value,
    default_exposure: Exposure,
) -> Result<ServiceMapping, ServiceDecodeError> {
    let object = value
        .as_object()
        .ok_or_else(|| ServiceDecodeError::new("mapping must be an object"))?;
    let exposure = object
        .get("exposure")
        .and_then(Value::as_str)
        .map(parse_exposure)
        .transpose()?
        .unwrap_or_else(|| default_exposure.clone());
    if exposure != default_exposure {
        return Err(ServiceDecodeError::new(
            "mapping exposure does not belong to this local service status",
        ));
    }
    let listener = parse_listener(
        object
            .get("listener")
            .and_then(Value::as_str)
            .ok_or_else(|| ServiceDecodeError::new("mapping listener is missing"))?,
        object
            .get("port")
            .and_then(value_u16)
            .ok_or_else(|| ServiceDecodeError::new("mapping port is missing"))?,
    )?;
    let mount = PathMount::parse(
        object
            .get("path")
            .or_else(|| object.get("mount"))
            .and_then(Value::as_str)
            .unwrap_or("/"),
    )
    .map_err(value_error)?;
    let backend = parse_backend_value(object)
        .ok_or_else(|| ServiceDecodeError::new("mapping backend is missing"))?;
    let proxy_protocol = object
        .get("proxyProtocol")
        .or_else(|| object.get("proxy_protocol"))
        .and_then(Value::as_str)
        .map(ProxyProtocol::parse)
        .transpose()
        .map_err(value_error)?
        .unwrap_or(ProxyProtocol::None);
    let hostname = object
        .get("hostname")
        .or_else(|| object.get("host"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mapping = ServiceMapping {
        exposure,
        listener,
        mount,
        backend,
        proxy_protocol,
        hostname,
    };
    mapping.validate().map_err(value_error)?;
    Ok(mapping)
}

fn parse_tcp_mappings(
    value: &Value,
    exposure: Exposure,
    output: &mut Vec<ServiceMapping>,
) -> Result<(), ServiceDecodeError> {
    let object = value
        .as_object()
        .ok_or_else(|| ServiceDecodeError::new("TCP status must be an object"))?;
    for (port_text, entry) in object {
        let port = port_text.parse::<Port>().map_err(value_error)?;
        let entry = entry
            .as_object()
            .ok_or_else(|| ServiceDecodeError::new("TCP mapping must be an object"))?;
        let listener = if entry
            .get("TerminateTLS")
            .or_else(|| entry.get("terminateTLS"))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty())
            || bool_field(entry, &["TLS-TERMINATED-TCP", "TLS_TERMINATED_TCP"])
        {
            Listener::TlsTerminatedTcp(port)
        } else if bool_field(entry, &["HTTP"]) {
            Listener::Http(port)
        } else if bool_field(entry, &["HTTPS"]) {
            Listener::Https(port)
        } else {
            Listener::Tcp(port)
        };
        let Some(backend) = parse_backend_value_map(entry) else {
            if matches!(listener, Listener::Http(_) | Listener::Https(_)) {
                // The CLI stores the HTTP(S) listener in TCP and its actual
                // backend in the corresponding Web handler.
                continue;
            }
            return Err(ServiceDecodeError::new("TCP mapping backend is missing"));
        };
        let proxy_protocol = parse_proxy_protocol(entry)?;
        let mapping = ServiceMapping {
            exposure: exposure.clone(),
            listener,
            mount: PathMount::Root,
            backend,
            proxy_protocol,
            hostname: None,
        };
        mapping.validate().map_err(value_error)?;
        output.push(mapping);
    }
    Ok(())
}

fn parse_web_mappings(
    value: &Value,
    exposure: Exposure,
    listener_by_port: &BTreeMap<u16, Listener>,
    output: &mut Vec<ServiceMapping>,
) -> Result<(), ServiceDecodeError> {
    let object = value
        .as_object()
        .ok_or_else(|| ServiceDecodeError::new("Web status must be an object"))?;
    for (host_port, entry) in object {
        let entry = entry
            .as_object()
            .ok_or_else(|| ServiceDecodeError::new("Web mapping must be an object"))?;
        let (hostname, port) = split_host_port(host_port)?;
        let listener = listener_by_port
            .get(&port.get())
            .cloned()
            .unwrap_or(Listener::Https(port));
        let handlers = entry
            .get("Handlers")
            .or_else(|| entry.get("handlers"))
            .ok_or_else(|| ServiceDecodeError::new("Web mapping handlers are missing"))?;
        let handlers = handlers
            .as_object()
            .ok_or_else(|| ServiceDecodeError::new("Web handlers must be an object"))?;
        for (mount_text, handler) in handlers {
            let handler = handler
                .as_object()
                .ok_or_else(|| ServiceDecodeError::new("Web handler must be an object"))?;
            let backend = parse_backend_value_map(handler)
                .ok_or_else(|| ServiceDecodeError::new("Web handler backend is missing"))?;
            let mapping = ServiceMapping {
                exposure: exposure.clone(),
                listener: listener.clone(),
                mount: PathMount::parse(mount_text).map_err(value_error)?,
                backend,
                proxy_protocol: ProxyProtocol::None,
                hostname: Some(hostname.clone()),
            };
            mapping.validate().map_err(value_error)?;
            output.push(mapping);
        }
    }
    Ok(())
}

fn split_host_port(value: &str) -> Result<(String, Port), ServiceDecodeError> {
    let (hostname, port_text) = value
        .rsplit_once(':')
        .ok_or_else(|| ServiceDecodeError::new("Web mapping key has no port"))?;
    if hostname.is_empty() {
        return Err(ServiceDecodeError::new("Web mapping hostname is empty"));
    }
    let port = port_text.parse::<Port>().map_err(value_error)?;
    Ok((hostname.to_owned(), port))
}

fn parse_listener(value: &str, port: u16) -> Result<Listener, ServiceDecodeError> {
    let port = Port::new(port).map_err(value_error)?;
    match value.to_ascii_lowercase().as_str() {
        "https" => Ok(Listener::Https(port)),
        "http" => Ok(Listener::Http(port)),
        "tcp" => Ok(Listener::Tcp(port)),
        "tls-terminated-tcp" | "tls_terminated_tcp" => Ok(Listener::TlsTerminatedTcp(port)),
        _ => Err(ServiceDecodeError::new("mapping listener is unsupported")),
    }
}

fn parse_exposure(value: &str) -> Result<Exposure, ServiceDecodeError> {
    match value.to_ascii_lowercase().as_str() {
        "tailnet" => Ok(Exposure::Tailnet),
        "public" => Ok(Exposure::Public),
        _ => Err(ServiceDecodeError::new("mapping exposure is unsupported")),
    }
}

fn parse_backend_value(object: &serde_json::Map<String, Value>) -> Option<Backend> {
    parse_backend_value_map(object)
}

fn parse_backend_value_map(object: &serde_json::Map<String, Value>) -> Option<Backend> {
    for name in [
        "Proxy",
        "Backend",
        "backend",
        "proxy",
        "TCPForward",
        "tcpForward",
        "File",
        "file",
        "Path",
        "path",
        "UnixSocket",
    ] {
        if let Some(value) = object.get(name).and_then(Value::as_str)
            && let Ok(backend) = Backend::parse(value)
        {
            return Some(backend);
        }
    }
    None
}

fn parse_proxy_protocol(
    object: &serde_json::Map<String, Value>,
) -> Result<ProxyProtocol, ServiceDecodeError> {
    let Some(value) = object
        .get("ProxyProtocol")
        .or_else(|| object.get("proxyProtocol"))
    else {
        return Ok(ProxyProtocol::None);
    };
    if let Some(value) = value.as_str() {
        return ProxyProtocol::parse(value).map_err(value_error);
    }
    match value.as_u64() {
        Some(1) => Ok(ProxyProtocol::Version1),
        Some(2) => Ok(ProxyProtocol::Version2),
        _ => Err(ServiceDecodeError::new(
            "proxy protocol must be none, 1, or 2",
        )),
    }
}

fn tcp_listener_modes(
    value: Option<&Value>,
) -> Result<BTreeMap<u16, Listener>, ServiceDecodeError> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let object = value
        .as_object()
        .ok_or_else(|| ServiceDecodeError::new("TCP status must be an object"))?;
    object
        .iter()
        .map(|(port_text, entry)| {
            let port = port_text.parse::<Port>().map_err(value_error)?;
            let entry = entry
                .as_object()
                .ok_or_else(|| ServiceDecodeError::new("TCP mapping must be an object"))?;
            let listener = if entry
                .get("TerminateTLS")
                .or_else(|| entry.get("terminateTLS"))
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
                || bool_field(entry, &["TLS-TERMINATED-TCP", "TLS_TERMINATED_TCP"])
            {
                Listener::TlsTerminatedTcp(port)
            } else if bool_field(entry, &["HTTP"]) {
                Listener::Http(port)
            } else if bool_field(entry, &["HTTPS"]) {
                Listener::Https(port)
            } else {
                Listener::Tcp(port)
            };
            Ok((port.get(), listener))
        })
        .collect()
}

fn parse_allow_funnel(value: Option<&Value>) -> Option<BTreeSet<String>> {
    let value = value?;
    if let Some(map) = value.as_object() {
        return Some(
            map.iter()
                .filter_map(|(host_port, enabled)| {
                    enabled
                        .as_bool()
                        .filter(|enabled| *enabled)
                        .map(|_| host_port.clone())
                })
                .collect(),
        );
    }
    value
        .as_bool()
        .filter(|enabled| !enabled)
        .map(|_| BTreeSet::new())
}

fn mapping_is_public(mapping: &ServiceMapping, public_hosts: &BTreeSet<String>) -> bool {
    let port = mapping.listener.port().to_string();
    if let Some(hostname) = mapping.hostname.as_deref() {
        return public_hosts.contains(&format!("{hostname}:{port}"));
    }
    public_hosts.iter().any(|host_port| {
        host_port
            .rsplit_once(':')
            .is_some_and(|(_, host_port)| host_port == port)
    })
}

fn bool_field(object: &serde_json::Map<String, Value>, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| object.get(*name).and_then(Value::as_bool).unwrap_or(false))
}

fn value_u16(value: &Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(|number| u16::try_from(number).ok())
        .or_else(|| value.as_str().and_then(|text| text.parse::<u16>().ok()))
}

fn value_error(error: ServiceValueError) -> ServiceDecodeError {
    ServiceDecodeError::new(error.0)
}

pub fn metrics_command(path: &Path, timeout: Duration, output_limit: usize) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::Metrics,
        vec![OsString::from("metrics"), OsString::from("print")],
    )
    .with_timeout(timeout)
    .with_limits(output_limit, 256 * 1024)
}

pub fn bugreport_command(
    path: &Path,
    timeout: Duration,
    note: Option<&str>,
    diagnose: bool,
) -> Result<LocalCommand, ServiceDecodeError> {
    if note.is_some_and(|value| {
        value
            .chars()
            .any(|character| character.is_control() && character != '\n' && character != '\t')
    }) {
        return Err(ServiceDecodeError::new(
            "bug-report note contains a control character",
        ));
    }
    let mut args = vec![OsString::from("bugreport")];
    if diagnose {
        args.push(OsString::from("--diagnose"));
    }
    if let Some(note) = note.filter(|value| !value.is_empty()) {
        args.push(OsString::from(note));
    }
    Ok(LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::BugReport,
        args,
    )
    .with_timeout(timeout))
}

pub fn parse_bugreport_identifier(output: &str) -> Result<String, ServiceDecodeError> {
    let mut found = None;
    for token in
        output.split(|character: char| !character.is_ascii_alphanumeric() && character != '-')
    {
        if token.starts_with("BUG-")
            && token.len() > 4
            && token[4..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            if found.is_some() {
                return Err(ServiceDecodeError::new(
                    "bug-report output contained multiple report identifiers",
                ));
            }
            found = Some(token.to_owned());
        }
    }
    found.ok_or_else(|| ServiceDecodeError::new("bug-report identifier was not returned"))
}

pub fn redacted_metrics(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    text.lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if lower.contains("password")
                || lower.contains("private_key")
                || lower.contains("private-key")
                || lower.contains("api_key")
                || lower.contains("api-key")
                || lower.contains("authorization")
                || lower.contains("bearer")
                || lower.contains("token")
                || lower.contains("secret")
            {
                "[redacted]".to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
