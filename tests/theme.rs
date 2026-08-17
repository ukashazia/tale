use tale::ui::theme::{ColorCapability, StyleRole, Theme, ThemeId};

#[test]
fn every_role_resolves_for_every_builtin_projection() {
    for id in ThemeId::ALL {
        for capability in ColorCapability::ALL {
            let theme = Theme::new(id, capability);
            assert_eq!(theme.id(), id);
            assert_eq!(theme.capability(), capability);
            for role in StyleRole::ALL {
                let _ = theme.style(role);
            }
        }
    }
}

#[test]
fn terminal_theme_leaves_neutral_backgrounds_unpainted() {
    let theme = Theme::new(ThemeId::Terminal, ColorCapability::TrueColor);
    for role in [
        StyleRole::Canvas,
        StyleRole::Surface,
        StyleRole::SurfaceRaised,
        StyleRole::SurfaceInset,
        StyleRole::Backdrop,
    ] {
        assert_eq!(theme.style(role).bg, Some(ratatui::style::Color::Reset));
    }
}

#[test]
fn terminal_theme_uses_dark_text_on_a_filled_section_heading() {
    for capability in [
        ColorCapability::TrueColor,
        ColorCapability::Ansi256,
        ColorCapability::Ansi16,
    ] {
        let theme = Theme::new(ThemeId::Terminal, capability);
        let heading = theme.style(StyleRole::SectionHeading);
        assert_eq!(heading.bg, theme.style(StyleRole::Focus).fg);
        assert_eq!(heading.fg, Some(ratatui::style::Color::Black));
        assert!(
            heading
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
    }
}

#[test]
fn operational_source_and_risk_roles_have_non_color_signals() {
    let meaningful_roles = [
        StyleRole::StateHealthy,
        StyleRole::StateInfo,
        StyleRole::StateWarning,
        StyleRole::StateDanger,
        StyleRole::StatePending,
        StyleRole::StateDisabled,
        StyleRole::StateUnknown,
        StyleRole::StateStale,
        StyleRole::StatePublic,
        StyleRole::StateDirect,
        StyleRole::StateRelay,
        StyleRole::StateOffline,
        StyleRole::SourceLocal,
        StyleRole::SourceAdmin,
        StyleRole::SourceCombined,
        StyleRole::RiskObserve,
        StyleRole::RiskReversible,
        StyleRole::RiskDisruptive,
        StyleRole::RiskDestructive,
        StyleRole::TaskQueued,
        StyleRole::TaskRunning,
        StyleRole::TaskSucceeded,
        StyleRole::TaskFailed,
        StyleRole::TaskCancelled,
        StyleRole::DiffAdded,
        StyleRole::DiffRemoved,
        StyleRole::DiffChanged,
        StyleRole::Secret,
        StyleRole::Redacted,
    ];
    for role in meaningful_roles {
        let signal = role.signal();
        assert!(!signal.unicode.is_empty());
        assert!(!signal.ascii.is_empty());
        assert!(!signal.label.is_empty());
    }
    for capability in [ColorCapability::Ansi16, ColorCapability::None] {
        let theme = Theme::new(ThemeId::Terminal, capability);
        for (index, role) in meaningful_roles.iter().enumerate() {
            for other in meaningful_roles.iter().skip(index + 1) {
                assert!(
                    theme.style(*role) != theme.style(*other) || role.signal() != other.signal(),
                    "reduced-color collision between {role:?} and {other:?}"
                );
            }
        }
    }
}

#[test]
fn offline_state_is_dimmed_without_looking_deleted() {
    for capability in ColorCapability::ALL {
        let style = Theme::new(ThemeId::Terminal, capability).style(StyleRole::StateOffline);
        assert!(style.add_modifier.contains(ratatui::style::Modifier::DIM));
        assert!(
            !style
                .add_modifier
                .contains(ratatui::style::Modifier::CROSSED_OUT)
        );
    }
}
