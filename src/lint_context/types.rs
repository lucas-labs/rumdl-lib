use pulldown_cmark::LinkType;
use std::borrow::Cow;
use std::ops::Range;

/// Pre-computed information about a line
#[derive(Debug, Clone)]
pub struct LineInfo {
    /// Byte offset where this line starts in the document
    pub(crate) byte_offset: usize,
    /// Length of the line in bytes (without newline)
    pub(crate) byte_len: usize,
    /// Number of bytes of leading whitespace (for substring extraction)
    pub indent: usize,
    /// Visual column width of leading whitespace (with proper tab expansion)
    /// Per CommonMark, tabs expand to the next column that is a multiple of 4.
    /// Use this for numeric comparisons like checking for indented code blocks (>= 4).
    pub(crate) visual_indent: usize,
    /// Whether the line is blank (empty or only whitespace)
    pub is_blank: bool,
    /// Whether this line is inside a code block
    pub in_code_block: bool,
    /// Whether this line is inside front matter
    pub in_front_matter: bool,
    /// Whether this line is inside an HTML block
    pub(crate) in_html_block: bool,
    /// Whether this line is part of a list block (precomputed for O(1) lookup)
    pub in_list_block: bool,
    /// Whether this line is part of a table block (precomputed for O(1) lookup)
    pub(crate) in_table_block: bool,
    /// Whether this line is inside an HTML comment
    pub in_html_comment: bool,
    /// List item information if this line starts a list item
    /// Boxed to reduce LineInfo size: most lines are not list items
    pub list_item: Option<Box<ListItemInfo>>,
    /// Heading information if this line is a heading: an ATX heading line, or
    /// the last text line of a setext heading, whose underline is the line after
    /// Boxed to reduce LineInfo size: most lines are not headings
    ///
    /// Only CommonMark headings are recorded. A line like `#Heading`, with no
    /// space after its `#`s, renders as paragraph text and is recorded in
    /// `atx_missing_space` instead.
    pub heading: Option<Box<HeadingInfo>>,
    /// Set when the line is shaped like an ATX heading but has no space after
    /// its `#`s (`#Heading`, `##x`, `#tag`). CommonMark reads it as paragraph
    /// text, and so does every rule except the ones reporting the missing
    /// space. Unset on a line that is text of a setext heading, where the `#`
    /// is part of the heading text.
    pub(crate) atx_missing_space: Option<AtxMissingSpace>,
    /// Whether the line holds text of a setext heading: one of the lines of the
    /// paragraph the underline below them makes a heading of. The heading is
    /// recorded in `heading` on the last of those lines, with `text_lines`
    /// counting them. A setext heading inside a blockquote is reported through
    /// `headings()` and leaves this unset, as it leaves `heading` unset.
    pub(crate) is_setext_heading_text: bool,
    /// Blockquote information if this line is a blockquote
    /// Boxed to reduce LineInfo size: most lines are not blockquotes
    pub(crate) blockquote: Option<Box<BlockquoteInfo>>,
    /// Whether this line is inside a mkdocstrings autodoc block
    pub(crate) in_mkdocstrings: bool,
    /// Whether this line is part of an ESM import/export block (MDX only)
    /// Whether this line is a continuation of a multi-line code span from a previous line
    pub(crate) in_code_span_continuation: bool,
    /// Whether this line is a horizontal rule (---, ***, ___, etc.)
    /// Pre-computed for consistent detection across all rules
    pub(crate) is_horizontal_rule: bool,
    /// Whether this line is inside a math block ($$ ... $$)
    pub(crate) in_math_block: bool,
    /// Whether this line is inside a Pandoc/Quarto div block (::: ... :::)
    /// Whether this line is a Quarto/Pandoc div marker (opening ::: {.class} or closing :::)
    /// Analogous to `is_horizontal_rule` — marks structural delimiters that are not paragraph text
    pub(crate) is_div_marker: bool,
    /// Whether the line is the marker of a container whose body is Markdown: a
    /// MkDocs admonition or content tab opener, a PyMdown block fence, or a MyST
    /// colon fence. Like `is_div_marker`, a structural delimiter rather than
    /// paragraph text: the body below the marker is the container's own content,
    /// not a continuation of the marker line.
    /// Whether this line contains or is inside a JSX expression (MDX only)
    /// Whether this line is inside an MDX comment {/* ... */} (MDX only)
    /// Whether this line is inside an MkDocs admonition block (!!! or ???)
    pub(crate) in_admonition: bool,
    /// Whether this line is inside an MkDocs content tab block (===)
    pub(crate) in_content_tab: bool,
    /// Whether this line is inside an HTML block with markdown attribute (MkDocs grid cards, etc.)
    pub(crate) in_mkdocs_html_markdown: bool,
    /// Whether this line is a definition list item (: definition)
    /// Whether this line is inside an Obsidian comment (%%...%% syntax, Obsidian flavor only)
    /// Whether this line is inside a PyMdown Blocks region (/// ... ///, MkDocs flavor only)
    /// Whether this line is inside a kramdown extension block ({::comment}...{:/comment}, {::nomarkdown}...{:/nomarkdown})
    /// Whether this line is a kramdown block IAL ({:.class #id}) or ALD ({:ref: .class})
    /// Whether this line is inside a JSX component block (MDX only, e.g. `<Tabs>...</Tabs>`)
    /// Whether this line is inside a footnote definition body (continuation lines)
    pub(crate) in_footnote_definition: bool,
}

