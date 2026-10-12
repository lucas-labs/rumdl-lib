//! Utilities for applying fixes consistently between CLI and LSP
//!
//! This module provides shared logic for applying markdown fixes to ensure
//! that both CLI batch fixes and LSP individual fixes produce identical results.

use crate::rule::{Fix, LintWarning};
use crate::utils::ensure_consistent_line_endings;
use std::borrow::Cow;
use std::ops::Range;

/// Apply a list of warning fixes to content, simulating how the LSP client would apply them
/// This is used for testing consistency between CLI and LSP fix methods
pub(crate) fn apply_warning_fixes(content: &str, warnings: &[LintWarning]) -> Result<String, String> {
    let mut fixes: Vec<(usize, &Fix)> = warnings
        .iter()
        .enumerate()
        .filter_map(|(i, w)| w.fix.as_ref().map(|fix| (i, fix)))
        .flat_map(|(i, fix)| {
            // A logical fix may carry additional edits at separate ranges
            // (e.g. MD054 ref-emit fixes that rewrite a link in place AND
            // append a new ref definition at EOF). Flatten so each edit
            // participates in the same dedup/sort/apply pipeline.
            std::iter::once((i, fix)).chain(fix.additional_edits.iter().map(move |e| (i, e)))
        })
        .collect();

    // No-op fast path: if there are no actual fixes to apply, return the
    // content unchanged. This avoids unnecessary line-ending normalization
    // when all warnings were filtered out (e.g., by inline config) or had
    // no fix attached.
    if fixes.is_empty() {
        return Ok(content.to_string());
    }

    // Sort ascending so the dedup/coalesce pass sees fixes that share a range
    // as adjacent neighbors. Tie-break on warning index so declaration order
    // is preserved when we later concatenate same-offset zero-width inserts.
    fixes.sort_by(|(idx_a, fix_a), (idx_b, fix_b)| {
        let range_cmp = fix_a.range.start.cmp(&fix_b.range.start);
        if range_cmp != std::cmp::Ordering::Equal {
            return range_cmp;
        }
        let end_cmp = fix_a.range.end.cmp(&fix_b.range.end);
        if end_cmp != std::cmp::Ordering::Equal {
            return end_cmp;
        }
        idx_a.cmp(idx_b)
    });

    // Dedup identical (range, replacement) pairs AND coalesce same-offset
    // zero-width inserts into a single logical edit by concatenating their
    // replacements in declaration order.
    //
    // The coalesce step is required because `replace_range(N..N, X)` followed
    // by `replace_range(N..N, Y)` on the *same* document position produces
    // `Y X` — `X` is already at offset N when `Y` inserts, so `Y` lands
    // before it. With per-warning insertion (e.g., several MD054 ref-emit
    // fixes appending different `[label]: url` definitions at EOF), that
    // would reverse declaration order. Concatenating up front gives one
    // `replace_range(N..N, X + Y)` that lands `X` then `Y` in source order.
    let mut applicable: Vec<ApplicableEdit<'_>> = Vec::with_capacity(fixes.len());
    let mut i = 0;
    while i < fixes.len() {
        let (_, current) = fixes[i];
        let mut combined: Option<String> = None;
        let is_zero_width = current.range.start == current.range.end;
        let mut j = i + 1;
        while j < fixes.len() {
            let (_, next) = fixes[j];
            if next.range != current.range {
                break;
            }
            if next.replacement == current.replacement {
                // Pure duplicate — drop and continue scanning siblings.
                j += 1;
                continue;
            }
            if !is_zero_width {
                // Two different replacements competing for the same non-zero
                // range is a rule-authoring bug at the call site, not something
                // we can sensibly merge. Stop here so the apply loop sees only
                // the first replacement (matching prior behavior).
                break;
            }
            // Zero-width inserts at the same offset: concatenate.
            let buf = combined.get_or_insert_with(|| current.replacement.clone());
            buf.push_str(&next.replacement);
            j += 1;
        }

        applicable.push(ApplicableEdit {
            range: current.range.clone(),
            replacement: match combined {
                Some(owned) => Cow::Owned(owned),
                None => Cow::Borrowed(current.replacement.as_str()),
            },
        });
        i = j;
    }

    // Reverse-sort by range start so earlier-offset edits stay valid as later
    // ones mutate the buffer. Coalescing collapsed the previous tertiary
    // tiebreak case, so a simple two-key sort is enough.
    applicable.sort_by(|a, b| {
        let cmp = b.range.start.cmp(&a.range.start);
        if cmp != std::cmp::Ordering::Equal {
            return cmp;
        }
        b.range.end.cmp(&a.range.end)
    });

    // The edits that survive, collected last to first and assembled in one
    // forward pass, so applying many fixes costs one copy of the document
    // rather than a shift of its tail per edit.
    let mut kept: Vec<ApplicableEdit<'_>> = Vec::with_capacity(applicable.len());

    // Track the lowest byte offset touched by an already-applied fix.
    // Since fixes are sorted in reverse order (highest start first),
    // any subsequent fix whose range.end > min_applied_start would
    // overlap with an already-applied fix and corrupt the result.
    let mut min_applied_start = usize::MAX;

    for edit in applicable {
        // Every range addresses the content as the rule saw it, so validate against
        // that and not the buffer edits have already shrunk. Measuring the mutated
        // buffer rejected ranges that were merely about to be skipped as
        // overlapping, and the whole document's fix failed instead: MD039 declined
        // to fix `[ a ![ x ](i.png) b ](t.md)` at all, and whether it did depended
        // on how much unrelated text the document held after that line.
        if edit.range.end > content.len() {
            return Err(format!(
                "Fix range end {} exceeds content length {}",
                edit.range.end,
                content.len()
            ));
        }

        if edit.range.start > edit.range.end {
            return Err(format!(
                "Invalid fix range: start {} > end {}",
                edit.range.start, edit.range.end
            ));
        }

        // Reject ranges that do not lie on UTF-8 char boundaries. replace_range
        // would panic on such a range; a rule emitting one is a bug, so surface
        // it as an error rather than corrupting or crashing on the document.
        if !content.is_char_boundary(edit.range.start) || !content.is_char_boundary(edit.range.end) {
            return Err(format!(
                "Fix range {}..{} does not lie on UTF-8 char boundaries",
                edit.range.start, edit.range.end
            ));
        }

        // Skip fixes that overlap with an already-applied fix to prevent
        // offset corruption (e.g., nested link/image constructs in MD039).
        if edit.range.end > min_applied_start {
            continue;
        }

        min_applied_start = edit.range.start;
        kept.push(edit);
    }

    let growth: usize = kept.iter().map(|edit| edit.replacement.len()).sum();
    let mut result = String::with_capacity(content.len() + growth);
    let mut copied = 0;
    for edit in kept.iter().rev() {
        result.push_str(&content[copied..edit.range.start]);
        result.push_str(&edit.replacement);
        copied = edit.range.end;
    }
    result.push_str(&content[copied..]);

    // Ensure line endings are consistent with the original document
    Ok(ensure_consistent_line_endings(content, &result))
}

/// One physical edit ready to apply. Either passes through a single `Fix`'s
/// replacement borrow or holds the concatenation of several same-offset
/// zero-width inserts.
struct ApplicableEdit<'a> {
    range: Range<usize>,
    replacement: Cow<'a, str>,
}
