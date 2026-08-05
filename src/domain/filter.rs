use std::time::Duration;

use super::Timestamp;
use super::device::{AdminDevice, Device, Liveness};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FilterOperator {
    Match,
    LessThan,
    LessOrEqual,
    GreaterThan,
    GreaterOrEqual,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FilterValueKind {
    Text,
    Boolean,
    Duration,
    Enumeration(&'static [&'static str]),
    SnapshotValue,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct FilterFieldSpec {
    pub canonical_name: &'static str,
    pub aliases: &'static [&'static str],
    pub operators: &'static [FilterOperator],
    pub value_kind: FilterValueKind,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct FilterSchema {
    pub fields: &'static [FilterFieldSpec],
}

const MATCH: &[FilterOperator] = &[FilterOperator::Match];
const AGE_OPERATORS: &[FilterOperator] = &[
    FilterOperator::Match,
    FilterOperator::LessThan,
    FilterOperator::LessOrEqual,
    FilterOperator::GreaterThan,
    FilterOperator::GreaterOrEqual,
];
const DEVICE_FILTER_FIELDS: &[FilterFieldSpec] = &[
    field("id", &[], MATCH, FilterValueKind::Text, "Device identity"),
    field("name", &[], MATCH, FilterValueKind::Text, "Display name"),
    field(
        "online",
        &[],
        MATCH,
        FilterValueKind::Boolean,
        "Online state",
    ),
    field(
        "owner",
        &[],
        MATCH,
        FilterValueKind::SnapshotValue,
        "Owner name or ID",
    ),
    field(
        "os",
        &[],
        MATCH,
        FilterValueKind::SnapshotValue,
        "Operating system",
    ),
    field(
        "path",
        &[],
        MATCH,
        FilterValueKind::SnapshotValue,
        "Connection path",
    ),
    field(
        "tag",
        &[],
        MATCH,
        FilterValueKind::SnapshotValue,
        "Device tag",
    ),
    field(
        "lastSeen",
        &["last_seen"],
        AGE_OPERATORS,
        FilterValueKind::Duration,
        "Last-seen age",
    ),
    field(
        "approval",
        &[],
        MATCH,
        FilterValueKind::Enumeration(&["approved", "pending", "revoked"]),
        "Approval state",
    ),
    field(
        "version",
        &["clientVersion"],
        MATCH,
        FilterValueKind::Text,
        "Client version",
    ),
];

const fn field(
    canonical_name: &'static str,
    aliases: &'static [&'static str],
    operators: &'static [FilterOperator],
    value_kind: FilterValueKind,
    description: &'static str,
) -> FilterFieldSpec {
    FilterFieldSpec {
        canonical_name,
        aliases,
        operators,
        value_kind,
        description,
    }
}

pub const fn device_schema() -> FilterSchema {
    FilterSchema {
        fields: DEVICE_FILTER_FIELDS,
    }
}