impl LineInfo {
    /// Get the line content as a string slice from the source document
    pub(crate) fn content<'a>(&self, source: &'a str) -> &'a str {
        &source[self.byte_offset..self.byte_offset + self.byte_len]
    }

    /// Check if this line is inside MkDocs-specific indented content (admonitions, tabs, or markdown HTML).
    /// This content uses 4-space indentation which pulldown-cmark would interpret as code blocks,
    /// but in MkDocs flavor it's actually container content that should be preserved.
    #[inline]
    pub(crate) fn in_mkdocs_container(&self) -> bool {
        self.in_admonition || self.in_content_tab || self.in_mkdocs_html_markdown
    }

    /// Whether this line could be part of a paragraph block (CommonMark `paragraph` token).
    ///
    /// Returns true for ordinary prose lines, including those inside blockquotes and list items.
    /// Returns false for lines that belong to non-paragraph blocks: headings, code blocks,
    /// HTML blocks, math blocks, horizontal rules, front matter, structural div markers, and
    /// flavor-specific extension blocks. This is the per-line view; cross-line constructs like
    /// setext underlines aren't visible here and need additional context to detect.
    ///
    /// Used by rules (e.g. MD009 strict mode) that need to distinguish "trailing whitespace
    /// could produce a meaningful `<br>`" from "trailing whitespace is on a structural boundary."
    #[inline]
    pub(crate) fn is_paragraph_context(&self) -> bool {
        !self.in_code_block
            && !self.in_front_matter
            && !self.in_html_block
            && !self.in_html_comment
            && !self.in_math_block
            && !self.is_horizontal_rule
            && !self.is_div_marker
            && true
            && true
            && true
            && true
            && self.heading.is_none()
    }
}

/// Information about a list item
/// Text a definition-list definition holds directly, in lines
///
/// One of the definition's paragraphs, or the text of a tight definition.
/// Terms, and blocks nested in the definition, are not definition text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionText {
    /// First line of the text (1-indexed)
    pub(crate) start_line: usize,
    /// Last line of the text (1-indexed, inclusive)
    pub(crate) end_line: usize,
    /// Byte length of what precedes the text on its first line when that line
    /// is the definition's marker line: 4 for `:   text`, counting any
    /// indentation before the colon. `None` when the text starts on a later
    /// line, as a definition's second paragraph does.
    pub(crate) marker_prefix_len: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ListItemInfo {
    /// The marker used (*, -, +, or number with . or ))
    pub(crate) marker: String,
    /// Whether it's ordered true or unordered false
    pub(crate) is_ordered: bool,
    /// Column where the marker starts (0-based)
    pub marker_column: usize,
    /// Column where content after marker starts
    pub content_column: usize,
}

/// Heading style type
#[derive(Debug, Clone, PartialEq)]
pub enum HeadingStyle {
    /// ATX style heading (# Heading)
    ATX,
    /// Setext style heading with = underline
    Setext1,
    /// Setext style heading with - underline
    Setext2,
}

