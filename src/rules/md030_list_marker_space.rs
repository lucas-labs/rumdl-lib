//!
//! Rule MD030: Spaces after list markers
//!
//! See [docs/md030.md](../../docs/md030.md) for full documentation, configuration, and examples.

use crate::rule::{LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::blockquote::parse_blockquote_prefix;
use crate::utils::calculate_indentation_width_default;
use crate::utils::list_fix_guard::{Allowed, drop_structure_changing_fixes};
use crate::utils::list_indent_shift::{
    Continuation, EndedItems, Nesting, OwnerFrame, classify_continuation, close_ended_items, continuation_params,
    keep_ended_items_closed, move_owned_line,
};
use crate::utils::range_utils::calculate_match_range;

mod md030_config;
pub use md030_config::MD030Config;

#[derive(Clone, Default)]
pub struct MD030ListMarkerSpace {
    config: MD030Config,
}

impl MD030ListMarkerSpace {}

impl Rule for MD030ListMarkerSpace {
    fn name(&self) -> &'static str {
        "MD030"
    }

    fn description(&self) -> &'static str {
        "Spaces after list markers should be consistent"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = Vec::new();

        // Early return if no list content
        if self.should_skip(ctx) {
            return Ok(warnings);
        }

        let lines = ctx.raw_lines();

        // Track which lines we've already processed (to avoid duplicates)
        let mut processed_lines = std::collections::HashSet::new();

        // Re-spacing a marker moves its item's content column, and every line the
        // item owns has to move with it or the document changes meaning: a wider
        // `1.` marker leaves its `   1. inner` child under-indented and flattened,
        // and a narrower one leaves a nested fence four columns past the new
        // content column, where it turns into an indented code block. The moves
        // ride on the re-spacing warning's own fix (see `list_indent_shift`).
        //
        // Active list items (and inline bullets) whose content the current line may
        // continue. Because the loop walks top to bottom, every owning item is
        // already on the stack by the time we reach its content.
        let mut stack: Vec<OwnerFrame> = Vec::new();
        // The first warning of the outermost list being walked, and whether a
        // narrowed item in it would take in a line that cannot move out of it.
        // Its items shift only against each other, so a list that cannot be fixed
        // leaves the lists around it fixable.
        let mut list_warnings = 0;
        let mut unfixable = false;

        // Main pass: move each continuation/nested line with the items that own it
        // as the loop reaches it, and check parser-recognized list items.
        for (line_num, line_info) in ctx.lines.iter().enumerate() {
            let line_num_1based = line_num + 1;
            let line = lines[line_num];

            // Drop the items this line ends. A lazy continuation line ends nothing
            // and stays where it is.
            let before = stack.clone();
            let lazy = close_ended_items(ctx, line_num_1based, lines, &mut stack);

            // Skip code blocks, math blocks, PyMdown blocks, and MkDocs markdown HTML divs (grid cards use custom spacing)
            let is_list_item = line_info.list_item.is_some()
                && !line_info.in_code_block
                && !line_info.in_math_block
                && !line_info.in_mkdocs_html_markdown
                && !line_info.in_footnote_definition;

            if !is_list_item {
                // A continuation/nested line moves with the items that own it, and a
                // line that ends items keeps out of them once they narrow.
                if !lazy && !line.trim().is_empty() {
                    let mut owners = stack.clone();
                    match keep_ended_items_closed(ctx, line_num, &before, stack.len(), Nesting::Relative) {
                        EndedItems::StayClosed => {}
                        EndedItems::ShiftOut(frame) => owners.push(frame),
                        EndedItems::Unfixable => unfixable = true,
                    }
                    move_owned_line(ctx, line, line_num, &owners, &mut warnings);
                }
                continue;
            }

            processed_lines.insert(line_num_1based);
            let Some(list_info) = &line_info.list_item else {
                continue;
            };

            if stack.is_empty() {
                if std::mem::take(&mut unfixable) {
                    Self::decline_fixes(&mut warnings[list_warnings..]);
                }
                list_warnings = warnings.len();
            }

            // The item is content of its parent, so its leading indent moves too.
            move_owned_line(ctx, line, line_num, &stack, &mut warnings);

            let marker_end = list_info.marker_column + list_info.marker.len();

            // MD030 only applies when there is content after the marker.
            if !Self::has_content_after_marker(line, marker_end) {
                continue;
            }

            let actual_spaces = list_info.content_column.saturating_sub(marker_end);

            // Spacing comes from the shared MD030 config logic: the ol-align-column
            // override for ordered lists, otherwise the fixed single-/multi-line value.
            let is_multi_line = self.is_multi_line_list_item(ctx, line_num_1based, lines);
            let expected_spaces =
                self.config
                    .expected_spaces(list_info.is_ordered, is_multi_line, list_info.marker.len());

            let warning = (actual_spaces != expected_spaces).then(|| {
                warnings.push(self.spacing_fix_warning(
                    ctx,
                    line,
                    line_num,
                    marker_end..marker_end + actual_spaces,
                    expected_spaces,
                    format!("Spaces after list markers (Expected: {expected_spaces}; Actual: {actual_spaces})"),
                ));
                warnings.len() - 1
            });

            // Push this item's frame so the lines it owns move with it, and space any
            // inline nested bullet (`1. - x`), which gets its own frame.
            let own = expected_spaces as isize - actual_spaces as isize;
            if let Some(frame) = OwnerFrame::for_item(ctx, line_num_1based, own, warning) {
                stack.push(frame);
                if list_info.is_ordered {
                    self.align_inline_bullet(
                        ctx,
                        line_num_1based,
                        lines,
                        list_info.content_column,
                        &mut stack,
                        &mut warnings,
                    );
                }
            }
        }

        if unfixable {
            Self::decline_fixes(&mut warnings[list_warnings..]);
        }
        // Re-spacing must not change the structure; the pass below exists to
        // create lists, so it is not held to that.
        drop_structure_changing_fixes(ctx, &mut warnings, Allowed::Nothing);

        // Second pass: Detect list-like patterns the parser didn't recognize
        // This handles cases like "1.Text" where there's no space after the marker
        for (line_idx, line) in lines.iter().enumerate() {
            let line_num = line_idx + 1;

            // Skip if already processed or in code block/front matter/math block
            if processed_lines.contains(&line_num) {
                continue;
            }
            if let Some(line_info) = ctx.lines.get(line_idx)
                && (line_info.in_code_block
                    || line_info.in_front_matter
                    || line_info.in_html_comment
                    || line_info.in_math_block
                    || line_info.in_mkdocs_html_markdown
                    || line_info.in_footnote_definition)
            {
                continue;
            }

            // Skip indented code blocks
            if self.is_indented_code_block(line, line_idx, lines) {
                continue;
            }

            // Try to detect list-like patterns using regex-based detection
            if let Some(warning) = self.check_unrecognized_list_marker(ctx, line, line_num, lines) {
                warnings.push(warning);
            }
        }

        Ok(warnings)
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::List
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        if ctx.content.is_empty() {
            return true;
        }

        // Fast byte-level check for list markers (including ordered lists)
        let bytes = ctx.content.as_bytes();
        !bytes.contains(&b'*')
            && !bytes.contains(&b'-')
            && !bytes.contains(&b'+')
            && !bytes.iter().any(|&b| b.is_ascii_digit())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, crate::rule::LintError> {
        if self.should_skip(ctx) {
            return Ok(ctx.content.to_string());
        }

        // Derive fixes directly from check() so detection and fixing share one code path.
        // This guarantees that every violation check() reports is also fixed, with no
        // possibility of the two paths diverging due to mismatched skip conditions.
        let warnings = self.check(ctx)?;
        if warnings.is_empty() {
            return Ok(ctx.content.to_string());
        }

        let warnings = warnings;

        crate::utils::fix_utils::apply_warning_fixes(ctx.content, &warnings)
            .map_err(crate::rule::LintError::InvalidInput)
    }
}

