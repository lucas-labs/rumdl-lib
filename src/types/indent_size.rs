/// Indentation size (1-8 spaces)
///
/// Enforces reasonable indentation bounds. While Markdown technically allows any
/// indentation, values outside 1-8 are either mistakes or impractical. Common values
/// are 2 (default) and 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IndentSize(u8);

impl IndentSize {
    /// Minimum indentation (1 space)
    pub const MIN: u8 = 1;
    /// Maximum indentation (8 spaces)
    pub const MAX: u8 = 8;

    /// Get the underlying value (guaranteed to be 1-8).
    pub(crate) fn get(self) -> u8 {
        self.0
    }

    /// Convert from a default value (for use in config defaults).
    ///
    /// # Panics
    /// Panics if the value is not in range 1-8. This is intended for const defaults only.
    pub(crate) const fn from_const(value: u8) -> Self {
        assert!(
            value >= Self::MIN && value <= Self::MAX,
            "IndentSize must be between 1 and 8"
        );
        Self(value)
    }
}

impl Default for IndentSize {
    fn default() -> Self {
        Self(2) // Safe: 2 is a common default
    }
}

impl From<IndentSize> for usize {
    fn from(val: IndentSize) -> Self {
        val.0 as usize
    }
}

impl From<IndentSize> for u8 {
    fn from(val: IndentSize) -> Self {
        val.0
    }
}
