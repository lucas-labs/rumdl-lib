//! Utilities for determining if a position in markdown should be skipped from processing
//!
//! This module provides centralized context detection for various markdown constructs
//! that should typically be skipped when processing rules.

use crate::config::MarkdownFlavor;
use crate::lint_context::{HtmlTag, LintContext};
use crate::utils::mkdocs_icons;
use regex::Regex;
use std::sync::LazyLock;

/// Enhanced inline math pattern that handles both single $ and double $$ delimiters.
/// Matches:
/// - Display math: $$...$$ (zero or more non-$ characters)
/// - Inline math: $...$ (zero or more non-$ non-newline characters)
///
/// The display math pattern is tried first to correctly handle $$content$$.
/// Critically, both patterns allow ZERO characters between delimiters,
/// so empty math like $$ or $ $ is consumed and won't pair with other $ signs.
static INLINE_MATH_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\$[^$]*\$\$|\$[^$\n]*\$").unwrap());

/// Range representing a span of bytes (start inclusive, end exclusive)
#[derive(Debug, Clone, Copy)]
pub struct ByteRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// What a scan of the content found for HTML comments.
#[derive(Debug, Default)]
pub struct HtmlCommentScan {
    /// Byte ranges of the complete comments, in document order.
    pub(crate) ranges: Vec<ByteRange>,
    /// Byte offset of a `<!--` that no `-->` closes. At most one is reported:
    /// once the document has no closer left, no later opener can close either.
    pub(crate) unterminated: Option<usize>,
}

/// Scan for HTML comments, keeping the position of an opener that never closes.
///
/// Same scan as [`compute_html_comment_ranges_filtered`], which discards the
/// unterminated opener. MD086 reports it, so it is carried out of the scan
/// rather than re-derived by a second, possibly disagreeing pass.
///
/// `scan_from` is the byte offset the document's body starts at, so front
/// matter can be excluded. A `<!--` in a YAML or TOML value is data, and
/// pairing it with a `-->` in the body would mark everything in between as a
/// comment, hiding it from every rule.
pub(crate) fn scan_html_comments(
    content: &str,
    code_span_ranges: &[(usize, usize)],
    code_block_ranges: &[(usize, usize)],
    scan_from: usize,
) -> HtmlCommentScan {
    let in_code = |pos: usize| {
        code_span_ranges.iter().any(|&(start, end)| pos >= start && pos < end)
            || code_block_ranges.iter().any(|&(start, end)| pos >= start && pos < end)
    };

    let mut ranges = Vec::new();
    let mut search_from = scan_from.min(content.len());
    while let Some(rel) = content[search_from..].find("<!--") {
        let open = search_from + rel;
        if in_code(open) {
            // Literal `<!--` inside code: not a comment opener.
            search_from = open + "<!--".len();
            continue;
        }
        // Find the next `-->` that is not itself inside code. The search starts
        // two bytes into the opener, because the opener's own trailing dashes can
        // serve as the closer's leading dashes: `<!-->` and `<!--->` are complete
        // comments, and a line holding one ends an HTML block.
        let mut close_from = open + 2;
        let end = loop {
            let Some(crel) = content[close_from..].find("-->") else {
                break None;
            };
            let close = close_from + crel;
            if in_code(close) {
                close_from = close + "-->".len();
                continue;
            }
            break Some(close + "-->".len());
        };
        match end {
            Some(end) => {
                ranges.push(ByteRange { start: open, end });
                search_from = end;
            }
            // Unterminated comment (no closer anywhere): it covers no range,
            // and no opener after it can close either, so the scan is done.
            None => {
                return HtmlCommentScan {
                    ranges,
                    unterminated: Some(open),
                };
            }
        }
    }
    HtmlCommentScan {
        ranges,
        unterminated: None,
    }
}

