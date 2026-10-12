/// Configuration for MD024 (Multiple headings with the same content)
#[derive(Debug, Clone, PartialEq)]
pub struct MD024Config {
    /// Allow duplicate headings if they're nested at different levels (default: false)
    pub(crate) allow_different_nesting: bool,

    /// Only check siblings (same parent) for duplicates (default: true)
    ///
    /// Unlike markdownlint, rumdl defaults this to true to reduce false positives
    /// in common documentation patterns like CHANGELOGs.
    ///
    /// Note: This may cause duplicate anchor IDs when linking. Most renderers
    /// (GitHub, GitLab, etc.) handle this by adding numeric suffixes.
    pub(crate) siblings_only: bool,

    /// Treat headings with different custom link anchors (e.g. `{#custom-id}`) as distinct (default: true)
    ///
    /// When true, headings that share the same visible text but carry different `{#id}` suffixes
    /// produce distinct deduplication keys and are not flagged as duplicates. This matches the
    /// effective behavior of markdownlint, which compares raw heading text (retaining the suffix).
    ///
    /// Set to false to restore the previous behavior where `{#id}` suffixes are ignored during
    /// deduplication.
    pub(crate) allow_different_link_anchors: bool,
}

impl Default for MD024Config {
    fn default() -> Self {
        Self {
            allow_different_nesting: false,
            siblings_only: true,
            allow_different_link_anchors: true,
        }
    }
}
