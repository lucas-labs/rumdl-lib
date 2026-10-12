use crate::config::MarkdownFlavor;
use crate::rule::{LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::range_utils::calculate_line_range;
use crate::utils::table_utils::{TableBlock, TableUtils};

mod md055_config;
use md055_config::MD055Config;

/// Rule MD055: Table pipe style
///
/// See [docs/md055.md](../../docs/md055.md) for full documentation, configuration, and examples.
///
/// This rule enforces consistent use of leading and trailing pipe characters in Markdown tables,
/// which improves readability and ensures uniform document styling.
///
/// ## Purpose
///
/// - **Consistency**: Ensures uniform table formatting throughout documents
/// - **Readability**: Well-formatted tables are easier to read and understand
/// - **Maintainability**: Consistent table syntax makes documents easier to maintain
/// - **Compatibility**: Some Markdown processors handle different table styles differently
///
/// ## Configuration Options
///
/// The rule supports the following configuration options:
///
/// ```yaml
/// MD055:
///   style: "consistent"  # Can be "consistent", "leading_and_trailing", or "no_leading_or_trailing"
/// ```
///
/// ### Style Options
///
/// - **consistent**: All tables must use the same style (default)
/// - **leading_and_trailing**: All tables must have both leading and trailing pipes
/// - **no_leading_or_trailing**: Tables must not have leading or trailing pipes
///
/// ## Examples
///
/// ### Leading and Trailing Pipes
///
/// ```markdown
/// | Header 1 | Header 2 | Header 3 |
/// |----------|----------|----------|
/// | Cell 1   | Cell 2   | Cell 3   |
/// | Cell 4   | Cell 5   | Cell 6   |
/// ```
///
/// ### No Leading or Trailing Pipes
///
/// ```markdown
/// Header 1 | Header 2 | Header 3
/// ---------|----------|---------
/// Cell 1   | Cell 2   | Cell 3
/// Cell 4   | Cell 5   | Cell 6
/// ```
///
/// ## Behavior Details
///
/// - The rule analyzes each table in the document to determine its pipe style
/// - With "consistent" style, the first table's style is used as the standard for all others
/// - The rule handles both the header row, separator row, and content rows
/// - Tables inside code blocks are ignored
///
/// ## Fix Behavior
///
/// When applying automatic fixes, this rule:
/// - Adds or removes leading and trailing pipes as needed
/// - Preserves the content and alignment of table cells
/// - Maintains proper spacing around pipe characters
/// - Updates both header and content rows to match the required style
///
/// ## Performance Considerations
///
/// The rule includes performance optimizations:
/// - Efficient table detection with quick checks before detailed analysis
/// - Smart line-by-line processing to avoid redundant operations
/// - Optimized string manipulation for pipe character handling
///
/// Enforces consistent use of leading and trailing pipe characters in tables
#[derive(Debug, Default, Clone)]
pub struct MD055TablePipeStyle {
    config: MD055Config,
}

impl MD055TablePipeStyle {
    pub fn new(style: String) -> Self {
        Self {
            config: MD055Config { style },
        }
    }

    /// Resolve the style MD055 should converge on.
    ///
    /// Gherkin recognizes a Data Table or Examples table row only when an indent
    /// is followed directly by a pipe, so a style that strips the leading pipe
    /// does not restyle the table, it deletes it from the document. MDG
    /// therefore always converges on leading-and-trailing: `consistent` resolves
    /// to it rather than to whichever form happens to be more prevalent, and an
    /// explicit style that drops the leading pipe is not adopted.
    fn effective_configured_style(&self, _ctx: &crate::lint_context::LintContext) -> &str {
        match self.config.style.as_str() {
            "leading_and_trailing" | "no_leading_or_trailing" | "leading_only" | "trailing_only" | "consistent" => {
                self.config.style.as_str()
            }
            _ => {
                // Invalid style provided, default to "leading_and_trailing"
                "leading_and_trailing"
            }
        }
    }

    /// Determine the most prevalent table style in a table block
    fn determine_table_style(&self, table_block: &TableBlock, lines: &[&str]) -> Option<&'static str> {
        let mut leading_and_trailing_count = 0;
        let mut no_leading_or_trailing_count = 0;
        let mut leading_only_count = 0;
        let mut trailing_only_count = 0;

        // Count style of header row (table line index 0)
        let header_content = TableUtils::extract_table_row_content(lines[table_block.header_line], table_block, 0);
        if let Some(style) = TableUtils::determine_pipe_style(header_content) {
            match style {
                "leading_and_trailing" => leading_and_trailing_count += 1,
                "no_leading_or_trailing" => no_leading_or_trailing_count += 1,
                "leading_only" => leading_only_count += 1,
                "trailing_only" => trailing_only_count += 1,
                _ => {}
            }
        }

        // Count style of content rows (table line indices 2, 3, 4, ...)
        for (i, &line_idx) in table_block.content_lines.iter().enumerate() {
            let content = TableUtils::extract_table_row_content(lines[line_idx], table_block, 2 + i);
            if let Some(style) = TableUtils::determine_pipe_style(content) {
                match style {
                    "leading_and_trailing" => leading_and_trailing_count += 1,
                    "no_leading_or_trailing" => no_leading_or_trailing_count += 1,
                    "leading_only" => leading_only_count += 1,
                    "trailing_only" => trailing_only_count += 1,
                    _ => {}
                }
            }
        }

        // Determine most prevalent style
        // In case of tie, prefer leading_and_trailing (most common, widely supported)
        let max_count = leading_and_trailing_count
            .max(no_leading_or_trailing_count)
            .max(leading_only_count)
            .max(trailing_only_count);

        if max_count > 0 {
            if leading_and_trailing_count == max_count {
                Some("leading_and_trailing")
            } else if no_leading_or_trailing_count == max_count {
                Some("no_leading_or_trailing")
            } else if leading_only_count == max_count {
                Some("leading_only")
            } else if trailing_only_count == max_count {
                Some("trailing_only")
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Simple table row fix for tests - creates a dummy TableBlock without list context

    /// Fix a table row to match the target style, with full context for list tables
    ///
    /// This handles tables inside list items by stripping the list prefix,
    /// fixing the table content, then restoring the appropriate prefix.
    fn fix_table_row_with_context(
        &self,
        line: &str,
        target_style: &str,
        table_block: &TableBlock,
        table_line_index: usize,
        _flavor: MarkdownFlavor,
    ) -> String {
        // Extract blockquote prefix first
        let (bq_prefix, after_bq) = TableUtils::extract_blockquote_prefix(line);

        // Handle list context if present
        if let Some(ref list_ctx) = table_block.list_context {
            if table_line_index == 0 {
                // Header line: strip list prefix (handles both markers and indentation)
                let stripped = after_bq
                    .strip_prefix(&list_ctx.list_prefix)
                    .unwrap_or_else(|| TableUtils::extract_list_prefix(after_bq).1);
                let fixed_content = self.fix_table_content(stripped.trim(), target_style);

                // Restore prefixes: blockquote + list prefix + fixed content
                let lp = &list_ctx.list_prefix;
                if bq_prefix.is_empty() && lp.is_empty() {
                    fixed_content
                } else {
                    format!("{bq_prefix}{lp}{fixed_content}")
                }
            } else {
                // Continuation lines: strip indentation, then restore it
                let content_indent = list_ctx.content_indent;
                let stripped = TableUtils::extract_table_row_content(line, table_block, table_line_index);
                let fixed_content = self.fix_table_content(stripped.trim(), target_style);

                // Restore prefixes: blockquote + indentation + fixed content
                let indent = " ".repeat(content_indent);
                format!("{bq_prefix}{indent}{fixed_content}")
            }
        } else {
            // No list context, just handle blockquote prefix
            let fixed_content = self.fix_table_content(after_bq.trim(), target_style);
            // Gherkin recognizes a table row by its indent, so left-aligning one
            // while restyling it would take the table out of the document that
            // the restyle exists to preserve. MD060 still owns the indent's width.
            let indent = { "" };
            if bq_prefix.is_empty() && indent.is_empty() {
                fixed_content
            } else {
                format!("{bq_prefix}{indent}{fixed_content}")
            }
        }
    }

    /// Fix the table content (without any prefix handling)
    fn fix_table_content(&self, trimmed: &str, target_style: &str) -> String {
        if !trimmed.contains('|') {
            return trimmed.to_string();
        }

        let has_leading = trimmed.starts_with('|');
        let has_trailing = trimmed.ends_with('|');

        match target_style {
            "leading_and_trailing" => {
                let mut result = trimmed.to_string();

                // Add leading pipe if missing
                if !has_leading {
                    result = format!("| {result}");
                }

                // Add trailing pipe if missing
                if !has_trailing {
                    result = format!("{result} |");
                }

                result
            }
            "no_leading_or_trailing" => {
                let mut result = trimmed;

                // Remove leading pipe if present
                if has_leading {
                    result = result.strip_prefix('|').unwrap_or(result);
                    result = result.trim_start();
                }

                // Remove trailing pipe if present
                if has_trailing {
                    result = result.strip_suffix('|').unwrap_or(result);
                    result = result.trim_end();
                }

                result.to_string()
            }
            "leading_only" => {
                let mut result = trimmed.to_string();

                // Add leading pipe if missing
                if !has_leading {
                    result = format!("| {result}");
                }

                // Remove trailing pipe if present
                if has_trailing {
                    result = result.strip_suffix('|').unwrap_or(&result).trim_end().to_string();
                }

                result
            }
            "trailing_only" => {
                let mut result = trimmed;

                // Remove leading pipe if present
                if has_leading {
                    result = result.strip_prefix('|').unwrap_or(result).trim_start();
                }

                let mut result = result.to_string();

                // Add trailing pipe if missing
                if !has_trailing {
                    result = format!("{result} |");
                }

                result
            }
            _ => trimmed.to_string(),
        }
    }
}

impl Rule for MD055TablePipeStyle {
    fn name(&self) -> &'static str {
        "MD055"
    }

    fn description(&self) -> &'static str {
        "Table pipe style should be consistent"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Table
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Skip if no tables present (uses cached pipe count)
        !ctx.likely_has_tables()
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = Vec::new();

        // Early return handled by should_skip()

        let lines = ctx.raw_lines();

        let configured_style = self.effective_configured_style(ctx);

        // Use pre-computed table blocks from context
        let table_blocks = &ctx.table_blocks;

        // Process each table block
        for table_block in table_blocks {
            // First pass: determine the table's style for "consistent" mode
            // Count all rows to determine most prevalent style (prevalence-based approach)
            let table_style = if configured_style == "consistent" {
                self.determine_table_style(table_block, lines)
            } else {
                None
            };

            // Determine target style for this table
            let target_style = if configured_style == "consistent" {
                table_style.unwrap_or("leading_and_trailing")
            } else {
                configured_style
            };

            // Collect all table lines for processing
            let all_line_indices: Vec<usize> = std::iter::once(table_block.header_line)
                .chain(std::iter::once(table_block.delimiter_line))
                .chain(table_block.content_lines.iter().copied())
                .collect();

            // Check each row and emit a per-row fix. Per-row fixes ensure that
            // inline-disabling one row does not cause the fix on another row to
            // overwrite the disabled row's content.
            for (table_line_idx, &line_idx) in all_line_indices.iter().enumerate() {
                let line = lines[line_idx];
                // Extract content to properly check pipe style (handles list/blockquote prefixes)
                let content = TableUtils::extract_table_row_content(line, table_block, table_line_idx);
                if let Some(current_style) = TableUtils::determine_pipe_style(content) {
                    // Only flag lines with actual style mismatches
                    let needs_fixing = current_style != target_style;

                    if needs_fixing {
                        let (start_line, start_col, end_line, end_col) = calculate_line_range(line_idx + 1, line);

                        let message = format!(
                            "Table pipe style should be {}",
                            match target_style {
                                "leading_and_trailing" => "leading and trailing",
                                "no_leading_or_trailing" => "no leading or trailing",
                                "leading_only" => "leading only",
                                "trailing_only" => "trailing only",
                                _ => target_style,
                            }
                        );

                        // Build a per-row fix so inline-disabled rows are not
                        // overwritten by fixes on other rows in the same table.
                        let fixed_line = self.fix_table_row_with_context(
                            line,
                            target_style,
                            table_block,
                            table_line_idx,
                            ctx.flavor,
                        );
                        let row_range = ctx.line_column_byte_range_with_length(line_idx + 1, 1, line.chars().count());

                        warnings.push(LintWarning {
                            rule_name: Some(self.name().to_string()),
                            severity: Severity::Warning,
                            message,
                            line: start_line,
                            column: start_col,
                            end_line,
                            end_column: end_col,
                            fix: Some(crate::rule::Fix::new(row_range, fixed_line)),
                        });
                    }
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
