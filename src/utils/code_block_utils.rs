//!
//! Utility functions for detecting and handling code blocks and code spans in Markdown for rumdl.
//!
//! Code block detection is delegated to pulldown-cmark, which correctly implements the
//! CommonMark specification. This handles edge cases like:
//! - Backtick fences with backticks in the info string (invalid per spec)
//! - Nested fences (longer fence contains shorter fence as content)
//! - Mixed fence types (tilde fence contains backticks as content)
//! - Indented code blocks with proper list context handling

use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

use super::parser_options::rumdl_parser_options;

/// Detailed information about a code block captured during parsing
#[derive(Debug, Clone)]
pub struct CodeBlockDetail {
    /// Byte offset where this code block starts
    pub(crate) start: usize,
    /// Byte offset where this code block ends
    pub(crate) end: usize,
    /// Whether this is a fenced code block true or indented false
    pub(crate) is_fenced: bool,
    /// The info string from fenced blocks (e.g., "rust" from ```rust), empty for indented
    pub(crate) info_string: String,
}

/// A strong emphasis span captured during parsing
#[derive(Debug, Clone)]
pub struct StrongSpanDetail {
    /// Byte offset where the strong span starts (including **)
    pub(crate) start: usize,
    /// Byte offset where the strong span ends (including **)
    pub(crate) end: usize,
    /// Whether this uses asterisk (**) or underscore (__) markers
    pub(crate) is_asterisk: bool,
}

/// Text a definition holds directly, captured during parsing
///
/// One of the definition's paragraphs, or, for a tight definition, the run of
/// inline content the parser reports with no paragraph around it. Blocks nested
/// in the definition (lists, blockquotes, code) hold their own text and are not
/// part of one.
#[derive(Debug, Clone)]
pub struct DefinitionTextDetail {
    /// Byte offset where the text starts
    pub(crate) start: usize,
    /// Byte offset where the text ends
    pub(crate) end: usize,
    /// Byte offset where the definition holding the text starts: its `:`
    /// marker, or indentation before it inside a container
    pub(crate) definition_start: usize,
}

/// Ordered list membership: maps line number (1-indexed) to list ID
pub type LineToListMap = std::collections::HashMap<usize, usize>;
/// Ordered list start values: maps list ID to the start value
pub type ListStartValues = std::collections::HashMap<usize, u64>;

/// Result of the central pulldown-cmark parse, capturing all data needed by individual rules
pub struct ParseResult {
    /// Code block byte ranges (start, end)
    pub(crate) code_blocks: Vec<(usize, usize)>,
    /// Inline code span byte ranges (start, end)
    pub(crate) code_spans: Vec<(usize, usize)>,
    /// Detailed code block info (fenced vs indented, info string)
    pub(crate) code_block_details: Vec<CodeBlockDetail>,
    /// Strong emphasis span details
    pub(crate) strong_spans: Vec<StrongSpanDetail>,
    /// Ordered list membership: maps line number (1-indexed) to list ID
    pub(crate) line_to_list: LineToListMap,
    /// Ordered list start values: maps list ID to start value
    pub(crate) list_start_values: ListStartValues,
    /// HTML block byte ranges (start, end)
    ///
    /// Needed to know how far an unclosed `<!--` reaches. The block ends where
    /// CommonMark says it does, which is the end of the enclosing container
    /// rather than the end of the document: an unclosed comment in a blockquote
    /// stops at the quote, and one in a list item stops at the blank line.
    pub(crate) html_blocks: Vec<(usize, usize)>,
    /// Definition list item byte ranges (start, end) in document order: a
    /// definition together with the terms before it, blank lines after it left
    /// out, items of nested lists too. Only definitions passing
    /// `opens_definition`, and their terms, count.
    pub(crate) definition_items: Vec<(usize, usize)>,
    /// Definition list term byte ranges (start, end)
    pub(crate) definition_terms: Vec<(usize, usize)>,
    /// Text held directly by a definition, in document order
    pub(crate) definition_texts: Vec<DefinitionTextDetail>,
}

/// Classification of code blocks relative to list contexts
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeBlockContext {
    /// Code block that separates lists (root-level, with blank lines)
    Standalone,
    /// Code block that continues a list (properly indented)
    Indented,
    /// Code block adjacent to list content (edge case, defaults to non-breaking)
    Adjacent,
}

