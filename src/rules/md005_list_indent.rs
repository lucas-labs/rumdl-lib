//!
//! Rule MD005: Inconsistent indentation for list items at the same level
//!
//! See [docs/md005.md](../../docs/md005.md) for full documentation, configuration, and examples.

use crate::utils::blockquote::effective_indent_in_blockquote;
use crate::utils::list_fix_guard::{Allowed, drop_structure_changing_fixes};
use crate::utils::list_indent_shift::{Nesting, move_owned_lines};
use crate::utils::range_utils::calculate_match_range;

use crate::lint_context::{ParsedListBlock, ParsedListBlocks, ParsedListItem};
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
// No regex patterns needed for this rule
use std::cell::RefCell;
use std::collections::HashMap;

/// Type alias for parent content column groups, keyed by (parent_col, is_ordered).
/// Used by `group_by_parent_content_column` to separate ordered and unordered items.
type ParentContentGroups<'a> = HashMap<(usize, bool), Vec<(usize, usize, &'a crate::lint_context::LineInfo)>>;

/// Rule MD005: Inconsistent indentation for list items at the same level
#[derive(Clone, Default)]
pub struct MD005ListIndent {
    /// Expected indentation for top-level lists (from MD007 config)
    top_level_indent: usize,
}

/// Cache for fast line information lookups to avoid O(n²) scanning
struct LineCacheInfo {
    /// Indentation level for each line (0 for empty lines)
    indentation: Vec<usize>,
    /// Blockquote nesting level for each line (0 for non-blockquote lines)
    blockquote_levels: Vec<usize>,
    /// Line content references for blockquote-aware indent calculation
    line_contents: Vec<String>,
    /// Bit flags: bit 0 = has_content, bit 1 = is_list_item, bit 2 = is_continuation_content
    flags: Vec<u8>,
    /// Parent list item line number for each list item (1-indexed, 0 = no parent)
    /// Pre-computed in O(n) to avoid O(n²) backward scanning
    parent_map: HashMap<usize, usize>,
    /// For each list item (1-indexed), the first later line holding a list item
    /// at or left of its marker column, which ends the item; absent when nothing
    /// does before the end of the document. Every question asked about a parent's
    /// continuation content concerns lines before this one.
    item_end: HashMap<usize, usize>,
    /// Memoized [`Self::continuation_decided`] results, keyed by the parent line
    /// and the thresholds and blockquote context the scan used.
    decided: RefCell<HashMap<ScanKey, Option<Decision>>>,
    /// Memoized [`Self::first_item_at_column`] results, keyed by
    /// `(parent_line, from_line, marker_column)`.
    first_item_at: RefCell<HashMap<(usize, usize, usize), Option<usize>>>,
}

/// `(parent_line, tight_threshold, loose_threshold, parent_bq_level, parent_bq_prefix_len)`
type ScanKey = (usize, usize, usize, usize, usize);

/// The line (1-indexed) that settles whether a parent item has continuation
/// content, and the answer: the continuation line's indent, or `None` when that
/// line ends the item instead.
type Decision = (usize, Option<usize>);

const FLAG_HAS_CONTENT: u8 = 1;
const FLAG_IS_LIST_ITEM: u8 = 2;

