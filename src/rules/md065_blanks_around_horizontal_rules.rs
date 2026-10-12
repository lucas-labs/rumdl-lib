use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};

/// Rule MD065: Blanks around horizontal rules
///
/// See [docs/md065.md](../../docs/md065.md) for full documentation and examples.
///
/// Ensures horizontal rules have blank lines before and after them

#[derive(Clone, Default)]
pub struct MD065BlanksAroundHorizontalRules;

impl MD065BlanksAroundHorizontalRules {
    /// Check if a line is blank (including blockquote continuation lines)
    ///
    /// Uses the shared `is_blank_in_blockquote_context` utility function for
    /// consistent blank line detection across all rules.
    fn is_blank_line(line: &str) -> bool {
        crate::utils::regex_cache::is_blank_in_blockquote_context(line)
    }

    /// Check if this might be a setext heading underline (not a horizontal rule)
    fn is_setext_heading_marker(lines: &[&str], line_index: usize) -> bool {
        if line_index == 0 {
            return false;
        }

        let line = lines[line_index].trim();
        let prev_line = lines[line_index - 1].trim();

        // Setext markers are only - or = (not * or _)
        // And the previous line must have content (not blank, not itself an HR)
        // CommonMark: setext underlines can have leading/trailing spaces but NO internal spaces
        if prev_line.is_empty() {
            return false;
        }

        // If the previous line is itself a horizontal rule, this cannot be a setext heading
        if crate::lint_context::is_horizontal_rule_line(prev_line) {
            return false;
        }

        // Check if all non-space characters are the same marker (- or =)
        // and there are no internal spaces (spaces between markers)
        let has_hyphen = line.contains('-');
        let has_equals = line.contains('=');

        // Must have exactly one type of marker
        if has_hyphen == has_equals {
            return false; // Either has both or neither
        }

        let marker = if has_hyphen { '-' } else { '=' };

        // Setext underline: optional leading spaces, then only marker chars, then optional trailing spaces
        // No internal spaces allowed
        let trimmed = line.trim();
        trimmed.chars().all(|c| c == marker)
    }

    /// Count the number of blank lines before a given line index
    fn count_blank_lines_before(lines: &[&str], line_index: usize) -> usize {
        let mut count = 0;
        let mut i = line_index;
        while i > 0 {
            i -= 1;
            if Self::is_blank_line(lines[i]) {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Count the number of blank lines after a given line index
    fn count_blank_lines_after(lines: &[&str], line_index: usize) -> usize {
        let mut count = 0;
        let mut i = line_index + 1;
        while i < lines.len() {
            if Self::is_blank_line(lines[i]) {
                count += 1;
                i += 1;
            } else {
                break;
            }
        }
        count
    }
}

impl Rule for MD065BlanksAroundHorizontalRules {
    fn name(&self) -> &'static str {
        "MD065"
    }

    fn description(&self) -> &'static str {
        "Horizontal rules should be surrounded by blank lines"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Whitespace
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // A thematic break may be written with spaces between its markers (`* * *`,
        // `- - -`, `_ _ _`), so no substring of the content identifies one. The check
        // below reads the same per-line flag the rule itself acts on, which is already
        // computed and already excludes code blocks and front matter.
        ctx.content.is_empty() || !ctx.lines.iter().any(|line| line.is_horizontal_rule)
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let content = ctx.content;
        let mut warnings = Vec::new();

        if content.is_empty() {
            return Ok(Vec::new());
        }

        let lines = ctx.raw_lines();

        for (i, line_info) in ctx.lines.iter().enumerate() {
            // Use pre-computed is_horizontal_rule from LineInfo
            // This already excludes code blocks, frontmatter, and does proper HR detection
            if !line_info.is_horizontal_rule {
                continue;
            }

            // Skip the underline of a setext heading. The parser records the
            // heading on the line above, inside blockquotes too; the text check
            // covers underlines it leaves unrecorded, such as one below `#tag`.
            if ctx.heading_on_line(i).is_some_and(|heading| heading.is_setext())
                || Self::is_setext_heading_marker(lines, i)
            {
                continue;
            }

            // Check for blank line before HR (unless at start of document)
            if i > 0 && Self::count_blank_lines_before(lines, i) == 0 {
                let bq_prefix = ctx.blockquote_prefix_for_blank_line(i);
                warnings.push(LintWarning {
                    rule_name: Some(self.name().to_string()),
                    message: "Missing blank line before horizontal rule".to_string(),
                    line: i + 1,
                    column: 1,
                    end_line: i + 1,
                    end_column: 2,
                    severity: Severity::Warning,
                    fix: Some(Fix::new(ctx.line_column_byte_range(i + 1, 1), format!("{bq_prefix}\n"))),
                });
            }

            // Check for blank line after HR (unless at end of document)
            if i < lines.len() - 1 && Self::count_blank_lines_after(lines, i) == 0 {
                let bq_prefix = ctx.blockquote_prefix_for_blank_line(i);
                warnings.push(LintWarning {
                    rule_name: Some(self.name().to_string()),
                    message: "Missing blank line after horizontal rule".to_string(),
                    line: i + 1,
                    column: lines[i].chars().count() + 1,
                    end_line: i + 1,
                    end_column: lines[i].chars().count() + 2,
                    severity: Severity::Warning,
                    fix: Some(Fix::new(
                        ctx.line_column_byte_range(i + 1, lines[i].len() + 1),
                        format!("{bq_prefix}\n"),
                    )),
                });
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

        for (i, line) in lines.iter().enumerate() {
            // Check for warning about missing blank line before this line
            let warning_before = warnings
                .iter()
                .position(|w| w.line == i + 1 && w.message.contains("before horizontal rule"));

            if let Some(idx) = warning_before {
                let bq_prefix = ctx.blockquote_prefix_for_blank_line(i);
                result.push(bq_prefix);
                warnings.remove(idx);
            }

            result.push((*line).to_string());

            // Check for warning about missing blank line after this line
            let warning_after = warnings
                .iter()
                .position(|w| w.line == i + 1 && w.message.contains("after horizontal rule"));

            if let Some(idx) = warning_after {
                let bq_prefix = ctx.blockquote_prefix_for_blank_line(i);
                result.push(bq_prefix);
                warnings.remove(idx);
            }
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
