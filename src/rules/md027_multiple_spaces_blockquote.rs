use crate::utils::range_utils::calculate_match_range;

use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use regex::Regex;
use std::sync::LazyLock;

/// Configuration for MD027 (Multiple spaces after blockquote symbol).
///
/// `list_items` mirrors markdownlint's option but rumdl's default is `false`
/// rather than `true`. See `docs/markdownlint-comparison.md` for the rationale:
/// list items inside blockquotes inherently need extra indentation, so flagging
/// them by default produces noise. Set `list-items = true` to opt into the
/// strict markdownlint behavior.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MD027Config {
    /// When `true`, also flag blockquoted lines that introduce or continue a
    /// list item. When `false` (default), such lines are skipped.
    pub(crate) list_items: bool,
}

// New patterns for detecting malformed blockquote attempts where user intent is clear
static MALFORMED_BLOCKQUOTE_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        // Double > without space: >>text (looks like nested but missing spaces)
        (
            Regex::new(r"^(\s*)>>([^\s>].*|$)").unwrap(),
            "missing spaces in nested blockquote",
        ),
        // Triple > without space: >>>text
        (
            Regex::new(r"^(\s*)>>>([^\s>].*|$)").unwrap(),
            "missing spaces in deeply nested blockquote",
        ),
        // Space then > then text: > >text (extra > by mistake)
        (
            Regex::new(r"^(\s*)>\s+>([^\s>].*|$)").unwrap(),
            "extra blockquote marker",
        ),
        // Multiple spaces then >: (spaces)>text (indented blockquote without space)
        (
            Regex::new(r"^(\s{4,})>([^\s].*|$)").unwrap(),
            "indented blockquote missing space",
        ),
    ]
});

// Cached regex for blockquote validation
static BLOCKQUOTE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*>").unwrap());

/// Rule MD027: No multiple spaces after blockquote symbol
///
/// See [docs/md027.md](../../docs/md027.md) for full documentation, configuration, and examples.

#[derive(Debug, Default, Clone)]
pub struct MD027MultipleSpacesBlockquote {
    config: MD027Config,
}

impl MD027MultipleSpacesBlockquote {}

