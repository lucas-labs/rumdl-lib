use crate::filtered_lines::FilteredLinesExt;
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::rules::code_fence_utils::CodeFenceStyle;
use crate::utils::range_utils::calculate_match_range;

mod md048_config;
use md048_config::MD048Config;

/// Parsed fence marker candidate on a single line.
#[derive(Debug, Clone, Copy)]
struct FenceMarker<'a> {
    /// Fence character (` or ~).
    fence_char: char,
    /// Length of the contiguous fence run.
    fence_len: usize,
    /// Byte index where the fence run starts.
    fence_start: usize,
    /// Remaining text after the fence run.
    rest: &'a str,
}

/// Parse a candidate fence marker line.
///
/// CommonMark only recognizes fenced code block markers when indented by at most
/// three spaces (outside container contexts). This parser enforces that bound and
/// returns the marker run and trailing text for further opening/closing checks.
#[inline]
fn parse_fence_marker(line: &str) -> Option<FenceMarker<'_>> {
    let bytes = line.as_bytes();
    let mut pos = 0usize;
    while pos < bytes.len() && bytes[pos] == b' ' {
        pos += 1;
    }
    if pos > 3 {
        return None;
    }

    let fence_char = match bytes.get(pos).copied() {
        Some(b'`') => '`',
        Some(b'~') => '~',
        _ => return None,
    };

    let marker = if fence_char == '`' { b'`' } else { b'~' };
    let mut end = pos;
    while end < bytes.len() && bytes[end] == marker {
        end += 1;
    }
    let fence_len = end - pos;
    if fence_len < 3 {
        return None;
    }

    Some(FenceMarker {
        fence_char,
        fence_len,
        fence_start: pos,
        rest: &line[end..],
    })
}

#[inline]
fn is_closing_fence(marker: FenceMarker<'_>, opening_fence_char: char, opening_fence_len: usize) -> bool {
    marker.fence_char == opening_fence_char && marker.fence_len >= opening_fence_len && marker.rest.trim().is_empty()
}

#[inline]
fn needs_fence_conversion(fence_char: char, target_style: CodeFenceStyle) -> bool {
    (fence_char == '`' && target_style == CodeFenceStyle::Tilde)
        || (fence_char == '~' && target_style == CodeFenceStyle::Backtick)
}

/// Rule MD048: Code fence style
///
/// See [docs/md048.md](../../docs/md048.md) for full documentation, configuration, and examples.
#[derive(Clone)]
pub struct MD048CodeFenceStyle {
    config: MD048Config,
}

impl MD048CodeFenceStyle {
    pub fn new(style: CodeFenceStyle) -> Self {
        Self {
            config: MD048Config { style },
        }
    }

    /// Resolve the fence style MD048 should converge on.
    ///
    /// A Gherkin Doc String is only ever a backtick fence, so a tilde fence can
    /// never be one, and a configuration demanding tilde fences cannot be
    /// satisfied in this flavor. MDG therefore always converges on backtick:
    /// `consistent` resolves to backtick rather than to whichever marker
    /// happens to be more common, and an explicit `tilde` is not adopted.
    fn effective_target_style(&self, ctx: &crate::lint_context::LintContext) -> CodeFenceStyle {
        match self.config.style {
            CodeFenceStyle::Consistent => self.detect_style(ctx).unwrap_or(CodeFenceStyle::Backtick),
            style => style,
        }
    }

    fn detect_style(&self, ctx: &crate::lint_context::LintContext) -> Option<CodeFenceStyle> {
        // Count occurrences of each fence style (prevalence-based approach)
        let mut backtick_count = 0;
        let mut tilde_count = 0;
        let mut in_code_block = false;
        let mut opening_fence_char = '`';
        let mut opening_fence_len = 0usize;

        for filtered_line in ctx.filtered_lines().skip_front_matter() {
            let _i = filtered_line.line_num - 1;
            let line = filtered_line.content;
            // Skip lines inside Azure DevOps colon code fences — they are
            // opaque content and must not influence backtick/tilde style detection.

            // Skip lines inside MyST colon directives — they are structural
            // containers, not code fences.

            let Some(marker) = parse_fence_marker(line) else {
                continue;
            };

            // Skip MyST backtick directives (info string starts with {name})

            if !in_code_block {
                // Opening fence - count it
                if marker.fence_char == '`' {
                    backtick_count += 1;
                } else {
                    tilde_count += 1;
                }
                in_code_block = true;
                opening_fence_char = marker.fence_char;
                opening_fence_len = marker.fence_len;
            } else if is_closing_fence(marker, opening_fence_char, opening_fence_len) {
                in_code_block = false;
            }
        }

        // Use the most prevalent style
        // In case of a tie, prefer backticks (more common, widely supported)
        if backtick_count >= tilde_count && backtick_count > 0 {
            Some(CodeFenceStyle::Backtick)
        } else if tilde_count > 0 {
            Some(CodeFenceStyle::Tilde)
        } else {
            None
        }
    }
}

