/// Configuration for MD018 (No missing space after hash in heading)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MD018Config {
    /// Enable MagicLink support for issue/PR references like #123, #10
    /// When true, numeric patterns like `#10` at the start of a line are
    /// not flagged as malformed headings, allowing PyMdown MagicLink syntax.
    /// Default: false (all patterns are flagged)
    pub(crate) magiclink: bool,

    /// Recognize `#word` patterns as tags instead of malformed headings.
    /// When true, single-hash patterns like `#tag`, `#project/active` and
    /// `#3d_printing` are skipped. Following Obsidian, a tag must contain at
    /// least one non-numerical character, so `#1984` is still flagged.
    /// When null/unset, defaults to true for Obsidian flavor and false
    /// otherwise.
    pub(crate) tags: Option<bool>,
}

impl MD018Config {
    /// Whether tags mode is enabled, considering the flavor default.
    /// Explicit config takes precedence; otherwise Obsidian flavor enables tags.
    pub(crate) fn tags_enabled(&self, _flavor: crate::config::MarkdownFlavor) -> bool {
        self.tags.unwrap_or(false)
    }
}