/// Re-resolve an unterminated `<!--` that an Obsidian comment hides.
///
/// [`scan_html_comments`] has to run before Obsidian comments are known,
/// because detecting those needs the HTML comment ranges. A `<!--` inside a
/// `%%` comment is hidden text rather than an opener, so when the reported one
/// falls in a comment the scan is repeated with those comments among the ranges
/// whose delimiters are literal. That resumes the search at the next opener
/// instead of suppressing the finding, which would hide a real unclosed comment
/// below the hidden one.
///
/// Only the opener is taken from the second scan. Skipping strictly more of the
/// content cannot turn up a closer the first scan missed, so the complete
/// comments it found still stand.
pub(crate) fn unterminated_html_comment_outside(
    reported: Option<usize>,
    hidden_ranges: &[(usize, usize)],
    content: &str,
    code_span_ranges: &[(usize, usize)],
    code_block_ranges: &[(usize, usize)],
    scan_from: usize,
) -> Option<usize> {
    let offset = reported?;
    if !hidden_ranges
        .iter()
        .any(|&(start, end)| offset >= start && offset < end)
    {
        return reported;
    }
    let mut literal_ranges = code_block_ranges.to_vec();
    literal_ranges.extend_from_slice(hidden_ranges);
    scan_html_comments(content, code_span_ranges, &literal_ranges, scan_from).unterminated
}

/// The range an unclosed `<!--` hides, if it hides anything.
///
/// Whether an unclosed opener swallows the text after it is a block-structure
/// question, so it is answered by the parser rather than by inspecting the line.
/// At the start of a line `<!--` opens an HTML block and everything to the end
/// of that block is comment text; mid-paragraph CommonMark has no comment to
/// open, so the marker renders literally and hides nothing.
///
/// The block is the unit because it is not always the rest of the document. An
/// opener inside a blockquote or a list item ends with its container, leaving
/// the content after it visible, and one indented four spaces is code and opens
/// no block at all. `html_blocks` are the parser's ranges, so each of those
/// falls out without a separate rule here.
///
/// The range starts at the opener, not at the block, because a block may have
/// begun earlier with a tag: in `<div>` followed by `<!--`, only the text from
/// the marker on is inside the comment.
pub(crate) fn unterminated_comment_range(opener: usize, html_blocks: &[(usize, usize)]) -> Option<ByteRange> {
    html_blocks
        .iter()
        .find(|&&(start, end)| opener >= start && opener < end)
        .map(|&(_, end)| ByteRange { start: opener, end })
}