pub const fn activity_schema() -> FilterSchema {
    FilterSchema { fields: &[] }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilterExpression {
    pub terms: Vec<FilterTerm>,
}

impl FilterExpression {
    pub const fn empty() -> Self {
        Self { terms: Vec::new() }
    }

    pub fn matches(&self, device: &Device, now: Timestamp) -> bool {
        self.matches_with_dns(device, None, now)
    }

    pub fn matches_with_dns(
        &self,
        device: &Device,
        dns_name: Option<&str>,
        now: Timestamp,
    ) -> bool {
        self.terms
            .iter()
            .all(|term| term.matches(device, dns_name, now))
    }

    pub fn requires_admin_data(&self) -> bool {
        self.terms.iter().any(|term| {
            matches!(
                term,
                FilterTerm::Field {
                    field: FilterField::Approval
                        | FilterField::KeyExpiry
                        | FilterField::ClientVersion
                        | FilterField::Sharing
                        | FilterField::Posture
                        | FilterField::RouteRole,
                    ..
                }
            ) || matches!(
                term,
                FilterTerm::StructuredField {
                    field: FilterField::Approval
                        | FilterField::KeyExpiry
                        | FilterField::ClientVersion
                        | FilterField::Sharing
                        | FilterField::Posture
                        | FilterField::RouteRole,
                    ..
                }
            )
        })
    }

    pub fn matches_admin(&self, device: &AdminDevice, now: Timestamp) -> bool {
        self.terms
            .iter()
            .all(|term| term.matches_admin(device, now))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum FilterTerm {
    Text(String),
    Field {
        field: FilterField,
        negated: bool,
        values: Vec<String>,
        comparison: Option<Comparison>,
    },
    StructuredField {
        field: FilterField,
        negated: bool,
        value: String,
        mode: FieldMatchMode,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FilterField {
    Id,
    Name,
    Online,
    Owner,
    Os,
    Path,
    Tag,
    LastSeen,
    Property,
    Approval,
    KeyExpiry,
    ClientVersion,
    Sharing,
    Posture,
    RouteRole,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FieldMatchMode {
    Exact,
    Contains,
    StartsWith,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Comparison {
    Less(Duration),
    LessOrEqual(Duration),
    Greater(Duration),
    GreaterOrEqual(Duration),
    Equal(Duration),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilterError {
    pub position: usize,
    pub message: String,
}

impl std::fmt::Display for FilterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} at column {}",
            self.message,
            self.position + 1
        )
    }
}

impl std::error::Error for FilterError {}

pub fn parse(input: &str) -> Result<FilterExpression, FilterError> {
    let tokens = tokenize(input)?;
    let mut terms = Vec::with_capacity(tokens.len());
    for token in tokens {
        terms.push(parse_term(&token)?);
    }
    Ok(FilterExpression { terms })
}

fn tokenize(input: &str) -> Result<Vec<(String, usize)>, FilterError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut start = None;
    let mut quoted = false;
    let mut escaped = false;
    for (position, character) in input.char_indices() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quoted {
            escaped = true;
            if start.is_none() {
                start = Some(position);
            }
            continue;
        }
        if character == '"' {
            quoted = !quoted;
            current.push(character);
            if start.is_none() {
                start = Some(position);
            }
            continue;
        }
        if character.is_whitespace() && !quoted {
            if !current.is_empty() {
                let token_start = start.map_or(position, |value| value);
                tokens.push((std::mem::take(&mut current), token_start));
                start = None;
            }
        } else {
            if start.is_none() {
                start = Some(position);
            }
            current.push(character);
        }
    }
    if escaped || quoted {
        return Err(FilterError {
            position: input.len().saturating_sub(1),
            message: "incomplete quoted value".to_owned(),
        });
    }
    if !current.is_empty() {
        tokens.push((current, start.map_or(0, |value| value)));
    }
    Ok(tokens)
}

fn parse_term((token, position): &(String, usize)) -> Result<FilterTerm, FilterError> {
    let (negated, body) = token
        .strip_prefix('!')
        .map_or((false, token.as_str()), |value| (true, value));
    if body.is_empty() {
        return Err(error(*position, "incomplete filter term"));
    }

    let colon = body.find(':');
    let Some(colon) = colon else {
        if negated {
            return Err(error(*position, "negation must apply to a field"));
        }
        return Ok(FilterTerm::Text(normalize_value(body)?));
    };
    let field_text = &body[..colon];
    let value_text = &body[colon + 1..];
    let field = parse_field(field_text).ok_or(error(*position, "unknown filter field"))?;
    if value_text.is_empty() {
        return Err(error(*position + colon + 1, "filter value is required"));
    }

    if field == FilterField::LastSeen && starts_with_comparison(value_text) {
        let (operator, duration_text) = comparison_parts(value_text)
            .ok_or(error(*position + colon + 1, "incomplete comparison"))?;
        let duration = parse_filter_duration(duration_text)
            .ok_or(error(*position + colon + 1, "invalid duration"))?;
        return Ok(FilterTerm::Field {
            field,
            negated,
            values: Vec::new(),
            comparison: Some(Comparison::from_operator(operator, duration)),
        });
    }

    if let Some((mode, structured_value)) = value_text.split_once('=') {
        let mode = match mode {
            "contains" => FieldMatchMode::Contains,
            "starts_with" => FieldMatchMode::StartsWith,
            _ => {
                return Err(error(
                    *position + colon + 1,
                    "unknown structured filter operator",
                ));
            }
        };
        if structured_value.is_empty() {
            return Err(error(*position + colon + 1, "filter value is required"));
        }
        return Ok(FilterTerm::StructuredField {
            field,
            negated,
            value: normalize_value(structured_value)?,
            mode,
        });
    }

    let values = split_values(value_text, *position + colon + 1)?;
    if values.is_empty() {
        return Err(error(*position + colon + 1, "filter value is required"));
    }
    if field == FilterField::Online {
        for value in &values {
            if !matches!(
                value.as_str(),
                "true" | "false" | "online" | "offline" | "unknown"
            ) {
                return Err(error(
                    *position + colon + 1,
                    "online expects true, false, or unknown",
                ));
            }
        }
    }
    if field == FilterField::Property {
        for value in &values {
            if !matches!(
                value.as_str(),
                "exit-node" | "exit-node-option" | "subnet-router" | "ssh" | "shared"
            ) {
                return Err(error(
                    *position + colon + 1,
                    "property expects exit-node, exit-node-option, subnet-router, ssh, or shared",
                ));
            }
        }
    }
    if field == FilterField::Approval {
        for value in &values {
            if !matches!(
                value.as_str(),
                "true" | "false" | "approved" | "pending" | "unknown"
            ) {
                return Err(error(
                    *position + colon + 1,
                    "approval expects approved, pending, or unknown",
                ));
            }
        }
    }
    if field == FilterField::KeyExpiry {
        for value in &values {
            if !matches!(value.as_str(), "expired" | "soon" | "disabled" | "unknown") {
                return Err(error(
                    *position + colon + 1,
                    "key-expiry expects expired, soon, disabled, or unknown",
                ));
            }
        }
    }
    if field == FilterField::Sharing {
        for value in &values {
            if !matches!(
                value.as_str(),
                "true" | "false" | "external" | "internal" | "unknown"
            ) {
                return Err(error(
                    *position + colon + 1,
                    "sharing expects external, internal, or unknown",
                ));
            }
        }
    }
    if field == FilterField::Posture {
        for value in &values {
            if !matches!(value.as_str(), "present" | "empty" | "unknown") {
                return Err(error(
                    *position + colon + 1,
                    "posture expects present, empty, or unknown",
                ));
            }
        }
    }
    if field == FilterField::RouteRole {
        for value in &values {
            if !matches!(
                value.as_str(),
                "subnet-router" | "exit-node" | "exit-node-option" | "none" | "unknown"
            ) {
                return Err(error(
                    *position + colon + 1,
                    "route-role expects subnet-router, exit-node, exit-node-option, none, or unknown",
                ));
            }
        }
    }
    Ok(FilterTerm::Field {
        field,
        negated,
        values,
        comparison: None,
    })
}

fn split_values(value: &str, position: usize) -> Result<Vec<String>, FilterError> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in value.chars() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                if current.trim().is_empty() {
                    return Err(error(position, "empty OR value"));
                }
                values.push(normalize_value(current.trim())?);
                current.clear();
            }
            other => current.push(other),
        }
    }
    if quoted {
        return Err(error(position, "incomplete quoted value"));
    }
    if current.trim().is_empty() {
        return Err(error(position, "empty OR value"));
    }
    values.push(normalize_value(current.trim())?);
    Ok(values)
}

