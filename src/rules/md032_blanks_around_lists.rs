use crate::lint_context::LazyContLine;
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::blockquote::{content_after_blockquote, effective_indent_in_blockquote, parse_blockquote_prefix};
use crate::utils::calculate_indentation_width_default;
use crate::utils::list_fix_guard::{Allowed, drop_structure_changing_fixes};
use crate::utils::range_utils::calculate_line_range;
use regex::Regex;
use std::sync::LazyLock;

mod md032_config;
pub(super) use md032_config::MD032Config;

// Detects ordered list items starting with a number other than 1
static ORDERED_LIST_NON_ONE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*([2-9]|\d{2,})\.\s").unwrap());

/// Check if a line is a thematic break (horizontal rule)
/// Per CommonMark: 0-3 spaces of indentation, then 3+ of same char (-, *, _), optionally with spaces between
fn is_thematic_break(line: &str) -> bool {
    // Per CommonMark, thematic breaks can have 0-3 spaces of indentation (< 4 columns)
    if calculate_indentation_width_default(line) > 3 {
        return false;
    }

    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return false;
    }

    let chars: Vec<char> = trimmed.chars().collect();
    let first_non_space = chars.iter().find(|&&c| c != ' ');

    if let Some(&marker) = first_non_space {
        if marker != '-' && marker != '*' && marker != '_' {
            return false;
        }
        let marker_count = chars.iter().filter(|&&c| c == marker).count();
        let other_count = chars.iter().filter(|&&c| c != marker && c != ' ').count();
        marker_count >= 3 && other_count == 0
    } else {
        false
    }
}

/// Rule MD032: Lists should be surrounded by blank lines
///
/// This rule enforces that lists are surrounded by blank lines, which improves document
/// readability and ensures consistent rendering across different Markdown processors.
///
/// ## Purpose
///
/// - **Readability**: Blank lines create visual separation between lists and surrounding content
/// - **Parsing**: Many Markdown parsers require blank lines around lists for proper rendering
/// - **Consistency**: Ensures uniform document structure and appearance
/// - **Compatibility**: Improves compatibility across different Markdown implementations
///
/// ## Examples
///
/// ### Correct
///
/// ```markdown
/// This is a paragraph of text.
///
/// - Item 1
/// - Item 2
/// - Item 3
///
/// This is another paragraph.
/// ```
///
/// ### Incorrect
///
/// ```markdown
/// This is a paragraph of text.
/// - Item 1
/// - Item 2
/// - Item 3
/// This is another paragraph.
/// ```
///
/// ## Behavior Details
///
/// This rule checks for the following:
///
/// - **List Start**: There should be a blank line before the first item in a list
///   (unless the list is at the beginning of the document or after front matter)
/// - **List End**: There should be a blank line after the last item in a list
///   (unless the list is at the end of the document)
/// - **Nested Lists**: Properly handles nested lists and list continuations
/// - **List Types**: Works with ordered lists, unordered lists, and all valid list markers (-, *, +)
///
/// ## Special Cases
///
/// This rule handles several special cases:
///
/// - **Front Matter**: YAML front matter is detected and skipped
/// - **Code Blocks**: Lists inside code blocks are ignored
/// - **List Content**: Indented content belonging to list items is properly recognized as part of the list
/// - **Document Boundaries**: Lists at the beginning or end of the document have adjusted requirements
///
/// ## Fix Behavior
///
/// When applying automatic fixes, this rule:
/// - Adds a blank line before the first list item when needed
/// - Adds a blank line after the last list item when needed
/// - Preserves document structure and existing content
///
/// ## Performance Optimizations
///
/// The rule includes several optimizations:
/// - Fast path checks before applying more expensive regex operations
/// - Efficient list item detection
/// - Pre-computation of code block lines to avoid redundant processing
#[derive(Debug, Clone, Default)]
pub struct MD032BlanksAroundLists {
    config: MD032Config,
}

/// The lines covered by MD032's list blocks, answering whether a line lies
/// in any of them by binary search. The blocks may overlap and need not be
/// ordered: sorted by start, each keeps the furthest end reached so far, so a
/// line is covered exactly when the last block starting at or before it
/// reaches it.
struct ListCoverage {
    starts: Vec<usize>,
    reach: Vec<usize>,
}

impl ListCoverage {
    fn new(list_blocks: &[(usize, usize, String)]) -> Self {
        let mut spans: Vec<(usize, usize)> = list_blocks.iter().map(|&(start, end, _)| (start, end)).collect();
        spans.sort_unstable();
        let mut furthest = 0;
        let (starts, reach) = spans
            .into_iter()
            .map(|(start, end)| {
                furthest = furthest.max(end);
                (start, furthest)
            })
            .unzip();
        Self { starts, reach }
    }

    fn contains(&self, line_num: usize) -> bool {
        let before = self.starts.partition_point(|&start| start <= line_num);
        before > 0 && self.reach[before - 1] >= line_num
    }
}

impl MD032BlanksAroundLists {}

