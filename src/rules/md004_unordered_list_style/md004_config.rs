use super::UnorderedListStyle;

/// Configuration for MD004 (Unordered list style)
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct MD004Config {
    /// The style for unordered list markers
    pub(crate) style: UnorderedListStyle,
}
