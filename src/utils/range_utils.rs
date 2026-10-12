//! Utilities for position/range conversions

use std::collections::HashSet;
use std::ops::Range;

/// Find the nearest valid UTF-8 character boundary at or before the given byte index.
/// This is critical for safely slicing strings that may contain multi-byte UTF-8 characters.
///
/// # Safety
/// Returns a byte index that is guaranteed to be a valid character boundary,
/// or the string length if the index is beyond the string.
fn find_char_boundary(s: &str, byte_idx: usize) -> usize {
    if byte_idx >= s.len() {
        return s.len();
    }

    // If the index is already at a character boundary, return it
    if s.is_char_boundary(byte_idx) {
        return byte_idx;
    }

    // Find the nearest character boundary by scanning backwards
    // This is safe because we know byte_idx < s.len()
    let mut pos = byte_idx;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Convert a byte index within a line into a 1-indexed character column.
///
/// rumdl reports diagnostic columns as character offsets, not byte offsets, so
/// any position derived from a byte index (regex match, `str::find`, parser byte
/// offset) must pass through this before being stored in a `LintWarning`.
/// Multi-byte UTF-8 characters are handled by snapping to the nearest character
/// boundary at or before `byte_idx`.
pub(crate) fn byte_to_char_count(s: &str, byte_idx: usize) -> usize {
    let safe_byte_idx = find_char_boundary(s, byte_idx);
    s[..safe_byte_idx].chars().count() + 1 // 1-indexed
}

#[derive(Debug)]
pub struct LineIndex<'a> {
    line_starts: Vec<usize>,
    content: &'a str,
}

impl<'a> LineIndex<'a> {
    /// Create a `LineIndex` from pre-computed line starts and code block byte ranges.
    ///
    /// Instead of re-scanning content to find code blocks, this converts
    /// the already-detected byte ranges into line-level information.
    pub(crate) fn with_line_starts_and_code_blocks(
        content: &'a str,
        line_starts: Vec<usize>,
        code_block_byte_ranges: &[(usize, usize)],
    ) -> Self {
        let mut code_block_lines = HashSet::new();

        for &(block_start, block_end) in code_block_byte_ranges {
            let start_line = match line_starts.binary_search(&block_start) {
                Ok(idx) => idx,
                Err(idx) => idx.saturating_sub(1),
            };
            let end_line = if block_end == 0 {
                0
            } else {
                match line_starts.binary_search(&block_end) {
                    // block_end exactly at a line start means the block ended on the previous line
                    Ok(idx) => idx.saturating_sub(1),
                    Err(idx) => idx.saturating_sub(1),
                }
            };
            for line_idx in start_line..=end_line {
                code_block_lines.insert(line_idx);
            }
        }

        Self { line_starts, content }
    }

    /// Get the content of a line by 0-based index using pre-computed byte offsets.
    /// Returns the line content without the trailing newline character.
    fn get_line(&self, line_idx: usize) -> Option<&'a str> {
        let start = *self.line_starts.get(line_idx)?;
        let end = self
            .line_starts
            .get(line_idx + 1)
            .copied()
            .unwrap_or(self.content.len());
        let line = &self.content[start..end];
        // Strip trailing newline (and optional \r before it)
        let line = line.strip_suffix('\n').unwrap_or(line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        Some(line)
    }

    pub(crate) fn line_col_to_byte_range(&self, line: usize, column: usize) -> Range<usize> {
        let line = line.saturating_sub(1);
        let line_start = *self.line_starts.get(line).unwrap_or(&self.content.len());

        let current_line = self.get_line(line).unwrap_or("");
        // Column is 1-indexed character position, not byte position
        let char_col = column.saturating_sub(1);
        let char_count = current_line.chars().count();
        let safe_char_col = char_col.min(char_count);

        // Convert character position to byte position
        let byte_offset = current_line
            .char_indices()
            .nth(safe_char_col)
            .map_or(current_line.len(), |(idx, _)| idx);

        let start = line_start + byte_offset;
        start..start
    }

