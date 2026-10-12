/// Shared table detection and processing utilities for markdown linting rules
///
/// This module provides optimized table detection and processing functionality
/// that can be shared across multiple table-related rules (MD055, MD056, MD058).
use super::blockquote::strip_blockquote_prefix;

/// Represents a table block in the document
#[derive(Debug, Clone)]
pub struct TableBlock {
    pub(crate) start_line: usize,
    pub end_line: usize,
    pub header_line: usize,
    pub delimiter_line: usize,
    pub content_lines: Vec<usize>,
    /// If the table is inside a list item, this contains:
    /// - The list marker prefix for the header line (e.g., "- ", "1. ")
    /// - The content indent (number of spaces for continuation lines)
    pub(crate) list_context: Option<ListTableContext>,
}

/// Context information for tables inside list items
#[derive(Debug, Clone)]
pub struct ListTableContext {
    /// The list marker prefix including any leading whitespace (e.g., "- ", "  1. ")
    pub(crate) list_prefix: String,
    /// Number of spaces for continuation lines to align with content
    pub(crate) content_indent: usize,
}

/// Shared table detection utilities
pub struct TableUtils;

impl TableUtils {
    /// Returns true if the line has at least one unescaped pipe separator outside inline code and
    /// math spans.
    ///
    /// Skips pipes inside backtick code spans (`` `...` ``) and dollar-sign math spans (`$...$`,
    /// `$$...$$`) to avoid false positives from prose like `` `echo a | sed 's/a/b/'` `` or math
    /// like `$|S|$` (absolute value notation).
    ///
    /// Note: a bare `$` that opens a span without a matching closing `$` keeps the scanner in
    /// math mode for the rest of the line, suppressing any subsequent pipes. This is conservative
    /// and means that `$5 | $10`-style price comparisons (without outer pipes) are not detected
    /// as table separators — an accepted trade-off to avoid false positives from real math.
    fn has_unescaped_pipe_outside_spans(text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        let mut in_code = false;
        let mut code_delim_len = 0usize;
        let mut in_math = false;
        let mut math_delim_len = 0usize;

        while i < chars.len() {
            let ch = chars[i];

            if ch == '\\' && !in_code && !in_math {
                // Skip escaped character (only outside code and math spans —
                // backslashes are literal inside code spans per CommonMark).
                i += if i + 1 < chars.len() { 2 } else { 1 };
                continue;
            }

            if ch == '`' && !in_math {
                let mut run = 1usize;
                while i + run < chars.len() && chars[i + run] == '`' {
                    run += 1;
                }

                if in_code {
                    if run == code_delim_len {
                        in_code = false;
                        code_delim_len = 0;
                    }
                    // Mismatched backtick run inside a code span: consumed but span stays open.
                } else {
                    in_code = true;
                    code_delim_len = run;
                }

                i += run;
                continue;
            }

            if ch == '$' && !in_code {
                let mut run = 1usize;
                while i + run < chars.len() && chars[i + run] == '$' {
                    run += 1;
                }

                if in_math {
                    if run == math_delim_len {
                        in_math = false;
                        math_delim_len = 0;
                    }
                    // Mismatched $-run inside a math span: consumed but span stays open.
                } else {
                    in_math = true;
                    math_delim_len = run;
                }

                i += run;
                continue;
            }

            if ch == '|' && !in_code && !in_math {
                return true;
            }

            i += 1;
        }

        false
    }

    /// Check if a line looks like a potential table row
    /// Flavor-aware form of [`Self::is_potential_table_row`]
    ///
    /// Under Obsidian, a line whose only pipes sit inside wikilink aliases is
    /// prose rather than a table row, so those pipes are masked before the check.
    pub(crate) fn is_potential_table_row_with_flavor(line: &str, _flavor: crate::config::MarkdownFlavor) -> bool {
        // Masking is the identity without an opener to mask inside, and this runs on
        // every line of the document.
        Self::is_potential_table_row(line)
    }