/// Whether a tag opens inline content rather than a block
fn is_inline_tag(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::Link { .. }
            | Tag::Image { .. }
    )
}

/// Whether a tag end closes inline content rather than a block
fn is_inline_tag_end(tag_end: TagEnd) -> bool {
    matches!(
        tag_end,
        TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::Link
            | TagEnd::Image
    )
}

/// Whether the definition starting at `start` opens with a colon followed by
/// whitespace or the end of its line.
///
/// That is the marker the definition-list extensions agree on (PHP Markdown
/// Extra, Python-Markdown, Pandoc). pulldown-cmark also opens a definition on a
/// colon touching its text, as in `:warning:` or a `:::` fence closing a div,
/// which those read as prose or as the fence it is.
fn opens_definition(content: &str, start: usize) -> bool {
    content[start..]
        .trim_start_matches([' ', '\t'])
        .strip_prefix(':')
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t', '\n', '\r']))
}

/// Utility functions for detecting and handling code blocks in Markdown
pub struct CodeBlockUtils;

impl CodeBlockUtils {
    /// Detect all code blocks in the content (NOT including inline code spans)
    ///
    /// Uses pulldown-cmark for spec-compliant CommonMark parsing. This correctly handles:
    /// - Fenced code blocks (``` and ~~~)
    /// - Indented code blocks (4 spaces or tab)
    /// - Code blocks inside lists, blockquotes, and other containers
    /// - Edge cases like backticks in info strings (which invalidate the fence)
    ///
    /// Returns a sorted vector of (start, end) byte offset tuples.
    pub(crate) fn detect_code_blocks(content: &str) -> Vec<(usize, usize)> {
        Self::detect_code_blocks_and_spans(content).code_blocks
    }

