use std::time::Duration;

use super::Timestamp;
use super::device::{AdminDevice, Device, Liveness};
use super::service::ServiceMapping;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FilterOperator {
    Match,
    StartsWith,
    LessThan,
    LessOrEqual,
    GreaterThan,
    GreaterOrEqual,
}

impl FilterOperator {
    /// Text typed after the field separator to select this operator.
    pub const fn syntax(self) -> &'static str {
        match self {
            Self::Match => "",
            Self::StartsWith => "starts_with=",
            Self::LessThan => "<",
            Self::LessOrEqual => "<=",
            Self::GreaterThan => ">",
            Self::GreaterOrEqual => ">=",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Match => "substring",
            Self::StartsWith => "prefix",
            Self::LessThan => "newer than",
            Self::LessOrEqual => "newer or equal",
            Self::GreaterThan => "older than",
            Self::GreaterOrEqual => "older or equal",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FilterValueKind {
    /// Values are fixed and every one of them is offered.
    Enumeration(&'static [&'static str]),
    /// Values come from the rows currently held in the snapshot.
    Snapshot,
    /// Values are durations such as `30m` or `7d`.
    Duration,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct FilterFieldSpec {
    pub name: &'static str,
    pub field: FilterField,
    pub operators: &'static [FilterOperator],
    pub value_kind: FilterValueKind,
    pub description: &'static str,
}

impl FilterFieldSpec {
    pub const fn enumeration(&self) -> &'static [&'static str] {
        match self.value_kind {
            FilterValueKind::Enumeration(values) => values,
            FilterValueKind::Snapshot | FilterValueKind::Duration => &[],
        }
    }

    /// The syntax an operand must follow, used by error reporting.
    pub fn expected_syntax(&self) -> String {
        match self.value_kind {
            FilterValueKind::Enumeration(values) => format!("{}:{}", self.name, values.join("|")),
            FilterValueKind::Snapshot => format!("{}:<text>", self.name),
            FilterValueKind::Duration => format!("{}:<7d", self.name),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct FilterFieldGroup {
    pub label: &'static str,
    pub fields: &'static [FilterFieldSpec],
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct FilterSchema {
    /// What a bare word without a field matches on this route.
    pub free_text: &'static str,
    pub groups: &'static [FilterFieldGroup],
}

impl FilterSchema {
    pub fn fields(&self) -> impl Iterator<Item = &'static FilterFieldSpec> {
        self.groups.iter().flat_map(|group| group.fields.iter())
    }

    pub fn field(&self, name: &str) -> Option<&'static FilterFieldSpec> {
        self.fields()
            .find(|spec| spec.name.eq_ignore_ascii_case(name))
    }

    pub fn is_empty(&self) -> bool {
        self.fields().next().is_none()
    }
}

const TEXT_OPERATORS: &[FilterOperator] = &[FilterOperator::Match, FilterOperator::StartsWith];
const ENUM_OPERATORS: &[FilterOperator] = &[FilterOperator::Match];
const AGE_OPERATORS: &[FilterOperator] = &[
    FilterOperator::LessThan,
    FilterOperator::LessOrEqual,
    FilterOperator::GreaterThan,
    FilterOperator::GreaterOrEqual,
];

const ONLINE_VALUES: &[&str] = &["true", "false", "unknown"];
const PATH_VALUES: &[&str] = &["direct", "derp", "peer-relay", "idle", "no-path", "unknown"];
const PROPERTY_VALUES: &[&str] = &[
    "exit-node",
    "exit-node-option",
    "subnet-router",
    "ssh",
    "shared",
];
const APPROVAL_VALUES: &[&str] = &["approved", "pending", "unknown"];
const KEY_EXPIRY_VALUES: &[&str] = &["expired", "soon", "disabled", "unknown"];
const SHARING_VALUES: &[&str] = &["external", "internal", "unknown"];
const POSTURE_VALUES: &[&str] = &["present", "empty", "unknown"];
const ROUTE_ROLE_VALUES: &[&str] = &[
    "subnet-router",
    "exit-node",
    "exit-node-option",
    "none",
    "unknown",
];

const DEVICE_MACHINE_FIELDS: &[FilterFieldSpec] = &[
    spec(
        "id",
        FilterField::Id,
        TEXT_OPERATORS,
        FilterValueKind::Snapshot,
        "device id",
    ),
    spec(
        "name",
        FilterField::Name,
        TEXT_OPERATORS,
        FilterValueKind::Snapshot,
        "host name",
    ),
    spec(
        "owner",
        FilterField::Owner,
        TEXT_OPERATORS,
        FilterValueKind::Snapshot,
        "owning user",
    ),
    spec(
        "tag",
        FilterField::Tag,
        TEXT_OPERATORS,
        FilterValueKind::Snapshot,
        "acl tag",
    ),
    spec(
        "os",
        FilterField::Os,
        TEXT_OPERATORS,
        FilterValueKind::Snapshot,
        "platform",
    ),
];

const DEVICE_CONNECTION_FIELDS: &[FilterFieldSpec] = &[
    spec(
        "online",
        FilterField::Online,
        ENUM_OPERATORS,
        FilterValueKind::Enumeration(ONLINE_VALUES),
        "control link",
    ),
    spec(
        "path",
        FilterField::Path,
        ENUM_OPERATORS,
        FilterValueKind::Enumeration(PATH_VALUES),
        "data path",
    ),
    spec(
        "last-seen",
        FilterField::LastSeen,
        AGE_OPERATORS,
        FilterValueKind::Duration,
        "seen age",
    ),
    spec(
        "property",
        FilterField::Property,
        ENUM_OPERATORS,
        FilterValueKind::Enumeration(PROPERTY_VALUES),
        "capability",
    ),
];

const DEVICE_ADMIN_FIELDS: &[FilterFieldSpec] = &[
    spec(
        "approval",
        FilterField::Approval,
        ENUM_OPERATORS,
        FilterValueKind::Enumeration(APPROVAL_VALUES),
        "approval",
    ),
    spec(
        "key-expiry",
        FilterField::KeyExpiry,
        ENUM_OPERATORS,
        FilterValueKind::Enumeration(KEY_EXPIRY_VALUES),
        "key expiry",
    ),
    spec(
        "version",
        FilterField::ClientVersion,
        TEXT_OPERATORS,
        FilterValueKind::Snapshot,
        "client build",
    ),
    spec(
        "sharing",
        FilterField::Sharing,
        ENUM_OPERATORS,
        FilterValueKind::Enumeration(SHARING_VALUES),
        "membership",
    ),
    spec(
        "posture",
        FilterField::Posture,
        ENUM_OPERATORS,
        FilterValueKind::Enumeration(POSTURE_VALUES),
        "posture data",
    ),
    spec(
        "route-role",
        FilterField::RouteRole,
        ENUM_OPERATORS,
        FilterValueKind::Enumeration(ROUTE_ROLE_VALUES),
        "route role",
    ),
];

const DEVICE_FILTER_GROUPS: &[FilterFieldGroup] = &[
    FilterFieldGroup {
        label: "Machine",
        fields: DEVICE_MACHINE_FIELDS,
    },
    FilterFieldGroup {
        label: "Connection",
        fields: DEVICE_CONNECTION_FIELDS,
    },
    FilterFieldGroup {
        label: "Administration",
        fields: DEVICE_ADMIN_FIELDS,
    },
];

const fn spec(
    name: &'static str,
    field: FilterField,
    operators: &'static [FilterOperator],
    value_kind: FilterValueKind,
    description: &'static str,
) -> FilterFieldSpec {
    FilterFieldSpec {
        name,
        field,
        operators,
        value_kind,
        description,
    }
}

pub const fn device_schema() -> FilterSchema {
    FilterSchema {
        free_text: "matches name, host, owner, tags, and addresses",
        groups: DEVICE_FILTER_GROUPS,
    }
}

pub const fn tasks_schema() -> FilterSchema {
    FilterSchema {
        free_text: "matches action, target, state, and summary",
        groups: &[],
    }
}

const EXPOSURE_VALUES: &[&str] = &["tailnet", "public"];
const LISTENER_VALUES: &[&str] = &["https", "http", "tcp", "tls-terminated-tcp"];

const SERVICE_EXPOSURE_FIELDS: &[FilterFieldSpec] = &[
    spec(
        "exposure",
        FilterField::Exposure,
        ENUM_OPERATORS,
        FilterValueKind::Enumeration(EXPOSURE_VALUES),
        "who can reach it",
    ),
    spec(
        "listener",
        FilterField::Listener,
        ENUM_OPERATORS,
        FilterValueKind::Enumeration(LISTENER_VALUES),
        "protocol",
    ),
    spec(
        "port",
        FilterField::Port,
        TEXT_OPERATORS,
        FilterValueKind::Snapshot,
        "listening port",
    ),
];

const SERVICE_TARGET_FIELDS: &[FilterFieldSpec] = &[
    spec(
        "path",
        FilterField::Mount,
        TEXT_OPERATORS,
        FilterValueKind::Snapshot,
        "mount path",
    ),
    spec(
        "backend",
        FilterField::Backend,
        TEXT_OPERATORS,
        FilterValueKind::Snapshot,
        "what it proxies to",
    ),
];

const SERVICE_FILTER_GROUPS: &[FilterFieldGroup] = &[
    FilterFieldGroup {
        label: "Exposure",
        fields: SERVICE_EXPOSURE_FIELDS,
    },
    FilterFieldGroup {
        label: "Target",
        fields: SERVICE_TARGET_FIELDS,
    },
];

pub const fn service_schema() -> FilterSchema {
    FilterSchema {
        free_text: "matches listener, path, and backend",
        groups: SERVICE_FILTER_GROUPS,
    }
}

/// `:profiles` is a handful of rows describing local configuration. A field
/// grammar would be more machinery than it has rows, so the whole row is the
/// haystack, the way `:tasks` works.
pub const fn profiles_schema() -> FilterSchema {
    FilterSchema {
        free_text: "matches profile, tailnet, state, credential, and backend",
        groups: &[],
    }
}

pub const fn config_schema() -> FilterSchema {
    FilterSchema {
        free_text: "matches setting, value, and source",
        groups: &[],
    }
}

pub const fn collection_schema() -> FilterSchema {
    FilterSchema {
        free_text: "matches text in every visible column",
        groups: &[],
    }
}

pub const fn empty_schema() -> FilterSchema {
    FilterSchema {
        free_text: "",
        groups: &[],
    }
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

    /// Whether any term reads the clock, which is true for an age comparison
    /// and for key expiry. Terms that do not are stable between ticks, so a
    /// result derived from them can be cached without a timestamp.
    pub fn requires_now(&self) -> bool {
        self.terms.iter().any(|term| match term {
            FilterTerm::Text(_) => false,
            FilterTerm::Field {
                field, comparison, ..
            } => comparison.is_some() || *field == FilterField::KeyExpiry,
            FilterTerm::StructuredField { field, .. } => *field == FilterField::KeyExpiry,
        })
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

    pub fn matches_mapping(&self, mapping: &ServiceMapping) -> bool {
        self.terms.iter().all(|term| term.matches_mapping(mapping))
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
    Exposure,
    Listener,
    Port,
    Mount,
    Backend,
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
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilterError {
    pub position: usize,
    pub message: String,
    /// The syntax the term should have followed.
    pub expected: String,
}

impl std::fmt::Display for FilterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} at column {}",
            self.message,
            self.position.saturating_add(1)
        )
    }
}

impl std::error::Error for FilterError {}

pub fn parse(input: &str, schema: &FilterSchema) -> Result<FilterExpression, FilterError> {
    let tokens = tokenize(input)?;
    let mut terms = Vec::with_capacity(tokens.len());
    for token in tokens {
        terms.push(parse_term(&token, schema)?);
    }
    Ok(FilterExpression { terms })
}

/// Byte spans of every whitespace-separated token, honouring quoted sections.
pub fn token_spans(input: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;
    let mut quoted = false;
    let mut escaped = false;
    for (position, character) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quoted {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
            if start.is_none() {
                start = Some(position);
            }
            continue;
        }
        if character.is_whitespace() && !quoted {
            if let Some(begin) = start.take() {
                spans.push((begin, position));
            }
        } else if start.is_none() {
            start = Some(position);
        }
    }
    if let Some(begin) = start {
        spans.push((begin, input.len()));
    }
    spans
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
                let token_start = start.unwrap_or(position);
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
            message: "unclosed quote".to_owned(),
            expected: "close the value with a matching \"".to_owned(),
        });
    }
    if !current.is_empty() {
        tokens.push((current, start.unwrap_or(0)));
    }
    Ok(tokens)
}

