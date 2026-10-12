use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};

/// Rule MD071: Blank line after frontmatter
///
/// Ensures there is a blank line after YAML/TOML/JSON frontmatter.
/// This improves readability and prevents issues with some markdown parsers.
///
/// See [docs/md071.md](../../docs/md071.md) for full documentation.
#[derive(Clone, Default)]
pub struct MD071BlankLineAfterFrontmatter;

impl MD071BlankLineAfterFrontmatter {}

impl Rule for MD071BlankLineAfterFrontmatter {
    fn name(&self) -> &'static str {
        "MD071"
    }

    fn description(&self) -> &'static str {
        "Blank line after frontmatter"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let content = ctx.content;
        let mut warnings = Vec::new();

        if content.is_empty() {
            return Ok(warnings);
        }

        let fm_end_line = ctx.front_matter_end_line();
        if fm_end_line == 0 {
            // No frontmatter
            return Ok(warnings);
        }

        let lines = ctx.raw_lines();

        // fm_end_line is 1-indexed, so the line after frontmatter is at index fm_end_line
        if let Some(next_line) = lines.get(fm_end_line)
            && !next_line.trim().is_empty()
        {
            // Missing blank line after frontmatter
            let end_col = lines.get(fm_end_line - 1).map_or(1, |l| l.chars().count() + 1);
            warnings.push(LintWarning {
                rule_name: Some(self.name().to_string()),
                message: "Missing blank line after frontmatter".to_string(),
                line: fm_end_line, // Report on the closing delimiter line
                column: 1,
                end_line: fm_end_line,
                end_column: end_col,
                severity: Severity::Warning,
                fix: Some(Fix::new(
                    ctx.line_column_byte_range(fm_end_line, end_col),
                    "\n".to_string(),
                )),
            });
        }

        Ok(warnings)
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        let content = ctx.content;
        let warnings = self.check(ctx)?;
        let warnings = warnings;

        if warnings.is_empty() {
            return Ok(content.to_string());
        }

        let fm_end_line = ctx.front_matter_end_line();
        if fm_end_line == 0 {
            return Ok(content.to_string());
        }

        // Check if original content ended with newline
        let had_trailing_newline = content.ends_with('\n');

        let lines = ctx.raw_lines();
        let mut result = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            result.push((*line).to_string());

            // Insert blank line after frontmatter closing delimiter (index fm_end_line - 1)
            if i == fm_end_line - 1
                && let Some(next_line) = lines.get(i + 1)
                && !next_line.trim().is_empty()
            {
                result.push(String::new());
            }
        }

        // `raw_lines` holds no entry after the final newline, so the file's own
        // final newline is restored after the join, even when the last line is
        // blank.
        let mut fixed = result.join("\n");
        if had_trailing_newline {
            fixed.push('\n');
        }

        Ok(fixed)
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::FrontMatter
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        ctx.content.is_empty() || !ctx.content.starts_with("---") && !ctx.content.starts_with("+++")
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
