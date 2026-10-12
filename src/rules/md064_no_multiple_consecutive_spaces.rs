/// Rule MD064: No multiple consecutive spaces
///
/// See [docs/md064.md](../../docs/md064.md) for full documentation, configuration, and examples.
///
/// This rule is triggered when multiple consecutive spaces are found in markdown content.
/// Multiple spaces between words serve no purpose and can indicate formatting issues.
///
/// For example:
///
/// ```markdown
/// This is   a sentence with extra spaces.
/// ```
///
/// Should be:
///
/// ```markdown
/// This is a sentence with extra spaces.
/// ```
///
/// This rule does NOT flag:
/// - Spaces inside inline code spans (`` `code   here` ``)
/// - Spaces inside fenced or indented code blocks
/// - Leading whitespace (indentation)
/// - Trailing whitespace (handled by MD009)
/// - Spaces inside HTML comments, HTML tags, or HTML blocks
/// - Table rows (alignment padding is intentional)
/// - Front matter content
use crate::filtered_lines::FilteredLinesExt;
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::blockquote::parse_blockquote_prefix;
use crate::utils::sentence_utils::is_after_sentence_ending;
use crate::utils::skip_context::is_table_line;
use std::collections::HashSet;
use std::sync::Arc;

/// Regex to find multiple consecutive spaces (2 or more)
use regex::Regex;
use std::sync::LazyLock;

static MULTIPLE_SPACES_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    // Match 2 or more consecutive spaces
    Regex::new(r" {2,}").unwrap()
});

/// Configuration for MD064 (No multiple consecutive spaces)
#[derive(Debug, Clone, PartialEq)]
pub struct MD064Config {
    /// Allow exactly two spaces after sentence-ending punctuation (default: false)
    ///
    /// When enabled, allows exactly 2 spaces after sentence-ending punctuation
    /// (`.`, `!`, `?`) while still flagging multiple spaces elsewhere. This
    /// supports the traditional typewriter convention of two spaces after sentences.
    ///
    /// Sentence-ending punctuation includes:
    /// - Period: `.`
    /// - Exclamation mark: `!`
    /// - Question mark: `?`
    ///
    /// Also recognizes closing punctuation after sentence endings:
    /// - Quotes: `."`, `!"`, `?"`, `.'`, `!'`, `?'`
    /// - Parentheses: `.)`, `!)`, `?)`
    /// - Brackets: `.]`, `!]`, `?]`
    /// - Ellipsis: `...`
    ///
    /// Example with `allow-sentence-double-space = true`:
    /// ```markdown
    /// First sentence.  Second sentence.    <- OK (2 spaces after period)
    /// Multiple   spaces here.              <- Flagged (3 spaces, not after sentence)
    /// Word  word in middle.                <- Flagged (2 spaces, not after sentence)
    /// ```
    pub(crate) allow_sentence_double_space: bool,
}

fn default_allow_sentence_double_space() -> bool {
    false
}

