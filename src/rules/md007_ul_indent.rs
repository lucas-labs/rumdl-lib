/// Rule MD007: Unordered list indentation
///
/// See [docs/md007.md](../../docs/md007.md) for full documentation, configuration, and examples.
use crate::rule::{LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::list_fix_guard::{Allowed, drop_structure_changing_fixes};
use crate::utils::list_indent_shift::{Nesting, move_owned_lines};
use std::collections::HashMap;

pub mod md007_config;
use md007_config::MD007Config;

#[derive(Debug, Clone, Default)]
pub struct MD007ULIndent {
    config: MD007Config,
}

impl MD007ULIndent {
    pub fn new(indent: usize) -> Self {
        Self {
            config: MD007Config {
                indent: crate::types::IndentSize::from_const(indent as u8),
                start_indented: false,
                start_indent: crate::types::IndentSize::from_const(2),
                style: md007_config::IndentStyle::TextAligned,
                style_explicit: false,  // Allow auto-detection for programmatic construction
                indent_explicit: false, // Programmatic construction uses default behavior
            },
        }
    }

    /// Convert character position to visual column (accounting for tabs)
    fn char_pos_to_visual_column(content: &str, char_pos: usize) -> usize {
        let mut visual_col = 0;

        for (current_pos, ch) in content.chars().enumerate() {
            if current_pos >= char_pos {
                break;
            }
            if ch == '\t' {
                // Tab moves to next multiple of 4
                visual_col = (visual_col / 4 + 1) * 4;
            } else {
                visual_col += 1;
            }
        }
        visual_col
    }

    /// Pop list-stack entries that a content line at (`bq_depth`, `visual_indent`)
    /// has closed. An open item still contains the line only when the line stays
    /// in the item's blockquote context (or a deeper one) and begins at or past
    /// the item's content column; otherwise the item has ended. Keeping the stack
    /// accurate prevents a later list from being mistaken for a sublist of an item
    /// that already closed (which would, e.g., wrongly extend the ordered-ancestor
    /// exemption past a terminating paragraph, blockquote, or code block).
    /// Visual indentation of a line measured in the same coordinate space the
    /// stack uses for `content_col`: for a blockquoted line that is the width of
    /// the leading whitespace *after* the `>` prefix(es); for any other line it is
    /// the absolute `visual_indent`. Comparing a blockquoted line's absolute indent
    /// (which counts the `>` markers) against a blockquote-relative content column
    /// would otherwise treat in-quote content as if it had dedented out of the item.
    /// Measure the line's indentation in the coordinate space of a blockquote at the
    /// given nesting `depth`: strip exactly `depth` `>` markers (each with one optional
    /// following space or tab) and return the leading whitespace of the remainder as
    /// visual columns. At depth 0 this is the line's own visual indent.
    ///
    /// The remainder may itself begin with deeper `>` markers; the whitespace measured
    /// is whatever precedes them, so an interrupting deeper quote reports the column at
    /// which its `>` begins inside the shallower container. That lets a closed-item check
    /// compare the line against an item using the item's own quote coordinate space,
    /// avoiding any relative-vs-absolute mismatch.
    fn indent_relative_to_depth(
        ctx: &crate::lint_context::LintContext,
        line_info: &crate::lint_context::LineInfo,
        depth: usize,
    ) -> usize {
        if depth == 0 {
            return line_info.visual_indent;
        }
        // The blockquote's pre-parsed `content` has its leading whitespace stripped,
        // so it cannot report the in-quote indentation. Walk the `>` prefix(es) on the
        // raw line (mirroring the list-item indent calculation) and measure the
        // whitespace that follows, which is the indent inside the quote container.
        let line_content = line_info.content(ctx.content);
        let mut remaining = line_content;
        let mut content_start = 0;
        let mut stripped_levels = 0;
        while stripped_levels < depth {
            let trimmed = remaining.trim_start();
            if !trimmed.starts_with('>') {
                break;
            }
            content_start += remaining.len() - trimmed.len();
            content_start += 1;
            let after_gt = &trimmed[1..];
            if let Some(stripped) = after_gt.strip_prefix(' ') {
                content_start += 1;
                remaining = stripped;
            } else if let Some(stripped) = after_gt.strip_prefix('\t') {
                content_start += 1;
                remaining = stripped;
            } else {
                remaining = after_gt;
            }
            stripped_levels += 1;
        }
        let content_after_prefix = &line_content[content_start..];
        let ws_chars = content_after_prefix
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count();
        Self::char_pos_to_visual_column(content_after_prefix, ws_chars)
    }

    fn terminate_closed_items(
        ctx: &crate::lint_context::LintContext,
        line_info: &crate::lint_context::LineInfo,
        list_stack: &mut Vec<(usize, usize, bool, usize, usize, bool, usize)>,
        line_bq_depth: usize,
    ) {
        while let Some(&(_, _, _, content_col, item_bq_depth, _, _)) = list_stack.last() {
            let closed = match item_bq_depth.cmp(&line_bq_depth) {
                // The line has exited a deeper blockquote the item lived in.
                std::cmp::Ordering::Greater => true,
                // The line is in the same or a deeper blockquote than the item.
                // Measure the line's indent in the item's own quote coordinate space
                // and close the item when the line begins left of the item's content.
                // For a same-depth line this is the in-container indent; for a deeper
                // interrupting quote it is the column where that quote's `>` begins
                // inside the item's container, so a `> > quote` left of the item's
                // content (e.g. interrupting `> 1. ordered`) closes it, while a quote
                // indented into the item's content keeps it open.
                std::cmp::Ordering::Equal | std::cmp::Ordering::Less => {
                    content_col > Self::indent_relative_to_depth(ctx, line_info, item_bq_depth)
                }
            };
            if closed {
                list_stack.pop();
            } else {
                break;
            }
        }
    }

    /// Calculate expected indentation for a nested list item.
    ///
    /// This uses per-parent logic rather than document-wide style selection:
    /// - When parent is **ordered**: align with parent's text (handles variable-width markers)
    /// - When parent is **unordered**: use configured indent (fixed-width markers)
    ///
    /// If user explicitly sets `style`, that choice is respected uniformly.
    /// "Do What I Mean" behavior: if user sets `indent` but not `style`, use fixed style.
    fn calculate_expected_indent(
        &self,
        nesting_level: usize,
        parent_info: Option<(bool, usize)>, // (is_ordered, content_visual_col)
    ) -> usize {
        if nesting_level == 0 {
            return 0;
        }

        // If user explicitly set style, respect their choice uniformly
        if self.config.style_explicit {
            return match self.config.style {
                md007_config::IndentStyle::Fixed => nesting_level * self.config.indent.get() as usize,
                md007_config::IndentStyle::TextAligned => {
                    parent_info.map_or(nesting_level * 2, |(_, content_col)| content_col)
                }
            };
        }

        // "Do What I Mean": if indent is explicitly set (but style is not), use fixed style
        // This is the expected behavior when users configure `indent = 4` - they want 4-space increments
        if self.config.indent_explicit {
            match parent_info {
                Some((true, parent_content_col)) => {
                    // Parent is ordered: return text-aligned as primary expected value.
                    // The caller also accepts the fixed indent as an alternative.
                    return parent_content_col;
                }
                _ => {
                    // Parent is unordered or no parent: use fixed indent
                    return nesting_level * self.config.indent.get() as usize;
                }
            }
        }

        // Smart default: per-parent type decision
        match parent_info {
            Some((true, parent_content_col)) => {
                // Parent is ordered: align with parent's text position
                // This handles variable-width markers ("1." vs "10." vs "100.")
                parent_content_col
            }
            Some((false, parent_content_col)) => {
                // Parent is unordered: check if it's at the expected fixed position
                // If yes, continue with fixed style (for pure unordered lists)
                // If no, parent is offset (e.g., inside ordered list), use text-aligned
                let parent_level = nesting_level.saturating_sub(1);
                let expected_parent_marker = parent_level * self.config.indent.get() as usize;
                // Parent's marker column is content column minus marker width (2 for "- ")
                let parent_marker_col = parent_content_col.saturating_sub(2);

                if parent_marker_col == expected_parent_marker {
                    // Parent is at expected fixed position, continue with fixed style
                    nesting_level * self.config.indent.get() as usize
                } else {
                    // Parent is offset, use text-aligned
                    parent_content_col
                }
            }
            None => {
                // No parent found (shouldn't happen at nesting_level > 0)
                nesting_level * self.config.indent.get() as usize
            }
        }
    }
}

impl Rule for MD007ULIndent {
    fn name(&self) -> &'static str {
        "MD007"
    }

    fn description(&self) -> &'static str {
        "Unordered list indentation"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = Vec::new();
        let mut list_stack: Vec<(usize, usize, bool, usize, usize, bool, usize)> = Vec::new(); // Stack of (marker_visual_col, line_num, is_ordered, content_visual_col, blockquote_depth, chain, source_content_visual_col) for tracking nesting. `chain` marks an unordered item that inherited the ordered-ancestor MD007 exemption or was checked under the fixed-style clamp. `content_visual_col` is the corrected (post-fix) column that expectation math builds on; `source_content_visual_col` is the column as written, which containment is judged against.

        // Line of each re-indented item -> (its warning, how far its content moves).
        let mut moves = HashMap::new();

        for (line_idx, line_info) in ctx.lines.iter().enumerate() {
            let parsed_list_item = ctx.list_item_on_line(line_idx + 1);
            // Skip if this line is in a code block, front matter, or mkdocstrings
            let is_skipped_region = |info: &crate::lint_context::LineInfo| {
                info.in_code_block || info.in_front_matter || info.in_mkdocstrings || info.in_footnote_definition
            };
            // Exception: a fenced code block can open on a list-marker line
            // (e.g. "- ```"). Such a line is flagged `in_code_block` but is
            // genuinely a list item, so it must fall through to the list-item
            // handling below to be pushed onto the ancestor stack; otherwise its
            // descendants resolve one nesting level too shallow and get wrongly
            // flagged (and "fixed") as over-indented. The line must ITSELF open a
            // backtick/tilde fence: a list-like line interior to a code construct
            // that pulldown-cmark does not parse (e.g. an Azure `:::` block) is also
            // `in_code_block` with a `list_item`, but it is opaque code, not a list
            // item, so it stays skipped. The other skipped regions (front matter,
            // mkdocstrings, footnote definitions) genuinely contain their list
            // items, so those are still skipped.
            let opens_fence_on_marker_line = parsed_list_item
                .and_then(|item| line_info.content(ctx.content).get(item.content_column()..))
                .is_some_and(|after_marker| {
                    let after_marker = after_marker.trim_start();
                    after_marker.starts_with("```") || after_marker.starts_with("~~~")
                });
            let fence_opening_marker_line = opens_fence_on_marker_line
                && line_info.in_code_block
                && !line_info.in_front_matter
                && !line_info.in_mkdocstrings
                && !line_info.in_footnote_definition;
            if is_skipped_region(line_info) && !fence_opening_marker_line {
                // The opening line of such a region (e.g. an unindented code fence)
                // breaks out of any open list just like a paragraph would, so the
                // stale list stack must be cleared even though the region's lines
                // are otherwise skipped. Interior lines (code contents, etc.) are
                // immaterial: only act on the region's first non-blank line, using
                // its indentation to decide which items it closed.
                let region_start = line_idx == 0 || !is_skipped_region(&ctx.lines[line_idx - 1]);
                if region_start && !line_info.is_blank {
                    let bq_depth = line_info.blockquote.as_ref().map_or(0, |bq| bq.nesting_level);
                    Self::terminate_closed_items(ctx, line_info, &mut list_stack, bq_depth);
                }
                continue;
            }

            // Check if this line has a list item
            if let Some(list_item) = parsed_list_item {
                // For blockquoted lists, we need to calculate indentation relative to the blockquote content
                // not the full line. This is because blockquoted lists follow the same indentation rules
                // as regular lists, just within their blockquote context.
                let (content_for_calculation, adjusted_marker_column) = if line_info.blockquote.is_some() {
                    // Find the position after ALL blockquote prefixes (handles nested > > > etc)
                    let line_content = line_info.content(ctx.content);
                    let mut remaining = line_content;
                    let mut content_start = 0;

                    loop {
                        let trimmed = remaining.trim_start();
                        if !trimmed.starts_with('>') {
                            break;
                        }
                        // Account for leading whitespace
                        content_start += remaining.len() - trimmed.len();
                        // Account for '>'
                        content_start += 1;
                        let after_gt = &trimmed[1..];
                        // Handle optional whitespace after '>' (space or tab)
                        if let Some(stripped) = after_gt.strip_prefix(' ') {
                            content_start += 1;
                            remaining = stripped;
                        } else if let Some(stripped) = after_gt.strip_prefix('\t') {
                            content_start += 1;
                            remaining = stripped;
                        } else {
                            remaining = after_gt;
                        }
                    }

                    // Extract the content after the blockquote prefix
                    let content_after_prefix = &line_content[content_start..];
                    // Adjust the marker column to be relative to the content after the prefix
                    let adjusted_col = if list_item.marker_column() >= content_start {
                        list_item.marker_column() - content_start
                    } else {
                        // This shouldn't happen, but handle it gracefully
                        list_item.marker_column()
                    };
                    (content_after_prefix.to_string(), adjusted_col)
                } else {
                    (line_info.content(ctx.content).to_string(), list_item.marker_column())
                };

                // Convert marker position to visual column
                let visual_marker_column =
                    Self::char_pos_to_visual_column(&content_for_calculation, adjusted_marker_column);

                // Calculate content visual column for text-aligned style
                let visual_content_column = if line_info.blockquote.is_some() {
                    // For blockquoted content, we already have the adjusted content
                    let adjusted_content_col =
                        if list_item.content_column() >= (line_info.byte_len - content_for_calculation.len()) {
                            list_item.content_column() - (line_info.byte_len - content_for_calculation.len())
                        } else {
                            list_item.content_column()
                        };
                    Self::char_pos_to_visual_column(&content_for_calculation, adjusted_content_col)
                } else {
                    Self::char_pos_to_visual_column(line_info.content(ctx.content), list_item.content_column())
                };

                // For nesting detection, treat 1-space indent as if it's at column 0
                // because 1 space is insufficient to establish a nesting relationship
                // UNLESS the user has explicitly configured indent=1, in which case 1 space IS valid nesting
                let visual_marker_for_nesting = if visual_marker_column == 1 && self.config.indent.get() != 1 {
                    0
                } else {
                    visual_marker_column
                };

                // Determine blockquote depth for this line
                let bq_depth = line_info.blockquote.as_ref().map_or(0, |bq| bq.nesting_level);

                // Clean up stack - remove items at same or deeper indentation,
                // but only consider items at the same blockquote depth
                while let Some(&(indent, _, _, _, item_bq_depth, _, _)) = list_stack.last() {
                    if item_bq_depth == bq_depth && indent >= visual_marker_for_nesting {
                        list_stack.pop();
                    } else if item_bq_depth > bq_depth {
                        // Pop items from deeper blockquote contexts that we've left
                        list_stack.pop();
                    } else {
                        break;
                    }
                }

                // The loop above only reconciles items at the same (or deeper)
                // blockquote depth. A list item that enters a deeper blockquote than an
                // ancestor (e.g. `> > - item` after `> 1. ordered`, or `> - item` after
                // a top-level `1. ordered`) starts a separate container when that quote
                // begins left of the ancestor's content. Measured in the ancestor's own
                // quote coordinate space, the deeper quote's marker is then to the left
                // of the ancestor's content column, so the ancestor is closed. Pop it
                // here, otherwise a closed ordered ancestor would linger and wrongly
                // extend its exemption to a later, separately indented unordered list.
                // A deeper quote indented into the ancestor's content is part of that
                // item and keeps it open. Same-depth nesting and items already inside a
                // blockquote are left to the loop above and the exemption check below.
                while let Some(&(_, _, _, content_col, item_bq_depth, _, _)) = list_stack.last() {
                    if item_bq_depth < bq_depth
                        && content_col > Self::indent_relative_to_depth(ctx, line_info, item_bq_depth)
                    {
                        list_stack.pop();
                    } else {
                        break;
                    }
                }

                // For ordered list items, just track them in the stack
                if list_item.is_ordered() {
                    // For ordered lists, we don't check indentation but we need to track for text-aligned children
                    // Use the actual positions since we don't enforce indentation for ordered lists
                    list_stack.push((
                        visual_marker_column,
                        line_idx,
                        true,
                        visual_content_column,
                        bq_depth,
                        false,
                        visual_content_column,
                    ));
                    continue;
                }

                // At this point, we know this is an unordered list item.
                //
                // markdownlint applies MD007 to a sublist only if its parent lists
                // are all also unordered. An unordered item that is genuinely nested
                // under an ordered ancestor is therefore exempt from the indentation
                // check, at any depth. Two conditions must both hold:
                //
                //   1. threshold: an ordered ancestor at this blockquote depth has its
                //      content column at or left of this bullet's marker, so the bullet
                //      is indented far enough to be that ordered item's sublist. A
                //      bullet indented less than the ordered content column is a new
                //      top-level list, which markdownlint still checks.
                //   2. chain: the nearest same-depth ancestor is itself ordered, or is
                //      an unordered item that already inherited the exemption. This
                //      stops the exemption from leaking past a non-nested unordered
                //      parent to its children. For `100. ordered` / `   - parent` /
                //      `     - child`, the parent is left of the ordered content column
                //      (not nested, not exempt), so the child resolves against the real
                //      unordered layout and is still checked.
                //
                // The MkDocs flavor is excluded: it deliberately enforces
                // Python-Markdown's stricter continuation indent under ordered parents
                // (insufficient indent there is a real rendering bug, not a style nit).
                let threshold_ok = list_stack
                    .iter()
                    .any(|item| item.4 == bq_depth && item.2 && item.3 <= visual_marker_column);
                // The marker-based cleanup above keeps a list item whose marker sits
                // between the previous item's marker and its content column, even
                // though per CommonMark such an item is the previous one's sibling,
                // not its child. Harmless under the exemption, but the clamp would
                // then resolve against the too-deep parent and its fix would rewrite
                // the sibling into a child (and contradict MD005). Before judging the
                // chain, pop every trailing item whose source content column this
                // marker sits left of, ordered ones included: with nested ordered
                // lists, threshold_ok can hold via an outer ordered ancestor while
                // the innermost ordered item does not contain the marker at all.
                if threshold_ok && self.config.style_explicit && self.config.style == md007_config::IndentStyle::Fixed {
                    while let Some(&(_, _, _, _, item_bq_depth, _, source_content_col)) = list_stack.last() {
                        if item_bq_depth == bq_depth && source_content_col > visual_marker_column {
                            list_stack.pop();
                        } else {
                            break;
                        }
                    }
                }
                let chain_ok = list_stack
                    .iter()
                    .rev()
                    .find(|item| item.4 == bq_depth)
                    .is_some_and(|item| item.2 || item.5);
                let ordered_chain = threshold_ok && chain_ok;
                // An explicit style = "fixed" opts out of the exemption: the item is
                // checked against the fixed expectation, clamped below to the parent's
                // content column. The clamp is what makes checking safe here: a bullet
                // left of its parent's content column leaves the item, so an unclamped
                // fixed expectation would let the fix restructure the document.
                let clamp_to_parent = ordered_chain
                    && self.config.style_explicit
                    && self.config.style == md007_config::IndentStyle::Fixed;
                if ordered_chain && !clamp_to_parent {
                    list_stack.push((
                        visual_marker_column,
                        line_idx,
                        false,
                        visual_content_column,
                        bq_depth,
                        true,
                        visual_content_column,
                    ));
                    continue;
                }

                // Count only items at the same blockquote depth for nesting level
                let nesting_level = list_stack.iter().filter(|item| item.4 == bq_depth).count();

                // Get parent info for per-parent calculation (only from same blockquote depth)
                let parent_info = list_stack
                    .iter()
                    .rev()
                    .find(|item| item.4 == bq_depth)
                    .map(|&(_, _, is_ordered, content_col, _, _, _)| (is_ordered, content_col));

                // Calculate expected indent using per-parent logic
                // When start_indented is true, only depth-0 items use the start_indent value.
                // For nested items (depth >= 1), the parent's actual position in the stack
                // already reflects the start_indent shift, so calculate_expected_indent
                // naturally produces the correct result.
                let mut expected_indent = if self.config.start_indented && nesting_level == 0 {
                    self.config.start_indent.get() as usize
                } else {
                    self.calculate_expected_indent(nesting_level, parent_info)
                };

                // Clamp the fixed expectation to the parent's content column. The
                // floor cascades: a fixed child stores its corrected content column
                // on the stack, so a grandchild's floor is measured from where the
                // fix puts the child, not from the source.
                if clamp_to_parent && let Some((_, parent_content_col)) = parent_info {
                    expected_indent = expected_indent.max(parent_content_col);
                }

                // When indent is explicitly set and parent is ordered, also accept
                // the fixed indent value (nesting_level * indent). This lets users
                // choose either text-aligned or their configured indent under ordered lists.
                // Not under the clamp: there the fixed value can sit left of the
                // parent's content column, and suggesting it would detach the item.
                let also_acceptable = if !clamp_to_parent
                    && self.config.indent_explicit
                    && parent_info.is_some_and(|(is_ordered, _)| is_ordered)
                {
                    Some(nesting_level * self.config.indent.get() as usize)
                } else {
                    None
                };

                // Add current item to stack
                // Use actual marker position for cleanup logic
                // For text-aligned children, store the EXPECTED content position after fix
                // (not the actual position) to prevent error cascade
                // When accepted via also_acceptable, use that indent for content col
                let accepted_indent = if also_acceptable.is_some_and(|alt| visual_marker_column == alt) {
                    visual_marker_column
                } else {
                    expected_indent
                };
                // Store the content column the item will have *after* its indent is
                // fixed: the corrected marker column plus this marker's actual width
                // (marker char + the spaces after it). Using the real width rather than a
                // hard-coded 2 keeps a child aligned to a parent whose marker was widened
                // by a non-default MD030 (e.g. `-   ` under `ul-multi = 3`, content column
                // 4); hard-coding 2 stored column 2 and flagged the correctly-nested child
                // as over-indented, then "fixed" it to a column where it detaches into a
                // sibling. For the common single-space marker the width is 2, so the
                // stored value is unchanged.
                let marker_width = visual_content_column.saturating_sub(visual_marker_column);
                let expected_content_visual_col = accepted_indent + marker_width;
                // A clamped item stays part of the ordered-rooted chain: its own
                // descendants must resolve through the chain flag exactly as they
                // would have through an exempt ancestor, or they would fall back to
                // the unclamped fixed expectation and detach.
                list_stack.push((
                    visual_marker_column,
                    line_idx,
                    false,
                    expected_content_visual_col,
                    bq_depth,
                    clamp_to_parent,
                    visual_content_column,
                ));

                // A top-level item (depth 0) is expected at column 0 when start_indented
                // is false. Column 0 is already correct, so skip it; any other column
                // (1, 2, or 3) is a misindented top-level list and must be flagged with
                // "Expected 0". Four or more leading spaces form an indented code block,
                // not a list, so such lines never reach here as list items.
                if !self.config.start_indented && nesting_level == 0 && visual_marker_column == 0 {
                    continue;
                }

                if visual_marker_column != expected_indent && also_acceptable != Some(visual_marker_column) {
                    // Use the fixed indent as the suggested value when the alternative was available
                    if let Some(alt) = also_acceptable {
                        expected_indent = alt;
                    }
                    // Generate fix for this list item
                    let fix = {
                        let correct_indent = " ".repeat(expected_indent);

                        // Build the replacement string - need to preserve everything before the list marker
                        // For blockquoted lines, this includes the blockquote prefix
                        let replacement = if line_info.blockquote.is_some() {
                            // Count the blockquote markers
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
                            // Add correct indentation after the blockquote prefix
                            // Include one space after the blockquote marker(s) as part of the indent
                            format!("{blockquote_prefix} {correct_indent}")
                        } else {
                            correct_indent
                        };

                        // Calculate the byte positions
                        // The range should cover from start of line to the marker position
                        let start_byte = line_info.byte_offset;
                        let mut end_byte = line_info.byte_offset;

                        // Calculate where the marker starts
                        for (i, ch) in line_info.content(ctx.content).chars().enumerate() {
                            if i >= list_item.marker_column() {
                                break;
                            }
                            end_byte += ch.len_utf8();
                        }

                        Some(crate::rule::Fix::new(start_byte..end_byte, replacement))
                    };

                    warnings.push(LintWarning {
                        rule_name: Some(self.name().to_string()),
                        message: format!(
                            "Expected {expected_indent} spaces for indent depth {nesting_level}, found {visual_marker_column}"
                        ),
                        line: line_idx + 1, // Convert to 1-indexed
                        column: 1,          // Start of line
                        end_line: line_idx + 1,
                        end_column: visual_marker_column + 1, // End of visual indentation
                        severity: Severity::Warning,
                        fix,
                    });
                    moves.insert(
                        line_idx,
                        (
                            warnings.len() - 1,
                            expected_indent as isize - visual_marker_column as isize,
                        ),
                    );
                }
            } else if !line_info.is_blank {
                // A non-blank, non-list content line that breaks out of the open
                // list terminates every list item whose content begins to its
                // right: an item's children must be indented past its content
                // column, so a line indented less cannot belong to it. Popping
                // these closed items keeps list_stack accurate, so a later list
                // is not mistaken for a sublist of one that has already ended
                // (e.g. a top-level paragraph closing an ordered list, after
                // which a separately indented bullet is a new top-level list).
                //
                // A CommonMark lazy continuation line is the exception: plain
                // paragraph text that immediately follows the item (no blank line
                // between) continues the item's open paragraph and so keeps the
                // list open. Constructs that interrupt a paragraph (ATX heading,
                // thematic break, fenced code, HTML block, HTML comment, div block)
                // end the list even without a blank line, matching markdownlint. A
                // line beginning with
                // a list marker is likewise not lazy paragraph text - it would start
                // a new list item - so it must still terminate stale ancestors (e.g.
                // a deeper bullet that pulldown-cmark treats as item content rather
                // than a sublist).
                //
                // Blockquotes need container awareness: a continuation in the *same*
                // quote (`> text` after `> 1. item`) is lazy, but newly entering a
                // quote (`> text` after a non-quoted item) interrupts the paragraph
                // and ends the list. So compare the previous line's quote depth, and
                // examine the marker on the quote-stripped content.
                let bq_depth = line_info.blockquote.as_ref().map_or(0, |bq| bq.nesting_level);
                let prev_line = line_idx.checked_sub(1).map(|i| &ctx.lines[i]);
                let prev_blank = prev_line.is_none_or(|p| p.is_blank);
                let prev_bq_depth = prev_line
                    .and_then(|p| p.blockquote.as_ref())
                    .map_or(0, |bq| bq.nesting_level);
                let same_container = prev_bq_depth == bq_depth;
                let text = line_info
                    .blockquote
                    .as_ref()
                    .map_or_else(|| line_info.content(ctx.content), |bq| bq.content.as_str());
                let trimmed = text.trim_start();
                let starts_like_list_marker = match trimmed.as_bytes().first() {
                    Some(b'-' | b'*' | b'+') => {
                        matches!(trimmed.as_bytes().get(1), Some(b' ' | b'\t'))
                    }
                    Some(c) if c.is_ascii_digit() => {
                        // CommonMark allows at most 9 digits in an ordered list marker.
                        // A longer digit run is not a marker, so the line can be lazy
                        // paragraph text rather than a list-interrupting item.
                        let after_digits = trimmed.trim_start_matches(|ch: char| ch.is_ascii_digit());
                        let num_digits = trimmed.len() - after_digits.len();
                        let mut rest = after_digits.chars();
                        (1..=9).contains(&num_digits)
                            && matches!(rest.next(), Some('.' | ')'))
                            && matches!(rest.next(), Some(' ' | '\t') | None)
                    }
                    _ => false,
                };
                // Lazy continuation only extends an OPEN paragraph. The previous line
                // must itself be paragraph text (or a list-item line whose paragraph the
                // current line continues), not a closed block such as a fenced code
                // block, heading, thematic break, HTML block/comment, or div marker.
                // After such a block, an unindented line starts a new paragraph and
                // closes the list instead of lazily continuing it.
                let prev_is_open_paragraph = prev_line.is_some_and(|p| {
                    !p.is_blank
                        && !p.in_code_block
                        && p.heading.is_none()
                        && !p.is_horizontal_rule
                        && !p.in_html_block
                        && !p.in_html_comment
                        && !p.is_div_marker
                });
                let is_lazy_paragraph_continuation = !prev_blank
                    && prev_is_open_paragraph
                    && same_container
                    && !starts_like_list_marker
                    && line_info.heading.is_none()
                    && !line_info.is_horizontal_rule
                    && !line_info.in_code_block
                    && !line_info.in_html_block
                    && !line_info.in_html_comment
                    && !line_info.is_div_marker;
                if is_lazy_paragraph_continuation {
                    // Lazy continuation: the list stays open, leave the stack intact.
                    continue;
                }
                Self::terminate_closed_items(ctx, line_info, &mut list_stack, bq_depth);
            }
        }

        // Re-indenting a marker moves its item's content column, so the lines the
        // item owns move with it. MD007 places every marker at an absolute column,
        // so nested markers are left to their own warnings.
        move_owned_lines(ctx, &moves, Nesting::Absolute, &mut warnings);
        drop_structure_changing_fixes(ctx, &mut warnings, Allowed::Nothing);
        Ok(warnings)
    }

    /// Optimized check using document structure
    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        // Get all warnings with their fixes
        let warnings = self.check(ctx)?;
        let warnings = warnings;

        // If no warnings, return original content
        if warnings.is_empty() {
            return Ok(ctx.content.to_string());
        }

        crate::utils::fix_utils::apply_warning_fixes(ctx.content, &warnings).map_err(LintError::InvalidInput)
    }

    /// Get the category of this rule for selective processing
    fn category(&self) -> RuleCategory {
        RuleCategory::List
    }

    /// Check if this rule should be skipped
    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Fast path: check if document likely has lists
        if ctx.content.is_empty() || !ctx.likely_has_lists() {
            return true;
        }
        // Verify unordered list items actually exist
        !ctx.has_unordered_list_items()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
