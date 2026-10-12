use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::mkdocs_attr_list::is_block_attribute_line;

/// Rule MD058: Blanks around tables
///
/// See [docs/md058.md](../../docs/md058.md) for full documentation, configuration, and examples.
///
/// Ensures tables have blank lines before and after them
///
/// Configuration for MD058 rule
#[derive(Debug, Clone, PartialEq)]
pub struct MD058Config {
    /// Minimum number of blank lines before tables
    pub minimum_before: usize,
    /// Minimum number of blank lines after tables
    pub minimum_after: usize,
}

impl Default for MD058Config {
    fn default() -> Self {
        Self {
            minimum_before: default_minimum_before(),
            minimum_after: default_minimum_after(),
        }
    }
}

fn default_minimum_before() -> usize {
    1
}

fn default_minimum_after() -> usize {
    1
}

#[derive(Clone, Default)]
pub struct MD058BlanksAroundTables {
    config: MD058Config,
}

impl MD058BlanksAroundTables {
    /// Create a new instance with the given configuration
    pub fn from_config_struct(config: MD058Config) -> Self {
        Self { config }
    }

    /// Check if a line is blank (including blockquote continuation lines and
    /// lines holding nothing but HTML comments)
    ///
    /// Delegates to the shared `is_blank_in_blockquote_context` utility function.
    /// This ensures consistent blank line detection across all rules that need
    /// to handle blockquote-prefixed blank lines (MD058, MD065, etc.), and to
    /// `is_blank_or_comment_only` for the comment convention the blank-line
    /// rules share (#866).
    fn is_blank_line(&self, line: &str) -> bool {
        crate::utils::regex_cache::is_blank_in_blockquote_context(line)
            || crate::utils::blank_lines::is_blank_or_comment_only(line)
    }