impl LineCacheInfo {
    /// Build cache from context in one O(n) pass
    fn new(ctx: &crate::lint_context::LintContext) -> Self {
        let total_lines = ctx.lines.len();
        let mut indentation = Vec::with_capacity(total_lines);
        let mut blockquote_levels = Vec::with_capacity(total_lines);
        let mut line_contents = Vec::with_capacity(total_lines);
        let mut flags = Vec::with_capacity(total_lines);
        let mut parent_map = HashMap::new();
        let mut item_end = HashMap::new();

        // Track most recent list item at each indentation level for O(1) parent lookups
        // Key: marker_column, Value: line_num (1-indexed)
        //
        // Algorithm correctness invariant:
        // For each list item L at line N with marker_column M:
        //   parent_map[N] = the line number of the most recent list item P where:
        //     1. P.line < N (appears before L)
        //     2. P.marker_column < M (less indented than L)
        //     3. P.marker_column is maximal among all candidates (closest parent)
        //
        // This matches the original O(n) backward scan logic but pre-computes in O(n).
        let mut indent_stack: Vec<(usize, usize)> = Vec::new();

        for (idx, line_info) in ctx.lines.iter().enumerate() {
            let line_content = line_info.content(ctx.content);
            let content = line_content.trim_start();
            let line_indent = line_info.byte_len - content.len();

            indentation.push(line_indent);

            // Store blockquote level for blockquote-aware indent calculation
            let bq_level = line_info.blockquote.as_ref().map_or(0, |bq| bq.nesting_level);
            blockquote_levels.push(bq_level);

            // Store line content for blockquote-aware indent calculation
            line_contents.push(line_content.to_string());

            let mut flag = 0u8;
            if !content.is_empty() {
                flag |= FLAG_HAS_CONTENT;
            }
            if let Some(list_item) = ctx.list_item_on_line(idx + 1) {
                flag |= FLAG_IS_LIST_ITEM;

                let line_num = idx + 1; // Convert to 1-indexed
                let marker_column = list_item.marker_column();

                // Maintain a monotonic stack of indentation levels (O(1) amortized)
                while let Some(&(indent, popped_line)) = indent_stack.last() {
                    if indent < marker_column {
                        break;
                    }
                    item_end.insert(popped_line, line_num);
                    indent_stack.pop();
                }

                if let Some((_, parent_line)) = indent_stack.last() {
                    parent_map.insert(line_num, *parent_line);
                }

                indent_stack.push((marker_column, line_num));
            }
            flags.push(flag);
        }

        Self {
            indentation,
            blockquote_levels,
            line_contents,
            flags,
            parent_map,
            item_end,
            decided: RefCell::default(),
            first_item_at: RefCell::default(),
        }
    }

    /// Check if line has content
    fn has_content(&self, idx: usize) -> bool {
        self.flags.get(idx).is_some_and(|&f| f & FLAG_HAS_CONTENT != 0)
    }

    /// Check if line is a list item
    fn is_list_item(&self, idx: usize) -> bool {
        self.flags.get(idx).is_some_and(|&f| f & FLAG_IS_LIST_ITEM != 0)
    }

    /// Get blockquote info for a line (level and prefix length)
    fn blockquote_info(&self, line: usize) -> (usize, usize) {
        if line == 0 || line > self.line_contents.len() {
            return (0, 0);
        }
        let idx = line - 1;
        let bq_level = self.blockquote_levels.get(idx).copied().unwrap_or(0);
        if bq_level == 0 {
            return (0, 0);
        }
        // Calculate prefix length from line content
        let content = &self.line_contents[idx];
        let mut prefix_len = 0;
        let mut found = 0;
        for c in content.chars() {
            prefix_len += c.len_utf8();
            if c == '>' {
                found += 1;
                if found == bq_level {
                    // Include optional space after last >
                    if content.get(prefix_len..prefix_len + 1) == Some(" ") {
                        prefix_len += 1;
                    }
                    break;
                }
            }
        }
        (bq_level, prefix_len)
    }

