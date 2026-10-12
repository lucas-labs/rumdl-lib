use regex::Regex;
/// MkDocs Snippets extension detection utilities
///
/// The Snippets extension allows including content from external files
/// using ASCII scissors syntax: `--8<--`
///
/// Common patterns:
/// - `--8<-- "filename.md"` - Include entire file
/// - `--8<-- "filename.md:start:end"` - Include specific lines
/// - `<!-- --8<-- [start:section] -->` - Start marker for section
/// - `<!-- --8<-- [end:section] -->` - End marker for section
///
use std::sync::LazyLock;

/// Pattern to match valid snippet markers: -{1,}8<-{1,}
/// Based on PyMdown Extensions Snippets specification
static BARE_SNIPPET_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^;*-+8<-+$", // Optional semicolons, then dashes-8<-dashes only
    )
    .unwrap()
});

/// Check if a line is a bare snippet block delimiter (for multi-line blocks)
pub(crate) fn is_snippet_block_delimiter(line: &str) -> bool {
    let trimmed = line.trim();
    // Check for trailing whitespace (space or tab) BEFORE full trim
    let trimmed_start = line.trim_start();
    let has_trailing_whitespace = trimmed_start.ends_with(' ') || trimmed_start.ends_with('\t');
    // Bare markers without trailing whitespace are valid block delimiters
    BARE_SNIPPET_MARKER.is_match(trimmed) && !has_trailing_whitespace
}
