use crate::filtered_lines::FilteredLinesExt;
use crate::lint_context::LintContext;
use crate::lint_context::types::HeadingStyle;
use crate::utils::range_utils::calculate_line_range;
use std::collections::HashSet;

use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};

mod md012_config;
use md012_config::MD012Config;

/// Rule MD012: No multiple consecutive blank lines
///
/// See [docs/md012.md](../../docs/md012.md) for full documentation, configuration, and examples.

#[derive(Debug, Clone)]
pub struct MD012NoMultipleBlanks {
    config: MD012Config,
    /// Maximum blank lines allowed adjacent to headings (above).
    /// Derived from MD022's lines-above config to avoid conflicts.
    heading_blanks_above: usize,
    /// Maximum blank lines allowed adjacent to headings (below).
    /// Derived from MD022's lines-below config to avoid conflicts.
    heading_blanks_below: usize,
}

impl Default for MD012NoMultipleBlanks {
    fn default() -> Self {
        Self {
            config: MD012Config::default(),
            heading_blanks_above: 1,
            heading_blanks_below: 1,
        }
    }
}

impl MD012NoMultipleBlanks {
    pub fn new(maximum: usize) -> Self {
        use crate::types::PositiveUsize;
        Self {
            config: MD012Config {
                maximum: PositiveUsize::new(maximum).unwrap_or(PositiveUsize::from_const(1)),
            },
            heading_blanks_above: 1,
            heading_blanks_below: 1,
        }
    }

    /// The effective maximum blank lines allowed for heading-adjacent runs.
    /// Returns the larger of MD012's own maximum and the relevant MD022 limit,
    /// so MD012 never flags blanks that MD022 requires.
    fn effective_max_above(&self) -> usize {
        self.config.maximum.get().max(self.heading_blanks_above)
    }

    fn effective_max_below(&self) -> usize {
        self.config.maximum.get().max(self.heading_blanks_below)
    }

    /// Generate warnings for excess blank lines beyond the given maximum.
    fn generate_excess_warnings(
        &self,
        blank_start: usize,
        blank_count: usize,
        effective_max: usize,
        lines: &[&str],
        lines_to_check: &HashSet<usize>,
        ctx: &LintContext,
    ) -> Vec<LintWarning> {
        let mut warnings = Vec::new();

        let location = if blank_start == 0 {
            "at start of file"
        } else {
            "between content"
        };

        for i in effective_max..blank_count {
            let excess_line_num = blank_start + i;
            if lines_to_check.contains(&excess_line_num) {
                let excess_line = excess_line_num + 1;
                let excess_line_content = lines.get(excess_line_num).unwrap_or(&"");
                let (start_line, start_col, end_line, end_col) = calculate_line_range(excess_line, excess_line_content);
                warnings.push(LintWarning {
                    rule_name: Some(self.name().to_string()),
                    severity: Severity::Warning,
                    message: format!("Multiple consecutive blank lines {location}"),
                    line: start_line,
                    column: start_col,
                    end_line,
                    end_column: end_col,
                    fix: Some(Fix::new(
                        {
                            let line_start = ctx.line_start_byte(excess_line).unwrap_or(0);
                            let line_end = ctx.line_start_byte(excess_line + 1).unwrap_or(line_start + 1);
                            line_start..line_end
                        },
                        String::new(),
                    )),
                });
            }
        }

        warnings
    }
}

/// Check if the given 0-based line index is part of a heading.
///
/// Returns true if:
/// - The line has heading info, or is one of the text lines of a setext heading
///   whose paragraph spans several lines, OR
/// - The previous line is a Setext heading text line (covers the Setext underline)
fn is_heading_context(ctx: &LintContext, line_idx: usize) -> bool {
    if ctx
        .lines
        .get(line_idx)
        .is_some_and(|li| li.heading.is_some() || li.is_setext_heading_text)
    {
        return true;
    }
    // Check if previous line is a Setext heading text line, so this line is the underline
    if line_idx > 0
        && let Some(prev_info) = ctx.lines.get(line_idx - 1)
        && let Some(ref heading) = prev_info.heading
        && matches!(heading.style, HeadingStyle::Setext1 | HeadingStyle::Setext2)
    {
        return true;
    }
    false
}

