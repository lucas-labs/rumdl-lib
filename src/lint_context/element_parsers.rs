use crate::config::MarkdownFlavor;
use crate::utils::code_block_utils::CodeBlockUtils;
use crate::utils::rumdl_parser_options;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::sync::LazyLock;

use super::types::*;

/// Parse all inline code spans in the content using pulldown-cmark streaming parser.
///
/// Note: For MkDocs content, `scan_mkdocs_container_code_spans()` must be called separately
/// to detect code spans that pulldown-cmark misses inside 4-space-indented containers.
/// This is done during LintContext construction in mod.rs.
pub(super) fn parse_code_spans(content: &str, lines: &[LineInfo]) -> Vec<CodeSpan> {
    // Quick check - if no backticks, no code spans
    if !content.contains('`') {
        return Vec::new();
    }

    // Use pulldown-cmark's streaming parser with byte offsets
    let parser = Parser::new(content).into_offset_iter();
    let mut ranges = Vec::new();

    for (event, range) in parser {
        if let Event::Code(_) = event {
            ranges.push((range.start, range.end));
        }
    }

    build_code_spans_from_ranges(content, lines, &ranges)
}

pub(super) fn build_code_spans_from_ranges(
    _content: &str,
    lines: &[LineInfo],
    ranges: &[(usize, usize)],
) -> Vec<CodeSpan> {
    let mut code_spans = Vec::new();
    if ranges.is_empty() {
        return code_spans;
    }

    for &(start_pos, end_pos) in ranges {
        // Use binary search to find line number - O(log n) instead of O(n)
        // Find the rightmost line whose byte_offset <= start_pos
        let line_idx = lines
            .partition_point(|line| line.byte_offset <= start_pos)
            .saturating_sub(1);
        let line_num = line_idx + 1;

        // Find end column using binary search
        let end_line_idx = lines
            .partition_point(|line| line.byte_offset <= end_pos)
            .saturating_sub(1);

        code_spans.push(CodeSpan {
            line: line_num,
            end_line: end_line_idx + 1,
            byte_offset: start_pos,
            byte_end: end_pos,
        });
    }

    // Sort by position to ensure consistent ordering
    code_spans.sort_by_key(|span| span.byte_offset);

    code_spans
}

/// Parse all math spans (inline $...$ and display $$...$$) using pulldown-cmark
pub(super) fn parse_math_spans(content: &str, lines: &[LineInfo]) -> Vec<MathSpan> {
    let mut math_spans = Vec::new();

    // Quick check - if no $ signs, no math spans
    if !content.contains('$') {
        return math_spans;
    }

    // Use pulldown-cmark with ENABLE_MATH option
    let mut options = Options::empty();
    options.insert(Options::ENABLE_MATH);
    let parser = Parser::new_ext(content, options).into_offset_iter();

    for (event, range) in parser {
        let (is_display, _math_content) = match &event {
            Event::InlineMath(text) => (false, text.as_ref()),
            Event::DisplayMath(text) => (true, text.as_ref()),
            _ => continue,
        };

        let start_pos = range.start;
        let end_pos = range.end;

        // Use binary search to find line number - O(log n) instead of O(n)
        let line_idx = lines
            .partition_point(|line| line.byte_offset <= start_pos)
            .saturating_sub(1);
        let line_num = line_idx + 1;

        // Find end column using binary search
        let end_line_idx = lines
            .partition_point(|line| line.byte_offset <= end_pos)
            .saturating_sub(1);

        math_spans.push(MathSpan {
            line: line_num,
            end_line: end_line_idx + 1,
            byte_offset: start_pos,
            byte_end: end_pos,
            is_display,
        });
    }

    // Sort by position to ensure consistent ordering
    math_spans.sort_by_key(|span| span.byte_offset);

    math_spans
}

