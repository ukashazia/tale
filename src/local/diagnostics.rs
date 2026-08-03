use std::ffi::OsString;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::domain::Timestamp;
use crate::domain::diagnostic::{
    DerpLatency, DiagnosticPath, DnsAnswer, DnsQueryResult, DnsStatus, NetcheckObservation,
    PingSample, PingSummary, WhoisResult,
};

use super::process::{LocalCommand, LocalOperation, OutputMode};

pub const PING_OUTPUT_LIMIT: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DnsRecordType {
    A,
    Aaaa,
    Cname,
    Mx,
    Ns,
    Ptr,
    Srv,
    Txt,
}

impl DnsRecordType {
    pub const fn label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::Aaaa => "AAAA",
            Self::Cname => "CNAME",
            Self::Mx => "MX",
            Self::Ns => "NS",
            Self::Ptr => "PTR",
            Self::Srv => "SRV",
            Self::Txt => "TXT",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "A" => Some(Self::A),
            "AAAA" => Some(Self::Aaaa),
            "CNAME" => Some(Self::Cname),
            "MX" => Some(Self::Mx),
            "NS" => Some(Self::Ns),
            "PTR" => Some(Self::Ptr),
            "SRV" => Some(Self::Srv),
            "TXT" => Some(Self::Txt),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum WhoisProtocol {
    Tcp,
    Udp,
}

impl WhoisProtocol {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DiagnosticRequest {
    Ping {
        target: String,
    },
    Netcheck {
        live: bool,
    },
    DnsStatus,
    DnsQuery {
        name: String,
        record_type: DnsRecordType,
    },
    Whois {
        target: String,
        protocol: Option<WhoisProtocol>,
    },
}

impl DiagnosticRequest {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Ping { .. } => "ping",
            Self::Netcheck { live: false } => "netcheck",
            Self::Netcheck { live: true } => "netcheck live",
            Self::DnsStatus => "dns status",
            Self::DnsQuery { .. } => "dns query",
            Self::Whois { .. } => "whois",
        }
    }
}

pub fn ping_command(path: &Path, timeout: Duration, target: &str) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::Ping,
        vec![
            OsString::from("ping"),
            OsString::from("--c=10"),
            OsString::from("--timeout=5s"),
            OsString::from("--until-direct=true"),
            OsString::from(target),
        ],
    )
    .with_timeout(timeout)
    .with_modes(OutputMode::Lines, OutputMode::Lines)
    .with_limits(PING_OUTPUT_LIMIT, PING_OUTPUT_LIMIT)
}

pub fn netcheck_command(path: &Path, timeout: Option<Duration>, live: bool) -> LocalCommand {
    let format = if live { "json-line" } else { "json" };
    let mut args = vec![
        OsString::from("netcheck"),
        OsString::from(format!("--format={format}")),
    ];
    if live {
        args.push(OsString::from("--every=2s"));
    }
    let command = LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::Netcheck,
        args,
    )
    .with_modes(OutputMode::Lines, OutputMode::Lines)
    .with_limits(4 * 1024 * 1024, 256 * 1024);
    match timeout {
        Some(timeout) => command.with_timeout(timeout),
        None => command.without_timeout(),
    }
}

pub fn dns_status_command(path: &Path, timeout: Duration) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::DnsStatus,
        vec![
            OsString::from("dns"),
            OsString::from("status"),
            OsString::from("--json"),
        ],
    )
    .with_timeout(timeout)
    .with_limits(4 * 1024 * 1024, 256 * 1024)
}

pub fn dns_query_command(
    path: &Path,
    timeout: Duration,
    name: &str,
    record_type: DnsRecordType,
) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::DnsQuery,
        vec![
            OsString::from("dns"),
            OsString::from("query"),
            OsString::from("--json"),
            OsString::from(name),
            OsString::from(record_type.label()),
        ],
    )
    .with_timeout(timeout)
    .with_limits(4 * 1024 * 1024, 256 * 1024)
}