    /// The first line in `start_line..=end_line` (1-indexed) that settles
    /// whether a parent item has continuation content there, with the answer.
    ///
    /// For blockquote-aware detection, also pass the parent's blockquote level and
    /// blockquote prefix length. These are used to calculate effective indentation
    /// for lines inside blockquotes.
    ///
    /// Per CommonMark, tight (lazy) continuation — content with no blank line
    /// preceding it inside the item — is valid at any indent strictly past the
    /// marker column. Loose continuation — content following a blank line — must
    /// be indented to the parent's content column to remain inside the item.
    /// Callers therefore pass two thresholds: `tight_threshold` for content
    /// before the first blank line in the range, and `loose_threshold` for
    /// content after a blank line. Once a blank line is followed by content
    /// below `loose_threshold` the list item has ended, so no later content in
    /// the range can be continuation.
    fn scan_continuation(
        &self,
        start_line: usize,
        end_line: usize,
        tight_threshold: usize,
        loose_threshold: usize,
        parent_bq_level: usize,
        parent_bq_prefix_len: usize,
    ) -> Option<Decision> {
        if start_line == 0 || start_line > end_line || end_line > self.indentation.len() {
            return None;
        }

        // For blockquote lists, subtract the blockquote prefix so we compare
        // against the effective indentation within the blockquote context.
        let adjust = |t: usize| {
            if parent_bq_level > 0 {
                t.saturating_sub(parent_bq_prefix_len)
            } else {
                t
            }
        };
        let tight = adjust(tight_threshold);
        let loose = adjust(loose_threshold);

        let mut seen_blank = false;
        for idx in start_line - 1..end_line {
            if !self.has_content(idx) {
                seen_blank = true;
                continue;
            }
            if self.is_list_item(idx) {
                continue;
            }

            // Calculate effective indent (blockquote-aware)
            let line_bq_level = self.blockquote_levels.get(idx).copied().unwrap_or(0);
            let raw_indent = self.indentation[idx];
            let effective_indent = if line_bq_level == parent_bq_level && parent_bq_level > 0 {
                effective_indent_in_blockquote(&self.line_contents[idx], parent_bq_level, raw_indent)
            } else {
                raw_indent
            };

            let threshold = if seen_blank { loose } else { tight };
            if effective_indent >= threshold {
                return Some((idx + 1, Some(effective_indent)));
            }
            // After a blank line, content below the loose threshold ends the
            // list item; nothing further in the range can be continuation.
            if seen_blank {
                return Some((idx + 1, None));
            }
        }
        None
    }

    /// [`Self::scan_continuation`] over everything `parent_line` can own,
    /// computed once per parent: each of its children asks about a prefix of the
    /// same range, and the first decisive line answers all of them.
    fn continuation_decided(
        &self,
        parent_line: usize,
        (tight_threshold, loose_threshold): (usize, usize),
        (parent_bq_level, parent_bq_prefix_len): (usize, usize),
    ) -> Option<Decision> {
        let key = (
            parent_line,
            tight_threshold,
            loose_threshold,
            parent_bq_level,
            parent_bq_prefix_len,
        );
        if let Some(&decision) = self.decided.borrow().get(&key) {
            return decision;
        }
        let last_owned = self
            .item_end
            .get(&parent_line)
            .map_or(self.indentation.len(), |&end| end - 1);
        let decision = self.scan_continuation(
            parent_line + 1,
            last_owned,
            tight_threshold,
            loose_threshold,
            parent_bq_level,
            parent_bq_prefix_len,
        );
        self.decided.borrow_mut().insert(key, decision);
        decision
    }

    /// The indent of the first continuation line between `parent_line` and
    /// `before_line` (both exclusive, 1-indexed), if the parent has one there.
    ///
    /// `before_line` must be a line the parent owns, such as one of its children.
    fn continuation_before(
        &self,
        parent_line: usize,
        before_line: usize,
        thresholds: (usize, usize),
        parent_bq: (usize, usize),
    ) -> Option<usize> {
        match self.continuation_decided(parent_line, thresholds, parent_bq) {
            Some((decided_at, indent)) if decided_at < before_line => indent,
            _ => None,
        }
    }

    /// The first list item at `marker_column` from `from_line` on (1-indexed)
    /// within what `parent_line` owns.
    fn first_item_at_column(
        &self,
        ctx: &crate::lint_context::LintContext,
        parent_line: usize,
        from_line: usize,
        marker_column: usize,
    ) -> Option<usize> {
        let key = (parent_line, from_line, marker_column);
        if let Some(&line) = self.first_item_at.borrow().get(&key) {
            return line;
        }
        let end = self
            .item_end
            .get(&parent_line)
            .copied()
            .unwrap_or(self.indentation.len() + 1);
        let line = (from_line..end).find(|&line_num| {
            ctx.list_item_on_line(line_num)
                .is_some_and(|item| item.marker_column() == marker_column)
        });
        self.first_item_at.borrow_mut().insert(key, line);
        line
    }
}

impl MD005ListIndent {
    /// Gap tolerance for grouping list blocks as one logical structure.
    /// Markdown allows blank lines within lists, so we need some tolerance.
    /// 2 lines handles: 1 blank line + potential interruption
    const LIST_GROUP_GAP_TOLERANCE: usize = 2;

