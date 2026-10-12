//! HTML with a `markdown` attribute detection
//!
//! Both Python-Markdown's `md_in_html` extension (MkDocs) and kramdown (Jekyll)
//! let an element opt its content into Markdown parsing with a `markdown`
//! attribute:
//! - `<div class="grid cards" markdown>` - Material grid cards
//! - `<div markdown="1">`, `<details markdown="block">`, `<div markdown="span">`
//! - `<p markdown="1">` and `<h5 markdown="1">` - common in Jekyll docs
//!
//! `<div markdown="0">` is kramdown's opposite and declares the content to be
//! raw HTML, so it does not open a block here.

/// Elements a `markdown` attribute can open a Markdown block on.
///
/// Derived from Python-Markdown's `markdown.util.BLOCK_LEVEL_ELEMENTS`, minus
/// the two groups `md_in_html` excludes from `span_and_blocks_tags`:
///
/// - its raw tags (`canvas`, `math`, `option`, `pre`, `script`, `style`,
///   `textarea`), whose content is never parsed - and four of which CommonMark
///   itself treats as raw-text HTML blocks under every flavor;
/// - its empty tag `hr`, which holds no content, so a tracked block opened on it
///   would never find a closing tag and would swallow the rest of the document.
///
/// kramdown accepts the attribute on any element at all, so this is the narrower
/// of the two rules.
const MARKDOWN_ATTRIBUTE_ELEMENTS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "colgroup",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "group",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "iframe",
    "legend",
    "li",
    "main",
    "map",
    "menu",
    "nav",
    "noscript",
    "object",
    "ol",
    "output",
    "p",
    "progress",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
    "video",
];

/// Name of the element a line opens, when that element declares its content to
/// be Markdown.
///
/// Attributes are read with their quoting honoured, so the word `markdown`
/// sitting inside another attribute's value is not an opt-in: neither
/// `<div class="markdown-body">` nor `<video title="editing markdown files">`
/// opens a block. The tag has to close on this line to open anything.
fn markdown_html_open_tag(line: &str) -> Option<String> {
    let line = line.trim_start();
    let bytes = line.as_bytes();
    if bytes.first() != Some(&b'<') || !bytes.get(1).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }

    let mut i = 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
        i += 1;
    }
    let tag = line[1..i].to_ascii_lowercase();
    if !MARKDOWN_ATTRIBUTE_ELEMENTS.contains(&tag.as_str()) {
        return None;
    }

    // The last `markdown` attribute wins, matching how a browser resolves a
    // repeated attribute.
    let mut declared: Option<&str> = None;
    while i < bytes.len() {
        match bytes[i] {
            b'>' => {
                return declared.filter(|value| *value != "0").map(|_| tag);
            }
            b' ' | b'\t' | b'/' => i += 1,
            _ => {
                let name_start = i;
                while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'=' | b'>' | b'/') {
                    i += 1;
                }
                let name = &line[name_start..i];

                let mut value = "";
                let mut j = i;
                while j < bytes.len() && matches!(bytes[j], b' ' | b'\t') {
                    j += 1;
                }
                if bytes.get(j) == Some(&b'=') {
                    j += 1;
                    while j < bytes.len() && matches!(bytes[j], b' ' | b'\t') {
                        j += 1;
                    }
                    match bytes.get(j) {
                        Some(&quote @ (b'"' | b'\'')) => {
                            let start = j + 1;
                            // An unterminated quote runs off the end of the line, so
                            // there is no complete opening tag here.
                            let end = start + line[start..].find(quote as char)?;
                            value = &line[start..end];
                            j = end + 1;
                        }
                        _ => {
                            let start = j;
                            while j < bytes.len() && !matches!(bytes[j], b' ' | b'\t' | b'>') {
                                j += 1;
                            }
                            value = &line[start..j];
                        }
                    }
                    i = j;
                }

                if name.eq_ignore_ascii_case("markdown") {
                    declared = Some(value);
                }
            }
        }
    }
    None
}

/// Track state for markdown HTML block parsing
#[derive(Debug, Default)]
pub struct MarkdownHtmlTracker {
    /// Stack of open tags (tag name, depth at that level)
    tag_stack: Vec<(String, usize)>,
    /// Current nesting depth
    depth: usize,
}

impl MarkdownHtmlTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Process a line and return whether the line is inside a markdown HTML block.
    /// Returns true if:
    /// - This line opens a new markdown HTML block
    /// - This line is part of an existing markdown HTML block (even if it closes it)
    pub(crate) fn process_line(&mut self, line: &str) -> bool {
        let trimmed = line.trim();

        // Check for opening tag
        if let Some(tag) = markdown_html_open_tag(line) {
            // Check if this line also closes the tag (self-contained)
            let closes_here = Self::count_closes_lowered(&line.to_lowercase(), &tag) > 0;

            self.depth += 1;
            self.tag_stack.push((tag, self.depth));
            if closes_here {
                self.depth -= 1;
                self.tag_stack.pop();
            }
            return true;
        }

        // If we're inside a markdown HTML block at the start of this line
        if !self.tag_stack.is_empty() {
            // Lowercase the line once for all tag comparisons
            let line_lower = trimmed.to_lowercase();

            // Collect tag names by reference before mutating depth
            let tags: Vec<String> = self.tag_stack.iter().map(|(tag, _)| tag.clone()).collect();
            for tag in &tags {
                let opens = Self::count_opens_lowered(&line_lower, tag);
                let closes = Self::count_closes_lowered(&line_lower, tag);

                self.depth += opens;

                for _ in 0..closes {
                    if self.depth > 0 {
                        self.depth -= 1;
                    }
                }
            }

            // Clean up stack when depth reaches initial level
            while let Some((_, start_depth)) = self.tag_stack.last() {
                if self.depth < *start_depth {
                    self.tag_stack.pop();
                } else {
                    break;
                }
            }

            // Return true because this line was inside the block at the start
            // (even if it also closes the block)
            return true;
        }

        false
    }

    /// Count opening tags of a specific type in a pre-lowercased line.
    /// `tag` is already lowercase (stored that way in `tag_stack`).
    fn count_opens_lowered(line_lower: &str, tag: &str) -> usize {
        let open_pattern = format!("<{tag}");
        let mut count = 0;
        let mut search_start = 0;

        while let Some(pos) = line_lower[search_start..].find(&open_pattern) {
            let abs_pos = search_start + pos;
            let after_tag = abs_pos + open_pattern.len();

            // Verify it's a tag boundary (followed by whitespace, >, or /)
            if after_tag >= line_lower.len()
                || line_lower[after_tag..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/')
            {
                count += 1;
            }
            search_start = after_tag;
        }
        count
    }

    /// Count closing tags of a specific type in a pre-lowercased line.
    /// `tag` is already lowercase (stored that way in `tag_stack`).
    fn count_closes_lowered(line_lower: &str, tag: &str) -> usize {
        let close_pattern = format!("</{tag}");
        let mut count = 0;
        let mut search_start = 0;

        while let Some(pos) = line_lower[search_start..].find(&close_pattern) {
            let abs_pos = search_start + pos;
            let after_tag = abs_pos + close_pattern.len();

            // Find the closing > (may have whitespace before it)
            if let Some(rest) = line_lower.get(after_tag..)
                && rest.trim_start().starts_with('>')
            {
                count += 1;
            }
            search_start = after_tag;
        }
        count
    }
}