fn normalize_value(value: &str) -> Result<String, FilterError> {
    if value.starts_with('"') || value.ends_with('"') {
        if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
            return Err(error(0, "incomplete quoted value"));
        }
        return Ok(value[1..value.len().saturating_sub(1)].to_lowercase());
    }
    Ok(value.to_lowercase())
}

fn parse_field(value: &str) -> Option<FilterField> {
    match value.to_ascii_lowercase().as_str() {
        "id" => Some(FilterField::Id),
        "name" => Some(FilterField::Name),
        "online" => Some(FilterField::Online),
        "state" => Some(FilterField::Online),
        "owner" => Some(FilterField::Owner),
        "os" => Some(FilterField::Os),
        "path" => Some(FilterField::Path),
        "tag" => Some(FilterField::Tag),
        "lastseen" => Some(FilterField::LastSeen),
        "property" => Some(FilterField::Property),
        "approval" | "authorized" => Some(FilterField::Approval),
        "keyexpiry" | "key-expiry" => Some(FilterField::KeyExpiry),
        "version" | "clientversion" | "client-version" => Some(FilterField::ClientVersion),
        "sharing" | "shared" => Some(FilterField::Sharing),
        "posture" => Some(FilterField::Posture),
        "routerole" | "route-role" | "role" => Some(FilterField::RouteRole),
        _ => None,
    }
}

fn starts_with_comparison(value: &str) -> bool {
    value.starts_with('<') || value.starts_with('>') || value.starts_with('=')
}

fn comparison_parts(value: &str) -> Option<(&str, &str)> {
    for operator in ["<=", ">=", "<", ">", "="] {
        if let Some(duration) = value.strip_prefix(operator) {
            return Some((operator, duration));
        }
    }
    None
}

