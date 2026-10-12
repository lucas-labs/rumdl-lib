//! Recognition of whole-line GitHub Agentic Workflows control directives.
//!
//! This intentionally recognizes only syntax that gh-aw treats as structural.
//! Template expressions such as `${{ github.ref }}` and directive-looking text
//! embedded in prose remain ordinary Markdown.
