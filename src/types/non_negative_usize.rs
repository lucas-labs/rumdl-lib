/// A non-negative usize (≥0)
///
/// Many configuration values allow explicitly specifying `0` to disable a behavior (e.g., no blank
/// lines required) while still requiring validation for negative inputs during deserialization.
/// `NonNegativeUsize` captures that intent and guarantees the stored value is never negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NonNegativeUsize(usize);

impl NonNegativeUsize {
    /// Create a new NonNegativeUsize from an unsigned value.
    #[inline]
    pub(crate) fn new(value: usize) -> Self {
        Self(value)
    }

    /// Get the underlying value.
    #[inline]
    pub(crate) fn get(self) -> usize {
        self.0
    }
}

impl From<NonNegativeUsize> for usize {
    fn from(val: NonNegativeUsize) -> Self {
        val.0
    }
}
