//! Utilities for handling Kramdown-specific syntax
//!
//! Kramdown is a superset of Markdown that adds additional features like
//! Inline Attribute Lists (IAL) for adding attributes to elements.

/// Check if a line is a Kramdown block attribute (IAL - Inline Attribute List)
///
/// Kramdown IAL syntax allows adding attributes to block elements:
/// - `{:.class}` - CSS class
/// - `{:#id}` - Element ID
/// - `{:attribute="value"}` - Generic attributes
/// - `{:.class #id attribute="value"}` - Combinations
///
/// # Examples
///
/// ```
/// use rumdl_lib::utils::kramdown_utils::is_kramdown_block_attribute;
///
/// assert!(is_kramdown_block_attribute("{:.wrap}"));
/// assert!(is_kramdown_block_attribute("{:#my-id}"));
/// assert!(is_kramdown_block_attribute("{:.class #id}"));
/// assert!(is_kramdown_block_attribute("{:style=\"color: red\"}"));
///
/// assert!(!is_kramdown_block_attribute("{just text}"));
/// assert!(!is_kramdown_block_attribute("{}"));
/// assert!(!is_kramdown_block_attribute("{"));
/// ```
pub(crate) fn is_kramdown_block_attribute(line: &str) -> bool {
    let trimmed = line.trim();

    // Must start with { and end with }
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') || trimmed.len() < 3 {
        return false;
    }

    // Check if it matches Kramdown IAL patterns
    // Valid patterns start with {: or {# or {.
    let second_char = trimmed.chars().nth(1);
    matches!(second_char, Some(':') | Some('#') | Some('.'))
}