impl Rule for MD027MultipleSpacesBlockquote {
    fn name(&self) -> &'static str {
        "MD027"
    }

    fn description(&self) -> &'static str {
        "Multiple spaces after quote marker (>)"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Blockquote
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = Vec::new();

        for (line_idx, line_info) in ctx.lines.iter().enumerate() {
            let line_num = line_idx + 1;

            // Skip lines in code blocks, HTML blocks, and other skippable regions
            if line_info.in_code_block
                || line_info.in_html_block
                || line_info.in_html_comment
                || line_info.in_front_matter
                || line_info.in_mkdocstrings
            {
                continue;
            }

            // Check if this line is a blockquote using cached info
            if let Some(blockquote) = &line_info.blockquote {
                // Part 1: Check for multiple spaces after the blockquote marker.
                //
                // When `list_items = false` (rumdl default), skip lines that are part
                // of a list inside a blockquote — the extra spaces are list-indent,
                // not formatting noise. When `list_items = true` (markdownlint default),
                // flag those lines too.
                let skip_list_lines = !self.config.list_items;
                let is_likely_list_continuation = skip_list_lines
                    && (ctx.is_in_list_block(line_num)
                        || line_info.list_item.is_some()
                        || self.previous_blockquote_line_had_list(ctx, line_idx));
                if blockquote.has_multiple_spaces_after_marker && !is_likely_list_continuation {
                    // Find where the extra spaces start in the line
                    // We need to find the position after the markers and first space/tab
                    let mut byte_pos = 0;
                    let mut found_markers = 0;
                    let mut found_first_space = false;

                    for (i, ch) in line_info.content(ctx.content).char_indices() {
                        if found_markers < blockquote.nesting_level {
                            if ch == '>' {
                                found_markers += 1;
                            }
                        } else if !found_first_space && (ch == ' ' || ch == '\t') {
                            // This is the first space/tab after markers
                            found_first_space = true;
                        } else if found_first_space && (ch == ' ' || ch == '\t') {
                            // This is where extra spaces start
                            byte_pos = i;
                            break;
                        }
                    }

                    // Count how many extra spaces/tabs there are
                    let extra_spaces_bytes = line_info.content(ctx.content)[byte_pos..]
                        .chars()
                        .take_while(|&c| c == ' ' || c == '\t')
                        .fold(0, |acc, ch| acc + ch.len_utf8());

                    if extra_spaces_bytes > 0 {
                        // When blockquote content is empty, remove all spaces
                        // after the marker to avoid creating trailing whitespace
                        let (fix_byte_pos, fix_bytes) = if blockquote.content.is_empty() {
                            // Remove the first space too (byte_pos - 1 points to
                            // the first space we skipped)
                            let first_space_pos = byte_pos - 1;
                            let all_spaces_bytes = line_info.content(ctx.content)[first_space_pos..]
                                .chars()
                                .take_while(|&c| c == ' ' || c == '\t')
                                .fold(0, |acc, ch| acc + ch.len_utf8());
                            (first_space_pos, all_spaces_bytes)
                        } else {
                            (byte_pos, extra_spaces_bytes)
                        };

                        let (start_line, start_col, end_line, end_col) =
                            calculate_match_range(line_num, line_info.content(ctx.content), fix_byte_pos, fix_bytes);

                        warnings.push(LintWarning {
                            rule_name: Some(self.name().to_string()),
                            line: start_line,
                            column: start_col,
                            end_line,
                            end_column: end_col,
                            message: "Multiple spaces after quote marker (>)".to_string(),
                            severity: Severity::Warning,
                            fix: Some(Fix::new(
                                {
                                    let start_byte = ctx.line_column_byte_range(line_num, start_col).start;
                                    let end_byte = ctx.line_column_byte_range(line_num, end_col).start;
                                    start_byte..end_byte
                                },
                                String::new(),
                            )),
                        });
                    }
                }
            } else {
                // Part 2: Check for malformed blockquote attempts on non-blockquote lines
                let malformed_attempts = self.detect_malformed_blockquote_attempts(line_info.content(ctx.content));
                for (start, len, fixed_line, description) in malformed_attempts {
                    let (start_line, start_col, end_line, end_col) =
                        calculate_match_range(line_num, line_info.content(ctx.content), start, len);

                    warnings.push(LintWarning {
                        rule_name: Some(self.name().to_string()),
                        line: start_line,
                        column: start_col,
                        end_line,
                        end_column: end_col,
                        message: format!("Malformed quote: {description}"),
                        severity: Severity::Warning,
                        fix: Some(Fix::new(
                            ctx.line_column_byte_range_with_length(
                                line_num,
                                1,
                                line_info.content(ctx.content).chars().count(),
                            ),
                            fixed_line,
                        )),
                    });
                }
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
        crate::utils::fix_utils::apply_warning_fixes(ctx.content, &warnings)
            .map_err(crate::rule::LintError::InvalidInput)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// Check if this rule should be skipped
    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        ctx.content.is_empty() || !ctx.likely_has_blockquotes()
    }
}

impl MD027MultipleSpacesBlockquote {
    /// Check if a previous line in the same blockquote context had a list item
    /// This helps identify list continuation lines even when list block detection
    /// doesn't catch all continuation lines
    fn previous_blockquote_line_had_list(&self, ctx: &crate::lint_context::LintContext, line_idx: usize) -> bool {
        // Look backwards for a blockquote line with a list item
        // Stop when we hit a non-blockquote line or find a list item
        for prev_idx in (0..line_idx).rev() {
            let prev_line = &ctx.lines[prev_idx];

            // If previous line is not a blockquote, stop searching
            if prev_line.blockquote.is_none() {
                return false;
            }

            // If previous line has a list item, this could be list continuation
            if prev_line.list_item.is_some() {
                return true;
            }

            // If it's in a list block, that's also good enough
            if ctx.is_in_list_block(prev_idx + 1) {
                return true;
            }
        }
        false
    }

