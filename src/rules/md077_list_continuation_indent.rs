//!
//! Rule MD077: List continuation content indentation
//!
//! See [docs/md077.md](../../docs/md077.md) for full documentation, configuration, and examples.

use std::ops::ControlFlow;

use crate::lint_context::{LineInfo, LintContext};
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::list_indent_shift::is_lazy_continuation;

mod md077_config;
use md077_config::MD077Config;

/// How strictly MD077 enforces continuation-line indentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContinuationStyle {
    /// Permit any continuation indent in `[0, content_column]` (CommonMark lazy
    /// continuation). Only over-indentation and content that escapes the list
    /// after a blank line are flagged.
    #[default]
    Any,
    /// Require every continuation line to align to the item's content column.
    /// Tight under-indented lazy continuation (which `any` permits) is also
    /// flagged and snapped up to the content column.
    Aligned,
}

/// Rule MD077: List continuation content indentation
///
/// In both tight continuation (no blank line) and loose continuation (after a
/// blank line), content must not be **over-indented** beyond the item's content
/// column. Additionally, in loose continuation content must not be
/// **under-indented** below the content column (W+N rule), or it falls out of the
/// list; tight under-indent is valid CommonMark lazy continuation and is left
/// alone. Content indented to the content column + 4 or more is an indented code
/// block, not continuation, and is not flagged.
///
/// Under the MkDocs flavor, a minimum of 4 spaces is enforced for ordered list
/// items to satisfy Python-Markdown.
///
/// With `style = "aligned"`, tight under-indented continuation is additionally
/// flagged and aligned to the content column (mdformat parity).
#[derive(Clone, Default)]
pub struct MD077ListContinuationIndent {
    config: MD077Config,
}

impl MD077ListContinuationIndent {}

impl MD077ListContinuationIndent {
    /// Width of a GFM task checkbox prefix including its trailing space:
    /// `[ ] `, `[x] `, or `[X] ` — always exactly 4 bytes.
    const TASK_CHECKBOX_PREFIX_LEN: usize = 4;

    /// Returns true if the item line starts a GFM task list item, i.e. its
    /// content column begins with `[ ] `, `[x] `, or `[X] `. The trailing
    /// space is part of the match — `- [ ]` with no body is an empty list
    /// item, not a task.
    ///
    /// Task items have a second, conventionally-accepted continuation column
    /// at `content_col + 4` (aligned after the checkbox). MD013 reflow
    /// produces this column for wrapped task lines, so MD077 has to accept
    /// it to avoid a fix loop with MD013.
    ///
    /// `content_col` is a byte offset into `line`, not a visual column. The
    /// CommonMark list parser produces byte-offset content columns, and the
    /// checkbox prefix `[ ] ` is pure ASCII, so this byte-level comparison
    /// is correct. Leading indent mixing tabs and spaces is irrelevant here
    /// because `content_col` already points past any leading whitespace.
    fn is_task_list_item(line: &str, content_col: usize) -> bool {
        line.as_bytes()
            .get(content_col..content_col + Self::TASK_CHECKBOX_PREFIX_LEN)
            .is_some_and(|window| matches!(window, b"[ ] " | b"[x] " | b"[X] "))
    }

    /// Check if a trimmed line is a block-level construct (not list continuation).
    fn is_block_level_construct(trimmed: &str) -> bool {
        // Footnote definition: [^label]:
        if trimmed.starts_with("[^") && trimmed.contains("]:") {
            return true;
        }
        // Abbreviation definition: *[text]:
        if trimmed.starts_with("*[") && trimmed.contains("]:") {
            return true;
        }
        // Reference link definition: [label]: url
        // Must start with [ but not be a regular link, footnote, or abbreviation
        if trimmed.starts_with('[') && !trimmed.starts_with("[^") && trimmed.contains("]: ") {
            return true;
        }
        false
    }

    /// Check if a trimmed line is a fenced code block delimiter (opener or closer).
    fn is_code_fence(trimmed: &str) -> bool {
        let bytes = trimmed.as_bytes();
        if bytes.len() < 3 {
            return false;
        }
        let ch = bytes[0];
        (ch == b'`' || ch == b'~') && bytes[1] == ch && bytes[2] == ch
    }

    /// Whether a line is the delimiter of a fenced code block. Inside an
    /// indented code block, fence-like text is code content.
    fn is_fence_delimiter(ctx: &LintContext, info: &LineInfo, trimmed: &str) -> bool {
        let line_end = info.byte_offset + info.byte_len;
        Self::is_code_fence(trimmed)
            && ctx
                .code_block_details
                .iter()
                .any(|b| b.is_fenced && b.start <= line_end && info.byte_offset < b.end)
    }

