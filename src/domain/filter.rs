use std::time::Duration;

use super::Timestamp;
use super::device::{Device, Liveness};

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
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FilterField {
    Online,
    Owner,
    Os,
    Path,
    Tag,
    LastSeen,
    Property,
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
        "online" => Some(FilterField::Online),
        "owner" => Some(FilterField::Owner),
        "os" => Some(FilterField::Os),
        "path" => Some(FilterField::Path),
        "tag" => Some(FilterField::Tag),
        "lastseen" => Some(FilterField::LastSeen),
        "property" => Some(FilterField::Property),
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
                let matched = if let Some(comparison) = comparison {
                    comparison.matches(device.age_at(now))
                } else {
                    values.iter().any(|value| match field {
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
                        FilterField::LastSeen => device
                            .last_seen
                            .is_some_and(|last_seen| last_seen.to_string() == *value),
                    })
                };
                if *negated { !matched } else { matched }
            }
        }
    }
}

fn error(position: usize, message: &str) -> FilterError {
    FilterError {
        position,
        message: message.to_owned(),
    }
}
