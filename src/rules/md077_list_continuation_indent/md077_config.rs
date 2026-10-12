use super::ContinuationStyle;

/// Configuration for MD077 (List continuation content indentation)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MD077Config {
    /// How strictly continuation-line indentation is enforced.
    pub(crate) style: ContinuationStyle,
    /// Fixed continuation indent relative to the list marker, e.g. `4` for
    /// MkDocs-style documents. When set, it replaces the content-column-derived
    /// requirement, except that under a flavor requiring strict list indentation
    /// it may only raise the 4-space minimum, never lower it. `None` keeps the
    /// default content-column behavior.
    pub(crate) indent: Option<usize>,
}
