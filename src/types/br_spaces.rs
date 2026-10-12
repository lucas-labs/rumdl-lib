/// Number of trailing spaces that MD009 accepts as a hard line break.
///
/// CommonMark renders a hard line break for two or more trailing spaces, so a
/// value below 2 cannot describe one. Such a value turns the exception off
/// instead: every trailing space is then reported. This mirrors markdownlint,
/// where `br_spaces` of 0 or 1 "disallows any trailing spaces".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BrSpaces(usize);

impl BrSpaces {
    /// Fewest trailing spaces that render a hard line break (CommonMark).
    pub const MIN: usize = 2;

    pub(crate) const fn new(value: usize) -> Self {
        Self(value)
    }

    /// The trailing-space count kept as a hard line break, or `None` when the
    /// exception is off and every trailing space is reported.
    pub(crate) fn line_break(self) -> Option<usize> {
        (self.0 >= Self::MIN).then_some(self.0)
    }
}

impl Default for BrSpaces {
    fn default() -> Self {
        Self(Self::MIN)
    }
}

impl From<BrSpaces> for usize {
    fn from(val: BrSpaces) -> Self {
        val.0
    }
}
