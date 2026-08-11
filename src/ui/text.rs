use ratatui::text::{Line, Span};

use crate::ui::theme::{StyleRole, Theme};

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

pub fn format_timestamp(value: crate::domain::Timestamp) -> String {
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    let formatted = i64::try_from(value)
        .ok()
        .and_then(|value| OffsetDateTime::from_unix_timestamp(value).ok())
        .and_then(|value| value.format(&Rfc3339).ok());
    match formatted {
        Some(value) => value,
        None => "invalid timestamp".to_owned(),
    }
}

/// A view's border title: what it is, how much of it is showing, and the terms
/// that narrowed it. This is where route context lives now that no separate
/// header row repeats the route name.
pub fn view_title(
    theme: Theme,
    base: &str,
    shown: usize,
    total: usize,
    detail: &[String],
) -> Line<'static> {
    let primary = theme.style(StyleRole::TextPrimary);
    let accent = theme.style(StyleRole::KeyHint);
    let mut spans = vec![Span::styled(format!(" {base} · "), primary)];
    spans.push(Span::styled(
        if shown == total {
            total.to_string()
        } else {
            format!("{shown} of {total}")
        },
        if shown == total { primary } else { accent },
    ));
    spans.push(Span::styled(" ", primary));
    for part in detail.iter().filter(|part| !part.is_empty()) {
        spans.push(Span::styled(format!("· {part} "), accent));
    }
    Line::from(spans)
}

/// A border title with stable identity followed by non-default view state.
pub fn status_title(theme: Theme, base: &str, detail: &[String]) -> Line<'static> {
    let primary = theme.style(StyleRole::TextPrimary);
    let accent = theme.style(StyleRole::KeyHint);
    let mut spans = vec![Span::styled(format!(" {base} "), primary)];
    for part in detail.iter().filter(|part| !part.is_empty()) {
        spans.push(Span::styled(format!("· {part} "), accent));
    }
    Line::from(spans)
}

/// What to say when a view has nothing in it. An empty box is a dead end; this
/// names the reason and the next step, the way a good empty screen should.
pub fn empty_state(
    theme: Theme,
    resource: &str,
    route: &str,
    admin_active: bool,
    state: crate::admin::AdminResourceState,
    error: Option<&str>,
) -> Vec<Line<'static>> {
    use crate::admin::AdminResourceState as State;
    let mut lines = vec![
        muted_help(theme, format!("No {resource} to show")),
        Line::default(),
    ];
    if !admin_active {
        lines.push(muted_help(
            theme,
            format!("An active admin profile is required to show {resource}."),
        ));
        lines.push(Line::default());
        lines.push(action_hint(theme, "  choose one       ", ":profiles"));
        lines.push(action_hint(
            theme,
            "  or add one       ",
            "tale auth add <name>",
        ));
        lines.push(action_hint(
            theme,
            "  then reopen      ",
            format!(":{route}"),
        ));
        return lines;
    }
    if let Some(error) = error {
        lines.push(muted_help(theme, error));
        lines.push(Line::default());
        lines.push(action_hint(theme, "  retry            ", "r"));
        return lines;
    }
    lines.push(match state {
        State::Idle => inline_action(
            theme,
            "Not requested yet. Press ",
            "r",
            format!(" to load {resource}."),
        ),
        State::Unauthenticated => inline_action(
            theme,
            "The admin credential was not accepted. Re-authenticate with ",
            "tale auth",
            ".",
        ),
        State::Failed => inline_action(
            theme,
            format!("Loading {resource} failed. Press "),
            "r",
            " to retry.",
        ),
        State::Loading => muted_help(theme, format!("Loading {resource}…")),
        State::Forbidden => muted_help(
            theme,
            format!("This credential is not allowed to read {resource}."),
        ),
        State::PlanRestricted => muted_help(
            theme,
            format!("This tailnet's plan does not include {resource}."),
        ),
        State::Unsupported => muted_help(theme, format!("The server did not return {resource}.")),
        State::Ready | State::Stale => {
            muted_help(theme, format!("This tailnet has no {resource}."))
        }
    });
    lines
}

/// Explanatory copy with one actionable token. Commands, routes, and key hints
/// share the accent role so a reader can find the next step before reading the
/// surrounding sentence.
pub fn inline_action(
    theme: Theme,
    before: impl Into<String>,
    action: impl Into<String>,
    after: impl Into<String>,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(before.into(), theme.style(StyleRole::TextMuted)),
        Span::styled(action.into(), theme.style(StyleRole::KeyHint)),
        Span::styled(after.into(), theme.style(StyleRole::TextMuted)),
    ])
}

/// A two-column hint whose right-hand side is the action the user can take.
pub fn action_hint(
    theme: Theme,
    label: impl Into<String>,
    action: impl Into<String>,
) -> Line<'static> {
    inline_action(theme, label, action, "")
}

pub fn muted_help(theme: Theme, value: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(
        value.into(),
        theme.style(StyleRole::TextMuted),
    ))
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

/// Tags are stored with the `tag:` prefix the API requires, but the prefix is
/// the same on every row and buys the reader nothing, so it is dropped here.
pub fn tag_list(tags: &[String]) -> String {
    if tags.is_empty() {
        return "-".to_owned();
    }
    tags.iter()
        .map(|tag| tag.strip_prefix("tag:").unwrap_or(tag))
        .collect::<Vec<_>>()
        .join(", ")
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

#[cfg(test)]
mod tests {
    use super::{action_hint, empty_state, inline_action, view_title};
    use crate::admin::AdminResourceState;
    use crate::ui::theme::{ColorCapability, StyleRole, Theme, ThemeId};

    fn theme() -> Theme {
        Theme::new(ThemeId::TailscaleDark, ColorCapability::TrueColor)
    }

    #[test]
    fn inline_actions_use_the_accent_role() {
        let theme = theme();
        let line = inline_action(theme, "Open ", ":profiles", " to choose one");

        assert_eq!(
            line.spans.get(1).map(|span| span.style),
            Some(theme.style(StyleRole::KeyHint))
        );
    }

    #[test]
    fn view_title_accents_every_non_default_state() {
        let theme = theme();
        let detail = vec!["/owner:alice".to_owned(), "last seen ↓".to_owned()];
        let title = view_title(theme, "devices", 1, 13, &detail);

        assert_eq!(
            title.spans.first().map(|span| span.style),
            Some(theme.style(StyleRole::TextPrimary))
        );
        assert!(
            title
                .spans
                .iter()
                .skip(1)
                .filter(|span| !span.content.trim().is_empty())
                .all(|span| span.style == theme.style(StyleRole::KeyHint))
        );
    }

    #[test]
    fn empty_state_routes_commands_and_action_hints_use_the_accent_role() {
        let theme = theme();
        let lines = empty_state(
            theme,
            "access policy",
            "access",
            false,
            AdminResourceState::Idle,
            None,
        );
        let actions = lines
            .iter()
            .filter_map(|line| line.spans.get(1))
            .map(|span| (span.content.as_ref(), span.style))
            .collect::<Vec<_>>();

        assert_eq!(
            actions,
            vec![
                (":profiles", theme.style(StyleRole::KeyHint)),
                ("tale auth add <name>", theme.style(StyleRole::KeyHint)),
                (":access", theme.style(StyleRole::KeyHint)),
            ]
        );

        let hint = action_hint(theme, "  retry   ", "r");
        assert_eq!(
            hint.spans.get(1).map(|span| span.style),
            Some(theme.style(StyleRole::KeyHint))
        );
    }
}