/// Parsed link information
#[derive(Debug, Clone)]
pub struct ParsedLink<'a> {
    /// Line number (1-indexed)
    pub line: usize,
    /// Byte offset in document
    pub(crate) byte_offset: usize,
    /// End byte offset in document
    pub(crate) byte_end: usize,
    /// Link text
    pub(crate) text: Cow<'a, str>,
    /// Link URL or reference
    pub url: Cow<'a, str>,
    /// Whether this is a reference link `[text][ref]` vs inline `[text](url)`
    pub is_reference: bool,
    /// Link type from pulldown-cmark
    pub(crate) link_type: LinkType,
}

/// Information about a broken link reported by pulldown-cmark
#[derive(Debug, Clone)]
pub struct BrokenLinkInfo {
    /// Byte span in the source document
    pub(crate) span: std::ops::Range<usize>,
    /// The type of the broken link
    pub(crate) link_type: LinkType,
}

/// Parsed footnote reference (e.g., `[^1]`, `[^note]`)
#[derive(Debug, Clone)]
pub struct FootnoteRef {}

/// Parsed image information
#[derive(Debug, Clone)]
pub struct ParsedImage<'a> {
    /// Line number (1-indexed)
    pub line: usize,
    /// Byte offset in document
    pub(crate) byte_offset: usize,
    /// End byte offset in document
    pub(crate) byte_end: usize,
    /// Alt text
    pub(crate) alt_text: Cow<'a, str>,
    /// Image URL or reference
    pub url: Cow<'a, str>,
    /// Whether this is a reference image ![alt][ref] vs inline ![alt](url)
    pub is_reference: bool,
    /// Link type from pulldown-cmark
    pub(crate) link_type: LinkType,
}

/// Reference definition `[ref]: url "title"`
#[derive(Debug, Clone)]
pub struct ReferenceDef {
    /// Line number (1-indexed)
    pub line: usize,
    /// Reference ID (normalized to lowercase)
    pub(crate) id: String,
    /// URL
    pub url: String,
    /// Byte offset where the reference definition starts
    pub(crate) byte_offset: usize,
    /// Byte offset where the reference definition ends
    pub(crate) byte_end: usize,
}

/// Parsed code span information
#[derive(Debug, Clone)]
pub struct CodeSpan {
    /// Line number where the code span starts (1-indexed)
    pub(crate) line: usize,
    /// Line number where the code span ends (1-indexed)
    pub(crate) end_line: usize,
    /// Byte offset in document
    pub(crate) byte_offset: usize,
    /// End byte offset in document
    pub(crate) byte_end: usize,
}

/// Parsed math span information (inline $...$ or display $$...$$)
#[derive(Debug, Clone)]
pub struct MathSpan {
    /// Line number where the math span starts (1-indexed)
    pub(crate) line: usize,
    /// Line number where the math span ends (1-indexed)
    pub(crate) end_line: usize,
    /// Byte offset in document
    pub(crate) byte_offset: usize,
    /// End byte offset in document
    pub(crate) byte_end: usize,
    /// Whether this is display math ($$...$$) vs inline ($...$)
    pub(crate) is_display: bool,
}

/// Information about a heading
#[derive(Debug, Clone)]
pub struct HeadingInfo {
    /// Heading level (1-6 for ATX, 1-2 for Setext)
    pub level: u8,
    /// Style of heading
    pub(crate) style: HeadingStyle,
    /// Column where heading text starts
    pub(crate) content_column: usize,
    /// The heading text (without markers and without custom ID syntax)
    pub text: String,
    /// Custom header ID if present (e.g., from {#custom-id} syntax)
    pub(crate) custom_id: Option<String>,
    /// Original heading text including custom ID syntax. A setext heading's is
    /// the whole paragraph its underline ends, on one line: each soft line break
    /// is the space it renders as, and a hard line break's backslash goes with
    /// its line ending.
    pub(crate) raw_text: String,
    /// How many source lines hold the heading text: one for an ATX heading, and
    /// every line of the paragraph a setext underline makes a heading of. The
    /// heading is recorded on the last of them, so the first is `text_lines - 1`
    /// lines above it.
    pub text_lines: usize,
    /// Whether it has a closing sequence (for ATX)
    pub(crate) has_closing_sequence: bool,
    /// The closing sequence if present
    pub(crate) closing_sequence: String,
}

