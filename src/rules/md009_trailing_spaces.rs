use crate::filtered_lines::FilteredLinesExt;
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::hard_break::br_produces_useful_break;
use crate::utils::range_utils::calculate_trailing_range;
use crate::utils::regex_cache::{ORDERED_LIST_MARKER_REGEX, UNORDERED_LIST_MARKER_REGEX};

mod md009_config;
use md009_config::MD009Config;

#[derive(Debug, Clone, Default)]
pub struct MD009TrailingSpaces {
    config: MD009Config,
}

impl MD009TrailingSpaces {
    pub fn new(br_spaces: usize, strict: bool) -> Self {
        Self {
            config: MD009Config {
                br_spaces: crate::types::BrSpaces::new(br_spaces),
                strict,
                list_item_empty_lines: false,
            },
        }
    }

    fn count_trailing_spaces(line: &str) -> usize {
        line.chars().rev().take_while(|&c| c == ' ').count()
    }

    fn count_trailing_spaces_ascii(line: &str) -> usize {
        line.as_bytes().iter().rev().take_while(|&&b| b == b' ').count()
    }

    /// Count all trailing whitespace characters (ASCII and Unicode).
    /// This includes U+2000..U+200A (various Unicode spaces), ASCII space, tab, etc.
    fn count_trailing_whitespace(line: &str) -> usize {
        line.chars().rev().take_while(|c| c.is_whitespace()).count()
    }

    fn trimmed_len_ascii_whitespace(line: &str) -> usize {
        line.as_bytes()
            .iter()
            .rposition(|b| !b.is_ascii_whitespace())
            .map_or(0, |idx| idx + 1)
    }

    fn calculate_trailing_range_ascii(
        line: usize,
        line_len: usize,
        content_end: usize,
    ) -> (usize, usize, usize, usize) {
        // Return 1-indexed columns to match calculate_trailing_range behavior
        (line, content_end + 1, line, line_len + 1)
    }

    fn is_empty_list_item_line(line: &str, prev_line: Option<&str>) -> bool {
        // A line is an empty list item line if:
        // 1. It's blank or only contains spaces
        // 2. The previous line is a list item
        if !line.trim().is_empty() {
            return false;
        }

        if let Some(prev) = prev_line {
            // Check for unordered list markers (*, -, +) with proper formatting
            UNORDERED_LIST_MARKER_REGEX.is_match(prev) || ORDERED_LIST_MARKER_REGEX.is_match(prev)
        } else {
            false
        }
    }
}