    /// Count the number of blank lines before a given line index
    fn count_blank_lines_before(&self, lines: &[&str], line_index: usize) -> usize {
        let mut count = 0;
        let mut i = line_index;
        while i > 0 {
            i -= 1;
            if self.is_blank_line(lines[i]) {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Count the number of blank lines after a given line index
    fn count_blank_lines_after(&self, lines: &[&str], line_index: usize) -> usize {
        let mut count = 0;
        let mut i = line_index + 1;
        while i < lines.len() {
            if self.is_blank_line(lines[i]) {
                count += 1;
                i += 1;
            } else {
                break;
            }
        }
        count
    }
}

impl Rule for MD058BlanksAroundTables {
    fn name(&self) -> &'static str {
        "MD058"
    }

    fn description(&self) -> &'static str {
        "Tables should be surrounded by blank lines"
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
        let mut warnings = Vec::new();

        // Early return for empty content or content without tables
        if content.is_empty() || !content.contains('|') {
            return Ok(Vec::new());
        }

        let lines = ctx.raw_lines();

        // Use pre-computed table blocks from context
        let table_blocks = &ctx.table_blocks;

        for table_block in table_blocks {
            // Check for sufficient blank lines before table
            if table_block.start_line > 0 {
                let blank_lines_before = self.count_blank_lines_before(lines, table_block.start_line);
                if blank_lines_before < self.config.minimum_before {
                    let needed = self.config.minimum_before - blank_lines_before;
                    let message = if self.config.minimum_before == 1 {
                        "Missing blank line before table".to_string()
                    } else {
                        format!("Missing {needed} blank lines before table")
                    };

                    let bq_prefix = ctx.blockquote_prefix_for_blank_line(table_block.start_line);
                    let replacement = format!("{bq_prefix}\n").repeat(needed);
                    warnings.push(LintWarning {
                        rule_name: Some(self.name().to_string()),
                        message,
                        line: table_block.start_line + 1,
                        column: 1,
                        end_line: table_block.start_line + 1,
                        end_column: 2,
                        severity: Severity::Warning,
                        fix: Some(Fix::new(
                            ctx.line_column_byte_range(table_block.start_line + 1, 1),
                            replacement,
                        )),
                    });
                }
            }

            // Check for sufficient blank lines after table
            if table_block.end_line < lines.len() - 1 {
                // Check if the next line is a block attribute list attached to the table
                let next_line_is_attribute = if table_block.end_line + 1 < lines.len() {
                    is_block_attribute_line(lines[table_block.end_line + 1], ctx.flavor)
                } else {
                    false
                };

                // Skip check if next line is a block attribute
                if !next_line_is_attribute {
                    let blank_lines_after = self.count_blank_lines_after(lines, table_block.end_line);
                    if blank_lines_after < self.config.minimum_after {
                        let needed = self.config.minimum_after - blank_lines_after;
                        let message = if self.config.minimum_after == 1 {
                            "Missing blank line after table".to_string()
                        } else {
                            format!("Missing {needed} blank lines after table")
                        };

                        let bq_prefix = ctx.blockquote_prefix_for_blank_line(table_block.end_line);
                        let replacement = format!("{bq_prefix}\n").repeat(needed);
                        warnings.push(LintWarning {
                            rule_name: Some(self.name().to_string()),
                            message,
                            line: table_block.end_line + 1,
                            column: lines[table_block.end_line].chars().count() + 1,
                            end_line: table_block.end_line + 1,
                            end_column: lines[table_block.end_line].chars().count() + 2,
                            severity: Severity::Warning,
                            fix: Some(Fix::new(
                                ctx.line_column_byte_range(
                                    table_block.end_line + 1,
                                    lines[table_block.end_line].len() + 1,
                                ),
                                replacement,
                            )),
                        });
                    }
                }
            }
        }

        Ok(warnings)
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        let content = ctx.content;

        let warnings = self.check(ctx)?;
        let mut warnings = warnings;
        if warnings.is_empty() {
            return Ok(content.to_string());
        }

        let lines = ctx.raw_lines();
        let mut result = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            // Check for warnings about missing blank lines before table
            let warning_before = warnings
                .iter()
                .position(|w| w.line == i + 1 && w.message.contains("before table"));

            if let Some(idx) = warning_before {
                let warning = &warnings[idx];
                // Extract number of needed blank lines from the message or use config default
                let needed_blanks = if warning.message.contains("Missing blank line before") {
                    1
                } else if let Some(start) = warning.message.find("Missing ") {
                    if let Some(end) = warning.message.find(" blank lines before") {
                        warning.message[start + 8..end].parse::<usize>().unwrap_or(1)
                    } else {
                        1
                    }
                } else {
                    1
                };

                // Add the required number of blank lines with blockquote prefix
                let bq_prefix = ctx.blockquote_prefix_for_blank_line(i);
                for _ in 0..needed_blanks {
                    result.push(bq_prefix.clone());
                }
                warnings.remove(idx);
            }

            result.push(lines[i].to_string());

            // Check for warnings about missing blank lines after table
            let warning_after = warnings
                .iter()
                .position(|w| w.line == i + 1 && w.message.contains("after table"));

            if let Some(idx) = warning_after {
                let warning = &warnings[idx];
                // Extract number of needed blank lines from the message or use config default
                let needed_blanks = if warning.message.contains("Missing blank line after") {
                    1
                } else if let Some(start) = warning.message.find("Missing ") {
                    if let Some(end) = warning.message.find(" blank lines after") {
                        warning.message[start + 8..end].parse::<usize>().unwrap_or(1)
                    } else {
                        1
                    }
                } else {
                    1
                };

                // Add the required number of blank lines with blockquote prefix
                let bq_prefix = ctx.blockquote_prefix_for_blank_line(i);
                for _ in 0..needed_blanks {
                    result.push(bq_prefix.clone());
                }
                warnings.remove(idx);
            }

            i += 1;
        }

        let mut fixed = result.join("\n");
        if content.ends_with('\n') {
            fixed.push('\n');
        }

        Ok(fixed)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