impl MD030ListMarkerSpace {
    /// Check if a list item line has content after the marker
    /// Returns false if the line ends after the marker (with optional whitespace)
    /// MD030 only applies when there IS content on the same line as the marker
    #[inline]
    /// Drop the fixes of one list's warnings. Re-spacing only some of its items
    /// would shift them against the rest and change the nesting, so the list's
    /// spacing fixes are kept or dropped together.
    fn decline_fixes(warnings: &mut [LintWarning]) {
        for warning in warnings {
            warning.fix = None;
        }
    }

    fn has_content_after_marker(line: &str, marker_end: usize) -> bool {
        if marker_end >= line.len() {
            return false;
        }
        !line[marker_end..].trim().is_empty()
    }

    /// Build a warning that replaces the whitespace run at byte range `span` within
    /// the line at `line_idx` (0-based) with `want` spaces. Shared by the marker-
    /// spacing checks and the nested-content re-indentation.
    fn spacing_fix_warning(
        &self,
        ctx: &crate::lint_context::LintContext,
        line: &str,
        line_idx: usize,
        span: std::ops::Range<usize>,
        want: usize,
        message: String,
    ) -> LintWarning {
        let (start_line, start_col, end_line, end_col) =
            calculate_match_range(line_idx + 1, line, span.start, span.len());
        let base = ctx.line_offsets.get(line_idx).copied().unwrap_or(0);
        LintWarning {
            rule_name: Some(self.name().to_string()),
            severity: Severity::Warning,
            line: start_line,
            column: start_col,
            end_line,
            end_column: end_col,
            message,
            fix: Some(crate::rule::Fix::new(
                base + span.start..base + span.end,
                " ".repeat(want),
            )),
        }
    }