    /// Minimum indentation increase to be considered a child (not same level).
    /// Per Markdown convention, nested items need at least 2 more spaces.
    const MIN_CHILD_INDENT_INCREASE: usize = 2;

    /// Tolerance for considering items at "same level" despite minor indent differences.
    /// Allows for 1 space difference to accommodate inconsistent formatting.
    const SAME_LEVEL_TOLERANCE: i32 = 1;

    /// Standard continuation list indentation offset from parent content column.
    /// Lists that are continuation content typically indent 2 spaces from parent content.
    const STANDARD_CONTINUATION_OFFSET: usize = 2;

    /// Creates a warning for an indent mismatch.
    fn create_indent_warning(
        &self,
        ctx: &crate::lint_context::LintContext,
        line_num: usize,
        line_info: &crate::lint_context::LineInfo,
        actual_indent: usize,
        expected_indent: usize,
    ) -> LintWarning {
        let message = format!(
            "Expected indentation of {} {}, found {}",
            expected_indent,
            if expected_indent == 1 { "space" } else { "spaces" },
            actual_indent
        );

        let (start_line, start_col, end_line, end_col) = if actual_indent > 0 {
            calculate_match_range(line_num, line_info.content(ctx.content), 0, actual_indent)
        } else {
            calculate_match_range(line_num, line_info.content(ctx.content), 0, 1)
        };

        // For blockquote-nested lists, we need to preserve the blockquote prefix
        // Similar to how MD007 handles this case
        let (fix_range, replacement) = if line_info.blockquote.is_some() {
            // Calculate the range from start of line to the list marker position
            let start_byte = line_info.byte_offset;
            let mut end_byte = line_info.byte_offset;

            // Get the list marker position from list_item
            let marker_column = ctx
                .list_item_on_line(line_num)
                .map_or(actual_indent, ParsedListItem::marker_column);

            // Calculate where the marker starts
            for (i, ch) in line_info.content(ctx.content).chars().enumerate() {
                if i >= marker_column {
                    break;
                }
                end_byte += ch.len_utf8();
            }

            // Build the blockquote prefix
            let mut blockquote_count = 0;
            for ch in line_info.content(ctx.content).chars() {
                if ch == '>' {
                    blockquote_count += 1;
                } else if ch != ' ' && ch != '\t' {
                    break;
                }
            }

            // Build the blockquote prefix (one '>' per level, with spaces between for nested)
            let blockquote_prefix = if blockquote_count > 1 {
                (0..blockquote_count)
                    .map(|_| "> ")
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            } else {
                ">".to_string()
            };

            // Build replacement with blockquote prefix + correct indentation
            let correct_indent = " ".repeat(expected_indent);
            let replacement = format!("{blockquote_prefix} {correct_indent}");

            (start_byte..end_byte, replacement)
        } else {
            // Non-blockquote case: original logic
            let fix_range = if actual_indent > 0 {
                let start_byte = ctx.line_offsets.get(line_num - 1).copied().unwrap_or(0);
                let end_byte = start_byte + actual_indent;
                start_byte..end_byte
            } else {
                let byte_pos = ctx.line_offsets.get(line_num - 1).copied().unwrap_or(0);
                byte_pos..byte_pos
            };

            let replacement = if expected_indent > 0 {
                " ".repeat(expected_indent)
            } else {
                String::new()
            };

            (fix_range, replacement)
        };

        LintWarning {
            rule_name: Some(self.name().to_string()),
            line: start_line,
            column: start_col,
            end_line,
            end_column: end_col,
            message,
            severity: Severity::Warning,
            fix: Some(Fix::new(fix_range, replacement)),
        }
    }