fn parse_filter_duration(value: &str) -> Option<Duration> {
    let index = value.find(|character: char| !character.is_ascii_digit())?;
    let (number, unit) = value.split_at(index);
    if number.is_empty() || unit.is_empty() {
        return None;
    }
    let amount = number.parse::<u64>().ok()?;
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        "w" => 604_800,
        _ => return None,
    };
    Duration::from_secs(amount.checked_mul(multiplier)?).checked_add(Duration::from_nanos(0))
}

impl Comparison {
    fn from_operator(operator: &str, duration: Duration) -> Self {
        match operator {
            "<" => Self::Less(duration),
            "<=" => Self::LessOrEqual(duration),
            ">" => Self::Greater(duration),
            ">=" => Self::GreaterOrEqual(duration),
            _ => Self::Equal(duration),
        }
    }

    fn matches(self, age: Option<u64>) -> bool {
        let Some(age) = age else {
            return false;
        };
        let seconds = age;
        match self {
            Self::Less(value) => seconds < value.as_secs(),
            Self::LessOrEqual(value) => seconds <= value.as_secs(),
            Self::Greater(value) => seconds > value.as_secs(),
            Self::GreaterOrEqual(value) => seconds >= value.as_secs(),
            Self::Equal(value) => seconds == value.as_secs(),
        }
    }
}

impl FilterTerm {
    fn matches(&self, device: &Device, dns_name: Option<&str>, now: Timestamp) -> bool {
        match self {
            Self::Text(value) => {
                device.search_text().contains(value)
                    || dns_name.is_some_and(|name| name.to_lowercase().contains(value))
            }
            Self::Field {
                field,
                negated,
                values,
                comparison,
            } => {
                if is_admin_filter_field(*field) {
                    return true;
                }
                let matched = if let Some(comparison) = comparison {
                    comparison.matches(device.age_at(now))
                } else {
                    values.iter().any(|value| match field {
                        FilterField::Id => device.id.0.eq_ignore_ascii_case(value),
                        FilterField::Name => device.display_name.eq_ignore_ascii_case(value),
                        FilterField::Online => match value.as_str() {
                            "true" | "online" => device.liveness == Liveness::Online,
                            "false" | "offline" => device.liveness == Liveness::Offline,
                            "unknown" => device.liveness == Liveness::Unknown,
                            _ => false,
                        },
                        FilterField::Owner => device.owner_matches(value),
                        FilterField::Os => device.os.label().eq_ignore_ascii_case(value),
                        FilterField::Path => device.path.label().eq_ignore_ascii_case(value),
                        FilterField::Tag => device.tag_matches(value),
                        FilterField::Property => device.property_matches(value),
                        FilterField::Approval
                        | FilterField::KeyExpiry
                        | FilterField::ClientVersion
                        | FilterField::Sharing
                        | FilterField::Posture
                        | FilterField::RouteRole => true,
                        FilterField::LastSeen => device
                            .last_seen
                            .is_some_and(|last_seen| last_seen.to_string() == *value),
                    })
                };
                if *negated { !matched } else { matched }
            }
            Self::StructuredField {
                field,
                negated,
                value,
                mode,
            } => {
                if is_admin_filter_field(*field) {
                    return true;
                }
                let matched = structured_device_field_matches(*field, value, *mode, device);
                if *negated { !matched } else { matched }
            }
        }
    }

    fn matches_admin(&self, device: &AdminDevice, now: Timestamp) -> bool {
        let matched = match self {
            Self::Text(value) => {
                let mut fields = vec![device.stable_id.as_str(), device.display_name()];
                if let Some(hostname) = device.hostname.as_deref() {
                    fields.push(hostname);
                }
                if let Some(owner) = device.user_id.as_deref() {
                    fields.push(owner);
                }
                fields.extend(device.tags.iter().map(String::as_str));
                fields.extend(device.addresses.iter().map(String::as_str));
                fields
                    .iter()
                    .any(|field| field.to_ascii_lowercase().contains(value))
            }
            Self::Field {
                field,
                values,
                comparison,
                ..
            } => {
                if let Some(comparison) = comparison {
                    comparison.matches(
                        device
                            .last_seen
                            .map(|last_seen| now.saturating_sub(last_seen)),
                    )
                } else {
                    values
                        .iter()
                        .any(|value| admin_field_matches(*field, value, device, now))
                }
            }
            Self::StructuredField {
                field,
                negated,
                value,
                mode,
            } => {
                let matched = structured_admin_field_matches(*field, value, *mode, device, now);
                if *negated { !matched } else { matched }
            }
        };
        match self {
            Self::Field { negated, .. } if *negated => !matched,
            _ => matched,
        }
    }
}

