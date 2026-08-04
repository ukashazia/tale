use serde_json::{Map, Value};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::Timestamp;

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum ExportError {
    #[error("export collection is unsupported")]
    UnsupportedCollection,
    #[error("export schema contains a secret field")]
    SecretField,
    #[error("export timestamp is invalid")]
    InvalidTimestamp,
    #[error("export schema version is unsupported")]
    UnsupportedSchemaVersion,
    #[error("export row contains an invalid value")]
    InvalidRow,
    #[error("CSV export requires a supported row schema")]
    UnsupportedCsvSchema,
    #[error("export row does not match the declared schema")]
    SchemaRowMismatch,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum ExportCollection {
    Devices,
    Users,
    Routes,
    Dns,
    CredentialMetadata,
    Audit,
    HealthFindings,
    FlowLogs,
}

impl ExportCollection {
    pub const fn schema_name(self) -> &'static str {
        match self {
            Self::Devices => "devices",
            Self::Users => "users",
            Self::Routes => "routes",
            Self::Dns => "dns",
            Self::CredentialMetadata => "credentials_metadata",
            Self::Audit => "audit",
            Self::HealthFindings => "health_findings",
            Self::FlowLogs => "flow_logs",
        }
    }

    pub const fn row_kind(self) -> &'static str {
        match self {
            Self::Devices => "device",
            Self::Users => "user",
            Self::Routes => "route",
            Self::Dns => "dns",
            Self::CredentialMetadata => "credential_metadata",
            Self::Audit => "audit",
            Self::HealthFindings => "health_finding",
            Self::FlowLogs => "flow_log",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ExportSource {
    pub id: String,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ExportMetadata {
    pub schema: ExportCollection,
    pub schema_version: u32,
    pub tale_version: String,
    pub sources: Vec<ExportSource>,
    pub observed_at: Timestamp,
    pub route: String,
    pub active_filter: String,
    pub active_sort: String,
    pub truncated: bool,
    pub complete: bool,
    pub export_timestamp: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ExportDocument {
    pub metadata: ExportMetadata,
    pub rows: Vec<ExportRow>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ExportRow {
    Device {
        id: String,
        name: String,
        addresses: Vec<String>,
        source: String,
        observed_at: Timestamp,
    },
    User {
        id: String,
        name: String,
        role: String,
        source: String,
        observed_at: Timestamp,
    },
    Route {
        id: String,
        cidr: String,
        advertiser: String,
        approval: String,
        source: String,
        observed_at: Timestamp,
    },
    Dns {
        name: String,
        value: String,
        source: String,
        observed_at: Timestamp,
    },
    CredentialMetadata {
        id: String,
        credential_type: String,
        status: String,
        created_at: Option<Timestamp>,
        expires_at: Option<Timestamp>,
        source: String,
        observed_at: Timestamp,
    },
    Audit {
        event_id: String,
        event_time: String,
        action: String,
        actor: String,
        target: String,
        source: String,
        observed_at: Timestamp,
    },
    HealthFinding {
        id: String,
        rule_id: String,
        severity: String,
        title: String,
        affected_resource_ids: Vec<String>,
        source_ids: Vec<String>,
        derived: bool,
        observed_at: Timestamp,
    },
    FlowLog {
        reporting_node: String,
        logged: String,
        start: String,
        end: String,
        traffic_class: String,
        protocol: String,
        source: String,
        destination: String,
        tx_packets: u64,
        tx_bytes: u64,
        rx_packets: u64,
        rx_bytes: u64,
    },
}

impl ExportRow {
    pub const fn collection(&self) -> ExportCollection {
        match self {
            Self::Device { .. } => ExportCollection::Devices,
            Self::User { .. } => ExportCollection::Users,
            Self::Route { .. } => ExportCollection::Routes,
            Self::Dns { .. } => ExportCollection::Dns,
            Self::CredentialMetadata { .. } => ExportCollection::CredentialMetadata,
            Self::Audit { .. } => ExportCollection::Audit,
            Self::HealthFinding { .. } => ExportCollection::HealthFindings,
            Self::FlowLog { .. } => ExportCollection::FlowLogs,
        }
    }

    pub fn stable_key(&self) -> String {
        match self {
            Self::Device { id, .. }
            | Self::User { id, .. }
            | Self::Route { id, .. }
            | Self::CredentialMetadata { id, .. }
            | Self::HealthFinding { id, .. } => id.clone(),
            Self::Dns { name, value, .. } => format!("{name}\u{0}{value}"),
            Self::Audit { event_id, .. } => event_id.clone(),
            Self::FlowLog {
                reporting_node,
                logged,
                source,
                destination,
                ..
            } => format!("{reporting_node}\u{0}{logged}\u{0}{source}\u{0}{destination}"),
        }
    }

    fn fields(&self) -> Vec<(&'static str, Value)> {
        match self {
            Self::Device {
                id,
                name,
                addresses,
                source,
                observed_at,
            } => vec![
                ("id", Value::String(id.clone())),
                ("name", Value::String(name.clone())),
                ("addresses", compact_list(addresses)),
                ("source", Value::String(source.clone())),
                ("observed_at", timestamp_value(*observed_at)),
            ],
            Self::User {
                id,
                name,
                role,
                source,
                observed_at,
            } => vec![
                ("id", Value::String(id.clone())),
                ("name", Value::String(name.clone())),
                ("role", Value::String(role.clone())),
                ("source", Value::String(source.clone())),
                ("observed_at", timestamp_value(*observed_at)),
            ],
            Self::Route {
                id,
                cidr,
                advertiser,
                approval,
                source,
                observed_at,
            } => vec![
                ("id", Value::String(id.clone())),
                ("cidr", Value::String(cidr.clone())),
                ("advertiser", Value::String(advertiser.clone())),
                ("approval", Value::String(approval.clone())),
                ("source", Value::String(source.clone())),
                ("observed_at", timestamp_value(*observed_at)),
            ],
            Self::Dns {
                name,
                value,
                source,
                observed_at,
            } => vec![
                ("name", Value::String(name.clone())),
                ("value", Value::String(value.clone())),
                ("source", Value::String(source.clone())),
                ("observed_at", timestamp_value(*observed_at)),
            ],
            Self::CredentialMetadata {
                id,
                credential_type,
                status,
                created_at,
                expires_at,
                source,
                observed_at,
            } => vec![
                ("id", Value::String(id.clone())),
                ("credential_type", Value::String(credential_type.clone())),
                ("status", Value::String(status.clone())),
                (
                    "created_at",
                    created_at.map_or(Value::Null, timestamp_value),
                ),
                (
                    "expires_at",
                    expires_at.map_or(Value::Null, timestamp_value),
                ),
                ("source", Value::String(source.clone())),
                ("observed_at", timestamp_value(*observed_at)),
            ],
            Self::Audit {
                event_id,
                event_time,
                action,
                actor,
                target,
                source,
                observed_at,
            } => vec![
                ("event_id", Value::String(event_id.clone())),
                ("event_time", Value::String(event_time.clone())),
                ("action", Value::String(action.clone())),
                ("actor", Value::String(actor.clone())),
                ("target", Value::String(target.clone())),
                ("source", Value::String(source.clone())),
                ("observed_at", timestamp_value(*observed_at)),
            ],
            Self::HealthFinding {
                id,
                rule_id,
                severity,
                title,
                affected_resource_ids,
                source_ids,
                derived,
                observed_at,
            } => vec![
                ("id", Value::String(id.clone())),
                ("rule_id", Value::String(rule_id.clone())),
                ("severity", Value::String(severity.clone())),
                ("title", Value::String(title.clone())),
                ("affected_resource_ids", compact_list(affected_resource_ids)),
                ("source_ids", compact_list(source_ids)),
                ("derived", Value::Bool(*derived)),
                ("observed_at", timestamp_value(*observed_at)),
            ],
            Self::FlowLog {
                reporting_node,
                logged,
                start,
                end,
                traffic_class,
                protocol,
                source,
                destination,
                tx_packets,
                tx_bytes,
                rx_packets,
                rx_bytes,
            } => vec![
                ("reporting_node", Value::String(reporting_node.clone())),
                ("logged", Value::String(logged.clone())),
                ("start", Value::String(start.clone())),
                ("end", Value::String(end.clone())),
                ("traffic_class", Value::String(traffic_class.clone())),
                ("protocol", Value::String(protocol.clone())),
                ("source", Value::String(source.clone())),
                ("destination", Value::String(destination.clone())),
                ("tx_packets", Value::from(*tx_packets)),
                ("tx_bytes", Value::from(*tx_bytes)),
                ("rx_packets", Value::from(*rx_packets)),
                ("rx_bytes", Value::from(*rx_bytes)),
            ],
        }
    }
}

fn timestamp_value(value: Timestamp) -> Value {
    Value::String(timestamp_text(value))
}

fn timestamp_text(value: Timestamp) -> String {
    let Ok(value) = i64::try_from(value) else {
        return "1970-01-01T00:00:00Z".to_owned();
    };
    let Ok(timestamp) = OffsetDateTime::from_unix_timestamp(value) else {
        return "1970-01-01T00:00:00Z".to_owned();
    };
    timestamp
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn timestamp_is_invalid(value: Timestamp) -> bool {
    i64::try_from(value)
        .ok()
        .and_then(|value| OffsetDateTime::from_unix_timestamp(value).ok())
        .is_none()
}

fn timestamp_text_is_invalid(value: &str) -> bool {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_or(true, |timestamp| timestamp.offset() != time::UtcOffset::UTC)
}

fn compact_list(values: &[String]) -> Value {
    let mut values = values.to_vec();
    values.sort();
    values.dedup();
    Value::Array(values.into_iter().map(Value::String).collect())
}

impl ExportDocument {
    pub fn sort_rows(&mut self) {
        self.rows.sort_by(|left, right| {
            left.stable_key()
                .cmp(&right.stable_key())
                .then_with(|| row_tiebreak(left).cmp(&row_tiebreak(right)))
        });
    }

    pub fn validate(&self) -> Result<(), ExportError> {
        if self.metadata.schema_version != 1 {
            return Err(ExportError::UnsupportedSchemaVersion);
        }
        if self
            .metadata
            .export_timestamp
            .as_deref()
            .is_some_and(timestamp_text_is_invalid)
            || timestamp_is_invalid(self.metadata.observed_at)
            || self
                .metadata
                .sources
                .iter()
                .any(|source| timestamp_is_invalid(source.observed_at))
        {
            return Err(ExportError::InvalidTimestamp);
        }
        if self
            .rows
            .iter()
            .any(|row| row.collection() != self.metadata.schema)
        {
            return Err(ExportError::SchemaRowMismatch);
        }
        if self.rows.iter().any(|row| match row {
            ExportRow::Device { observed_at, .. }
            | ExportRow::User { observed_at, .. }
            | ExportRow::Route { observed_at, .. }
            | ExportRow::Dns { observed_at, .. }
            | ExportRow::HealthFinding { observed_at, .. } => timestamp_is_invalid(*observed_at),
            ExportRow::CredentialMetadata {
                created_at,
                expires_at,
                observed_at,
                ..
            } => {
                created_at.is_some_and(timestamp_is_invalid)
                    || expires_at.is_some_and(timestamp_is_invalid)
                    || timestamp_is_invalid(*observed_at)
            }
            ExportRow::Audit {
                event_time,
                observed_at,
                ..
            } => timestamp_text_is_invalid(event_time) || timestamp_is_invalid(*observed_at),
            ExportRow::FlowLog {
                logged, start, end, ..
            } => {
                timestamp_text_is_invalid(logged)
                    || timestamp_text_is_invalid(start)
                    || timestamp_text_is_invalid(end)
            }
        }) {
            return Err(ExportError::InvalidTimestamp);
        }
        if self.rows.iter().any(|row| match row {
            ExportRow::Device { id, .. } | ExportRow::User { id, .. } => id.is_empty(),
            ExportRow::CredentialMetadata { id, .. }
            | ExportRow::Audit { event_id: id, .. }
            | ExportRow::HealthFinding { id, .. } => id.is_empty(),
            ExportRow::FlowLog { reporting_node, .. } => reporting_node.is_empty(),
            ExportRow::Route { .. } | ExportRow::Dns { .. } => false,
        }) {
            return Err(ExportError::InvalidRow);
        }
        Ok(())
    }

    pub fn json_bytes(&self) -> Result<Vec<u8>, ExportError> {
        self.validate()?;
        let mut rows = self.rows.clone();
        rows.sort_by(|left, right| {
            left.stable_key()
                .cmp(&right.stable_key())
                .then_with(|| row_tiebreak(left).cmp(&row_tiebreak(right)))
        });
        self.json_bytes_for_rows(&rows)
    }

    pub fn json_bytes_in_order(&self) -> Result<Vec<u8>, ExportError> {
        self.validate()?;
        self.json_bytes_for_rows(&self.rows)
    }

    fn json_bytes_for_rows(&self, rows: &[ExportRow]) -> Result<Vec<u8>, ExportError> {
        let mut root = Map::new();
        root.insert("metadata".to_owned(), self.metadata_json());
        root.insert(
            "rows".to_owned(),
            Value::Array(
                rows.iter()
                    .map(|row| {
                        let mut object = Map::new();
                        object.insert(
                            "_row_kind".to_owned(),
                            Value::String(self.metadata.schema.row_kind().to_owned()),
                        );
                        for (key, value) in row.fields() {
                            object.insert(key.to_owned(), value);
                        }
                        Value::Object(object)
                    })
                    .collect(),
            ),
        );
        serde_json::to_vec(&Value::Object(root)).map_err(|_| ExportError::InvalidRow)
    }

    fn metadata_json(&self) -> Value {
        let mut sources = self.metadata.sources.iter().collect::<Vec<_>>();
        sources.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.observed_at.cmp(&right.observed_at))
        });
        let mut object = Map::new();
        object.insert(
            "schema".to_owned(),
            Value::String(self.metadata.schema.schema_name().to_owned()),
        );
        object.insert(
            "schema_version".to_owned(),
            Value::from(self.metadata.schema_version),
        );
        object.insert(
            "tale_version".to_owned(),
            Value::String(self.metadata.tale_version.clone()),
        );
        object.insert(
            "sources".to_owned(),
            Value::Array(
                sources
                    .iter()
                    .map(|source| {
                        let mut value = Map::new();
                        value.insert("id".to_owned(), Value::String(source.id.clone()));
                        value.insert(
                            "observed_at".to_owned(),
                            timestamp_value(source.observed_at),
                        );
                        Value::Object(value)
                    })
                    .collect(),
            ),
        );
        object.insert(
            "observed_at".to_owned(),
            timestamp_value(self.metadata.observed_at),
        );
        object.insert(
            "route".to_owned(),
            Value::String(self.metadata.route.clone()),
        );
        object.insert(
            "filter".to_owned(),
            Value::String(self.metadata.active_filter.clone()),
        );
        object.insert(
            "sort".to_owned(),
            Value::String(self.metadata.active_sort.clone()),
        );
        object.insert("truncated".to_owned(), Value::Bool(self.metadata.truncated));
        object.insert("complete".to_owned(), Value::Bool(self.metadata.complete));
        if let Some(timestamp) = &self.metadata.export_timestamp {
            object.insert(
                "export_timestamp".to_owned(),
                Value::String(timestamp.clone()),
            );
        }
        Value::Object(object)
    }

    pub fn csv_bytes(&self) -> Result<Vec<u8>, ExportError> {
        self.validate()?;
        let mut rows = self.rows.clone();
        rows.sort_by(|left, right| {
            left.stable_key()
                .cmp(&right.stable_key())
                .then_with(|| row_tiebreak(left).cmp(&row_tiebreak(right)))
        });
        self.csv_bytes_for_rows(&rows)
    }

    pub fn csv_bytes_in_order(&self) -> Result<Vec<u8>, ExportError> {
        self.validate()?;
        self.csv_bytes_for_rows(&self.rows)
    }

    fn csv_bytes_for_rows(&self, rows: &[ExportRow]) -> Result<Vec<u8>, ExportError> {
        let columns = csv_columns(self.metadata.schema);
        let metadata_columns = [
            "_schema_version",
            "_tale_version",
            "_truncated",
            "_complete",
            "_export_timestamp",
        ];
        let mut output = String::new();
        output.push_str("_row_kind,_schema,_observed_at,_sources,_filter,_sort,");
        output.push_str(&columns.join(","));
        output.push(',');
        output.push_str(&metadata_columns.join(","));
        output.push('\n');
        let mut source_values = self.metadata.sources.iter().collect::<Vec<_>>();
        source_values.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.observed_at.cmp(&right.observed_at))
        });
        let sources = source_values
            .iter()
            .map(|source| source.id.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let mut metadata_values = vec![
            "metadata".to_owned(),
            self.metadata.schema.schema_name().to_owned(),
            timestamp_text(self.metadata.observed_at),
            sources.clone(),
            self.metadata.active_filter.clone(),
            self.metadata.active_sort.clone(),
        ];
        metadata_values.extend(columns.iter().map(|_| String::new()));
        metadata_values.extend([
            self.metadata.schema_version.to_string(),
            self.metadata.tale_version.clone(),
            self.metadata.truncated.to_string(),
            self.metadata.complete.to_string(),
            self.metadata.export_timestamp.clone().unwrap_or_default(),
        ]);
        output.push_str(
            &metadata_values
                .iter()
                .map(|value| csv_escape(value))
                .collect::<Vec<_>>()
                .join(","),
        );
        output.push('\n');
        for row in rows {
            let values = row
                .fields()
                .into_iter()
                .map(|(_, value)| csv_value(&value))
                .collect::<Vec<_>>();
            let mut fields = vec![
                self.metadata.schema.row_kind().to_owned(),
                self.metadata.schema.schema_name().to_owned(),
                timestamp_text(self.metadata.observed_at),
                sources.clone(),
                self.metadata.active_filter.clone(),
                self.metadata.active_sort.clone(),
            ];
            fields.extend(values);
            fields.extend(metadata_columns.iter().map(|_| String::new()));
            output.push_str(
                &fields
                    .iter()
                    .map(|value| csv_escape(value))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            output.push('\n');
        }
        Ok(output.into_bytes())
    }
}

fn row_tiebreak(row: &ExportRow) -> Vec<u8> {
    serde_json::to_vec(&row.fields()).unwrap_or_else(|_| Vec::new())
}

fn csv_columns(collection: ExportCollection) -> Vec<&'static str> {
    match collection {
        ExportCollection::Devices => vec!["id", "name", "addresses", "source", "observed_at"],
        ExportCollection::Users => vec!["id", "name", "role", "source", "observed_at"],
        ExportCollection::Routes => {
            vec![
                "id",
                "cidr",
                "advertiser",
                "approval",
                "source",
                "observed_at",
            ]
        }
        ExportCollection::Dns => vec!["name", "value", "source", "observed_at"],
        ExportCollection::CredentialMetadata => vec![
            "id",
            "credential_type",
            "status",
            "created_at",
            "expires_at",
            "source",
            "observed_at",
        ],
        ExportCollection::Audit => {
            vec![
                "event_id",
                "event_time",
                "action",
                "actor",
                "target",
                "source",
                "observed_at",
            ]
        }
        ExportCollection::HealthFindings => vec![
            "id",
            "rule_id",
            "severity",
            "title",
            "affected_resource_ids",
            "source_ids",
            "derived",
            "observed_at",
        ],
        ExportCollection::FlowLogs => vec![
            "reporting_node",
            "logged",
            "start",
            "end",
            "traffic_class",
            "protocol",
            "source",
            "destination",
            "tx_packets",
            "tx_bytes",
            "rx_packets",
            "rx_bytes",
        ],
    }
}

fn csv_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => match serde_json::to_string(value) {
            Ok(serialized) => serialized,
            Err(_) => "[]".to_owned(),
        },
    }
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> ExportDocument {
        ExportDocument {
            metadata: ExportMetadata {
                schema: ExportCollection::HealthFindings,
                schema_version: 1,
                tale_version: "tale/test".to_owned(),
                sources: vec![ExportSource {
                    id: "fixture".to_owned(),
                    observed_at: 100,
                }],
                observed_at: 100,
                route: "overview".to_owned(),
                active_filter: "none".to_owned(),
                active_sort: "severity,id".to_owned(),
                truncated: false,
                complete: true,
                export_timestamp: None,
            },
            rows: Vec::new(),
        }
    }

    #[test]
    fn empty_exports_still_have_metadata_rows() {
        let value = document();
        let json = value.json_bytes();
        assert!(json.is_ok());
        assert!(json.is_ok_and(|bytes| !bytes.is_empty()));
        let csv = value.csv_bytes();
        assert!(csv.is_ok());
        assert!(csv.is_ok_and(|bytes| {
            String::from_utf8(bytes).is_ok_and(|text| text.lines().count() == 2)
        }));
    }
}
