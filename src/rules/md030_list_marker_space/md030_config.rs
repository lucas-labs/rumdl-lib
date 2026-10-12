use crate::types::{OlAlignColumn, PositiveUsize};

/// Configuration for MD030 (Spaces after list markers).
///
/// Public so other rules (notably MD013's reflow) can load it through the shared
/// [`crate::rule_config_serde::load_rule_config`] path and reuse
/// [`MD030Config::expected_spaces`], rather than re-deriving the spacing rules.
#[derive(Debug, Clone, PartialEq)]
pub struct MD030Config {
    /// Spaces for single-line unordered list items (default: 1)
    pub(crate) ul_single: PositiveUsize,

    /// Spaces for multi-line unordered list items (default: 1)
    pub(crate) ul_multi: PositiveUsize,

    /// Spaces for single-line ordered list items (default: 1)
    pub(crate) ol_single: PositiveUsize,

    /// Spaces for multi-line ordered list items (default: 1)
    pub(crate) ol_multi: PositiveUsize,

    /// Align ordered list text to this column, measured from the start of the
    /// marker (default: 0 = off; valid values are 3-6, with 4 the usual choice).
    /// Narrower markers are padded up to the column; markers too wide for it
    /// overflow with one space rather than pushing the rest of the list over.
    /// See docs/md030.md.
    pub(crate) ol_align_column: OlAlignColumn,
}

fn default_spaces() -> PositiveUsize {
    PositiveUsize::from_const(1)
}

impl Default for MD030Config {
    fn default() -> Self {
        Self {
            ul_single: default_spaces(),
            ul_multi: default_spaces(),
            ol_single: default_spaces(),
            ol_multi: default_spaces(),
            ol_align_column: OlAlignColumn::default(),
        }
    }
}

impl MD030Config {
    /// Number of spaces that should follow a list marker, given whether the list
    /// is ordered, whether the item spans multiple lines, and the marker width
    /// (e.g. 1 for `-`, 2 for `1.`).
    ///
    /// The `ol-align-column` override takes precedence for ordered lists: it
    /// aligns text to the target column measured from the marker start, padding a
    /// narrow marker and letting one too wide overflow with a single space, capped
    /// at 4 spaces (5+ would start a CommonMark indented code block). Otherwise the
    /// fixed `ul-single`/`ul-multi`/`ol-single`/`ol-multi` value applies.
    pub(crate) fn expected_spaces(&self, is_ordered: bool, is_multi: bool, marker_len: usize) -> usize {
        if is_ordered && let Some(target_column) = self.ol_align_column.enabled() {
            return target_column.saturating_sub(marker_len).clamp(1, 4);
        }
        match (is_ordered, is_multi) {
            (false, false) => self.ul_single.get(),
            (false, true) => self.ul_multi.get(),
            (true, false) => self.ol_single.get(),
            (true, true) => self.ol_multi.get(),
        }
    }
}