    /// Checks consistency within a group of items and emits warnings.
    /// Uses first-established indent as the expected value when inconsistencies are found.
    fn check_indent_consistency(
        &self,
        ctx: &crate::lint_context::LintContext,
        items: &[(usize, usize, &crate::lint_context::LineInfo)],
        warnings: &mut Vec<(LintWarning, isize)>,
    ) {
        if items.len() < 2 {
            return;
        }

        // Sort items by line number to find first-established pattern
        let mut sorted_items: Vec<_> = items.iter().collect();
        sorted_items.sort_by_key(|(line_num, _, _)| *line_num);

        let indents: std::collections::HashSet<usize> = sorted_items.iter().map(|(_, indent, _)| *indent).collect();

        if indents.len() > 1 {
            // Items have inconsistent indentation
            // Use the first established indent as the expected value
            let expected_indent = sorted_items.first().map_or(0, |(_, i, _)| *i);

            for (line_num, indent, line_info) in items {
                if *indent != expected_indent {
                    warnings.push((
                        self.create_indent_warning(ctx, *line_num, line_info, *indent, expected_indent),
                        expected_indent as isize - *indent as isize,
                    ));
                }
            }
        }
    }

    /// Groups items by their semantic parent's content column AND list type.
    ///
    /// By grouping by (parent_content_column, is_ordered), we enforce consistency
    /// within each list type separately. This prevents oscillation with MD007, which
    /// only adjusts unordered list indentation and may expect different values than
    /// what ordered lists use. (fixes #287)
    fn group_by_parent_content_column<'a>(
        &self,
        level: usize,
        group: &[(usize, usize, &'a crate::lint_context::LineInfo)],
        all_list_items: &[(usize, usize, &crate::lint_context::LineInfo, ParsedListItem<'_>)],
        level_map: &HashMap<usize, usize>,
    ) -> ParentContentGroups<'a> {
        let parent_level = level - 1;

        // Build line->is_ordered map for O(1) lookup
        let is_ordered_map: HashMap<usize, bool> = all_list_items
            .iter()
            .map(|(ln, _, _, item)| (*ln, item.is_ordered()))
            .collect();

        // Collect parent-level items sorted by line number for binary search
        let parent_items: Vec<(usize, usize)> = all_list_items
            .iter()
            .filter(|(ln, _, _, _)| level_map.get(ln) == Some(&parent_level))
            .map(|(ln, _, _, item)| (*ln, item.content_column()))
            .collect();

        let mut parent_content_groups: ParentContentGroups<'a> = HashMap::new();

        for (line_num, indent, line_info) in group {
            let item_is_ordered = is_ordered_map.get(line_num).copied().unwrap_or(false);

            // Find the most recent parent-level item before this line using binary search
            let idx = parent_items.partition_point(|&(ln, _)| ln < *line_num);
            let parent_content_col = if idx > 0 { Some(parent_items[idx - 1].1) } else { None };

            if let Some(parent_col) = parent_content_col {
                parent_content_groups
                    .entry((parent_col, item_is_ordered))
                    .or_default()
                    .push((*line_num, *indent, *line_info));
            }
        }

        parent_content_groups
    }

    /// Group related list blocks that should be treated as one logical list structure
    fn group_related_list_blocks<'a>(&self, list_blocks: ParsedListBlocks<'a>) -> Vec<Vec<ParsedListBlock<'a>>> {
        let mut blocks = list_blocks.into_iter();
        let Some(first_block) = blocks.next() else {
            return Vec::new();
        };

        let mut groups = Vec::new();
        let mut current_group = vec![first_block];
        let mut prev_block = first_block;

        for current_block in blocks {
            // Check if blocks are consecutive (no significant gap between them)
            let line_gap = current_block.start_line().saturating_sub(prev_block.end_line());

            // Group blocks if they are close together
            // This handles cases where mixed list types are split but should be treated together.
            // Lists in different blockquotes are never one structure: each
            // measures its indentation from its own quote's margin.
            let same_quote = current_block.blockquote_prefix().matches('>').count()
                == prev_block.blockquote_prefix().matches('>').count();
            if same_quote && line_gap <= Self::LIST_GROUP_GAP_TOLERANCE {
                current_group.push(current_block);
            } else {
                // Start a new group
                groups.push(current_group);
                current_group = vec![current_block];
            }
            prev_block = current_block;
        }
        groups.push(current_group);

        groups
    }