    /// Detect malformed blockquote attempts where user intent is clear
    fn detect_malformed_blockquote_attempts(&self, line: &str) -> Vec<(usize, usize, String, String)> {
        let mut results = Vec::new();

        for (pattern, issue_type) in MALFORMED_BLOCKQUOTE_PATTERNS.iter() {
            if let Some(cap) = pattern.captures(line) {
                let match_obj = cap.get(0).unwrap();
                let start = match_obj.start();
                let len = match_obj.len();

                // Extract potential blockquote components
                if let Some((fixed_line, description)) = self.extract_blockquote_fix_from_match(&cap, issue_type, line)
                {
                    // Only proceed if this looks like a genuine blockquote attempt
                    if self.looks_like_blockquote_attempt(line, &fixed_line) {
                        results.push((start, len, fixed_line, description));
                    }
                }
            }
        }

        results
    }

    /// Extract the proper blockquote format from a malformed match
    fn extract_blockquote_fix_from_match(
        &self,
        cap: &regex::Captures,
        issue_type: &str,
        _original_line: &str,
    ) -> Option<(String, String)> {
        match issue_type {
            "missing spaces in nested blockquote" => {
                // >>text -> > > text
                let indent = cap.get(1).map_or("", |m| m.as_str());
                let content = cap.get(2).map_or("", |m| m.as_str());
                Some((
                    format!("{}> > {}", indent, content.trim()),
                    "Missing spaces in nested blockquote".to_string(),
                ))
            }
            "missing spaces in deeply nested blockquote" => {
                // >>>text -> > > > text
                let indent = cap.get(1).map_or("", |m| m.as_str());
                let content = cap.get(2).map_or("", |m| m.as_str());
                Some((
                    format!("{}> > > {}", indent, content.trim()),
                    "Missing spaces in deeply nested blockquote".to_string(),
                ))
            }
            "extra blockquote marker" => {
                // > >text -> > text
                let indent = cap.get(1).map_or("", |m| m.as_str());
                let content = cap.get(2).map_or("", |m| m.as_str());
                Some((
                    format!("{}> {}", indent, content.trim()),
                    "Extra blockquote marker".to_string(),
                ))
            }
            "indented blockquote missing space" => {
                // (spaces)>text -> (spaces)> text
                let indent = cap.get(1).map_or("", |m| m.as_str());
                let content = cap.get(2).map_or("", |m| m.as_str());
                Some((
                    format!("{}> {}", indent, content.trim()),
                    "Indented blockquote missing space".to_string(),
                ))
            }
            _ => None,
        }
    }

    /// Check if the pattern looks like a genuine blockquote attempt
    fn looks_like_blockquote_attempt(&self, original: &str, fixed: &str) -> bool {
        // Basic heuristics to avoid false positives

        // 1. Content should not be too short (avoid flagging things like ">>>" alone)
        let trimmed_original = original.trim();
        if trimmed_original.len() < 5 {
            // More restrictive
            return false;
        }

        // 2. Should contain some text content after the markers
        let content_after_markers = trimmed_original.trim_start_matches('>').trim_start_matches(' ');
        if content_after_markers.is_empty() || content_after_markers.len() < 3 {
            // More restrictive
            return false;
        }

        // 3. Content should contain some alphabetic characters (not just symbols)
        if !content_after_markers.chars().any(char::is_alphabetic) {
            return false;
        }

        // 4. Fixed version should actually be a valid blockquote
        // Check if it starts with optional whitespace followed by >
        if !BLOCKQUOTE_PATTERN.is_match(fixed) {
            return false;
        }

        // 5. Avoid flagging things that might be code or special syntax
        if content_after_markers.starts_with('#') // Headers
            || content_after_markers.starts_with('[') // Links
            || content_after_markers.starts_with('`') // Code
            || content_after_markers.starts_with("http") // URLs
            || content_after_markers.starts_with("www.") // URLs
            || content_after_markers.starts_with("ftp")
        // URLs
        {
            return false;
        }

        // 6. Content should look like prose, not code or markup
        let word_count = content_after_markers.split_whitespace().count();
        if word_count < 3 {
            // Should be at least 3 words to look like prose
            return false;
        }

        true
    }
}