/// An ATX-shaped line with no space after its opening `#`s. See
/// [`LineInfo::atx_missing_space`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtxMissingSpace {
    /// Number of opening `#`s (1-6)
    pub(crate) level: u8,
}

/// A heading recognized in the rendered Markdown document.
///
/// Unlike [`ValidHeading`], this view includes headings inside blockquotes and
/// malformed ATX headings retained for diagnostics such as MD018. Consumers
/// can select the semantics they need without reparsing source lines.
#[derive(Debug, Clone, Copy)]
pub struct ParsedHeading<'a> {
    /// Parsed heading metadata.
    pub(crate) heading: &'a HeadingInfo,
    /// Metadata of every line holding the heading text, first to last:
    /// `line_info` alone, unless the heading is a setext heading whose
    /// paragraph spans lines.
    pub(crate) text_line_infos: &'a [LineInfo],
}

impl<'a> ParsedHeading<'a> {
    /// Whether this is a Setext-style heading.
    #[inline]
    pub(crate) fn is_setext(&self) -> bool {
        matches!(self.heading.style, HeadingStyle::Setext1 | HeadingStyle::Setext2)
    }

    /// Byte range of the heading text in the document.
    ///
    /// Markers, closing ATX sequences and custom-ID syntax are excluded. The
    /// text of a setext heading is the whole paragraph its underline ends, so
    /// the range runs from the text on the paragraph's first line to the text
    /// on its last, across the line breaks and container prefixes between them.
    #[must_use]
    pub(crate) fn text_byte_range(&self, source: &str) -> Range<usize> {
        text_byte_range(self.heading, self.text_line_infos, source)
    }

    /// Position of the heading text as `(line, column, end_line, end_column)`,
    /// 1-indexed with character columns and an exclusive end.
    ///
    /// A warning about a heading spans this range, which covers every text line
    /// of a setext heading whose paragraph spans more than one.
    #[must_use]
    pub(crate) fn text_position_range(&self, ctx: &super::LintContext) -> (usize, usize, usize, usize) {
        let range = self.text_byte_range(ctx.content);
        let (line, column) = ctx.offset_to_line_col(range.start);
        let (end_line, end_column) = ctx.offset_to_line_col(range.end);
        (line, column, end_line, end_column)
    }
}

/// See [`ParsedHeading::text_byte_range`].
fn text_byte_range(heading: &HeadingInfo, text_lines: &[LineInfo], source: &str) -> Range<usize> {
    let first_line = &text_lines[0];
    let first = first_line.content(source);
    let content_start = heading.content_column.min(first.len());
    // An ATX heading's text is written on its line after the marker and ends
    // before its closing sequence, the attribute list a custom ID sits in and
    // an anchor element, each of which the source is walked back over. The
    // display text locates the start when it is written as one piece; markup
    // inside it keeps the range on the whole source, which ends on a character
    // boundary whatever that markup holds.
    if matches!(heading.style, HeadingStyle::ATX) {
        let region = &first[content_start..];
        let mut end = crate::utils::header_id_utils::heading_text_end(region);
        if heading.has_closing_sequence
            && let Some(text) = region[..end].trim_end().strip_suffix(heading.closing_sequence.as_str())
        {
            end = crate::utils::header_id_utils::heading_text_end(text);
        }
        let start = region[..end].find(&heading.text).unwrap_or(0);
        let offset = first_line.byte_offset + content_start;
        return offset + start..offset + end;
    }
    // A setext heading's text ends where the display text of the last line
    // holding any ends: past that line's container prefix and before the
    // attribute list a custom ID sits in or an anchor element. A line holding
    // only those is not text, so the end moves up past it, never above the
    // first line.
    let start = first_line.byte_offset + content_start;
    let end = text_lines
        .iter()
        .rev()
        .find_map(|line| {
            let content = line.content(source);
            let (text_start, text) = match line.blockquote.as_deref() {
                Some(quote) => (quote.prefix.len().min(content.len()), quote.content.as_str()),
                None => (line.indent, &content[line.indent..]),
            };
            let text_end = crate::utils::header_id_utils::heading_text_end(text);
            (text_end > 0).then(|| line.byte_offset + text_start + text_end)
        })
        .unwrap_or(start);
    start..end.max(start)
}