impl Rule for MD009TrailingSpaces {
    fn name(&self) -> &'static str {
        "MD009"
    }

    fn description(&self) -> &'static str {
        "Trailing spaces should be removed"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let content = ctx.content;

        let mut warnings = Vec::new();

        // Use pre-computed lines (needed for looking back at prev_line)
        let lines = ctx.raw_lines();

        let mut filtered = ctx.filtered_lines().skip_front_matter().skip_pymdown_blocks();
        if !self.config.strict {
            filtered = filtered.skip_code_blocks();
        }

        for filtered_line in filtered {
            let line_num = filtered_line.line_num - 1;
            let line = filtered_line.content;

            let line_is_ascii = line.is_ascii();
            // Count ASCII trailing spaces for br_spaces comparison
            let trailing_ascii_spaces = if line_is_ascii {
                Self::count_trailing_spaces_ascii(line)
            } else {
                Self::count_trailing_spaces(line)
            };
            // For non-ASCII lines, also count all trailing whitespace (including Unicode)
            // to ensure the fix range covers everything that trim_end() removes
            let trailing_all_whitespace = if line_is_ascii {
                trailing_ascii_spaces
            } else {
                Self::count_trailing_whitespace(line)
            };

            // Skip if no trailing whitespace
            if trailing_all_whitespace == 0 {
                continue;
            }

            // Handle empty lines
            let trimmed_len = if line_is_ascii {
                Self::trimmed_len_ascii_whitespace(line)
            } else {
                line.trim_end().len()
            };
            if trimmed_len == 0 {
                if trailing_all_whitespace > 0 {
                    // Check if this is an empty list item line and config allows it
                    let prev_line = if line_num > 0 { Some(lines[line_num - 1]) } else { None };
                    if self.config.list_item_empty_lines && Self::is_empty_list_item_line(line, prev_line) {
                        continue;
                    }

                    // Calculate precise character range for all trailing whitespace on empty line
                    let (start_line, start_col, end_line, end_col) = if line_is_ascii {
                        Self::calculate_trailing_range_ascii(line_num + 1, line.len(), 0)
                    } else {
                        calculate_trailing_range(line_num + 1, line, 0)
                    };
                    let line_start = *ctx.line_offsets.get(line_num).unwrap_or(&0);
                    let fix_range = if line_is_ascii {
                        line_start..line_start + line.len()
                    } else {
                        ctx.line_column_byte_range_with_length(line_num + 1, 1, line.chars().count())
                    };

                    warnings.push(LintWarning {
                        rule_name: Some(self.name().to_string()),
                        line: start_line,
                        column: start_col,
                        end_line,
                        end_column: end_col,
                        message: "Empty line has trailing spaces".to_string(),
                        severity: Severity::Warning,
                        fix: Some(Fix::new(fix_range, String::new())),
                    });
                }
                continue;
            }

            // Check if it's a valid line break (only ASCII spaces count for br_spaces).
            // The br_spaces exception applies when the trailing whitespace produces a `<br>`;
            // a br_spaces below 2 turns the exception off, so nothing matches it.
            // In non-strict mode we accept br_spaces anywhere that isn't the literal final
            // line of the document. In strict mode (markdownlint parity) we additionally
            // require the line to be a non-last line of a paragraph: structural lines
            // (headings, fences, setext underlines, horizontal rules, ...) and last lines
            // of paragraphs (single-line paragraphs, lines before blanks, list-item ends,
            // ...) all get flagged because the `<br>` they would emit is unobservable.
            let is_truly_last_line = line_num == lines.len() - 1 && !content.ends_with('\n');
            let has_only_ascii_trailing = trailing_ascii_spaces == trailing_all_whitespace;
            let matches_br_spaces = self.config.br_spaces.line_break() == Some(trailing_ascii_spaces);
            if !is_truly_last_line && has_only_ascii_trailing && matches_br_spaces {
                let allow = if self.config.strict {
                    br_produces_useful_break(ctx, line_num)
                } else {
                    true
                };
                if allow {
                    continue;
                }
            }

            // Check if this is an empty blockquote line ("> " or ">> " etc)
            // These are allowed by MD028 to have a single trailing ASCII space
            let trimmed = if line_is_ascii {
                &line[..trimmed_len]
            } else {
                line.trim_end()
            };
            let is_empty_blockquote_with_space = trimmed.chars().all(|c| c == '>' || c == ' ' || c == '\t')
                && trimmed.contains('>')
                && has_only_ascii_trailing
                && trailing_ascii_spaces == 1;

            if is_empty_blockquote_with_space {
                continue; // Allow single trailing ASCII space for empty blockquote lines
            }
            // Calculate precise character range for all trailing whitespace
            let (start_line, start_col, end_line, end_col) = if line_is_ascii {
                Self::calculate_trailing_range_ascii(line_num + 1, line.len(), trimmed.len())
            } else {
                calculate_trailing_range(line_num + 1, line, trimmed.len())
            };
            let line_start = *ctx.line_offsets.get(line_num).unwrap_or(&0);
            let fix_range = if line_is_ascii {
                let start = line_start + trimmed.len();
                let end = start + trailing_all_whitespace;
                start..end
            } else {
                ctx.line_column_byte_range_with_length(
                    line_num + 1,
                    trimmed.chars().count() + 1,
                    trailing_all_whitespace,
                )
            };

            warnings.push(LintWarning {
                rule_name: Some(self.name().to_string()),
                line: start_line,
                column: start_col,
                end_line,
                end_column: end_col,
                message: if trailing_all_whitespace == 1 {
                    "Trailing space found".to_string()
                } else {
                    format!("{trailing_all_whitespace} trailing spaces found")
                },
                severity: Severity::Warning,
                fix: Some(Fix::new(fix_range, String::new())),
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

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Skip if content is empty.
        // We cannot skip based on ASCII-space-only check because Unicode whitespace
        // characters (e.g., U+2000 EN QUAD) also count as trailing whitespace.
        // The per-line is_ascii fast path in check()/fix() handles performance.
        ctx.content.is_empty()
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Whitespace
    }
}
