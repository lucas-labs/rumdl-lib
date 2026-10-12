/// Rule MD031: Blank lines around fenced code blocks
///
/// See [docs/md031.md](../../docs/md031.md) for full documentation, configuration, and examples.
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::blank_lines::is_blank_or_comment_only;
use crate::utils::calculate_indentation_width_default;
use crate::utils::mkdocs_attr_list::is_block_attribute_line;
use crate::utils::range_utils::calculate_line_range;

/// Configuration for MD031 rule
#[derive(Debug, Clone, PartialEq)]
pub struct MD031Config {
    /// Whether to require blank lines around code blocks in lists
    pub(crate) list_items: bool,
}

impl Default for MD031Config {
    fn default() -> Self {
        Self {
            list_items: default_list_items(),
        }
    }
}

fn default_list_items() -> bool {
    true
}

/// Rule MD031: Fenced code blocks should be surrounded by blank lines
#[derive(Clone, Default)]
pub struct MD031BlanksAroundFences {
    config: MD031Config,
}

impl MD031BlanksAroundFences {
    /// Check if a line is effectively empty (blank, a line holding nothing but
    /// HTML comments, or an empty blockquote line like ">")
    /// Uses the pre-computed blockquote info from LintContext for accurate detection
    fn is_effectively_empty_line(line_idx: usize, lines: &[&str], ctx: &crate::lint_context::LintContext) -> bool {
        let line = lines.get(line_idx).unwrap_or(&"");

        // A blank line, or one contributing nothing but comments (#866)
        if is_blank_or_comment_only(line) {
            return true;
        }

        // Check if this is an empty blockquote line (like ">", "> ", ">>", etc.)
        if let Some(line_info) = ctx.lines.get(line_idx)
            && let Some(ref bq) = line_info.blockquote
        {
            // If the blockquote content is empty, this is effectively a blank line
            return bq.content.trim().is_empty();
        }

        false
    }

    /// Check if a line is inside a list item
    fn is_in_list(&self, line_index: usize, lines: &[&str]) -> bool {
        // Look backwards to find if we're in a list item
        for i in (0..=line_index).rev() {
            let line = lines[i];
            let trimmed = line.trim_start();

            // If we hit a blank line, we're no longer in a list
            if trimmed.is_empty() {
                return false;
            }

            // Check for ordered list (number followed by . or ))
            if trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                let mut chars = trimmed.chars().skip_while(char::is_ascii_digit);
                if let Some(next) = chars.next()
                    && (next == '.' || next == ')')
                    && chars.next() == Some(' ')
                {
                    return true;
                }
            }

            // Check for unordered list
            if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
                return true;
            }

            // If this line is indented (3+ columns), it might be a continuation of a list item
            let is_indented = calculate_indentation_width_default(line) >= 3;
            if is_indented {
                continue; // Keep looking backwards for the list marker
            }

            // If we reach here and haven't found a list marker, and we're not at an indented line,
            // then we're not in a list
            return false;
        }

        false
    }

    /// Check if blank line should be required based on configuration
    fn should_require_blank_line(&self, line_index: usize, lines: &[&str]) -> bool {
        if self.config.list_items {
            // Always require blank lines when list_items is true
            true
        } else {
            // Don't require blank lines inside lists when list_items is false
            !self.is_in_list(line_index, lines)
        }
    }

    /// Check if the current line is immediately after frontmatter (prev line is closing delimiter)
    fn is_right_after_frontmatter(line_index: usize, ctx: &crate::lint_context::LintContext) -> bool {
        line_index > 0
            && ctx.lines.get(line_index - 1).is_some_and(|info| info.in_front_matter)
            && ctx.lines.get(line_index).is_some_and(|info| !info.in_front_matter)
    }

    /// Derive fenced code block line ranges from pre-computed code_block_details.
    ///
    /// Returns a vector of (opening_line_idx, closing_line_idx) for each fenced code block.
    /// The indices are 0-based line numbers.
    fn fenced_block_line_ranges(ctx: &crate::lint_context::LintContext) -> Vec<(usize, usize)> {
        let lines = ctx.raw_lines();

        ctx.code_block_details
            .iter()
            .filter(|d| d.is_fenced)
            .map(|detail| {
                // Convert start byte offset to line index
                let start_line = ctx
                    .line_offsets
                    .partition_point(|&off| off <= detail.start)
                    .saturating_sub(1);

                // Convert end byte offset to line index
                let end_byte = if detail.end > 0 { detail.end - 1 } else { 0 };
                let end_line = ctx
                    .line_offsets
                    .partition_point(|&off| off <= end_byte)
                    .saturating_sub(1);

                // Verify this is actually a closing fence line (not just end of content)
                let end_line_content = lines.get(end_line).unwrap_or(&"");
                let trimmed = end_line_content.trim();
                let content_after_bq = if trimmed.starts_with('>') {
                    trimmed.trim_start_matches(['>', ' ']).trim()
                } else {
                    trimmed
                };
                let is_closing_fence = (content_after_bq.starts_with("```") || content_after_bq.starts_with("~~~"))
                    && content_after_bq
                        .chars()
                        .skip_while(|&c| c == '`' || c == '~')
                        .all(char::is_whitespace);

                if is_closing_fence {
                    (start_line, end_line)
                } else {
                    (start_line, lines.len().saturating_sub(1))
                }
            })
            .collect()
    }
}

