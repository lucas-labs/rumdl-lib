use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::rules::front_matter_utils::FrontMatterUtils;
use crate::rules::heading_utils::HeadingStyle;
use crate::rules::heading_utils::HeadingUtils;
use crate::utils::range_utils::calculate_heading_range;
use regex::Regex;

/// Rule MD001: Heading levels should only increment by one level at a time
///
/// See [docs/md001.md](../../docs/md001.md) for full documentation, configuration, and examples.
///
/// This rule enforces a fundamental principle of document structure: heading levels
/// should increase by exactly one level at a time to maintain a proper document hierarchy.
///
/// ## Purpose
///
/// Proper heading structure creates a logical document outline and improves:
/// - Readability for humans
/// - Accessibility for screen readers
/// - Navigation in rendered documents
/// - Automatic generation of tables of contents
///
/// ## Examples
///
/// ### Correct Heading Structure
/// ```markdown
/// # Heading 1
/// ## Heading 2
/// ### Heading 3
/// ## Another Heading 2
/// ```
///
/// ### Incorrect Heading Structure
/// ```markdown
/// # Heading 1
/// ### Heading 3 (skips level 2)
/// #### Heading 4
/// ```
///
/// ## Behavior
///
/// This rule:
/// - Tracks the heading level throughout the document
/// - Validates that each new heading is at most one level deeper than the previous heading
/// - Allows heading levels to decrease by any amount (e.g., going from ### to #)
/// - Works with both ATX (`#`) and Setext (underlined) heading styles
///
/// ## Fix Behavior
///
/// When applying automatic fixes, this rule:
/// - Changes the level of non-compliant headings to be one level deeper than the previous heading
/// - Preserves the original heading style (ATX or Setext)
/// - Maintains indentation and other formatting
///
/// ## Rationale
///
/// Skipping heading levels (e.g., from `h1` to `h3`) can confuse readers and screen readers
/// by creating gaps in the document structure. Consistent heading increments create a proper
/// hierarchical outline essential for well-structured documents.
///
/// ## Front Matter Title Support
///
/// When `front_matter_title` is enabled (default: true), this rule recognizes a `title:` field
/// in YAML/TOML frontmatter as an implicit level-1 heading. This allows documents like:
///
/// ```markdown
/// ---
/// title: My Document
/// ---
///
/// ## First Section
/// ```
///
/// Without triggering a warning about skipping from H1 to H2, since the frontmatter title
/// counts as the H1.
///
#[derive(Debug, Clone)]
pub struct MD001HeadingIncrement {
    /// Whether to treat frontmatter title field as an implicit H1
    pub(crate) front_matter_title: bool,
    /// Optional regex pattern to match custom title fields in frontmatter
    pub(crate) front_matter_title_pattern: Option<Regex>,
}

impl Default for MD001HeadingIncrement {
    fn default() -> Self {
        Self {
            front_matter_title: true,
            front_matter_title_pattern: None,
        }
    }
}

/// Result of computing the fix for a single heading
struct HeadingFixInfo {
    /// The level after fixing (may equal original if no fix needed)
    fixed_level: usize,
    /// The heading style to use for the replacement
    style: HeadingStyle,
    /// Whether this heading needs a fix
    needs_fix: bool,
}

impl MD001HeadingIncrement {
    /// Create a new instance with specified settings
    pub fn new(front_matter_title: bool) -> Self {
        Self {
            front_matter_title,
            front_matter_title_pattern: None,
        }
    }

    /// Check if the document has a front matter title field
    fn has_front_matter_title(&self, content: &str) -> bool {
        if !self.front_matter_title {
            return false;
        }

        // If we have a custom pattern, use it to search front matter content
        if let Some(ref pattern) = self.front_matter_title_pattern {
            let front_matter_lines = FrontMatterUtils::extract_front_matter(content);
            for line in front_matter_lines {
                if pattern.is_match(line) {
                    return true;
                }
            }
            return false;
        }

        // Default behavior: check for "title:" field
        FrontMatterUtils::has_front_matter_field(content, "title:")
    }

    /// Single source of truth for heading level computation and style mapping.
    ///
    /// Returns `(HeadingFixInfo, new_prev_level)`. Both `check()` and `fix()` call
    /// this, making it structurally impossible for them to diverge.
    fn compute_heading_fix(
        prev_level: Option<usize>,
        heading: &crate::lint_context::HeadingInfo,
    ) -> (HeadingFixInfo, Option<usize>) {
        let level = heading.level as usize;

        let (fixed_level, needs_fix) = if let Some(prev) = prev_level
            && level > prev + 1
        {
            (prev + 1, true)
        } else {
            (level, false)
        };

        // Map heading style, adjusting Setext variant based on the fixed level
        let style = match heading.style {
            crate::lint_context::HeadingStyle::ATX => HeadingStyle::Atx,
            crate::lint_context::HeadingStyle::Setext1 | crate::lint_context::HeadingStyle::Setext2 => {
                if fixed_level == 1 {
                    HeadingStyle::Setext1
                } else {
                    HeadingStyle::Setext2
                }
            }
        };

        let info = HeadingFixInfo {
            fixed_level,
            style,
            needs_fix,
        };
        (info, Some(fixed_level))
    }
}

impl Rule for MD001HeadingIncrement {
    fn name(&self) -> &'static str {
        "MD001"
    }

    fn description(&self) -> &'static str {
        "Heading levels should only increment by one level at a time"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = Vec::new();

        let mut prev_level: Option<usize> = if self.has_front_matter_title(ctx.content) {
            Some(1)
        } else {
            None
        };

        for valid_heading in ctx.valid_headings() {
            let heading = valid_heading.heading;
            let line_info = valid_heading.line_info;

            let level = heading.level as usize;

            let (fix_info, new_prev) = Self::compute_heading_fix(prev_level, heading);
            prev_level = new_prev;

            if fix_info.needs_fix {
                let line_content = line_info.content(ctx.content);
                let original_indent = &line_content[..line_info.indent];
                let replacement =
                    HeadingUtils::convert_heading_style(&heading.raw_text, fix_info.fixed_level as u32, fix_info.style);

                let (start_line, start_col, end_line, end_col) =
                    calculate_heading_range(valid_heading.first_line_num(), valid_heading.line_num, line_content);

                warnings.push(LintWarning {
                    rule_name: Some(self.name().to_string()),
                    line: start_line,
                    column: start_col,
                    end_line,
                    end_column: end_col,
                    message: format!(
                        "Expected heading level {}, but found heading level {}",
                        fix_info.fixed_level, level
                    ),
                    severity: Severity::Error,
                    fix: Some(Fix::new(
                        ctx.line_content_byte_range(valid_heading.line_num),
                        format!("{original_indent}{replacement}"),
                    )),
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
        RuleCategory::Heading
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Fast path: check if document likely has headings
        if ctx.content.is_empty() || !ctx.likely_has_headings() {
            return true;
        }
        // Verify valid headings actually exist
        !ctx.has_valid_headings()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
