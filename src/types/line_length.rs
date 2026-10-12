use std::fmt;

/// A line length value that can be 0 (meaning no limit) or a positive value (≥1)
///
/// Many configuration values for line length need to support both:
/// - 0: Special value meaning "no line length limit"
/// - ≥1: Actual line length limit
///
/// This type enforces those constraints at deserialization time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineLength(Option<usize>);

impl LineLength {
    /// Create a new LineLength, where 0 means no limit and values ≥1 are actual limits.
    pub fn new(value: usize) -> Self {
        if value == 0 { Self(None) } else { Self(Some(value)) }
    }

    /// Get the underlying value (0 for no limit, otherwise the actual limit).
    pub(crate) fn get(self) -> usize {
        self.0.unwrap_or(0)
    }

    /// Check if this represents "no limit" (value was 0).
    pub(crate) fn is_unlimited(self) -> bool {
        self.0.is_none()
    }

    /// Get the effective limit for comparisons.
    /// Returns usize::MAX for unlimited, otherwise the actual limit.
    pub(crate) fn effective_limit(self) -> usize {
        self.0.unwrap_or(usize::MAX)
    }

    /// Convert from a default value (for use in config defaults).
    ///
    /// # Panics
    /// Never panics - accepts any value including 0.
    pub(crate) const fn from_const(value: usize) -> Self {
        if value == 0 { Self(None) } else { Self(Some(value)) }
    }
}

impl From<LineLength> for usize {
    fn from(val: LineLength) -> Self {
        val.get()
    }
}

impl fmt::Display for LineLength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_unlimited() {
            write!(f, "unlimited")
        } else {
            write!(f, "{}", self.get())
        }
    }
}

impl Default for LineLength {
    fn default() -> Self {
        Self::from_const(80) // Default line length is 80
    }
}