    pub(crate) fn is_potential_table_row(line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.contains('|') {
            return false;
        }

        // Skip lines that are clearly not table rows
        // Unordered list items with space or tab after marker
        if trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("+ ")
            || trimmed.starts_with("-\t")
            || trimmed.starts_with("*\t")
            || trimmed.starts_with("+\t")
        {
            return false;
        }

        // Skip ordered list items: digits followed by . or ) then space/tab
        if let Some(first_non_digit) = trimmed.find(|c: char| !c.is_ascii_digit())
            && first_non_digit > 0
        {
            let after_digits = &trimmed[first_non_digit..];
            if after_digits.starts_with(". ")
                || after_digits.starts_with(".\t")
                || after_digits.starts_with(") ")
                || after_digits.starts_with(")\t")
            {
                return false;
            }
        }

        // Skip ATX headings (# through ######)
        if trimmed.starts_with('#') {
            let hash_count = trimmed.bytes().take_while(|&b| b == b'#').count();
            if hash_count <= 6 {
                let after_hashes = &trimmed[hash_count..];
                if after_hashes.is_empty() || after_hashes.starts_with(' ') || after_hashes.starts_with('\t') {
                    return false;
                }
            }
        }

        // For rows without explicit outer pipes, require a real separator outside
        // inline code and math spans to avoid prose/command false positives.
        let has_outer_pipes = trimmed.starts_with('|') && trimmed.ends_with('|');
        if !has_outer_pipes && !Self::has_unescaped_pipe_outside_spans(trimmed) {
            return false;
        }

        // Must have at least 2 parts when split by |
        let parts: Vec<&str> = trimmed.split('|').collect();
        if parts.len() < 2 {
            return false;
        }

        // Check if it looks like a table row by having reasonable content between pipes
        let mut valid_parts = 0;
        let mut total_non_empty_parts = 0;

        for part in &parts {
            let part_trimmed = part.trim();
            // Skip empty parts (from leading/trailing pipes)
            if part_trimmed.is_empty() {
                continue;
            }
            total_non_empty_parts += 1;

            // Count parts that look like table cells (reasonable content, no newlines)
            if !part_trimmed.contains('\n') {
                valid_parts += 1;
            }
        }

        // Check if all non-empty parts are valid (no newlines)
        if total_non_empty_parts > 0 && valid_parts != total_non_empty_parts {
            // Some cells contain newlines, not a valid table row
            return false;
        }

        // GFM allows tables with all empty cells (e.g., |||)
        // These are valid if they have proper table formatting (leading and trailing pipes)
        if total_non_empty_parts == 0 {
            // Empty cells are only valid with proper pipe formatting
            return trimmed.starts_with('|') && trimmed.ends_with('|') && parts.len() >= 3;
        }

        // GFM allows single-column tables, so >= 1 valid part is enough
        // when the line has proper table formatting (pipes)
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            // Properly formatted table row with pipes on both ends
            valid_parts >= 1
        } else {
            // For rows without proper pipe formatting, require at least 2 cells
            valid_parts >= 2
        }
    }

    /// Check if a line is a table delimiter row (e.g., |---|---|)
    pub(crate) fn is_delimiter_row(line: &str) -> bool {
        let trimmed = line.trim();
        if !trimmed.contains('|') || !trimmed.contains('-') {
            return false;
        }

        // Split by pipes and check each part
        let parts: Vec<&str> = trimmed.split('|').collect();
        let mut valid_delimiter_parts = 0;
        let mut total_non_empty_parts = 0;

        for part in &parts {
            let part_trimmed = part.trim();
            if part_trimmed.is_empty() {
                continue; // Skip empty parts from leading/trailing pipes
            }

            total_non_empty_parts += 1;

            // Check if this part looks like a delimiter (contains dashes and optionally colons)
            if part_trimmed.chars().all(|c| c == '-' || c == ':' || c.is_whitespace()) && part_trimmed.contains('-') {
                valid_delimiter_parts += 1;
            }
        }

        // All non-empty parts must be valid delimiters, and there must be at least one
        total_non_empty_parts > 0 && valid_delimiter_parts == total_non_empty_parts
    }

    /// Find all table blocks in the content with optimized detection
    /// This version accepts code_blocks and code_spans directly for use during LintContext construction
    pub(crate) fn find_table_blocks_with_code_info(
        content: &str,
        code_blocks: &[(usize, usize)],
        code_spans: &[crate::lint_context::CodeSpan],
        html_comment_ranges: &[crate::utils::skip_context::ByteRange],
        flavor: crate::config::MarkdownFlavor,
    ) -> Vec<TableBlock> {
        let lines: Vec<&str> = content.lines().collect();
        let mut tables = Vec::new();
        let mut i = 0;

        // Pre-compute line positions for efficient code block checking.
        // `str::lines()` strips the trailing `\r` from CRLF lines, so advancing by
        // `line.len() + 1` undercounts by one byte per CRLF line; the positions then
        // drift out of sync with the raw byte offsets that `code_blocks` uses, which
        // can misclassify a later table header as being inside a code block. Walk the
        // actual line terminator (`\n` or `\r\n`) from the raw bytes instead.
        let mut line_positions = Vec::with_capacity(lines.len());
        let content_bytes = content.as_bytes();
        let mut pos = 0;
        for line in &lines {
            line_positions.push(pos);
            pos += line.len();
            if content_bytes.get(pos) == Some(&b'\r') {
                pos += 1;
            }
            if content_bytes.get(pos) == Some(&b'\n') {
                pos += 1;
            }
        }

        // Stack of active list content indents for continuation table tracking.
        // Supports nested lists: when a child list is seen, we push; when we
        // dedent past a level, we pop back to the enclosing list.
        let mut list_indent_stack: Vec<usize> = Vec::new();

        while i < lines.len() {
            // Skip lines in code blocks, code spans, or HTML comments
            let line_start = line_positions[i];
            let in_code =
                crate::utils::code_block_utils::CodeBlockUtils::is_in_code_block_or_span(code_blocks, line_start) || {
                    // Binary search on sorted code spans
                    let idx = code_spans.partition_point(|span| span.byte_offset <= line_start);
                    idx > 0 && line_start < code_spans[idx - 1].byte_end
                };
            let in_html_comment = {
                // Binary search on sorted HTML comment ranges
                let idx = html_comment_ranges.partition_point(|range| range.start <= line_start);
                idx > 0 && line_start < html_comment_ranges[idx - 1].end
            };

            if in_code || in_html_comment {
                i += 1;
                continue;
            }

            // Strip blockquote prefix for table detection
            let line_content = strip_blockquote_prefix(lines[i]);

            // Update active list tracking
            let (list_prefix, list_content, content_indent) = Self::extract_list_prefix(line_content);
            if !list_prefix.is_empty() {
                // Line has a list marker. Pop any deeper/equal levels, then push this one.
                while list_indent_stack.last().is_some_and(|&top| top >= content_indent) {
                    list_indent_stack.pop();
                }
                list_indent_stack.push(content_indent);
            } else if !line_content.trim().is_empty() {
                // Non-blank line without a marker: pop any levels we've dedented past
                let leading = line_content.len() - line_content.trim_start().len();
                while list_indent_stack.last().is_some_and(|&top| leading < top) {
                    list_indent_stack.pop();
                }
            }
            // Blank lines keep the stack unchanged (blank lines don't end list items)

            // Check if this is a list item that contains a table row on the same line,
            // or a continuation table indented under an active list item
            let (is_same_line_list_table, effective_content) =
                if !list_prefix.is_empty() && Self::is_potential_table_row_content(list_content, flavor) {
                    (true, list_content)
                } else {
                    (false, line_content)
                };

            // Detect continuation list tables: no marker on this line, but indented
            // under an active list item (e.g., "- Text\n  | h1 | h2 |")
            let continuation_indent = if !is_same_line_list_table && list_prefix.is_empty() {
                let leading = line_content.len() - line_content.trim_start().len();
                // Find the deepest list level this line is indented under
                list_indent_stack
                    .iter()
                    .rev()
                    .find(|&&indent| leading >= indent)
                    .copied()
            } else {
                None
            };

            let is_continuation_list_table = continuation_indent.is_some()
                && {
                    let indent = continuation_indent.unwrap();
                    let leading = line_content.len() - line_content.trim_start().len();
                    // Per CommonMark, 4+ spaces beyond content indent is a code block
                    leading < indent + 4
                }
                && Self::is_potential_table_row_with_flavor(effective_content, flavor);

            let is_any_list_table = is_same_line_list_table || is_continuation_list_table;

            // For continuation list tables, use the matched list indent
            let effective_content_indent = if is_same_line_list_table {
                content_indent
            } else if is_continuation_list_table {
                continuation_indent.unwrap()
            } else {
                0
            };

            // Look for potential table start
            if is_any_list_table || Self::is_potential_table_row_with_flavor(effective_content, flavor) {
                // For list tables (same-line or continuation), check indented continuation lines
                // For regular tables, check the next line directly
                let (next_line_content, delimiter_has_valid_indent) = if i + 1 < lines.len() {
                    let next_raw = strip_blockquote_prefix(lines[i + 1]);
                    if is_any_list_table {
                        // Verify the delimiter line has proper indentation
                        let leading_spaces = next_raw.len() - next_raw.trim_start().len();
                        if leading_spaces >= effective_content_indent {
                            // Has proper indentation, strip it and check as delimiter
                            (
                                Self::strip_list_continuation_indent(next_raw, effective_content_indent),
                                true,
                            )
                        } else {
                            // Not enough indentation - not a list table
                            (next_raw, false)
                        }
                    } else {
                        (next_raw, true)
                    }
                } else {
                    ("", true)
                };

                // For list tables, only accept if delimiter has valid indentation
                let effective_is_list_table = is_any_list_table && delimiter_has_valid_indent;

                if i + 1 < lines.len() && Self::is_delimiter_row(next_line_content) {
                    // Found a table! Find its end
                    let table_start = i;
                    let header_line = i;
                    let delimiter_line = i + 1;
                    let mut table_end = i + 1; // Include the delimiter row
                    let mut content_lines = Vec::new();

                    // Continue while we have table rows
                    let mut j = i + 2;
                    while j < lines.len() {
                        let line = lines[j];
                        // Strip blockquote prefix for checking
                        let raw_content = strip_blockquote_prefix(line);

                        // For list tables, strip expected indentation
                        let line_content = if effective_is_list_table {
                            Self::strip_list_continuation_indent(raw_content, effective_content_indent)
                        } else {
                            raw_content
                        };

                        if line_content.trim().is_empty() {
                            // Empty line ends the table
                            break;
                        }

                        // For list tables, the continuation line must have proper indentation
                        if effective_is_list_table {
                            let leading_spaces = raw_content.len() - raw_content.trim_start().len();
                            if leading_spaces < effective_content_indent {
                                // Not enough indentation - end of table
                                break;
                            }
                        }

                        if Self::is_potential_table_row_with_flavor(line_content, flavor) {
                            content_lines.push(j);
                            table_end = j;
                            j += 1;
                        } else {
                            // Non-table line ends the table
                            break;
                        }
                    }

                    let list_context = if effective_is_list_table {
                        if is_same_line_list_table {
                            // Same-line: prefix is the actual list marker (e.g., "- ")
                            Some(ListTableContext {
                                list_prefix: list_prefix.to_string(),
                                content_indent: effective_content_indent,
                            })
                        } else {
                            // Continuation: prefix is the indentation spaces
                            Some(ListTableContext {
                                list_prefix: " ".repeat(effective_content_indent),
                                content_indent: effective_content_indent,
                            })
                        }
                    } else {
                        None
                    };

                    tables.push(TableBlock {
                        start_line: table_start,
                        end_line: table_end,
                        header_line,
                        delimiter_line,
                        content_lines,
                        list_context,
                    });
                    i = table_end + 1;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }

        tables
    }

    /// Strip list continuation indentation from a line.
    /// For lines that are continuations of a list item's content, strip the expected indent.
    fn strip_list_continuation_indent(line: &str, expected_indent: usize) -> &str {
        let bytes = line.as_bytes();
        let mut spaces = 0;

        for &b in bytes {
            if b == b' ' {
                spaces += 1;
            } else if b == b'\t' {
                // Tab counts as up to 4 spaces, rounding up to next multiple of 4
                spaces = (spaces / 4 + 1) * 4;
            } else {
                break;
            }

            if spaces >= expected_indent {
                break;
            }
        }

        // Strip at most expected_indent characters
        let strip_count = spaces.min(expected_indent).min(line.len());
        // Count actual bytes to strip (handling tabs)
        let mut byte_count = 0;
        let mut counted_spaces = 0;
        for &b in bytes {
            if counted_spaces >= strip_count {
                break;
            }
            if b == b' ' {
                counted_spaces += 1;
                byte_count += 1;
            } else if b == b'\t' {
                counted_spaces = (counted_spaces / 4 + 1) * 4;
                byte_count += 1;
            } else {
                break;
            }
        }

        &line[byte_count..]
    }

    /// Count the number of cells in a table row with flavor-specific behavior
    ///
    /// Pipes inside code spans are treated as content, not cell delimiters.
    ///
    /// This function strips blockquote prefixes before counting cells, so it works
    /// correctly for tables inside blockquotes.
    pub fn count_cells_with_flavor(row: &str, flavor: crate::config::MarkdownFlavor) -> usize {
        // Strip blockquote prefix if present before counting cells
        let (_, content) = Self::extract_blockquote_prefix(row);
        Self::split_table_row_with_flavor(content, flavor).len()
    }

    /// Count the number of consecutive backslashes immediately preceding `pos` in `chars`.
    fn count_preceding_backslashes(chars: &[char], pos: usize) -> usize {
        let mut count = 0;
        let mut k = pos;
        while k > 0 {
            k -= 1;
            if chars[k] == '\\' {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Locate the inline code spans in `chars` as half-open char ranges covering
    /// the opening delimiter, the content and the closing delimiter.
    ///
    /// Backticks preceded by an odd number of backslashes are escaped (literal text)
    /// and do not open or close code spans. An even number of backslashes means the
    /// backslashes themselves are escaped, so the backtick is a real delimiter. A run
    /// of backticks with no matching closing run of the same length is literal text,
    /// and scanning resumes just after it.
    ///
    /// This is the single definition of a code span for table parsing: both pipe
    /// masking and wikilink detection read it, so they cannot disagree about where
    /// code starts and ends.
    fn inline_code_spans(chars: &[char]) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] != '`' {
                i += 1;
                continue;
            }

            // A backtick preceded by an odd number of backslashes is escaped
            if Self::count_preceding_backslashes(chars, i) % 2 != 0 {
                i += 1;
                continue;
            }

            // Count consecutive backticks at start
            let start = i;
            let mut backtick_count = 0;
            while i < chars.len() && chars[i] == '`' {
                backtick_count += 1;
                i += 1;
            }

            // Look for a closing run of exactly the same length. Per CommonMark,
            // backslash escapes do NOT work inside code spans -- all characters
            // including backslashes are literal -- so no escape check applies here.
            let mut j = i;
            while j < chars.len() {
                if chars[j] == '`' {
                    let mut close_count = 0;
                    while j < chars.len() && chars[j] == '`' {
                        close_count += 1;
                        j += 1;
                    }

                    if close_count == backtick_count {
                        spans.push((start, j));
                        i = j;
                        break;
                    }
                    // Run of a different length: keep searching (j is already past it)
                } else {
                    j += 1;
                }
            }
            // With no matching closing run the opener is literal text; `i` already
            // sits just past it, so a later backtick can still open a span.
        }

        spans
    }

    /// Mask pipes inside inline code blocks with a placeholder character.
    ///
    /// The mask is the same byte width as what it replaces, so offsets into the
    /// masked string still address the original text.
    pub(crate) fn mask_pipes_in_inline_code(text: &str) -> String {
        if !text.contains('`') {
            return text.to_string();
        }

        let chars: Vec<char> = text.chars().collect();
        let spans = Self::inline_code_spans(&chars);
        if spans.is_empty() {
            return text.to_string();
        }

        let mut result = String::with_capacity(text.len());
        let mut cursor = 0;
        for (start, end) in spans {
            result.extend(chars[cursor..start].iter());
            // The delimiters are backticks, so masking every pipe across the whole
            // span leaves them untouched and only rewrites the content.
            for &ch in &chars[start..end] {
                result.push(if ch == '|' { '_' } else { ch });
            }
            cursor = end;
        }
        result.extend(chars[cursor..].iter());

        result
    }

    /// Mask escaped pipes for accurate table cell parsing
    ///
    /// In GFM tables, escape handling happens BEFORE cell boundary detection:
    /// - `\|` → escaped pipe → masked (stays as cell content)
    /// - `\\|` → escaped backslash + pipe → NOT masked (pipe is a delimiter)
    ///
    /// This function only handles escaped pipes. Pipes inside inline code spans
    /// are handled separately by `mask_pipes_in_inline_code`.
    pub(crate) fn mask_pipes_for_table_parsing(text: &str) -> String {
        let mut result = String::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '\\' {
                if i + 1 < chars.len() && chars[i + 1] == '\\' {
                    // Escaped backslash: \\ → push both and continue
                    // The next character (if it's a pipe) will be a real delimiter
                    result.push('\\');
                    result.push('\\');
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '|' {
                    // Escaped pipe: \| → mask the pipe
                    result.push('\\');
                    result.push('_'); // Mask the pipe
                    i += 2;
                } else {
                    // Single backslash not followed by \ or | → just push it
                    result.push(chars[i]);
                    i += 1;
                }
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }

        result
    }

    /// Split a table row into individual cell contents with flavor-specific behavior.
    ///
    /// Returns a Vec of cell content strings (not trimmed - preserves original spacing).
    /// This is the foundation for both cell counting and cell content extraction.
    ///
    /// Pipes inside code spans are treated as content, not cell delimiters.
    pub fn split_table_row_with_flavor(row: &str, _flavor: crate::config::MarkdownFlavor) -> Vec<String> {
        let trimmed = row.trim();

        if !trimmed.contains('|') {
            return Vec::new();
        }

        // First, mask escaped pipes (same for all flavors)
        let masked = Self::mask_pipes_for_table_parsing(trimmed);

        // Mask pipes inside inline code for all flavors
        let final_masked = Self::mask_pipes_in_inline_code(&masked);

        // In Obsidian, a pipe inside [[Target|Label]] separates the link from its
        // alias rather than one cell from the next

        let has_leading = final_masked.starts_with('|');
        let has_trailing = final_masked.ends_with('|');

        let mut masked_content = final_masked.as_str();
        let mut orig_content = trimmed;

        if has_leading {
            masked_content = &masked_content[1..];
            orig_content = &orig_content[1..];
        }

        // Track whether we actually strip a trailing pipe
        let stripped_trailing = has_trailing && !masked_content.is_empty();
        if stripped_trailing {
            masked_content = &masked_content[..masked_content.len() - 1];
            orig_content = &orig_content[..orig_content.len() - 1];
        }

        // Handle edge cases for degenerate inputs
        if masked_content.is_empty() {
            if stripped_trailing {
                // "||" case: two pipes with empty content between = one empty cell
                return vec![String::new()];
            } else {
                // "|" case: single pipe, not a valid table row
                return Vec::new();
            }
        }

        let masked_parts: Vec<&str> = masked_content.split('|').collect();
        let mut cells = Vec::new();
        let mut pos = 0;

        for masked_cell in masked_parts {
            let cell_len = masked_cell.len();
            let orig_cell = if pos + cell_len <= orig_content.len() {
                &orig_content[pos..pos + cell_len]
            } else {
                masked_cell
            };
            cells.push(orig_cell.to_string());
            pos += cell_len + 1; // +1 for the pipe delimiter
        }

        cells
    }

    /// Determine the pipe style of a table row
    ///
    /// Handles tables inside blockquotes by stripping the blockquote prefix
    /// before analyzing the pipe style.
    pub(crate) fn determine_pipe_style(line: &str) -> Option<&'static str> {
        // Strip blockquote prefix if present before analyzing pipe style
        let content = strip_blockquote_prefix(line);
        let trimmed = content.trim();
        if !trimmed.contains('|') {
            return None;
        }

        let has_leading = trimmed.starts_with('|');
        let has_trailing = trimmed.ends_with('|');

        match (has_leading, has_trailing) {
            (true, true) => Some("leading_and_trailing"),
            (true, false) => Some("leading_only"),
            (false, true) => Some("trailing_only"),
            (false, false) => Some("no_leading_or_trailing"),
        }
    }

    /// Extract blockquote prefix from a line, returning (prefix, content).
    ///
    /// This is useful for stripping the prefix before processing, then restoring it after.
    /// For example: `"> | H1 | H2 |"` returns `("> ", "| H1 | H2 |")`.
    pub(crate) fn extract_blockquote_prefix(line: &str) -> (&str, &str) {
        // Find where the actual content starts (after blockquote markers and spaces)
        let bytes = line.as_bytes();
        let mut pos = 0;

        // Skip leading whitespace (indent before blockquote marker)
        while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
            pos += 1;
        }

        // If no blockquote marker, return empty prefix
        if pos >= bytes.len() || bytes[pos] != b'>' {
            return ("", line);
        }

        // Skip all blockquote markers and spaces
        while pos < bytes.len() {
            if bytes[pos] == b'>' {
                pos += 1;
                // Skip optional space after >
                if pos < bytes.len() && bytes[pos] == b' ' {
                    pos += 1;
                }
            } else if bytes[pos] == b' ' || bytes[pos] == b'\t' {
                pos += 1;
            } else {
                break;
            }
        }

        // Split at the position where content starts
        (&line[..pos], &line[pos..])
    }

    /// Extract list marker prefix from a line, returning (prefix, content, content_indent).
    ///
    /// This handles unordered list markers (`-`, `*`, `+`) and ordered list markers (`1.`, `10)`, etc.)
    /// Returns:
    /// - prefix: The list marker including any leading whitespace and trailing space (e.g., "- ", "  1. ")
    /// - content: The content after the list marker
    /// - content_indent: The number of spaces needed for continuation lines to align with content
    ///
    /// For example:
    /// - `"- | H1 | H2 |"` returns `("- ", "| H1 | H2 |", 2)`
    /// - `"1. | H1 | H2 |"` returns `("1. ", "| H1 | H2 |", 3)`
    /// - `"  - table"` returns `("  - ", "table", 4)`
    ///
    /// Returns `("", line, 0)` if the line doesn't start with a list marker.
    pub(crate) fn extract_list_prefix(line: &str) -> (&str, &str, usize) {
        let bytes = line.as_bytes();

        // Skip leading whitespace
        let leading_spaces = bytes.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
        let mut pos = leading_spaces;

        if pos >= bytes.len() {
            return ("", line, 0);
        }

        // Check for unordered list marker: -, *, +
        if matches!(bytes[pos], b'-' | b'*' | b'+') {
            pos += 1;

            // Must be followed by space or tab (or end of line for marker-only lines)
            if pos >= bytes.len() || bytes[pos] == b' ' || bytes[pos] == b'\t' {
                // Skip the space after marker if present
                if pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
                    pos += 1;
                }
                let content_indent = pos;
                return (&line[..pos], &line[pos..], content_indent);
            }
            // Not a list marker (e.g., "-word" or "--")
            return ("", line, 0);
        }

        // Check for ordered list marker: digits followed by . or ) then space
        if bytes[pos].is_ascii_digit() {
            let digit_start = pos;
            while pos < bytes.len() && bytes[pos].is_ascii_digit() {
                pos += 1;
            }

            // Must have at least one digit
            if pos > digit_start && pos < bytes.len() {
                // Check for . or ) followed by space/tab
                if bytes[pos] == b'.' || bytes[pos] == b')' {
                    pos += 1;
                    if pos >= bytes.len() || bytes[pos] == b' ' || bytes[pos] == b'\t' {
                        // Skip the space after marker if present
                        if pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
                            pos += 1;
                        }
                        let content_indent = pos;
                        return (&line[..pos], &line[pos..], content_indent);
                    }
                }
            }
        }

        ("", line, 0)
    }

    /// Extract the table row content from a line, stripping any list/blockquote prefix.
    ///
    /// This is useful for processing table rows that may be inside list items or blockquotes.
    /// The line_index indicates which line of the table this is (0 = header, 1 = delimiter, etc.)
    pub fn extract_table_row_content<'a>(line: &'a str, table_block: &TableBlock, line_index: usize) -> &'a str {
        // First strip blockquote prefix
        let (_, after_blockquote) = Self::extract_blockquote_prefix(line);

        // Then handle list prefix if present
        if let Some(ref list_ctx) = table_block.list_context {
            if line_index == 0 {
                // Header line: strip list prefix (handles both markers and indentation)
                after_blockquote
                    .strip_prefix(&list_ctx.list_prefix)
                    .unwrap_or_else(|| Self::extract_list_prefix(after_blockquote).1)
            } else {
                // Continuation lines: strip indentation
                Self::strip_list_continuation_indent(after_blockquote, list_ctx.content_indent)
            }
        } else {
            after_blockquote
        }
    }

    /// Internal helper: Check if content (without list/blockquote prefix) looks like a table row.
    fn is_potential_table_row_content(content: &str, flavor: crate::config::MarkdownFlavor) -> bool {
        Self::is_potential_table_row_with_flavor(content, flavor)
    }
}