fn admin_field_matches(
    field: FilterField,
    value: &str,
    device: &AdminDevice,
    now: Timestamp,
) -> bool {
    match field {
        FilterField::Id => device.stable_id.eq_ignore_ascii_case(value),
        FilterField::Name => device.display_name().eq_ignore_ascii_case(value),
        FilterField::Online => match value {
            "true" | "online" => device.connected_to_control == Some(true),
            "false" | "offline" => device.connected_to_control == Some(false),
            "unknown" => device.connected_to_control.is_none(),
            _ => false,
        },
        FilterField::Owner => device
            .user_id
            .as_deref()
            .is_some_and(|owner| owner.eq_ignore_ascii_case(value)),
        FilterField::Os => device
            .os
            .as_ref()
            .is_some_and(|os| os.label().eq_ignore_ascii_case(value)),
        FilterField::Path => "admin observation".eq_ignore_ascii_case(value),
        FilterField::Tag => device
            .tags
            .iter()
            .any(|tag| tag.eq_ignore_ascii_case(value)),
        FilterField::LastSeen => device
            .last_seen
            .is_some_and(|last_seen| last_seen.to_string() == value),
        FilterField::Property => match value {
            "exit-node" => has_exit_advertisement(device),
            "exit-node-option" => has_exit_approval(device),
            "subnet-router" => {
                device.advertised_routes_returned && !device.advertised_routes.is_empty()
            }
            "ssh" => device.ssh_enabled == Some(true),
            "shared" => device.is_external == Some(true),
            _ => false,
        },
        FilterField::Approval => match value {
            "true" | "approved" => device.authorized == Some(true),
            "false" | "pending" => device.authorized == Some(false),
            "unknown" => device.authorized.is_none(),
            _ => false,
        },
        FilterField::KeyExpiry => match value {
            "disabled" => device.key_expiry_disabled == Some(true),
            "expired" => device
                .expires_at
                .is_some_and(|expires_at| expires_at <= now),
            "soon" => device.expires_at.is_some_and(|expires_at| {
                expires_at > now && expires_at <= now.saturating_add(7 * 24 * 60 * 60)
            }),
            "unknown" => device.expires_at.is_none() && device.key_expiry_disabled != Some(true),
            _ => false,
        },
        FilterField::ClientVersion => device
            .client_version
            .as_deref()
            .is_some_and(|version| version.eq_ignore_ascii_case(value)),
        FilterField::Sharing => match value {
            "true" | "external" => device.is_external == Some(true),
            "false" | "internal" => device.is_external == Some(false),
            "unknown" => device.is_external.is_none(),
            _ => false,
        },
        FilterField::Posture => match value {
            "present" => device.posture_present == Some(true),
            "empty" => device.posture_present == Some(false),
            "unknown" => device.posture_present.is_none(),
            _ => false,
        },
        FilterField::RouteRole => match value {
            "subnet-router" => {
                device.advertised_routes_returned && !device.advertised_routes.is_empty()
            }
            "exit-node" => has_exit_advertisement(device),
            "exit-node-option" => has_exit_approval(device),
            "none" => device.advertised_routes_returned && device.advertised_routes.is_empty(),
            "unknown" => !device.advertised_routes_returned && !device.enabled_routes_returned,
            _ => false,
        },
    }
}

fn structured_device_field_matches(
    field: FilterField,
    value: &str,
    mode: FieldMatchMode,
    device: &Device,
) -> bool {
    match field {
        FilterField::Id => text_matches(device.id.0.as_str(), value, mode),
        FilterField::Name => {
            text_matches(device.display_name.as_str(), value, mode)
                || text_matches(device.hostname.as_str(), value, mode)
        }
        FilterField::Owner => {
            device
                .owner
                .as_deref()
                .is_some_and(|candidate| text_matches(candidate, value, mode))
                || device
                    .owner_label
                    .as_deref()
                    .is_some_and(|candidate| text_matches(candidate, value, mode))
        }
        FilterField::Os => text_matches(device.os.label(), value, mode),
        FilterField::Path => text_matches(device.path.label(), value, mode),
        FilterField::Tag => device
            .tags
            .iter()
            .any(|candidate| text_matches(candidate, value, mode)),
        FilterField::Online => text_matches(device.liveness.label(), value, mode),
        FilterField::LastSeen => device
            .last_seen
            .is_some_and(|candidate| text_matches(candidate.to_string().as_str(), value, mode)),
        FilterField::ClientVersion => text_matches(device.version.as_str(), value, mode),
        FilterField::Property
        | FilterField::Approval
        | FilterField::KeyExpiry
        | FilterField::Sharing
        | FilterField::Posture
        | FilterField::RouteRole => true,
    }
}

