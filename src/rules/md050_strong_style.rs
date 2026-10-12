use crate::utils::range_utils::calculate_match_range;

use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::rules::strong_style::StrongStyle;
use crate::utils::code_block_utils::StrongSpanDetail;
use crate::utils::skip_context::{compute_html_code_ranges, should_skip_emphasis_span};

/// Convert a StrongSpanDetail to a StrongStyle
fn span_style(span: &StrongSpanDetail) -> StrongStyle {
    if span.is_asterisk {
        StrongStyle::Asterisk
    } else {
        StrongStyle::Underscore
    }
}

mod md050_config;
use md050_config::MD050Config;

/// Rule MD050: Strong style
///
/// See [docs/md050.md](../../docs/md050.md) for full documentation, configuration, and examples.
///
/// This rule is triggered when strong markers (** or __) are used in an inconsistent way.
#[derive(Debug, Default, Clone)]
pub struct MD050StrongStyle {
    config: MD050Config,
}

impl MD050StrongStyle {
    pub fn new(style: StrongStyle) -> Self {
        Self {
            config: MD050Config { style },
        }
    }

    fn detect_style_from_spans(
        &self,
        ctx: &crate::lint_context::LintContext,
        html_tags: &[crate::lint_context::HtmlTag],
        html_code_ranges: &[(usize, usize)],
        spans: &[StrongSpanDetail],
    ) -> Option<StrongStyle> {
        let mut asterisk_count = 0;
        let mut underscore_count = 0;

        for span in spans {
            if should_skip_emphasis_span(ctx, html_tags, html_code_ranges, span.start) {
                continue;
            }

            match span_style(span) {
                StrongStyle::Asterisk => asterisk_count += 1,
                StrongStyle::Underscore => underscore_count += 1,
                StrongStyle::Consistent => {}
            }
        }

        match (asterisk_count, underscore_count) {
            (0, 0) => None,
            (_, 0) => Some(StrongStyle::Asterisk),
            (0, _) => Some(StrongStyle::Underscore),
            // In case of a tie, prefer asterisk (matches CommonMark recommendation)
            (a, u) => {
                if a >= u {
                    Some(StrongStyle::Asterisk)
                } else {
                    Some(StrongStyle::Underscore)
                }
            }
        }
    }
}

impl Rule for MD050StrongStyle {
    fn name(&self) -> &'static str {
        "MD050"
    }

    fn description(&self) -> &'static str {
        "Strong emphasis style should be consistent"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Emphasis
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let content = ctx.content;
        let lines = ctx.raw_lines();

        let mut warnings = Vec::new();

        let spans = &ctx.strong_spans;
        let html_tags = ctx.html_tags();
        let html_code_ranges = compute_html_code_ranges(&html_tags);

        let target_style = match self.config.style {
            StrongStyle::Consistent => self
                .detect_style_from_spans(ctx, &html_tags, &html_code_ranges, spans)
                .unwrap_or(StrongStyle::Asterisk),
            _ => self.config.style,
        };

        for span in spans {
            // Only flag spans that use the wrong style
            if span_style(span) == target_style {
                continue;
            }

            // Skip too-short spans
            if span.end - span.start < 4 {
                continue;
            }

            // Only check skip context for wrong-style spans (the minority)
            if should_skip_emphasis_span(ctx, &html_tags, &html_code_ranges, span.start) {
                continue;
            }

            let (line_num, _col) = ctx.offset_to_line_col(span.start);
            let line_start = ctx.line_start_byte(line_num).unwrap_or(0);
            let line_content = lines.get(line_num - 1).unwrap_or(&"");
            let match_start_in_line = span.start - line_start;
            let match_len = span.end - span.start;

            let inner_text = &content[span.start + 2..span.end - 2];

            // NOTE: Intentional deviation from markdownlint behavior.
            // markdownlint reports two warnings per emphasis (one for opening marker,
            // one for closing marker). We report one warning per emphasis block because:
            // 1. The markers are semantically one unit - you can't fix one without the other
            // 2. Cleaner output - "10 issues" vs "20 issues" for 10 bold words
            // 3. The fix is atomic - replacing the entire emphasis at once
            let message = match target_style {
                StrongStyle::Asterisk => "Strong emphasis should use ** instead of __",
                StrongStyle::Underscore => "Strong emphasis should use __ instead of **",
                StrongStyle::Consistent => "Strong emphasis should use ** instead of __",
            };

            let (start_line, start_col, end_line, end_col) =
                calculate_match_range(line_num, line_content, match_start_in_line, match_len);

            warnings.push(LintWarning {
                rule_name: Some(self.name().to_string()),
                line: start_line,
                column: start_col,
                end_line,
                end_column: end_col,
                message: message.to_string(),
                severity: Severity::Warning,
                fix: Some(Fix::new(
                    span.start..span.end,
                    match target_style {
                        StrongStyle::Asterisk => format!("**{inner_text}**"),
                        StrongStyle::Underscore => format!("__{inner_text}__"),
                        StrongStyle::Consistent => format!("**{inner_text}**"),
                    },
                )),
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

    /// Check if this rule should be skipped
    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Strong uses double markers, but likely_has_emphasis checks for count > 1
        ctx.content.is_empty() || !ctx.likely_has_emphasis()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