    /// Calculate a proper byte range for replacing text with a specific length
    /// This is the correct function to use for LSP fixes
    ///
    /// # Safety
    /// This function correctly handles multi-byte UTF-8 characters by converting
    /// character positions (columns) to byte positions.
    pub(crate) fn line_col_to_byte_range_with_length(&self, line: usize, column: usize, length: usize) -> Range<usize> {
        let line = line.saturating_sub(1);
        let line_start = *self.line_starts.get(line).unwrap_or(&self.content.len());
        let line_end = self.line_starts.get(line + 1).copied().unwrap_or(self.content.len());
        let mut current_line = &self.content[line_start..line_end];
        if let Some(stripped) = current_line.strip_suffix('\n') {
            current_line = stripped.strip_suffix('\r').unwrap_or(stripped);
        }
        if current_line.is_ascii() {
            let line_len = current_line.len();
            let start_byte = column.saturating_sub(1).min(line_len);
            let end_byte = start_byte.saturating_add(length).min(line_len);
            let start = line_start + start_byte;
            let end = line_start + end_byte;
            return start..end;
        }
        // Column is 1-indexed character position, not byte position
        let char_col = column.saturating_sub(1);
        let char_count = current_line.chars().count();
        let safe_char_col = char_col.min(char_count);

        // Convert character positions to byte positions
        let mut char_indices = current_line.char_indices();
        let start_byte = char_indices
            .nth(safe_char_col)
            .map_or(current_line.len(), |(idx, _)| idx);

        // Calculate end position (start + length in characters)
        let end_char_col = (safe_char_col + length).min(char_count);
        let end_byte = current_line
            .char_indices()
            .nth(end_char_col)
            .map_or(current_line.len(), |(idx, _)| idx);

        let start = line_start + start_byte;
        let end = line_start + end_byte;
        start..end
    }

    /// Calculate byte range for entire line replacement (including newline)
    /// This is ideal for rules that need to replace complete lines
    pub(crate) fn whole_line_range(&self, line: usize) -> Range<usize> {
        let line_idx = line.saturating_sub(1);
        let start = *self.line_starts.get(line_idx).unwrap_or(&self.content.len());
        let end = self
            .line_starts
            .get(line_idx + 1)
            .copied()
            .unwrap_or(self.content.len());
        start..end
    }

    /// Calculate byte range spanning multiple lines (from start_line to end_line inclusive)
    /// Both lines are 1-indexed. This is useful for replacing entire blocks like tables.
    pub(crate) fn multi_line_range(&self, start_line: usize, end_line: usize) -> Range<usize> {
        let start_idx = start_line.saturating_sub(1);
        let end_idx = end_line.saturating_sub(1);

        let start = *self.line_starts.get(start_idx).unwrap_or(&self.content.len());
        let end = self.line_starts.get(end_idx + 1).copied().unwrap_or(self.content.len());
        start..end
    }

    /// Calculate byte range for text within a line (excluding newline)
    /// Useful for replacing specific parts of a line
    ///
    /// # Safety
    /// This function correctly handles multi-byte UTF-8 characters by converting
    /// character positions (columns) to byte positions.
    pub(crate) fn line_text_range(&self, line: usize, start_col: usize, end_col: usize) -> Range<usize> {
        let line_idx = line.saturating_sub(1);
        let line_start = *self.line_starts.get(line_idx).unwrap_or(&self.content.len());

        // Get the actual line content to ensure we don't exceed bounds
        let current_line = self.get_line(line_idx).unwrap_or("");
        let char_count = current_line.chars().count();

        // Convert character positions to byte positions
        let start_char_col = start_col.saturating_sub(1).min(char_count);
        let end_char_col = end_col.saturating_sub(1).min(char_count);

        let mut char_indices = current_line.char_indices();
        let start_byte = char_indices
            .nth(start_char_col)
            .map_or(current_line.len(), |(idx, _)| idx);

        let end_byte = current_line
            .char_indices()
            .nth(end_char_col)
            .map_or(current_line.len(), |(idx, _)| idx);

        let start = line_start + start_byte;
        let end = line_start + end_byte.max(start_byte);
        start..end
    }

    /// Calculate byte range from start of line to end of line content (excluding newline)
    /// Useful for replacing line content while preserving line structure
    pub(crate) fn line_content_range(&self, line: usize) -> Range<usize> {
        let line_idx = line.saturating_sub(1);
        let line_start = *self.line_starts.get(line_idx).unwrap_or(&self.content.len());

        let current_line = self.get_line(line_idx).unwrap_or("");
        let line_end = line_start + current_line.len();
        line_start..line_end
    }