pub fn whois_command(
    path: &Path,
    timeout: Duration,
    target: &str,
    protocol: Option<WhoisProtocol>,
) -> LocalCommand {
    let mut args = vec![OsString::from("whois"), OsString::from("--json")];
    if let Some(protocol) = protocol {
        args.push(OsString::from(format!("--proto={}", protocol.label())));
    }
    args.push(OsString::from(target));
    LocalCommand::new(path.as_os_str().to_os_string(), LocalOperation::Whois, args)
        .with_timeout(timeout)
        .with_limits(4 * 1024 * 1024, 256 * 1024)
}

pub fn validate_dns_query(name: &str, record_type: &str) -> Result<DnsRecordType, String> {
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return Err("DNS name must be non-empty and contain no whitespace".to_owned());
    }
    DnsRecordType::parse(record_type).ok_or_else(|| {
        "DNS record type must be one of A, AAAA, CNAME, MX, NS, PTR, SRV, TXT".to_owned()
    })
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ValidatedWhois {
    pub target: String,
    pub address: IpAddr,
    pub port: Option<u16>,
}

pub fn validate_whois_target(value: &str) -> Result<ValidatedWhois, String> {
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return Err("whois target must be an IP address or IP address with port".to_owned());
    }
    if let Ok(socket) = value.parse::<SocketAddr>() {
        return Ok(ValidatedWhois {
            target: value.to_owned(),
            address: socket.ip(),
            port: Some(socket.port()),
        });
    }
    if let Ok(address) = value.parse::<IpAddr>() {
        return Ok(ValidatedWhois {
            target: value.to_owned(),
            address,
            port: None,
        });
    }
    Err("whois target is not a valid IP address or socket address".to_owned())
}

pub fn parse_ping_line(line: &str, sequence: u64, observed_at: Timestamp) -> Option<PingSample> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if !lower.contains("pong") && !lower.contains("reply") {
        return None;
    }
    let latency_ms = trimmed.split_whitespace().find_map(parse_latency_token);
    let endpoint_or_region = value_after(trimmed, "via");
    let path = if lower.contains("peer-relay") || lower.contains("peer relay") {
        DiagnosticPath::PeerRelay {
            peer: value_after(trimmed, "via"),
        }
    } else if lower.contains("derp") {
        DiagnosticPath::Derp {
            region: value_after(trimmed, "derp"),
        }
    } else if lower.contains("direct") || endpoint_or_region.is_some() {
        DiagnosticPath::Direct
    } else {
        DiagnosticPath::Unknown
    };
    Some(PingSample {
        sequence,
        observed_at,
        latency_ms,
        path,
        endpoint_or_region,
        raw_line: trimmed.to_owned(),
    })
}

pub fn summarize_ping(expected: Option<u64>, samples: &[PingSample]) -> PingSummary {
    PingSummary::from_samples(expected, samples)
}

pub fn parse_netcheck_json(
    input: &str,
    observed_at: Timestamp,
) -> Result<NetcheckObservation, String> {
    let value: Value =
        serde_json::from_str(input).map_err(|error| format!("invalid netcheck JSON: {error}"))?;
    parse_netcheck_value(&value, observed_at)
}

pub fn parse_netcheck_lines(
    lines: &[String],
    observed_at: Timestamp,
) -> (Option<NetcheckObservation>, Vec<String>) {
    let mut latest = None;
    let mut errors = Vec::new();
    for line in lines {
        match serde_json::from_str::<Value>(line) {
            Ok(value) => match parse_netcheck_value(&value, observed_at) {
                Ok(observation) => latest = Some(observation),
                Err(error) => errors.push(error),
            },
            Err(error) => errors.push(format!("invalid netcheck line: {error}")),
        }
    }
    (latest, errors)
}

pub fn parse_dns_status(input: &str, observed_at: Timestamp) -> Result<DnsStatus, String> {
    let value: Value =
        serde_json::from_str(input).map_err(|error| format!("invalid DNS status JSON: {error}"))?;
    let split_routes = parse_split_routes(get(&value, &["SplitDNS", "SplitRoutes", "splitDns"]));
    Ok(DnsStatus {
        forwarder_enabled: first_bool(&value, &["Forwarder", "ForwarderEnabled", "LocalForwarder"]),
        magic_dns_enabled: first_bool(&value, &["MagicDNS", "MagicDNSEnabled", "magicDnsEnabled"]),
        magic_dns_suffix: first_string(&value, &["MagicDNSSuffix", "Suffix", "magicDnsSuffix"]),
        current_node_dns_name: first_string(
            &value,
            &["CurrentNodeDNSName", "DNSName", "currentNodeDnsName"],
        ),
        resolvers: parse_strings(get(&value, &["Resolvers", "Nameservers", "resolvers"])),
        split_routes,
        cert_domains: parse_strings(get(&value, &["CertDomains", "CertificateDomains"])),
        observed_at,
    })
}

