//! Utilities for extracting custom header IDs from various Markdown flavors
//!
//! This module supports multiple syntax formats for custom header IDs:
//!
//! ## Kramdown Format
//! - `{#custom-id}` - Simple ID without colon
//! - Example: `# Header {#my-id}`
//!
//! ## Python-markdown attr-list Format
//! - `{:#custom-id}` - ID with colon, no spaces
//! - `{: #custom-id}` - ID with colon and spaces
//! - `{: #custom-id .class}` - ID with classes
//! - `{: #custom-id .class data="value"}` - ID with full attributes
//! - Example: `# Header {: #my-id .highlight}`
//!
//! ## Position Support
//! - Inline: `# Header {#id}` (all formats)
//! - Next-line: Jekyll/kramdown style where attr-list appears on the line after the header
//!   ```markdown
//!   # Header
//!   {#next-line-id}
//!   ```
//!
//! ## HTML anchors
//! - `<a id="custom-id"></a>` or `<a name="custom-id"></a>` beside the heading text
//! - Example: `## <a name="my-id"></a>Header`
//!
//! An empty anchor element is stripped from the heading text but, unlike an
//! attr-list ID, it does not replace the slug generated from the text: the
//! rendered heading answers to both. Tags are read in source order as a browser
//! tokenizes them, so an `<a>` written inside another tag's attribute value is
//! part of that value. Anchor markup inside a code span or an HTML comment, or
//! whose `<` is backslash-escaped, is heading text and defines nothing.
//!
//! The module provides functions to detect and extract IDs from both inline
//! and standalone (next-line) attr-list syntax.

use regex::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;

/// The name of an HTML tag: a letter, then letters, digits and hyphens.
pub const HTML_TAG_NAME_PATTERN: &str = "[A-Za-z][A-Za-z0-9-]*";

/// Attribute list of an inline HTML open tag as CommonMark defines it.
///
/// A quoted value may contain `>` without ending the tag, which is why a tag
/// cannot be delimited by searching for the next `>`.
pub const HTML_TAG_ATTRIBUTES_PATTERN: &str = r#"(?:\s+[^\s"'>/=]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s"'=<>`]+))?)*"#;

/// An inline HTML open tag, self-closing or not, with its name captured.
pub static HTML_OPEN_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"<({HTML_TAG_NAME_PATTERN}){HTML_TAG_ATTRIBUTES_PATTERN}\s*/?>"
    ))
    .unwrap()
});

/// The closing tag of an `<a>` element at the start of the text, with any
/// whitespace before it.
static HTML_ANCHOR_CLOSING_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\s*</a\s*>").unwrap());

/// Pattern for custom header IDs supporting both kramdown and python-markdown attr-list formats
/// Supports: {#id}, { #id }, {:#id}, {: #id } and full attr-list with classes/attributes
/// Must contain #id but can have other attributes: {: #id .class data="value" }
/// More conservative: only matches when there's actually a hash followed by valid ID characters
static HEADER_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\{\s*:?\s*([^}]*?#[^}]*?)\s*\}\s*$").unwrap());

/// Pattern to validate that an ID contains only valid characters
static ID_VALIDATE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_\-:]+$").unwrap());

/// Pattern for standalone attr-list lines (Jekyll/kramdown style on line after heading)
/// Matches lines that are just attr-list syntax: {#id}, {: #id .class }, etc.
static STANDALONE_ATTR_LIST_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\{\s*:?\s*([^}]*#[a-zA-Z0-9_\-:]+[^}]*)\s*\}\s*$").unwrap());

/// The content of a heading split into what a reader sees, what a renderer
/// slugs, and the custom ID its attribute list declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingText {
    /// The text as a reader sees it: anchor elements and the attribute list
    /// removed, one of the two spaces an element sat between gone with it, and
    /// both ends trimmed.
    pub(crate) text: String,
    /// The text a slug is generated from: the same as `text`, except that every
    /// space an anchor element leaves behind stays. A browser shows
    /// `## Alpha <a id="x"></a>` as "Alpha" and `## Foo <a id="x"></a> Bar` as
    /// "Foo Bar", but GitHub and kramdown slug them to `alpha-` and `foo--bar`;
    /// each anchor style decides for itself whether to trim and collapse.
    pub(crate) slug_text: String,
    /// The ID from a `{#id}` or `{: #id}` attribute list, if any.
    pub(crate) custom_id: Option<String>,
}