fn parse_term(
    (token, position): &(String, usize),
    schema: &FilterSchema,
) -> Result<FilterTerm, FilterError> {
    let (negated, body) = token
        .strip_prefix('!')
        .map_or((false, token.as_str()), |value| (true, value));
    if body.is_empty() {
        return Err(error(*position, "empty term", &field_list_hint(schema)));
    }

    let Some(colon) = body.find(':') else {
        if negated {
            return Err(error(
                *position,
                "negation needs a field",
                "!field:value, for example !tag:server",
            ));
        }
        return Ok(FilterTerm::Text(normalize_value(body, *position)?));
    };
    let field_text = &body[..colon];
    let value_text = &body[colon.saturating_add(1)..];
    let value_position = position.saturating_add(colon).saturating_add(1);
    let Some(spec) = schema.field(field_text) else {
        return Err(error(
            *position,
            &format!("unknown field {field_text}"),
            &field_list_hint(schema),
        ));
    };
    if value_text.is_empty() {
        return Err(error(
            value_position,
            &format!("{} needs a value", spec.name),
            &spec.expected_syntax(),
        ));
    }

    if let Some((operator, duration_text)) = comparison_parts(value_text) {
        if !spec.operators.contains(&operator) {
            return Err(error(
                value_position,
                &format!("{} does not compare", spec.name),
                &spec.expected_syntax(),
            ));
        }
        let duration = parse_filter_duration(duration_text).ok_or_else(|| {
            error(
                value_position,
                &format!("{duration_text} is not a duration"),
                "a count and one of s, m, h, d, w, for example 7d",
            )
        })?;
        return Ok(FilterTerm::Field {
            field: spec.field,
            negated,
            values: Vec::new(),
            comparison: Some(Comparison::from_operator(operator, duration)),
        });
    }
    if spec.value_kind == FilterValueKind::Duration {
        return Err(error(
            value_position,
            &format!("{} needs a comparison", spec.name),
            &spec.expected_syntax(),
        ));
    }

    if let Some((prefix, structured_value)) = value_text.split_once('=') {
        // A bare `field:value` already matches on substring, so `starts_with=`
        // is the only refinement that says something the default does not.
        if prefix != "starts_with" {
            return Err(error(
                value_position,
                &format!("unknown operator {prefix}="),
                &format!("{0}:value or {0}:starts_with=value", spec.name),
            ));
        }
        let mode = FieldMatchMode::StartsWith;
        if !spec.operators.contains(&FilterOperator::StartsWith) {
            return Err(error(
                value_position,
                &format!("{} has fixed values", spec.name),
                &spec.expected_syntax(),
            ));
        }
        if structured_value.is_empty() {
            return Err(error(
                value_position,
                &format!("{} needs a value", spec.name),
                &spec.expected_syntax(),
            ));
        }
        return Ok(FilterTerm::StructuredField {
            field: spec.field,
            negated,
            value: normalize_value(structured_value, value_position)?,
            mode,
        });
    }

    let values = split_values(value_text, value_position, spec)?;
    for value in &values {
        let allowed = spec.enumeration();
        if !allowed.is_empty() && !allowed.contains(&value.as_str()) {
            return Err(error(
                value_position,
                &format!("{value} is not a value of {}", spec.name),
                &spec.expected_syntax(),
            ));
        }
    }
    Ok(FilterTerm::Field {
        field: spec.field,
        negated,
        values,
        comparison: None,
    })
}

