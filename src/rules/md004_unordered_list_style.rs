use crate::LintContext;
/// Rule MD004: Use consistent style for unordered list markers
///
/// See [docs/md004.md](../../docs/md004.md) for full documentation, configuration, and examples.
///
/// Enforces that all unordered list items in a Markdown document use the same marker style ("*", "+", or "-") or are consistent with the first marker used, depending on configuration.
///
/// ## Purpose
///
/// Ensures visual and stylistic consistency for unordered lists, making documents easier to read and maintain.
///
/// ## Configuration Options
///
/// The rule supports configuring the required marker style:
/// ```yaml
/// MD004:
///   style: dash      # Options: "dash", "asterisk", "plus", or "consistent" (default)
/// ```
///
/// ## Examples
///
/// ### Correct (with style: dash)
/// ```markdown
/// - Item 1
/// - Item 2
///   - Nested item
/// - Item 3
/// ```
///
/// ### Incorrect (with style: dash)
/// ```markdown
/// * Item 1
/// - Item 2
/// + Item 3
/// ```
///
/// ## Behavior
///
/// - Checks each unordered list item for its marker character.
/// - In "consistent" mode, the most prevalent marker sets the style for the document (in case of tie, prefers dash).
/// - Skips code blocks and front matter.
/// - Reports a warning if a list item uses a different marker than the configured or detected style.
///
/// ## Fix Behavior
///
/// - Rewrites all unordered list markers to match the configured or detected style.
/// - Preserves indentation and content after the marker.
///
/// ## Rationale
///
/// Consistent list markers improve readability and reduce distraction, especially in large documents or when collaborating with others. This rule helps enforce a uniform style across all unordered lists.
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};

mod md004_config;
use md004_config::MD004Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnorderedListStyle {
    Asterisk, // "*"
    Plus,     // "+"
    Dash,     // "-"
    #[default]
    Consistent, // Use the first marker in a file consistently
    Sublist,  // Each nesting level uses a different marker (*, +, -, cycling)
}

/// Rule MD004: Unordered list style
#[derive(Clone, Default)]
pub struct MD004UnorderedListStyle {
    config: MD004Config,
}

impl MD004UnorderedListStyle {
    pub fn new(style: UnorderedListStyle) -> Self {
        Self {
            config: MD004Config { style },
        }
    }

    /// Count marker prevalence across all unordered list items in the document
    /// Returns the most prevalent marker character, preferring dash in case of ties
    /// (and dash for a document with no unordered list items).
    fn count_marker_prevalence(&self, ctx: &crate::lint_context::LintContext) -> char {
        let mut asterisk_count = 0;
        let mut dash_count = 0;
        let mut plus_count = 0;

        for list_block in ctx.parsed_list_blocks() {
            for list_item in list_block.items() {
                if !list_item.is_ordered()
                    && let Some(marker) = list_item.marker_char()
                {
                    // Skip (rather than abort the whole count via `?`) on an
                    // empty marker, mirroring the guard in check(); aborting
                    // would return None and suppress all consistency warnings.
                    match marker {
                        '*' => asterisk_count += 1,
                        '-' => dash_count += 1,
                        '+' => plus_count += 1,
                        _ => {}
                    }
                }
            }
        }

        // Use the most prevalent marker as the target style
        // In case of a tie, prefer dash (most common, GitHub default)
        if dash_count >= asterisk_count && dash_count >= plus_count {
            '-'
        } else if asterisk_count >= plus_count {
            '*'
        } else {
            '+'
        }
    }
}