    /// Returns code block ranges, inline code span ranges, and detailed code block info
    /// in a single pulldown-cmark pass.
    pub(crate) fn detect_code_blocks_and_spans(content: &str) -> ParseResult {
        let mut blocks = Vec::new();
        let mut spans = Vec::new();
        let mut details = Vec::new();
        let mut strong_spans = Vec::new();
        let mut html_blocks = Vec::new();
        let mut code_block_start: Option<(usize, bool, String)> = None;

        // Definition structure. `block_stack` holds one entry per open block,
        // the definition's start for a definition and `None` for anything else,
        // so the innermost entry says whether content belongs to a definition
        // directly. Inline tags never touch it.
        //
        // Only a definition passing `opens_definition` counts, and a term
        // waits in `pending_terms` until a definition following it does. Extents
        // are recorded per item rather than per list: the parser's range for a
        // list can run over the paragraph after it, and a list can hold items
        // that do not count between items that do.
        let mut definition_items: Vec<(usize, usize)> = Vec::new();
        let mut pending_terms: Vec<(usize, usize)> = Vec::new();
        let mut definition_terms = Vec::new();
        let mut definition_texts = Vec::new();
        let mut block_stack: Vec<Option<usize>> = Vec::new();
        let mut definition_paragraph: Option<(usize, usize)> = None;
        let mut tight_run: Option<DefinitionTextDetail> = None;

        // List membership tracking for ordered lists
        let mut line_to_list = LineToListMap::new();
        let mut list_start_values = ListStartValues::new();
        let mut list_stack: Vec<(usize, bool, u64)> = Vec::new(); // (list_id, is_ordered, start_value)
        let mut next_list_id: usize = 0;

        // Pre-compute line start offsets for byte-to-line conversion
        let line_starts: Vec<usize> = std::iter::once(0)
            .chain(content.match_indices('\n').map(|(i, _)| i + 1))
            .collect();

        let byte_to_line = |byte_offset: usize| -> usize { line_starts.partition_point(|&start| start <= byte_offset) };

        let options = rumdl_parser_options();
        let parser = Parser::new_ext(content, options).into_offset_iter();

        for (event, range) in parser {
            match &event {
                Event::Start(tag) if !is_inline_tag(tag) => {
                    definition_texts.extend(tight_run.take());
                    if let (Tag::Paragraph, Some(Some(definition_start))) = (tag, block_stack.last()) {
                        definition_paragraph = Some((range.start, *definition_start));
                    }
                    let mut definition_start = None;
                    match tag {
                        Tag::DefinitionList => pending_terms.clear(),
                        Tag::DefinitionListTitle => pending_terms.push((range.start, range.end)),
                        Tag::DefinitionListDefinition if opens_definition(content, range.start) => {
                            // The definition's range takes in the blank lines after
                            // it, quoted ones too, which belong to no block.
                            let item_start = pending_terms.first().map_or(range.start, |&(start, _)| start);
                            let item_end = range.start
                                + content[range.clone()]
                                    .trim_end_matches([' ', '\t', '\n', '\r', '>'])
                                    .len();
                            definition_items.push((item_start, item_end));
                            definition_terms.append(&mut pending_terms);
                            definition_start = Some(range.start);
                        }
                        Tag::DefinitionListDefinition => pending_terms.clear(),
                        _ => {}
                    }
                    block_stack.push(definition_start);
                }
                Event::End(tag_end) if !is_inline_tag_end(*tag_end) => {
                    definition_texts.extend(tight_run.take());
                    if let (TagEnd::Paragraph, Some((start, definition_start))) = (tag_end, definition_paragraph.take())
                    {
                        definition_texts.push(DefinitionTextDetail {
                            start,
                            end: range.end,
                            definition_start,
                        });
                    }
                    if let TagEnd::DefinitionList = tag_end {
                        pending_terms.clear();
                    }
                    block_stack.pop();
                }
                // Inline content directly under a definition is a tight
                // definition's text, which the parser does not wrap in a paragraph.
                _ => {
                    if let Some(Some(definition_start)) = block_stack.last() {
                        match &mut tight_run {
                            Some(run) => run.end = run.end.max(range.end),
                            None => {
                                tight_run = Some(DefinitionTextDetail {
                                    start: range.start,
                                    end: range.end,
                                    definition_start: *definition_start,
                                });
                            }
                        }
                    }
                }
            }
            match event {
                Event::Start(Tag::CodeBlock(kind)) => {
                    let (is_fenced, info_string) = match &kind {
                        CodeBlockKind::Fenced(info) => (true, info.to_string()),
                        CodeBlockKind::Indented => (false, String::new()),
                    };
                    code_block_start = Some((range.start, is_fenced, info_string));
                }
                Event::End(TagEnd::CodeBlock) => {
                    if let Some((start, is_fenced, info_string)) = code_block_start.take() {
                        blocks.push((start, range.end));
                        details.push(CodeBlockDetail {
                            start,
                            end: range.end,
                            is_fenced,
                            info_string,
                        });
                    }
                }
                Event::Start(Tag::Strong) => {
                    if range.start + 2 <= content.len() {
                        let is_asterisk = &content[range.start..range.start + 2] == "**";
                        strong_spans.push(StrongSpanDetail {
                            start: range.start,
                            end: range.end,
                            is_asterisk,
                        });
                    }
                }
                Event::Start(Tag::List(start_num)) => {
                    let is_ordered = start_num.is_some();
                    let start_value = start_num.unwrap_or(1);
                    list_stack.push((next_list_id, is_ordered, start_value));
                    if is_ordered {
                        list_start_values.insert(next_list_id, start_value);
                    }
                    next_list_id += 1;
                }
                Event::End(TagEnd::List(_)) => {
                    list_stack.pop();
                }
                Event::Start(Tag::Item) => {
                    if let Some(&(list_id, is_ordered, _)) = list_stack.last()
                        && is_ordered
                    {
                        let line_num = byte_to_line(range.start);
                        line_to_list.insert(line_num, list_id);
                    }
                }
                Event::Start(Tag::HtmlBlock) => {
                    // The start event's range already spans the whole block.
                    html_blocks.push((range.start, range.end));
                }
                Event::Code(_) => {
                    spans.push((range.start, range.end));
                }
                _ => {}
            }
        }

        // Handle edge case: unclosed code block at end of content
        // pulldown-cmark should handle this, but be defensive
        if let Some((start, is_fenced, info_string)) = code_block_start {
            blocks.push((start, content.len()));
            details.push(CodeBlockDetail {
                start,
                end: content.len(),
                is_fenced,
                info_string,
            });
        }

        // Sort by start position (should already be sorted, but ensure consistency)
        blocks.sort_by_key(|&(start, _)| start);
        spans.sort_by_key(|&(start, _)| start);
        details.sort_by_key(|d| d.start);
        strong_spans.sort_by_key(|s| s.start);
        html_blocks.sort_by_key(|&(start, _)| start);
        ParseResult {
            definition_items,
            definition_terms,
            definition_texts,
            code_blocks: blocks,
            code_spans: spans,
            code_block_details: details,
            strong_spans,
            line_to_list,
            list_start_values,
            html_blocks,
        }
    }

