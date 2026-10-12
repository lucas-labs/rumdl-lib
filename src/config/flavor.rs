/// The markdown flavor of a document. rf reads every document as standard markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkdownFlavor {
    /// Standard Markdown without flavor-specific adjustments
    #[default]
    Standard,
}
