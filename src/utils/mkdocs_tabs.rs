use regex::Regex;
/// MkDocs Content Tabs detection utilities
///
/// The Tabbed extension provides support for grouped content tabs
/// using `===` markers for tab labels and content.
///
/// Common patterns:
/// - `=== "Tab 1"` - Tab with label
/// - `=== Tab` - Tab without quotes
/// - Content indented with 4 spaces under each tab
use std::sync::LazyLock;

/// Pattern to match tab markers
/// Matches: === "Label" or === Label
/// Lenient: accepts unclosed quotes, escaped quotes within quotes
static TAB_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(\s*)===\s+.*$", // Just need content after ===
    )
    .unwrap()
});

/// Check if a line is a tab marker
pub(crate) fn is_tab_marker(line: &str) -> bool {
    // First check if it starts like a tab marker
    let trimmed_start = line.trim_start();
    if !trimmed_start.starts_with("===") {
        return false;
    }

    // Reject double === (like "=== ===")
    // Check what comes after the first ===
    let after_marker = &trimmed_start[3..];
    if after_marker.trim_start().starts_with("===") {
        return false; // Double === is invalid
    }

    let trimmed = line.trim();

    // Must have content after ===
    if trimmed.len() <= 3 || !trimmed.chars().nth(3).is_some_and(char::is_whitespace) {
        return false;
    }

    // Be lenient with quote matching to handle real-world markdown
    // A future rule can warn about unclosed quotes
    // For now, just ensure there's some content after ===

    // Use the original regex as a final check
    TAB_MARKER.is_match(line)
}