fn structured_admin_field_matches(
    field: FilterField,
    value: &str,
    mode: FieldMatchMode,
    device: &AdminDevice,
    now: Timestamp,
) -> bool {
    if mode == FieldMatchMode::Exact {
        return admin_field_matches(field, value, device, now);
    }
    match field {
        FilterField::Id => text_matches(device.stable_id.as_str(), value, mode),
        FilterField::Name => text_matches(device.display_name(), value, mode),
        FilterField::Owner => device
            .user_id
            .as_deref()
            .is_some_and(|candidate| text_matches(candidate, value, mode)),
        FilterField::Os => device
            .os
            .as_ref()
            .is_some_and(|candidate| text_matches(candidate.label(), value, mode)),
        FilterField::Path => text_matches("admin observation", value, mode),
        FilterField::Tag => device
            .tags
            .iter()
            .any(|candidate| text_matches(candidate, value, mode)),
        FilterField::Online => text_matches(
            match device.connected_to_control {
                Some(true) => "online",
                Some(false) => "offline",
                None => "unknown",
            },
            value,
            mode,
        ),
        FilterField::LastSeen => device
            .last_seen
            .is_some_and(|candidate| text_matches(candidate.to_string().as_str(), value, mode)),
        FilterField::ClientVersion => device
            .client_version
            .as_deref()
            .is_some_and(|candidate| text_matches(candidate, value, mode)),
        FilterField::Property => [
            "exit-node",
            "exit-node-option",
            "subnet-router",
            "ssh",
            "shared",
        ]
        .iter()
        .any(|candidate| {
            admin_field_matches(field, candidate, device, now)
                && text_matches(candidate, value, mode)
        }),
        FilterField::Approval => ["approved", "pending", "unknown"].iter().any(|candidate| {
            admin_field_matches(field, candidate, device, now)
                && text_matches(candidate, value, mode)
        }),
        FilterField::KeyExpiry => {
            ["expired", "soon", "disabled", "unknown"]
                .iter()
                .any(|candidate| {
                    admin_field_matches(field, candidate, device, now)
                        && text_matches(candidate, value, mode)
                })
        }
        FilterField::Sharing => ["external", "internal", "unknown"].iter().any(|candidate| {
            admin_field_matches(field, candidate, device, now)
                && text_matches(candidate, value, mode)
        }),
        FilterField::Posture => ["present", "empty", "unknown"].iter().any(|candidate| {
            admin_field_matches(field, candidate, device, now)
                && text_matches(candidate, value, mode)
        }),
        FilterField::RouteRole => [
            "subnet-router",
            "exit-node",
            "exit-node-option",
            "none",
            "unknown",
        ]
        .iter()
        .any(|candidate| {
            admin_field_matches(field, candidate, device, now)
                && text_matches(candidate, value, mode)
        }),
    }
}

const fn is_admin_filter_field(field: FilterField) -> bool {
    matches!(
        field,
        FilterField::Approval
            | FilterField::KeyExpiry
            | FilterField::ClientVersion
            | FilterField::Sharing
            | FilterField::Posture
            | FilterField::RouteRole
    )
}

fn text_matches(candidate: &str, value: &str, mode: FieldMatchMode) -> bool {
    let candidate = candidate.to_ascii_lowercase();
    let value = value.to_ascii_lowercase();
    match mode {
        FieldMatchMode::Exact => candidate == value,
        FieldMatchMode::Contains => candidate.contains(&value),
        FieldMatchMode::StartsWith => candidate.starts_with(&value),
    }
}

fn has_exit_advertisement(device: &AdminDevice) -> bool {
    device.advertised_routes_returned
        && device
            .advertised_routes
            .iter()
            .any(|route| route == "0.0.0.0/0" || route == "::/0")
}

fn has_exit_approval(device: &AdminDevice) -> bool {
    device.enabled_routes_returned
        && device
            .enabled_routes
            .iter()
            .any(|route| route == "0.0.0.0/0" || route == "::/0")
}

fn error(position: usize, message: &str) -> FilterError {
    FilterError {
        position,
        message: message.to_owned(),
    }
}
