use std::collections::BTreeMap;

use super::Timestamp;
use super::device::DeviceId;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DiagnosticPath {
    Direct,
    Derp { region: Option<String> },
    PeerRelay { peer: Option<String> },
    Unknown,
}

impl DiagnosticPath {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Derp { .. } => "derp",
            Self::PeerRelay { .. } => "peer-relay",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PingSample {
    pub sequence: u64,
    pub observed_at: Timestamp,
    pub latency_ms: Option<u64>,
    pub path: DiagnosticPath,
    pub endpoint_or_region: Option<String>,
    pub raw_line: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PingSummary {
    pub expected: Option<u64>,
    pub received: u64,
    pub loss_percent: Option<u8>,
    pub minimum_ms: Option<u64>,
    pub average_ms: Option<u64>,
    pub maximum_ms: Option<u64>,
    pub last_path: Option<DiagnosticPath>,
    pub reached_direct: bool,
    pub parsed_samples: u64,
}

impl PingSummary {
    pub fn from_samples(expected: Option<u64>, samples: &[PingSample]) -> Self {
        let latencies: Vec<u64> = samples
            .iter()
            .filter_map(|sample| sample.latency_ms)
            .collect();
        let total = latencies
            .iter()
            .try_fold(0_u64, |sum, value| sum.checked_add(*value));
        let average_ms = match (total, latencies.len()) {
            (Some(total), count) if count > 0 => Some(total / count as u64),
            _ => None,
        };
        let received = samples.len() as u64;
        let loss_percent = expected.and_then(|count| {
            if count == 0 || received > count {
                None
            } else {
                let lost = count.saturating_sub(received);
                u8::try_from(lost.saturating_mul(100) / count).ok()
            }
        });
        let last_path = samples.last().map(|sample| sample.path.clone());
        Self {
            expected,
            received,
            loss_percent,
            minimum_ms: latencies.iter().copied().min(),
            average_ms,
            maximum_ms: latencies.iter().copied().max(),
            reached_direct: samples
                .iter()
                .any(|sample| sample.path == DiagnosticPath::Direct),
            last_path,
            parsed_samples: received,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DerpLatency {
    pub region_code: String,
    pub region_name: Option<String>,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct NetcheckObservation {
    pub udp: Option<bool>,
    pub ipv4: Option<bool>,
    pub ipv6: Option<bool>,
    pub mapping_varies_by_destination: Option<bool>,
    pub hairpinning: Option<bool>,
    pub port_mapping: Vec<String>,
    pub nearest_derp: Option<String>,
    pub derp_latency: Vec<DerpLatency>,
    pub sensitive_addresses: Vec<String>,
    pub observed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DnsStatus {
    pub forwarder_enabled: Option<bool>,
    pub magic_dns_enabled: Option<bool>,
    pub magic_dns_suffix: Option<String>,
    pub current_node_dns_name: Option<String>,
    pub resolvers: Vec<String>,
    pub split_routes: BTreeMap<String, Vec<String>>,
    pub cert_domains: Vec<String>,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DnsAnswer {
    pub value: String,
    pub record_type: Option<String>,
    pub ttl: Option<u64>,
    pub raw_detail: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DnsQueryResult {
    pub name: String,
    pub record_type: String,
    pub answers: Vec<DnsAnswer>,
    pub resolvers: Vec<String>,
    pub latency_ms: Option<u64>,
    pub result_class: String,
    pub observed_at: Timestamp,
    pub raw_detail: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WhoisResult {
    pub query: String,
    pub machine_id: Option<String>,
    pub machine_name: Option<String>,
    pub addresses: Vec<String>,
    pub tags: Vec<String>,
    pub user_identity: Option<String>,
    pub capabilities: Vec<String>,
    pub observed_at: Timestamp,
    pub raw_detail: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DiagnosticResult {
    Ping(PingSummary),
    Netcheck(NetcheckObservation),
    DnsStatus(DnsStatus),
    DnsQuery(DnsQueryResult),
    Whois(WhoisResult),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DiagnosticState {
    pub kind: String,
    pub samples: Vec<PingSample>,
    pub netcheck: Option<NetcheckObservation>,
    pub result: Option<DiagnosticResult>,
    pub linked_device_id: Option<DeviceId>,
}

impl DiagnosticState {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            samples: Vec::new(),
            netcheck: None,
            result: None,
            linked_device_id: None,
        }
    }
}
