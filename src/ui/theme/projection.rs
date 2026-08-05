use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    pub rgb: (u8, u8, u8),
    pub ansi256: u8,
    pub ansi16: Color,
}

impl Token {
    pub const fn new(rgb: (u8, u8, u8), ansi256: u8, ansi16: Color) -> Self {
        Self {
            rgb,
            ansi256,
            ansi16,
        }
    }
}
