/// Rule MD018: No missing space after ATX heading marker
///
/// See [docs/md018.md](../../docs/md018.md) for full documentation, configuration, and examples.
mod md018_config;

pub(super) use md018_config::MD018Config;

use crate::config::MarkdownFlavor;
use crate::lint_context::{AtxMissingSpace, LineInfo};
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::obsidian_tag::TAG_PATTERN;
use crate::utils::range_utils::{byte_to_char_count, calculate_single_line_range};
use regex::Regex;
use std::sync::LazyLock;

// Emoji and Unicode hashtag patterns
const EMOJI_HASHTAG_PATTERN_STR: &str = r"^#️⃣|^#⃣";
const UNICODE_HASHTAG_PATTERN_STR: &str = r"^#[\u{FE0F}\u{20E3}]";
static EMOJI_HASHTAG_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(EMOJI_HASHTAG_PATTERN_STR).unwrap());
static UNICODE_HASHTAG_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(UNICODE_HASHTAG_PATTERN_STR).unwrap());

// MagicLink issue/PR reference pattern: #123, #10, etc.
// Matches # followed by one or more digits, then either end of string,
// whitespace, or punctuation (not alphanumeric continuation)
const MAGICLINK_REF_PATTERN_STR: &str = r"^#\d+(?:\s|[^a-zA-Z0-9]|$)";
static MAGICLINK_REF_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(MAGICLINK_REF_PATTERN_STR).unwrap());

#[derive(Clone)]
pub struct MD018NoMissingSpaceAtx {
    config: MD018Config,
}

impl Default for MD018NoMissingSpaceAtx {
    fn default() -> Self {
        Self::new()
    }
}

impl MD018NoMissingSpaceAtx {
    pub fn new() -> Self {
        Self {
            config: MD018Config::default(),
        }
    }

    /// Check if a line is a MagicLink-style issue/PR reference (e.g., #123, #10)
    /// Used by MkDocs flavor to skip PyMdown MagicLink patterns
    fn is_magiclink_ref(line: &str) -> bool {
        MAGICLINK_REF_PATTERN.is_match(line.trim_start())
    }

    /// Check if a line is a tag (e.g., #tagname, #project/active, #3d_printing)
    fn is_tag(line: &str) -> bool {
        TAG_PATTERN.is_match(line.trim_start())
    }

    /// Whether tag patterns should be recognized for the given flavor
    fn tags_enabled(&self, flavor: MarkdownFlavor) -> bool {
        self.config.tags_enabled(flavor)
    }

    /// Check if an ATX heading line is missing space after the marker
    fn check_atx_heading_line(&self, line: &str, flavor: MarkdownFlavor) -> Option<(usize, String)> {
        // Look for ATX marker at start of line (with optional indentation)
        let trimmed_line = line.trim_start();
        let indent = line.len() - trimmed_line.len();

        if !trimmed_line.starts_with('#') {
            return None;
        }

        // Only flag patterns at column 1 (no indentation) to match markdownlint behavior
        // Indented patterns are likely:
        // - Multi-line link continuations (e.g., "  #sig-contribex](url)")
        // - List item content
        // - Other continuation contexts
        if indent > 0 {
            return None;
        }

        // Skip emoji hashtags and Unicode hashtag patterns
        let is_emoji = EMOJI_HASHTAG_PATTERN.is_match(trimmed_line);
        let is_unicode = UNICODE_HASHTAG_PATTERN.is_match(trimmed_line);
        if is_emoji || is_unicode {
            return None;
        }

        // Count the number of hashes
        let hash_count = trimmed_line.chars().take_while(|&c| c == '#').count();
        if hash_count == 0 || hash_count > 6 {
            return None;
        }

        // Check what comes after the hashes
        let after_hashes = &trimmed_line[hash_count..];

        // Skip if what follows the hashes is an emoji modifier or variant selector
        if after_hashes
            .chars()
            .next()
            .is_some_and(|ch| matches!(ch, '\u{FE0F}' | '\u{20E3}' | '\u{FE0E}'))
        {
            return None;
        }

        // If there's content immediately after hashes (no space), it needs fixing
        if !after_hashes.is_empty() && !after_hashes.starts_with(' ') && !after_hashes.starts_with('\t') {
            // Additional checks to avoid false positives
            let content = after_hashes.trim();

            // Skip if it's just more hashes (horizontal rule)
            if content.chars().all(|c| c == '#') {
                return None;
            }

            // Skip if content is too short to be meaningful
            if content.len() < 2 {
                return None;
            }

            // Skip if it starts with emphasis markers
            if content.starts_with('*') || content.starts_with('_') {
                return None;
            }

            // MagicLink config: skip MagicLink-style issue/PR refs (#123, #10, etc.)
            // MagicLink only uses single #, so check hash_count == 1
            if self.config.magiclink && hash_count == 1 && Self::is_magiclink_ref(line) {
                return None;
            }

            // Tags mode: skip tag syntax (#tagname, #project/active, etc.)
            // Tags only use single #
            if self.tags_enabled(flavor) && hash_count == 1 && Self::is_tag(line) {
                return None;
            }

            // This looks like a malformed heading that needs a space
            let fixed = format!("{}{} {}", " ".repeat(indent), "#".repeat(hash_count), after_hashes);
            return Some((indent + hash_count, fixed));
        }

        None
    }

