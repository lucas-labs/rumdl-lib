//!
//! Cached Regex Patterns and Fast Content Checks for Markdown Linting
//!
//! This module provides a centralized collection of pre-compiled, cached regex patterns
//! for all major Markdown constructs (headings, lists, code blocks, links, images, etc.).
//! It also includes fast-path utility functions for quickly checking if content
//! potentially contains certain Markdown elements, allowing rules to skip expensive
//! processing when unnecessary.
//!
//! # Performance
//!
//! Fixed patterns use `LazyLock` and compile on first use. Dynamic patterns are
//! cached by their pattern text so subsequent lookups reuse the compiled regex.
//!
//! # Usage
//!
//! - Use the provided statics for common Markdown patterns.
//! - Use the `regex_lazy!` macro for ad-hoc regexes that are not predefined.
//! - Use the utility functions for fast content checks before running regexes.

use fancy_regex::Regex as FancyRegex;
use regex::Regex;
use std::sync::LazyLock;

/// Macro for defining a lazily-initialized, cached regex pattern.
///
/// Use this for ad-hoc regexes that are not already defined in this module.
///
/// # Panics
///
/// This macro will panic at initialization if the regex pattern is invalid.
/// This is intentional for compile-time constant patterns - we want to catch
/// invalid patterns during development, not at runtime.
///
/// # Example
///
/// ```
/// use std::sync::LazyLock;
/// use rumdl_lib::regex_lazy;
/// let my_re = regex_lazy!(r"^foo.*bar$");
/// assert!(my_re.is_match("foobar"));
/// ```
#[macro_export]
macro_rules! regex_lazy {
    ($pattern:expr) => {{
        static REGEX: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new($pattern).unwrap());
        &*REGEX
    }};
}

/// Macro for getting regex from global cache.
///
/// # Panics
///
/// Panics if the regex pattern is invalid. This is acceptable for static patterns
/// where we want to fail fast during development.
#[macro_export]
macro_rules! regex_cached {
    ($pattern:expr) => {{ $crate::utils::regex_cache::get_cached_regex($pattern).expect("Failed to compile regex") }};
}

// Also make the macro available directly from this module

// =============================================================================
// URL REGEX PATTERNS - Centralized URL Detection
// =============================================================================
//
// ## Pattern Hierarchy (use the most specific pattern for your needs):
//
// | Pattern              | Use Case                                    | Parens | Trailing Punct |
// |----------------------|---------------------------------------------|--------|----------------|
// | URL_STANDARD_REGEX   | MD034 bare URL detection with auto-fix      | Yes    | Captured*      |
// | URL_WWW_REGEX        | www.domain URLs without protocol            | Yes    | Captured*      |
// | URL_IPV6_REGEX       | IPv6 URLs like https://[::1]/path           | Yes    | Captured*      |
// | URL_QUICK_CHECK_REGEX| Fast early-exit check (contains URL?)       | N/A    | N/A            |
// | URL_SIMPLE_REGEX     | Content detection, line length exemption    | No     | Excluded       |
//
// *Trailing punctuation is captured by the regex; use trim_trailing_punctuation() to clean.
//
// ## Design Principles:
// 1. Parentheses in paths are allowed for Wikipedia-style URLs (Issue #240)
// 2. Host portion excludes / so path is captured separately
// 3. Unbalanced trailing parens are handled by trim_trailing_punctuation()
// 4. All patterns exclude angle brackets <> to avoid matching autolinks
//
// ## URL Structure: protocol://host[:port][/path][?query][#fragment]

/// Simple URL pattern for content detection.
///
/// Less strict pattern that excludes trailing sentence punctuation (.,).
/// Use for line length exemption checks or content characteristic detection
/// where you just need to know if a URL exists, not extract it precisely.
pub const URL_SIMPLE_STR: &str = r#"(?:https?|ftps?|ftp)://[^\s<>]+[^\s<>.,]"#;

// Pre-compiled static patterns for performance

/// Simple URL regex - for content detection and line length exemption.
/// See [`URL_SIMPLE_STR`] for documentation.
pub static URL_SIMPLE_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(URL_SIMPLE_STR).unwrap());

/// Alias for `URL_SIMPLE_REGEX`. Used by MD013 for line length exemption.
pub static URL_PATTERN: LazyLock<Regex> = LazyLock::new(|| URL_SIMPLE_REGEX.clone());