/// Check if a byte position is within any of the pre-computed HTML comment ranges
/// Uses binary search for O(log n) complexity
pub(crate) fn is_in_html_comment_ranges(ranges: &[ByteRange], byte_pos: usize) -> bool {
    // Binary search to find a range that might contain byte_pos
    ranges
        .binary_search_by(|range| {
            if byte_pos < range.start {
                std::cmp::Ordering::Greater
            } else if byte_pos >= range.end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Check if a line's content is ENTIRELY within a single HTML comment.
///
/// Callers must pass the byte range of the line's *content* (leading and
/// trailing whitespace trimmed off), not the raw line bounds: an indented
/// comment begins at the `<!--` after the indent, so passing the column-0
/// line start would place `content_start` before the comment range and the
/// line would never be recognised as being inside the comment.
///
/// Returns true only if both `content_start` AND `content_end` fall within the
/// same comment range.
pub(crate) fn is_line_entirely_in_html_comment(ranges: &[ByteRange], content_start: usize, content_end: usize) -> bool {
    for range in ranges {
        // If the content start is within this range, check if the content end is also within it
        if content_start >= range.start && content_start < range.end {
            return content_end <= range.end;
        }
    }
    false
}

/// Check if a byte position is within a JSX expression (MDX: {expression})
#[inline]
pub(crate) fn is_in_jsx_expression(_ctx: &LintContext, _byte_pos: usize) -> bool {
    false
}

/// Check if a byte position is within an MDX comment ({/* ... */})
#[inline]
pub(crate) fn is_in_mdx_comment(_ctx: &LintContext, _byte_pos: usize) -> bool {
    false
}

/// Check if a line should be skipped due to MkDocs snippet syntax
pub(crate) fn is_mkdocs_snippet_line(_line: &str, _flavor: MarkdownFlavor) -> bool {
    false
}

/// Check if a byte position is within a math context.
///
/// `$$...$$` display math is recognized only when it begins its line; a
/// mid-line or stray-prose `$$...$$` is a literal, not math. Single-`$` inline
/// spans are recognized anywhere. This keeps every math-aware rule agreeing on
/// what is math.
pub(crate) fn is_in_math_context(ctx: &LintContext, byte_pos: usize) -> bool {
    // Use the cached ranges on the context; recomputing math_byte_ranges(content)
    // on every call made callers that invoke this per element O(elements * content).
    ctx.math_byte_ranges()
        .iter()
        .any(|&(start, end)| byte_pos >= start && byte_pos < end)
}

/// Paired `$$ ... $$` display-math byte ranges, half-open `[start, end)`.
///
/// A block only *opens* on a `$$` that begins its line, ignoring leading
/// whitespace and blockquote markers (`>`); a stray `$$` mid-prose is a
/// literal, not a block opener. This keeps the byte-level result consistent
/// with the line-level `compute_math_block_line_map` guard. Once open, the
/// block *closes* on the next `$$` anywhere - even when that closing `$$`
/// shares its line with LaTeX content (`\end{cases}$$`) or trailing Markdown
/// prose. An opener with no matching closer is dropped, not treated as an
/// unterminated block that swallows the rest of the document.
pub(crate) fn math_block_ranges(content: &str) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let mut ranges = Vec::new();
    let mut open: Option<usize> = None;
    let mut line_start = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line_start = i + 1;
                i += 1;
            }
            b'$' if i + 1 < bytes.len() && bytes[i + 1] == b'$' => {
                match open {
                    None => {
                        // Open only when this `$$` is the first non-blank,
                        // non-blockquote content on its line.
                        let starts_line = bytes[line_start..i]
                            .iter()
                            .all(|&b| b == b' ' || b == b'\t' || b == b'>');
                        if starts_line {
                            open = Some(i);
                        }
                    }
                    Some(start) => {
                        ranges.push((start, i + 2));
                        open = None;
                    }
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    ranges
}

/// All math byte ranges in `content`: line-start `$$...$$` display blocks
/// plus single-`$` inline spans. Ranges are half-open `[start, end)` and may
/// be unordered relative to each other; membership is by `any`-containment.
///
/// Precompute this once when classifying many positions in one document
/// (e.g. every emphasis span). [`is_in_math_context`] is the single-shot
/// equivalent and is defined in terms of the same two sources.
pub(crate) fn math_byte_ranges(content: &str) -> Vec<(usize, usize)> {
    let mut ranges = math_block_ranges(content);
    for m in INLINE_MATH_REGEX.find_iter(content) {
        if content[m.start()..m.end()].starts_with("$$") {
            continue;
        }
        ranges.push((m.start(), m.end()));
    }
    ranges
}

/// Check if a line contains table syntax
pub(crate) fn is_table_line(line: &str) -> bool {
    let trimmed = line.trim();

    // Check for table separator line
    if trimmed
        .chars()
        .all(|c| c == '|' || c == '-' || c == ':' || c.is_whitespace())
        && trimmed.contains('|')
        && trimmed.contains('-')
    {
        return true;
    }

    // Check for table content line (starts and/or ends with |)
    if (trimmed.starts_with('|') || trimmed.ends_with('|')) && trimmed.matches('|').count() >= 2 {
        return true;
    }

    false
}

/// Check if a byte position is within an MkDocs icon shortcode
/// Icon shortcodes use format like `:material-check:`, `:octicons-mark-github-16:`
pub(crate) fn is_in_icon_shortcode(line: &str, position: usize, _flavor: MarkdownFlavor) -> bool {
    // Only skip for MkDocs flavor, but check pattern for all flavors
    // since emoji shortcodes are universal
    mkdocs_icons::is_in_any_shortcode(line, position)
}

/// Check if a byte position is within PyMdown extension markup
/// Includes: Keys (++ctrl+alt++), Caret (^text^), Insert (^^text^^), Mark (==text==)
///
/// For MkDocs flavor: supports all PyMdown extensions
/// For Obsidian flavor: only supports Mark (==highlight==) syntax
pub(crate) fn is_in_pymdown_markup(line: &str, position: usize, flavor: MarkdownFlavor) -> bool {
    let _ = (line, position, flavor);
    false
}

/// Check if a byte position is within flavor-specific markup
/// For MkDocs: icon shortcodes and PyMdown extensions
/// For Obsidian: highlight syntax (==text==)
pub(crate) fn is_in_mkdocs_markup(line: &str, position: usize, flavor: MarkdownFlavor) -> bool {
    if is_in_icon_shortcode(line, position, flavor) {
        return true;
    }
    if is_in_pymdown_markup(line, position, flavor) {
        return true;
    }
    false
}

/// Check if a byte position within a line is inside a backtick-delimited code span.
///
/// This is a line-level fallback for cases where pulldown-cmark's code span detection
/// misses spans due to table parsing interference (e.g., pipes inside code spans
/// in table rows cause pulldown-cmark to misidentify cell boundaries).
fn is_in_inline_code_on_line(line: &str, byte_pos: usize) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'`' {
            let open_start = i;
            let mut backtick_count = 0;
            while i < bytes.len() && bytes[i] == b'`' {
                backtick_count += 1;
                i += 1;
            }

            // Search for matching closing backticks
            let mut j = i;
            while j < bytes.len() {
                if bytes[j] == b'`' {
                    let mut close_count = 0;
                    while j < bytes.len() && bytes[j] == b'`' {
                        close_count += 1;
                        j += 1;
                    }
                    if close_count == backtick_count {
                        // Found matching pair: code span covers open_start..j
                        if byte_pos >= open_start && byte_pos < j {
                            return true;
                        }
                        i = j;
                        break;
                    }
                } else {
                    j += 1;
                }
            }

            if j >= bytes.len() {
                // No matching close found, remaining text is not a code span
                break;
            }
        } else {
            i += 1;
        }
    }

    false
}

/// Check if a byte position is within an HTML tag. O(log n) via binary search.
fn is_byte_in_html_tag(html_tags: &[HtmlTag], byte_pos: usize) -> bool {
    let idx = html_tags.partition_point(|tag| tag.byte_offset <= byte_pos);
    idx > 0 && byte_pos < html_tags[idx - 1].byte_end
}

/// Check if a byte position is within HTML code content (`<code>...</code>`).
/// Uses pre-computed code ranges for O(log n) lookup via binary search.
fn is_byte_in_html_code_content(code_ranges: &[(usize, usize)], byte_pos: usize) -> bool {
    let idx = code_ranges.partition_point(|&(start, _)| start <= byte_pos);
    idx > 0 && byte_pos < code_ranges[idx - 1].1
}

/// Pre-compute ranges covered by `<code>...</code>` HTML tags.
/// Returns sorted Vec of (start, end) byte ranges.
pub(crate) fn compute_html_code_ranges(html_tags: &[HtmlTag]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut open_code_end: Option<usize> = None;

    for tag in html_tags {
        if tag.tag_name == "code" {
            if tag.is_self_closing {
                continue;
            } else if !tag.is_closing {
                open_code_end = Some(tag.byte_end);
            } else if tag.is_closing {
                if let Some(start) = open_code_end {
                    ranges.push((start, tag.byte_offset));
                }
                open_code_end = None;
            }
        }
    }
    // Handle unclosed <code> tag
    if let Some(start) = open_code_end {
        ranges.push((start, usize::MAX));
    }
    ranges
}

/// Determine whether an emphasis or strong span starting at `span_start` should be
/// skipped because it falls inside a non-prose context: code blocks/spans, inline
/// code, links, HTML tags or `<code>` content, MkDocs/PyMdown markup, math, JSX
/// expressions, MDX comments, front matter, or mkdocstrings blocks.
///
/// `html_tags` and `html_code_ranges` are passed in so callers iterating many spans
/// can compute them once via [`compute_html_code_ranges`].
pub(crate) fn should_skip_emphasis_span(
    ctx: &LintContext,
    html_tags: &[HtmlTag],
    html_code_ranges: &[(usize, usize)],
    span_start: usize,
) -> bool {
    let lines = ctx.raw_lines();
    let (line_num, col) = ctx.offset_to_line_col(span_start);

    // Skip matches in front matter or mkdocstrings blocks
    if ctx
        .line_info(line_num)
        .is_some_and(|info| info.in_front_matter || info.in_mkdocstrings)
    {
        return true;
    }

    // Check MkDocs markup
    let in_mkdocs_markup = lines
        .get(line_num.saturating_sub(1))
        .is_some_and(|line| is_in_mkdocs_markup(line, col.saturating_sub(1), ctx.flavor));

    // Line-level inline code fallback for cases pulldown-cmark misses
    let in_inline_code = lines
        .get(line_num.saturating_sub(1))
        .is_some_and(|line| is_in_inline_code_on_line(line, col.saturating_sub(1)));

    ctx.is_in_code_block_or_span(span_start)
        || in_inline_code
        || ctx.is_in_link(span_start)
        || is_byte_in_html_tag(html_tags, span_start)
        || is_byte_in_html_code_content(html_code_ranges, span_start)
        || in_mkdocs_markup
        || is_in_math_context(ctx, span_start)
        || is_in_jsx_expression(ctx, span_start)
        || is_in_mdx_comment(ctx, span_start)
}