/// Split heading content into display text, slug text and custom ID.
///
/// See [`HeadingText`] for what each part holds. The attribute-list formats are
/// those of [`extract_header_id`], which returns the display text and ID only.
pub(crate) fn extract_heading_text(line: &str) -> HeadingText {
    // An empty anchor element beside the text (`## <a name="foo"></a>Heading`) is
    // markup, not heading text.
    let (line, seams) = strip_html_anchor_elements(line);
    let line = line.as_ref();

    let (slug_text, custom_id) = match custom_id_at_end(line) {
        Some((attr_list_start, id)) => (line[..attr_list_start].trim_end(), Some(id)),
        None => (line, None),
    };
    HeadingText {
        text: display_text(slug_text, &seams),
        slug_text: slug_text.to_string(),
        custom_id,
    }
}

/// The ID declared by an attribute list that ends `line`, with the byte offset
/// where the attribute list starts. An attribute list whose ID fails validation
/// declares nothing and stays heading text.
fn custom_id_at_end(line: &str) -> Option<(usize, String)> {
    let captures = HEADER_ID_PATTERN.captures(line)?;
    let attr_list_start = captures.get(0)?.start();
    let attr_str = captures.get(1)?.as_str().trim();
    let hash_pos = attr_str.find('#')?;
    let after_hash = &attr_str[hash_pos + 1..];

    // In the simple kramdown form `{#id}` the ID runs to the end. In the full
    // attr-list form `{: #id .class key="value"}` it ends at the next attribute
    // (whitespace), class (dot) or value (equals sign).
    let is_simple_format = !attr_str.contains(' ') && !attr_str.contains('=') && attr_str.starts_with('#');
    let potential_id = if is_simple_format {
        after_hash
    } else {
        match after_hash.find(|c: char| c.is_whitespace() || c == '.' || c == '=') {
            Some(delimiter_pos) => &after_hash[..delimiter_pos],
            None => after_hash,
        }
    };

    (!potential_id.is_empty() && ID_VALIDATE_PATTERN.is_match(potential_id))
        .then(|| (attr_list_start, potential_id.to_string()))
}

/// Where the display text of `line` ends: before its trailing whitespace, the
/// attribute list a custom ID sits in and an empty anchor element at the end,
/// each stripped as the one after it exposes it. Zero when the line holds no
/// display text.
pub(crate) fn heading_text_end(line: &str) -> usize {
    let mut end = line.len();
    loop {
        end = line[..end].trim_end().len();
        if let Some((attr_list_start, _)) = custom_id_at_end(&line[..end]) {
            end = attr_list_start;
        } else if let Some((range, _)) = empty_anchor_elements(&line[..end]).pop()
            && range.end == end
        {
            end = range.start;
        } else {
            return end;
        }
    }
}

/// Remove the empty `<a>` elements that give a heading its anchors.
///
/// Exactly the element's bytes go, so the whitespace beside it stays part of
/// the text: `Foo<a id="x"></a> Bar` reads "Foo Bar", and whitespace left at
/// either end stays for the slug, since GitHub and kramdown slug
/// `Alpha <a id="x"></a>` as `#alpha-`. The offsets returned beside the text
/// are where each element sat, for [`display_text`].
fn strip_html_anchor_elements(text: &str) -> (Cow<'_, str>, Vec<usize>) {
    let anchors = empty_anchor_elements(text);
    if anchors.is_empty() {
        return (Cow::Borrowed(text), Vec::new());
    }

    let mut stripped = String::with_capacity(text.len());
    let mut seams = Vec::with_capacity(anchors.len());
    let mut copied_up_to = 0;
    for (range, _) in anchors {
        stripped.push_str(&text[copied_up_to..range.start]);
        seams.push(stripped.len());
        copied_up_to = range.end;
    }
    stripped.push_str(&text[copied_up_to..]);
    (Cow::Owned(stripped), seams)
}

