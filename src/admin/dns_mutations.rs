use std::collections::BTreeMap;
use std::net::IpAddr;

use serde_json::{Map, Value};

use crate::domain::dns::{AdminDnsPreferences, AdminNameservers, AdminSearchPaths, AdminSplitDns};

pub fn canonical_ordered_values(values: &[String], field: &str) -> Result<Vec<String>, String> {
    let mut result = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            return Err(format!("{field} cannot contain an empty value"));
        }
        if value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(format!(
                "{field} contains whitespace or control characters: {value}"
            ));
        }
        if !result.iter().any(|existing: &String| existing == value) {
            result.push(value.to_owned());
        }
    }
    Ok(result)
}

pub fn canonical_resolvers(values: &[String], field: &str) -> Result<Vec<String>, String> {
    let values = canonical_ordered_values(values, field)?;
    for value in &values {
        validate_resolver(value, field)?;
    }
    Ok(values)
}

pub fn validate_domain(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value == "." {
        return Err("DNS suffix cannot be empty".to_owned());
    }
    if value
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("DNS suffix cannot contain whitespace or control characters".to_owned());
    }
    if value.starts_with('.') || value.ends_with('.') {
        return Err("DNS suffix must not begin or end with a dot".to_owned());
    }
    for label in value.split('.') {
        if label.is_empty() || label.starts_with('-') || label.ends_with('-') {
            return Err(format!("invalid DNS suffix label: {label}"));
        }
        if !label
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            return Err(format!("invalid DNS suffix label: {label}"));
        }
    }
    Ok(value.to_ascii_lowercase())
}

pub fn split_mapping_body(domain: &str, resolvers: Option<&[String]>) -> Result<Value, String> {
    let domain = validate_domain(domain)?;
    let mut map = Map::new();
    let value = match resolvers {
        Some(resolvers) => {
            let values = canonical_resolvers(resolvers, "split-DNS resolver")?;
            let values = values
                .into_iter()
                .map(|value| {
                    validate_resolver(&value, "split-DNS resolver").map(|_| Value::String(value))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Value::Array(values)
        }
        None => Value::Null,
    };
    map.insert(domain, value);
    Ok(Value::Object(map))
}

pub fn nameservers_body(values: &[String]) -> Result<Value, String> {
    let values = canonical_resolvers(values, "nameserver")?;
    Ok(serde_json::json!({ "dns": values }))
}

fn validate_resolver(value: &str, field: &str) -> Result<(), String> {
    value
        .parse::<IpAddr>()
        .map(|_| ())
        .map_err(|_| format!("{field} must be an IP address: {value}"))
}

pub fn preferences_body(magic_dns: bool) -> Value {
    serde_json::json!({ "magicDNS": magic_dns })
}

pub fn search_paths_body(values: &[String]) -> Result<Value, String> {
    let values = canonical_ordered_values(values, "search path")?;
    for value in &values {
        validate_domain(value)?;
    }
    Ok(serde_json::json!({ "searchPaths": values }))
}

pub fn split_entries_to_map(value: &AdminSplitDns) -> BTreeMap<String, Option<Vec<String>>> {
    value
        .entries
        .iter()
        .map(|(domain, resolvers)| (domain.clone(), resolvers.clone()))
        .collect()
}

pub fn verify_nameservers(actual: &AdminNameservers, requested: &[String]) -> Result<(), String> {
    let requested = canonical_ordered_values(requested, "nameserver")?;
    if actual.values == requested {
        Ok(())
    } else {
        Err(format!(
            "server returned nameservers [{}], requested [{}]",
            actual.values.join(", "),
            requested.join(", ")
        ))
    }
}

pub fn verify_preferences(actual: &AdminDnsPreferences, requested: bool) -> Result<(), String> {
    if actual.magic_dns == Some(requested) {
        Ok(())
    } else {
        Err(format!(
            "server returned MagicDNS {}, requested {requested}",
            actual
                .magic_dns
                .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
        ))
    }
}

pub fn verify_search_paths(actual: &AdminSearchPaths, requested: &[String]) -> Result<(), String> {
    let requested = canonical_ordered_values(requested, "search path")?;
    if actual.values == requested {
        Ok(())
    } else {
        Err(format!(
            "server returned search paths [{}], requested [{}]",
            actual.values.join(", "),
            requested.join(", ")
        ))
    }
}

pub fn verify_split_mapping(
    actual: &AdminSplitDns,
    domain: &str,
    requested: Option<&[String]>,
) -> Result<(), String> {
    let domain = validate_domain(domain)?;
    let actual = actual
        .entries
        .iter()
        .find(|(entry_domain, _)| entry_domain.eq_ignore_ascii_case(&domain))
        .and_then(|(_, values)| values.as_deref());
    let requested = requested.map(|values| values.to_vec());
    if actual == requested.as_deref() {
        Ok(())
    } else {
        Err(format!(
            "server returned split-DNS mapping for {domain} as {}, requested {}",
            format_resolvers(actual),
            format_resolvers(requested.as_deref())
        ))
    }
}

fn format_resolvers(values: Option<&[String]>) -> String {
    values.map_or_else(|| "removed".to_owned(), |values| values.join(", "))
}
