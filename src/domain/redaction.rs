use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};

use super::Timestamp;
use super::diagnostic::{DnsQueryResult, NetcheckObservation, PingSummary};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DiagnosticReportInput {
    pub tale_version: String,
    pub tailscale_version: String,
    pub platform: String,
    pub local_state: String,
    pub health_categories: Vec<String>,
    pub peer_identity: Option<String>,
    pub peer_os: Option<String>,
    pub peer_path: Option<String>,
    pub ping: Option<PingSummary>,
    pub netcheck: Option<NetcheckObservation>,
    pub dns: Option<DnsQueryResult>,
    pub observed_at: Timestamp,
    pub stale: bool,
    pub names: Vec<String>,
    pub addresses: Vec<String>,
    pub paths: Vec<String>,
    pub public_endpoints: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RedactedReport {
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct Redactor {
    identities: BTreeMap<String, String>,
    addresses: BTreeMap<String, String>,
    paths: BTreeMap<String, String>,
    endpoints: BTreeMap<String, String>,
    next_identity: usize,
    next_address: usize,
    next_path: usize,
    next_endpoint: usize,
}

impl Redactor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn identity(&mut self, value: &str) -> String {
        if value.is_empty() {
            return "not returned".to_owned();
        }
        if let Some(label) = self.identities.get(value) {
            return label.clone();
        }
        self.next_identity = self.next_identity.saturating_add(1);
        let label = format!("id-{}", self.next_identity);
        self.identities.insert(value.to_owned(), label.clone());
        label
    }

    pub fn address(&mut self, value: &str) -> String {
        if let Some(label) = self.addresses.get(value) {
            return label.clone();
        }
        self.next_address = self.next_address.saturating_add(1);
        let label = format!("address-{}", self.next_address);
        self.addresses.insert(value.to_owned(), label.clone());
        label
    }

    pub fn path(&mut self, value: &str) -> String {
        if let Some(label) = self.paths.get(value) {
            return label.clone();
        }
        self.next_path = self.next_path.saturating_add(1);
        let label = format!("path-{}", self.next_path);
        self.paths.insert(value.to_owned(), label.clone());
        label
    }

    pub fn endpoint(&mut self, value: &str) -> String {
        if let Some(label) = self.endpoints.get(value) {
            return label.clone();
        }
        self.next_endpoint = self.next_endpoint.saturating_add(1);
        let label = format!("endpoint-{}", self.next_endpoint);
        self.endpoints.insert(value.to_owned(), label.clone());
        label
    }

    pub fn text(&mut self, value: &str) -> String {
        let mut value = self.replace_known(value);
        value = value
            .split_whitespace()
            .map(|token| {
                let trimmed = token.trim_matches(|character: char| {
                    matches!(character, ',' | '.' | ';' | ':' | '(' | ')' | '[' | ']')
                });
                if trimmed.contains('@') {
                    self.identity(trimmed)
                } else if is_ip_like(trimmed) {
                    self.address(trimmed)
                } else if trimmed.starts_with('/')
                    || trimmed.starts_with("~/")
                    || trimmed.contains('\\')
                {
                    self.path(trimmed)
                } else {
                    token.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        value
    }

    fn replace_known(&self, value: &str) -> String {
        let mut result = value.to_owned();
        for (original, label) in &self.identities {
            result = result.replace(original, label);
        }
        for (original, label) in &self.addresses {
            result = result.replace(original, label);
        }
        for (original, label) in &self.paths {
            result = result.replace(original, label);
        }
        for (original, label) in &self.endpoints {
            result = result.replace(original, label);
        }
        result
    }
}

pub fn redact_diagnostic_report(input: &DiagnosticReportInput) -> RedactedReport {
    let mut redactor = Redactor::new();
    let peer = input.peer_identity.as_deref().map_or_else(
        || "not returned".to_owned(),
        |value| redactor.identity(value),
    );
    let name_count = input.names.len();
    let address_count = input.addresses.len();
    let path_count = input.paths.len();
    let endpoint_count = input.public_endpoints.len();
    for name in &input.names {
        let _ = redactor.identity(name);
    }
    for address in &input.addresses {
        let _ = redactor.address(address);
    }
    for path in &input.paths {
        let _ = redactor.path(path);
    }
    for endpoint in &input.public_endpoints {
        let _ = redactor.endpoint(endpoint);
    }
    let health = health_categories(&input.health_categories);
    let mut lines = vec![
        format!("Tale version: {}", input.tale_version),
        format!("Tailscale version: {}", input.tailscale_version),
        format!("Platform: {}", input.platform),
        format!("Local state: {}", input.local_state),
        format!("Health: {health}"),
        format!("Peer: {peer}"),
        format!(
            "Peer OS/path: {}/{}",
            input
                .peer_os
                .as_deref()
                .map_or("not returned", |value| value),
            input
                .peer_path
                .as_deref()
                .map_or("not returned", |value| value)
        ),
        format!("Observed at: {}", input.observed_at),
        format!("Stale: {}", input.stale),
        format!(
            "Redacted values: names={name_count} addresses={address_count} paths={path_count} endpoints={endpoint_count}"
        ),
    ];
    if let Some(ping) = &input.ping {
        lines.push(format!(
            "Ping: received={} loss={} min={} avg={} max={} path={} direct={}",
            ping.received,
            format_optional_u8(ping.loss_percent),
            format_optional_u64(ping.minimum_ms),
            format_optional_u64(ping.average_ms),
            format_optional_u64(ping.maximum_ms),
            ping.last_path
                .as_ref()
                .map_or("not returned", |path| path.label()),
            ping.reached_direct
        ));
    }
    if let Some(netcheck) = &input.netcheck {
        lines.push(format!(
            "Netcheck: udp={} ipv4={} ipv6={} mapping_varies={} hairpinning={} nearest_derp={} derp_regions={}",
            format_optional_bool(netcheck.udp),
            format_optional_bool(netcheck.ipv4),
            format_optional_bool(netcheck.ipv6),
            format_optional_bool(netcheck.mapping_varies_by_destination),
            format_optional_bool(netcheck.hairpinning),
            netcheck
                .nearest_derp
                .as_deref()
                .map_or("not returned", |value| value),
            netcheck.derp_latency.len()
        ));
    }
    if let Some(dns) = &input.dns {
        lines.push(format!(
            "DNS: type={} result={} answers={} latency={}",
            dns.record_type,
            diagnostic_result_class(&dns.result_class),
            dns.answers.len(),
            format_optional_u64(dns.latency_ms)
        ));
    }
    RedactedReport {
        text: lines.join("\n"),
    }
}

fn health_categories(values: &[String]) -> String {
    let mut categories = BTreeSet::new();
    for value in values {
        let lower = value.to_ascii_lowercase();
        let category = if lower.contains("dns") {
            "dns"
        } else if lower.contains("auth") || lower.contains("login") {
            "authentication"
        } else if lower.contains("route") || lower.contains("subnet") {
            "routing"
        } else if lower.contains("derp") || lower.contains("relay") || lower.contains("direct") {
            "connectivity"
        } else {
            "reported"
        };
        categories.insert(category);
    }
    if categories.is_empty() {
        "not returned".to_owned()
    } else {
        categories.into_iter().collect::<Vec<_>>().join(", ")
    }
}

fn diagnostic_result_class(value: &str) -> String {
    let normalized = value.to_ascii_lowercase();
    if normalized.contains("nxdomain") || normalized.contains("empty") {
        "empty".to_owned()
    } else if normalized.contains("error") || normalized.contains("fail") {
        "error".to_owned()
    } else if normalized.contains("success") || normalized == "noerror" {
        "success".to_owned()
    } else {
        "other".to_owned()
    }
}

fn is_ip_like(value: &str) -> bool {
    let without_cidr = value.split('/').next().map_or(value, |part| part);
    without_cidr.parse::<IpAddr>().is_ok()
        || without_cidr.parse::<SocketAddr>().is_ok()
        || without_cidr
            .strip_prefix('[')
            .and_then(|part| part.strip_suffix(']'))
            .is_some_and(|part| part.parse::<IpAddr>().is_ok())
}

fn format_optional_bool(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "not returned",
    }
}

fn format_optional_u8(value: Option<u8>) -> String {
    value.map_or_else(|| "not returned".to_owned(), |value| format!("{value}%"))
}

fn format_optional_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "not returned".to_owned(), |value| format!("{value}ms"))
}