    /// Column of the `>` that opens a blockquote deeper than `depth`, if the
    /// line has one.
    fn deeper_quote_column(ctx: &LintContext, info: &LineInfo, depth: usize) -> Option<usize> {
        if info.blockquote.as_ref()?.nesting_level <= depth {
            return None;
        }
        // The prefix holds only whitespace and `>`, so the (depth + 1)th `>` is
        // the one that opens the deeper quote.
        info.content(ctx.content).match_indices('>').nth(depth).map(|(i, _)| i)
    }

    /// Whether moving a paragraph line from `from` to `to` turns its text into
    /// the start of a block.
    ///
    /// Four or more columns past the content column a line cannot interrupt the
    /// paragraph, so a fence, blockquote marker, setext underline, thematic break
    /// or table row there is paragraph text. Moved closer than that, the same
    /// text opens the block and swallows or splits what follows. Closer in, the
    /// parser already reads such a line as its block, so only this crossing
    /// changes what the line is.
    fn opens_block_when_moved(trimmed: &str, content_col: usize, from: usize, to: usize) -> bool {
        const MAX_BLOCK_INDENT: usize = 3;
        from > content_col + MAX_BLOCK_INDENT
            && to <= content_col + MAX_BLOCK_INDENT
            && (Self::is_code_fence(trimmed)
                || trimmed.starts_with('>')
                || crate::lint_context::is_setext_underline_content(trimmed)
                || crate::lint_context::is_horizontal_rule_content(trimmed)
                || crate::utils::skip_context::is_table_line(trimmed))
    }

    /// Check if a trimmed line starts with a list marker (*, -, +, or ordered).
    /// Used to avoid flagging deeply indented list items that the parser doesn't
    /// recognize as list items (e.g., with indent=8 configured in MD007).
    fn starts_with_list_marker(trimmed: &str) -> bool {
        let bytes = trimmed.as_bytes();
        match bytes.first() {
            Some(b'*' | b'-' | b'+') => bytes.get(1).is_some_and(|&b| b == b' ' || b == b'\t'),
            Some(b'0'..=b'9') => {
                let rest = trimmed.trim_start_matches(|c: char| c.is_ascii_digit());
                rest.starts_with(". ") || rest.starts_with(") ")
            }
            _ => false,
        }
    }

    /// When the line opens a fenced code block, the line number of the block's
    /// last line (its closer, or the last line of an unclosed block). The
    /// block's own byte range decides both, so an adjacent indented code block
    /// on either side is never mistaken for part of it.
    fn fenced_block_last_line(ctx: &LintContext, info: &LineInfo, trimmed: &str) -> Option<usize> {
        if !Self::is_code_fence(trimmed) {
            return None;
        }
        let line_end = info.byte_offset + info.byte_len;
        ctx.code_block_details
            .iter()
            .find(|b| b.is_fenced && info.byte_offset <= b.start && b.start <= line_end)
            .map(|b| ctx.offset_to_line_col(b.end.saturating_sub(1).max(b.start)).0)
    }

    /// Build an atomic fix that reindents a fenced code block from its opener
    /// through its matching closer.
    ///
    /// - **Opener and closer** are moved to `required` (the list item's
    ///   content column, which is what MD077 actually flagged).
    /// - **Interior lines** move by the same delta as the fence and are then
    ///   clamped to `required`. Shifting by the delta preserves each line's
    ///   offset from the fence, so the code block's internal structure (JSON
    ///   nesting, indented sub-blocks) survives the move; the clamp
    ///   guarantees fence pairing, since every non-blank line in the block
    ///   ends at column ≥ `required` and the block stays inside the list
    ///   item's scope after the fix.
    ///
    /// Only used for the under-indent direction (`required > opener_actual`);
    /// over-indented fences are intentionally left untouched (see the
    /// over-indent pass in `check`), so there is no down-shift case to handle.
    ///
    /// Why a compound fix rather than three independent fixes? MD077 and
    /// MD031 run in the same iterative fix loop. If we only moved the
    /// delimiters, an intermediate state would have mismatched
    /// opener/closer indentation and MD031 would misread the block as
    /// unpaired, injecting stray blank lines (issue #574).
    ///
    /// Why clamp the shift rather than shift alone? A bare delta shift is
    /// not idempotent: interior starting below the list scope (e.g., col 0
    /// under an opener at col 2 that needs to move to col 3) lands at col 1,
    /// still below the scope, and the next MD077 pass re-flags it and snaps
    /// it to `required`. The clamp reaches that end state in a single pass.
    /// Clamping alone is not enough either: it pins every interior line at
    /// or above `required` to its original column while the fence rises, so
    /// the block's interior indentation shrinks by the delta and one-space
    /// nesting disappears entirely.
    ///
    /// Leading tabs are normalized to spaces: CommonMark expands a tab to
    /// the next column that's a multiple of 4, so simply prepending spaces
    /// before a tab would let the tab snap back and cancel the shift. We
    /// replace the whole leading-whitespace byte range with spaces.
    fn build_compound_fence_fix(
        ctx: &LintContext,
        opener_line: usize,
        closer_line: usize,
        opener_actual: usize,
        required: usize,
    ) -> Option<Fix> {
        if required <= opener_actual {
            return None;
        }
        let opener_info = ctx.line_info(opener_line)?;
        let closer_info = ctx.line_info(closer_line)?;

        let fix_start = opener_info.byte_offset;
        let fix_end = closer_info.byte_offset + closer_info.byte_len;

        let mut replacement = String::new();
        for i in opener_line..=closer_line {
            let info = ctx.line_info(i)?;
            if i > opener_line {
                replacement.push('\n');
            }
            let line = info.content(ctx.content);
            if info.is_blank {
                // Blank lines have no content to shift; preserve verbatim.
                replacement.push_str(line);
            } else {
                let new_visual = if i == opener_line || i == closer_line {
                    required
                } else {
                    (info.visual_indent + (required - opener_actual)).max(required)
                };
                for _ in 0..new_visual {
                    replacement.push(' ');
                }
                replacement.push_str(&line[info.indent..]);
            }
        }

        Some(Fix::new(fix_start..fix_end, replacement))
    }