fn field_list_hint(schema: &FilterSchema) -> String {
    let names = schema
        .fields()
        .map(|spec| spec.name)
        .collect::<Vec<_>>()
        .join(", ");
    if names.is_empty() {
        "this view filters on free text only".to_owned()
    } else {
        format!("one of {names}")
    }
}

fn split_values(
    value: &str,
    position: usize,
    spec: &FilterFieldSpec,
) -> Result<Vec<String>, FilterError> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in value.chars() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                if current.trim().is_empty() {
                    return Err(error(
                        position,
                        "empty value between commas",
                        &spec.expected_syntax(),
                    ));
                }
                values.push(normalize_value(current.trim(), position)?);
                current.clear();
            }
            other => current.push(other),
        }
    }
    if quoted {
        return Err(error(
            position,
            "unclosed quote",
            "close the value with a matching \"",
        ));
    }
    if current.trim().is_empty() {
        return Err(error(
            position,
            "empty value between commas",
            &spec.expected_syntax(),
        ));
    }
    values.push(normalize_value(current.trim(), position)?);
    Ok(values)
}

fn normalize_value(value: &str, position: usize) -> Result<String, FilterError> {
    if value.starts_with('"') || value.ends_with('"') {
        if value.chars().count() < 2 || !value.starts_with('"') || !value.ends_with('"') {
            return Err(error(
                position,
                "unclosed quote",
                "close the value with a matching \"",
            ));
        }
        return Ok(value
            .get(1..value.len().saturating_sub(1))
            .map_or_else(String::new, str::to_lowercase));
    }
    Ok(value.to_lowercase())
}