    /// Check if a position is within a code block (for compatibility)
    pub(crate) fn is_in_code_block_or_span(blocks: &[(usize, usize)], pos: usize) -> bool {
        Self::is_in_code_block(blocks, pos)
    }

    /// Check if a byte position falls within any of the given sorted, non-overlapping ranges.
    ///
    /// Uses binary search on the sorted block ranges for O(log n) lookup.
    /// The blocks slice must be sorted by start position (as returned by
    /// `detect_code_blocks` and `detect_code_blocks_and_spans`).
    pub(crate) fn is_in_code_block(blocks: &[(usize, usize)], pos: usize) -> bool {
        // Binary search: find the last block whose start <= pos
        let idx = blocks.partition_point(|&(start, _)| start <= pos);
        // partition_point returns the first index where start > pos,
        // so the candidate is at idx - 1
        idx > 0 && pos < blocks[idx - 1].1
    }

    /// Analyze code block context relative to list parsing
    /// This is the core function implementing Design #3's three-tier classification
    pub(crate) fn analyze_code_block_context(
        lines: &[crate::lint_context::LineInfo],
        line_idx: usize,
        min_continuation_indent: usize,
    ) -> CodeBlockContext {
        if let Some(line_info) = lines.get(line_idx) {
            // Rule 1: Indentation Analysis - Is it sufficiently indented for list continuation?
            if line_info.indent >= min_continuation_indent {
                return CodeBlockContext::Indented;
            }

            // Rule 2: Blank Line Context - Check for structural separation indicators
            let (prev_blanks, next_blanks) = Self::count_surrounding_blank_lines(lines, line_idx);

            // Rule 3: Standalone Detection - Insufficient indentation + blank line separation
            // This is the key fix: root-level code blocks with blank lines separate lists
            if prev_blanks > 0 || next_blanks > 0 {
                return CodeBlockContext::Standalone;
            }

            // Rule 4: Default - Adjacent (conservative, non-breaking for edge cases)
            CodeBlockContext::Adjacent
        } else {
            // Fallback for invalid line index
            CodeBlockContext::Adjacent
        }
    }

    /// Count blank lines before and after the given line index
    fn count_surrounding_blank_lines(lines: &[crate::lint_context::LineInfo], line_idx: usize) -> (usize, usize) {
        let mut prev_blanks = 0;
        let mut next_blanks = 0;

        // Count blank lines before (look backwards)
        for i in (0..line_idx).rev() {
            if let Some(line) = lines.get(i) {
                if line.is_blank {
                    prev_blanks += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Count blank lines after (look forwards)
        for i in (line_idx + 1)..lines.len() {
            if let Some(line) = lines.get(i) {
                if line.is_blank {
                    next_blanks += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        (prev_blanks, next_blanks)
    }

    /// Calculate minimum indentation required for code block to continue a list
    /// Based on the most recent list item's marker width
    pub(crate) fn calculate_min_continuation_indent(
        content: &str,
        lines: &[crate::lint_context::LineInfo],
        current_line_idx: usize,
    ) -> usize {
        // Look backwards to find the most recent list item
        for i in (0..current_line_idx).rev() {
            if let Some(line_info) = lines.get(i) {
                if let Some(list_item) = &line_info.list_item {
                    // Calculate minimum continuation indent for this list item
                    return if list_item.is_ordered {
                        list_item.marker_column + list_item.marker.len() + 1 // +1 for space after marker
                    } else {
                        list_item.marker_column + 2 // Unordered lists need marker + space (min 2)
                    };
                }

                // Stop at structural separators that would break list context
                if line_info.heading.is_some() || Self::is_structural_separator(line_info.content(content)) {
                    break;
                }
            }
        }

        0 // No list context found
    }

    /// Check if content is a structural separator (headings, horizontal rules, etc.)
    fn is_structural_separator(content: &str) -> bool {
        let trimmed = content.trim();
        trimmed.starts_with("---")
            || trimmed.starts_with("***")
            || trimmed.starts_with("___")
            || crate::utils::skip_context::is_table_line(trimmed)
            || trimmed.starts_with('>') // Blockquotes
    }
}