    /// Walk the continuation lines owned by a single list item, invoking
    /// `per_line` for each *in-scope, non-blank, non-nested, non-skipped*
    /// line with its pre-computed visual column and loose/tight state.
    ///
    /// This is the **single source of truth** for MD077's item-scope
    /// traversal: both the sibling-column pre-pass and the main check loop
    /// route through this method so their termination semantics cannot
    /// drift. The callback sees only lines the rule actually needs to
    /// reason about; it can return `ControlFlow::Break` for early exit.
    ///
    /// Termination conditions (applied before the callback fires):
    /// - Headings and horizontal rules end the item unconditionally.
    /// - After a blank line, content at or below the marker column has
    ///   escaped the item; further lines are not delivered.
    ///
    /// Skipped silently (do not fire the callback):
    /// - Blank lines (toggle `saw_blank`).
    /// - Nested list items (reset `saw_blank`, track their content column).
    /// - Lines inside any still-open nested item's scope: the walk keeps a
    ///   stack of open nested (marker, content) columns, so a continuation
    ///   of a middle nesting level defers to that level's own walk instead
    ///   of being claimed by a shallower ancestor.
    /// - Reference/footnote/abbreviation definitions and similar block
    ///   constructs that aren't list continuation.
    /// - Lines that `should_skip_line` rejects (code-block interior etc.).
    fn walk_item_continuation<F>(
        ctx: &LintContext,
        item_line: usize,
        range_end: usize,
        marker_col: usize,
        mut per_line: F,
    ) where
        F: FnMut(&ContinuationLine<'_>) -> ControlFlow<()>,
    {
        let mut saw_blank = false;
        let mut saw_nested = false;
        // Stack of open nested items as (marker_column, content_column),
        // outermost first. A single scalar is not enough: with 3+ levels a
        // continuation of a middle level sits below the innermost content
        // column but at-or-past its own level's, and must still be skipped
        // here so the owning level's walk evaluates it.
        let mut nested_stack: Vec<(usize, usize)> = Vec::new();
        let item_info = ctx.line_info(item_line);
        let item_quotes = item_info
            .and_then(|i| i.blockquote.as_ref())
            .map_or(0, |q| q.nesting_level);
        let content_col = item_info
            .and_then(|i| i.list_item.as_ref())
            .map_or(marker_col + 1, |li| li.content_column);

        for line_num in (item_line + 1)..=range_end {
            let Some(info) = ctx.line_info(line_num) else {
                continue;
            };

            // A `>` left of the content column opens a blockquote outside the
            // item, which ends the item (an empty quote line is not a blank line
            // inside it).
            if Self::deeper_quote_column(ctx, info, item_quotes).is_some_and(|col| col < content_col) {
                break;
            }

            let trimmed = info.content(ctx.content).trim_start();

            if Self::should_skip_line(ctx, info, trimmed) {
                continue;
            }

            if info.is_blank {
                saw_blank = true;
                continue;
            }

            if let Some(ref li) = info.list_item {
                // A marker left of the content column cannot nest inside this
                // item: it closes the item and starts a list of its own, which
                // then owns every line below it.
                if li.marker_column > marker_col && li.marker_column < content_col {
                    break;
                }
                if li.marker_column > marker_col {
                    // A sibling-or-shallower marker closes every nested item at
                    // or past its column before this one opens.
                    while nested_stack.last().is_some_and(|&(m, _)| m >= li.marker_column) {
                        nested_stack.pop();
                    }
                    nested_stack.push((li.marker_column, li.content_column));
                    // Sticky: once a nested child appears, every later line is a
                    // (lazy) continuation of the deeper item, which owns it under
                    // CommonMark. Ancestors must defer so the deepest item claims
                    // and aligns it.
                    saw_nested = true;
                } else {
                    nested_stack.clear();
                }
                saw_blank = false;
                continue;
            }

            // A setext heading's text is the whole paragraph its underline ends,
            // so every one of those lines is a heading rather than a
            // continuation the walk could reindent.
            if info.heading.is_some() || info.is_setext_heading_text || info.is_horizontal_rule {
                break;
            }

            if Self::is_block_level_construct(trimmed) {
                continue;
            }

            let col = info.visual_indent;

            // A lazy continuation line continues the open paragraph of the
            // innermost nested item without closing it, whatever its indent, so
            // that item keeps owning the lines after it.
            if !nested_stack.is_empty() && is_lazy_continuation(ctx, line_num - 1) {
                continue;
            }

            // Any other line escapes every nested item whose content column it
            // sits below; if any nested item stays open, the line is that item's
            // continuation and its own walk owns the judgement.
            while nested_stack.last().is_some_and(|&(_, c)| c > col) {
                nested_stack.pop();
            }
            if !nested_stack.is_empty() {
                continue;
            }

            if saw_blank && col <= marker_col {
                break;
            }

            let line = ContinuationLine {
                line_num,
                info,
                trimmed,
                actual: col,
                saw_blank,
                saw_nested,
            };
            if per_line(&line).is_break() {
                break;
            }
        }
    }

    /// Whether the item's owned continuation range holds a *latent* structural
    /// construct: a list marker or table line the parser has not yet promoted to
    /// its own block, but would once an earlier continuation line is reindented.
    /// Such a promotion re-attributes the following lines to a different block
    /// (different content column) and breaks single-pass idempotency, so the
    /// caller skips the whole item when this returns true.
    ///
    /// A setext underline counts too: as a lazy continuation the parser reads a
    /// `=`/`-` run as paragraph text, but reindenting the prose line above it
    /// into the item turns the pair into a heading.
    ///
    /// Unlike [`Self::walk_item_continuation`], this scans the full owned range and
    /// does NOT stop at a heading or horizontal rule. A heading partway through
    /// the continuation can itself be parse-unstable: an empty backtick run, for
    /// instance, is read as a setext heading, and reindenting an earlier line
    /// dissolves it and promotes a list marker that sits past it. Stopping at the
    /// heading would hide that marker from this check and let the reindent break
    /// idempotency. Lines already parsed as list items are real nested children,
    /// not latent, and are handled by the walk's `saw_nested` logic instead.
    fn item_range_has_latent_structure(ctx: &LintContext, item_line: usize, range_end: usize) -> bool {
        (item_line + 1..=range_end).any(|line_num| {
            ctx.line_info(line_num).is_some_and(|info| {
                if info.is_blank || info.list_item.is_some() {
                    return false;
                }
                let trimmed = info.content(ctx.content).trim_start();
                !Self::should_skip_line(ctx, info, trimmed)
                    && (Self::starts_with_list_marker(trimmed)
                        || crate::utils::skip_context::is_table_line(trimmed)
                        || Self::is_latent_setext_underline(ctx, line_num, trimmed))
            })
        })
    }

    /// Whether reindenting the line above this one would turn the pair into a
    /// setext heading. An underline that is a lazy continuation is paragraph
    /// text, so the parser reports no heading and the walk runs straight past it.
    ///
    /// Only paragraph text can become the heading's text line, so an underline
    /// under an ATX heading, a thematic break or another underline is latent
    /// nothing and must not hold the whole item back. The line above has to pass
    /// both readings: `is_paragraph_context` speaks for the block it sits in,
    /// which is what tells a closing code fence from the prose it looks like,
    /// and the text predicate speaks for the line's own markers, which is what
    /// tells an empty list item from the text it holds none of. A blank line
    /// needs no test of its own: blank means it holds nothing once its markers
    /// are stripped, which is the first thing the text predicate rejects.
    fn is_latent_setext_underline(ctx: &LintContext, line_num: usize, trimmed: &str) -> bool {
        crate::lint_context::is_setext_underline_content(trimmed)
            && ctx.line_info(line_num - 1).is_some_and(|prev| {
                prev.is_paragraph_context() && crate::lint_context::is_paragraph_text_line(prev.content(ctx.content))
            })
    }

    /// Scan an item's owned range and report whether any *other* continuation
    /// line in the item uses the content column or the post-checkbox column.
    ///
    /// Used exclusively for tie-breaking the auto-fix target when an
    /// over-indented line is exactly equidistant from `content_col` and
    /// `task_col`. In that case the author's intent is ambiguous, so we
    /// snap to whichever valid column *they're already using* elsewhere in
    /// the same item. When neither or both columns are in use, the caller
    /// falls back to a canonical default.
    fn sibling_column_usage(
        ctx: &LintContext,
        item_line: usize,
        range_end: usize,
        marker_col: usize,
        content_col: usize,
        task_col: usize,
    ) -> (bool, bool) {
        let mut uses_content = false;
        let mut uses_task = false;

        Self::walk_item_continuation(ctx, item_line, range_end, marker_col, |line| {
            if line.actual == content_col {
                uses_content = true;
            }
            if line.actual == task_col {
                uses_task = true;
            }
            if uses_content && uses_task {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });

        (uses_content, uses_task)
    }

    /// Compute the auto-fix target for an over-indented continuation line.
    /// Snaps to the nearer of the two valid columns (content_col / task_col)
    /// for task items, and on an exact tie uses sibling-column context to
    /// pick whichever column the author is already using elsewhere in this
    /// item. Non-task items always snap to `required`.
    fn compute_fix_target(
        actual: usize,
        required: usize,
        task_col: Option<usize>,
        uses_content_col: bool,
        uses_task_col: bool,
    ) -> usize {
        let Some(t) = task_col else { return required };
        match actual.abs_diff(t).cmp(&actual.abs_diff(required)) {
            std::cmp::Ordering::Less => t,
            std::cmp::Ordering::Greater => required,
            std::cmp::Ordering::Equal => match (uses_task_col, uses_content_col) {
                (true, false) => t,
                _ => required,
            },
        }
    }

    /// Check if a line should be skipped (inside code, HTML, frontmatter, etc.)
    ///
    /// Code block *content* is skipped, but fence opener/closer lines are not —
    /// their indentation matters for list continuation in MkDocs.
    ///
    /// Footnote-definition lines (the `[^id]:` line and its indented body) are
    /// skipped too: a footnote body that follows a list is its own block, not
    /// list continuation, so its indentation is MD068's concern, not MD077's.
    /// Treating it as continuation produced false over-indent warnings and a
    /// damaging auto-fix that reindented the body and then tripped MD068.
    fn should_skip_line(ctx: &LintContext, info: &LineInfo, trimmed: &str) -> bool {
        if info.in_code_block && !Self::is_fence_delimiter(ctx, info, trimmed) {
            return true;
        }
        info.in_front_matter
            || info.in_footnote_definition
            || info.in_html_block
            || info.in_html_comment
            || false
            || info.in_mkdocstrings
            || false
            || info.in_math_block
            || info.in_admonition
            || info.in_content_tab
            || false
            || false
            || info.in_mkdocs_html_markdown
            || false
    }

    /// Build the warning for an over-indented continuation line. The fix is a
    /// single-line rewrite of the leading whitespace to `fix_target`.
    ///
    /// The over-indent pass never delivers fenced-code lines here (it skips
    /// anything `in_code_block`), so this builder does not need the compound
    /// fence handling that `build_under_indent_warning` uses: moving an
    /// over-indented fence's delimiters without its body would corrupt the code
    /// content, so over-indented fenced blocks are deliberately left as-is.
    fn build_over_indent_warning(
        ctx: &LintContext,
        line: &ContinuationLine<'_>,
        fix_target: usize,
        message: String,
    ) -> LintWarning {
        let line_content = line.info.content(ctx.content);
        let fix_start = line.info.byte_offset;
        let fix_end = fix_start + line.info.indent;
        LintWarning {
            rule_name: Some("MD077".to_string()),
            line: line.line_num,
            column: 1,
            end_line: line.line_num,
            end_column: line_content.chars().count() + 1,
            message,
            severity: Severity::Warning,
            fix: Some(Fix::new(fix_start..fix_end, " ".repeat(fix_target))),
        }
    }

    /// Build the warning for a loose-mode under-indented continuation line.
    /// When the line is the opener of a fenced code block, emit a compound
    /// fix that reindents opener + interior + closer atomically so MD031
    /// doesn't see a transiently-broken fence pair (see #574).
    ///
    /// Returns the warning plus, when the fix is compound, the closer
    /// line number so the caller can mark it flagged (preventing the
    /// main loop from double-flagging the closer as its own under-indent
    /// case). Keeping the "also flag this line" signal out of band keeps
    /// this function pure — it reads from `ctx` only and returns a
    /// plain value.
    fn build_under_indent_warning(
        ctx: &LintContext,
        line: &ContinuationLine<'_>,
        required: usize,
        message: String,
    ) -> UnderIndentOutcome {
        let line_content = line.info.content(ctx.content);
        let fence_closer = Self::fenced_block_last_line(ctx, line.info, line.trimmed);

        let (fix, warn_end_line, warn_end_column, compound_closer) = if let Some(closer_line) = fence_closer {
            let fix = Self::build_compound_fence_fix(ctx, line.line_num, closer_line, line.actual, required);
            let end_column = ctx
                .line_info(closer_line)
                .map_or(line_content.chars().count() + 1, |ci| {
                    ci.content(ctx.content).chars().count() + 1
                });
            let extra_flag = (closer_line != line.line_num).then_some(closer_line);
            (fix, closer_line, end_column, extra_flag)
        } else {
            let fix_start = line.info.byte_offset;
            let fix_end = fix_start + line.info.indent;
            let fix = Some(Fix::new(fix_start..fix_end, " ".repeat(required)));
            (fix, line.line_num, line_content.chars().count() + 1, None)
        };

        UnderIndentOutcome {
            warning: LintWarning {
                rule_name: Some("MD077".to_string()),
                line: line.line_num,
                column: 1,
                end_line: warn_end_line,
                end_column: warn_end_column,
                message,
                severity: Severity::Warning,
                fix,
            },
            also_flag_line: compound_closer,
        }
    }
}

/// A continuation line yielded by `walk_item_continuation`. Bundles the
/// per-line facts both checker branches need so helper functions don't
/// balloon their argument lists.
struct ContinuationLine<'a> {
    line_num: usize,
    info: &'a LineInfo,
    trimmed: &'a str,
    actual: usize,
    saw_blank: bool,
    /// True once a nested child item has appeared earlier in this item's scope.
    /// Subsequent lines are lazy continuation of the deeper item (which owns
    /// them under CommonMark), so an ancestor must not claim them.
    saw_nested: bool,
}

/// Result of `build_under_indent_warning`. Carries both the warning and,
/// when the fix is compound (fence opener → promote-to-required over the
/// whole block), the closer line so the caller can record it as already
/// handled. This keeps the warning builder free of external mutation.
struct UnderIndentOutcome {
    warning: LintWarning,
    also_flag_line: Option<usize>,
}

impl Rule for MD077ListContinuationIndent {
    fn name(&self) -> &'static str {
        "MD077"
    }

