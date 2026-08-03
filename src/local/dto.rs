use std::collections::BTreeMap;

use serde_json::Value;

use crate::domain::Timestamp;
use crate::domain::device::{ConnectionPath, DeviceId, LocalDevice, OperatingSystem};
use crate::domain::source::{LocalSnapshot, LocalState};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VersionDto {
    pub cli_version: String,
    pub daemon_version: Option<String>,
    pub build: Option<String>,
    pub raw: Value,
}

pub fn decode_version(input: &str) -> Result<super::client::VersionInfo, String> {
    let value: Value =
        serde_json::from_str(input).map_err(|error| format!("invalid JSON: {error}"))?;
    let cli_version = first_string(&value, &["version", "short", "full", "Version"])
        .ok_or_else(|| "required CLI version was not returned".to_owned())?;
    let daemon_version = first_string(&value, &["daemonVersion", "DaemonVersion"]);
    let build = first_string(&value, &["gitCommit", "GitCommit", "commit", "full"]);
    Ok(super::client::VersionInfo {
        version: cli_version,
        daemon_version,
        build,
    })
}

pub fn decode_status(
    input: &str,
    client_version: String,
    daemon_version: Option<String>,
    observed_at: Timestamp,
) -> Result<LocalSnapshot, String> {
    let value: Value =
        serde_json::from_str(input).map_err(|error| format!("invalid JSON: {error}"))?;
    let users = parse_users(get(&value, &["User", "Users"]));
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
    let backend_text = first_string(&value, &["BackendState", "backendState"]);
    let health_messages = parse_strings(get(&value, &["Health", "health"]));
    let auth_url = first_string(&value, &["AuthURL", "AuthUrl", "LoginURL", "loginUrl"]);
    let backend_state = backend_state(backend_text.as_deref(), &health_messages, auth_url);
    let current_tailnet = parse_name(get(&value, &["CurrentTailnet", "Tailnet"]));
    let magic_dns_suffix = first_string(&value, &["MagicDNSSuffix", "magicDnsSuffix"]);
    let cert_domains = parse_strings(get(&value, &["CertDomains", "CertificateDomains"]));
    Ok(LocalSnapshot {
        observed_at,
        client_version,
        daemon_version,
        backend_state,
        health_messages,
        current_tailnet,
        magic_dns_suffix,
        cert_domains,
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
    let public_key = first_string(value, &["PublicKey", "publicKey"]);
    let dns_name = first_string(value, &["DNSName", "DnsName", "dnsName"]);
    let display_name = match first_string(value, &["HostName", "Hostname", "Name", "DNSName"]) {
        Some(value) => value,
        None => id.clone(),
    };
    let hostname = match first_string(value, &["HostName", "Hostname", "Name"]) {
        Some(value) => value,
        None => display_name.clone(),
    };
    let os = parse_os(first_string(value, &["OS", "Os", "os"]));
    let version = first_string(value, &["ClientVersion", "Version", "version"]);
    let user_id = first_string(value, &["UserID", "UserId", "userId"]);
    let owner_label = user_id.as_deref().and_then(|user| users.get(user)).cloned();
    let tags = parse_strings(get(value, &["Tags", "tags"]));
    let tailscale_ips = parse_strings(get(
        value,
        &["TailscaleIPs", "TailscaleIps", "Addresses", "IPs"],
    ));
    let advertised_routes = parse_strings(get(value, &["AdvertisedRoutes", "Routes"]));
    let current_endpoint = first_string(value, &["CurAddr", "CurrentEndpoint", "Endpoint"]);
    let relay_region = first_string(value, &["Relay", "RelayRegion", "DERP"]);
    let online = first_bool(value, &["Online", "online"]);
    let active = first_bool(value, &["Active", "active"]).is_some_and(|value| value);
    let rx_bytes = first_u64(value, &["RxBytes", "RXBytes", "rxBytes"]);
    let tx_bytes = first_u64(value, &["TxBytes", "TXBytes", "txBytes"]);
    let created_at = first_timestamp(value, &["Created", "CreatedAt", "createdAt"]);
    let last_seen = first_timestamp(value, &["LastSeen", "lastSeen"]);
    let last_handshake = first_timestamp(value, &["LastHandshake", "lastHandshake"]);
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
    let exit_node = first_bool(value, &["ExitNode", "IsExitNode"]).is_some_and(|value| value);
    let exit_node_option =
        first_bool(value, &["ExitNodeOption", "IsExitNodeOption"]).is_some_and(|value| value);
    let ssh_host_keys_present = !parse_strings(get(value, &["SSHHostKeys", "SshHostKeys"]))
        .is_empty()
        || capabilities.get("ssh").copied().is_some_and(|value| value);
    let shared = first_bool(value, &["ShareeNode", "Shared", "shared"]).is_some_and(|value| value)
        || capabilities
            .get("shared")
            .copied()
            .is_some_and(|value| value);
    let path = parse_path(value, current_endpoint.as_deref(), relay_region.as_deref());
    Ok(LocalDevice {
        id: DeviceId::new(id),
        public_key,
        display_name,
        hostname,
        dns_name,
        os,
        version,
        owner_label,
        user_id,
        tags,
        tailscale_ips,
        advertised_routes,
        current_endpoint,
        relay_region,
        path,
        online,
        active,
        rx_bytes,
        tx_bytes,
        created_at,
        last_seen,
        last_handshake,
        exit_node,
        exit_node_option,
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

fn parse_os(value: Option<String>) -> crate::domain::device::OperatingSystem {
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

fn parse_path(
    value: &Value,
    endpoint: Option<&str>,
    relay: Option<&str>,
) -> crate::domain::device::ConnectionPath {
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
    let state = value.map(str::to_ascii_lowercase);
    match state.as_deref() {
        Some("needslogin")
        | Some("needs_login")
        | Some("needs machine auth")
        | Some("needsmachineauth") => LocalState::NeedsLogin { auth_url },
        Some("stopped") | Some("nostate") | Some("no_state") => LocalState::Stopped,
        Some("running") | Some("starting") => {
            if health.is_empty() {
                LocalState::Running
            } else {
                LocalState::Degraded {
                    health_messages: health.to_vec(),
                }
            }
        }
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
        Value::Number(_) | Value::String(_) => {
            value_u64(value).or_else(|| value.as_str().and_then(parse_timestamp))
        }
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
    let second = time_parts
        .next()
        .and_then(|value| value.split('.').next())
        .and_then(|value| value.parse::<u64>().ok())?;
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
