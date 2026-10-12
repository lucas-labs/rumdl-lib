use crate::rule::{FixCapability, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use std::collections::{HashMap, HashSet};

mod md024_config;
use md024_config::MD024Config;

#[derive(Clone, Debug, Default)]
pub struct MD024NoDuplicateHeading {
    config: MD024Config,
}

impl MD024NoDuplicateHeading {
    pub fn new(allow_different_nesting: bool, siblings_only: bool) -> Self {
        Self {
            config: MD024Config {
                allow_different_nesting,
                siblings_only,
                allow_different_link_anchors: true,
            },
        }
    }
}

impl Rule for MD024NoDuplicateHeading {
    fn name(&self) -> &'static str {
        "MD024"
    }

    fn description(&self) -> &'static str {
        "Multiple headings with the same content"
    }

    fn fix_capability(&self) -> FixCapability {
        FixCapability::Unfixable
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        // Early return for empty content
        if ctx.lines.is_empty() {
            return Ok(Vec::new());
        }

        // Dedup key pairs the heading's visible text with its `{#custom-id}` (if any).
        // Using a tuple avoids ambiguity when the text itself contains `#`.
        type HeadingKey<'a> = (&'a str, Option<&'a str>);

        let mut warnings = Vec::new();
        let mut seen_headings: HashSet<HeadingKey<'_>> = HashSet::new();
        let mut seen_headings_per_level: HashMap<u8, HashSet<HeadingKey<'_>>> = HashMap::new();

        // For siblings_only mode, track heading hierarchy
        let mut current_section_path: Vec<(u8, HeadingKey<'_>)> = Vec::new();
        let mut seen_siblings: HashMap<Vec<HeadingKey<'_>>, HashSet<HeadingKey<'_>>> = HashMap::new();

        // Track if we're in a snippet section (MkDocs flavor)

        // Process headings using cached heading information
        for (line_num, line_info) in ctx.lines.iter().enumerate() {
            // A Setext heading is recorded on the last line of its text, and the
            // parsed form carries the whole span its warning covers.
            if line_info.heading.is_some()
                && let Some(parsed) = ctx.heading_on_line(line_num + 1)
            {
                let heading = parsed.heading;
                // Skip empty headings
                if heading.text.is_empty() {
                    continue;
                }

                let heading_key: HeadingKey<'_> = if self.config.allow_different_link_anchors {
                    (heading.text.as_str(), heading.custom_id.as_deref())
                } else {
                    (heading.text.as_str(), None)
                };
                let level = heading.level;

                let is_duplicate = if self.config.siblings_only {
                    // Update the section path based on the current heading level.
                    while current_section_path
                        .last()
                        .is_some_and(|(parent_level, _)| *parent_level >= level)
                    {
                        current_section_path.pop();
                    }
                    let parent_path = current_section_path.iter().map(|(_, key)| *key).collect();
                    let is_duplicate = !seen_siblings.entry(parent_path).or_default().insert(heading_key);
                    current_section_path.push((level, heading_key));
                    is_duplicate
                } else if self.config.allow_different_nesting {
                    !seen_headings_per_level.entry(level).or_default().insert(heading_key)
                } else {
                    !seen_headings.insert(heading_key)
                };

                if !is_duplicate {
                    continue;
                }

                // The range covers the heading text, from its start on the
                // first text line to its end on the last.
                let (start_line, start_col, end_line, end_col) = parsed.text_position_range(ctx);

                warnings.push(LintWarning {
                    rule_name: Some(self.name().to_string()),
                    message: format!("Duplicate heading: '{}'.", heading.text),
                    line: start_line,
                    column: start_col,
                    end_line,
                    end_column: end_col,
                    severity: Severity::Error,
                    fix: None,
                });
            }
        }

        Ok(warnings)
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        // MD024 does not support auto-fixing. Removing duplicate headings is not a safe or meaningful fix.
        Ok(ctx.content.to_string())
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
