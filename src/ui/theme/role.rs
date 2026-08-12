use ratatui::style::Modifier;

/// A visual meaning requested by a view. Roles deliberately contain no color.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum StyleRole {
    Canvas,
    Surface,
    SurfaceRaised,
    SurfaceInset,
    Backdrop,
    BorderSubtle,
    BorderNormal,
    BorderFocused,
    BorderDanger,
    Divider,
    TextPrimary,
    TextMuted,
    TextDisabled,
    TextInverse,
    TextLink,
    TextCode,
    SectionHeading,
    KeyHint,
    KeyHintDisabled,
    Prompt,
    CompletionMatch,
    CompletionSelected,
    SyntaxField,
    SyntaxOperator,
    SyntaxValue,
    Selection,
    SelectionInactive,
    Focus,
    StateHealthy,
    StateInfo,
    StateWarning,
    StateDanger,
    StatePending,
    StateDisabled,
    StateUnknown,
    StateStale,
    StatePublic,
    StateDirect,
    StateRelay,
    StateOffline,
    SourceLocal,
    SourceAdmin,
    SourceCombined,
    RiskObserve,
    RiskReversible,
    RiskDisruptive,
    RiskDestructive,
    TaskQueued,
    TaskRunning,
    TaskSucceeded,
    TaskFailed,
    TaskCancelled,
    DiffAdded,
    DiffRemoved,
    DiffChanged,
    Secret,
    Redacted,
}

impl StyleRole {
    pub const ALL: [Self; 57] = [
        Self::Canvas,
        Self::Surface,
        Self::SurfaceRaised,
        Self::SurfaceInset,
        Self::Backdrop,
        Self::BorderSubtle,
        Self::BorderNormal,
        Self::BorderFocused,
        Self::BorderDanger,
        Self::Divider,
        Self::TextPrimary,
        Self::TextMuted,
        Self::TextDisabled,
        Self::TextInverse,
        Self::TextLink,
        Self::TextCode,
        Self::SectionHeading,
        Self::KeyHint,
        Self::KeyHintDisabled,
        Self::Prompt,
        Self::CompletionMatch,
        Self::CompletionSelected,
        Self::SyntaxField,
        Self::SyntaxOperator,
        Self::SyntaxValue,
        Self::Selection,
        Self::SelectionInactive,
        Self::Focus,
        Self::StateHealthy,
        Self::StateInfo,
        Self::StateWarning,
        Self::StateDanger,
        Self::StatePending,
        Self::StateDisabled,
        Self::StateUnknown,
        Self::StateStale,
        Self::StatePublic,
        Self::StateDirect,
        Self::StateRelay,
        Self::StateOffline,
        Self::SourceLocal,
        Self::SourceAdmin,
        Self::SourceCombined,
        Self::RiskObserve,
        Self::RiskReversible,
        Self::RiskDisruptive,
        Self::RiskDestructive,
        Self::TaskQueued,
        Self::TaskRunning,
        Self::TaskSucceeded,
        Self::TaskFailed,
        Self::TaskCancelled,
        Self::DiffAdded,
        Self::DiffRemoved,
        Self::DiffChanged,
        Self::Secret,
        Self::Redacted,
    ];

