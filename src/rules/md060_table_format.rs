use crate::rule::{LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::range_utils::calculate_line_range;
use crate::utils::regex_cache::BLOCKQUOTE_PREFIX_RE;
use crate::utils::table_utils::TableUtils;
use unicode_width::UnicodeWidthStr;

mod md060_config;
use crate::rules::md013_line_length::MD013Config;
pub use md060_config::ColumnAlign;
pub use md060_config::MD060Config;

/// Identifies the type of row in a table for formatting purposes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum RowType {
    /// The first row containing column headers
    Header,
    /// The second row containing delimiter dashes (e.g., `|---|---|`)
    Delimiter,
    /// Data rows following the delimiter
    Body,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ColumnAlignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
struct TableFormatResult {
    lines: Vec<String>,
    auto_compacted: bool,
    aligned_width: Option<usize>,
}

/// Formatting options for a single table row.
#[derive(Debug, Clone, Copy)]
struct RowFormatOptions {
    /// The type of row being formatted
    row_type: RowType,
    /// Whether to use compact delimiter style (no spaces around dashes)
    compact_delimiter: bool,
    /// Global column alignment override
    column_align: ColumnAlign,
    /// Header-specific column alignment (overrides column_align for header)
    column_align_header: Option<ColumnAlign>,
    /// Body-specific column alignment (overrides column_align for body)
    column_align_body: Option<ColumnAlign>,
}

/// Rule MD060: Table Column Alignment
///
/// See [docs/md060.md](../../docs/md060.md) for full documentation, configuration, and examples.
///
/// This rule enforces consistent column alignment in Markdown tables for improved readability
/// in source form. When enabled, it ensures table columns are properly aligned with appropriate
/// padding.
///
/// ## Purpose
///
/// - **Readability**: Aligned tables are significantly easier to read in source form
/// - **Maintainability**: Properly formatted tables are easier to edit and review
/// - **Consistency**: Ensures uniform table formatting throughout documents
/// - **Developer Experience**: Makes working with tables in plain text more pleasant
///
/// ## Configuration Options
///
/// The rule supports the following configuration options:
///
/// ```toml
/// [MD013]
/// line-length = 100  # MD060 inherits this by default
///
/// [MD060]
/// enabled = false      # Default: opt-in for conservative adoption
/// style = "aligned"    # Can be "aligned", "compact", "tight", or "any"
/// max-width = 0        # Default: inherit from MD013's line-length
/// ```
///
/// ### Style Options
///
/// - **aligned**: Columns are padded with spaces for visual alignment (default)
/// - **compact**: Minimal spacing with single spaces
/// - **tight**: No spacing, pipes directly adjacent to content
/// - **any**: Preserve existing formatting style
///
/// ### Max Width (auto-compact threshold)
///
/// Controls when tables automatically switch from aligned to compact formatting:
///
/// - **`max-width = 0`** (default): Smart inheritance from MD013
/// - **`max-width = N`**: Explicit threshold, independent of MD013
///
/// When `max-width = 0`:
/// - If MD013 is disabled → unlimited (no auto-compact)
/// - If MD013.tables = false → unlimited (no auto-compact)
/// - If MD013.line_length = 0 → unlimited (no auto-compact)
/// - Otherwise → inherits MD013's line-length
///
/// This matches the behavior of Prettier's table formatting.
///
/// #### Examples
///
/// ```toml
/// # Inherit from MD013 (recommended)
/// [MD013]
/// line-length = 100
///
/// [MD060]
/// style = "aligned"
/// max-width = 0  # Tables exceeding 100 chars will be compacted
/// ```
///
/// ```toml
/// # Explicit threshold
/// [MD060]
/// style = "aligned"
/// max-width = 120  # Independent of MD013
/// ```
///
/// ## Examples
///
/// ### Aligned Style (Good)
///
/// ```markdown
/// | Name  | Age | City      |
/// |-------|-----|-----------|
/// | Alice | 30  | Seattle   |
/// | Bob   | 25  | Portland  |
/// ```
///
/// ### Unaligned (Bad)
///
/// ```markdown
/// | Name | Age | City |
/// |---|---|---|
/// | Alice | 30 | Seattle |
/// | Bob | 25 | Portland |
/// ```
///
/// ## Unicode Support
///
/// This rule properly handles:
/// - **CJK Characters**: Chinese, Japanese, Korean characters are correctly measured as double-width
/// - **Basic Emoji**: Most emoji are handled correctly
/// - **Inline Code**: Pipes in inline code blocks are properly masked
///
/// ## Known Limitations
///
/// **Complex Unicode Sequences**: Tables containing certain Unicode characters are automatically
/// skipped to prevent alignment corruption. These include:
/// - Zero-Width Joiner (ZWJ) emoji: 👨‍👩‍👧‍👦, 👩‍💻
/// - Zero-Width Space (ZWS): Invisible word break opportunities
/// - Zero-Width Non-Joiner (ZWNJ): Ligature prevention marks
/// - Word Joiner (WJ): Non-breaking invisible characters
///
/// These characters have inconsistent or zero display widths across terminals and fonts,
/// making accurate alignment impossible. The rule preserves these tables as-is rather than
/// risk corrupting them.
///
/// This is an honest limitation of terminal display technology, similar to what other tools
/// like markdownlint experience.
///
/// ## Fix Behavior
///
/// When applying automatic fixes, this rule:
/// - Calculates proper display width for each column using Unicode width measurements
/// - Pads cells with trailing spaces to align columns
/// - Preserves cell content exactly (only spacing is modified)
/// - Respects alignment indicators in delimiter rows (`:---`, `:---:`, `---:`)
/// - Automatically switches to compact mode for tables exceeding max_width
/// - Skips tables with ZWJ emoji to prevent corruption
#[derive(Debug, Clone, Default)]
pub struct MD060TableFormat {
    config: MD060Config,
    md013_config: MD013Config,
    md013_disabled: bool,
}

impl MD060TableFormat {
    pub(crate) fn new(enabled: bool, style: String) -> Self {
        use crate::types::LineLength;
        Self {
            config: MD060Config {
                enabled,
                style,
                max_width: LineLength::from_const(0),
                column_align: ColumnAlign::Auto,
                column_align_header: None,
                column_align_body: None,
                loose_last_column: false,
                aligned_delimiter: false,
            },
            md013_config: MD013Config::default(),
            md013_disabled: false,
        }
    }

    pub fn from_config_struct(config: MD060Config, md013_config: MD013Config, md013_disabled: bool) -> Self {
        Self {
            config,
            md013_config,
            md013_disabled,
        }
    }

    /// Get the effective max width for table formatting.
    ///
    /// Priority order:
    /// 1. Explicit `max_width > 0` always takes precedence
    /// 2. When `max_width = 0` (inherit mode), check MD013 configuration:
    ///    - If MD013 is globally disabled → unlimited
    ///    - If `MD013.tables = false` → unlimited
    ///    - If `MD013.line_length = 0` → unlimited
    ///    - Otherwise → inherit MD013's line_length
    fn effective_max_width(&self) -> usize {
        // Explicit max_width always takes precedence
        if !self.config.max_width.is_unlimited() {
            return self.config.max_width.get();
        }

        // max_width = 0 means "inherit" - but inherit UNLIMITED if:
        // 1. MD013 is globally disabled
        // 2. MD013.tables = false (user doesn't care about table line length)
        // 3. MD013.line_length = 0 (no line length limit at all)
        if self.md013_disabled || !self.md013_config.tables || self.md013_config.line_length.is_unlimited() {
            return usize::MAX; // Unlimited
        }

        // Otherwise inherit MD013's line-length
        self.md013_config.line_length.get()
    }

    /// Check if text contains characters that break Unicode width calculations
    ///
    /// Tables with these characters are skipped to avoid alignment corruption:
    /// - Zero-Width Joiner (ZWJ, U+200D): Complex emoji like 👨‍👩‍👧‍👦
    /// - Zero-Width Space (ZWS, U+200B): Invisible word break opportunity
    /// - Zero-Width Non-Joiner (ZWNJ, U+200C): Prevents ligature formation
    /// - Word Joiner (WJ, U+2060): Prevents line breaks without taking space
    ///
    /// These characters have inconsistent display widths across terminals,
    /// making accurate alignment impossible.
    fn contains_problematic_chars(text: &str) -> bool {
        text.contains('\u{200D}')  // ZWJ
            || text.contains('\u{200B}')  // ZWS
            || text.contains('\u{200C}')  // ZWNJ
            || text.contains('\u{2060}') // Word Joiner
    }

    fn calculate_cell_display_width(cell_content: &str) -> usize {
        let masked = TableUtils::mask_pipes_in_inline_code(cell_content);
        masked.trim().width()
    }

    /// Parse a table row into cells using Standard flavor (default behavior).
    /// Used for tests and backward compatibility.

    /// Parse a table row into cells, respecting flavor-specific behavior.
    ///
    /// Pipes inside code spans are treated as content, not cell delimiters.
    fn parse_table_row_with_flavor(line: &str, flavor: crate::config::MarkdownFlavor) -> Vec<String> {
        TableUtils::split_table_row_with_flavor(line, flavor)
    }

    fn is_delimiter_row(row: &[String]) -> bool {
        if row.is_empty() {
            return false;
        }
        row.iter().all(|cell| {
            let trimmed = cell.trim();
            // A delimiter cell must contain at least one dash
            // Empty cells are not delimiter cells
            !trimmed.is_empty()
                && trimmed.contains('-')
                && trimmed.chars().all(|c| c == '-' || c == ':' || c.is_whitespace())
        })
    }

    /// Extract blockquote prefix from a line (e.g., "> " or ">> ").
    /// Returns (prefix, content_without_prefix).
    fn extract_blockquote_prefix(line: &str) -> (&str, &str) {
        if let Some(m) = BLOCKQUOTE_PREFIX_RE.find(line) {
            (&line[..m.end()], &line[m.end()..])
        } else {
            ("", line)
        }
    }

    /// The indentation every row of an MDG Data Table or Examples table is
    /// rewritten to, or `None` when the table is not one Gherkin recognizes.
    ///
    /// Two spaces is always inside Gherkin's range, so rows written anywhere in
    /// it are normalized to two rather than left as written. Inside a list item
    /// the rows must also reach the item's content column to stay part of that
    /// item, and past Gherkin's limit no indentation satisfies both, so the
    /// table is left to the ordinary Markdown handling.
    fn mdg_table_indent(
        _table_lines: &[&str],
        _list_content_indent: Option<usize>,
        _flavor: crate::config::MarkdownFlavor,
    ) -> Option<String> {
        {
            return None;
        }
    }

    fn parse_column_alignments(delimiter_row: &[String]) -> Vec<ColumnAlignment> {
        delimiter_row
            .iter()
            .map(|cell| {
                let trimmed = cell.trim();
                let has_left_colon = trimmed.starts_with(':');
                let has_right_colon = trimmed.ends_with(':');

                match (has_left_colon, has_right_colon) {
                    (true, true) => ColumnAlignment::Center,
                    (false, true) => ColumnAlignment::Right,
                    _ => ColumnAlignment::Left,
                }
            })
            .collect()
    }

    fn calculate_column_widths(
        table_lines: &[&str],
        flavor: crate::config::MarkdownFlavor,
        loose_last_column: bool,
    ) -> Vec<usize> {
        let mut column_widths = Vec::new();
        let mut delimiter_cells: Option<Vec<String>> = None;
        let mut is_header = true;
        let mut header_last_col_width: Option<usize> = None;

        for line in table_lines {
            let cells = Self::parse_table_row_with_flavor(line, flavor);

            // Save delimiter row for later processing, but don't use it for width calculation
            if Self::is_delimiter_row(&cells) {
                delimiter_cells = Some(cells);
                is_header = false;
                continue;
            }

            for (i, cell) in cells.iter().enumerate() {
                let width = Self::calculate_cell_display_width(cell);
                if i >= column_widths.len() {
                    column_widths.push(width);
                } else {
                    column_widths[i] = column_widths[i].max(width);
                }
            }

            // Record the header row's last column width
            if is_header && !cells.is_empty() {
                let last_idx = cells.len() - 1;
                header_last_col_width = Some(Self::calculate_cell_display_width(&cells[last_idx]));
                is_header = false;
            }
        }

        // When loose, cap the last column width at the header's width
        if loose_last_column
            && let Some(header_width) = header_last_col_width
            && let Some(last) = column_widths.last_mut()
        {
            *last = header_width;
        }

        // GFM requires delimiter rows to have at least 3 dashes per column.
        // To ensure visual alignment, all columns must be at least width 3.
        let mut final_widths: Vec<usize> = column_widths.iter().map(|&w| w.max(3)).collect();

        // Adjust column widths to accommodate alignment indicators (colons) in delimiter row
        // This ensures the delimiter row has the same length as content rows
        if let Some(delimiter_cells) = delimiter_cells {
            for (i, cell) in delimiter_cells.iter().enumerate() {
                if i < final_widths.len() {
                    let trimmed = cell.trim();
                    let has_left_colon = trimmed.starts_with(':');
                    let has_right_colon = trimmed.ends_with(':');
                    let colon_count = (has_left_colon as usize) + (has_right_colon as usize);

                    // Minimum width needed: 3 dashes + colons
                    let min_width_for_delimiter = 3 + colon_count;
                    final_widths[i] = final_widths[i].max(min_width_for_delimiter);
                }
            }
        }

        final_widths
    }

    fn format_table_row(
        cells: &[String],
        column_widths: &[usize],
        column_alignments: &[ColumnAlignment],
        options: &RowFormatOptions,
    ) -> String {
        let formatted_cells: Vec<String> = cells
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let target_width = column_widths.get(i).copied().unwrap_or(0);

                match options.row_type {
                    RowType::Delimiter => {
                        let trimmed = cell.trim();
                        let has_left_colon = trimmed.starts_with(':');
                        let has_right_colon = trimmed.ends_with(':');

                        // Delimiter rows use the same cell format as content rows: | content |
                        // The "content" is dashes, possibly with colons for alignment
                        // For compact_delimiter mode, we don't add spaces, so we need 2 extra dashes
                        let extra_width = if options.compact_delimiter { 2 } else { 0 };
                        let dash_count = if has_left_colon && has_right_colon {
                            (target_width + extra_width).saturating_sub(2)
                        } else if has_left_colon || has_right_colon {
                            (target_width + extra_width).saturating_sub(1)
                        } else {
                            target_width + extra_width
                        };

                        let dashes = "-".repeat(dash_count.max(3)); // Minimum 3 dashes
                        let delimiter_content = if has_left_colon && has_right_colon {
                            format!(":{dashes}:")
                        } else if has_left_colon {
                            format!(":{dashes}")
                        } else if has_right_colon {
                            format!("{dashes}:")
                        } else {
                            dashes
                        };

                        // Add spaces around delimiter content unless compact_delimiter mode
                        if options.compact_delimiter {
                            delimiter_content
                        } else {
                            format!(" {delimiter_content} ")
                        }
                    }
                    RowType::Header | RowType::Body => {
                        let trimmed = cell.trim();
                        let current_width = Self::calculate_cell_display_width(cell);
                        let padding = target_width.saturating_sub(current_width);

                        // Determine which alignment to use based on row type
                        let effective_align = match options.row_type {
                            RowType::Header => options.column_align_header.unwrap_or(options.column_align),
                            RowType::Body => options.column_align_body.unwrap_or(options.column_align),
                            RowType::Delimiter => unreachable!(),
                        };

                        // Apply alignment: use override if specified, otherwise use delimiter indicators
                        let alignment = match effective_align {
                            ColumnAlign::Auto => column_alignments.get(i).copied().unwrap_or(ColumnAlignment::Left),
                            ColumnAlign::Left => ColumnAlignment::Left,
                            ColumnAlign::Center => ColumnAlignment::Center,
                            ColumnAlign::Right => ColumnAlignment::Right,
                        };

                        match alignment {
                            ColumnAlignment::Left => {
                                // Left: content on left, padding on right
                                format!(" {trimmed}{} ", " ".repeat(padding))
                            }
                            ColumnAlignment::Center => {
                                // Center: split padding on both sides
                                let left_padding = padding / 2;
                                let right_padding = padding - left_padding;
                                format!(" {}{trimmed}{} ", " ".repeat(left_padding), " ".repeat(right_padding))
                            }
                            ColumnAlignment::Right => {
                                // Right: padding on left, content on right
                                format!(" {}{trimmed} ", " ".repeat(padding))
                            }
                        }
                    }
                }
            })
            .collect();

        format!("|{}|", formatted_cells.join("|"))
    }

    fn format_table_compact(cells: &[String]) -> String {
        // An empty compact cell is a single space between pipes (`| |`),
        // matching mdformat's canonical form. This keeps rumdl's output stable
        // when both tools format the same file.
        let formatted_cells: Vec<String> = cells
            .iter()
            .map(|cell| match cell.trim() {
                "" => " ".to_string(),
                trimmed => format!(" {trimmed} "),
            })
            .collect();
        format!("|{}|", formatted_cells.join("|"))
    }

    fn format_table_tight(cells: &[String]) -> String {
        let formatted_cells: Vec<String> = cells.iter().map(|cell| cell.trim().to_string()).collect();
        format!("|{}|", formatted_cells.join("|"))
    }

    /// Format a delimiter row whose pipe positions align with the header pipe
    /// positions, used by `compact` / `tight` styles when `aligned_delimiter`
    /// is enabled. Body rows remain unchanged (compact or tight).
    ///
    /// `header_widths` is the display width of each header cell's trimmed
    /// content. Each delimiter cell receives that many dashes (minus one for
    /// each colon present), preserving `:---`, `---:`, `:---:` markers.
    /// `compact` controls whether to surround the dashes with single spaces.
    fn format_delimiter_aligned_to_header(delim_cells: &[String], header_widths: &[usize], compact: bool) -> String {
        let formatted_cells: Vec<String> = delim_cells
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let target_width = header_widths.get(i).copied().unwrap_or(0);
                let trimmed = cell.trim();
                let has_left_colon = trimmed.starts_with(':');
                let has_right_colon = trimmed.ends_with(':');
                let colon_count = usize::from(has_left_colon) + usize::from(has_right_colon);

                // GFM minimum: at least one dash per delimiter cell.
                let dash_count = target_width.saturating_sub(colon_count).max(1);
                let dashes = "-".repeat(dash_count);
                let delimiter_content = match (has_left_colon, has_right_colon) {
                    (true, true) => format!(":{dashes}:"),
                    (true, false) => format!(":{dashes}"),
                    (false, true) => format!("{dashes}:"),
                    (false, false) => dashes,
                };
                if compact {
                    format!(" {delimiter_content} ")
                } else {
                    delimiter_content
                }
            })
            .collect();

        format!("|{}|", formatted_cells.join("|"))
    }

    /// Returns display widths of each header cell's trimmed content.
    /// Used when `aligned_delimiter` is on to size the delimiter row.
    fn header_cell_widths(header_cells: &[String]) -> Vec<usize> {
        header_cells
            .iter()
            .map(|c| Self::calculate_cell_display_width(c))
            .collect()
    }

    /// Checks if a table is already aligned with consistent column widths
    /// and the delimiter row style matches the target style.
    ///
    /// A table is considered "already aligned" if:
    /// 1. All rows have the same display length
    /// 2. Each column has consistent cell width across all rows
    /// 3. The delimiter row has valid minimum widths (at least 3 chars per cell)
    /// 4. The delimiter row style matches the target style (compact_delimiter parameter)
    ///
    /// The `compact_delimiter` parameter indicates whether the target style is "aligned-no-space"
    /// (true = no spaces around dashes, false = spaces around dashes).
    fn is_table_already_aligned(
        table_lines: &[&str],
        flavor: crate::config::MarkdownFlavor,
        compact_delimiter: bool,
    ) -> bool {
        if table_lines.len() < 2 {
            return false;
        }

        // Check 1: All rows must have the same display width
        // Use .width() instead of .len() to handle CJK characters correctly
        // (CJK chars are 3 bytes but 2 display columns)
        let first_width = UnicodeWidthStr::width(table_lines[0]);
        if !table_lines
            .iter()
            .all(|line| UnicodeWidthStr::width(*line) == first_width)
        {
            return false;
        }

        // Parse all rows and check column count consistency
        let parsed: Vec<Vec<String>> = table_lines
            .iter()
            .map(|line| Self::parse_table_row_with_flavor(line, flavor))
            .collect();

        if parsed.is_empty() {
            return false;
        }

        let num_columns = parsed[0].len();
        if !parsed.iter().all(|row| row.len() == num_columns) {
            return false;
        }

        // Check delimiter row has valid minimum widths (3 chars: at least one dash + optional colons)
        // Delimiter row is always at index 1
        if let Some(delimiter_row) = parsed.get(1) {
            if !Self::is_delimiter_row(delimiter_row) {
                return false;
            }
            // Check each delimiter cell has at least one dash (minimum valid is "---" or ":--" etc)
            for cell in delimiter_row {
                let trimmed = cell.trim();
                let dash_count = trimmed.chars().filter(|&c| c == '-').count();
                if dash_count < 1 {
                    return false;
                }
            }

            // Check if delimiter row style matches the target style
            // compact_delimiter=true means "aligned-no-space" (no spaces around dashes)
            // compact_delimiter=false means "aligned" (spaces around dashes)
            let delimiter_has_spaces = delimiter_row
                .iter()
                .all(|cell| cell.starts_with(' ') && cell.ends_with(' '));

            // If target is compact (no spaces) but current has spaces, not aligned
            // If target is spaced but current has no spaces, not aligned
            if compact_delimiter && delimiter_has_spaces {
                return false;
            }
            if !compact_delimiter && !delimiter_has_spaces {
                return false;
            }
        }

        // Check each column has consistent width across all content rows
        // Use cell.width() to get display width INCLUDING padding, not trimmed content
        // This correctly handles CJK characters (display width 2, byte length 3)
        for col_idx in 0..num_columns {
            let mut widths = Vec::new();
            for (row_idx, row) in parsed.iter().enumerate() {
                // Skip delimiter row for content width check
                if row_idx == 1 {
                    continue;
                }
                if let Some(cell) = row.get(col_idx) {
                    widths.push(cell.width());
                }
            }
            // All content cells in this column should have the same display width
            if !widths.is_empty() && !widths.iter().all(|&w| w == widths[0]) {
                return false;
            }
        }

        // Check 5: Content padding distribution matches column alignment
        // For center-aligned columns, content must be centered (left/right padding differ by at most 1)
        // For right-aligned columns, content must be right-aligned (left padding >= right padding)
        // Padding is counted in space characters (always 1 byte each), so byte-length arithmetic is safe.
        if let Some(delimiter_row) = parsed.get(1) {
            let alignments = Self::parse_column_alignments(delimiter_row);
            for (col_idx, alignment) in alignments.iter().enumerate() {
                if *alignment == ColumnAlignment::Left {
                    continue;
                }
                for (row_idx, row) in parsed.iter().enumerate() {
                    // Skip delimiter row
                    if row_idx == 1 {
                        continue;
                    }
                    if let Some(cell) = row.get(col_idx) {
                        if cell.trim().is_empty() {
                            continue;
                        }
                        // Count leading/trailing space characters (always ASCII, so byte length = char count)
                        let left_pad = cell.len() - cell.trim_start().len();
                        let right_pad = cell.len() - cell.trim_end().len();

                        match alignment {
                            ColumnAlignment::Center => {
                                // Center: left and right padding must differ by at most 1
                                if left_pad.abs_diff(right_pad) > 1 {
                                    return false;
                                }
                            }
                            ColumnAlignment::Right => {
                                // Right: content pushed right means more padding on the left
                                if left_pad < right_pad {
                                    return false;
                                }
                            }
                            ColumnAlignment::Left => unreachable!(),
                        }
                    }
                }
            }
        }

        true
    }

    fn detect_table_style(table_lines: &[&str], flavor: crate::config::MarkdownFlavor) -> Option<String> {
        if table_lines.is_empty() {
            return None;
        }

        // Check all rows (except delimiter) to determine consistent style
        // A table is only "tight" or "compact" if ALL rows follow that pattern
        let mut is_tight = true;
        let mut is_compact = true;

        for line in table_lines {
            let cells = Self::parse_table_row_with_flavor(line, flavor);

            if cells.is_empty() {
                continue;
            }

            // Skip delimiter rows when detecting style
            if Self::is_delimiter_row(&cells) {
                continue;
            }

            // Check if this row has no padding
            let row_has_no_padding = cells.iter().all(|cell| !cell.starts_with(' ') && !cell.ends_with(' '));

            // Compact rows pad every cell with one space on each side. An
            // empty compact cell is the special case `" "` (single space
            // between pipes), matching mdformat's canonical empty cell.
            let row_has_single_space = cells.iter().all(|cell| match cell.trim() {
                "" => cell == " ",
                trimmed => cell == &format!(" {trimmed} "),
            });

            // If any row doesn't match tight, the table isn't tight
            if !row_has_no_padding {
                is_tight = false;
            }

            // If any row doesn't match compact, the table isn't compact
            if !row_has_single_space {
                is_compact = false;
            }

            // Early exit: if neither tight nor compact, it must be aligned
            if !is_tight && !is_compact {
                return Some("aligned".to_string());
            }
        }

        // Return the most restrictive style that matches
        if is_tight {
            Some("tight".to_string())
        } else if is_compact {
            Some("compact".to_string())
        } else {
            Some("aligned".to_string())
        }
    }

    fn fix_table_block(
        &self,
        lines: &[&str],
        table_block: &crate::utils::table_utils::TableBlock,
        flavor: crate::config::MarkdownFlavor,
    ) -> TableFormatResult {
        let mut result = Vec::new();
        let mut auto_compacted = false;
        let mut aligned_width = None;

        let table_lines: Vec<&str> = std::iter::once(lines[table_block.header_line])
            .chain(std::iter::once(lines[table_block.delimiter_line]))
            .chain(table_block.content_lines.iter().map(|&idx| lines[idx]))
            .collect();

        if table_lines.iter().any(|line| Self::contains_problematic_chars(line)) {
            return TableFormatResult {
                lines: table_lines.iter().map(std::string::ToString::to_string).collect(),
                auto_compacted: false,
                aligned_width: None,
            };
        }

        // Extract blockquote prefix from the header line (first line of table)
        // All lines in the same table should have the same blockquote level
        let (blockquote_prefix, _) = Self::extract_blockquote_prefix(table_lines[0]);

        // Extract list prefix if present (for tables inside list items)
        let list_context = &table_block.list_context;
        let (list_prefix, continuation_indent) = if let Some(ctx) = list_context {
            (ctx.list_prefix.as_str(), " ".repeat(ctx.content_indent))
        } else {
            ("", String::new())
        };
        let mdg_indent = Self::mdg_table_indent(
            &table_lines,
            list_context.as_ref().map(|ctx| ctx.content_indent),
            flavor,
        );

        // Strip blockquote prefix and list prefix from all lines for processing
        let stripped_lines: Vec<&str> = table_lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let after_blockquote = Self::extract_blockquote_prefix(line).1;
                if mdg_indent.is_some() {
                    after_blockquote.trim_start_matches([' ', '\t'])
                } else if list_context.is_some() {
                    if i == 0 {
                        // Header line: strip list prefix (handles both markers and indentation)
                        after_blockquote.strip_prefix(list_prefix).unwrap_or_else(|| {
                            crate::utils::table_utils::TableUtils::extract_list_prefix(after_blockquote).1
                        })
                    } else {
                        // Continuation lines: strip expected indentation
                        after_blockquote
                            .strip_prefix(&continuation_indent)
                            .unwrap_or(after_blockquote.trim_start())
                    }
                } else {
                    after_blockquote
                }
            })
            .collect();

        let style = self.config.style.as_str();

        match style {
            "any" => {
                let detected_style = Self::detect_table_style(&stripped_lines, flavor);
                if detected_style.is_none() {
                    return TableFormatResult {
                        lines: table_lines.iter().map(std::string::ToString::to_string).collect(),
                        auto_compacted: false,
                        aligned_width: None,
                    };
                }

                let target_style = detected_style.unwrap();

                // Parse column alignments from delimiter row (always at index 1)
                let delimiter_cells = Self::parse_table_row_with_flavor(stripped_lines[1], flavor);
                let column_alignments = Self::parse_column_alignments(&delimiter_cells);

                for (row_idx, line) in stripped_lines.iter().enumerate() {
                    let cells = Self::parse_table_row_with_flavor(line, flavor);
                    match target_style.as_str() {
                        "tight" => result.push(Self::format_table_tight(&cells)),
                        "compact" => result.push(Self::format_table_compact(&cells)),
                        _ => {
                            let column_widths =
                                Self::calculate_column_widths(&stripped_lines, flavor, self.config.loose_last_column);
                            let row_type = match row_idx {
                                0 => RowType::Header,
                                1 => RowType::Delimiter,
                                _ => RowType::Body,
                            };
                            let options = RowFormatOptions {
                                row_type,
                                compact_delimiter: false,
                                column_align: self.config.column_align,
                                column_align_header: self.config.column_align_header,
                                column_align_body: self.config.column_align_body,
                            };
                            result.push(Self::format_table_row(
                                &cells,
                                &column_widths,
                                &column_alignments,
                                &options,
                            ));
                        }
                    }
                }
            }
            "compact" | "tight" => {
                let compact = style == "compact";
                let header_widths = if self.config.aligned_delimiter && stripped_lines.len() >= 2 {
                    let header_cells = Self::parse_table_row_with_flavor(stripped_lines[0], flavor);
                    Some(Self::header_cell_widths(&header_cells))
                } else {
                    None
                };

                for (row_idx, line) in stripped_lines.iter().enumerate() {
                    let cells = Self::parse_table_row_with_flavor(line, flavor);
                    if row_idx == 1
                        && let Some(widths) = &header_widths
                    {
                        result.push(Self::format_delimiter_aligned_to_header(&cells, widths, compact));
                        continue;
                    }
                    result.push(if compact {
                        Self::format_table_compact(&cells)
                    } else {
                        Self::format_table_tight(&cells)
                    });
                }
            }
            "aligned" | "aligned-no-space" => {
                let compact_delimiter = style == "aligned-no-space";

                // Determine if we need to reformat: skip if table is already aligned
                // UNLESS any alignment or formatting options require reformatting
                let needs_reformat = self.config.column_align != ColumnAlign::Auto
                    || self.config.column_align_header.is_some()
                    || self.config.column_align_body.is_some()
                    || self.config.loose_last_column;

                if !needs_reformat && Self::is_table_already_aligned(&stripped_lines, flavor, compact_delimiter) {
                    return TableFormatResult {
                        lines: table_lines.iter().map(std::string::ToString::to_string).collect(),
                        auto_compacted: false,
                        aligned_width: None,
                    };
                }

                let column_widths =
                    Self::calculate_column_widths(&stripped_lines, flavor, self.config.loose_last_column);

                // Calculate aligned table width: 1 (leading pipe) + num_columns * 3 (| cell |) + sum(column_widths)
                let num_columns = column_widths.len();
                let calc_aligned_width = 1 + (num_columns * 3) + column_widths.iter().sum::<usize>();
                aligned_width = Some(calc_aligned_width);

                // Auto-compact: if aligned table exceeds max width, use compact formatting instead.
                // The effective output style is now `compact`, so honor `aligned-delimiter`
                // exactly as the explicit `compact` style does: align the delimiter row's pipes
                // to the header column widths while body rows stay compact.
                if calc_aligned_width > self.effective_max_width() {
                    auto_compacted = true;
                    let header_widths = if self.config.aligned_delimiter && stripped_lines.len() >= 2 {
                        let header_cells = Self::parse_table_row_with_flavor(stripped_lines[0], flavor);
                        Some(Self::header_cell_widths(&header_cells))
                    } else {
                        None
                    };
                    for (row_idx, line) in stripped_lines.iter().enumerate() {
                        let cells = Self::parse_table_row_with_flavor(line, flavor);
                        if row_idx == 1
                            && let Some(widths) = &header_widths
                        {
                            // Auto-compact always produces the single-space compact form.
                            result.push(Self::format_delimiter_aligned_to_header(&cells, widths, true));
                            continue;
                        }
                        result.push(Self::format_table_compact(&cells));
                    }
                } else {
                    // Parse column alignments from delimiter row (always at index 1)
                    let delimiter_cells = Self::parse_table_row_with_flavor(stripped_lines[1], flavor);
                    let column_alignments = Self::parse_column_alignments(&delimiter_cells);

                    for (row_idx, line) in stripped_lines.iter().enumerate() {
                        let cells = Self::parse_table_row_with_flavor(line, flavor);
                        let row_type = match row_idx {
                            0 => RowType::Header,
                            1 => RowType::Delimiter,
                            _ => RowType::Body,
                        };
                        let options = RowFormatOptions {
                            row_type,
                            compact_delimiter,
                            column_align: self.config.column_align,
                            column_align_header: self.config.column_align_header,
                            column_align_body: self.config.column_align_body,
                        };
                        result.push(Self::format_table_row(
                            &cells,
                            &column_widths,
                            &column_alignments,
                            &options,
                        ));
                    }
                }
            }
            _ => {
                return TableFormatResult {
                    lines: table_lines.iter().map(std::string::ToString::to_string).collect(),
                    auto_compacted: false,
                    aligned_width: None,
                };
            }
        }

        // Re-add blockquote prefix and list prefix to all formatted lines
        let prefixed_result: Vec<String> = result
            .into_iter()
            .enumerate()
            .map(|(i, line)| {
                if let Some(mdg_indent) = &mdg_indent {
                    format!("{blockquote_prefix}{mdg_indent}{line}")
                } else if list_context.is_some() {
                    if i == 0 {
                        // Header line: add list prefix
                        format!("{blockquote_prefix}{list_prefix}{line}")
                    } else {
                        // Continuation lines: add indentation
                        format!("{blockquote_prefix}{continuation_indent}{line}")
                    }
                } else {
                    format!("{blockquote_prefix}{line}")
                }
            })
            .collect();

        TableFormatResult {
            lines: prefixed_result,
            auto_compacted,
            aligned_width,
        }
    }
}

