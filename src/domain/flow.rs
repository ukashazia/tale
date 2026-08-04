use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};

use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use super::Timestamp;

pub const FLOW_RETENTION: Duration = Duration::days(30);
pub const MAX_FLOW_WINDOW: Duration = Duration::hours(24);
pub const MAX_FLOW_BODY_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_FLOW_MESSAGES: usize = 250_000;

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum FlowError {
    #[error("flow window must use RFC3339 UTC timestamps")]
    InvalidTimestamp,
    #[error("flow window start must be before or equal to end")]
    ReversedWindow,
    #[error("flow window may not exceed 24 hours")]
    WindowTooWide,
    #[error("flow window may not end in the future")]
    FutureWindow,
    #[error("flow window is outside the 30-day retention period")]
    OutsideRetention,
    #[error("flow response exceeded the 64 MiB byte limit")]
    BodyTooLarge,
    #[error("flow response exceeded the 250,000-message decoded limit")]
    MessageLimit,
    #[error("flow counter overflowed while aggregating")]
    CounterOverflow,
    #[error("flow aggregation was cancelled")]
    Cancelled,
    #[error("flow filter is invalid: {0}")]
    InvalidFilter(String),
    #[error("flow aggregation has no dimensions")]
    EmptyAggregation,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FlowWindow {
    pub start: OffsetDateTime,
    pub end: OffsetDateTime,
}

impl FlowWindow {
    pub fn new(
        start: OffsetDateTime,
        end: OffsetDateTime,
        now: OffsetDateTime,
    ) -> Result<Self, FlowError> {
        if start.offset() != time::UtcOffset::UTC || end.offset() != time::UtcOffset::UTC {
            return Err(FlowError::InvalidTimestamp);
        }
        if start > end {
            return Err(FlowError::ReversedWindow);
        }
        if end > now {
            return Err(FlowError::FutureWindow);
        }
        if end - start > MAX_FLOW_WINDOW {
            return Err(FlowError::WindowTooWide);
        }
        if start < now - FLOW_RETENTION {
            return Err(FlowError::OutsideRetention);
        }
        Ok(Self { start, end })
    }

    pub fn previous_hour(now: OffsetDateTime) -> Self {
        let now = now.to_offset(time::UtcOffset::UTC);
        Self {
            start: now - Duration::hours(1),
            end: now,
        }
    }

    pub fn from_rfc3339(start: &str, end: &str, now: OffsetDateTime) -> Result<Self, FlowError> {
        let start =
            OffsetDateTime::parse(start, &Rfc3339).map_err(|_| FlowError::InvalidTimestamp)?;
        let end = OffsetDateTime::parse(end, &Rfc3339).map_err(|_| FlowError::InvalidTimestamp)?;
        if start.offset() != time::UtcOffset::UTC || end.offset() != time::UtcOffset::UTC {
            return Err(FlowError::InvalidTimestamp);
        }
        Self::new(start, end, now)
    }

    pub fn query_values(&self) -> Result<(String, String), FlowError> {
        let start = self
            .start
            .format(&Rfc3339)
            .map_err(|_| FlowError::InvalidTimestamp)?;
        let end = self
            .end
            .format(&Rfc3339)
            .map_err(|_| FlowError::InvalidTimestamp)?;
        Ok((start, end))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FlowNode {
    pub node_id: String,
    pub name: Option<String>,
    pub addresses: Vec<String>,
    pub os: Option<String>,
    pub user: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum TrafficClass {
    Virtual,
    Subnet,
    Exit,
    Physical,
}

impl TrafficClass {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Virtual => "virtual",
            Self::Subnet => "subnet",
            Self::Exit => "exit",
            Self::Physical => "physical",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FlowConnection {
    pub proto: String,
    pub src: String,
    pub dst: String,
    pub src_port: Option<u16>,
    pub dst_port: Option<u16>,
    pub tx_packets: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub rx_bytes: u64,
}

impl FlowConnection {
    pub fn canonical_src(&self) -> String {
        canonical_endpoint(&self.src, self.src_port)
    }

    pub fn canonical_dst(&self) -> String {
        canonical_endpoint(&self.dst, self.dst_port)
    }
}

fn canonical_endpoint(address: &str, port: Option<u16>) -> String {
    let address = address.parse::<IpAddr>().map_or_else(
        |_| address.to_owned(),
        |value| match value {
            IpAddr::V4(value) => value.to_string(),
            IpAddr::V6(value) => format!("[{value}]"),
        },
    );
    port.map_or(address.clone(), |port| format!("{address}:{port}"))
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FlowMessage {
    pub node_id: String,
    pub reporting_node_name: Option<String>,
    pub logged: String,
    pub start: String,
    pub end: String,
    pub source_node: Option<FlowNode>,
    pub destination_nodes: Vec<FlowNode>,
    pub virtual_traffic: Vec<FlowConnection>,
    pub subnet_traffic: Vec<FlowConnection>,
    pub exit_traffic: Vec<FlowConnection>,
    pub physical_traffic: Vec<FlowConnection>,
}

impl FlowMessage {
    pub fn records(&self) -> impl Iterator<Item = FlowRecord> + '_ {
        [
            (TrafficClass::Virtual, self.virtual_traffic.as_slice()),
            (TrafficClass::Subnet, self.subnet_traffic.as_slice()),
            (TrafficClass::Exit, self.exit_traffic.as_slice()),
            (TrafficClass::Physical, self.physical_traffic.as_slice()),
        ]
        .into_iter()
        .flat_map(move |(class, connections)| {
            connections.iter().map(move |connection| FlowRecord {
                node_id: self.node_id.clone(),
                reporting_node_name: self.reporting_node_name.clone(),
                logged: self.logged.clone(),
                start: self.start.clone(),
                end: self.end.clone(),
                source_node_id: self.source_node.as_ref().map(|node| node.node_id.clone()),
                source_node_name: self.source_node.as_ref().and_then(|node| node.name.clone()),
                destination_node_ids: self
                    .destination_nodes
                    .iter()
                    .map(|node| node.node_id.clone())
                    .collect(),
                destination_node_names: self
                    .destination_nodes
                    .iter()
                    .filter_map(|node| node.name.clone())
                    .collect(),
                class,
                connection: connection.clone(),
            })
        })
    }

    pub fn filtered_record_count(&self, filter: &FlowFilter) -> usize {
        [
            (TrafficClass::Virtual, self.virtual_traffic.as_slice()),
            (TrafficClass::Subnet, self.subnet_traffic.as_slice()),
            (TrafficClass::Exit, self.exit_traffic.as_slice()),
            (TrafficClass::Physical, self.physical_traffic.as_slice()),
        ]
        .into_iter()
        .flat_map(|(class, connections)| {
            connections
                .iter()
                .filter(move |connection| filter.matches_parts(self, class, connection))
        })
        .count()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FlowRecord {
    pub node_id: String,
    pub reporting_node_name: Option<String>,
    pub logged: String,
    pub start: String,
    pub end: String,
    pub source_node_id: Option<String>,
    pub source_node_name: Option<String>,
    pub destination_node_ids: Vec<String>,
    pub destination_node_names: Vec<String>,
    pub class: TrafficClass,
    pub connection: FlowConnection,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct FlowFilter {
    pub reporting_node_id: Option<String>,
    pub reporting_node_label: Option<String>,
    pub source_node_id: Option<String>,
    pub source_node_label: Option<String>,
    pub destination_node_id: Option<String>,
    pub destination_node_label: Option<String>,
    pub protocol: Option<String>,
    pub source_address: Option<String>,
    pub destination_address: Option<String>,
    pub traffic_class: Option<TrafficClass>,
    pub source_port: Option<u16>,
    pub destination_port: Option<u16>,
    pub minimum_bytes: Option<u64>,
}

impl FlowFilter {
    pub fn matches(&self, record: &FlowRecord) -> bool {
        self.reporting_node_id
            .as_deref()
            .is_none_or(|value| value == record.node_id)
            && (self.reporting_node_id.is_some()
                || self
                    .reporting_node_label
                    .as_deref()
                    .is_none_or(|value| record.reporting_node_name.as_deref() == Some(value)))
            && self
                .source_node_id
                .as_deref()
                .is_none_or(|value| record.source_node_id.as_deref() == Some(value))
            && (self.source_node_id.is_some()
                || self
                    .source_node_label
                    .as_deref()
                    .is_none_or(|value| record.source_node_name.as_deref() == Some(value)))
            && self
                .destination_node_id
                .as_deref()
                .is_none_or(|value| record.destination_node_ids.iter().any(|node| node == value))
            && (self.destination_node_id.is_some()
                || self.destination_node_label.as_deref().is_none_or(|value| {
                    record
                        .destination_node_names
                        .iter()
                        .any(|node| node == value)
                }))
            && self
                .protocol
                .as_deref()
                .is_none_or(|value| value.eq_ignore_ascii_case(&record.connection.proto))
            && self
                .source_address
                .as_deref()
                .is_none_or(|value| value == record.connection.src)
            && self
                .destination_address
                .as_deref()
                .is_none_or(|value| value == record.connection.dst)
            && self.traffic_class.is_none_or(|value| value == record.class)
            && self
                .source_port
                .is_none_or(|value| record.connection.src_port == Some(value))
            && self
                .destination_port
                .is_none_or(|value| record.connection.dst_port == Some(value))
            && self.minimum_bytes.is_none_or(|value| {
                record
                    .connection
                    .tx_bytes
                    .checked_add(record.connection.rx_bytes)
                    .is_some_and(|total| total >= value)
            })
    }

    fn matches_parts(
        &self,
        message: &FlowMessage,
        class: TrafficClass,
        connection: &FlowConnection,
    ) -> bool {
        self.reporting_node_id
            .as_deref()
            .is_none_or(|value| value == message.node_id)
            && (self.reporting_node_id.is_some()
                || self
                    .reporting_node_label
                    .as_deref()
                    .is_none_or(|value| message.reporting_node_name.as_deref() == Some(value)))
            && self.source_node_id.as_deref().is_none_or(|value| {
                message
                    .source_node
                    .as_ref()
                    .is_some_and(|node| node.node_id == value)
            })
            && (self.source_node_id.is_some()
                || self.source_node_label.as_deref().is_none_or(|value| {
                    message
                        .source_node
                        .as_ref()
                        .and_then(|node| node.name.as_ref())
                        .is_some_and(|name| name == value)
                }))
            && self.destination_node_id.as_deref().is_none_or(|value| {
                message
                    .destination_nodes
                    .iter()
                    .any(|node| node.node_id == value)
            })
            && (self.destination_node_id.is_some()
                || self.destination_node_label.as_deref().is_none_or(|value| {
                    message
                        .destination_nodes
                        .iter()
                        .filter_map(|node| node.name.as_ref())
                        .any(|name| name == value)
                }))
            && self
                .protocol
                .as_deref()
                .is_none_or(|value| value.eq_ignore_ascii_case(&connection.proto))
            && self
                .source_address
                .as_deref()
                .is_none_or(|value| value == connection.src)
            && self
                .destination_address
                .as_deref()
                .is_none_or(|value| value == connection.dst)
            && self.traffic_class.is_none_or(|value| value == class)
            && self
                .source_port
                .is_none_or(|value| connection.src_port == Some(value))
            && self
                .destination_port
                .is_none_or(|value| connection.dst_port == Some(value))
            && self.minimum_bytes.is_none_or(|value| {
                connection
                    .tx_bytes
                    .checked_add(connection.rx_bytes)
                    .is_some_and(|total| total >= value)
            })
    }

    pub fn validate(&self) -> Result<(), FlowError> {
        for (name, value) in [
            ("reporting node ID", self.reporting_node_id.as_deref()),
            ("reporting node label", self.reporting_node_label.as_deref()),
            ("source node ID", self.source_node_id.as_deref()),
            ("source node label", self.source_node_label.as_deref()),
            ("destination node ID", self.destination_node_id.as_deref()),
            (
                "destination node label",
                self.destination_node_label.as_deref(),
            ),
            ("protocol", self.protocol.as_deref()),
            ("source address", self.source_address.as_deref()),
            ("destination address", self.destination_address.as_deref()),
        ] {
            if value.is_some_and(|value| value.chars().any(char::is_control)) {
                return Err(FlowError::InvalidFilter(format!(
                    "{name} contains a control character"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum AggregateDimension {
    ReportingNode,
    SourceNode,
    DestinationNode,
    TrafficClass,
    Protocol,
    SourcePort,
    DestinationPort,
}

impl AggregateDimension {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ReportingNode => "reporting_node",
            Self::SourceNode => "source_node",
            Self::DestinationNode => "destination_node",
            Self::TrafficClass => "traffic_class",
            Self::Protocol => "protocol",
            Self::SourcePort => "source_port",
            Self::DestinationPort => "destination_port",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AggregatedFlow {
    pub key: Vec<String>,
    pub tx_packets: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub rx_bytes: u64,
    pub records: usize,
}

pub fn aggregate_checked(
    messages: &[FlowMessage],
    filter: &FlowFilter,
    dimensions: &[AggregateDimension],
) -> Result<Vec<AggregatedFlow>, FlowError> {
    aggregate_checked_cancellable(messages, filter, dimensions, None)
}

pub fn aggregate_checked_cancellable(
    messages: &[FlowMessage],
    filter: &FlowFilter,
    dimensions: &[AggregateDimension],
    cancellation: Option<&AtomicBool>,
) -> Result<Vec<AggregatedFlow>, FlowError> {
    filter.validate()?;
    if dimensions.is_empty() {
        return Err(FlowError::EmptyAggregation);
    }
    let mut rows: BTreeMap<Vec<String>, AggregatedFlow> = BTreeMap::new();
    for message in messages {
        if cancellation.is_some_and(|value| value.load(Ordering::Relaxed)) {
            return Err(FlowError::Cancelled);
        }
        for record in message.records() {
            if cancellation.is_some_and(|value| value.load(Ordering::Relaxed)) {
                return Err(FlowError::Cancelled);
            }
            if !filter.matches(&record) {
                continue;
            }
            let key = dimensions
                .iter()
                .map(|dimension| dimension_value(*dimension, &record))
                .collect::<Vec<_>>();
            let row = rows.entry(key.clone()).or_insert(AggregatedFlow {
                key,
                tx_packets: 0,
                tx_bytes: 0,
                rx_packets: 0,
                rx_bytes: 0,
                records: 0,
            });
            row.tx_packets = row
                .tx_packets
                .checked_add(record.connection.tx_packets)
                .ok_or(FlowError::CounterOverflow)?;
            row.tx_bytes = row
                .tx_bytes
                .checked_add(record.connection.tx_bytes)
                .ok_or(FlowError::CounterOverflow)?;
            row.rx_packets = row
                .rx_packets
                .checked_add(record.connection.rx_packets)
                .ok_or(FlowError::CounterOverflow)?;
            row.rx_bytes = row
                .rx_bytes
                .checked_add(record.connection.rx_bytes)
                .ok_or(FlowError::CounterOverflow)?;
            row.records = row
                .records
                .checked_add(1)
                .ok_or(FlowError::CounterOverflow)?;
        }
    }
    Ok(rows.into_values().collect())
}

fn dimension_value(dimension: AggregateDimension, record: &FlowRecord) -> String {
    match dimension {
        AggregateDimension::ReportingNode => record.node_id.clone(),
        AggregateDimension::SourceNode => record
            .source_node_id
            .clone()
            .unwrap_or_else(|| "<not returned>".to_owned()),
        AggregateDimension::DestinationNode => {
            if record.destination_node_ids.is_empty() {
                "<not returned>".to_owned()
            } else {
                record.destination_node_ids.join(",")
            }
        }
        AggregateDimension::TrafficClass => record.class.label().to_owned(),
        AggregateDimension::Protocol => record.connection.proto.clone(),
        AggregateDimension::SourcePort => record
            .connection
            .src_port
            .map_or_else(|| "<not returned>".to_owned(), |value| value.to_string()),
        AggregateDimension::DestinationPort => record
            .connection
            .dst_port
            .map_or_else(|| "<not returned>".to_owned(), |value| value.to_string()),
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum FlowMode {
    Raw,
    Aggregate(Vec<AggregateDimension>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FlowSnapshot {
    pub window: FlowWindow,
    pub messages: Vec<FlowMessage>,
    pub mode: FlowMode,
    pub complete: bool,
    pub limitation: Option<String>,
    pub observed_at: Timestamp,
    pub filter: FlowFilter,
    pub aggregates: Option<Vec<AggregatedFlow>>,
    visible_record_count: usize,
}

impl FlowSnapshot {
    pub fn from_messages(
        window: FlowWindow,
        messages: Vec<FlowMessage>,
        mode: FlowMode,
        observed_at: Timestamp,
    ) -> Result<Self, FlowError> {
        if messages.len() > MAX_FLOW_MESSAGES {
            return Err(FlowError::MessageLimit);
        }
        let visible_record_count = messages.iter().try_fold(0_usize, |total, message| {
            total
                .checked_add(message.filtered_record_count(&FlowFilter::default()))
                .ok_or(FlowError::CounterOverflow)
        })?;
        Ok(Self {
            window,
            messages,
            mode,
            complete: true,
            limitation: None,
            observed_at,
            filter: FlowFilter::default(),
            aggregates: None,
            visible_record_count,
        })
    }

    pub fn visible_record_count(&self) -> usize {
        self.visible_record_count
    }

    pub fn set_filter(&mut self, filter: FlowFilter) {
        self.visible_record_count = self
            .messages
            .iter()
            .map(|message| message.filtered_record_count(&filter))
            .sum();
        self.filter = filter;
    }

    pub fn has_clock_skew(&self) -> bool {
        self.messages.iter().any(|message| {
            let Ok(logged) = OffsetDateTime::parse(&message.logged, &Rfc3339) else {
                return false;
            };
            let Ok(start) = OffsetDateTime::parse(&message.start, &Rfc3339) else {
                return false;
            };
            let Ok(end) = OffsetDateTime::parse(&message.end, &Rfc3339) else {
                return false;
            };
            logged >= self.window.start
                && logged <= self.window.end
                && (start < self.window.start || end > self.window.end)
        })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct FlowGeneration {
    pub generation: u64,
    pub cancellable: bool,
}

impl FlowGeneration {
    pub const fn new() -> Self {
        Self {
            generation: 0,
            cancellable: false,
        }
    }

    pub fn begin(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.cancellable = true;
        self.generation
    }

    pub fn cancel(&mut self, generation: u64) -> bool {
        if self.generation != generation || !self.cancellable {
            return false;
        }
        self.cancellable = false;
        true
    }

    pub const fn accepts(&self, generation: u64) -> bool {
        self.generation == generation && self.cancellable
    }
}

impl Default for FlowGeneration {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_bounded_and_explicit() {
        let now = OffsetDateTime::UNIX_EPOCH + Duration::days(10);
        assert!(FlowWindow::new(now - Duration::hours(24), now, now).is_ok());
        assert_eq!(
            FlowWindow::new(now - Duration::hours(25), now, now),
            Err(FlowError::WindowTooWide)
        );
    }

    #[test]
    fn aggregation_is_checked_and_sorted() {
        let now = OffsetDateTime::UNIX_EPOCH + Duration::days(10);
        let window = FlowWindow::new(now - Duration::hours(1), now, now);
        assert!(window.is_ok());
        let message = FlowMessage {
            node_id: "node-a".to_owned(),
            reporting_node_name: None,
            logged: "2026-08-04T00:00:00Z".to_owned(),
            start: "2026-08-04T00:00:00Z".to_owned(),
            end: "2026-08-04T00:01:00Z".to_owned(),
            source_node: None,
            destination_nodes: Vec::new(),
            virtual_traffic: vec![FlowConnection {
                proto: "tcp".to_owned(),
                src: "100.64.0.1".to_owned(),
                dst: "100.64.0.2".to_owned(),
                src_port: Some(22),
                dst_port: Some(22),
                tx_packets: 1,
                tx_bytes: 2,
                rx_packets: 3,
                rx_bytes: 4,
            }],
            subnet_traffic: Vec::new(),
            exit_traffic: Vec::new(),
            physical_traffic: Vec::new(),
        };
        let result = aggregate_checked(
            &[message],
            &FlowFilter::default(),
            &[AggregateDimension::Protocol],
        );
        assert_eq!(result.map(|rows| rows[0].rx_bytes), Ok(4));
    }
}