    /// Stable non-color signaling. Renderers keep the label alongside the symbol.
    pub const fn signal(self) -> SemanticSignal {
        match self {
            Self::StateHealthy => SemanticSignal::new("✓", "+", "healthy"),
            Self::TaskSucceeded => SemanticSignal::new("✓", "+", "succeeded"),
            Self::DiffAdded => SemanticSignal::new("+", "+", "added"),
            Self::StateInfo => SemanticSignal::new("i", "i", "info"),
            Self::StateDirect => SemanticSignal::new("●", "i", "direct"),
            Self::SourceLocal => SemanticSignal::new("L", "L", "local"),
            Self::StateWarning => SemanticSignal::new("▲", "!", "warning"),
            Self::StateStale => SemanticSignal::new("▲", "!", "stale"),
            Self::StateRelay => SemanticSignal::new("▲", "!", "relay"),
            Self::RiskReversible => SemanticSignal::new("▲", "!", "reversible"),
            Self::DiffChanged => SemanticSignal::new("~", "!", "changed"),
            Self::StateDanger => SemanticSignal::new("◆", "X", "danger"),
            Self::StatePublic => SemanticSignal::new("◆", "X", "public"),
            Self::TaskFailed => SemanticSignal::new("×", "X", "failed"),
            Self::RiskDestructive => SemanticSignal::new("◆", "X", "destructive"),
            Self::DiffRemoved => SemanticSignal::new("-", "-", "removed"),
            Self::StatePending => SemanticSignal::new("◌", "~", "pending"),
            Self::TaskQueued => SemanticSignal::new("◌", "~", "queued"),
            Self::TaskRunning => SemanticSignal::new("◌", "~", "running"),
            Self::StateDisabled => SemanticSignal::new("○", "-", "disabled"),
            Self::StateOffline => SemanticSignal::new("○", "-", "offline"),
            Self::TaskCancelled => SemanticSignal::new("○", "-", "cancelled"),
            Self::StateUnknown => SemanticSignal::new("?", "?", "unknown"),
            Self::SourceAdmin => SemanticSignal::new("A", "A", "admin"),
            Self::SourceCombined => SemanticSignal::new("L+A", "L+A", "local+admin"),
            Self::RiskObserve => SemanticSignal::new("○", "O", "observe"),
            Self::RiskDisruptive => SemanticSignal::new("▲", "!", "disruptive"),
            Self::Secret => SemanticSignal::new("◆", "*", "secret"),
            Self::Redacted => SemanticSignal::new("■", "#", "redacted"),
            _ => SemanticSignal::new("", "", ""),
        }
    }

    pub(crate) const fn no_color_modifier(self) -> Modifier {
        match self {
            Self::Canvas | Self::Surface | Self::TextPrimary | Self::BorderNormal => {
                Modifier::empty()
            }
            Self::SurfaceRaised | Self::BorderFocused | Self::Focus | Self::Prompt => {
                Modifier::BOLD
            }
            Self::SurfaceInset | Self::TextCode | Self::SyntaxValue => Modifier::ITALIC,
            Self::SyntaxField => Modifier::BOLD.union(Modifier::UNDERLINED),
            Self::SyntaxOperator => Modifier::DIM.union(Modifier::ITALIC),
            Self::Backdrop | Self::TextDisabled | Self::StateDisabled => {
                Modifier::DIM.union(Modifier::CROSSED_OUT)
            }
            Self::BorderSubtle | Self::Divider | Self::TextMuted | Self::SelectionInactive => {
                Modifier::DIM
            }
            Self::BorderDanger
            | Self::StateDanger
            | Self::StatePublic
            | Self::RiskDestructive
            | Self::TaskFailed
            | Self::DiffRemoved => Modifier::BOLD.union(Modifier::REVERSED),
            Self::TextInverse
            | Self::SectionHeading
            | Self::CompletionSelected
            | Self::Selection => Modifier::REVERSED,
            Self::TextLink
            | Self::KeyHint
            | Self::CompletionMatch
            | Self::StateInfo
            | Self::StateDirect
            | Self::SourceLocal
            | Self::RiskObserve => Modifier::UNDERLINED,
            Self::KeyHintDisabled => Modifier::DIM.union(Modifier::CROSSED_OUT),
            Self::StateHealthy | Self::TaskSucceeded | Self::DiffAdded | Self::Secret => {
                Modifier::BOLD
            }
            Self::StateWarning
            | Self::StateStale
            | Self::StateRelay
            | Self::RiskReversible
            | Self::DiffChanged => Modifier::BOLD.union(Modifier::UNDERLINED),
            Self::StatePending | Self::TaskQueued | Self::TaskRunning => {
                Modifier::ITALIC.union(Modifier::UNDERLINED)
            }
            Self::StateUnknown | Self::TaskCancelled | Self::Redacted => Modifier::CROSSED_OUT,
            Self::StateOffline => Modifier::DIM,
            Self::SourceAdmin => Modifier::ITALIC,
            Self::SourceCombined => Modifier::BOLD.union(Modifier::ITALIC),
            Self::RiskDisruptive => Modifier::BOLD.union(Modifier::UNDERLINED),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SemanticSignal {
    pub unicode: &'static str,
    pub ascii: &'static str,
    pub label: &'static str,
}

impl SemanticSignal {
    const fn new(unicode: &'static str, ascii: &'static str, label: &'static str) -> Self {
        Self {
            unicode,
            ascii,
            label,
        }
    }
}