fn comparison_parts(value: &str) -> Option<(FilterOperator, &str)> {
    for (syntax, operator) in [
        ("<=", FilterOperator::LessOrEqual),
        (">=", FilterOperator::GreaterOrEqual),
        ("<", FilterOperator::LessThan),
        (">", FilterOperator::GreaterThan),
    ] {
        if let Some(duration) = value.strip_prefix(syntax) {
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
    Some(Duration::from_secs(amount.checked_mul(multiplier)?))
}

impl Comparison {
    const fn from_operator(operator: FilterOperator, duration: Duration) -> Self {
        match operator {
            FilterOperator::LessOrEqual => Self::LessOrEqual(duration),
            FilterOperator::GreaterThan => Self::Greater(duration),
            FilterOperator::GreaterOrEqual => Self::GreaterOrEqual(duration),
            _ => Self::Less(duration),
        }
    }

    fn matches(self, age: Option<u64>) -> bool {
        let Some(seconds) = age else {
            return false;
        };
        match self {
            Self::Less(value) => seconds < value.as_secs(),
            Self::LessOrEqual(value) => seconds <= value.as_secs(),
            Self::Greater(value) => seconds > value.as_secs(),
            Self::GreaterOrEqual(value) => seconds >= value.as_secs(),
        }
    }
}

/// Closed vocabularies compare exactly; free-text ones take a substring.
fn mapping_field_matches(field: FilterField, value: &str, mapping: &ServiceMapping) -> bool {
    match field {
        FilterField::Exposure => mapping.exposure.label().eq_ignore_ascii_case(value),
        FilterField::Listener => mapping.listener.label().eq_ignore_ascii_case(value),
        FilterField::Port => contains_matches(&mapping.listener.port().to_string(), value),
        FilterField::Mount => contains_matches(mapping.mount.as_path(), value),
        FilterField::Backend => contains_matches(&mapping.backend.argument(), value),
        _ => false,
    }
}

impl FilterTerm {
    fn matches(&self, device: &Device, dns_name: Option<&str>, now: Timestamp) -> bool {
        match self {
            Self::Text(value) => device
                .search_fields()
                .into_iter()
                .chain(dns_name)
                .any(|field| contains_matches(field, value)),
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
                        // Free-text fields take a substring; closed vocabularies
                        // stay exact, the parser having pinned them to a value.
                        FilterField::Id => contains_matches(device.id.0.as_str(), value),
                        FilterField::Name => {
                            contains_matches(&device.display_name, value)
                                || contains_matches(&device.hostname, value)
                        }
                        FilterField::Online => match value.as_str() {
                            "true" => device.liveness == Liveness::Online,
                            "false" => device.liveness == Liveness::Offline,
                            "unknown" => device.liveness == Liveness::Unknown,
                            _ => false,
                        },
                        FilterField::Owner => {
                            device
                                .owner
                                .as_deref()
                                .is_some_and(|owner| contains_matches(owner, value))
                                || device
                                    .owner_label
                                    .as_deref()
                                    .is_some_and(|owner| contains_matches(owner, value))
                        }
                        FilterField::Os => contains_matches(device.os.label(), value),
                        FilterField::Path => device.path.label().eq_ignore_ascii_case(value),
                        FilterField::Tag => {
                            device.tags.iter().any(|tag| contains_matches(tag, value))
                        }
                        FilterField::Property => device.property_matches(value),
                        FilterField::Approval
                        | FilterField::KeyExpiry
                        | FilterField::ClientVersion
                        | FilterField::Sharing
                        | FilterField::Posture
                        | FilterField::RouteRole
                        | FilterField::LastSeen => true,
                        // Mapping fields describe a Serve entry, never a device.
                        FilterField::Exposure
                        | FilterField::Listener
                        | FilterField::Port
                        | FilterField::Mount
                        | FilterField::Backend => false,
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

    fn matches_mapping(&self, mapping: &ServiceMapping) -> bool {
        let port = mapping.listener.port().to_string();
        let backend = mapping.backend.argument();
        match self {
            Self::Text(value) => [
                mapping.listener.label(),
                mapping.mount.as_path(),
                backend.as_str(),
                port.as_str(),
            ]
            .into_iter()
            .any(|field| contains_matches(field, value)),
            Self::Field {
                field,
                negated,
                values,
                comparison,
            } => {
                // Mappings carry no age, so a comparison can never hold.
                let matched = comparison.is_none()
                    && values
                        .iter()
                        .any(|value| mapping_field_matches(*field, value, mapping));
                if *negated { !matched } else { matched }
            }
            Self::StructuredField {
                field,
                negated,
                value,
                mode,
            } => {
                let candidate = match field {
                    FilterField::Exposure => mapping.exposure.label(),
                    FilterField::Listener => mapping.listener.label(),
                    FilterField::Port => port.as_str(),
                    FilterField::Mount => mapping.mount.as_path(),
                    FilterField::Backend => backend.as_str(),
                    _ => return !*negated,
                };
                let matched = text_matches(candidate, value, *mode);
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
                fields.iter().any(|field| contains_matches(field, value))
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
        FilterField::Exposure
        | FilterField::Listener
        | FilterField::Port
        | FilterField::Mount
        | FilterField::Backend => false,
        FilterField::Id => contains_matches(&device.stable_id, value),
        FilterField::Name => {
            contains_matches(device.display_name(), value)
                || device
                    .hostname
                    .as_deref()
                    .is_some_and(|hostname| contains_matches(hostname, value))
        }
        FilterField::Online => match value {
            "true" => device.connected_to_control == Some(true),
            "false" => device.connected_to_control == Some(false),
            "unknown" => device.connected_to_control.is_none(),
            _ => false,
        },
        FilterField::Owner => device
            .user_id
            .as_deref()
            .is_some_and(|owner| contains_matches(owner, value)),
        FilterField::Os => device
            .os
            .as_ref()
            .is_some_and(|os| contains_matches(os.label(), value)),
        FilterField::Path => "admin observation".eq_ignore_ascii_case(value),
        FilterField::Tag => device.tags.iter().any(|tag| contains_matches(tag, value)),
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
            "approved" => device.authorized == Some(true),
            "pending" => device.authorized == Some(false),
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
            .is_some_and(|version| contains_matches(version, value)),
        FilterField::Sharing => match value {
            "external" => device.is_external == Some(true),
            "internal" => device.is_external == Some(false),
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
        FilterField::ClientVersion => device
            .version
            .as_deref()
            .is_some_and(|candidate| text_matches(candidate, value, mode)),
        FilterField::Property
        | FilterField::Approval
        | FilterField::KeyExpiry
        | FilterField::Sharing
        | FilterField::Posture
        | FilterField::RouteRole => true,
        FilterField::Exposure
        | FilterField::Listener
        | FilterField::Port
        | FilterField::Mount
        | FilterField::Backend => false,
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
        FilterField::Property => PROPERTY_VALUES.iter().any(|candidate| {
            admin_field_matches(field, candidate, device, now)
                && text_matches(candidate, value, mode)
        }),
        FilterField::Approval => APPROVAL_VALUES.iter().any(|candidate| {
            admin_field_matches(field, candidate, device, now)
                && text_matches(candidate, value, mode)
        }),
        FilterField::KeyExpiry => KEY_EXPIRY_VALUES.iter().any(|candidate| {
            admin_field_matches(field, candidate, device, now)
                && text_matches(candidate, value, mode)
        }),
        FilterField::Sharing => SHARING_VALUES.iter().any(|candidate| {
            admin_field_matches(field, candidate, device, now)
                && text_matches(candidate, value, mode)
        }),
        FilterField::Posture => POSTURE_VALUES.iter().any(|candidate| {
            admin_field_matches(field, candidate, device, now)
                && text_matches(candidate, value, mode)
        }),
        FilterField::RouteRole => ROUTE_ROLE_VALUES.iter().any(|candidate| {
            admin_field_matches(field, candidate, device, now)
                && text_matches(candidate, value, mode)
        }),
        FilterField::Exposure
        | FilterField::Listener
        | FilterField::Port
        | FilterField::Mount
        | FilterField::Backend => false,
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
    match mode {
        FieldMatchMode::Exact => {
            candidate.chars().count() == value.chars().count()
                && starts_with_matches(candidate, value)
        }
        FieldMatchMode::Contains => contains_matches(candidate, value),
        FieldMatchMode::StartsWith => starts_with_matches(candidate, value),
    }
}

/// Case-insensitive substring test. A named field takes this rule: `name:build`
/// finds `build-01` without demanding the whole value, but the term still has to
/// appear as written, so `os:ios` cannot reach `windows`.
pub fn contains_matches(candidate: &str, value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    candidate.char_indices().any(|(index, _)| {
        candidate
            .get(index..)
            .is_some_and(|tail| starts_with_matches(tail, value))
    })
}

fn starts_with_matches(candidate: &str, value: &str) -> bool {
    let mut characters = candidate.chars();
    value.chars().all(|wanted| {
        characters
            .next()
            .is_some_and(|character| characters_match(character, wanted))
    })
}

/// Case-insensitive subsequence test used to rank completion and navigation
/// candidates. Collection filters deliberately use `contains_matches` so a
/// returned row always contains the query in one of its searchable values.
pub fn fuzzy_matches(candidate: &str, value: &str) -> bool {
    let mut characters = candidate.chars();
    value
        .chars()
        .all(|wanted| characters.any(|character| characters_match(character, wanted)))
}

fn characters_match(character: char, wanted: char) -> bool {
    character.eq_ignore_ascii_case(&wanted)
        || (!character.is_ascii() && character.to_lowercase().eq(wanted.to_lowercase()))
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

fn error(position: usize, message: &str, expected: &str) -> FilterError {
    FilterError {
        position,
        message: message.to_owned(),
        expected: expected.to_owned(),
    }
}
