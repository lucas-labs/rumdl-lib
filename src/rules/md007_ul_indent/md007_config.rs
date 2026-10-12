use crate::types::IndentSize;

/// Indentation style for unordered lists
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum IndentStyle {
    /// Text-aligned: Nested items align with parent's text content (rumdl default)
    #[default]
    TextAligned,
    /// Fixed: Use fixed multiples of indent size (markdownlint compatible)
    Fixed,
}

/// Configuration for MD007 (Unordered list indentation)
#[derive(Debug, Clone, PartialEq)]
pub struct MD007Config {
    /// Indentation size for nested unordered lists (default: 2)
    pub(crate) indent: IndentSize,

    /// Allow first level lists to start indented (default: false)
    pub(crate) start_indented: bool,

    /// Number of spaces for first level indent when start_indented is true (default: 2)
    pub(crate) start_indent: IndentSize,

    /// Indentation style: text-aligned (default) or fixed (markdownlint compatible)
    pub(crate) style: IndentStyle,

    /// Whether style was explicitly set in config (used for smart auto-detection)
    /// When false and indent != 2, we auto-select style based on document content:
    /// - Pure unordered lists → fixed style (markdownlint compatible)
    /// - Mixed ordered/unordered → text-aligned (avoids oscillation)
    pub(crate) style_explicit: bool,

    /// Whether indent was explicitly set in config (used for "Do What I Mean" behavior)
    /// When indent is explicitly set but style is not, automatically use fixed style
    pub(crate) indent_explicit: bool,
}

fn default_indent() -> IndentSize {
    IndentSize::from_const(2)
}

fn default_start_indent() -> IndentSize {
    IndentSize::from_const(2)
}

impl Default for MD007Config {
    fn default() -> Self {
        Self {
            indent: default_indent(),
            start_indented: false,
            start_indent: default_start_indent(),
            style: IndentStyle::default(),
            style_explicit: false,
            indent_explicit: false,
        }
    }
}