    /// Check if a list item is continuation content of a parent list item
    /// Uses pre-computed parent map for O(1) lookup instead of O(n) backward scanning
    fn is_continuation_content(
        &self,
        ctx: &crate::lint_context::LintContext,
        cache: &LineCacheInfo,
        list_line: usize,
        list_indent: usize,
    ) -> bool {
        // Use pre-computed parent map instead of O(n) backward scan
        let parent_line = cache.parent_map.get(&list_line).copied();

        if let Some(parent_line) = parent_line
            && let Some(parent_list_item) = ctx.list_item_on_line(parent_line)
        {
            let line_info = parent_list_item.line_info();
            let parent_marker_column = parent_list_item.marker_column();
            let parent_content_column = parent_list_item.content_column();

            // Get parent's blockquote info for blockquote-aware continuation detection
            let parent_bq_level = line_info.blockquote.as_ref().map_or(0, |bq| bq.nesting_level);
            let parent_bq_prefix_len = line_info.blockquote.as_ref().map_or(0, |bq| bq.prefix.len());

            // Check if there are continuation lines between parent and current list.
            // Tight (lazy) continuation is valid at any indent past the marker; loose
            // continuation (after a blank line) requires the parent's content column.
            let thresholds = (parent_marker_column + 1, parent_content_column);
            let continuation_indent = cache.continuation_before(
                parent_line,
                list_line,
                thresholds,
                (parent_bq_level, parent_bq_prefix_len),
            );

            if let Some(continuation_indent) = continuation_indent {
                let is_standard_continuation =
                    list_indent == parent_content_column + Self::STANDARD_CONTINUATION_OFFSET;
                let matches_content_indent = list_indent == continuation_indent;

                if matches_content_indent || is_standard_continuation {
                    return true;
                }
            }

            // Special case: if this list item is at the same indentation as previous
            // continuation lists, it might be part of the same continuation block
            if list_indent > parent_marker_column {
                // Check if previous list items at this indentation are also continuation
                if self.has_continuation_list_at_indent(ctx, cache, parent_line, list_line, list_indent, thresholds) {
                    return true;
                }

                // Get blockquote info for continuation check
                let parent_bq = cache.blockquote_info(parent_line);
                if cache
                    .continuation_before(parent_line, list_line, thresholds, parent_bq)
                    .is_some()
                {
                    return true;
                }
            }
        }

        false
    }

    /// Check if there are continuation lists at the same indentation after a parent:
    /// an earlier list item at `list_indent` that the parent's continuation
    /// content precedes.
    ///
    /// `thresholds` is a `(tight, loose)` pair; see [`LineCacheInfo::scan_continuation`].
    fn has_continuation_list_at_indent(
        &self,
        ctx: &crate::lint_context::LintContext,
        cache: &LineCacheInfo,
        parent_line: usize,
        current_line: usize,
        list_indent: usize,
        thresholds: (usize, usize),
    ) -> bool {
        let parent_bq = cache.blockquote_info(parent_line);
        match cache.continuation_decided(parent_line, thresholds, parent_bq) {
            Some((decided_at, Some(_))) => cache
                .first_item_at_column(ctx, parent_line, decided_at + 1, list_indent)
                .is_some_and(|line_num| line_num < current_line),
            _ => false,
        }
    }

