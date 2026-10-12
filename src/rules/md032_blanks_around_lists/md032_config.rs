/// Configuration for MD032 (Lists should be surrounded by blank lines)
#[derive(Debug, Clone, PartialEq)]
pub struct MD032Config {
    /// Allow lazy continuation of list items (default: true)
    ///
    /// When true (default), text following a list item without indentation is treated
    /// as lazy continuation per CommonMark spec and no warning is generated.
    ///
    /// When false, warns when unindented text follows a list item without a blank line.
    /// This helps catch cases where text was intended to be a separate paragraph.
    ///
    /// Example with `allow_lazy_continuation = false`:
    /// ```markdown
    /// 1. List item
    /// Some text.    <- Warning: should have blank line or indentation
    /// ```
    pub(crate) allow_lazy_continuation: bool,
}

fn default_allow_lazy_continuation() -> bool {
    true
}

impl Default for MD032Config {
    fn default() -> Self {
        Self {
            allow_lazy_continuation: default_allow_lazy_continuation(),
        }
    }
}
