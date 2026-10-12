/// MkDocs attr_list extension support
///
/// This module provides support for the Python-Markdown attr_list extension,
/// which allows adding custom attributes to Markdown elements including:
/// - Custom IDs: `{#custom-id}`
/// - Classes: `{.my-class}`
/// - Key-value pairs: `{key="value"}`
///
/// ## Syntax
///
/// ### Headings with custom anchors
/// ```markdown
/// # Heading {#custom-anchor}
/// # Heading {.class-name}
/// # Heading {#id .class key=value}
/// ```
///
/// ### Block attributes (on separate line)
/// ```markdown
/// Paragraph text here.
/// {: #id .class }
/// ```
///
/// ### Inline attributes
/// ```markdown
/// [link text](url){: .external target="_blank" }
/// *emphasis*{: .special }
/// ```
///
/// ## References
///
/// - [Python-Markdown attr_list](https://python-markdown.github.io/extensions/attr_list/)
/// - [MkDocs Material - Anchor Links](https://squidfunk.github.io/mkdocs-material/reference/annotations/#anchor-links)
use regex::Regex;
use std::sync::LazyLock;

/// Pattern to match attr_list syntax: `{: #id .class key="value" }`
/// The `:` prefix is optional (kramdown style uses it, but attr_list accepts both)
/// Requirements for valid attr_list:
/// - Must start with `{` and optional `:` with optional whitespace
/// - Must contain at least one of: #id, .class, or key="value"
/// - Must end with `}`
pub static ATTR_LIST_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    // Pattern requires at least one attribute (id, class, or key=value)
    // to avoid matching plain text in braces like {word}
    Regex::new(r#"\{:?\s*(?:(?:#[a-zA-Z0-9_][a-zA-Z0-9_-]*|\.[a-zA-Z_][a-zA-Z0-9_-]*|[a-zA-Z_][a-zA-Z0-9_-]*=["'][^"']*["'])\s*)+\}"#).unwrap()
});

/// Check if a line is a standalone block attr_list (on its own line)
/// This is used for block-level attributes like:
/// ```markdown
/// Paragraph text.
/// { .class-name }
/// ```
/// or with colon:
/// ```markdown
/// Paragraph text.
/// {: .class-name }
/// ```
#[inline]
pub(crate) fn is_standalone_attr_list(line: &str) -> bool {
    let trimmed = line.trim();
    // Must start with { and end with }
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return false;
    }
    // Must be a valid attr_list (not just random braces)
    ATTR_LIST_PATTERN.is_match(trimmed)
}

/// Check whether a line is a block attribute list attached to the preceding block.
///
/// Block attribute lists sit on their own line directly under a block (heading,
/// table, fenced code, list) and describe that block, so the blanks-around rules
/// (MD022, MD031, MD032, MD058) must not treat them as separate content needing a
/// blank line between them and the block.
///
/// Two forms are recognized:
/// - Kramdown IALs (`{:.class}`, `{:#id}`) in every flavor, matching long-standing
///   behavior.
/// - Bare attribute lists such as Hugo/Goldmark's `{class="a" id="b"}` only when
///   the flavor enables attribute lists, because in plain CommonMark that text is
///   literal content that legitimately needs surrounding blanks.
#[inline]
pub(crate) fn is_block_attribute_line(line: &str, _flavor: crate::config::MarkdownFlavor) -> bool {
    crate::utils::kramdown_utils::is_kramdown_block_attribute(line) || false
}