// List patterns
pub static UNORDERED_LIST_MARKER_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\s*)([*+-])(\s+)").unwrap());
pub static ORDERED_LIST_MARKER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)(\d+)([.)])(\s+)").unwrap());

// Emphasis patterns

// Blockquote patterns
pub static BLOCKQUOTE_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\s*>+\s*)").unwrap());

/// Check if a line is blank in the context of blockquotes.
///
/// A line is considered "blank" if:
/// - It's empty or contains only whitespace
/// - It's a blockquote continuation line with no content (e.g., ">", ">>", "> ")
///
/// This is essential for rules like MD058 (blanks-around-tables), MD065 (blanks-around-horizontal-rules),
/// and any other rule that needs to detect blank lines that might be inside blockquotes.
///
/// # Examples
/// ```
/// use rumdl_lib::utils::regex_cache::is_blank_in_blockquote_context;
///
/// assert!(is_blank_in_blockquote_context(""));           // Empty line
/// assert!(is_blank_in_blockquote_context("   "));        // Whitespace only
/// assert!(is_blank_in_blockquote_context(">"));          // Blockquote continuation
/// assert!(is_blank_in_blockquote_context("> "));         // Blockquote with trailing space
/// assert!(is_blank_in_blockquote_context(">>"));         // Nested blockquote
/// assert!(is_blank_in_blockquote_context("> > "));       // Spaced nested blockquote
/// assert!(!is_blank_in_blockquote_context("> text"));    // Blockquote with content
/// assert!(!is_blank_in_blockquote_context("text"));      // Regular text
/// ```
pub(crate) fn is_blank_in_blockquote_context(line: &str) -> bool {
    // Peel one `\s*>+\s*` prefix at a time. A line is blank-in-blockquote iff,
    // after removing every blockquote prefix, nothing but whitespace remains.
    // This is iterative rather than recursive so a line of hundreds of thousands
    // of spaced markers ("> > > ...") cannot overflow the stack: each prefix
    // match consumes at least one `>`, so `rest` strictly shrinks every pass.
    let mut rest = line;
    loop {
        if rest.trim().is_empty() {
            return true;
        }
        match BLOCKQUOTE_PREFIX_RE.find(rest) {
            // `>+` guarantees the match consumes at least one byte; the explicit
            // `end > 0` guard keeps the loop total even if the pattern changes.
            Some(m) if m.end() > 0 => rest = &rest[m.end()..],
            _ => return false,
        }
    }
}

// MD013 specific patterns
pub static IMAGE_REF_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^!\[.*?\]\[.*?\]$").unwrap());
pub static LINK_REF_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\[.*?\]:\s*\S+(\s+["'(].*)?\s*$"#).unwrap());

// Email pattern
pub static EMAIL_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap());

// Wiki-style links: [[wiki]] or [[wiki|display text]]
pub static WIKI_LINK_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\]]+)\]\]").unwrap());

// Math formulas: $inline$ or $$display$$
pub static INLINE_MATH_REGEX: LazyLock<FancyRegex> =
    LazyLock::new(|| FancyRegex::new(r"(?<!\$)\$(?!\$)([^\$]+)\$(?!\$)").unwrap());
pub static DISPLAY_MATH_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\$([^\$]+)\$\$").unwrap());

// Emoji shortcodes: :emoji:
pub static EMOJI_SHORTCODE_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":([a-zA-Z0-9_+-]+):").unwrap());

// HTML tags (opening, closing, self-closing)
pub static HTML_TAG_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"</?[a-zA-Z][^>]*>|<[a-zA-Z][^>]*/\s*>").unwrap());

// HTML entities: named (&nbsp;), decimal (&#59;) and hexadecimal (&#x3B; or &#X3B;)
pub static HTML_ENTITY_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&[a-zA-Z][a-zA-Z0-9]*;|&#\d+;|&#[xX][0-9a-fA-F]+;").unwrap());

// Hugo/Go template shortcodes: {{< figure ... >}} and {{% shortcode %}}
// Matches both delimiters: {{< ... >}} (shortcode) and {{% ... %}} (template)
// Handles multi-line content with embedded quotes and newlines
pub static HUGO_SHORTCODE_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{[<%][\s\S]*?[%>]\}\}").unwrap());