pub fn parse_dns_query(
    input: &str,
    name: String,
    record_type: DnsRecordType,
    observed_at: Timestamp,
) -> Result<DnsQueryResult, String> {
    let value: Value =
        serde_json::from_str(input).map_err(|error| format!("invalid DNS query JSON: {error}"))?;
    let answers = parse_answers(get(&value, &["Answers", "Answer", "answers"]));
    let result_class = match first_string(&value, &["Result", "Status", "RCode", "rcode"]) {
        Some(value) => value,
        None if answers.is_empty() => "empty".to_owned(),
        None => "success".to_owned(),
    };
    Ok(DnsQueryResult {
        name,
        record_type: record_type.label().to_owned(),
        answers,
        resolvers: parse_strings(get(&value, &["Resolvers", "Resolver"])),
        latency_ms: first_latency(&value, &["Latency", "LatencyMs", "latencyMs"]),
        result_class,
        observed_at,
        raw_detail: bounded_json(&value),
    })
}

pub fn parse_whois(
    input: &str,
    query: String,
    observed_at: Timestamp,
) -> Result<WhoisResult, String> {
    let value: Value =
        serde_json::from_str(input).map_err(|error| format!("invalid whois JSON: {error}"))?;
    let machine = get(&value, &["Machine", "Node", "machine"]);
    let machine_id = machine
        .and_then(|value| first_string(value, &["ID", "Id", "NodeID"]))
        .or_else(|| first_string(&value, &["ID", "NodeID"]));
    let machine_name = machine
        .and_then(|value| first_string(value, &["Name", "HostName", "DNSName"]))
        .or_else(|| first_string(&value, &["Name", "HostName", "DNSName"]));
    Ok(WhoisResult {
        query,
        machine_id,
        machine_name,
        addresses: machine
            .and_then(|value| get(value, &["Addresses", "TailscaleIPs", "IPs"]))
            .map_or_else(Vec::new, |value| parse_strings(Some(value))),
        tags: machine
            .and_then(|value| get(value, &["Tags"]))
            .map_or_else(Vec::new, |value| parse_strings(Some(value))),
        user_identity: first_string(&value, &["User", "UserName", "LoginName", "Email"]),
        capabilities: parse_strings(get(&value, &["Capabilities", "capabilities"])),
        observed_at,
        raw_detail: bounded_json(&value),
    })
}

fn parse_netcheck_value(
    value: &Value,
    observed_at: Timestamp,
) -> Result<NetcheckObservation, String> {
    if !value.is_object() {
        return Err("netcheck result was not an object".to_owned());
    }
    let mut derp_latency =
        parse_derp_latency(get(value, &["DERPLatency", "RegionLatency", "DerpLatency"]));
    derp_latency.sort_by(|left, right| {
        left.latency_ms
            .cmp(&right.latency_ms)
            .then_with(|| left.region_code.cmp(&right.region_code))
    });
    let sensitive_addresses = parse_address_fields(value);
    Ok(NetcheckObservation {
        udp: first_bool(value, &["UDP", "Udp", "udp"]),
        ipv4: first_bool(value, &["IPv4", "IPv4Available", "ipv4Available"]),
        ipv6: first_bool(value, &["IPv6", "IPv6Available", "ipv6Available"]),
        mapping_varies_by_destination: first_bool(
            value,
            &[
                "MappingVariesByDestIP",
                "MappingVariesByDestination",
                "mappingVariesByDestination",
            ],
        ),
        hairpinning: first_bool(value, &["HairPinning", "Hairpinning", "hairpinning"]),
        port_mapping: parse_strings(get(value, &["PortMapping", "PortMappings", "portMapping"])),
        nearest_derp: first_string(value, &["NearestDERP", "PreferredDERP", "nearestDerp"]),
        derp_latency,
        sensitive_addresses,
        observed_at: Some(observed_at),
    })
}

