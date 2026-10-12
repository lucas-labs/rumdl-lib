/// Rule MD069: No duplicate list markers
///
/// See [docs/md069.md](../../docs/md069.md) for full documentation, configuration, and examples.
///
/// This rule detects duplicate list markers that typically occur from copy-paste
/// with editor auto-list-continuation. For example:
///
/// ```markdown
/// - - duplicate marker (accidental)
/// ```
///
/// Should be:
///
/// ```markdown
/// - duplicate marker
/// ```
///
/// CommonMark parses `- - text` as a valid nested list, but this pattern is
/// never intentionally used in practice. Intentional nested lists use indentation:
///
/// ```markdown
/// - Parent
///   - Child
/// ```
use crate::filtered_lines::FilteredLinesExt;
use crate::lint_context::is_horizontal_rule_line;
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::range_utils::byte_to_char_count;
use regex::Regex;
use std::sync::LazyLock;

/// Pattern: start of line, optional whitespace, list marker, space(s), another list marker, space(s)
/// Captures: (indent)(first_marker)(spaces)(second_marker)(trailing_spaces)
static DUPLICATE_MARKER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)([*+-])(\s+)([*+-])(\s+)").unwrap());

#[derive(Clone, Default)]
pub struct MD069NoDuplicateListMarkers;

impl MD069NoDuplicateListMarkers {
    pub fn new() -> Self {
        Self
    }
}

impl Rule for MD069NoDuplicateListMarkers {
    fn name(&self) -> &'static str {
        "MD069"
    }

    fn description(&self) -> &'static str {
        "Duplicate list markers"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = Vec::new();

        for filtered_line in ctx
            .filtered_lines()
            .skip_front_matter()
            .skip_code_blocks()
            .skip_html_blocks()
            .skip_html_comments()
            .skip_jsx_expressions()
            .skip_mdx_comments()
            .skip_obsidian_comments()
        {
            let line_num = filtered_line.line_num;
            let line = filtered_line.content;

            if let Some(caps) = DUPLICATE_MARKER_REGEX.captures(line) {
                // Skip horizontal rules (e.g., "* * *", "- - -", "_ _ _")
                // These are valid thematic breaks, not duplicate list markers
                if is_horizontal_rule_line(line) {
                    continue;
                }

                let indent = caps.get(1).map_or("", |m| m.as_str());
                let first_marker = caps.get(2).map_or("", |m| m.as_str());
                let spaces_after_first = caps.get(3).map_or("", |m| m.as_str());
                let second_marker = caps.get(4).map_or("", |m| m.as_str());
                let spaces_after_second = caps.get(5).map_or("", |m| m.as_str());

                // Calculate the full match length
                let match_len = indent.len()
                    + first_marker.len()
                    + spaces_after_first.len()
                    + second_marker.len()
                    + spaces_after_second.len();

                // Get the rest of the line after the duplicate markers
                let rest = &line[match_len..];

                // Build the fixed line: indent + second_marker + single space + rest
                let fixed_line = format!("{indent}{second_marker} {rest}");

                // Calculate byte range for the fix
                let line_start = ctx.line_start_byte(line_num).unwrap_or(0);
                let line_end = ctx.line_start_byte(line_num + 1).unwrap_or(ctx.content.len());

                // Clamp line_end to content length
                let line_end = line_end.min(ctx.content.len());

                // Preserve newline if present
                let has_newline = line_end > line_start && ctx.content[line_start..line_end].ends_with('\n');
                let replacement = if has_newline {
                    format!("{fixed_line}\n")
                } else {
                    fixed_line.clone()
                };

                warnings.push(LintWarning {
                    rule_name: Some(self.name().to_string()),
                    message: format!(
                        "Duplicate list marker '{first_marker} {second_marker}' - likely copy-paste error"
                    ),
                    line: line_num,
                    column: byte_to_char_count(line, indent.len()),
                    end_line: line_num,
                    end_column: byte_to_char_count(line, match_len),
                    severity: Severity::Warning,
                    fix: Some(Fix::new(line_start..line_end, replacement)),
                });
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

    fn category(&self) -> RuleCategory {
        RuleCategory::List
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Fast check: if no list markers exist at all, skip
        !ctx.content.contains('-') && !ctx.content.contains('*') && !ctx.content.contains('+')
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