/// Find the maximum fence length using `target_char` within the body of a fenced block.
///
/// Scans from the line after `opening_line` until the matching closing fence
/// (same `opening_char`, length >= `opening_fence_len`, no trailing content).
/// Returns the maximum number of consecutive `target_char` characters found at
/// the start of any interior bare fence line (after stripping leading whitespace).
///
/// This is used to compute the minimum fence length needed when converting a
/// fence from one style to another so that nesting remains unambiguous.
/// For example, converting a `~~~` outer fence that contains ```` ``` ```` inner
/// fences to backtick style requires using ```` ```` ```` (4 backticks) so that
/// the inner 3-backtick bare fences cannot inadvertently close the outer block.
///
/// Only bare interior sequences (no trailing content) are counted. Per CommonMark
/// spec section 4.5, a closing fence must be followed only by optional whitespace —
/// lines with info strings (e.g. `` ```rust ``) can never be closing fences, so
/// they never create ambiguity regardless of the outer fence's style.
fn max_inner_fence_length_of_char(
    lines: &[&str],
    opening_line: usize,
    opening_fence_len: usize,
    opening_char: char,
    target_char: char,
) -> usize {
    let mut max_len = 0usize;

    for line in lines.iter().skip(opening_line + 1) {
        let Some(marker) = parse_fence_marker(line) else {
            continue;
        };

        // Stop at the closing fence of the outer block.
        if is_closing_fence(marker, opening_char, opening_fence_len) {
            break;
        }

        // Count only bare sequences (no info string). Lines with info strings
        // can never be closing fences per CommonMark and pose no ambiguity risk.
        if marker.fence_char == target_char && marker.rest.trim().is_empty() {
            max_len = max_len.max(marker.fence_len);
        }
    }

    max_len
}