    /// If the content at byte `content_col` of `line` is an inline unordered marker
    /// (`-`/`*`/`+` followed by spaces and then content, as in `1. - x`), return the
    /// byte offset of the spaces after that bullet and their current count. `None`
    /// for anything else (e.g. `1. -x` or `1. *emphasis*`, which aren't markers).
    fn inline_unordered_spaces(line: &str, content_col: usize) -> Option<(usize, usize)> {
        if !matches!(line.as_bytes().get(content_col), Some(b'-' | b'*' | b'+')) {
            return None;
        }
        let offset = content_col + 1;
        let rest = line.get(offset..)?;
        let spaces = rest.len() - rest.trim_start_matches(' ').len();
        if spaces == 0 || rest[spaces..].is_empty() {
            return None;
        }
        Some((offset, spaces))
    }

    /// Whether the list item on `line_num` spans multiple lines *as written* (it has
    /// continuation or nested content).
    ///
    /// Maintainer note: MD013's list reflow needs the multi-line shape of the
    /// *rewritten* item instead — plain prose continuation collapses onto the marker
    /// line during reflow — so it deliberately does not reuse this and derives its own
    /// `is_multi` from the post-reflow shape (see `md013_line_length.rs`). The two are
    /// related but technically distinct: this reflects the current text, MD013's the
    /// predicted output. The spacing *policy* they share lives in
    /// [`MD030Config::expected_spaces`]; if the meaning of "multi-line" changes here,
    /// check whether MD013's reflow prediction needs the matching change.
    fn is_multi_line_list_item(&self, ctx: &crate::lint_context::LintContext, line_num: usize, lines: &[&str]) -> bool {
        let Some((marker_column, bq_level, min_indent)) = continuation_params(ctx, line_num) else {
            return false;
        };
        Self::has_continuation(ctx, line_num, lines, marker_column, bq_level, min_indent)
    }

    /// Whether any line after `line_num` (1-based) belongs to an item with the given
    /// continuation threshold, scanning until the item ends. Shared by the multi-line
    /// check and the inline-bullet check.
    fn has_continuation(
        ctx: &crate::lint_context::LintContext,
        line_num: usize,
        lines: &[&str],
        marker_column: usize,
        bq_level: usize,
        min_indent: usize,
    ) -> bool {
        for next in (line_num + 1)..=lines.len() {
            match classify_continuation(ctx, next, lines, marker_column, bq_level, min_indent) {
                Continuation::Belongs => return true,
                Continuation::Ends => break,
                Continuation::Skip => {}
            }
        }
        false
    }

    /// The first item of a nested unordered list shares the ordered marker's line
    /// (`1. - x`), where the parser exposes only the outer marker. Space that inline
    /// bullet like a sibling bullet on its own line, and push a frame so its own
    /// continuation lines move with it.
    fn align_inline_bullet(
        &self,
        ctx: &crate::lint_context::LintContext,
        line_num: usize,
        lines: &[&str],
        content_column: usize,
        stack: &mut Vec<OwnerFrame>,
        warnings: &mut Vec<LintWarning>,
    ) {
        let line = lines[line_num - 1];
        let Some((offset, spaces)) = Self::inline_unordered_spaces(line, content_column) else {
            return;
        };
        let bullet_content_col = offset + spaces;
        // ul-multi if the bullet itself spans lines, else ul-single, measured with a
        // raw indent (bq_level 0) like a bullet that begins its own line. The inline
        // bullet is always unordered (marker width 1).
        let multi = Self::has_continuation(ctx, line_num, lines, content_column, 0, bullet_content_col);
        let want = self.config.expected_spaces(false, multi, 1);
        if spaces == want {
            return;
        }
        warnings.push(self.spacing_fix_warning(
            ctx,
            line,
            line_num - 1,
            offset..offset + spaces,
            want,
            format!("Spaces after list markers (Expected: {want}; Actual: {spaces})"),
        ));
        stack.push(OwnerFrame {
            marker_column: content_column,
            bq_level: 0,
            min_indent: bullet_content_col,
            own: want as isize - spaces as isize,
            warning: Some(warnings.len() - 1),
        });
    }

