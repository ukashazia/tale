use std::fmt;

use serde_json::Value;
use similar::{ChangeTag, TextDiff};

use crate::domain::Timestamp;
use crate::domain::policy_workflow::{
    PolicyDiff, PolicyDocument, PolicyPreview, PolicyPreviewMatch, PolicySelectorType,
    PolicyValidation, ServerPolicyTest, ValidationDiagnostic, hash_bytes,
};

use super::dto::{PolicyPreviewDto, PolicyValidationDto};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PolicyTextError {
    InvalidUtf8,
}

impl fmt::Display for PolicyTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the policy bytes are not valid UTF-8 text")
    }
}

impl std::error::Error for PolicyTextError {}

pub fn decode_validation(
    response: PolicyValidationDto,
    candidate: &PolicyDocument,
    observed_at: Timestamp,
) -> PolicyValidation {
    let message = response.message.map(|value| bounded_safe_text(&value));
    let mut diagnostics = Vec::new();
    let mut server_tests = Vec::new();
    for item in response.data.unwrap_or_default() {
        let errors = item.errors.map_or_else(Vec::new, |value| {
            value
                .into_iter()
                .take(64)
                .map(|value| bounded_safe_text(&value))
                .collect()
        });
        let warnings = item.warnings.map_or_else(Vec::new, |value| {
            value
                .into_iter()
                .take(64)
                .map(|value| bounded_safe_text(&value))
                .collect()
        });
        let item_message = item
            .message
            .map_or_else(String::new, |value| bounded_safe_text(&value));
        if item.name.is_some() || item.passed.is_some() {
            server_tests.push(ServerPolicyTest {
                name: item.name.map_or_else(
                    || "server test".to_owned(),
                    |value| bounded_safe_text(&value),
                ),
                passed: item.passed.is_some_and(|value| value),
                message: (!item_message.is_empty()).then_some(item_message.clone()),
            });
        }
        if item.user.is_some()
            || item.severity.is_some()
            || item.line.is_some()
            || item.column.is_some()
            || item.range.is_some()
            || item.source.is_some()
            || item.destination.is_some()
            || item.expected.is_some()
            || item.actual.is_some()
            || item.test_id.is_some()
            || !errors.is_empty()
            || !warnings.is_empty()
        {
            diagnostics.push(ValidationDiagnostic {
                message: item_message,
                user: item.user.map(|value| bounded_safe_text(&value)),
                severity: item.severity.map(|value| bounded_safe_text(&value)),
                line: item.line,
                column: item.column,
                range: safe_value(item.range),
                source: item.source.map(|value| bounded_safe_text(&value)),
                destination: item.destination.map(|value| bounded_safe_text(&value)),
                expected: safe_value(item.expected),
                actual: safe_value(item.actual),
                test_id: item.test_id.map(|value| bounded_safe_text(&value)),
                errors,
                warnings,
            });
        }
    }
    let valid = message.is_none()
        && diagnostics.iter().all(|item| item.errors.is_empty())
        && server_tests.iter().all(|item| item.passed);
    PolicyValidation {
        candidate_hash: candidate.hash().to_owned(),
        validated_at: observed_at,
        valid,
        bounded_safe_detail: message.clone(),
        message,
        diagnostics,
        server_tests,
        observed_at,
    }
}

fn bounded_safe_text(value: &str) -> String {
    crate::admin::client::redact_text(value)
        .chars()
        .take(1024)
        .collect()
}

fn safe_value(value: Option<Value>) -> Option<String> {
    let value = value?;
    let redacted = crate::admin::dto::redact_json_value(&value);
    let text = serde_json::to_string(&redacted).ok()?;
    Some(text.chars().take(512).collect())
}