impl Rule for MD004UnorderedListStyle {
    fn name(&self) -> &'static str {
        "MD004"
    }

    fn description(&self) -> &'static str {
        "Use consistent style for unordered list markers"
    }

    fn check(&self, ctx: &LintContext) -> LintResult {
        // Early returns for performance
        if ctx.content.is_empty() {
            return Ok(Vec::new());
        }

        // Quick check for any list markers before processing
        if !ctx.likely_has_lists() {
            return Ok(Vec::new());
        }

        let mut warnings = Vec::new();

        // For consistent mode, count occurrences of each marker (prevalence-based approach)
        let target_marker_for_consistent = if self.config.style == UnorderedListStyle::Consistent {
            Some(self.count_marker_prevalence(ctx))
        } else {
            None
        };

        // Use centralized list blocks for better performance and accuracy
        for list_block in ctx.parsed_list_blocks() {
            // Check each list item in this block
            // We need to check individual items even in mixed lists (ordered with nested unordered)
            for list_item in list_block.items() {
                let _line_info = list_item.line_info();
                // Skip lines inside PyMdown blocks

                // Skip ordered list items - we only care about unordered ones
                if list_item.is_ordered() {
                    continue;
                }

                // Get the marker character. The parser populates a non-empty
                // marker for unordered items, but guard defensively so a
                // future parse path producing an empty marker cannot panic.
                let Some(marker) = list_item.marker_char() else {
                    continue;
                };

                // Calculate offset for the marker position
                let offset = list_item.marker_byte_offset();

                match self.config.style {
                    UnorderedListStyle::Consistent => {
                        // For consistent mode, check against the most prevalent marker
                        if let Some(target) = target_marker_for_consistent
                            && marker != target
                        {
                            let (line, col) = ctx.offset_to_line_col(offset);
                            warnings.push(LintWarning {
                                line,
                                column: col,
                                end_line: line,
                                end_column: col + 1,
                                message: format!("List marker '{marker}' does not match expected style '{target}'"),
                                severity: Severity::Warning,
                                rule_name: Some(self.name().to_string()),
                                fix: Some(Fix::new(offset..offset + 1, target.to_string())),
                            });
                        }
                    }
                    UnorderedListStyle::Sublist => {
                        // Calculate expected marker based on indentation level
                        // Each 2 spaces of indentation represents a nesting level
                        let nesting_level = list_item.marker_column() / 2;
                        let expected_marker = match nesting_level % 3 {
                            0 => '*',
                            1 => '+',
                            2 => '-',
                            _ => {
                                // This should never happen as % 3 only returns 0, 1, or 2
                                // but fallback to asterisk for safety
                                '*'
                            }
                        };
                        if marker != expected_marker {
                            let (line, col) = ctx.offset_to_line_col(offset);
                            warnings.push(LintWarning {
                                        line,
                                        column: col,
                                        end_line: line,
                                        end_column: col + 1,
                                        message: format!(
                                            "List marker '{marker}' does not match expected style '{expected_marker}' for nesting level {nesting_level}"
                                        ),
                                        severity: Severity::Warning,
                                        rule_name: Some(self.name().to_string()),
                                        fix: Some(Fix::new(offset..offset + 1, expected_marker.to_string())),
                                    });
                        }
                    }
                    _ => {
                        // Handle specific style requirements (asterisk, dash, plus)
                        let target_marker = match self.config.style {
                            UnorderedListStyle::Asterisk => '*',
                            UnorderedListStyle::Dash => '-',
                            UnorderedListStyle::Plus => '+',
                            UnorderedListStyle::Consistent | UnorderedListStyle::Sublist => {
                                // These cases are handled separately above
                                // but fallback to asterisk for safety
                                '*'
                            }
                        };
                        if marker != target_marker {
                            let (line, col) = ctx.offset_to_line_col(offset);
                            warnings.push(LintWarning {
                                line,
                                column: col,
                                end_line: line,
                                end_column: col + 1,
                                message: format!(
                                    "List marker '{marker}' does not match expected style '{target_marker}'"
                                ),
                                severity: Severity::Warning,
                                rule_name: Some(self.name().to_string()),
                                fix: Some(Fix::new(offset..offset + 1, target_marker.to_string())),
                            });
                        }
                    }
                }
            }
        }

        Ok(warnings)
    }

    fn fix(&self, ctx: &LintContext) -> Result<String, LintError> {
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
        RuleCategory::List
    }

    /// Check if this rule should be skipped
    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        ctx.content.is_empty() || !ctx.likely_has_lists()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
