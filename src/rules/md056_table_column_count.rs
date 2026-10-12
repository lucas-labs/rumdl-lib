use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::range_utils::calculate_line_range;
use crate::utils::table_utils::TableUtils;

/// Rule MD056: Table column count
///
/// See [docs/md056.md](../../docs/md056.md) for full documentation, configuration, and examples.
/// Ensures all rows in a table have the same number of cells
#[derive(Debug, Clone)]
pub struct MD056TableColumnCount;

impl Default for MD056TableColumnCount {
    fn default() -> Self {
        MD056TableColumnCount
    }
}

impl MD056TableColumnCount {
    /// Try to fix a table row content (with list context awareness)
    fn fix_table_row_content(
        &self,
        row_content: &str,
        expected_count: usize,
        flavor: crate::config::MarkdownFlavor,
        table_block: &crate::utils::table_utils::TableBlock,
        line_index: usize,
        original_line: &str,
    ) -> Option<String> {
        let current_count = TableUtils::count_cells_with_flavor(row_content, flavor);

        if current_count == expected_count || current_count == 0 {
            return None;
        }

        let fixed = self.fix_row_by_truncation(row_content, expected_count, flavor)?;
        Some(self.restore_prefixes(&fixed, table_block, line_index, original_line))
    }

    /// Restore list/blockquote prefixes to a fixed row
    fn restore_prefixes(
        &self,
        fixed_content: &str,
        table_block: &crate::utils::table_utils::TableBlock,
        line_index: usize,
        original_line: &str,
    ) -> String {
        // Extract blockquote prefix from original
        let (blockquote_prefix, _) = TableUtils::extract_blockquote_prefix(original_line);

        // Handle list context
        if let Some(ref list_ctx) = table_block.list_context {
            if line_index == 0 {
                // Header line: use list prefix
                format!("{blockquote_prefix}{}{fixed_content}", list_ctx.list_prefix)
            } else {
                // Continuation lines: use indentation
                let indent = " ".repeat(list_ctx.content_indent);
                format!("{blockquote_prefix}{indent}{fixed_content}")
            }
        } else {
            // No list context, just blockquote
            if blockquote_prefix.is_empty() {
                fixed_content.to_string()
            } else {
                format!("{blockquote_prefix}{fixed_content}")
            }
        }
    }

    /// Fix a table row by truncating or adding cells
    fn fix_row_by_truncation(
        &self,
        row: &str,
        expected_count: usize,
        flavor: crate::config::MarkdownFlavor,
    ) -> Option<String> {
        let current_count = TableUtils::count_cells_with_flavor(row, flavor);

        if current_count == expected_count || current_count == 0 {
            return None;
        }

        let trimmed = row.trim();
        let has_leading_pipe = trimmed.starts_with('|');
        let has_trailing_pipe = trimmed.ends_with('|');

        // Delegate to shared cell splitting (returns only cell contents, no empty leading/trailing parts)
        let cells = TableUtils::split_table_row_with_flavor(trimmed, flavor);
        let mut cell_contents: Vec<&str> = cells.iter().map(|c| c.trim()).collect();

        // Adjust cell count to match expected count
        match current_count.cmp(&expected_count) {
            std::cmp::Ordering::Greater => {
                // Too many cells, remove excess
                cell_contents.truncate(expected_count);
            }
            std::cmp::Ordering::Less => {
                // Too few cells, add empty ones
                while cell_contents.len() < expected_count {
                    cell_contents.push("");
                }
            }
            std::cmp::Ordering::Equal => {
                // Perfect number of cells, no adjustment needed
            }
        }

        // Reconstruct row
        let mut result = String::new();
        if has_leading_pipe {
            result.push('|');
        }

        for (i, cell) in cell_contents.iter().enumerate() {
            result.push_str(&format!(" {cell} "));
            if i < cell_contents.len() - 1 || has_trailing_pipe {
                result.push('|');
            }
        }

        Some(result)
    }
}

impl Rule for MD056TableColumnCount {
    fn name(&self) -> &'static str {
        "MD056"
    }

    fn description(&self) -> &'static str {
        "Table column count should be consistent"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Table
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Skip if no tables present
        !ctx.likely_has_tables()
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let content = ctx.content;
        let flavor = ctx.flavor;
        let mut warnings = Vec::new();

        // Early return for empty content or content without tables
        if content.is_empty() || !content.contains('|') {
            return Ok(Vec::new());
        }

        let lines = ctx.raw_lines();

        // Use pre-computed table blocks from context
        let table_blocks = &ctx.table_blocks;

        for table_block in table_blocks {
            // Collect all table lines for building the whole-table fix
            let all_line_indices: Vec<usize> = std::iter::once(table_block.header_line)
                .chain(std::iter::once(table_block.delimiter_line))
                .chain(table_block.content_lines.iter().copied())
                .collect();

            // Determine expected column count from header row (strip list/blockquote prefix first)
            let header_content = TableUtils::extract_table_row_content(lines[table_block.header_line], table_block, 0);
            let expected_count = TableUtils::count_cells_with_flavor(header_content, flavor);

            if expected_count == 0 {
                continue; // Skip invalid tables
            }

            // Check each row and emit a per-row fix. Per-row fixes ensure that
            // inline-disabling one row does not cause the fix on another row to
            // overwrite the disabled row's content.
            for (i, &line_idx) in all_line_indices.iter().enumerate() {
                let line = lines[line_idx];
                let row_content = TableUtils::extract_table_row_content(line, table_block, i);
                let count = TableUtils::count_cells_with_flavor(row_content, flavor);

                if count > 0 && count != expected_count {
                    let (start_line, start_col, end_line, end_col) = calculate_line_range(line_idx + 1, line);

                    // Build a per-row fix so inline-disabled rows are not
                    // overwritten by fixes on other rows in the same table.
                    let fixed_line = self
                        .fix_table_row_content(row_content, expected_count, flavor, table_block, i, line)
                        .unwrap_or_else(|| line.to_string());
                    let row_range = ctx.line_column_byte_range_with_length(line_idx + 1, 1, line.chars().count());

                    warnings.push(LintWarning {
                        rule_name: Some(self.name().to_string()),
                        message: format!("Table row has {count} cells, but expected {expected_count}"),
                        line: start_line,
                        column: start_col,
                        end_line,
                        end_column: end_col,
                        severity: Severity::Warning,
                        fix: Some(Fix::new(row_range, fixed_line)),
                    });
                }
            }
        }

        Ok(warnings)
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        if self.should_skip(ctx) {
            return Ok(ctx.content.to_string());
        }
        let warnings = self.check(ctx)?;
        if warnings.is_empty() {
            return Ok(ctx.content.to_string());
        }
        let warnings = warnings;
        crate::utils::fix_utils::apply_warning_fixes(ctx.content, &warnings).map_err(LintError::InvalidInput)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