pub fn decode_preview(
    response: PolicyPreviewDto,
    candidate: &PolicyDocument,
    selector_type: PolicySelectorType,
    selector: &str,
    observed_at: Timestamp,
) -> PolicyPreview {
    let matches = response.matches.map_or_else(Vec::new, |items| {
        items
            .into_iter()
            .map(|item| PolicyPreviewMatch {
                users: item.users.map_or_else(Vec::new, |value| {
                    value
                        .into_iter()
                        .take(128)
                        .map(|value| bounded_safe_text(&value))
                        .collect()
                }),
                ports: item.ports.map_or_else(Vec::new, |value| {
                    value
                        .into_iter()
                        .take(128)
                        .map(|value| bounded_safe_text(&value))
                        .collect()
                }),
                line_number: item.line_number,
            })
            .collect()
    });
    PolicyPreview {
        candidate_hash: candidate.hash().to_owned(),
        selector_type,
        selector: selector.to_owned(),
        matches,
        observed_at,
    }
}

pub fn decode_preview_checked(
    response: PolicyPreviewDto,
    candidate: &PolicyDocument,
    selector_type: PolicySelectorType,
    selector: &str,
    observed_at: Timestamp,
) -> Result<PolicyPreview, String> {
    if let Some(returned_type) = response.selector_type.as_deref()
        && returned_type != selector_type.api_value()
    {
        return Err("server permission preview returned a different selector type".to_owned());
    }
    if let Some(returned_selector) = response.preview_for.as_deref()
        && returned_selector != selector
    {
        return Err("server permission preview returned a different selector".to_owned());
    }
    Ok(decode_preview(
        response,
        candidate,
        selector_type,
        selector,
        observed_at,
    ))
}

pub fn build_policy_diff(
    base: &PolicyDocument,
    candidate: &PolicyDocument,
) -> Result<PolicyDiff, PolicyTextError> {
    let base_text = std::str::from_utf8(base.bytes()).map_err(|_| PolicyTextError::InvalidUtf8)?;
    let candidate_text =
        std::str::from_utf8(candidate.bytes()).map_err(|_| PolicyTextError::InvalidUtf8)?;
    let diff = TextDiff::from_lines(base_text, candidate_text);
    let mut additions = 0usize;
    let mut removals = 0usize;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => additions = additions.saturating_add(1),
            ChangeTag::Delete => removals = removals.saturating_add(1),
            ChangeTag::Equal => {}
        }
    }
    let text = diff
        .unified_diff()
        .header("remote/base", "candidate")
        .to_string();
    Ok(PolicyDiff {
        base_hash: hash_bytes(base.bytes()),
        candidate_hash: hash_bytes(candidate.bytes()),
        base_observed_at: base.observed_at(),
        candidate_observed_at: candidate.observed_at(),
        text,
        additions,
        removals,
    })
}

pub fn validation_has_errors(validation: &PolicyValidation) -> bool {
    validation
        .diagnostics
        .iter()
        .any(|diagnostic| !diagnostic.errors.is_empty())
        || validation.server_tests.iter().any(|test| !test.passed)
        || !validation.valid
}

#[cfg(test)]
mod tests {
    use super::build_policy_diff;
    use crate::domain::policy_workflow::PolicyDocument;

    #[test]
    fn diff_is_textual_and_does_not_rewrite_source_bytes() {
        let base = PolicyDocument::from_bytes(b"// keep\r\nold\n".to_vec(), 1)
            .map_err(|_| ())
            .ok();
        let candidate = PolicyDocument::from_bytes(b"// keep\r\nnew\n".to_vec(), 1)
            .map_err(|_| ())
            .ok();
        assert!(base.is_some() && candidate.is_some());
        let result = base
            .as_ref()
            .zip(candidate.as_ref())
            .and_then(|(base, candidate)| build_policy_diff(base, candidate).ok());
        assert!(result.is_some());
        assert_eq!(
            base.as_ref().map(|value| value.bytes()),
            Some(b"// keep\r\nold\n".as_slice())
        );
        assert_eq!(
            candidate.as_ref().map(|value| value.bytes()),
            Some(b"// keep\r\nnew\n".as_slice())
        );
        assert_eq!(
            result.map(|value| (value.additions, value.removals)),
            Some((1, 1))
        );
    }
}
