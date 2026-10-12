use crate::types::BrSpaces;

/// Configuration for MD009 (Trailing spaces)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MD009Config {
    /// Number of trailing spaces kept as a hard line break (default: 2).
    /// A value below 2 turns the exception off and flags every trailing space.
    pub(crate) br_spaces: BrSpaces,

    /// Strict mode - also flag a line-break run where it renders no `<br>` (default: false)
    pub(crate) strict: bool,

    /// Allow trailing spaces in empty list item lines (default: false)
    pub(crate) list_item_empty_lines: bool,
}