impl Default for MD064Config {
    fn default() -> Self {
        Self {
            allow_sentence_double_space: default_allow_sentence_double_space(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MD064NoMultipleConsecutiveSpaces {
    config: MD064Config,
}

impl Default for MD064NoMultipleConsecutiveSpaces {
    fn default() -> Self {
        Self::new()
    }
}

impl MD064NoMultipleConsecutiveSpaces {
    pub(crate) fn new() -> Self {
        Self {
            config: MD064Config::default(),
        }
    }

    /// Check if a byte position is inside an inline code span
    fn is_in_code_span(&self, code_spans: &[crate::lint_context::CodeSpan], byte_pos: usize) -> bool {
        code_spans
            .iter()
            .any(|span| byte_pos >= span.byte_offset && byte_pos < span.byte_end)
    }

    /// Check if a match is trailing whitespace at the end of a line
    /// Trailing spaces are handled by MD009, so MD064 should skip them entirely
    fn is_trailing_whitespace(&self, line: &str, match_end: usize) -> bool {
        // If the match extends to the end of the line, it's trailing whitespace
        let remaining = &line[match_end..];
        remaining.is_empty() || remaining.chars().all(|c| c == '\n' || c == '\r')
    }

    /// Check if the match is part of leading indentation
    fn is_leading_indentation(&self, line: &str, match_start: usize) -> bool {
        // Check if everything before the match is whitespace
        line[..match_start].chars().all(|c| c == ' ' || c == '\t')
    }

    /// Check if the match is immediately after a list marker (handled by MD030)
    fn is_after_list_marker(&self, line: &str, match_start: usize) -> bool {
        // Strip blockquote prefix to handle lists inside blockquotes (e.g., "> 1.  item")
        let before_text = if let Some(parsed) = parse_blockquote_prefix(line) {
            let prefix_len = parsed.prefix.len();
            if match_start <= prefix_len {
                return false;
            }
            line[prefix_len..match_start].trim_start()
        } else {
            line[..match_start].trim_start()
        };

        // Unordered list markers: *, -, +
        if before_text == "*" || before_text == "-" || before_text == "+" {
            return true;
        }

        // Ordered list markers: digits followed by . or )
        // Examples: "1.", "2)", "10.", "123)"
        if before_text.len() >= 2 {
            let last_char = before_text.chars().last().unwrap();
            if last_char == '.' || last_char == ')' {
                let prefix = &before_text[..before_text.len() - 1];
                if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
                    return true;
                }
            }
        }

        false
    }

    /// Check if the match is immediately after a blockquote marker (handled by MD027)
    /// Patterns: "> ", ">  ", ">>", "> > "
    fn is_after_blockquote_marker(&self, line: &str, match_start: usize) -> bool {
        let before = line[..match_start].trim_start();

        // Check if it's only blockquote markers (> characters, possibly with spaces between)
        if before.is_empty() {
            return false;
        }

        // Pattern: one or more '>' characters, optionally followed by space and more '>'
        let trimmed = before.trim_end();
        if trimmed.chars().all(|c| c == '>') {
            return true;
        }

        // Pattern: "> " at end (nested blockquote with space)
        if trimmed.ends_with('>') {
            let inner = trimmed.trim_end_matches('>').trim();
            if inner.is_empty() || inner.chars().all(|c| c == '>') {
                return true;
            }
        }

        false
    }

    /// Check if the space count looks like a tab replacement (multiple of 4)
    /// Tab replacements (4, 8, 12, etc. spaces) are intentional and should not be collapsed.
    /// This prevents MD064 from undoing MD010's tab-to-spaces conversion.
    fn is_tab_replacement_pattern(&self, space_count: usize) -> bool {
        space_count >= 4 && space_count.is_multiple_of(4)
    }

    /// Check if the match is inside or after a reference link definition
    /// Pattern: `[label]: URL` or `[label]:  URL`
    fn is_reference_link_definition(&self, line: &str, match_start: usize) -> bool {
        let trimmed = line.trim_start();
        let leading_spaces = line.len() - trimmed.len();

        // Reference link pattern: [label]: URL
        if trimmed.starts_with('[')
            && let Some(bracket_end) = trimmed.find("]:")
        {
            let colon_pos = leading_spaces + bracket_end + 2;
            // Check if the match is right after the ]: marker
            if match_start >= colon_pos - 1 && match_start <= colon_pos + 1 {
                return true;
            }
        }

        false
    }

    /// Check if the match is after a footnote marker
    /// Pattern: [^label]:  text
    fn is_after_footnote_marker(&self, line: &str, match_start: usize) -> bool {
        let trimmed = line.trim_start();

        // Footnote pattern: [^label]: text
        if trimmed.starts_with("[^")
            && let Some(bracket_end) = trimmed.find("]:")
        {
            let leading_spaces = line.len() - trimmed.len();
            let colon_pos = leading_spaces + bracket_end + 2;
            // Check if the match is right after the ]: marker
            if match_start >= colon_pos.saturating_sub(1) && match_start <= colon_pos + 1 {
                return true;
            }
        }

        false
    }

    /// Check if the match is after a definition list marker
    /// Pattern: :   Definition text
    fn is_after_definition_marker(&self, line: &str, match_start: usize) -> bool {
        let before = line[..match_start].trim_start();

        // Definition list marker is just ":"
        before == ":"
    }

    /// Check if the match is immediately after a task list checkbox.
    /// Standard GFM: only `[ ]`, `[x]`, `[X]` are valid checkboxes.
    /// Obsidian flavor: any single character inside brackets is a valid checkbox
    /// (e.g., `[/]`, `[-]`, `[>]`, `[✓]`).
    fn is_after_task_checkbox(&self, line: &str, match_start: usize, _flavor: crate::config::MarkdownFlavor) -> bool {
        let before = line[..match_start].trim_start();

        // Zero-allocation iterator-based check for: marker + space + '[' + char + ']'
        let mut chars = before.chars();
        let pattern = (
            chars.next(),
            chars.next(),
            chars.next(),
            chars.next(),
            chars.next(),
            chars.next(),
        );

        match pattern {
            (Some('*' | '-' | '+'), Some(' '), Some('['), Some(c), Some(']'), None) => {
                {
                    // Standard GFM: only space, 'x', or 'X' are valid
                    matches!(c, ' ' | 'x' | 'X')
                }
            }
            _ => false,
        }
    }

    /// Collect line numbers that belong to a "column-aligned" list.
    ///
    /// A list is column-aligned when it has at least two items and every
    /// item's first line contains a run of two or more consecutive spaces
    /// somewhere in its post-marker content (excluding trailing whitespace).
    /// In that case the spacing is intentional column alignment — common in
    /// `cdk init`-style "Useful commands" sections — and per-line MD064 fixes
    /// would corrupt the layout while leaving lint green. Skipping the rule
    /// for those item lines keeps detection list-consistent and the fix safe.
    ///
    /// Each list is judged on its own items: a nested list is a different list
    /// from its parent, so an unaligned child does not disqualify the aligned
    /// parent items around it, and vice versa.
    fn aligned_list_item_lines(&self, ctx: &crate::lint_context::LintContext) -> HashSet<usize> {
        let mut aligned = HashSet::new();
        for block in &ctx.list_blocks {
            for items in ctx.list_block_item_groups(block) {
                if items.len() < 2 {
                    continue;
                }
                let all_aligned = items
                    .iter()
                    .all(|&line_num| self.item_line_has_internal_alignment(ctx, line_num));
                if all_aligned {
                    aligned.extend(items);
                }
            }
        }
        aligned
    }

    /// True when the given list-item line has a run of 2+ consecutive spaces
    /// in its post-marker, non-trailing content.
    fn item_line_has_internal_alignment(&self, ctx: &crate::lint_context::LintContext, line_num: usize) -> bool {
        let Some(line_info) = ctx.line_info(line_num) else {
            return false;
        };
        let Some(item) = &line_info.list_item else {
            return false;
        };
        let content = line_info.content(ctx.content);
        if item.content_column >= content.len() {
            return false;
        }
        content[item.content_column..].trim_end().contains("  ")
    }

    /// Check if this is a table row without outer pipes (GFM extension)
    /// Pattern: text | text | text (no leading/trailing pipe)
    fn is_table_without_outer_pipes(&self, line: &str) -> bool {
        let trimmed = line.trim();

        // Must contain at least one pipe but not start or end with pipe
        if !trimmed.contains('|') {
            return false;
        }

        // If it starts or ends with |, it's a normal table (handled by is_table_line)
        if trimmed.starts_with('|') || trimmed.ends_with('|') {
            return false;
        }

        // Check if it looks like a table row: has multiple pipe-separated cells
        // Could be data row (word | word) or separator row (--- | ---)
        // Table cells can be empty, so we just check for at least 2 parts
        let parts: Vec<&str> = trimmed.split('|').collect();
        if parts.len() >= 2 {
            // At least first or last cell should have content (not just whitespace)
            // to distinguish from accidental pipes in text
            let first_has_content = !parts.first().unwrap_or(&"").trim().is_empty();
            let last_has_content = !parts.last().unwrap_or(&"").trim().is_empty();
            if first_has_content || last_has_content {
                return true;
            }
        }

        false
    }
}

impl Rule for MD064NoMultipleConsecutiveSpaces {
    fn name(&self) -> &'static str {
        "MD064"
    }

    fn description(&self) -> &'static str {
        "Multiple consecutive spaces"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let content = ctx.content;

        // Early return: if no double spaces at all, skip
        if !content.contains("  ") {
            return Ok(vec![]);
        }

        // Config is already correct - engine applies inline overrides before calling check()
        let mut warnings = Vec::new();
        let code_spans: Arc<Vec<crate::lint_context::CodeSpan>> = ctx.code_spans();

        // Pre-compute lines belonging to column-aligned list blocks. The
        // alignment whitespace there is intentional, and per-line fixes would
        // corrupt the surrounding layout — see `aligned_list_item_lines`.
        let aligned_lines = self.aligned_list_item_lines(ctx);

        // Process content lines, automatically skipping front matter, code blocks, HTML, PyMdown blocks, and Obsidian comments
        for line in ctx
            .filtered_lines()
            .skip_front_matter()
            .skip_code_blocks()
            .skip_html_blocks()
            .skip_html_comments()
            .skip_mkdocstrings()
            .skip_esm_blocks()
            .skip_jsx_expressions()
            .skip_mdx_comments()
            .skip_pymdown_blocks()
            .skip_obsidian_comments()
        {
            // Quick check: skip if line doesn't contain double spaces
            if !line.content.contains("  ") {
                continue;
            }

            // Skip lines that belong to a column-aligned list block. The
            // surrounding items use the same intentional alignment, so flagging
            // outliers here would produce a destructive partial fix.
            if aligned_lines.contains(&line.line_num) {
                continue;
            }

            // Skip table rows (alignment padding is intentional)
            if is_table_line(line.content) {
                continue;
            }

            // Skip tables without outer pipes (GFM extension)
            if self.is_table_without_outer_pipes(line.content) {
                continue;
            }

            let line_start_byte = ctx.line_start_byte(line.line_num).unwrap_or(0);

            // Find all occurrences of multiple consecutive spaces
            for mat in MULTIPLE_SPACES_REGEX.find_iter(line.content) {
                let match_start = mat.start();
                let match_end = mat.end();
                let space_count = match_end - match_start;

                // Skip if this is leading indentation
                if self.is_leading_indentation(line.content, match_start) {
                    continue;
                }

                // Skip trailing whitespace (handled by MD009)
                if self.is_trailing_whitespace(line.content, match_end) {
                    continue;
                }

                // Skip tab replacement patterns (4, 8, 12, etc. spaces)
                // This prevents MD064 from undoing MD010's tab-to-spaces conversion
                if self.is_tab_replacement_pattern(space_count) {
                    continue;
                }

                // Skip spaces after list markers (handled by MD030)
                if self.is_after_list_marker(line.content, match_start) {
                    continue;
                }

                // Skip spaces after blockquote markers (handled by MD027)
                if self.is_after_blockquote_marker(line.content, match_start) {
                    continue;
                }

                // Skip spaces after footnote markers
                if self.is_after_footnote_marker(line.content, match_start) {
                    continue;
                }

                // Skip spaces after reference link definition markers
                if self.is_reference_link_definition(line.content, match_start) {
                    continue;
                }

                // Skip spaces after definition list markers
                if self.is_after_definition_marker(line.content, match_start) {
                    continue;
                }

                // Skip spaces after task list checkboxes
                if self.is_after_task_checkbox(line.content, match_start, ctx.flavor) {
                    continue;
                }

                // Allow exactly 2 spaces after sentence-ending punctuation if configured
                // This supports the traditional typewriter convention of two spaces after sentences
                if self.config.allow_sentence_double_space
                    && space_count == 2
                    && is_after_sentence_ending(line.content, match_start)
                {
                    continue;
                }

                // Calculate absolute byte position
                let abs_byte_start = line_start_byte + match_start;

                // Skip if inside an inline code span
                if self.is_in_code_span(&code_spans, abs_byte_start) {
                    continue;
                }

                // Inline HTML is raw: spaces in a comment or inside a tag (an
                // attribute value like `title="a  b"`) are not prose to collapse.
                if ctx.is_in_html_comment(abs_byte_start) || ctx.is_in_html_tag(abs_byte_start) {
                    continue;
                }

                // Calculate byte range for the fix
                let abs_byte_end = line_start_byte + match_end;

                // Determine the replacement: if allow_sentence_double_space is enabled
                // and this is after a sentence ending, collapse to 2 spaces, otherwise to 1
                let replacement =
                    if self.config.allow_sentence_double_space && is_after_sentence_ending(line.content, match_start) {
                        "  ".to_string() // Collapse to two spaces after sentence
                    } else {
                        " ".to_string() // Collapse to single space
                    };

                warnings.push(LintWarning {
                    rule_name: Some(self.name().to_string()),
                    message: format!("Multiple consecutive spaces ({space_count}) found"),
                    line: line.line_num,
                    column: match_start + 1, // 1-indexed
                    end_line: line.line_num,
                    end_column: match_end + 1, // 1-indexed
                    severity: Severity::Warning,
                    fix: Some(Fix::new(abs_byte_start..abs_byte_end, replacement)),
                });
            }
        }

        Ok(warnings)
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        let content = ctx.content;

        // Early return if no double spaces
        if !content.contains("  ") {
            return Ok(content.to_string());
        }

        // Get warnings to identify what needs to be fixed
        let warnings = self.check(ctx)?;
        let warnings = warnings;
        if warnings.is_empty() {
            return Ok(content.to_string());
        }

        // Collect all fixes and sort by position (reverse order to avoid position shifts)
        let mut fixes: Vec<(std::ops::Range<usize>, String)> = warnings
            .into_iter()
            .filter_map(|w| w.fix.map(|f| (f.range, f.replacement)))
            .collect();

        fixes.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));

        // Apply fixes
        let mut result = content.to_string();
        for (range, replacement) in fixes {
            if range.start < result.len() && range.end <= result.len() {
                result.replace_range(range, &replacement);
            }
        }

        Ok(result)
    }

    /// Get the category of this rule for selective processing
    fn category(&self) -> RuleCategory {
        RuleCategory::Whitespace
    }

    /// Check if this rule should be skipped
    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        ctx.content.is_empty() || !ctx.content.contains("  ")
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