    /// Detect list-like patterns that the parser didn't recognize (e.g., "1.Text" with no space)
    /// This implements user-intention-based detection: if it looks like a list item, flag it
    fn check_unrecognized_list_marker(
        &self,
        ctx: &crate::lint_context::LintContext,
        line: &str,
        line_num: usize,
        lines: &[&str],
    ) -> Option<LintWarning> {
        // Strip blockquote prefix to analyze the content.
        // Track the prefix length so fix positions are relative to the original line.
        let (bq_prefix_len, content) = match parse_blockquote_prefix(line) {
            Some(parsed) => (parsed.prefix.len(), parsed.content),
            None => (0, line),
        };

        let trimmed = content.trim_start();
        let indent_len = content.len() - trimmed.len();

        // Note: We intentionally do NOT apply heuristic detection to unordered list markers
        // (*, -, +) because they have too many non-list uses: emphasis, globs, diffs, etc.
        // The parser handles valid unordered list items; we only do heuristic detection
        // for ordered lists where "1.Text" is almost always a list item with missing space.

        // Check for ordered list markers (digits followed by .) without proper spacing
        if let Some(dot_pos) = trimmed.find('.') {
            let before_dot = &trimmed[..dot_pos];
            if before_dot.chars().all(|c| c.is_ascii_digit()) && !before_dot.is_empty() {
                let after_dot = &trimmed[dot_pos + 1..];
                // Only flag if there's content directly after the marker (no space, no tab)
                if !after_dot.is_empty() && !after_dot.starts_with(' ') && !after_dot.starts_with('\t') {
                    let first_char = after_dot.chars().next().unwrap_or(' ');

                    // For CLEAR user intent, only flag if:
                    // 1. Starts with uppercase letter (strong list indicator), OR
                    // 2. Starts with [ or ( (link/paren content)
                    // Lowercase and digits are ambiguous (could be decimal, version, etc.)
                    let is_clear_intent = first_char.is_ascii_uppercase() || first_char == '[' || first_char == '(';

                    if is_clear_intent {
                        let is_multi_line = self.is_multi_line_for_unrecognized(line_num, lines);
                        // Ordered marker `<digits>.` has width `before_dot.len() + 1`.
                        let expected_spaces = self.config.expected_spaces(true, is_multi_line, before_dot.len() + 1);

                        let marker = format!("{before_dot}.");
                        let marker_pos = indent_len;
                        let marker_end = marker_pos + marker.len();
                        // Offset from the start of the original line (including blockquote prefix).
                        let offset_in_line = bq_prefix_len + marker_end;

                        let (start_line, start_col, end_line, end_col) =
                            calculate_match_range(line_num, line, offset_in_line, 0);

                        let correct_spaces = " ".repeat(expected_spaces);
                        let line_start_byte = ctx.line_offsets.get(line_num - 1).copied().unwrap_or(0);
                        let fix_position = line_start_byte + offset_in_line;

                        return Some(LintWarning {
                            rule_name: Some("MD030".to_string()),
                            severity: Severity::Warning,
                            line: start_line,
                            column: start_col,
                            end_line,
                            end_column: end_col,
                            message: format!("Spaces after list markers (Expected: {expected_spaces}; Actual: 0)"),
                            fix: Some(crate::rule::Fix::new(fix_position..fix_position, correct_spaces)),
                        });
                    }
                }
            }
        }

        None
    }

    /// Simplified multi-line check for unrecognized list items
    fn is_multi_line_for_unrecognized(&self, line_num: usize, lines: &[&str]) -> bool {
        // For unrecognized list items, we can't rely on parser info
        // Check if the next line exists and appears to be a continuation
        if line_num < lines.len() {
            let next_line = lines[line_num]; // line_num is 1-based, so this is the next line
            let next_trimmed = next_line.trim();
            // If next line is non-empty and indented, it might be a continuation
            if !next_trimmed.is_empty() && next_line.starts_with(' ') {
                return true;
            }
        }
        false
    }

    /// Check if a line is part of an indented code block (4+ columns with blank line before)
    fn is_indented_code_block(&self, line: &str, line_idx: usize, lines: &[&str]) -> bool {
        // Must have 4+ columns of indentation (accounting for tab expansion)
        if calculate_indentation_width_default(line) < 4 {
            return false;
        }

        // If it's the first line, it's not an indented code block
        if line_idx == 0 {
            return false;
        }

        // Check if there's a blank line before this line or before the start of the indented block
        if self.has_blank_line_before_indented_block(line_idx, lines) {
            return true;
        }

        false
    }

    /// Check if there's a blank line before the start of an indented block
    fn has_blank_line_before_indented_block(&self, line_idx: usize, lines: &[&str]) -> bool {
        // Walk backwards to find the start of the indented block
        let mut current_idx = line_idx;

        // Find the first line in this indented block
        while current_idx > 0 {
            let current_line = lines[current_idx];
            let prev_line = lines[current_idx - 1];

            // If current line is not indented (< 4 columns), we've gone too far
            if calculate_indentation_width_default(current_line) < 4 {
                break;
            }

            // If previous line is not indented, check if it's blank
            if calculate_indentation_width_default(prev_line) < 4 {
                return prev_line.trim().is_empty();
            }

            current_idx -= 1;
        }

        false
    }
}
