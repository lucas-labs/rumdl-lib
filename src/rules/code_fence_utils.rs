use std::fmt;

/// The style for code fence markers (MD048)
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default, Hash)]
pub enum CodeFenceStyle {
    /// Consistent with the first code fence style found
    #[default]
    Consistent,
    /// Backtick style (```)
    Backtick,
    /// Tilde style (~~~)
    Tilde,
}

impl fmt::Display for CodeFenceStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodeFenceStyle::Backtick => write!(f, "backtick"),
            CodeFenceStyle::Tilde => write!(f, "tilde"),
            CodeFenceStyle::Consistent => write!(f, "consistent"),
        }
    }
}