/// The text a reader sees once the elements that sat at `seams` are gone.
///
/// An element written between two spaces (`Foo <a id="x"></a> Bar`) leaves
/// them adjacent, and a browser shows adjacent spaces as one, so one of each
/// such pair goes with the element. Spaces the author wrote in a run stay, and
/// whitespace at either end is trimmed as CommonMark trims a heading.
fn display_text(slug_text: &str, seams: &[usize]) -> String {
    let mut text = slug_text.to_string();
    for &seam in seams.iter().rev() {
        if seam == 0 || seam >= text.len() {
            continue;
        }
        let is_blank = |byte: u8| matches!(byte, b' ' | b'\t');
        if is_blank(text.as_bytes()[seam - 1]) && is_blank(text.as_bytes()[seam]) {
            text.remove(seam);
        }
    }
    text.trim().to_string()
}

/// The empty `<a>` elements of `text` in source order, each as the byte range
/// of the whole element and its open tag.
///
/// Tags are read the way a browser tokenizes them, one after another, so an
/// `<a>` written inside another tag's attribute value is part of that value
/// and no element. A tag inside a code span or an HTML comment, or whose `<`
/// is backslash-escaped, is heading text; the scan resumes just past its `<`,
/// since the text it was taken for may hold a real tag of its own.
fn empty_anchor_elements(text: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    if !text.contains('<') {
        return Vec::new();
    }

    let opaque = opaque_ranges(text);
    let mut anchors = Vec::new();
    let mut pos = 0;
    while let Some(tag) = HTML_OPEN_TAG.captures_at(text, pos) {
        let open_tag = tag.get(0).unwrap();
        if is_within(&opaque, open_tag.start()) || is_backslash_escaped(text, open_tag.start()) {
            pos = open_tag.start() + 1;
            continue;
        }
        pos = open_tag.end();

        if !tag[1].eq_ignore_ascii_case("a") {
            continue;
        }
        if let Some(closing_tag) = HTML_ANCHOR_CLOSING_TAG.find(&text[open_tag.end()..]) {
            pos = open_tag.end() + closing_tag.end();
            anchors.push((open_tag.start()..pos, open_tag.as_str()));
        }
    }

    anchors
}

/// Byte ranges of `text` in which a tag is not an element: code spans, HTML
/// comments and images.
///
/// A code span opens with a run of backticks and closes with a run of exactly
/// the same length; an opener that never closes is literal backticks. A comment
/// runs from `<!--` to the next `-->`, or to the end of the text; `<!-->` and
/// `<!--->` are complete comments, so the search for the closer starts right
/// after `<!`. An image's description becomes its alt text, in which markup is
/// escaped, so `![<a id="x"></a>](img.png)` renders no element and no target.
fn opaque_ranges(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut pos = 0;

    while pos < bytes.len() {
        if bytes[pos] == b'`' {
            let run_end = pos + bytes[pos..].iter().take_while(|&&b| b == b'`').count();
            let run_len = run_end - pos;
            match closing_backtick_run(bytes, run_end, run_len) {
                Some(close_start) => {
                    ranges.push((pos, close_start + run_len));
                    pos = close_start + run_len;
                }
                None => pos = run_end,
            }
        } else if bytes[pos..].starts_with(b"<!--") {
            let end = text[pos + 2..]
                .find("-->")
                .map_or(bytes.len(), |offset| pos + 2 + offset + 3);
            ranges.push((pos, end));
            pos = end;
        } else if bytes[pos..].starts_with(b"![")
            && !is_backslash_escaped(text, pos)
            && !is_backslash_escaped(text, pos + 1)
            && let Some(end) = image_end(text, pos)
        {
            ranges.push((pos, end));
            pos = end;
        } else {
            pos += 1;
        }
    }

    ranges
}

/// Offset just past the inline image whose `![` starts at `start`, if its
/// description closes and a destination follows.
///
/// The description may nest brackets and escape them with a backslash. The
/// destination is `(...)` with balanced parentheses or a `[label]`. A bare
/// `![text]` is a collapsed or shortcut reference image only if the document
/// defines the label, which this text-level scan cannot see, so it stays text.
fn image_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let description_end = balanced_end(bytes, start + 2, b'[', b']')?;
    match bytes.get(description_end) {
        Some(b'(') => balanced_end(bytes, description_end + 1, b'(', b')'),
        Some(b'[') => balanced_end(bytes, description_end + 1, b'[', b']'),
        _ => None,
    }
}

/// Offset just past the `close` byte that balances an `open` consumed before
/// `from`, honouring backslash escapes; `None` when it never closes.
fn balanced_end(bytes: &[u8], from: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 1usize;
    let mut pos = from;
    while pos < bytes.len() {
        match bytes[pos] {
            b'\\' => pos += 1,
            byte if byte == open => depth += 1,
            byte if byte == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(pos + 1);
                }
            }
            _ => {}
        }
        pos += 1;
    }
    None
}

