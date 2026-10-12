use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};

/// Rule MD047: File should end with a single newline
///
/// See [docs/md047.md](../../docs/md047.md) for full documentation, configuration, and examples.

#[derive(Debug, Default, Clone)]
pub struct MD047SingleTrailingNewline;

impl Rule for MD047SingleTrailingNewline {
    fn name(&self) -> &'static str {
        "MD047"
    }

    fn description(&self) -> &'static str {
        "Files should end with a single newline character"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Whitespace
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Skip empty files - they don't need trailing newlines
        ctx.content.is_empty()
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let content = ctx.content;
        let mut warnings = Vec::new();

        // Empty content is fine
        if content.is_empty() {
            return Ok(warnings);
        }

        // Holds for LF and CRLF alike; a CRLF document conforms the `\n` fix
        // below to `\r\n` where warnings are collected.
        let has_trailing_newline = content.ends_with('\n');

        // Check for missing trailing newline
        if !has_trailing_newline {
            let lines = &ctx.lines;
            let last_line_num = lines.len();
            let last_line_content = lines.last().map_or("", |s| s.content(content));

            // Calculate precise character range for the end of file
            // For missing newline, highlight the end of the last line
            let last_line_chars = last_line_content.chars().count();
            let (start_line, start_col, end_line, end_col) =
                (last_line_num, last_line_chars + 1, last_line_num, last_line_chars + 1);

            warnings.push(LintWarning {
                rule_name: Some(self.name().to_string()),
                message: String::from("File should end with a single newline character"),
                line: start_line,
                column: start_col,
                end_line,
                end_column: end_col,
                severity: Severity::Warning,
                fix: Some(Fix::new(content.len()..content.len(), "\n".to_string())),
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
        crate::utils::fix_utils::apply_warning_fixes(ctx.content, &warnings).map_err(LintError::InvalidInput)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
