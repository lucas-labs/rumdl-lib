/// Target text column for MD030's `ol-align-column` setting: `0` (off) or `3..=6`.
///
/// Ordered list text is aligned to this column, measured from the start of the
/// marker. The range is not a style preference but a CommonMark constraint: at most
/// 4 spaces may follow a list marker (5 or more start an indented code block), and
/// the narrowest marker (`1.`) is 2 columns wide, so text can only land between
/// column 3 (`1.` + 1 space) and column 6 (`1.` + 4 spaces). `0` disables alignment.
///
/// Validated at construction and during config deserialization, so an out-of-range
/// value is rejected with a clear error rather than silently degrading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct OlAlignColumn(usize);

impl OlAlignColumn {
    /// The target column when alignment is enabled, or `None` when off (`0`).
    pub(crate) fn enabled(self) -> Option<usize> {
        (self.0 > 0).then_some(self.0)
    }
}
