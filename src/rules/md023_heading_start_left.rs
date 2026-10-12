/// Rule MD023: Headings must start at the left margin
///
/// See [docs/md023.md](../../docs/md023.md) for full documentation, configuration, and examples.
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::range_utils::calculate_single_line_range;

#[derive(Clone)]
pub struct MD023HeadingStartLeft;

impl Rule for MD023HeadingStartLeft {
    fn name(&self) -> &'static str {
        "MD023"
    }

    fn description(&self) -> &'static str {
        "Headings must start at the beginning of the line"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        // Early return for empty content
        if ctx.lines.is_empty() {
            return Ok(vec![]);
        }

        let mut warnings = Vec::new();

        // Process all headings using cached heading information
        for (line_num, line_info) in ctx.lines.iter().enumerate() {
            // Skip lines inside PyMdown blocks, admonitions, and content tabs:
            // those containers are indentation-scoped, so a heading indented
            // there is intentionally nested, not an accidental indent. This
            // deliberately excludes markdown="1" HTML divs (tag-scoped, content
            // needs no indentation, and detected in every flavor).
            if line_info.in_admonition || line_info.in_content_tab {
                continue;
            }

            if let Some(heading) = &line_info.heading {
                // Skip hashtag-like patterns (e.g., #tag, #123, #29039) for ATX level 1
                // These are likely issue refs or social hashtags, not intended headings
                if heading.level == 1 && matches!(heading.style, crate::lint_context::HeadingStyle::ATX) {
                    // Get first "word" of heading text (up to space, comma, or closing paren)
                    let first_word: String = heading
                        .text
                        .trim()
                        .chars()
                        .take_while(|c| !c.is_whitespace() && *c != ',' && *c != ')')
                        .collect();
                    if let Some(first_char) = first_word.chars().next() {
                        // Skip if first word starts with lowercase or number
                        if first_char.is_lowercase() || first_char.is_numeric() {
                            continue;
                        }
                    }
                }

                let indentation = line_info.indent;
                let is_setext = matches!(
                    heading.style,
                    crate::lint_context::HeadingStyle::Setext1 | crate::lint_context::HeadingStyle::Setext2
                );

                // A setext heading's text is the whole paragraph its underline
                // ends, and CommonMark strips the leading whitespace of every one
                // of those lines, so each of them carries indentation of its own
                if is_setext {
                    // For Setext headings, we need to fix both the heading text and underline
                    let underline_line = line_num + 1;
                    let first_text_line = line_num + 1 - heading.text_lines;

                    for text_line in first_text_line..=line_num {
                        let text_indentation = ctx.lines[text_line].indent;
                        if text_indentation == 0 {
                            continue;
                        }

                        // Calculate precise character range for the indentation
                        let (start_line_calc, start_col, end_line, end_col) = calculate_single_line_range(
                            text_line + 1, // Convert to 1-indexed
                            1,
                            text_indentation,
                        );

                        // Add warning for the heading text line
                        warnings.push(LintWarning {
                            rule_name: Some(self.name().to_string()),
                            line: start_line_calc,
                            column: start_col,
                            end_line,
                            end_column: end_col,
                            severity: Severity::Warning,
                            message: format!("Setext heading should not be indented by {text_indentation} spaces"),
                            fix: Some(Fix::new(
                                {
                                    // indent is in bytes, so use byte offset directly
                                    let line_start = ctx.line_start_byte(text_line + 1).unwrap_or(0);
                                    line_start..line_start + text_indentation
                                },
                                String::new(),
                            )),
                        });
                    }

                    // Add warning for the underline - only if it's indented
                    if underline_line < ctx.lines.len() {
                        let underline_indentation = ctx.lines[underline_line].indent;
                        if underline_indentation > 0 {
                            let (underline_start_line, underline_start_col, underline_end_line, underline_end_col) =
                                calculate_single_line_range(underline_line + 1, 1, underline_indentation);

                            warnings.push(LintWarning {
                                rule_name: Some(self.name().to_string()),
                                line: underline_start_line,
                                column: underline_start_col,
                                end_line: underline_end_line,
                                end_column: underline_end_col,
                                severity: Severity::Warning,
                                message: "Setext heading underline should not be indented".to_string(),
                                fix: Some(Fix::new(
                                    {
                                        let line_start = ctx.line_start_byte(underline_line + 1).unwrap_or(0);
                                        line_start..line_start + underline_indentation
                                    },
                                    String::new(),
                                )),
                            });
                        }
                    }
                } else if indentation > 0 {
                    // For ATX headings, just fix the single line

                    // Calculate precise character range for the indentation
                    let (atx_start_line, atx_start_col, atx_end_line, atx_end_col) = calculate_single_line_range(
                        line_num + 1, // Convert to 1-indexed
                        1,
                        indentation,
                    );

                    warnings.push(LintWarning {
                        rule_name: Some(self.name().to_string()),
                        line: atx_start_line,
                        column: atx_start_col,
                        end_line: atx_end_line,
                        end_column: atx_end_col,
                        severity: Severity::Warning,
                        message: format!("Heading should not be indented by {indentation} spaces"),
                        fix: Some(Fix::new(
                            {
                                let line_start = ctx.line_start_byte(line_num + 1).unwrap_or(0);
                                line_start..line_start + indentation
                            },
                            String::new(),
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

    /// Get the category of this rule for selective processing
    fn category(&self) -> RuleCategory {
        RuleCategory::Heading
    }

    /// Check if this rule should be skipped
    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Fast path: check if document likely has headings
        if !ctx.likely_has_headings() {
            return true;
        }
        // Verify headings actually exist
        ctx.lines.iter().all(|line| line.heading.is_none())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
