use std::fs;
use std::path::{Path, PathBuf};

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

fn rust_files(root: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
}

#[test]
fn production_colors_are_owned_only_by_the_theme_module() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let theme_root = root.join("ui/theme");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    for path in files {
        if path.starts_with(&theme_root) {
            continue;
        }
        let contents = fs::read_to_string(&path);
        assert!(contents.is_ok(), "could not inspect {}", path.display());
        if let Ok(contents) = contents {
            for forbidden in ["Color::", ".fg(", ".bg("] {
                assert!(
                    !contents.contains(forbidden),
                    "literal style escape hatch {forbidden} in {}",
                    path.display()
                );
            }
        }
    }
}