impl Rule for MD060TableFormat {
    fn name(&self) -> &'static str {
        "MD060"
    }

    fn description(&self) -> &'static str {
        "Table columns should be consistently aligned"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Table
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        !ctx.likely_has_tables()
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = Vec::new();

        let lines = ctx.raw_lines();
        let table_blocks = &ctx.table_blocks;

        for table_block in table_blocks {
            let format_result = self.fix_table_block(lines, table_block, ctx.flavor);

            let table_line_indices: Vec<usize> = std::iter::once(table_block.header_line)
                .chain(std::iter::once(table_block.delimiter_line))
                .chain(table_block.content_lines.iter().copied())
                .collect();

            // Build the whole-table fix once for all warnings in this table
            // This ensures that applying Quick Fix on any row fixes the entire table
            let table_start_line = table_block.start_line + 1; // Convert to 1-indexed
            let table_end_line = table_block.end_line + 1; // Convert to 1-indexed

            // Build the complete fixed table content
            let mut fixed_table_lines: Vec<String> = Vec::with_capacity(table_line_indices.len());
            for (i, &line_idx) in table_line_indices.iter().enumerate() {
                let fixed_line = &format_result.lines[i];
                // Add newline for all lines except the last if the original didn't have one
                if line_idx < lines.len() - 1 {
                    fixed_table_lines.push(format!("{fixed_line}\n"));
                } else {
                    fixed_table_lines.push(fixed_line.clone());
                }
            }
            let table_replacement = fixed_table_lines.concat();
            let table_range = ctx.line_span_byte_range(table_start_line, table_end_line);

            for (i, &line_idx) in table_line_indices.iter().enumerate() {
                let original = lines[line_idx];
                let fixed = &format_result.lines[i];

                if original != fixed {
                    let (start_line, start_col, end_line, end_col) = calculate_line_range(line_idx + 1, original);

                    let message = if format_result.auto_compacted {
                        if let Some(width) = format_result.aligned_width {
                            format!(
                                "Table too wide for aligned formatting ({} chars > max-width: {})",
                                width,
                                self.effective_max_width()
                            )
                        } else {
                            "Table too wide for aligned formatting".to_string()
                        }
                    } else {
                        "Table columns should be aligned".to_string()
                    };

                    // Each warning uses the same whole-table fix
                    // This ensures Quick Fix on any row aligns the entire table
                    warnings.push(LintWarning {
                        rule_name: Some(self.name().to_string()),
                        severity: Severity::Warning,
                        message,
                        line: start_line,
                        column: start_col,
                        end_line,
                        end_column: end_col,
                        fix: Some(crate::rule::Fix::new(table_range.clone(), table_replacement.clone())),
                    });
                }
            }
        }

        Ok(warnings)
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        let content = ctx.content;
        let lines = ctx.raw_lines();
        let table_blocks = &ctx.table_blocks;

        // Nothing to format when there are no tables; return the content verbatim
        // so non-table documents are never altered.
        if table_blocks.is_empty() {
            return Ok(content.to_string());
        }

        let mut result_lines: Vec<String> = lines.iter().map(|&s| s.to_string()).collect();

        for table_block in table_blocks {
            let format_result = self.fix_table_block(lines, table_block, ctx.flavor);

            let table_line_indices: Vec<usize> = std::iter::once(table_block.header_line)
                .chain(std::iter::once(table_block.delimiter_line))
                .chain(table_block.content_lines.iter().copied())
                .collect();

            for (i, &line_idx) in table_line_indices.iter().enumerate() {
                result_lines[line_idx].clone_from(&format_result.lines[i]);
            }
        }

        let mut fixed = result_lines.join("\n");
        // `raw_lines()` drops the trailing empty line, so `join("\n")` collapses a
        // run of trailing blank lines down to a single newline. Restore the
        // original trailing-newline run exactly so trailing blank lines are
        // preserved and the fix is idempotent.
        let original_trailing_newlines = content.len() - content.trim_end_matches('\n').len();
        fixed.truncate(fixed.trim_end_matches('\n').len());
        fixed.push_str(&"\n".repeat(original_trailing_newlines));
        Ok(fixed)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