impl MD032BlanksAroundLists {
    /// Check if a blank line should be required before a list based on the previous line context
    fn should_require_blank_line_before(
        ctx: &crate::lint_context::LintContext,
        prev_line_num: usize,
        current_line_num: usize,
    ) -> bool {
        // Always require blank lines after code blocks, front matter, etc.
        if ctx
            .line_info(prev_line_num)
            .is_some_and(|info| info.in_code_block || info.in_front_matter)
        {
            return true;
        }

        // Always allow nested lists (lists indented within other list items)
        if Self::is_nested_list(ctx, prev_line_num, current_line_num) {
            return false;
        }

        // Default: require blank line (matching markdownlint's behavior)
        true
    }

    /// Check if the current list is nested within another list item
    fn is_nested_list(
        ctx: &crate::lint_context::LintContext,
        prev_line_num: usize,    // 1-indexed
        current_line_num: usize, // 1-indexed
    ) -> bool {
        // Check if current line is indented (typical for nested lists)
        if current_line_num > 0 && current_line_num - 1 < ctx.lines.len() {
            let current_line = &ctx.lines[current_line_num - 1];
            if current_line.indent >= 2 {
                // Check if previous line is a list item or list content
                if prev_line_num > 0 && prev_line_num - 1 < ctx.lines.len() {
                    let prev_line = &ctx.lines[prev_line_num - 1];
                    // Previous line is a list item or indented content
                    if prev_line.list_item.is_some() || prev_line.indent >= 2 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if a lazy continuation fix should be applied to a line.
    /// Returns false for lines inside code blocks, front matter, or HTML comments.
    fn should_apply_lazy_fix(ctx: &crate::lint_context::LintContext, line_num: usize) -> bool {
        ctx.lines
            .get(line_num.saturating_sub(1))
            .is_some_and(|li| !li.in_code_block && !li.in_front_matter && !li.in_html_comment && true)
    }

    /// Whether a line is a Pandoc/Quarto fenced-div marker in a flavor where
    /// those are structure rather than prose. MD032 looks through them when it
    /// scans for the blank line around a list, and the same line must not be
    /// read as prose that lazily continues the last item either: indenting a
    /// closing `:::` into the item pulls the div's fence into the list.
    fn is_transparent_div_marker(
        _ctx: &crate::lint_context::LintContext,
        _info: &crate::lint_context::LineInfo,
    ) -> bool {
        {
            return false;
        }
    }

    /// Whether a lazy continuation line reported by the parser is one MD032
    /// should act on: inside a list block, and not a transparent div marker.
    fn is_reportable_lazy_line(
        ctx: &crate::lint_context::LintContext,
        coverage: &ListCoverage,
        line_num: usize,
    ) -> bool {
        if !coverage.contains(line_num) {
            return false;
        }
        ctx.lines
            .get(line_num.saturating_sub(1))
            .is_some_and(|info| !Self::is_transparent_div_marker(ctx, info))
    }

    /// Calculate the fix for a lazy continuation line.
    /// Returns the byte range to replace and the replacement string.
    fn calculate_lazy_continuation_fix(
        ctx: &crate::lint_context::LintContext,
        line_num: usize,
        lazy_info: &LazyContLine,
    ) -> Option<Fix> {
        let line_info = ctx.lines.get(line_num.saturating_sub(1))?;
        let line_content = line_info.content(ctx.content);

        if lazy_info.blockquote_level == 0 {
            // Regular list (no blockquote): replace leading whitespace with proper indent
            let start_byte = line_info.byte_offset;
            let end_byte = start_byte + lazy_info.current_indent;
            let replacement = " ".repeat(lazy_info.expected_indent);

            Some(Fix::new(start_byte..end_byte, replacement))
        } else {
            // List inside blockquote: preserve blockquote prefix, fix indent after it
            let after_bq = content_after_blockquote(line_content, lazy_info.blockquote_level);
            let prefix_byte_len = line_content.len().saturating_sub(after_bq.len());
            if prefix_byte_len == 0 {
                return None;
            }

            let current_indent = after_bq.len() - after_bq.trim_start().len();
            let start_byte = line_info.byte_offset + prefix_byte_len;
            let end_byte = start_byte + current_indent;
            let replacement = " ".repeat(lazy_info.expected_indent);

            Some(Fix::new(start_byte..end_byte, replacement))
        }
    }

    /// Apply a lazy continuation fix to a single line.
    /// Replaces the current indentation with the expected indentation.
    fn apply_lazy_fix_to_line(line: &str, lazy_info: &LazyContLine) -> String {
        if lazy_info.blockquote_level == 0 {
            // Regular list: strip current indent, add expected indent
            let content = line.trim_start();
            format!("{}{}", " ".repeat(lazy_info.expected_indent), content)
        } else {
            // Blockquote list: preserve blockquote prefix, fix indent after it
            let after_bq = content_after_blockquote(line, lazy_info.blockquote_level);
            let prefix_len = line.len().saturating_sub(after_bq.len());
            if prefix_len == 0 {
                return line.to_string();
            }

            let prefix = &line[..prefix_len];
            let rest = after_bq.trim_start();
            format!("{}{}{}", prefix, " ".repeat(lazy_info.expected_indent), rest)
        }
    }

    /// Find the first non-transparent line before the given line (1-indexed).
    /// Returns (line_num, is_blank) where:
    /// - line_num is the 1-indexed line of actual content (0 if start of document)
    /// - is_blank is true if that line is blank (meaning separation exists)
    ///
    /// Transparent elements (HTML comments, Quarto div markers) are skipped,
    /// matching markdownlint-cli behavior.
    fn find_preceding_content(ctx: &crate::lint_context::LintContext, before_line: usize) -> (usize, bool) {
        for line_num in (1..before_line).rev() {
            let idx = line_num - 1;
            if let Some(info) = ctx.lines.get(idx) {
                // A line contributing nothing but comments separates the blocks
                // around it the way an empty one does (#866)
                if is_blank_in_context(info.content(ctx.content)) {
                    return (line_num, true);
                }
                // Skip the interior of a multi-line comment - it's transparent
                if info.in_html_comment || false {
                    continue;
                }
                // Skip Pandoc/Quarto div markers in Pandoc-compatible flavor - they're transparent
                if Self::is_transparent_div_marker(ctx, info) {
                    continue;
                }
                return (line_num, is_blank_in_context(info.content(ctx.content)));
            }
        }
        // Start of document = effectively blank-separated
        (0, true)
    }

    /// Find the first non-transparent line after the given line (1-indexed).
    /// Returns (line_num, is_blank) where:
    /// - line_num is the 1-indexed line of actual content (0 if end of document)
    /// - is_blank is true if that line is blank (meaning separation exists)
    ///
    /// Transparent elements (HTML comments, Quarto div markers) are skipped.
    fn find_following_content(ctx: &crate::lint_context::LintContext, after_line: usize) -> (usize, bool) {
        let num_lines = ctx.lines.len();
        for line_num in (after_line + 1)..=num_lines {
            let idx = line_num - 1;
            if let Some(info) = ctx.lines.get(idx) {
                // A line contributing nothing but comments separates the blocks
                // around it the way an empty one does (#866)
                if is_blank_in_context(info.content(ctx.content)) {
                    return (line_num, true);
                }
                // Skip the interior of a multi-line comment - it's transparent
                if info.in_html_comment || false {
                    continue;
                }
                // Skip Pandoc/Quarto div markers in Pandoc-compatible flavor - they're transparent
                if Self::is_transparent_div_marker(ctx, info) {
                    continue;
                }
                return (line_num, is_blank_in_context(info.content(ctx.content)));
            }
        }
        // End of document = effectively blank-separated
        (0, true)
    }

    /// Whether the list block's own last line contributes nothing but comments.
    ///
    /// A comment written directly below the final item is parsed into that item's
    /// segment, so it lands inside the block rather than after it. The block then
    /// ends on a line that already separates it from what follows, the same way an
    /// empty line would (#866).
    fn block_ends_in_comment_line(lines: &[&str], end_line: usize) -> bool {
        lines.get(end_line - 1).is_some_and(|line| is_blank_in_context(line))
    }

    // Shared by check() and fix(): standalone code blocks need separation,
    // while indented code belonging to the list must remain attached.
    /// The blockquote prefix for a blank line inserted after a list.
    ///
    /// The list's last line gives the prefix, keeping its marker spacing. That
    /// line can be lazy paragraph text quoted less deeply than the list, so the
    /// following line, which sits at the list's depth, gives it then.
    fn prefix_for_blank_after(
        ctx: &crate::lint_context::LintContext,
        end_line: usize,
        following_line: usize,
        list_bq_level: usize,
    ) -> String {
        let end_bq_level = ctx
            .line_info(end_line)
            .and_then(|info| info.blockquote.as_ref())
            .map_or(0, |bq| bq.nesting_level);
        let source = if end_bq_level == list_bq_level {
            end_line
        } else {
            following_line
        };
        ctx.blockquote_prefix_for_blank_line(source - 1)
    }

    fn is_following_content_excluded(ctx: &crate::lint_context::LintContext, line_num: usize, prefix: &str) -> bool {
        ctx.line_info(line_num).is_some_and(|info| {
            info.in_front_matter
                || (info.in_code_block
                    && effective_indent_in_blockquote(
                        info.content(ctx.content),
                        prefix.chars().filter(|&c| c == '>').count(),
                        info.indent,
                    ) >= 2)
        })
    }

    // Convert centralized list blocks to the format expected by perform_checks
    fn convert_list_blocks(&self, ctx: &crate::lint_context::LintContext) -> Vec<(usize, usize, String)> {
        let mut blocks: Vec<(usize, usize, String)> = Vec::new();

        for block in &ctx.list_blocks {
            // Skip list blocks inside footnote definitions
            if ctx
                .line_info(block.start_line)
                .is_some_and(|info| info.in_footnote_definition)
            {
                continue;
            }

            // For MD032, we need to check if there are code blocks that should
            // split the list into separate segments

            // Simple approach: if there's a fenced code block between list items,
            // split at that point
            let mut segments: Vec<(usize, usize)> = Vec::new();
            let mut current_start = block.start_line;
            let mut prev_item_line = 0;

            // Helper to get blockquote level (count of '>' chars) from a line
            let get_blockquote_level = |line_num: usize| -> usize {
                if line_num == 0 || line_num > ctx.lines.len() {
                    return 0;
                }
                let line_content = ctx.lines[line_num - 1].content(ctx.content);
                parse_blockquote_prefix(line_content).map_or(0, |bq| bq.nesting_level)
            };

            let mut prev_bq_level = 0;

            for &item_line in &block.item_lines {
                let current_bq_level = get_blockquote_level(item_line);

                if prev_item_line > 0 {
                    // Check if blockquote level changed between items
                    let blockquote_level_changed = prev_bq_level != current_bq_level;

                    // Check if there's a standalone code fence between prev_item_line and item_line
                    // A code fence that's indented as part of a list item should NOT split the list
                    let mut has_standalone_code_fence = false;

                    // Calculate minimum indentation for list item content
                    let min_indent_for_content = if block.is_ordered {
                        // For ordered lists, content should be indented at least to align with text after marker
                        // e.g., "1. " = 3 chars, so content should be indented 3+ spaces
                        3 // Minimum for "1. "
                    } else {
                        // For unordered lists, content should be indented at least 2 spaces
                        2 // For "- " or "* "
                    };

                    for check_line in (prev_item_line + 1)..item_line {
                        if check_line - 1 < ctx.lines.len() {
                            let line = &ctx.lines[check_line - 1];
                            let line_content = line.content(ctx.content);
                            if line.in_code_block
                                && (line_content.trim().starts_with("```") || line_content.trim().starts_with("~~~"))
                            {
                                // Check if this code fence is indented as part of the list item
                                // If it's indented enough to be part of the list item, it shouldn't split
                                if line.indent < min_indent_for_content {
                                    has_standalone_code_fence = true;
                                    break;
                                }
                            }
                        }
                    }

                    if has_standalone_code_fence || blockquote_level_changed {
                        // End current segment before this item
                        segments.push((current_start, prev_item_line));
                        current_start = item_line;
                    }
                }
                prev_item_line = item_line;
                prev_bq_level = current_bq_level;
            }

            // Add the final segment
            // For the last segment, end at the last list item (not the full block end)
            if prev_item_line > 0 {
                segments.push((current_start, prev_item_line));
            }

            // Convert segments to blocks
            for (start, end) in &segments {
                // Extend the end to include any continuation lines immediately after the last item
                let mut actual_end = *end;

                if *end < block.end_line {
                    // Get the blockquote level for this block
                    let block_bq_level = block.blockquote_prefix.chars().filter(|&c| c == '>').count();

                    // For blockquote lists, use a simpler min_continuation_indent
                    // (the content column without the blockquote prefix portion)
                    let min_continuation_indent = if block_bq_level > 0 {
                        // For lists in blockquotes, content should align with text after marker
                        if block.is_ordered {
                            block.max_marker_width
                        } else {
                            2 // "- " or "* "
                        }
                    } else {
                        ctx.lines
                            .get(*end - 1)
                            .and_then(|line_info| line_info.list_item.as_ref())
                            .map_or(2, |item| item.content_column)
                    };

                    for check_line in (*end + 1)..=block.end_line {
                        if check_line - 1 < ctx.lines.len() {
                            let line = &ctx.lines[check_line - 1];
                            let line_content = line.content(ctx.content);
                            // Stop at next list item or non-continuation content. Only a
                            // heading ends the item: `#2, #3` and other no-space `#` lines
                            // are paragraph text of the item.
                            if block.item_lines.contains(&check_line) || line.heading.is_some() {
                                break;
                            }
                            // Don't extend through code blocks
                            if line.in_code_block {
                                break;
                            }

                            // Calculate effective indent for blockquote lines
                            let effective_indent =
                                effective_indent_in_blockquote(line_content, block_bq_level, line.indent);

                            // Include indented continuation if indent meets threshold
                            if effective_indent >= min_continuation_indent {
                                actual_end = check_line;
                            }
                            // Include lazy continuation lines for structural purposes
                            // Per CommonMark, only paragraph text can be lazy continuation
                            // Thematic breaks, code fences, etc. cannot be lazy continuations
                            // Always include lazy lines in block range - the config controls whether to WARN
                            else if !line.is_blank
                                && line.heading.is_none()
                                && !block.item_lines.contains(&check_line)
                                && !is_thematic_break(line_content)
                            {
                                // This is a lazy continuation line - include it in the block range
                                actual_end = check_line;
                            } else if !line.is_blank {
                                // Non-blank line that's not a continuation - stop here
                                break;
                            }
                        }
                    }
                }

                blocks.push((*start, actual_end, block.blockquote_prefix.clone()));
            }
        }

        // Filter out lists entirely inside HTML comments
        blocks.retain(|(start, end, _)| {
            // Check if ALL lines of this block are inside HTML comments
            let all_in_comment = (*start..=*end).all(|line_num| {
                ctx.lines
                    .get(line_num - 1)
                    .is_some_and(|info| info.in_html_comment || false)
            });
            !all_in_comment
        });

        blocks
    }

    fn perform_checks(
        &self,
        ctx: &crate::lint_context::LintContext,
        lines: &[&str],
        list_blocks: &[(usize, usize, String)],
    ) -> (Vec<LintWarning>, usize) {
        let mut warnings = Vec::new();
        let num_lines = lines.len();
        let coverage = ListCoverage::new(list_blocks);

        // Check for ordered lists starting with non-1 that aren't recognized as lists
        // These need blank lines before them to be parsed as lists by CommonMark
        for (line_idx, line) in lines.iter().enumerate() {
            let line_num = line_idx + 1;

            // Skip if this line is already part of a recognized list
            if coverage.contains(line_num) {
                continue;
            }

            // Skip if in code block, front matter, or HTML comment
            if ctx.line_info(line_num).is_some_and(|info| {
                info.in_code_block
                    || info.in_front_matter
                    || info.in_html_comment
                    || false
                    || info.in_html_block
                    || false
            }) {
                continue;
            }

            // Check if this line starts with a number other than 1
            if ORDERED_LIST_NON_ONE_RE.is_match(line) {
                // Check if there's a blank line before this
                if line_idx > 0 {
                    let prev_line = lines[line_idx - 1];
                    let prev_is_blank = is_blank_in_context(prev_line);
                    let prev_line_info = ctx.line_info(line_idx);
                    let prev_excluded = prev_line_info.is_some_and(|info| info.in_code_block || info.in_front_matter);

                    // Inside a recognized MkDocs admonition or content tab body, the
                    // list-item metadata for nested lists is stale (the flavor-blind
                    // parse classifies the indented body as code), so real list items
                    // land in this fallback. Exempt an item only when an ordered list
                    // marker line at the same indent precedes it in the container run,
                    // walking over more-indented lines (wrapped continuations, deeper
                    // nesting) and stopping at any same-or-less indented non-marker
                    // line: prose there means the item cannot start a new list, which
                    // is exactly the ambiguity this fallback exists to flag. Checked
                    // directly on in_admonition/in_content_tab rather than the
                    // in_mkdocs_container() helper: that helper's third component,
                    // in_mkdocs_html_markdown, is populated in every flavor
                    // (markdown="1" HTML divs are tag-scoped, not indentation-scoped),
                    // so using it would also change behavior for standard-flavor
                    // documents.
                    let prev_in_mkdocs_container =
                        prev_line_info.is_some_and(|info| info.in_admonition || info.in_content_tab);
                    let continues_stale_container_list = prev_in_mkdocs_container && {
                        let item_indent = calculate_indentation_width_default(line);
                        let mut found_marker = false;
                        for j in (0..line_idx).rev() {
                            let in_container = ctx
                                .line_info(j + 1)
                                .is_some_and(|info| info.in_admonition || info.in_content_tab);
                            if !in_container {
                                break;
                            }
                            let candidate = lines[j];
                            if is_blank_in_context(candidate) {
                                continue;
                            }
                            let candidate_indent = calculate_indentation_width_default(candidate);
                            if crate::utils::regex_cache::ORDERED_LIST_MARKER_REGEX.is_match(candidate)
                                && candidate_indent == item_indent
                            {
                                found_marker = true;
                                break;
                            }
                            if candidate_indent <= item_indent {
                                break;
                            }
                        }
                        found_marker
                    };

                    // Check if previous line looks like a sentence continuation
                    // If the previous line is non-blank text that doesn't end with a sentence
                    // terminator, this is likely a paragraph continuation, not a list item
                    // e.g., "...in Chapter\n19. For now..." is a broken sentence, not a list
                    let prev_trimmed = prev_line.trim();
                    let is_sentence_continuation = continues_stale_container_list
                        || (!prev_is_blank
                            && !prev_trimmed.is_empty()
                            && !prev_trimmed.ends_with('.')
                            && !prev_trimmed.ends_with('!')
                            && !prev_trimmed.ends_with('?')
                            && !prev_trimmed.ends_with(':')
                            && !prev_trimmed.ends_with(';')
                            && !prev_trimmed.ends_with('>')
                            && !prev_trimmed.ends_with('-')
                            && !prev_trimmed.ends_with('*'));

                    if prev_is_blank || !is_sentence_continuation {
                        if !prev_is_blank && !prev_excluded {
                            // This ordered list item starting with non-1 needs a blank line before it
                            let (start_line, start_col, end_line, end_col) = calculate_line_range(line_num, line);

                            let bq_prefix = ctx.blockquote_prefix_for_blank_line(line_idx);
                            warnings.push(LintWarning {
                                line: start_line,
                                column: start_col,
                                end_line,
                                end_column: end_col,
                                severity: Severity::Warning,
                                rule_name: Some(self.name().to_string()),
                                message: "Ordered list starting with non-1 should be preceded by blank line"
                                    .to_string(),
                                fix: Some(Fix::new(
                                    ctx.line_column_byte_range_with_length(line_num, 1, 0),
                                    format!("{bq_prefix}\n"),
                                )),
                            });
                        }

                        // Also check if a blank line is needed AFTER this ordered list item
                        // This ensures single-pass idempotency
                        if line_idx + 1 < num_lines {
                            let next_line = lines[line_idx + 1];
                            let next_is_blank = is_blank_in_context(next_line);
                            let next_excluded = ctx.line_info(line_idx + 2).is_some_and(|info| info.in_front_matter);

                            if !next_is_blank && !next_excluded && !next_line.trim().is_empty() {
                                // Check if next line is a continuation of this ordered list
                                // Only other ordered items or indented continuations count;
                                // unordered list markers are a different list requiring separation
                                let next_trimmed = next_line.trim_start();
                                let next_is_ordered_content = ORDERED_LIST_NON_ONE_RE.is_match(next_line)
                                    || next_line.starts_with("1. ")
                                    || (next_line.len() > next_trimmed.len()
                                        && !next_trimmed.starts_with("- ")
                                        && !next_trimmed.starts_with("* ")
                                        && !next_trimmed.starts_with("+ ")); // indented continuation (not a nested unordered list)

                                if !next_is_ordered_content {
                                    let (start_line, start_col, end_line, end_col) =
                                        calculate_line_range(line_num, line);
                                    let bq_prefix = ctx.blockquote_prefix_for_blank_line(line_idx);
                                    warnings.push(LintWarning {
                                        line: start_line,
                                        column: start_col,
                                        end_line,
                                        end_column: end_col,
                                        severity: Severity::Warning,
                                        rule_name: Some(self.name().to_string()),
                                        message: "List should be followed by blank line".to_string(),
                                        fix: Some(Fix::new(
                                            ctx.line_column_byte_range_with_length(line_num + 1, 1, 0),
                                            format!("{bq_prefix}\n"),
                                        )),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // The warnings above turn paragraph text into a list, which is their
        // point; the ones below separate lists that already parse as lists.
        let separating_from = warnings.len();

        for &(start_line, end_line, ref prefix) in list_blocks {
            let block_bq_level = prefix.chars().filter(|&c| c == '>').count();
            // Skip lists that start inside HTML/MDX comments
            if ctx
                .line_info(start_line)
                .is_some_and(|info| info.in_html_comment || false)
            {
                continue;
            }

            if start_line > 1 {
                // Look past HTML comments to find actual preceding content
                let (content_line, has_blank_separation) = Self::find_preceding_content(ctx, start_line);

                // If blank separation exists (through HTML comments), no warning needed
                if !has_blank_separation && content_line > 0 {
                    let prev_line_str = lines[content_line - 1];
                    let is_prev_excluded = ctx
                        .line_info(content_line)
                        .is_some_and(|info| info.in_code_block || info.in_front_matter);
                    let prev_bq_level = parse_blockquote_prefix(prev_line_str).map_or(0, |bq| bq.nesting_level);
                    let prefixes_match = prev_bq_level == block_bq_level;

                    // Only require blank lines for content in the same context (same blockquote level)
                    // and when the context actually requires it
                    let should_require = Self::should_require_blank_line_before(ctx, content_line, start_line);
                    if !is_prev_excluded && prefixes_match && should_require {
                        // Calculate precise character range for the entire list line that needs a blank line before it
                        let (start_line, start_col, end_line, end_col) =
                            calculate_line_range(start_line, lines[start_line - 1]);

                        warnings.push(LintWarning {
                            line: start_line,
                            column: start_col,
                            end_line,
                            end_column: end_col,
                            severity: Severity::Warning,
                            rule_name: Some(self.name().to_string()),
                            message: "List should be preceded by blank line".to_string(),
                            fix: Some(Fix::new(
                                ctx.line_column_byte_range_with_length(start_line, 1, 0),
                                format!("{}\n", ctx.blockquote_prefix_for_blank_line(start_line - 1)),
                            )),
                        });
                    }
                }
            }

            if end_line < num_lines && !Self::block_ends_in_comment_line(lines, end_line) {
                // Look past HTML comments to find actual following content
                let (content_line, has_blank_separation) = Self::find_following_content(ctx, end_line);

                // If blank separation exists (through HTML comments), no warning needed
                if !has_blank_separation && content_line > 0 {
                    let next_line_str = lines[content_line - 1];
                    // Check if next line is excluded - front matter or indented code blocks within lists
                    // We want blank lines before standalone code blocks, but not within list items
                    let is_next_excluded = Self::is_following_content_excluded(ctx, content_line, prefix);
                    let next_line_bq_level = parse_blockquote_prefix(next_line_str).map_or(0, |bq| bq.nesting_level);

                    // Check blockquote levels to detect boundary transitions
                    // If the list ends inside a blockquote but the following line exits the blockquote
                    // (fewer > chars in prefix), no blank line is needed - the blockquote boundary
                    // provides semantic separation
                    let end_line_str = lines[end_line - 1];
                    let end_line_bq_level = parse_blockquote_prefix(end_line_str).map_or(0, |bq| bq.nesting_level);
                    let exits_blockquote = end_line_bq_level > 0 && next_line_bq_level < end_line_bq_level;

                    let prefixes_match = next_line_bq_level == block_bq_level;

                    // Only require blank lines for content in the same context (same blockquote level)
                    // Skip if the following line exits a blockquote - boundary provides separation
                    if !is_next_excluded && prefixes_match && !exits_blockquote {
                        // Calculate precise character range for the last line of the list (not the line after)
                        let (start_line_last, start_col_last, end_line_last, end_col_last) =
                            calculate_line_range(end_line, lines[end_line - 1]);

                        warnings.push(LintWarning {
                            line: start_line_last,
                            column: start_col_last,
                            end_line: end_line_last,
                            end_column: end_col_last,
                            severity: Severity::Warning,
                            rule_name: Some(self.name().to_string()),
                            message: "List should be followed by blank line".to_string(),
                            fix: Some(Fix::new(
                                ctx.line_column_byte_range_with_length(end_line + 1, 1, 0),
                                format!(
                                    "{}\n",
                                    Self::prefix_for_blank_after(ctx, end_line, content_line, block_bq_level)
                                ),
                            )),
                        });
                    }
                }
            }
        }
        (warnings, separating_from)
    }
}

impl Rule for MD032BlanksAroundLists {
    fn name(&self) -> &'static str {
        "MD032"
    }

    fn description(&self) -> &'static str {
        "Lists should be surrounded by blank lines"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let (mut warnings, separating_from) = self.unguarded_warnings(ctx);
        drop_structure_changing_fixes(ctx, &mut warnings[separating_from..], Allowed::Nothing);
        Ok(warnings)
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        Ok(self.fix_with_structure_impl(ctx, &self.withheld_fixes(ctx)))
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Skip if no list blocks exist (includes ordered and unordered lists)
        // Note: list_blocks is pre-computed in LintContext, so this is already efficient
        ctx.content.is_empty() || ctx.list_blocks.is_empty()
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::List
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl MD032BlanksAroundLists {
    /// Every warning with its fix as computed, and the index from which the
    /// warnings separate lists that already parse as lists.
    fn unguarded_warnings(&self, ctx: &crate::lint_context::LintContext) -> (Vec<LintWarning>, usize) {
        let lines = ctx.raw_lines();
        // Early return for empty content
        if lines.is_empty() {
            return (Vec::new(), 0);
        }

        let list_blocks = self.convert_list_blocks(ctx);

        if list_blocks.is_empty() {
            return (Vec::new(), 0);
        }

        let (mut warnings, separating_from) = self.perform_checks(ctx, lines, &list_blocks);

        // When lazy continuation is not allowed, detect and warn about lazy continuation
        // lines WITHIN list blocks (text that continues a list item but with less
        // indentation than expected). Lazy continuation at the END of list blocks is
        // already handled by the segment extension logic above.
        if !self.config.allow_lazy_continuation {
            let lazy_cont_lines = ctx.lazy_continuation_lines();
            let coverage = ListCoverage::new(&list_blocks);

            for lazy_info in lazy_cont_lines.iter() {
                let line_num = lazy_info.line_num;

                // Only warn about lazy continuation lines that are WITHIN a list block
                // (i.e., between list items). End-of-block lazy continuation is already
                // handled by the existing "list should be followed by blank line" logic.
                if !Self::is_reportable_lazy_line(ctx, &coverage, line_num) {
                    continue;
                }

                // Get the expected indent for context in the warning message
                let line_content = lines.get(line_num.saturating_sub(1)).unwrap_or(&"");
                let (start_line, start_col, end_line, end_col) = calculate_line_range(line_num, line_content);

                // Calculate fix: add proper indentation to the lazy continuation line
                let fix = if Self::should_apply_lazy_fix(ctx, line_num) {
                    Self::calculate_lazy_continuation_fix(ctx, line_num, lazy_info)
                } else {
                    None
                };

                warnings.push(LintWarning {
                    line: start_line,
                    column: start_col,
                    end_line,
                    end_column: end_col,
                    severity: Severity::Warning,
                    rule_name: Some(self.name().to_string()),
                    message: "Lazy continuation line should be properly indented or preceded by blank line".to_string(),
                    fix,
                });
            }
        }

        (warnings, separating_from)
    }

    /// The fixes the structure guard withholds, as the line each one edits and
    /// whether it inserts a blank line before that line (else it re-indents a
    /// lazy continuation line). The fix path builds its own edits, so it reads
    /// the guard's verdict from the warnings.
    fn withheld_fixes(&self, ctx: &crate::lint_context::LintContext) -> std::collections::HashSet<(usize, bool)> {
        let (unguarded, separating_from) = self.unguarded_warnings(ctx);
        let mut guarded = unguarded[separating_from..].to_vec();
        drop_structure_changing_fixes(ctx, &mut guarded, Allowed::Nothing);
        unguarded[separating_from..]
            .iter()
            .zip(&guarded)
            .filter(|(_, after)| after.fix.is_none())
            .filter_map(|(before, _)| before.fix.as_ref())
            .map(|fix| {
                let line = ctx.offset_to_line_col(fix.range.start).0;
                (line, fix.range.is_empty() && fix.replacement.ends_with('\n'))
            })
            .collect()
    }

    /// Helper method for fixing implementation
    fn fix_with_structure_impl(
        &self,
        ctx: &crate::lint_context::LintContext,
        withheld: &std::collections::HashSet<(usize, bool)>,
    ) -> String {
        let lines = ctx.raw_lines();
        let num_lines = lines.len();
        if num_lines == 0 {
            return String::new();
        }

        let list_blocks = self.convert_list_blocks(ctx);
        if list_blocks.is_empty() {
            return ctx.content.to_string();
        }

        // Phase 0: Collect lazy continuation line fixes (if not allowed)
        // Map of line_num -> LazyContLine for applying fixes
        let mut lazy_fixes: std::collections::BTreeMap<usize, LazyContLine> = std::collections::BTreeMap::new();
        if !self.config.allow_lazy_continuation {
            let lazy_cont_lines = ctx.lazy_continuation_lines();
            let coverage = ListCoverage::new(&list_blocks);
            for lazy_info in lazy_cont_lines.iter() {
                let line_num = lazy_info.line_num;
                // Only fix lines within a list block
                if !Self::is_reportable_lazy_line(ctx, &coverage, line_num) {
                    continue;
                }
                // Only fix if not in code block, front matter, or HTML comment
                if !Self::should_apply_lazy_fix(ctx, line_num) {
                    continue;
                }
                if !withheld.contains(&(line_num, false)) {
                    lazy_fixes.insert(line_num, lazy_info.clone());
                }
            }
        }

        let mut insertions: std::collections::BTreeMap<usize, String> = std::collections::BTreeMap::new();

        // Phase 1: Identify needed insertions
        for &(start_line, end_line, ref prefix) in &list_blocks {
            let block_bq_level = prefix.chars().filter(|&c| c == '>').count();
            // Skip lists that start inside HTML/MDX comments
            if ctx
                .line_info(start_line)
                .is_some_and(|info| info.in_html_comment || false)
            {
                continue;
            }

            // Check before block
            if start_line > 1 {
                // Look past HTML comments to find actual preceding content
                let (content_line, has_blank_separation) = Self::find_preceding_content(ctx, start_line);

                // If blank separation exists (through HTML comments), no fix needed
                if !has_blank_separation && content_line > 0 {
                    let prev_line_str = lines[content_line - 1];
                    let is_prev_excluded = ctx
                        .line_info(content_line)
                        .is_some_and(|info| info.in_code_block || info.in_front_matter);
                    let prev_bq_level = parse_blockquote_prefix(prev_line_str).map_or(0, |bq| bq.nesting_level);

                    let should_require = Self::should_require_blank_line_before(ctx, content_line, start_line);
                    // Compare depth so compact and spaced markers share the same context.
                    if !is_prev_excluded && prev_bq_level == block_bq_level && should_require {
                        // Use centralized helper for consistent blockquote prefix (no trailing space)
                        let bq_prefix = ctx.blockquote_prefix_for_blank_line(start_line - 1);
                        if !withheld.contains(&(start_line, true)) {
                            insertions.insert(start_line, bq_prefix);
                        }
                    }
                }
            }

            // Check after block
            if end_line < num_lines && !Self::block_ends_in_comment_line(lines, end_line) {
                // Look past HTML comments to find actual following content
                let (content_line, has_blank_separation) = Self::find_following_content(ctx, end_line);

                // If blank separation exists (through HTML comments), no fix needed
                if !has_blank_separation && content_line > 0 {
                    let next_line_str = lines[content_line - 1];
                    // Match check(): standalone code blocks need separation, but
                    // indented code inside a list item must stay attached to it.
                    let is_next_excluded = Self::is_following_content_excluded(ctx, content_line, prefix);
                    let next_line_bq_level = parse_blockquote_prefix(next_line_str).map_or(0, |bq| bq.nesting_level);

                    // Check blockquote levels to detect boundary transitions
                    let end_line_str = lines[end_line - 1];
                    let end_line_bq_level = parse_blockquote_prefix(end_line_str).map_or(0, |bq| bq.nesting_level);
                    let exits_blockquote = end_line_bq_level > 0 && next_line_bq_level < end_line_bq_level;

                    // Compare depth so compact and spaced markers share the same context.
                    // Skip if exiting a blockquote - boundary provides separation
                    if !is_next_excluded && next_line_bq_level == block_bq_level && !exits_blockquote {
                        // Use centralized helper for consistent blockquote prefix (no trailing space)
                        let bq_prefix = Self::prefix_for_blank_after(ctx, end_line, content_line, block_bq_level);
                        if !withheld.contains(&(end_line + 1, true)) {
                            insertions.insert(end_line + 1, bq_prefix);
                        }
                    }
                }
            }
        }

        if insertions.is_empty() && lazy_fixes.is_empty() {
            return ctx.content.to_string();
        }

        // Phase 2: Reconstruct with insertions and lazy fixes
        let mut result_lines: Vec<String> = Vec::with_capacity(num_lines + insertions.len());
        for (i, line) in lines.iter().enumerate() {
            let current_line_num = i + 1;
            if let Some(prefix_to_insert) = insertions.get(&current_line_num)
                && (result_lines.is_empty() || result_lines.last().unwrap() != prefix_to_insert)
            {
                result_lines.push(prefix_to_insert.clone());
            }

            // Disabled lines were excluded when collecting fixes.
            if let Some(lazy_info) = lazy_fixes.get(&current_line_num) {
                let fixed_line = Self::apply_lazy_fix_to_line(line, lazy_info);
                result_lines.push(fixed_line);
            } else {
                result_lines.push(line.to_string());
            }
        }

        // Preserve the final newline if the original content had one
        let line_ending = crate::utils::detect_line_ending(ctx.content);
        let mut result = result_lines.join(line_ending);
        if ctx.content.ends_with('\n') {
            result.push_str(line_ending);
        }
        result
    }
}

// Checks if a line is blank, considering compact and spaced blockquote markers.
// A line contributing nothing but HTML comments counts as blank too (#866).
fn is_blank_in_context(line: &str) -> bool {
    parse_blockquote_prefix(line)
        .map_or(line, |bq| bq.content)
        .trim()
        .is_empty()
        || crate::utils::blank_lines::is_blank_or_comment_only(line)
}
