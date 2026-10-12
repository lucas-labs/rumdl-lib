use crate::types::PositiveUsize;

/// Configuration for MD010 (No hard tabs)
#[derive(Debug, Clone, PartialEq)]
pub struct MD010Config {
    /// Number of spaces per tab (default: 4)
    pub(crate) spaces_per_tab: PositiveUsize,

    /// Check for hard tabs inside code blocks (default: false).
    /// When false, tabs inside fenced and indented code blocks are skipped.
    pub(crate) code_blocks: bool,

    /// Fence languages whose tabs are never reported (default: empty).
    /// Matched case-insensitively against the first word of the info string.
    /// Only has an effect when `code-blocks` is true, and an indented code block
    /// has no info string, so it can never match.
    pub(crate) ignore_code_languages: Vec<String>,
}

fn default_spaces_per_tab() -> PositiveUsize {
    PositiveUsize::from_const(4)
}

fn default_code_blocks() -> bool {
    false
}

impl Default for MD010Config {
    fn default() -> Self {
        Self {
            spaces_per_tab: default_spaces_per_tab(),
            code_blocks: default_code_blocks(),
            ignore_code_languages: Vec::new(),
        }
    }
}