/// Parse HTML tags in the content.
///
/// Returns `(html_tags, jsx_component_tags)`. JSX component tags (uppercase-named,
/// only under JSX-supporting flavors) are split out rather than dropped, so that
/// HTML-specific rules keep ignoring them while rules that must look inside a
/// component's opening tag (e.g. MD034 attribute URLs) can still consult them.
pub(super) fn parse_html_tags(
    content: &str,
    lines: &[LineInfo],
    code_blocks: &[(usize, usize)],
    _flavor: MarkdownFlavor,
) -> (Vec<HtmlTag>, Vec<HtmlTag>) {
    static HTML_TAG_REGEX: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?i)<(/?)([a-zA-Z][a-zA-Z0-9-]*)(?:\s+[^>]*?)?\s*(/?)>").unwrap());

    let bytes = content.as_bytes();
    let content_len = bytes.len();
    let mut html_tags = Vec::new();
    let jsx_component_tags = Vec::new();
    let mut search_pos = 0;

    // Find each '<' and run the regex from that position instead of scanning full content
    while search_pos < content_len {
        let Some(lt_offset) = bytes[search_pos..].iter().position(|&b| b == b'<') else {
            break;
        };
        let lt_pos = search_pos + lt_offset;
        search_pos = lt_pos + 1;

        // CommonMark backslash escapes turn an odd-backslashed `<` into
        // literal text. An even run leaves the `<` active because the
        // backslashes escape in pairs.
        if bytes[..lt_pos].iter().rev().take_while(|&&byte| byte == b'\\').count() % 2 == 1 {
            continue;
        }

        // Quick check: next char after '<' must be '/' or ASCII alpha for a valid tag
        if lt_pos + 1 >= content_len {
            break;
        }
        let next = bytes[lt_pos + 1];
        if next != b'/' && !next.is_ascii_alphabetic() {
            continue;
        }

        // Skip if in code block
        if CodeBlockUtils::is_in_code_block_or_span(code_blocks, lt_pos) {
            continue;
        }

        // Determine search window: from '<' to the next '>' (with a reasonable limit)
        // This handles multi-line tags where attributes span lines
        let mut window_end = bytes[lt_pos..]
            .iter()
            .position(|&b| b == b'>')
            .map_or(content_len.min(lt_pos + 4096), |offset| lt_pos + offset + 1);
        // The 4096-byte cap is a raw byte offset that can fall inside a multi-byte
        // UTF-8 character; walk back to the nearest char boundary before slicing.
        while window_end > lt_pos && !content.is_char_boundary(window_end) {
            window_end -= 1;
        }
        let window = &content[lt_pos..window_end];

        if let Some(cap) = HTML_TAG_REGEX.captures(window) {
            let full_match = cap.get(0).unwrap();
            // Only accept matches starting at position 0 (the '<' we found)
            if full_match.start() != 0 {
                continue;
            }

            let match_start = lt_pos;
            let match_end = lt_pos + full_match.end();

            let is_closing = !cap.get(1).unwrap().as_str().is_empty();
            let tag_name_original = cap.get(2).unwrap().as_str();
            let tag_name = tag_name_original.to_lowercase();
            let is_self_closing = !cap.get(3).unwrap().as_str().is_empty();

            // Find which line this tag is on using binary search
            let line_idx = lines.partition_point(|info| info.byte_offset <= match_start);
            let line_idx = line_idx.saturating_sub(1);
            let line_num = line_idx + 1;

            let tag = HtmlTag {
                line: line_num,
                byte_offset: match_start,
                byte_end: match_end,
                tag_name,
                is_closing,
                is_self_closing,
            };

            // Split JSX component tags (uppercase-named, JSX flavors only) from real
            // HTML tags. Both are recorded in document order, so each list stays sorted
            // by byte offset for binary search.
            {
                html_tags.push(tag);
            }

            // Advance past the match to avoid overlapping
            search_pos = match_end;
        }
    }

    (html_tags, jsx_component_tags)
}

/// Detect lazy continuation lines within list items using pulldown-cmark's SoftBreak events.
///
/// Lazy continuation occurs when text continues a list item paragraph but with less
/// indentation than expected. pulldown-cmark identifies this via SoftBreak followed by Text
/// within a list Item, where the Text starts at a column less than the item's content column.
pub(super) fn detect_lazy_continuation_lines(
    content: &str,
    lines: &[LineInfo],
    line_offsets: &[usize],
) -> Vec<LazyContLine> {
    use crate::utils::blockquote::effective_indent_in_blockquote;

    let mut lazy_lines = Vec::new();
    let parser = Parser::new_ext(content, rumdl_parser_options());

    // Stack of (expected_indent_within_context, blockquote_level) for nested items
    let mut item_stack: Vec<(usize, usize)> = vec![];
    let mut after_soft_break = false;

    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::Item) => {
                let line_num = byte_to_line(line_offsets, range.start);
                let line_info = lines.get(line_num.saturating_sub(1));
                let line_content = line_info.map_or("", |li| li.content(content));

                // Determine blockquote level from the line content
                let bq_level = line_content
                    .chars()
                    .take_while(|c| *c == '>' || c.is_whitespace())
                    .filter(|&c| c == '>')
                    .count();

                // Calculate expected indent relative to blockquote context
                let expected_indent = if bq_level > 0 {
                    // For blockquote lists, expected indent is the marker width
                    line_info
                        .and_then(|li| li.list_item.as_ref())
                        .map_or(2, |item| item.content_column.saturating_sub(item.marker_column))
                } else {
                    // For regular lists, use content_column directly
                    line_info
                        .and_then(|li| li.list_item.as_ref())
                        .map_or(0, |item| item.content_column)
                };

                item_stack.push((expected_indent, bq_level));
                after_soft_break = false;
            }
            Event::End(TagEnd::Item) => {
                item_stack.pop();
                after_soft_break = false;
            }
            Event::SoftBreak if !item_stack.is_empty() => {
                after_soft_break = true;
            }
            // Detect content starting after a soft break - text, code, or inline formatting
            Event::Text(_)
            | Event::Code(_)
            | Event::Start(Tag::Emphasis)
            | Event::Start(Tag::Strong)
            | Event::Start(Tag::Strikethrough)
            | Event::Start(Tag::Subscript)
            | Event::Start(Tag::Superscript)
            | Event::Start(Tag::Link { .. })
            | Event::Start(Tag::Image { .. })
                if after_soft_break =>
            {
                if let Some(&(expected_indent, expected_bq_level)) = item_stack.last() {
                    let line_num = byte_to_line(line_offsets, range.start);
                    let line_info = lines.get(line_num.saturating_sub(1));
                    let line_content = line_info.map_or("", |li| li.content(content));
                    let fallback_indent = line_info.map_or(0, |li| li.indent);

                    let actual_indent =
                        effective_indent_in_blockquote(line_content, expected_bq_level, fallback_indent);

                    if actual_indent < expected_indent {
                        lazy_lines.push(LazyContLine {
                            line_num,
                            expected_indent,
                            current_indent: actual_indent,
                            blockquote_level: expected_bq_level,
                        });
                    }
                }
                after_soft_break = false;
            }
            _ => {
                after_soft_break = false;
            }
        }
    }

    lazy_lines
}

/// Convert a byte offset to a 1-indexed line number
fn byte_to_line(line_offsets: &[usize], byte_offset: usize) -> usize {
    match line_offsets.binary_search(&byte_offset) {
        Ok(idx) => idx + 1,
        Err(idx) => idx.max(1),
    }
}
