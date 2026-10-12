/// Configuration for MD035 (Horizontal rule style)
#[derive(Debug, Clone, PartialEq)]
pub struct MD035Config {
    /// The style for horizontal rules (default: "consistent")
    /// Can be "---", "***", "___", "- - -", "* * *", "_ _ _", or "consistent"
    pub(crate) style: String,
}

/// "consistent" adopts whichever style the document already uses most, which is what
/// an unconfigured MD035 has always enforced. A concrete style here would silently
/// turn the rule into "every document must use this marker".
fn default_style() -> String {
    "consistent".to_string()
}

impl Default for MD035Config {
    fn default() -> Self {
        Self { style: default_style() }
    }
}
