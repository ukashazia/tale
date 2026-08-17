use crate::domain::redaction::Redactor;

const OUTPUT_TRUNCATION_MARKER: &str = "\n...[output truncated]...\n";
const DETAIL_TRUNCATION_MARKER: &str = "\n...[detail truncated]";

pub(crate) fn bounded_ends(value: &str, cap: usize) -> String {
    if value.len() <= cap {
        return value.to_owned();
    }
    if cap <= OUTPUT_TRUNCATION_MARKER.len() {
        return OUTPUT_TRUNCATION_MARKER[..cap].to_owned();
    }
    let available = cap - OUTPUT_TRUNCATION_MARKER.len();
    let head_limit = available / 2;
    let tail_limit = available.saturating_sub(head_limit);
    let head_end = boundary_at_or_before(value, head_limit);
    let tail_start = boundary_at_or_after(value, value.len().saturating_sub(tail_limit));
    format!(
        "{}{}{}",
        &value[..head_end],
        OUTPUT_TRUNCATION_MARKER,
        &value[tail_start..]
    )
}

pub(crate) fn push_bounded_ends(detail: &mut String, value: &str, cap: usize) {
    if !detail.is_empty() {
        detail.push('\n');
    }
    detail.push_str(value);
    if detail.len() > cap.saturating_mul(2) {
        *detail = bounded_ends(detail, cap);
    }
}

pub(crate) fn bounded_prefix_bytes(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let end = boundary_at_or_before(value, limit);
    format!("{}{DETAIL_TRUNCATION_MARKER}", &value[..end])
}

pub(crate) fn bounded_prefix_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

pub(crate) fn redacted_bounded_ends(value: &str, cap: usize) -> String {
    let mut redactor = Redactor::new();
    bounded_ends(&redactor.text(value), cap)
}

fn boundary_at_or_before(value: &str, limit: usize) -> usize {
    if value.is_char_boundary(limit) {
        return limit;
    }
    value
        .char_indices()
        .take_while(|(index, _)| *index < limit)
        .map(|(index, _)| index)
        .last()
        .unwrap_or(0)
}

fn boundary_at_or_after(value: &str, limit: usize) -> usize {
    if value.is_char_boundary(limit) {
        return limit;
    }
    value
        .char_indices()
        .find(|(index, _)| *index > limit)
        .map_or(value.len(), |(index, _)| index)
}
