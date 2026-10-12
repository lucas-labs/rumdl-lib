use regex::Regex;
/// MkDocs Admonitions detection utilities
///
/// The Admonitions extension provides specially-styled content blocks for
/// notes, warnings, tips, and other callouts using `!!!` and `???` markers.
///
/// Common patterns:
/// - `!!! note "Title"` - Standard admonition
/// - `??? warning "Title"` - Collapsible admonition (closed by default)
/// - `???+ tip "Title"` - Collapsible admonition (open by default)
/// - `!!! note` - Admonition without title (uses type as title)
/// - `!!! type inline` - Inline admonition (left-aligned)
/// - `!!! type inline end` - Inline admonition (right-aligned)
use std::sync::LazyLock;

/// Pattern to match admonition start markers
/// Matches: !!! type, ??? type, ???+ type, with optional "title" and modifiers
/// Type must be alphanumeric with optional dashes/underscores (no special chars)
/// Lenient: accepts unclosed quotes for real-world markdown handling
static ADMONITION_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(\s*)(?:!!!|\?\?\?\+?)\s+([a-zA-Z][a-zA-Z0-9_-]*)(?:\s+(?:inline(?:\s+end)?))?.*$"#).unwrap()
});

/// Pattern to match just the admonition marker without capturing groups
static ADMONITION_MARKER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\s*)(?:!!!|\?\?\?\+?)\s+").unwrap());

/// Pattern to validate admonition type characters
static VALID_TYPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9_-]*$").unwrap());

// Common admonition types recognized by MkDocs
// Note: Any word is valid as a custom type, so this list is informational
// Types: note, abstract, info, tip, success, question, warning, failure, danger, bug, example, quote

/// Check if a line is an admonition start marker
pub(crate) fn is_admonition_start(line: &str) -> bool {
    // First check with the basic marker
    if !ADMONITION_MARKER.is_match(line) {
        return false;
    }

    // Extract and validate the type
    let trimmed = line.trim_start();
    let after_marker = if let Some(stripped) = trimmed.strip_prefix("!!!") {
        stripped
    } else if let Some(stripped) = trimmed.strip_prefix("???+") {
        stripped
    } else if let Some(stripped) = trimmed.strip_prefix("???") {
        stripped
    } else {
        return false;
    };

    let after_marker = after_marker.trim_start();
    if after_marker.is_empty() {
        return false;
    }

    // Extract the type (first word)
    let type_part = after_marker.split_whitespace().next().unwrap_or("");

    // Validate the type contains only allowed characters
    if !VALID_TYPE.is_match(type_part) {
        return false;
    }

    // Final check with the full regex
    ADMONITION_START.is_match(line)
}
