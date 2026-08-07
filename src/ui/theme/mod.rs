//! Allocation-free semantic styling for every Tale rendering path.
//!
//! Views select [`StyleRole`] values. Numeric colors and terminal projections
//! remain private to this module so domain and widget code cannot assign meaning
//! to a shade accidentally.

mod projection;
mod role;

use projection::Token;
use ratatui::style::{Color, Modifier, Style};
pub use role::{SemanticSignal, StyleRole};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ThemeId {
    TailscaleDark,
    TailscaleLight,
    Terminal,
}

impl ThemeId {
    pub const ALL: [Self; 3] = [Self::TailscaleDark, Self::TailscaleLight, Self::Terminal];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TailscaleDark => "tailscale-dark",
            Self::TailscaleLight => "tailscale-light",
            Self::Terminal => "terminal",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "tailscale-dark" => Some(Self::TailscaleDark),
            "tailscale-light" => Some(Self::TailscaleLight),
            "terminal" => Some(Self::Terminal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ColorCapability {
    TrueColor,
    Ansi256,
    Ansi16,
    None,
}

impl ColorCapability {
    pub const ALL: [Self; 4] = [Self::TrueColor, Self::Ansi256, Self::Ansi16, Self::None];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TrueColor => "truecolor",
            Self::Ansi256 => "ansi256",
            Self::Ansi16 => "ansi16",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Theme {
    id: ThemeId,
    capability: ColorCapability,
}

impl Theme {
    pub const fn new(id: ThemeId, capability: ColorCapability) -> Self {
        Self { id, capability }
    }

    pub const fn id(self) -> ThemeId {
        self.id
    }
    pub const fn capability(self) -> ColorCapability {
        self.capability
    }

    pub fn style(self, role: StyleRole) -> Style {
        if self.capability == ColorCapability::None {
            return Style::default()
                .fg(Color::Reset)
                .bg(Color::Reset)
                .add_modifier(role.no_color_modifier());
        }
        let spec = role_spec(role);
        let mut style = Style::default().add_modifier(spec.modifier);
        if let Some(fg) = spec.fg {
            style = style.fg(self.project(fg));
        }
        if let Some(bg) = spec.bg {
            style = style.bg(self.project(bg));
        }
        style
    }

    /// Composes meanings using the mandated precedence, independent of caller order.
    pub fn compose(self, composition: StyleComposition) -> Style {
        let mut style = self.style(composition.base);
        for role in [
            composition.source,
            composition.state,
            composition.risk,
            composition.selection,
            composition.focus,
            composition.safety,
        ]
        .into_iter()
        .flatten()
        {
            style = style.patch(self.style(role));
        }
        style
    }

    fn project(self, token: TokenKind) -> Color {
        if self.id == ThemeId::Terminal && token.is_neutral() {
            return Color::Reset;
        }
        let value = token.value(self.id);
        match self.capability {
            ColorCapability::TrueColor => Color::Rgb(value.rgb.0, value.rgb.1, value.rgb.2),
            ColorCapability::Ansi256 => Color::Indexed(value.ansi256),
            ColorCapability::Ansi16 => value.ansi16,
            ColorCapability::None => Color::Reset,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct StyleComposition {
    pub base: StyleRole,
    pub source: Option<StyleRole>,
    pub state: Option<StyleRole>,
    pub risk: Option<StyleRole>,
    pub selection: Option<StyleRole>,
    pub focus: Option<StyleRole>,
    pub safety: Option<StyleRole>,
}

impl StyleComposition {
    pub const fn new(base: StyleRole) -> Self {
        Self {
            base,
            source: None,
            state: None,
            risk: None,
            selection: None,
            focus: None,
            safety: None,
        }
    }
}

#[derive(Clone, Copy)]
struct RoleSpec {
    fg: Option<TokenKind>,
    bg: Option<TokenKind>,
    modifier: Modifier,
}

impl RoleSpec {
    const fn fg(fg: TokenKind) -> Self {
        Self {
            fg: Some(fg),
            bg: None,
            modifier: Modifier::empty(),
        }
    }
    const fn bg(fg: TokenKind, bg: TokenKind) -> Self {
        Self {
            fg: Some(fg),
            bg: Some(bg),
            modifier: Modifier::empty(),
        }
    }
    const fn modified(mut self, modifier: Modifier) -> Self {
        self.modifier = modifier;
        self
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TokenKind {
    Canvas,
    Surface,
    Raised,
    Inset,
    Backdrop,
    Primary,
    Muted,
    Disabled,
    BorderSubtle,
    Border,
    /// Text that sits on the selection fill. Gray900 in both themes, because
    /// the fill is the same accent in both and only dark ink clears 4.5:1 on it.
    SelectionInk,
    Focus,
    FocusStrong,
    Healthy,
    Info,
    Admin,
    Warning,
    Danger,
}

impl TokenKind {
    const fn is_neutral(self) -> bool {
        matches!(
            self,
            Self::Canvas
                | Self::Surface
                | Self::Raised
                | Self::Inset
                | Self::Backdrop
                | Self::Primary
                | Self::Muted
                | Self::Disabled
                | Self::BorderSubtle
                | Self::Border
        )
    }

    fn value(self, id: ThemeId) -> Token {
        let dark = id != ThemeId::TailscaleLight;
        match (self, dark) {
            // Gray900. The brand toolkit names this as the dark background.
            (Self::Canvas | Self::Inset | Self::Backdrop, true) => {
                Token::new((31, 30, 30), 234, Color::Black)
            }
            // The toolkit has no step between Gray900 and Gray600, so the two
            // elevation surfaces are interpolated across that gap.
            (Self::Surface, true) => Token::new((42, 41, 41), 235, Color::Black),
            (Self::Raised | Self::BorderSubtle, true) => {
                Token::new((53, 52, 52), 236, Color::DarkGray)
            }
            (Self::Primary, true) => Token::new((250, 249, 248), 231, Color::White),
            (Self::Muted, true) => Token::new((175, 172, 171), 145, Color::Gray),
            (Self::Disabled, true) => Token::new((112, 110, 109), 242, Color::DarkGray),
            (Self::Border, true) => Token::new((68, 67, 66), 238, Color::Gray),
            (Self::SelectionInk, true) => Token::new((31, 30, 30), 234, Color::Black),
            (Self::Focus | Self::Info, true) => Token::new((133, 170, 245), 111, Color::LightBlue),
            (Self::FocusStrong, true) => Token::new((90, 130, 222), 68, Color::Blue),
            (Self::Healthy, true) => Token::new((51, 194, 127), 78, Color::Green),
            (Self::Admin, true) => Token::new((190, 143, 225), 140, Color::Magenta),
            (Self::Warning, true) => Token::new((229, 153, 62), 215, Color::Yellow),
            (Self::Danger, true) => Token::new((246, 143, 135), 210, Color::Red),
            // 255 rather than the nearer 231, so canvas stays under surface.
            (Self::Canvas, false) => Token::new((250, 249, 248), 255, Color::White),
            (Self::Surface | Self::Raised, false) => Token::new((255, 255, 255), 231, Color::White),
            (Self::Inset | Self::BorderSubtle, false) => {
                Token::new((238, 235, 234), 255, Color::Gray)
            }
            (Self::Backdrop, false) => Token::new((218, 214, 213), 188, Color::Gray),
            (Self::Border, false) => Token::new((218, 214, 213), 188, Color::DarkGray),
            (Self::Primary | Self::SelectionInk, false) => {
                Token::new((31, 30, 30), 234, Color::Black)
            }
            (Self::Muted, false) => Token::new((112, 110, 109), 242, Color::DarkGray),
            (Self::Disabled, false) => Token::new((175, 172, 171), 145, Color::Gray),
            (Self::Focus, false) => Token::new((63, 93, 179), 61, Color::Blue),
            (Self::FocusStrong, false) => Token::new((90, 130, 222), 68, Color::LightBlue),
            (Self::Healthy, false) => Token::new((9, 130, 93), 29, Color::Green),
            // 68 rather than the nearer 61, which focus already holds.
            (Self::Info, false) => Token::new((75, 112, 204), 68, Color::Blue),
            (Self::Admin, false) => Token::new((128, 82, 161), 97, Color::Magenta),
            (Self::Warning, false) => Token::new((187, 85, 4), 130, Color::Yellow),
            (Self::Danger, false) => Token::new((178, 45, 48), 88, Color::Red),
        }
    }
}

fn role_spec(role: StyleRole) -> RoleSpec {
    use StyleRole as R;
    use TokenKind as T;
    let primary = T::Primary;
    match role {
        R::Canvas => RoleSpec::bg(primary, T::Canvas),
        R::Surface => RoleSpec::bg(primary, T::Surface),
        R::SurfaceRaised => RoleSpec::bg(primary, T::Raised),
        R::SurfaceInset => RoleSpec::bg(primary, T::Inset),
        R::Backdrop => RoleSpec::bg(T::Muted, T::Backdrop).modified(Modifier::DIM),
        R::BorderSubtle | R::Divider => RoleSpec::fg(T::BorderSubtle),
        R::BorderNormal => RoleSpec::fg(T::Border),
        R::BorderFocused | R::Focus => RoleSpec::fg(T::Focus).modified(Modifier::BOLD),
        R::BorderDanger => RoleSpec::fg(T::Danger).modified(Modifier::BOLD),
        R::TextPrimary => RoleSpec::fg(primary),
        R::TextMuted => RoleSpec::fg(T::Muted),
        R::TextDisabled => RoleSpec::fg(T::Disabled).modified(Modifier::DIM),
        R::TextInverse => RoleSpec::fg(T::Canvas),
        R::SectionHeading => RoleSpec::bg(T::Canvas, T::Focus).modified(Modifier::BOLD),
        R::TextLink => RoleSpec::fg(T::Info).modified(Modifier::UNDERLINED),
        R::TextCode => RoleSpec::fg(T::Focus).modified(Modifier::ITALIC),
        R::KeyHint => RoleSpec::fg(T::Focus).modified(Modifier::BOLD),
        R::KeyHintDisabled => {
            RoleSpec::fg(T::Disabled).modified(Modifier::DIM.union(Modifier::CROSSED_OUT))
        }
        R::Prompt => RoleSpec::fg(primary).modified(Modifier::BOLD),
        R::CompletionMatch => RoleSpec::fg(T::Focus).modified(Modifier::UNDERLINED),
        // Three tokens that stay apart in truecolor, 256, and 16-colour terminals.
        R::SyntaxField => RoleSpec::fg(T::Focus).modified(Modifier::BOLD),
        R::SyntaxOperator => RoleSpec::fg(T::Muted),
        R::SyntaxValue => RoleSpec::fg(T::Admin),
        // One rule for both themes: the fill is the brand's core accent tone and
        // only dark ink clears 4.5:1 on it. The old rule put near-white primary
        // on a light-blue fill, which measured 2.17:1.
        R::CompletionSelected | R::Selection => {
            RoleSpec::bg(T::SelectionInk, T::FocusStrong).modified(Modifier::BOLD)
        }
        R::SelectionInactive => RoleSpec::bg(primary, T::Raised).modified(Modifier::UNDERLINED),
        R::StateHealthy | R::TaskSucceeded | R::DiffAdded => {
            RoleSpec::fg(T::Healthy).modified(Modifier::BOLD)
        }
        R::StateInfo | R::StateDirect | R::SourceLocal | R::RiskObserve => {
            RoleSpec::fg(T::Info).modified(Modifier::UNDERLINED)
        }
        R::StateWarning | R::StateStale | R::StateRelay | R::RiskReversible | R::DiffChanged => {
            RoleSpec::fg(T::Warning).modified(Modifier::BOLD)
        }
        R::StateDanger | R::StatePublic | R::TaskFailed | R::DiffRemoved => {
            RoleSpec::fg(T::Danger).modified(Modifier::BOLD)
        }
        R::StatePending | R::TaskQueued | R::TaskRunning => {
            RoleSpec::fg(T::Focus).modified(Modifier::ITALIC)
        }
        R::StateDisabled | R::StateUnknown | R::StateOffline | R::TaskCancelled => {
            RoleSpec::fg(T::Disabled).modified(Modifier::CROSSED_OUT)
        }
        R::SourceAdmin => RoleSpec::fg(T::Admin).modified(Modifier::ITALIC),
        R::SourceCombined => {
            RoleSpec::fg(T::Admin).modified(Modifier::BOLD.union(Modifier::ITALIC))
        }
        R::RiskDisruptive => {
            RoleSpec::fg(T::Warning).modified(Modifier::BOLD.union(Modifier::UNDERLINED))
        }
        R::RiskDestructive => {
            RoleSpec::fg(T::Danger).modified(Modifier::BOLD.union(Modifier::REVERSED))
        }
        R::Secret => RoleSpec::fg(T::Warning).modified(Modifier::BOLD),
        R::Redacted => RoleSpec::fg(T::Muted).modified(Modifier::CROSSED_OUT),
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorCapability, StyleComposition, StyleRole, Theme, ThemeId, TokenKind};
    use ratatui::style::{Color, Modifier};

    fn luminance((red, green, blue): (u8, u8, u8)) -> f64 {
        fn channel(value: u8) -> f64 {
            let value = f64::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
    }

    fn contrast(left: TokenKind, right: TokenKind, id: ThemeId) -> f64 {
        let left = luminance(left.value(id).rgb);
        let right = luminance(right.value(id).rgb);
        let (lighter, darker) = if left >= right {
            (left, right)
        } else {
            (right, left)
        };
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn truecolor_contrast_pairs_meet_the_documented_gates() {
        for id in [ThemeId::TailscaleDark, ThemeId::TailscaleLight] {
            for surface in [
                TokenKind::Canvas,
                TokenKind::Surface,
                TokenKind::Raised,
                TokenKind::Inset,
            ] {
                assert!(contrast(TokenKind::Primary, surface, id) >= 4.5);
            }
            for surface in [TokenKind::Canvas, TokenKind::Surface, TokenKind::Raised] {
                assert!(
                    contrast(TokenKind::Muted, surface, id) >= 4.5,
                    "muted {:?} on {:?}: {}",
                    id,
                    surface as u8,
                    contrast(TokenKind::Muted, surface, id)
                );
            }
            for boundary in [
                TokenKind::Focus,
                TokenKind::Healthy,
                TokenKind::Warning,
                TokenKind::Danger,
                TokenKind::Admin,
            ] {
                assert!(contrast(boundary, TokenKind::Surface, id) >= 3.0);
            }
        }
    }

    /// The selection fill is the one surface the surface gates above never
    /// covered, which is how it shipped at 2.17:1.
    #[test]
    fn selection_ink_clears_the_small_text_gate_on_the_selection_fill() {
        for id in ThemeId::ALL {
            let measured = contrast(TokenKind::SelectionInk, TokenKind::FocusStrong, id);
            assert!(
                measured >= 4.5,
                "selection ink on fill for {id:?}: {measured}"
            );
        }
    }

    #[test]
    fn reduced_color_projections_preserve_distinguishable_styles() {
        for id in ThemeId::ALL {
            let ansi256 = Theme::new(id, ColorCapability::Ansi256);
            let roles = [
                StyleRole::StateHealthy,
                StyleRole::StateWarning,
                StyleRole::StateDanger,
                StyleRole::SourceAdmin,
                StyleRole::Focus,
            ];
            for (index, role) in roles.iter().enumerate() {
                for other in roles.iter().skip(index + 1) {
                    assert_ne!(ansi256.style(*role), ansi256.style(*other));
                }
            }
            let no_color = Theme::new(id, ColorCapability::None);
            for role in StyleRole::ALL {
                let style = no_color.style(role);
                assert_eq!(style.fg, Some(Color::Reset));
                assert_eq!(style.bg, Some(Color::Reset));
            }
            assert_ne!(
                no_color.style(StyleRole::Selection),
                no_color.style(StyleRole::Focus)
            );
            assert!(
                no_color
                    .style(StyleRole::StateDanger)
                    .add_modifier
                    .contains(Modifier::REVERSED)
            );
        }
    }

    #[test]
    fn composition_uses_safety_as_the_highest_precedence() {
        let theme = Theme::new(ThemeId::TailscaleDark, ColorCapability::TrueColor);
        let style = theme.compose(StyleComposition {
            base: StyleRole::TextPrimary,
            source: Some(StyleRole::SourceAdmin),
            state: Some(StyleRole::StateOffline),
            risk: Some(StyleRole::RiskDestructive),
            selection: Some(StyleRole::Selection),
            focus: Some(StyleRole::Focus),
            safety: Some(StyleRole::Redacted),
        });
        assert_eq!(style.fg, theme.style(StyleRole::Redacted).fg);
        assert!(style.add_modifier.contains(Modifier::CROSSED_OUT));
    }
}