    fn description(&self) -> &'static str {
        "List continuation content indentation"
    }

    fn check(&self, ctx: &LintContext) -> LintResult {
        if ctx.content.is_empty() {
            return Ok(Vec::new());
        }

        let strict_indent = false;
        let total_lines = ctx.lines.len();
        let mut warnings = Vec::new();
        let mut flagged_lines = std::collections::HashSet::new();

        // Collect all list item lines sorted, with their content_column,
        // marker_column, and — if the item is a GFM task — its post-checkbox
        // column. Precomputing task_col here (instead of re-reading line_info
        // inside the hot inner loop) keeps the per-item cost O(1).
        //
        // We need the owned range to extend past block.end_line because the
        // parser excludes under-indented continuation from the block, and
        // MD077 specifically has to evaluate those escaped lines.
        let mut items: Vec<(usize, usize, usize, Option<usize>)> = Vec::new();
        for block in &ctx.list_blocks {
            for &item_line in &block.item_lines {
                if let Some(info) = ctx.line_info(item_line)
                    && let Some(ref li) = info.list_item
                {
                    // Lists inside blockquotes are out of scope for MD077. Their
                    // content column is measured on the raw line, so it is
                    // meaningless for a lazy continuation sitting outside the
                    // quote: scoping them lets two items with different content
                    // columns each claim such a line and reindent it the other
                    // way on alternating passes.
                    if info.blockquote.is_some() {
                        continue;
                    }
                    let line = info.content(ctx.content);
                    let task_col = Self::is_task_list_item(line, li.content_column)
                        .then_some(li.content_column + Self::TASK_CHECKBOX_PREFIX_LEN);
                    items.push((item_line, li.marker_column, li.content_column, task_col));
                }
            }
        }
        items.sort_unstable();
        items.dedup_by_key(|&mut (ln, _, _, _)| ln);

        // Precompute each item's owned-range end. The range ends at the line
        // before the next item whose marker column is <= this item's (its next
        // sibling or an ancestor), or the last line if none follows.
        //
        // The direct "scan forward for the next marker_col <= mine" is O(n^2) on
        // a monotonically deepening list, where no later item ever qualifies so
        // every item scans to the end. A monotonic stack (nearest
        // smaller-or-equal marker column to the right) computes all range ends in
        // one linear pass. Walking right-to-left, pop every stacked item whose
        // marker column is strictly greater than the current one (they can never
        // be the sibling/ancestor of anything further left that is <= current),
        // leaving the nearest qualifying item on top.
        let mut range_ends = vec![total_lines; items.len()];
        let mut stack: Vec<usize> = Vec::new();
        for i in (0..items.len()).rev() {
            let marker_col = items[i].1;
            while let Some(&top) = stack.last() {
                if items[top].1 > marker_col {
                    stack.pop();
                } else {
                    break;
                }
            }
            range_ends[i] = stack.last().map_or(total_lines, |&j| items[j].0 - 1);
            stack.push(i);
        }

        // Precompute each item's required indent and owned line range so both
        // passes below scope identically.
        let scoped: Vec<(usize, usize, usize, Option<usize>, usize, usize)> = items
            .iter()
            .enumerate()
            .map(|(item_idx, &(item_line, marker_col, content_col, task_col))| {
                // A configured indent replaces the content column, but under a
                // flavor that requires strict list indentation it may only raise
                // the 4-space minimum. That minimum is a compatibility floor
                // rather than a style default: below it the renderer stops
                // treating the line as continuation, so honoring a smaller
                // configured value would make `fix` rewrite documents into a
                // form the flavor's renderer no longer reads as part of the item.
                let required = match self.config.indent {
                    Some(indent) if strict_indent => (marker_col + indent).max(4),
                    Some(indent) => marker_col + indent,
                    None if strict_indent => content_col.max(4),
                    None => content_col,
                };
                (
                    item_line,
                    marker_col,
                    content_col,
                    task_col,
                    required,
                    range_ends[item_idx],
                )
            })
            .collect();

        // Precompute which lines can ever reach the per-line continuation
        // callback. `walk_item_continuation` unconditionally skips (regardless of
        // which item owns the line) blank lines, list-item lines, headings,
        // horizontal rules, `should_skip_line` lines, and block-level constructs;
        // only the lines that survive all of those can be flagged. An item whose
        // owned range contains none of them produces no warning, so both passes
        // can skip it without walking the range at all.
        //
        // This keeps a document of purely deeply-nested list items linear: such a
        // document has no continuation prose, so every item skips its walk
        // instead of re-scanning the whole tail (which was O(n^2) in the item
        // count). Documents that do have continuation prose are unaffected - the
        // guard only ever skips items that could not have produced a warning.
        let prose_candidate_lines: Vec<usize> = (1..=total_lines)
            .filter(|&line_num| {
                let Some(info) = ctx.line_info(line_num) else {
                    return false;
                };
                let trimmed = info.content(ctx.content).trim_start();
                !Self::should_skip_line(ctx, info, trimmed)
                    && !info.is_blank
                    && info.list_item.is_none()
                    && info.heading.is_none()
                    && !info.is_setext_heading_text
                    && !info.is_horizontal_rule
                    && !Self::is_block_level_construct(trimmed)
            })
            .collect();
        // True when a continuation candidate falls in `(after_line, range_end]`,
        // i.e. the half-open range `walk_item_continuation` actually visits.
        let range_has_prose_candidate = |after_line: usize, range_end: usize| -> bool {
            let start = prose_candidate_lines.partition_point(|&l| l <= after_line);
            prose_candidate_lines.get(start).is_some_and(|&l| l <= range_end)
        };

        // Pass 1 - under-indented continuation.
        //
        // Loose under-indent (after a blank line) sits below the content column
        // and would escape the list item; it is always flagged. Tight
        // under-indent is valid CommonMark lazy continuation, so it is left
        // alone under `style = "any"`.
        //
        // Under `style = "aligned"`, tight under-indent is additionally flagged
        // and snapped up to the content column. MD077 scopes past the parser's
        // block end (to catch escaped loose content), and the parser absorbs
        // col-0 blockquotes, fences, and tables that sit tight under a list item
        // as lazy continuation. Reindenting those would change the rendered
        // structure, so the tight-aligned branch guards two ways: it skips lines
        // the parser classified in place (code, blockquote - see
        // `confirmed_structure`), and it bails on an item whose continuation
        // holds a construct the parser would re-parse once indentation changes
        // (a latent list marker, table or setext underline - see
        // `has_latent_structure`). Headings and horizontal rules already
        // terminate the walk; reference/footnote/abbreviation definitions are
        // skipped inside it. Erring toward skipping yields false negatives
        // (safe) rather than structural false positives.
        //
        // This pass runs first so that a deeply nested item claims an ambiguous
        // line - one that is under-indented for it yet over-indented for a
        // shallower ancestor - before pass 2 can mis-attribute it to the
        // ancestor as an over-indent and snap it the wrong way.
        let aligned = self.config.style == ContinuationStyle::Aligned;
        for &(item_line, marker_col, _content_col, _task_col, required, range_end) in &scoped {
            // No continuation candidate in this item's range: it cannot produce a
            // warning, so skip the walk (and the latent-structure scan) entirely.
            if !range_has_prose_candidate(item_line, range_end) {
                continue;
            }
            // "Latent structure": a line in this item's continuation that the
            // parser has NOT yet promoted to a list item or table, but that
            // would be once an earlier continuation line is reindented. Such a
            // promotion re-attributes the following lines to a different block
            // (different content column) and breaks single-pass idempotency, so
            // reindenting anything in the item is unsafe and the whole item is
            // skipped. This is a *prediction* of how the parser would re-parse
            // after a fix, which is why it stays a heuristic (markers, tables and
            // setext underlines are the constructs the parser absorbs as lazy
            // continuation rather than flagging on the line itself; code and
            // blockquotes are classified in place and handled per line below).
            // Well-formed prose continuation matches none, so this only bails
            // on pathological input (a safe false negative). Real nested items
            // never reach the callback (the walk handles them via `saw_nested`).
            let has_latent_structure = aligned && Self::item_range_has_latent_structure(ctx, item_line, range_end);
            // Which setting produced `required` decides what the message may
            // claim. The content column carries a structural consequence (below
            // it the content leaves the item) and the strict-flavor minimum
            // carries a renderer requirement, but a configured indent above
            // both is a house style, so it is reported as one.
            let from_configured_indent = self.config.indent.is_some_and(|indent| marker_col + indent == required);
            Self::walk_item_continuation(ctx, item_line, range_end, marker_col, |line| {
                let actual = line.actual;
                let under_indented = actual < required;
                let loose_escape = line.saw_blank && under_indented;
                // Don't reindent a line the parser classified in place as a code
                // block (fenced or indented) or a blockquote: these are read
                // straight from its per-line output, so the rule cannot drift
                // from how rumdl parses the document. Headings and horizontal
                // rules terminate the walk before the callback; latent list
                // markers and tables are handled by `has_latent_structure`.
                let confirmed_structure = line.info.in_code_block || line.info.blockquote.is_some();
                let aligned_tight = aligned
                    && !has_latent_structure
                    && !line.saw_blank
                    && !line.saw_nested
                    && under_indented
                    && !confirmed_structure;
                if (loose_escape || aligned_tight) && flagged_lines.insert(line.line_num) {
                    let message = if line.saw_blank {
                        if from_configured_indent {
                            format!(
                                "Content after blank line in list item needs {required} spaces of \
                                 indentation to match the configured indent (found {actual})",
                            )
                        } else if strict_indent {
                            format!(
                                "Content inside list item needs {required} spaces of indentation \
                                 for MkDocs compatibility (found {actual})",
                            )
                        } else {
                            format!(
                                "Content after blank line in list item needs {required} spaces of \
                                 indentation to remain part of the list (found {actual})",
                            )
                        }
                    } else {
                        format!("Continuation line under-indented (expected {required}, found {actual})")
                    };
                    let outcome = Self::build_under_indent_warning(ctx, line, required, message);
                    if let Some(closer_line) = outcome.also_flag_line {
                        flagged_lines.insert(closer_line);
                    }
                    warnings.push(outcome.warning);
                }
                ControlFlow::Continue(())
            });
        }

        // Pass 2 - over-indented continuation (tight or loose): prose pushed
        // past the content column is snapped back. Fenced code blocks are
        // skipped here (`!in_code_block`): an over-indented fence is cosmetic
        // (the code still renders), and reindenting only its delimiters - the
        // body is skipped by `should_skip_line` - would alter the literal code
        // content. Indented code blocks (content column + 4 or more) are also
        // `in_code_block`, so a blank line before such a body does not exempt
        // it from being recognized as code rather than over-indented prose.
        for &(item_line, marker_col, content_col, task_col, required, range_end) in &scoped {
            // No continuation candidate in this item's range: nothing to flag.
            if !range_has_prose_candidate(item_line, range_end) {
                continue;
            }
            // For task items, gather sibling-column usage once so the auto-fix
            // can tie-break equidistant over-indents toward whichever valid
            // column the author is already using.
            let (uses_content_col, uses_task_col) = match task_col {
                Some(t) => Self::sibling_column_usage(ctx, item_line, range_end, marker_col, content_col, t),
                None => (false, false),
            };

            Self::walk_item_continuation(ctx, item_line, range_end, marker_col, |line| {
                let actual = line.actual;
                if actual <= required {
                    return ControlFlow::Continue(());
                }
                let fix_target = Self::compute_fix_target(actual, required, task_col, uses_content_col, uses_task_col);
                if !line.info.in_code_block
                    && Some(actual) != task_col
                    && !Self::starts_with_list_marker(line.trimmed)
                    && !Self::opens_block_when_moved(line.trimmed, content_col, actual, fix_target)
                    && flagged_lines.insert(line.line_num)
                {
                    let message = match task_col {
                        Some(t) => format!(
                            "Continuation line over-indented \
                             (expected {required} or {t}, found {actual})"
                        ),
                        None => {
                            format!("Continuation line over-indented (expected {required}, found {actual})")
                        }
                    };
                    warnings.push(Self::build_over_indent_warning(ctx, line, fix_target, message));
                }
                ControlFlow::Continue(())
            });
        }

        // The two passes emit independently, so order by position before
        // returning - callers and tests expect document order.
        warnings.sort_by_key(|w| (w.line, w.column));

        Ok(warnings)
    }

    fn fix(&self, ctx: &LintContext) -> Result<String, LintError> {
        let warnings = self.check(ctx)?;
        let warnings = warnings;
        if warnings.is_empty() {
            return Ok(ctx.content.to_string());
        }

        // Sort fixes by byte position descending to apply from end to start
        let mut fixes: Vec<Fix> = warnings.into_iter().filter_map(|w| w.fix).collect();
        fixes.sort_by_key(|f| std::cmp::Reverse(f.range.start));

        let mut content = ctx.content.to_string();
        for fix in fixes {
            if fix.range.start <= content.len() && fix.range.end <= content.len() {
                content.replace_range(fix.range, &fix.replacement);
            }
        }

        Ok(content)
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::List
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        ctx.content.is_empty() || ctx.list_blocks.is_empty()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