    /// Split a line the parser found missing the space after its `#`s into its
    /// marker and the text that follows it, or `None` when the line is left
    /// alone: indented (MD018 flags column 1 only, like markdownlint), an emoji
    /// or Unicode hashtag, a MagicLink reference, or a tag.
    fn missing_space_split<'a>(
        &self,
        line: &'a str,
        indent: usize,
        missing: AtxMissingSpace,
        flavor: MarkdownFlavor,
    ) -> Option<(&'a str, &'a str)> {
        if indent > 0 {
            return None;
        }
        let trimmed = &line[indent..];
        if EMOJI_HASHTAG_PATTERN.is_match(trimmed) || UNICODE_HASHTAG_PATTERN.is_match(trimmed) {
            return None;
        }
        if missing.level == 1
            && ((self.config.magiclink && Self::is_magiclink_ref(line))
                || (self.tags_enabled(flavor) && Self::is_tag(line)))
        {
            return None;
        }
        let (marker, after_marker) = trimmed.split_at_checked(usize::from(missing.level))?;
        (!after_marker.is_empty()).then_some((marker, after_marker))
    }

    /// Whether a line the parser recorded nothing for may still be a heading
    /// missing its space: an ATX-shaped line the heading pass does not reach,
    /// such as one inside a link's span. A setext heading's text lines are
    /// heading text, recorded only on the last of them, so they are excluded.
    fn may_be_unrecorded_missing_space(line_info: &LineInfo) -> bool {
        line_info.heading.is_none()
            && !line_info.is_setext_heading_text
            && !line_info.in_code_block
            && !line_info.in_front_matter
            && !line_info.in_html_comment
            && true
            && !line_info.is_blank
    }

    // Calculate the byte range for a specific line in the content
    fn get_line_byte_range(&self, content: &str, line_num: usize) -> std::ops::Range<usize> {
        let mut current_line = 1;
        let mut start_byte = 0;

        for (i, c) in content.char_indices() {
            if current_line == line_num && c == '\n' {
                return start_byte..i;
            } else if c == '\n' {
                current_line += 1;
                if current_line == line_num {
                    start_byte = i + 1;
                }
            }
        }

        // If we're looking for the last line and it doesn't end with a newline
        if current_line == line_num {
            return start_byte..content.len();
        }

        // Fallback if line not found (shouldn't happen)
        0..0
    }
}

