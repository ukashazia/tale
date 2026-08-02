pub fn ellipsize(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    if width <= 1 {
        return "…".chars().take(width).collect();
    }
    let mut result: String = value.chars().take(width.saturating_sub(1)).collect();
    result.push('…');
    result
}

pub fn pad_or_trim(value: &str, width: usize) -> String {
    let value = ellipsize(value, width);
    let length = value.chars().count();
    if length >= width {
        value
    } else {
        format!("{value:<width$}")
    }
}