/// A valid heading from a filtered iteration
///
/// Every recorded heading is CommonMark-compliant; paragraph text such as
/// `#tag` or `#123` is never recorded as one.
#[derive(Debug, Clone)]
pub struct ValidHeading<'a> {
    /// The 1-indexed number of the line the heading is recorded on: the ATX
    /// line, or the last text line of a setext heading, whose underline is the
    /// line after it
    pub(crate) line_num: usize,
    /// Reference to the heading information
    pub(crate) heading: &'a HeadingInfo,
    /// Reference to the full line info of the line the heading is recorded on
    /// (for rules that need additional context)
    pub(crate) line_info: &'a LineInfo,
}

impl<'a> ValidHeading<'a> {
    /// The 1-indexed number of the first line holding the heading text.
    #[inline]
    pub(crate) fn first_line_num(&self) -> usize {
        self.line_num + 1 - self.heading.text_lines
    }
}

/// Iterator over the headings recorded on a document's lines, in order.
///
/// Every recorded heading is a CommonMark heading; a line like `#NoSpace` is
/// paragraph text and never appears here (MD018 reads it from
/// [`LineInfo::atx_missing_space`]).
pub struct ValidHeadingsIter<'a> {
    lines: &'a [LineInfo],
    current_index: usize,
}

impl<'a> ValidHeadingsIter<'a> {
    pub(super) fn new(lines: &'a [LineInfo]) -> Self {
        Self {
            lines,
            current_index: 0,
        }
    }
}

impl<'a> Iterator for ValidHeadingsIter<'a> {
    type Item = ValidHeading<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.current_index < self.lines.len() {
            let idx = self.current_index;
            self.current_index += 1;

            let line_info = &self.lines[idx];
            if let Some(heading) = line_info.heading.as_deref() {
                return Some(ValidHeading {
                    line_num: idx + 1, // Convert 0-indexed to 1-indexed
                    heading,
                    line_info,
                });
            }
        }
        None
    }
}

/// Information about a blockquote line
#[derive(Debug, Clone)]
pub struct BlockquoteInfo {
    /// Nesting level (1 for >, 2 for >>, etc.)
    pub(crate) nesting_level: usize,
    /// The blockquote prefix (e.g., "> ", ">> ", etc.)
    pub(crate) prefix: String,
    /// Content after the blockquote marker(s)
    pub(crate) content: String,
    /// Whether the line has multiple spaces after the marker
    pub(crate) has_multiple_spaces_after_marker: bool,
}

/// Information about a list block
#[derive(Debug, Clone)]
pub struct ListBlock {
    /// Line number where the list starts (1-indexed)
    pub(crate) start_line: usize,
    /// Line number where the list ends (1-indexed)
    pub(crate) end_line: usize,
    /// Whether it's ordered or unordered
    pub(crate) is_ordered: bool,
    /// The consistent marker for unordered lists (if any)
    pub(crate) marker: Option<String>,
    /// Blockquote prefix for this list (empty if not in blockquote)
    pub(crate) blockquote_prefix: String,
    /// Lines that are list items within this block
    pub(crate) item_lines: Vec<usize>,
    /// Nesting level (0 for top-level lists)
    pub(crate) nesting_level: usize,
    /// Maximum marker width seen in this block (e.g., 3 for "1. ", 4 for "10. ")
    pub(crate) max_marker_width: usize,
}

/// A borrowed list item recognized in the parsed document.
///
/// This view gives rules stable access to list syntax and its source line
/// without exposing how list items are stored inside [`LineInfo`]. Columns are
/// the parser's existing source columns; rules that need visual columns must
/// continue to apply their established tab and container policy.
#[derive(Debug, Clone, Copy)]
pub struct ParsedListItem<'a> {
    line_num: usize,
    item: &'a ListItemInfo,
    line_info: &'a LineInfo,
}

impl<'a> ParsedListItem<'a> {
    pub(super) fn new(line_num: usize, item: &'a ListItemInfo, line_info: &'a LineInfo) -> Self {
        Self {
            line_num,
            item,
            line_info,
        }
    }