    /// Check a group of related list blocks as one logical list structure
    fn check_list_block_group(
        &self,
        ctx: &crate::lint_context::LintContext,
        cache: &LineCacheInfo,
        group: &[ParsedListBlock<'_>],
        warnings: &mut Vec<(LintWarning, isize)>,
    ) {
        // First pass: collect all candidate items without filtering
        // We need to process in line order so parents are seen before children
        let mut candidate_items: Vec<(usize, usize, &crate::lint_context::LineInfo, ParsedListItem<'_>)> = Vec::new();

        for list_block in group {
            for list_item in list_block.items() {
                let item_line = list_item.line_num();
                let line_info = list_item.line_info();
                // Calculate the effective indentation (considering blockquotes)
                let effective_indent = if let Some(blockquote) = &line_info.blockquote {
                    // For blockquoted lists, use relative indentation within the blockquote
                    list_item.marker_column().saturating_sub(blockquote.nesting_level * 2)
                } else {
                    // For normal lists, use the marker column directly
                    list_item.marker_column()
                };

                candidate_items.push((item_line, effective_indent, line_info, list_item));
            }
        }

        // Sort by line number so parents are processed before children
        candidate_items.sort_by_key(|(line_num, _, _, _)| *line_num);

        // Second pass: filter out continuation content AND their children
        // When a parent is skipped, all its descendants must also be skipped
        let mut skipped_lines: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut all_list_items: Vec<(usize, usize, &crate::lint_context::LineInfo, ParsedListItem<'_>)> = Vec::new();

        for (item_line, effective_indent, line_info, list_item) in candidate_items {
            // Skip list items inside footnote definitions
            if line_info.in_footnote_definition {
                skipped_lines.insert(item_line);
                continue;
            }
            // Skip list items that are continuation content
            if self.is_continuation_content(ctx, cache, item_line, effective_indent) {
                skipped_lines.insert(item_line);
                continue;
            }

            // Also skip items whose parent was skipped (children of continuation content)
            if let Some(&parent_line) = cache.parent_map.get(&item_line)
                && skipped_lines.contains(&parent_line)
            {
                skipped_lines.insert(item_line);
                continue;
            }

            all_list_items.push((item_line, effective_indent, line_info, list_item));
        }

        if all_list_items.is_empty() {
            return;
        }

        // Sort by line number to process in order
        all_list_items.sort_by_key(|(line_num, _, _, _)| *line_num);

        // Build level mapping based on hierarchical structure
        // Key insight: We need to identify which items are meant to be at the same level
        // even if they have slightly different indentations (inconsistent formatting)
        let mut level_map: HashMap<usize, usize> = HashMap::new();
        let mut level_indents: HashMap<usize, Vec<usize>> = HashMap::new(); // Track all indents seen at each level

        // Track the most recent item at each indent level for O(1) parent lookups
        // Key: indent value, Value: (level, line_num)
        let mut indent_to_level: HashMap<usize, (usize, usize)> = HashMap::new();

        // Process items in order to build the level hierarchy - now O(n) instead of O(n²)
        for (line_num, indent, _, _) in &all_list_items {
            let level = if indent_to_level.is_empty() {
                // First item establishes level 1
                level_indents.entry(1).or_default().push(*indent);
                1
            } else {
                // Find the appropriate level for this item
                let mut determined_level = 0;

                // First, check if this indent matches any existing level exactly
                if let Some(&(existing_level, _)) = indent_to_level.get(indent) {
                    determined_level = existing_level;
                } else {
                    // No exact match - determine level based on hierarchy
                    // Find the most recent item with clearly less indentation (parent)
                    // Instead of scanning backward O(n), look through tracked indents O(k) where k is number of unique indents
                    let mut best_parent: Option<(usize, usize, usize)> = None; // (indent, level, line)

                    for (&tracked_indent, &(tracked_level, tracked_line)) in &indent_to_level {
                        if tracked_indent < *indent {
                            // This is a potential parent (less indentation)
                            // Keep the one with the largest indent (closest parent)
                            if best_parent.is_none() || tracked_indent > best_parent.unwrap().0 {
                                best_parent = Some((tracked_indent, tracked_level, tracked_line));
                            }
                        }
                    }

                    if let Some((parent_indent, parent_level, _parent_line)) = best_parent {
                        // A clear parent has at least MIN_CHILD_INDENT_INCREASE spaces less indentation
                        if parent_indent + Self::MIN_CHILD_INDENT_INCREASE <= *indent {
                            // This is a child of the parent
                            determined_level = parent_level + 1;
                        } else if (*indent as i32 - parent_indent as i32).abs() <= Self::SAME_LEVEL_TOLERANCE {
                            // Within SAME_LEVEL_TOLERANCE - likely meant to be same level but inconsistent
                            determined_level = parent_level;
                        } else {
                            // Less than 2 space difference but more than 1
                            // This is ambiguous - could be same level or child
                            // Check if any existing level has a similar indent
                            let mut found_similar = false;
                            if let Some(indents_at_level) = level_indents.get(&parent_level) {
                                for &level_indent in indents_at_level {
                                    if (level_indent as i32 - *indent as i32).abs() <= Self::SAME_LEVEL_TOLERANCE {
                                        determined_level = parent_level;
                                        found_similar = true;
                                        break;
                                    }
                                }
                            }
                            if !found_similar {
                                // Treat as child since it has more indent
                                determined_level = parent_level + 1;
                            }
                        }
                    }

                    // If still not determined, default to level 1
                    if determined_level == 0 {
                        determined_level = 1;
                    }

                    // Record this indent for the level
                    level_indents.entry(determined_level).or_default().push(*indent);
                }

                determined_level
            };

            level_map.insert(*line_num, level);
            // Track this indent and level for future O(1) lookups
            indent_to_level.insert(*indent, (level, *line_num));
        }

        // Now group items by their level
        let mut level_groups: HashMap<usize, Vec<(usize, usize, &crate::lint_context::LineInfo)>> = HashMap::new();
        for (line_num, indent, line_info, _) in &all_list_items {
            let level = level_map[line_num];
            level_groups
                .entry(level)
                .or_default()
                .push((*line_num, *indent, *line_info));
        }

        // For each level, check consistency
        for (level, mut group) in level_groups {
            group.sort_by_key(|(line_num, _, _)| *line_num);

            if level == 1 {
                // Top-level items should have the configured indentation
                for (line_num, indent, line_info) in &group {
                    if *indent != self.top_level_indent {
                        warnings.push((
                            self.create_indent_warning(ctx, *line_num, line_info, *indent, self.top_level_indent),
                            self.top_level_indent as isize - *indent as isize,
                        ));
                    }
                }
            } else {
                // For sublists (level > 1), group items by their semantic parent's content column.
                // This handles ordered lists where marker widths vary (e.g., "1. " vs "10. ").
                let parent_content_groups =
                    self.group_by_parent_content_column(level, &group, &all_list_items, &level_map);

                // Check consistency within each parent content column group
                for items in parent_content_groups.values() {
                    self.check_indent_consistency(ctx, items, warnings);
                }
            }
        }
    }

    /// Migrated to use centralized list blocks for better performance and accuracy
    fn check_optimized(&self, ctx: &crate::lint_context::LintContext) -> Vec<LintWarning> {
        let content = ctx.content;

        // Early returns for common cases
        if content.is_empty() {
            return Vec::new();
        }

        // Quick check for any list blocks before processing
        let list_blocks = ctx.parsed_list_blocks();
        if list_blocks.is_empty() {
            return Vec::new();
        }

        let mut warnings = Vec::new();

        // Build cache once for all groups instead of per-group
        let cache = LineCacheInfo::new(ctx);

        // Group consecutive list blocks that should be treated as one logical structure
        // This is needed because mixed list types (ordered/unordered) get split into separate blocks
        let block_groups = self.group_related_list_blocks(list_blocks);

        for group in block_groups {
            self.check_list_block_group(ctx, &cache, &group, &mut warnings);
        }

        // Re-indenting a marker moves its item's content column, so the lines the
        // item owns move with it. Every marker MD005 flags gets its own warning, so
        // nested markers are left to those.
        let moves = warnings
            .iter()
            .enumerate()
            .filter(|(_, (_, delta))| *delta != 0)
            .map(|(index, (warning, delta))| (warning.line - 1, (index, *delta)))
            .collect();
        let mut warnings: Vec<LintWarning> = warnings.into_iter().map(|(warning, _)| warning).collect();
        move_owned_lines(ctx, &moves, Nesting::Absolute, &mut warnings);
        warnings
    }
}

impl Rule for MD005ListIndent {
    fn name(&self) -> &'static str {
        "MD005"
    }

    fn description(&self) -> &'static str {
        "List indentation should be consistent"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = self.check_optimized(ctx);
        drop_structure_changing_fixes(ctx, &mut warnings, Allowed::Nothing);
        Ok(warnings)
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        let warnings = self.check(ctx)?;
        let warnings = warnings;
        if warnings.is_empty() {
            return Ok(ctx.content.to_string());
        }

        crate::utils::fix_utils::apply_warning_fixes(ctx.content, &warnings).map_err(LintError::InvalidInput)
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::List
    }

    /// Check if this rule should be skipped
    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Skip if content is empty or has no list items
        ctx.content.is_empty() || !ctx.has_list_items()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
