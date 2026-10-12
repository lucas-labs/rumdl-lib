use regex::Regex;
/// MkDocs Footnotes detection utilities
///
/// The Footnotes extension provides support for footnotes with references
/// and definitions using `[^ref]` syntax.
///
/// Common patterns:
/// - `[^1]` - Footnote reference
/// - `[^note]` - Named footnote reference
/// - `[^1]: Definition` - Footnote definition
/// - Multi-line footnote definitions with 4-space indentation
use std::sync::LazyLock;

/// Pattern to match footnote definitions at start of line
/// `[^1]: Definition text`
/// Lenient: accepts empty definitions for real-world markdown
static FOOTNOTE_DEF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(\s*)\[\^([a-zA-Z0-9_-]+)\]:\s*", // \s* instead of \s+ to allow empty
    )
    .unwrap()
});

/// Check if a line contains a footnote definition
pub(crate) fn is_footnote_definition(line: &str) -> bool {
    FOOTNOTE_DEF.is_match(line)
}