    /// The 1-indexed source line containing this item.
    #[inline]
    pub(crate) fn line_num(self) -> usize {
        self.line_num
    }

    /// Full metadata for the source line containing this item.
    #[inline]
    pub(crate) fn line_info(self) -> &'a LineInfo {
        self.line_info
    }

    /// The marker as parsed (`*`, `-`, `+`, or an ordered-list marker).
    #[inline]
    pub(crate) fn marker(self) -> &'a str {
        &self.item.marker
    }

    /// The first character of the marker, if present.
    #[inline]
    pub(crate) fn marker_char(self) -> Option<char> {
        self.item.marker.chars().next()
    }

    /// Whether this is an ordered-list item.
    #[inline]
    pub(crate) fn is_ordered(self) -> bool {
        self.item.is_ordered
    }

    /// Source column where the marker starts.
    #[inline]
    pub(crate) fn marker_column(self) -> usize {
        self.item.marker_column
    }

    /// Source column where content after the marker starts.
    #[inline]
    pub(crate) fn content_column(self) -> usize {
        self.item.content_column
    }

    /// Absolute byte offset where the marker starts.
    #[inline]
    pub(crate) fn marker_byte_offset(self) -> usize {
        self.line_info.byte_offset + self.item.marker_column
    }
}

/// A borrowed parsed list block and its items.
#[derive(Debug, Clone, Copy)]
pub struct ParsedListBlock<'a> {
    block: &'a ListBlock,
    lines: &'a [LineInfo],
}

impl<'a> ParsedListBlock<'a> {
    pub(super) fn new(block: &'a ListBlock, lines: &'a [LineInfo]) -> Self {
        Self { block, lines }
    }

    /// First source line in the block (1-indexed).
    #[inline]
    pub(crate) fn start_line(self) -> usize {
        self.block.start_line
    }

    /// Last source line in the block (1-indexed, inclusive).
    #[inline]
    pub(crate) fn end_line(self) -> usize {
        self.block.end_line
    }

    /// Blockquote prefix shared by the block.
    #[inline]
    pub(crate) fn blockquote_prefix(self) -> &'a str {
        &self.block.blockquote_prefix
    }

    /// Iterate over parsed items belonging to this block, in source order.
    pub(crate) fn items(self) -> ParsedListBlockItemsIter<'a> {
        ParsedListBlockItemsIter {
            item_lines: &self.block.item_lines,
            lines: self.lines,
            current_index: 0,
        }
    }
}

/// Borrowed collection of parsed list blocks.
#[derive(Debug, Clone, Copy)]
pub struct ParsedListBlocks<'a> {
    blocks: &'a [ListBlock],
    lines: &'a [LineInfo],
}

impl<'a> ParsedListBlocks<'a> {
    pub(super) fn new(blocks: &'a [ListBlock], lines: &'a [LineInfo]) -> Self {
        Self { blocks, lines }
    }

    #[inline]
    pub(crate) fn is_empty(self) -> bool {
        self.blocks.is_empty()
    }

    pub(crate) fn iter(self) -> ParsedListBlocksIter<'a> {
        ParsedListBlocksIter {
            blocks: self.blocks.iter(),
            lines: self.lines,
        }
    }
}

impl<'a> IntoIterator for ParsedListBlocks<'a> {
    type Item = ParsedListBlock<'a>;
    type IntoIter = ParsedListBlocksIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct ParsedListBlocksIter<'a> {
    blocks: std::slice::Iter<'a, ListBlock>,
    lines: &'a [LineInfo],
}

impl<'a> Iterator for ParsedListBlocksIter<'a> {
    type Item = ParsedListBlock<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.blocks.next().map(|block| ParsedListBlock::new(block, self.lines))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.blocks.size_hint()
    }
}

impl ExactSizeIterator for ParsedListBlocksIter<'_> {}

pub struct ParsedListBlockItemsIter<'a> {
    item_lines: &'a [usize],
    lines: &'a [LineInfo],
    current_index: usize,
}

impl<'a> Iterator for ParsedListBlockItemsIter<'a> {
    type Item = ParsedListItem<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(&line_num) = self.item_lines.get(self.current_index) {
            self.current_index += 1;
            let Some(line_index) = line_num.checked_sub(1) else {
                continue;
            };
            let Some(line_info) = self.lines.get(line_index) else {
                continue;
            };
            if let Some(item) = line_info.list_item.as_deref() {
                return Some(ParsedListItem::new(line_num, item, line_info));
            }
        }
        None
    }
}