/// Start of the first backtick run of exactly `run_len` after `from`, if any.
fn closing_backtick_run(bytes: &[u8], from: usize, run_len: usize) -> Option<usize> {
    let mut pos = from;
    while pos < bytes.len() {
        if bytes[pos] != b'`' {
            pos += 1;
            continue;
        }
        let run_end = pos + bytes[pos..].iter().take_while(|&&b| b == b'`').count();
        if run_end - pos == run_len {
            return Some(pos);
        }
        pos = run_end;
    }
    None
}

fn is_within(ranges: &[(usize, usize)], pos: usize) -> bool {
    ranges.iter().any(|&(start, end)| start <= pos && pos < end)
}

/// Whether the character at byte `pos` of `text` is backslash-escaped.
///
/// CommonMark reads `\<` as a literal `<` and `\\<` as a literal backslash
/// followed by a tag, so the character is escaped when an odd number of
/// backslashes precede it.
pub(crate) fn is_backslash_escaped(text: &str, pos: usize) -> bool {
    text.as_bytes()[..pos].iter().rev().take_while(|&&b| b == b'\\').count() % 2 == 1
}

/// Check if a line is a standalone attr-list (Jekyll/kramdown style)
///
/// This detects attr-list syntax that appears on its own line, typically
/// the line after a header to provide additional attributes.
///
/// # Examples
/// ```
/// use rumdl_lib::utils::header_id_utils::is_standalone_attr_list;
///
/// assert!(is_standalone_attr_list("{#custom-id}"));
/// assert!(is_standalone_attr_list("{: #spaced .class }"));
/// assert!(!is_standalone_attr_list("Some text {#not-standalone}"));
/// assert!(!is_standalone_attr_list(""));
/// ```
pub(crate) fn is_standalone_attr_list(line: &str) -> bool {
    STANDALONE_ATTR_LIST_PATTERN.is_match(line)
}

/// Extract ID from a standalone attr-list line
///
/// Returns the ID if the line is a valid standalone attr-list with an ID.
///
/// # Examples
/// ```
/// use rumdl_lib::utils::header_id_utils::extract_standalone_attr_list_id;
///
/// assert_eq!(extract_standalone_attr_list_id("{#custom-id}"), Some("custom-id".to_string()));
/// assert_eq!(extract_standalone_attr_list_id("{: #spaced .class }"), Some("spaced".to_string()));
/// assert_eq!(extract_standalone_attr_list_id("not an attr-list"), None);
/// ```
pub(crate) fn extract_standalone_attr_list_id(line: &str) -> Option<String> {
    if let Some(captures) = STANDALONE_ATTR_LIST_PATTERN.captures(line)
        && let Some(attr_content) = captures.get(1)
    {
        let attr_str = attr_content.as_str().trim();

        // Use the same logic as extract_header_id for consistency
        if let Some(hash_pos) = attr_str.find('#') {
            let after_hash = &attr_str[hash_pos + 1..];

            // Check if this looks like a simple kramdown ID: {#id} with no spaces or attributes
            let is_simple_format = !attr_str.contains(' ') && !attr_str.contains('=') && attr_str.starts_with('#');

            if is_simple_format {
                // Simple format: entire content after # should be the ID
                let potential_id = after_hash;
                if ID_VALIDATE_PATTERN.is_match(potential_id) && !potential_id.is_empty() {
                    return Some(potential_id.to_string());
                }
            } else {
                // Complex format: find proper delimiters (space for next attribute, dot for class)
                if let Some(delimiter_pos) = after_hash.find(|c: char| c.is_whitespace() || c == '.' || c == '=') {
                    let potential_id = &after_hash[..delimiter_pos];
                    if ID_VALIDATE_PATTERN.is_match(potential_id) && !potential_id.is_empty() {
                        return Some(potential_id.to_string());
                    }
                } else {
                    // No delimiter found in complex format, ID goes to end
                    let potential_id = after_hash;
                    if ID_VALIDATE_PATTERN.is_match(potential_id) && !potential_id.is_empty() {
                        return Some(potential_id.to_string());
                    }
                }
            }
        }
    }
    None
}
