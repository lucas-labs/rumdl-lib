/// Represents the style for ordered lists
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub enum ListStyle {
    One,      // Use '1.' for all items
    OneOne,   // All ones (1. 1. 1.)
    Ordered,  // Sequential (1. 2. 3.)
    Ordered0, // Zero-based (0. 1. 2.)
    #[default]
    OneOrOrdered, // Either all ones OR sequential per-list (markdownlint default)
    Consistent, // Document-wide: use most prevalent style across all lists
}

/// Configuration for MD029 (Ordered list item prefix)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MD029Config {
    /// Style for ordered list numbering (default: "one-or-ordered" - matches markdownlint)
    pub(crate) style: ListStyle,
}
