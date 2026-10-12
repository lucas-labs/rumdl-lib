use crate::types::PositiveUsize;

/// Configuration for MD012 (No multiple consecutive blank lines)
#[derive(Debug, Clone, PartialEq)]
pub struct MD012Config {
    /// Maximum number of consecutive blank lines allowed within the document (default: 1)
    ///
    /// This setting controls blank lines within the document content.
    /// Blank lines at EOF are always enforced to be 0 (following POSIX/Prettier standards).
    pub(crate) maximum: PositiveUsize,
}

fn default_maximum() -> PositiveUsize {
    PositiveUsize::from_const(1)
}

impl Default for MD012Config {
    fn default() -> Self {
        Self {
            maximum: default_maximum(),
        }
    }
}