impl Rule for MD048CodeFenceStyle {
    fn name(&self) -> &'static str {
        "MD048"
    }

    fn description(&self) -> &'static str {
        "Code fence style should be consistent"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::CodeBlock
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = Vec::new();

        let target_style = self.effective_target_style(ctx);

        let lines = ctx.raw_lines();
        let mut in_code_block = false;
        let mut code_block_fence_char = '`';
        let mut code_block_fence_len = 0usize;
        // The fence length to use when writing the converted/lengthened closing fence.
        // May be longer than the original when inner fences require disambiguation by length.
        let mut converted_fence_len = 0usize;
        // True when the opening fence was already the correct style but its length is
        // ambiguous (interior has same-style fences of equal or greater length).
        let mut needs_lengthening = false;

        for filtered_line in ctx.filtered_lines().skip_front_matter() {
            let line_num = filtered_line.line_num - 1;
            let line = filtered_line.content;
            // Skip lines inside Azure DevOps colon code fences.

            // Skip lines inside MyST colon directives.

            let Some(marker) = parse_fence_marker(line) else {
                continue;
            };

            // Skip MyST backtick directives (info string starts with {name})
            let fence_char = marker.fence_char;
            let fence_len = marker.fence_len;

            if !in_code_block {
                in_code_block = true;
                code_block_fence_char = fence_char;
                code_block_fence_len = fence_len;

                let needs_conversion = needs_fence_conversion(fence_char, target_style);

                if needs_conversion {
                    let target_char = if target_style == CodeFenceStyle::Backtick {
                        '`'
                    } else {
                        '~'
                    };

                    // Compute how many target_char characters the converted fence needs.
                    // Must be strictly greater than any inner bare fence of the target style.
                    let prefix = &line[..marker.fence_start];
                    let info = marker.rest;
                    let max_inner = max_inner_fence_length_of_char(lines, line_num, fence_len, fence_char, target_char);
                    converted_fence_len = fence_len.max(max_inner + 1);
                    needs_lengthening = false;

                    let replacement = format!("{prefix}{}{info}", target_char.to_string().repeat(converted_fence_len));

                    let fence_start = marker.fence_start;
                    let fence_end = fence_start + fence_len;
                    let (start_line, start_col, end_line, end_col) =
                        calculate_match_range(line_num + 1, line, fence_start, fence_end - fence_start);

                    warnings.push(LintWarning {
                        rule_name: Some(self.name().to_string()),
                        message: format!(
                            "Code fence style: use {} instead of {}",
                            if target_style == CodeFenceStyle::Backtick {
                                "```"
                            } else {
                                "~~~"
                            },
                            if fence_char == '`' { "```" } else { "~~~" }
                        ),
                        line: start_line,
                        column: start_col,
                        end_line,
                        end_column: end_col,
                        severity: Severity::Warning,
                        fix: Some(Fix::new(
                            ctx.line_column_byte_range_with_length(line_num + 1, 1, line.len()),
                            replacement,
                        )),
                    });
                } else {
                    // No character conversion is required.
                    // Check for fence-length ambiguity:
                    // if the interior contains same-style bare fences of equal or greater
                    // length, the outer fence cannot be distinguished from an inner
                    // closing fence and must be made longer.
                    let prefix = &line[..marker.fence_start];
                    let info = marker.rest;
                    let max_inner = max_inner_fence_length_of_char(lines, line_num, fence_len, fence_char, fence_char);
                    if max_inner >= fence_len {
                        converted_fence_len = max_inner + 1;
                        needs_lengthening = true;

                        let replacement =
                            format!("{prefix}{}{info}", fence_char.to_string().repeat(converted_fence_len));

                        let fence_start = marker.fence_start;
                        let fence_end = fence_start + fence_len;
                        let (start_line, start_col, end_line, end_col) =
                            calculate_match_range(line_num + 1, line, fence_start, fence_end - fence_start);

                        warnings.push(LintWarning {
                            rule_name: Some(self.name().to_string()),
                            message: format!(
                                "Code fence length is ambiguous: outer fence ({fence_len} {}) \
                                 contains interior fence sequences of equal length; \
                                 use {converted_fence_len}",
                                if fence_char == '`' { "backticks" } else { "tildes" },
                            ),
                            line: start_line,
                            column: start_col,
                            end_line,
                            end_column: end_col,
                            severity: Severity::Warning,
                            fix: Some(Fix::new(
                                ctx.line_column_byte_range_with_length(line_num + 1, 1, line.len()),
                                replacement,
                            )),
                        });
                    } else {
                        converted_fence_len = fence_len;
                        needs_lengthening = false;
                    }
                }
            } else {
                // Inside a code block — check if this is the closing fence.
                let is_closing = is_closing_fence(marker, code_block_fence_char, code_block_fence_len);

                if is_closing {
                    let needs_conversion = needs_fence_conversion(fence_char, target_style);

                    if needs_conversion || needs_lengthening {
                        let target_char = if needs_conversion {
                            if target_style == CodeFenceStyle::Backtick {
                                '`'
                            } else {
                                '~'
                            }
                        } else {
                            fence_char
                        };

                        let prefix = &line[..marker.fence_start];
                        let replacement = format!(
                            "{prefix}{}{}",
                            target_char.to_string().repeat(converted_fence_len),
                            marker.rest
                        );

                        let fence_start = marker.fence_start;
                        let fence_end = fence_start + fence_len;
                        let (start_line, start_col, end_line, end_col) =
                            calculate_match_range(line_num + 1, line, fence_start, fence_end - fence_start);

                        let message = if needs_conversion {
                            format!(
                                "Code fence style: use {} instead of {}",
                                if target_style == CodeFenceStyle::Backtick {
                                    "```"
                                } else {
                                    "~~~"
                                },
                                if fence_char == '`' { "```" } else { "~~~" }
                            )
                        } else {
                            format!(
                                "Code fence length is ambiguous: closing fence ({fence_len} {}) \
                                 must match the lengthened outer fence; use {converted_fence_len}",
                                if fence_char == '`' { "backticks" } else { "tildes" },
                            )
                        };

                        warnings.push(LintWarning {
                            rule_name: Some(self.name().to_string()),
                            message,
                            line: start_line,
                            column: start_col,
                            end_line,
                            end_column: end_col,
                            severity: Severity::Warning,
                            fix: Some(Fix::new(
                                ctx.line_column_byte_range_with_length(line_num + 1, 1, line.len()),
                                replacement,
                            )),
                        });
                    }

                    in_code_block = false;
                    code_block_fence_len = 0;
                    converted_fence_len = 0;
                    needs_lengthening = false;
                }
                // Lines inside the block that are not the closing fence are left alone.
            }
        }

        Ok(warnings)
    }

    /// Check if this rule should be skipped for performance
    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Skip if content is empty or has no code fence markers
        ctx.content.is_empty() || (!ctx.likely_has_code() && !ctx.has_char('~'))
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
}