impl Rule for MD012NoMultipleBlanks {
    fn name(&self) -> &'static str {
        "MD012"
    }

    fn description(&self) -> &'static str {
        "Multiple consecutive blank lines"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Whitespace
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let content = ctx.content;

        // Early return for empty content
        if content.is_empty() {
            return Ok(Vec::new());
        }

        // Quick check for consecutive newlines or potential whitespace-only lines before processing
        // Look for multiple consecutive lines that could be blank (empty or whitespace-only)
        let lines = ctx.raw_lines();
        let has_potential_blanks = lines
            .windows(2)
            .any(|pair| pair[0].trim().is_empty() && pair[1].trim().is_empty());

        // Also check for blanks at EOF (markdownlint behavior). Only the CLI
        // normalises to LF; a CRLF document's trailing blanks are `\r\n\r\n`,
        // and the window check above already catches those.
        let ends_with_multiple_newlines = content.ends_with("\n\n");

        if !has_potential_blanks && !ends_with_multiple_newlines {
            return Ok(Vec::new());
        }

        let mut warnings = Vec::new();

        // Single-pass algorithm with immediate counter reset
        let mut blank_count = 0;
        let mut blank_start = 0;
        let mut last_line_num: Option<usize> = None;
        // Track the last non-blank content line for heading adjacency checks
        let mut prev_content_line_num: Option<usize> = None;

        // Use HashSet for O(1) lookups of lines that need to be checked
        let mut lines_to_check: HashSet<usize> = HashSet::new();

        // Use filtered_lines to automatically skip front-matter, code blocks, Quarto divs, math blocks,
        // PyMdown blocks, and Obsidian comments.
        // The in_code_block field in LineInfo is pre-computed using pulldown-cmark
        // and correctly handles both fenced code blocks and indented code blocks.
        // Flavor-specific fields (in_pandoc_div, in_pymdown_block, in_obsidian_comment) are only
        // set for their respective flavors, so the skip filters have no effect otherwise.
        for filtered_line in ctx
            .filtered_lines()
            .skip_front_matter()
            .skip_code_blocks()
            .skip_html_comments()
            .skip_html_blocks()
            .skip_quarto_divs()
            .skip_math_blocks()
            .skip_obsidian_comments()
            .skip_pymdown_blocks()
            .skip_jsx_expressions()
            .skip_mdx_comments()
            .skip_jsx_blocks()
            .skip_esm_blocks()
        {
            let line_num = filtered_line.line_num - 1; // Convert 1-based to 0-based for internal tracking
            let line = filtered_line.content;

            // Detect when lines were skipped (e.g., code block content)
            // If we jump more than 1 line, there was content between, which breaks blank sequences
            if let Some(last) = last_line_num
                && line_num > last + 1
            {
                // Lines were skipped (code block or similar)
                // Generate warnings for any accumulated blanks before the skip
                let effective_max = if prev_content_line_num.is_some_and(|idx| is_heading_context(ctx, idx)) {
                    self.effective_max_below()
                } else {
                    self.config.maximum.get()
                };
                if blank_count > effective_max {
                    warnings.extend(self.generate_excess_warnings(
                        blank_start,
                        blank_count,
                        effective_max,
                        lines,
                        &lines_to_check,
                        ctx,
                    ));
                }
                blank_count = 0;
                lines_to_check.clear();
                // Reset heading context across skipped regions (code blocks, etc.)
                prev_content_line_num = None;
            }
            last_line_num = Some(line_num);

            if line.trim().is_empty() {
                if blank_count == 0 {
                    blank_start = line_num;
                }
                blank_count += 1;
                // Store line numbers that exceed the limit
                if blank_count > self.config.maximum.get() {
                    lines_to_check.insert(line_num);
                }
            } else {
                // Determine effective maximum for this blank run.
                // Heading-adjacent blanks use the higher of MD012's maximum
                // and MD022's required blank lines, so MD012 doesn't conflict.
                // Start-of-file blanks (blank_start == 0) before a heading use
                // the normal maximum — no rule requires blanks at file start.
                let heading_below = prev_content_line_num.is_some_and(|idx| is_heading_context(ctx, idx));
                let heading_above = blank_start > 0 && is_heading_context(ctx, line_num);
                let effective_max = if heading_below && heading_above {
                    // Between two headings: use the larger of above/below limits
                    self.effective_max_above().max(self.effective_max_below())
                } else if heading_below {
                    self.effective_max_below()
                } else if heading_above {
                    self.effective_max_above()
                } else {
                    self.config.maximum.get()
                };

                if blank_count > effective_max {
                    warnings.extend(self.generate_excess_warnings(
                        blank_start,
                        blank_count,
                        effective_max,
                        lines,
                        &lines_to_check,
                        ctx,
                    ));
                }
                blank_count = 0;
                lines_to_check.clear();
                prev_content_line_num = Some(line_num);
            }
        }

        // Handle trailing blanks at EOF
        // Main loop only reports mid-document blanks (between content)
        // EOF handler reports trailing blanks with stricter rules (any blank at EOF is flagged)
        //
        // The blank_count at end of loop might include blanks BEFORE a code block at EOF,
        // which aren't truly "trailing blanks". We need to verify the actual last line is blank.
        let last_line_is_blank = lines.last().is_some_and(|l| l.trim().is_empty());

        // Blanks left over before a skipped region that runs to EOF are mid-document
        // blanks, so they use the same limits as the skip handling inside the loop.
        if blank_count > 0 && !last_line_is_blank {
            let effective_max = if prev_content_line_num.is_some_and(|idx| is_heading_context(ctx, idx)) {
                self.effective_max_below()
            } else {
                self.config.maximum.get()
            };
            if blank_count > effective_max {
                warnings.extend(self.generate_excess_warnings(
                    blank_start,
                    blank_count,
                    effective_max,
                    lines,
                    &lines_to_check,
                    ctx,
                ));
            }
        }

        // Check for trailing blank lines
        // EOF semantics: ANY blank line at EOF should be flagged (stricter than mid-document)
        // Only fire if the actual last line(s) of the file are blank
        if blank_count > 0 && last_line_is_blank {
            let location = "at end of file";

            // Report on the last line (which is blank)
            let report_line = lines.len();

            // Calculate fix: remove all trailing blank lines
            // Find where the trailing blanks start (blank_count tells us how many consecutive blanks)
            let fix_start = ctx.line_start_byte(report_line - blank_count + 1).unwrap_or(0);
            let fix_end = content.len();

            // Report one warning for the excess blank lines at EOF
            warnings.push(LintWarning {
                rule_name: Some(self.name().to_string()),
                severity: Severity::Warning,
                message: format!("Multiple consecutive blank lines {location}"),
                line: report_line,
                column: 1,
                end_line: report_line,
                end_column: 1,
                fix: Some(Fix::new(fix_start..fix_end, String::new())),
            });
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

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Skip if content is empty or doesn't have newlines (single line can't have multiple blanks)
        ctx.content.is_empty() || !ctx.has_char('\n')
    }
}
