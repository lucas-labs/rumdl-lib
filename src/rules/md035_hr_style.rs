//!
//! Rule MD035: Horizontal rule style
//!
//! See [docs/md035.md](../../docs/md035.md) for full documentation, configuration, and examples.

use crate::utils::range_utils::calculate_line_range;

use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};

mod md035_config;
use md035_config::MD035Config;

/// Represents the style for horizontal rules
#[derive(Clone, Default)]
pub struct MD035HRStyle {
    config: MD035Config,
}

impl MD035HRStyle {
    pub fn new(style: String) -> Self {
        Self {
            config: MD035Config { style },
        }
    }

    fn is_horizontal_rule(line: &str) -> bool {
        crate::utils::thematic_break::is_thematic_break(line)
    }

    /// Check if a line might be a Setext heading underline
    fn is_potential_setext_heading(lines: &[&str], i: usize) -> bool {
        if i == 0 {
            return false; // First line can't be a Setext heading underline
        }

        let line = lines[i].trim();
        let prev_line = lines[i - 1].trim();

        let is_dash_line = !line.is_empty() && line.chars().all(|c| c == '-');
        let is_equals_line = !line.is_empty() && line.chars().all(|c| c == '=');
        let prev_line_has_content = !prev_line.is_empty() && !Self::is_horizontal_rule(prev_line);
        (is_dash_line || is_equals_line) && prev_line_has_content
    }

    /// Find the most prevalent HR style in the document (excluding setext headings, code blocks, and frontmatter)
    fn most_prevalent_hr_style(lines: &[&str], ctx: &crate::lint_context::LintContext) -> Option<String> {
        use std::collections::HashMap;
        let mut counts: HashMap<&str, usize> = HashMap::new();
        let mut order: Vec<&str> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            // Skip if this line is in frontmatter, code block, or MkDocs markdown HTML div
            if let Some(line_info) = ctx.lines.get(i)
                && (line_info.in_front_matter || line_info.in_code_block || line_info.in_mkdocs_html_markdown)
            {
                continue;
            }

            if Self::is_horizontal_rule(line) && !Self::is_potential_setext_heading(lines, i) {
                let style = line.trim();
                let counter = counts.entry(style).or_insert(0);
                *counter += 1;
                if *counter == 1 {
                    order.push(style);
                }
            }
        }
        // Find the style with the highest count, breaking ties by first encountered
        counts
            .iter()
            .max_by_key(|&(style, count)| {
                (
                    *count,
                    -(order.iter().position(|&s| s == *style).unwrap_or(usize::MAX) as isize),
                )
            })
            .map(|(style, _)| style.to_string())
    }
}

impl Rule for MD035HRStyle {
    fn name(&self) -> &'static str {
        "MD035"
    }

    fn description(&self) -> &'static str {
        "Horizontal rule style"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Whitespace
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = Vec::new();
        let lines = ctx.raw_lines();

        // Use the configured style or find the most prevalent HR style
        let expected_style = if self.config.style.is_empty() || self.config.style == "consistent" {
            Self::most_prevalent_hr_style(lines, ctx).unwrap_or_else(|| "---".to_string())
        } else {
            self.config.style.clone()
        };

        for (i, line) in lines.iter().enumerate() {
            // Skip if this line is in frontmatter, code block, or MkDocs markdown HTML div (grid cards use indented HRs)
            if let Some(line_info) = ctx.lines.get(i)
                && (line_info.in_front_matter || line_info.in_code_block || line_info.in_mkdocs_html_markdown)
            {
                continue;
            }

            // Skip if this is a potential Setext heading underline
            if Self::is_potential_setext_heading(lines, i) {
                continue;
            }

            if Self::is_horizontal_rule(line) {
                // Check if this HR matches the expected style
                let has_indentation = line.len() > line.trim_start().len();
                let style_mismatch = line.trim() != expected_style;

                if style_mismatch || has_indentation {
                    // Calculate precise character range for the entire horizontal rule
                    let (start_line, start_col, end_line, end_col) = calculate_line_range(i + 1, line);

                    warnings.push(LintWarning {
                        rule_name: Some(self.name().to_string()),
                        line: start_line,
                        column: start_col,
                        end_line,
                        end_column: end_col,
                        message: if has_indentation {
                            "Horizontal rule should not be indented".to_string()
                        } else {
                            format!("Horizontal rule style should be \"{expected_style}\"")
                        },
                        severity: Severity::Warning,
                        fix: Some(Fix::new(
                            ctx.line_column_byte_range_with_length(i + 1, 1, line.chars().count()),
                            expected_style.clone(),
                        )),
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
        crate::utils::fix_utils::apply_warning_fixes(ctx.content, &warnings)
            .map_err(crate::rule::LintError::InvalidInput)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// Check if this rule should be skipped
    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // HR can use -, *, or _
        ctx.content.is_empty() || (!ctx.has_char('-') && !ctx.has_char('*') && !ctx.has_char('_'))
    }
}