fn parse_derp_latency(value: Option<&Value>) -> Vec<DerpLatency> {
    let Some(Value::Object(map)) = value else {
        return Vec::new();
    };
    map.iter()
        .map(|(region, value)| DerpLatency {
            region_code: region.clone(),
            region_name: value
                .as_object()
                .and_then(|_| first_string(value, &["Name", "RegionName", "name"])),
            latency_ms: value
                .as_object()
                .and_then(|_| first_latency(value, &["Latency", "LatencyMs", "latencyMs"]))
                .or_else(|| value.as_f64().and_then(seconds_or_millis)),
        })
        .collect()
}

fn parse_split_routes(value: Option<&Value>) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut routes = std::collections::BTreeMap::new();
    let Some(Value::Object(map)) = value else {
        return routes;
    };
    for (suffix, value) in map {
        routes.insert(suffix.clone(), parse_strings(Some(value)));
    }
    routes
}

fn parse_answers(value: Option<&Value>) -> Vec<DnsAnswer> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                if let Some(text) = value.as_str() {
                    DnsAnswer {
                        value: text.to_owned(),
                        record_type: None,
                        ttl: None,
                        raw_detail: None,
                    }
                } else {
                    DnsAnswer {
                        value: match first_string(value, &["Value", "Data", "Target", "value"]) {
                            Some(value) => value,
                            None => bounded_json(value),
                        },
                        record_type: first_string(value, &["Type", "type"]),
                        ttl: first_u64(value, &["TTL", "Ttl", "ttl"]),
                        raw_detail: Some(bounded_json(value)),
                    }
                }
            })
            .collect(),
        Some(value) => vec![DnsAnswer {
            value: match value_string(value) {
                Some(value) => value,
                None => bounded_json(value),
            },
            record_type: None,
            ttl: None,
            raw_detail: Some(bounded_json(value)),
        }],
        None => Vec::new(),
    }
}

fn first_latency(value: &Value, names: &[&str]) -> Option<u64> {
    names.iter().find_map(|name| {
        get(value, &[*name]).and_then(|value| {
            value_u64(value).or_else(|| value.as_f64().and_then(seconds_or_millis))
        })
    })
}

fn seconds_or_millis(value: f64) -> Option<u64> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let millis = if value < 1.0 { value * 1_000.0 } else { value };
    if millis > u64::MAX as f64 {
        None
    } else {
        Some(millis.round() as u64)
    }
}

fn parse_latency_token(token: &str) -> Option<u64> {
    let token =
        token.trim_matches(|character: char| matches!(character, ',' | ')' | '(' | '[' | ']'));
    let number = token.strip_suffix("ms")?;
    let whole = number.split('.').next()?.parse::<u64>().ok()?;
    Some(whole)
}

fn value_after(value: &str, marker: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let marker_position = lower.find(marker)?;
    let rest = value.get(marker_position + marker.len()..)?.trim();
    let rest = rest.split(" in ").next().map_or(rest, |value| value);
    let rest = rest.trim_matches(|character: char| matches!(character, ':' | '(' | ')' | ','));
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_owned())
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

fn parse_strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values.iter().filter_map(value_string).collect(),
        Some(Value::String(value)) if !value.is_empty() => vec![value.clone()],
        _ => Vec::new(),
    }
}

fn parse_address_fields(value: &Value) -> Vec<String> {
    let mut addresses = Vec::new();
    for name in [
        "IPv4",
        "IPv6",
        "IPv4Address",
        "IPv6Address",
        "PublicIP",
        "PublicIPv4",
        "PublicIPv6",
    ] {
        for address in parse_strings(get(value, &[name])) {
            if !addresses.iter().any(|existing| existing == &address) {
                addresses.push(address);
            }
        }
    }
    addresses
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

fn bounded_json(value: &Value) -> String {
    let text = value.to_string();
    if text.len() <= 8 * 1024 {
        text
    } else {
        let mut end = 0;
        for (index, character) in text.char_indices() {
            if index.saturating_add(character.len_utf8()) > 8 * 1024 {
                break;
            }
            end = index.saturating_add(character.len_utf8());
        }
        format!("{}...[detail truncated]", &text[..end])
    }
}