impl Rule for MD031BlanksAroundFences {
    fn name(&self) -> &'static str {
        "MD031"
    }

    fn description(&self) -> &'static str {
        "Fenced code blocks should be surrounded by blank lines"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = Vec::new();
        let lines = ctx.raw_lines();

        // Detect fenced code blocks using pulldown-cmark (handles list-indented fences correctly)
        let fenced_blocks = Self::fenced_block_line_ranges(ctx);

        // Helper to check if a line is a Pandoc/Quarto div marker (opening or closing)
        let is_pandoc_div_marker = |_line: &str| -> bool { false };

        // Check blank lines around each fenced code block
        for (opening_line, closing_line) in &fenced_blocks {
            // Skip fenced code blocks in skipped regions (comments, front-matter, etc.)
            if ctx.line_info(*opening_line + 1).is_some_and(|info| {
                info.in_html_comment || false || info.in_front_matter || info.in_mkdocstrings || false || false || false
            }) {
                continue;
            }

            // Check for blank line before opening fence
            // Skip if right after frontmatter
            // Skip if right after a Pandoc/Quarto div marker in Pandoc-compatible flavor
            // Use is_effectively_empty_line to handle blockquote blank lines (issue #284)
            let prev_line_is_pandoc_marker = *opening_line > 0 && is_pandoc_div_marker(lines[*opening_line - 1]);
            if *opening_line > 0
                && !Self::is_effectively_empty_line(*opening_line - 1, lines, ctx)
                && !Self::is_right_after_frontmatter(*opening_line, ctx)
                && !prev_line_is_pandoc_marker
                && self.should_require_blank_line(*opening_line, lines)
            {
                let (start_line, start_col, end_line, end_col) =
                    calculate_line_range(*opening_line + 1, lines[*opening_line]);

                let bq_prefix = ctx.blockquote_prefix_for_blank_line(*opening_line);
                warnings.push(LintWarning {
                    rule_name: Some(self.name().to_string()),
                    line: start_line,
                    column: start_col,
                    end_line,
                    end_column: end_col,
                    message: "No blank line before fenced code block".to_string(),
                    severity: Severity::Warning,
                    fix: Some(Fix::new(
                        ctx.line_column_byte_range_with_length(*opening_line + 1, 1, 0),
                        format!("{bq_prefix}\n"),
                    )),
                });
            }

            // Check for blank line after closing fence
            // Allow block attribute lists attached to the fence (Kramdown IALs in any
            // flavor, Hugo/MkDocs bare attr lists when the flavor enables them)
            // Skip if followed by a Pandoc/Quarto div marker in Pandoc-compatible flavor
            // Use is_effectively_empty_line to handle blockquote blank lines (issue #284)
            let next_line_is_pandoc_marker =
                *closing_line + 1 < lines.len() && is_pandoc_div_marker(lines[*closing_line + 1]);
            if *closing_line + 1 < lines.len()
                && !Self::is_effectively_empty_line(*closing_line + 1, lines, ctx)
                && !is_block_attribute_line(lines[*closing_line + 1], ctx.flavor)
                && !next_line_is_pandoc_marker
                && self.should_require_blank_line(*closing_line, lines)
            {
                let (start_line, start_col, end_line, end_col) =
                    calculate_line_range(*closing_line + 1, lines[*closing_line]);

                let bq_prefix = ctx.blockquote_prefix_for_blank_line(*closing_line);
                warnings.push(LintWarning {
                    rule_name: Some(self.name().to_string()),
                    line: start_line,
                    column: start_col,
                    end_line,
                    end_column: end_col,
                    message: "No blank line after fenced code block".to_string(),
                    severity: Severity::Warning,
                    fix: Some(Fix::new(
                        ctx.line_column_byte_range_with_length(*closing_line + 2, 1, 0),
                        format!("{bq_prefix}\n"),
                    )),
                });
            }
        }

        // Enforce blank lines around Azure DevOps colon code fences.

        // Enforce blank lines around MyST colon directives (:::{name} ... :::)

        // Handle MkDocs admonitions separately

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
        RuleCategory::CodeBlock
    }

    /// Check if this rule should be skipped
    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        if ctx.content.is_empty() {
            return true;
        }
        let has_fences = ctx.likely_has_code() || ctx.has_char('~');
        !has_fences && true && true && true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