    /// Get the global start byte offset for a given 1-based line number.
    pub(crate) fn get_line_start_byte(&self, line_num: usize) -> Option<usize> {
        if line_num == 0 {
            return None; // Lines are 1-based
        }
        // line_num is 1-based, line_starts index is 0-based
        self.line_starts.get(line_num - 1).copied()
    }
}

/// Calculate end position for a single-line range
pub(crate) fn calculate_single_line_range(
    line: usize,
    start_col: usize,
    length: usize,
) -> (usize, usize, usize, usize) {
    (line, start_col, line, start_col + length)
}

/// Calculate range for entire line.
///
/// The end column is a character count (rumdl's diagnostic convention), not a
/// byte length, so the range is correct on lines containing multi-byte UTF-8.
pub(crate) fn calculate_line_range(line: usize, line_content: &str) -> (usize, usize, usize, usize) {
    let trimmed_char_len = line_content.trim_end().chars().count();
    (line, 1, line, trimmed_char_len + 1)
}

/// Calculate the diagnostic range of a match on a line.
///
/// `match_start_byte` and `match_len_bytes` are byte quantities within
/// `line_content`, as regex matches and `str::find` produce. The returned
/// columns are 1-indexed character columns. Passing a character index as the
/// start misplaces the diagnostic on any line with multi-byte text before the
/// match. An offset inside a character is clamped to a boundary rather than
/// panicking.
pub(crate) fn calculate_match_range(
    line: usize,
    line_content: &str,
    match_start_byte: usize,
    match_len_bytes: usize,
) -> (usize, usize, usize, usize) {
    let line_len = line_content.len();
    if match_start_byte > line_len {
        // If match_start_byte is beyond line bounds, return a safe range at end of line
        let char_count = line_content.chars().count();
        return (line, char_count + 1, line, char_count + 1);
    }

    // Find safe character boundaries for the match range
    let safe_match_start = find_char_boundary(line_content, match_start_byte);
    let safe_match_end_byte = find_char_boundary(line_content, (match_start_byte + match_len_bytes).min(line_len));

    // Convert byte positions to character positions safely
    let char_start = byte_to_char_count(line_content, safe_match_start);
    let char_len = if safe_match_end_byte > safe_match_start {
        // Count characters in the safe range
        line_content[safe_match_start..safe_match_end_byte].chars().count()
    } else {
        0
    };
    (line, char_start, line, char_start + char_len)
}

/// Calculate range for trailing content (like trailing spaces)
///
/// # Safety
/// This function safely handles multi-byte UTF-8 characters by ensuring all
/// string slicing operations occur at valid character boundaries.
pub(crate) fn calculate_trailing_range(
    line: usize,
    line_content: &str,
    content_end: usize,
) -> (usize, usize, usize, usize) {
    // Find safe character boundary for content_end
    let safe_content_end = find_char_boundary(line_content, content_end);
    let char_content_end = byte_to_char_count(line_content, safe_content_end);
    let line_char_len = line_content.chars().count() + 1;
    (line, char_content_end, line, line_char_len)
}

/// Calculate range for a heading, from the start of the line its text begins on
/// to the end of the text on the line it ends on.
///
/// An ATX heading holds its text on one line, so both are the same line. The
/// text of a setext heading is the whole paragraph its underline ends, which can
/// span several lines, and `last_line_content` is the last of them.
pub(crate) fn calculate_heading_range(
    first_line: usize,
    last_line: usize,
    last_line_content: &str,
) -> (usize, usize, usize, usize) {
    let trimmed_char_len = last_line_content.trim_end().chars().count();
    (first_line, 1, last_line, trimmed_char_len + 1)
}

/// Calculate range that exceeds a limit (like line length)
pub(crate) fn calculate_excess_range(line: usize, line_content: &str, limit: usize) -> (usize, usize, usize, usize) {
    let char_limit = std::cmp::min(limit, line_content.chars().count());
    let line_char_len = line_content.chars().count() + 1;
    (line, char_limit + 1, line, line_char_len)
}