/// Cached CommonMark membership for one ordered list.
#[derive(Debug, Clone)]
pub(super) struct CommonMarkOrderedListInfo {
    pub(super) start_value: u64,
    pub(super) item_lines: Vec<usize>,
}

/// A borrowed ordered list as grouped by the CommonMark parser.
///
/// This grouping is independent of visual list blocks: nested ordered lists
/// have their own membership and start value even when their source lines are
/// interleaved with the parent list.
#[derive(Debug, Clone, Copy)]
pub struct CommonMarkOrderedList<'a> {
    list: &'a CommonMarkOrderedListInfo,
    lines: &'a [LineInfo],
}

impl<'a> CommonMarkOrderedList<'a> {
    pub(super) fn new(list: &'a CommonMarkOrderedListInfo, lines: &'a [LineInfo]) -> Self {
        Self { list, lines }
    }

    /// The number on the first item, as interpreted by CommonMark.
    #[inline]
    pub(crate) fn start_value(self) -> u64 {
        self.list.start_value
    }

    /// Iterate over this list's ordered items in source order.
    pub(crate) fn items(self) -> CommonMarkOrderedListItemsIter<'a> {
        CommonMarkOrderedListItemsIter {
            item_lines: &self.list.item_lines,
            lines: self.lines,
            current_index: 0,
        }
    }
}

/// Borrowed collection of CommonMark-grouped ordered lists in source order.
#[derive(Debug, Clone, Copy)]
pub struct CommonMarkOrderedLists<'a> {
    lists: &'a [CommonMarkOrderedListInfo],
    lines: &'a [LineInfo],
}

impl<'a> CommonMarkOrderedLists<'a> {
    pub(super) fn new(lists: &'a [CommonMarkOrderedListInfo], lines: &'a [LineInfo]) -> Self {
        Self { lists, lines }
    }

    /// Whether the document has no CommonMark-grouped ordered lists.
    #[inline]
    pub(crate) fn is_empty(self) -> bool {
        self.lists.is_empty()
    }

    /// Iterate over ordered lists in the order of their first source item.
    pub(crate) fn iter(self) -> CommonMarkOrderedListsIter<'a> {
        CommonMarkOrderedListsIter {
            lists: self.lists.iter(),
            lines: self.lines,
        }
    }
}

impl<'a> IntoIterator for CommonMarkOrderedLists<'a> {
    type Item = CommonMarkOrderedList<'a>;
    type IntoIter = CommonMarkOrderedListsIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over CommonMark-grouped ordered lists.
pub struct CommonMarkOrderedListsIter<'a> {
    lists: std::slice::Iter<'a, CommonMarkOrderedListInfo>,
    lines: &'a [LineInfo],
}

impl<'a> Iterator for CommonMarkOrderedListsIter<'a> {
    type Item = CommonMarkOrderedList<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.lists
            .next()
            .map(|list| CommonMarkOrderedList::new(list, self.lines))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.lists.size_hint()
    }
}

impl ExactSizeIterator for CommonMarkOrderedListsIter<'_> {}

/// Iterator over the parsed items in one CommonMark ordered list.
pub struct CommonMarkOrderedListItemsIter<'a> {
    item_lines: &'a [usize],
    lines: &'a [LineInfo],
    current_index: usize,
}

impl<'a> Iterator for CommonMarkOrderedListItemsIter<'a> {
    type Item = ParsedListItem<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(&line_num) = self.item_lines.get(self.current_index) {
            self.current_index += 1;
            let Some(line_index) = line_num.checked_sub(1) else {
                continue;
            };
            let Some(line_info) = self.lines.get(line_index) else {
                continue;
            };
            let Some(item) = line_info.list_item.as_deref() else {
                continue;
            };
            if item.is_ordered {
                return Some(ParsedListItem::new(line_num, item, line_info));
            }
        }
        None
    }
}

