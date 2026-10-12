use std::fmt;

/// A positive non-zero usize (≥1)
///
/// Many configuration values must be at least 1 (e.g., indentation sizes, spaces per tab).
/// This type enforces that constraint at deserialization time, preventing invalid configs
/// like "0 spaces per tab" or "0 character line length".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PositiveUsize(usize);

impl PositiveUsize {
    /// Create a new PositiveUsize, validating it's at least 1.
    ///
    /// # Errors
    /// Returns `PositiveUsizeError` if the value is 0.
    pub(crate) fn new(value: usize) -> Result<Self, PositiveUsizeError> {
        if value >= 1 {
            Ok(Self(value))
        } else {
            Err(PositiveUsizeError(value))
        }
    }

    /// Get the underlying value (guaranteed to be ≥1).
    pub(crate) fn get(self) -> usize {
        self.0
    }

    /// Convert from a default value (for use in config defaults).
    ///
    /// # Panics
    /// Panics if the value is 0. This is intended for const defaults only.
    pub(crate) const fn from_const(value: usize) -> Self {
        assert!(value >= 1, "PositiveUsize must be at least 1");
        Self(value)
    }
}

/// Error type for invalid PositiveUsize values.
#[derive(Debug, Clone, Copy)]
pub struct PositiveUsizeError(usize);

impl fmt::Display for PositiveUsizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Value must be at least 1, got {}. Zero is not a valid value for this configuration.",
            self.0
        )
    }
}

impl std::error::Error for PositiveUsizeError {}

impl From<PositiveUsize> for usize {
    fn from(val: PositiveUsize) -> Self {
        val.0
    }
}