impl Rule for MD018NoMissingSpaceAtx {
    fn name(&self) -> &'static str {
        "MD018"
    }

    fn description(&self) -> &'static str {
        "No space after hash in heading"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = Vec::new();

        // Check every line the parser found missing the space after its `#`s
        for (line_num, line_info) in ctx.lines.iter().enumerate() {
            // Skip lines inside HTML blocks, HTML comments, or PyMdown blocks
            if line_info.in_html_block || line_info.in_html_comment {
                continue;
            }

            if let Some(missing) = line_info.atx_missing_space {
                let line = line_info.content(ctx.content);
                if let Some((marker, after_marker)) =
                    self.missing_space_split(line, line_info.indent, missing, ctx.flavor)
                {
                    // The indent and '#' markers are ASCII, so convert the byte
                    // offset to a character column.
                    let hash_end_col = byte_to_char_count(line, line_info.indent + marker.len());
                    let (start_line, start_col, end_line, end_col) = calculate_single_line_range(
                        line_num + 1, // Convert to 1-indexed
                        hash_end_col,
                        0, // Zero-width to indicate missing space
                    );

                    warnings.push(LintWarning {
                        rule_name: Some(self.name().to_string()),
                        message: format!("No space after {marker} in heading"),
                        line: start_line,
                        column: start_col,
                        end_line,
                        end_column: end_col,
                        severity: Severity::Warning,
                        fix: Some(Fix::new(self.get_line_byte_range(ctx.content, line_num + 1), {
                            // Preserve original indentation (including tabs)
                            let original_indent = &line[..line_info.indent];
                            format!("{original_indent}{marker} {after_marker}")
                        })),
                    });
                }
            } else if Self::may_be_unrecorded_missing_space(line_info) {
                // Check for malformed headings that weren't detected as proper headings
                if let Some((hash_end_pos, fixed_line)) =
                    self.check_atx_heading_line(line_info.content(ctx.content), ctx.flavor)
                {
                    let (start_line, start_col, end_line, end_col) = calculate_single_line_range(
                        line_num + 1,     // Convert to 1-indexed
                        hash_end_pos + 1, // 1-indexed column
                        0,                // Zero-width to indicate missing space
                    );

                    warnings.push(LintWarning {
                        rule_name: Some(self.name().to_string()),
                        message: "No space after hash in heading".to_string(),
                        line: start_line,
                        column: start_col,
                        end_line,
                        end_column: end_col,
                        severity: Severity::Warning,
                        fix: Some(Fix::new(
                            self.get_line_byte_range(ctx.content, line_num + 1),
                            fixed_line,
                        )),
                    });
                }
            }
        }

        Ok(warnings)
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        let warnings = self.check(ctx)?;
        let warnings = warnings;
        let warning_lines: std::collections::HashSet<usize> = warnings.iter().map(|w| w.line).collect();

        let mut lines = Vec::new();

        for (idx, line_info) in ctx.lines.iter().enumerate() {
            let mut fixed = false;

            if !warning_lines.contains(&(idx + 1)) {
                lines.push(line_info.content(ctx.content).to_string());
                continue;
            }

            if let Some(missing) = line_info.atx_missing_space {
                let line = line_info.content(ctx.content);
                if let Some((marker, after_marker)) =
                    self.missing_space_split(line, line_info.indent, missing, ctx.flavor)
                {
                    // Add space after marker, preserving original indentation (including tabs)
                    let original_indent = &line[..line_info.indent];
                    lines.push(format!("{original_indent}{marker} {after_marker}"));
                    fixed = true;
                }
            } else if Self::may_be_unrecorded_missing_space(line_info) {
                // Fix malformed headings
                if let Some((_, fixed_line)) = self.check_atx_heading_line(line_info.content(ctx.content), ctx.flavor) {
                    lines.push(fixed_line);
                    fixed = true;
                }
            }

            if !fixed {
                lines.push(line_info.content(ctx.content).to_string());
            }
        }

        // `ctx.lines` holds no entry after the final newline, so the file's
        // own final newline is restored after the join, even when the last
        // line is blank.
        let mut result = lines.join("\n");
        if ctx.content.ends_with('\n') {
            result.push('\n');
        }

        Ok(result)
    }

    /// Get the category of this rule for selective processing
    fn category(&self) -> RuleCategory {
        RuleCategory::Heading
    }

    /// Check if this rule should be skipped
    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Fast path: check if document likely has headings
        !ctx.likely_has_headings()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
