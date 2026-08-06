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

pub fn format_age(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3_599 => format!("{}m", seconds / 60),
        3_600..=86_399 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

/// A view's border title: what it is, how much of it is showing, and the terms
/// that narrowed it. This is where route context lives now that no separate
/// header row repeats the route name.
pub fn view_title(base: &str, shown: usize, total: usize, detail: &[String]) -> String {
    let mut title = if shown == total {
        format!(" {base} · {total} ")
    } else {
        format!(" {base} · {shown} of {total} ")
    };
    for part in detail.iter().filter(|part| !part.is_empty()) {
        title.push_str(&format!("· {part} "));
    }
    title
}

/// What to say when a view has nothing in it. An empty box is a dead end; this
/// names the reason and the next step, the way a good empty screen should.
pub fn empty_state(
    resource: &str,
    route: &str,
    admin_configured: bool,
    state: crate::admin::AdminResourceState,
    error: Option<&str>,
) -> Vec<String> {
    use crate::admin::AdminResourceState as State;
    let mut lines = vec![format!("No {resource} to show"), String::new()];
    if !admin_configured {
        lines.push(format!(
            "{resource} comes from the admin API, and no admin profile is configured."
        ));
        lines.push(String::new());
        lines.push("  add a profile    tale auth add <name>".to_owned());
        lines.push(format!("  then reopen      : {route}"));
        return lines;
    }
    if let Some(error) = error {
        lines.push(error.to_owned());
        lines.push(String::new());
        lines.push("  retry            r".to_owned());
        return lines;
    }
    lines.push(match state {
        State::Loading => format!("Loading {resource}…"),
        State::Idle => format!("Not requested yet. Press r to load {resource}."),
        State::Forbidden => {
            format!("This credential is not allowed to read {resource}.")
        }
        State::PlanRestricted => format!("This tailnet's plan does not include {resource}."),
        State::Unauthenticated => {
            "The admin credential was not accepted. Re-authenticate with tale auth.".to_owned()
        }
        State::Unsupported => format!("The server did not return {resource}."),
        State::Failed => format!("Loading {resource} failed. Press r to retry."),
        State::Ready | State::Stale => format!("This tailnet has no {resource}."),
    });
    lines
}

/// Joins the capabilities a device actually has. A device with none says so
/// rather than listing everything it is not.
pub fn capability_list(capabilities: &[(&str, bool)]) -> String {
    let enabled = capabilities
        .iter()
        .filter(|(_, enabled)| *enabled)
        .map(|(name, _)| *name)
        .collect::<Vec<_>>();
    if enabled.is_empty() {
        "None".to_owned()
    } else {
        enabled.join(" · ")
    }
}

pub fn format_bytes(value: Option<u64>) -> String {
    let Some(value) = value else {
        return "not reported".to_owned();
    };
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut amount = value as f64;
    let mut unit = 0;
    while amount >= 1000.0 && unit + 1 < UNITS.len() {
        amount /= 1000.0;
        unit = unit.saturating_add(1);
    }
    let label = UNITS.get(unit).map_or("B", |unit| *unit);
    if unit == 0 {
        format!("{value} {label}")
    } else {
        format!("{amount:.1} {label}")
    }
}

/// How current a snapshot is. This describes Tale's copy of the data and never
/// the health of a device, network, account, or tailnet.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Freshness {
    Loading,
    Current,
    Stale,
    Unavailable,
    /// Nothing is wrong; the source was simply never set up.
    Unconfigured,
}

impl Freshness {
    pub fn phrase(self, age: Option<u64>) -> String {
        match (self, age) {
            (Self::Loading, _) => "Data: loading".to_owned(),
            (Self::Current, Some(age)) => {
                format!("Data: up to date · refreshed {} ago", format_age(age))
            }
            (Self::Current, None) => "Data: up to date".to_owned(),
            (Self::Stale, Some(age)) => {
                format!("Data: stale · last updated {} ago", format_age(age))
            }
            (Self::Stale, None) => "Data: stale".to_owned(),
            (Self::Unavailable, _) => "Data unavailable · r to retry".to_owned(),
            (Self::Unconfigured, _) => "Needs an admin profile".to_owned(),
        }
    }

    pub const fn style_role(self) -> crate::ui::theme::StyleRole {
        use crate::ui::theme::StyleRole;
        match self {
            Self::Loading => StyleRole::StatePending,
            Self::Current => StyleRole::TextMuted,
            Self::Stale => StyleRole::StateStale,
            Self::Unavailable => StyleRole::StateDanger,
            Self::Unconfigured => StyleRole::TextMuted,
        }
    }
}
