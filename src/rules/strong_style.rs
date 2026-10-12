use std::fmt;

/// The style for strong emphasis (MD050)
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default, Hash)]
pub enum StrongStyle {
    /// Consistent with the first strong style found
    #[default]
    Consistent,
    /// Asterisk style (**)
    Asterisk,
    /// Underscore style (__)
    Underscore,
}

impl fmt::Display for StrongStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StrongStyle::Asterisk => write!(f, "asterisk"),
            StrongStyle::Underscore => write!(f, "underscore"),
            StrongStyle::Consistent => write!(f, "consistent"),
        }
    }
}