/// Character frequency data for fast content analysis
#[derive(Debug, Clone, Default)]
pub struct CharFrequency {
    /// Count of # characters (headings)
    pub(crate) hash_count: usize,
    /// Count of * characters (emphasis, lists, horizontal rules)
    pub(crate) asterisk_count: usize,
    /// Count of _ characters (emphasis, horizontal rules)
    pub(crate) underscore_count: usize,
    /// Count of - characters (lists, horizontal rules, setext headings)
    pub(crate) hyphen_count: usize,
    /// Count of + characters (lists)
    pub(crate) plus_count: usize,
    /// Count of > characters (blockquotes)
    pub(crate) gt_count: usize,
    /// Count of | characters (tables)
    pub(crate) pipe_count: usize,
    /// Count of [ characters (links, images)
    pub(crate) bracket_count: usize,
    /// Count of ` characters (code spans, code blocks)
    pub(crate) backtick_count: usize,
    /// Count of < characters (HTML tags, autolinks)
    pub(crate) lt_count: usize,
    /// Count of ! characters (images)
    pub(crate) exclamation_count: usize,
    /// Count of newline characters
    pub(crate) newline_count: usize,
}

/// Pre-parsed HTML tag information
#[derive(Debug, Clone)]
pub struct HtmlTag {
    /// Line number (1-indexed)
    pub(crate) line: usize,
    /// Byte offset in document
    pub(crate) byte_offset: usize,
    /// End byte offset in document
    pub(crate) byte_end: usize,
    /// Tag name (e.g., "div", "img", "br")
    pub(crate) tag_name: String,
    /// Whether it's a closing tag (`</tag>`)
    pub(crate) is_closing: bool,
    /// Whether it's self-closing (`<tag />`)
    pub(crate) is_self_closing: bool,
}

/// Pre-parsed emphasis span information
#[derive(Debug, Clone)]
pub struct EmphasisSpan {}

/// A lazy continuation line detected by pulldown-cmark.
///
/// Lazy continuation occurs when text continues a list item paragraph but with less
/// indentation than expected.
#[derive(Debug, Clone)]
pub struct LazyContLine {
    /// 1-indexed line number
    pub(crate) line_num: usize,
    /// Expected indentation
    pub(crate) expected_indent: usize,
    /// Current indentation
    pub(crate) current_indent: usize,
    /// Blockquote nesting level
    pub(crate) blockquote_level: usize,
}

/// Check if a line is a horizontal rule (---, ***, ___) per CommonMark spec.
/// CommonMark rules for thematic breaks (horizontal rules):
/// - May have 0-3 spaces of leading indentation (but NOT tabs)
/// - Must have 3+ of the same character (-, *, or _)
/// - May have spaces between characters
/// - No other characters allowed
pub(crate) fn is_horizontal_rule_line(line: &str) -> bool {
    // CommonMark: HRs can have 0-3 spaces of leading indentation, not tabs
    let leading_spaces = line.len() - line.trim_start_matches(' ').len();
    if leading_spaces > 3 || line.starts_with('\t') {
        return false;
    }

    is_horizontal_rule_content(line.trim())
}

/// Check if trimmed content matches horizontal rule pattern.
/// Use `is_horizontal_rule_line` for full CommonMark compliance including indentation check.
pub(crate) fn is_horizontal_rule_content(trimmed: &str) -> bool {
    if trimmed.len() < 3 {
        return false;
    }

    let mut chars = trimmed.chars();
    let Some(first_char @ ('-' | '*' | '_')) = chars.next() else {
        return false;
    };

    // Count occurrences of the rule character, rejecting non-whitespace interlopers
    let mut count = 1; // Already matched the first character
    for ch in chars {
        if ch == first_char {
            count += 1;
        } else if ch != ' ' && ch != '\t' {
            return false;
        }
    }
    count >= 3
}

/// Check if content is a setext underline: a run of `=` or of `-`, leading and
/// trailing whitespace allowed, no internal spaces and no mixing of the two
/// markers. `= = =` is paragraph text, not an underline.
///
/// Callers working inside a container pass the content with the container's
/// prefix already stripped, so a blockquoted underline is recognized too.
pub(crate) fn is_setext_underline_content(content: &str) -> bool {
    let trimmed = content.trim();
    let mut chars = trimmed.chars();
    let Some(marker @ ('=' | '-')) = chars.next() else {
        return false;
    };
    chars.all(|c| c == marker)
}
