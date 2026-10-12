//! Filtered line iteration for markdown linting
//!
//! This module provides a zero-cost abstraction for iterating over markdown lines
//! while automatically filtering out non-content regions like front matter, code blocks,
//! and HTML blocks. This ensures rules only process actual markdown content.
//!
//! # Architecture
//!
//! The filtered iterator approach centralizes the logic of what content should be
//! processed by rules, eliminating error-prone manual checks in each rule implementation.
//!
//! # Examples
//!
//! ```rust
//! use rumdl_lib::lint_context::LintContext;
//! use rumdl_lib::filtered_lines::FilteredLinesExt;
//!
//! let content = "---\nurl: http://example.com\n---\n\n# Title\n\nContent";
//! let ctx = LintContext::new(content, rumdl_lib::config::MarkdownFlavor::Standard, None);
//!
//! // Simple: get all content lines (skips front matter by default)
//! for line in ctx.content_lines() {
//!     println!("Line {}: {}", line.line_num, line.content);
//! }
//!
//! // Advanced: custom filter configuration
//! for line in ctx.filtered_lines()
//!     .skip_code_blocks()
//!     .skip_front_matter()
//!     .skip_html_blocks() {
//!     println!("Line {}: {}", line.line_num, line.content);
//! }
//! ```

use crate::lint_context::{LineInfo, LintContext};

/// A single line from a filtered iteration, with guaranteed 1-indexed line numbers
#[derive(Debug, Clone)]
pub struct FilteredLine<'a> {
    /// The 1-indexed line number in the original document
    pub(crate) line_num: usize,
    /// The actual line content
    pub(crate) content: &'a str,
}

/// Configuration for filtering lines during iteration
///
/// Use the builder pattern to configure which types of content should be skipped:
///
/// ```rust
/// use rumdl_lib::filtered_lines::LineFilterConfig;
///
/// let config = LineFilterConfig::new()
///     .skip_front_matter()
///     .skip_code_blocks()
///     .skip_html_blocks()
///     .skip_html_comments()
///     .skip_mkdocstrings()
///     .skip_esm_blocks()
///     .skip_quarto_divs();
/// ```
#[derive(Debug, Clone, Default)]
pub struct LineFilterConfig {
    /// Skip lines inside front matter (YAML/TOML/JSON metadata)
    pub(crate) skip_front_matter: bool,
    /// Skip lines inside fenced code blocks
    pub(crate) skip_code_blocks: bool,
    /// Skip lines inside HTML blocks
    pub(crate) skip_html_blocks: bool,
    /// Skip lines inside HTML comments
    pub(crate) skip_html_comments: bool,
    /// Skip lines inside mkdocstrings blocks
    pub(crate) skip_mkdocstrings: bool,
    /// Skip lines inside ESM (ECMAScript Module) blocks
    pub(crate) skip_esm_blocks: bool,
    /// Skip lines inside math blocks ($$ ... $$)
    pub(crate) skip_math_blocks: bool,
    /// Skip lines inside Quarto div blocks (::: ... :::)
    pub(crate) skip_quarto_divs: bool,
    /// Skip lines containing or inside JSX expressions (MDX: {expression})
    pub(crate) skip_jsx_expressions: bool,
    /// Skip lines inside MDX comments ({/* ... */})
    pub(crate) skip_mdx_comments: bool,
    /// Skip lines inside MkDocs admonitions (!!! or ???)
    pub(crate) skip_admonitions: bool,
    /// Skip lines inside MkDocs content tabs (=== "Tab")
    pub(crate) skip_content_tabs: bool,
    /// Skip lines inside HTML blocks with markdown attribute (MkDocs grid cards, etc.)
    pub(crate) skip_mkdocs_html_markdown: bool,
    /// Skip lines inside Obsidian comments (%%...%%)
    pub(crate) skip_obsidian_comments: bool,
    /// Skip lines inside PyMdown Blocks (/// ... ///, MkDocs flavor only)
    pub(crate) skip_pymdown_blocks: bool,
    /// Skip lines that are div markers (::: opening or closing)
    /// Unlike `skip_quarto_divs` which skips ALL content inside divs,
    /// this only skips the marker lines themselves (structural delimiters)
    pub(crate) skip_div_markers: bool,
    /// Skip lines inside JSX component blocks (MDX only, e.g. `<Tabs>...</Tabs>`)
    pub(crate) skip_jsx_blocks: bool,
}

impl LineFilterConfig {
    /// Create a new filter configuration with all filters disabled
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Skip lines that are part of front matter (YAML/TOML/JSON)
    ///
    /// Front matter is metadata at the start of a markdown file and should
    /// not be processed by markdown linting rules.
    #[must_use]
    pub(crate) fn skip_front_matter(mut self) -> Self {
        self.skip_front_matter = true;
        self
    }

    /// Skip lines inside fenced code blocks
    ///
    /// Code blocks contain source code, not markdown, and most rules should
    /// not process them.
    #[must_use]
    pub(crate) fn skip_code_blocks(mut self) -> Self {
        self.skip_code_blocks = true;
        self
    }

    /// Skip lines inside HTML blocks
    ///
    /// HTML blocks contain raw HTML and most markdown rules should not
    /// process them.
    #[must_use]
    pub(crate) fn skip_html_blocks(mut self) -> Self {
        self.skip_html_blocks = true;
        self
    }

    /// Skip lines inside HTML comments
    ///
    /// HTML comments (<!-- ... -->) are metadata and should not be processed
    /// by most markdown linting rules.
    #[must_use]
    pub(crate) fn skip_html_comments(mut self) -> Self {
        self.skip_html_comments = true;
        self
    }

    /// Skip lines inside mkdocstrings blocks
    ///
    /// Mkdocstrings blocks contain auto-generated documentation and most
    /// markdown rules should not process them.
    #[must_use]
    pub(crate) fn skip_mkdocstrings(mut self) -> Self {
        self.skip_mkdocstrings = true;
        self
    }

    /// Skip lines inside ESM (ECMAScript Module) blocks
    ///
    /// ESM blocks contain JavaScript/TypeScript module code and most
    /// markdown rules should not process them.
    #[must_use]
    pub(crate) fn skip_esm_blocks(mut self) -> Self {
        self.skip_esm_blocks = true;
        self
    }

    /// Skip lines inside math blocks ($$ ... $$)
    ///
    /// Math blocks contain LaTeX/mathematical notation and markdown rules
    /// should not process them as regular markdown content.
    #[must_use]
    pub(crate) fn skip_math_blocks(mut self) -> Self {
        self.skip_math_blocks = true;
        self
    }

    /// Skip lines inside Quarto div blocks (::: ... :::)
    ///
    /// Quarto divs are fenced containers for callouts, panels, and other
    /// structured content. Rules may need to skip them for accurate processing.
    #[must_use]
    pub(crate) fn skip_quarto_divs(mut self) -> Self {
        self.skip_quarto_divs = true;
        self
    }

    /// Skip lines containing or inside JSX expressions (MDX: {expression})
    ///
    /// JSX expressions contain JavaScript code and most markdown rules
    /// should not process them as regular markdown content.
    #[must_use]
    pub(crate) fn skip_jsx_expressions(mut self) -> Self {
        self.skip_jsx_expressions = true;
        self
    }

    /// Skip lines inside MDX comments ({/* ... */})
    ///
    /// MDX comments are metadata and should not be processed by most
    /// markdown linting rules.
    #[must_use]
    pub(crate) fn skip_mdx_comments(mut self) -> Self {
        self.skip_mdx_comments = true;
        self
    }

    /// Skip lines inside Obsidian comments (%%...%%)
    ///
    /// Obsidian comments are content hidden from rendering and most
    /// markdown rules should not process them.
    #[must_use]
    pub(crate) fn skip_obsidian_comments(mut self) -> Self {
        self.skip_obsidian_comments = true;
        self
    }

    /// Skip lines inside PyMdown Blocks (/// ... ///)
    ///
    /// PyMdown Blocks are structured content blocks used by the PyMdown Extensions
    /// library for captions, collapsible details, admonitions, and other features.
    /// Rules may need to skip them for accurate processing.
    #[must_use]
    pub(crate) fn skip_pymdown_blocks(mut self) -> Self {
        self.skip_pymdown_blocks = true;
        self
    }

    /// Skip lines inside JSX component blocks (MDX only)
    ///
    /// JSX blocks like `<Tabs>...</Tabs>` contain content that pulldown-cmark
    /// may misparse. Use this to skip entire JSX component regions.
    #[must_use]
    pub(crate) fn skip_jsx_blocks(mut self) -> Self {
        self.skip_jsx_blocks = true;
        self
    }

    /// Check if a line should be filtered out based on this configuration
    fn should_filter(&self, line_info: &LineInfo) -> bool {
        // Kramdown extension blocks are always filtered unconditionally.
        // Their content should never be linted by any rule.
        false
            || (self.skip_front_matter && line_info.in_front_matter)
            || (self.skip_code_blocks && line_info.in_code_block)
            || (self.skip_html_blocks && line_info.in_html_block)
            || (self.skip_html_comments && line_info.in_html_comment)
            || (self.skip_mkdocstrings && line_info.in_mkdocstrings)
            || false
            || (self.skip_math_blocks && line_info.in_math_block)
            || false
            || false
            || false
            || (self.skip_admonitions && line_info.in_admonition)
            || (self.skip_content_tabs && line_info.in_content_tab)
            || (self.skip_mkdocs_html_markdown && line_info.in_mkdocs_html_markdown)
            || false
            || false
            || false
            || (self.skip_div_markers && line_info.is_div_marker)
            || false
    }
}

/// Iterator that yields filtered lines based on configuration
pub struct FilteredLinesIter<'a> {
    ctx: &'a LintContext<'a>,
    config: LineFilterConfig,
    current_index: usize,
}

impl<'a> FilteredLinesIter<'a> {
    /// Create a new filtered lines iterator
    fn new(ctx: &'a LintContext<'a>, config: LineFilterConfig) -> Self {
        Self {
            ctx,
            config,
            current_index: 0,
        }
    }
}

impl<'a> Iterator for FilteredLinesIter<'a> {
    type Item = FilteredLine<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let lines = &self.ctx.lines;
        let raw_lines = self.ctx.raw_lines();

        while self.current_index < lines.len() {
            let idx = self.current_index;
            self.current_index += 1;

            // Check if this line should be filtered
            if self.config.should_filter(&lines[idx]) {
                continue;
            }

            // Get the actual line content from pre-split lines
            let line_content = raw_lines.get(idx).copied().unwrap_or("");

            // Return the filtered line with 1-indexed line number
            return Some(FilteredLine {
                line_num: idx + 1, // Convert 0-indexed to 1-indexed
                content: line_content,
            });
        }

        None
    }
}

/// Extension trait that adds filtered iteration methods to `LintContext`
///
/// This trait provides convenient methods for iterating over lines while
/// automatically filtering out non-content regions.
pub trait FilteredLinesExt {
    /// Start building a filtered lines iterator
    ///
    /// Returns a `LineFilterConfig` builder that can be used to configure
    /// which types of content should be filtered out.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rumdl_lib::lint_context::LintContext;
    /// use rumdl_lib::filtered_lines::FilteredLinesExt;
    ///
    /// let content = "# Title\n\n```rust\ncode\n```\n\nContent";
    /// let ctx = LintContext::new(content, rumdl_lib::config::MarkdownFlavor::Standard, None);
    ///
    /// for line in ctx.filtered_lines().skip_code_blocks() {
    ///     println!("Line {}: {}", line.line_num, line.content);
    /// }
    /// ```
    fn filtered_lines(&self) -> FilteredLinesBuilder<'_>;
}

/// Builder type that allows chaining filter configuration and converting to an iterator
pub struct FilteredLinesBuilder<'a> {
    ctx: &'a LintContext<'a>,
    config: LineFilterConfig,
}

impl<'a> FilteredLinesBuilder<'a> {
    fn new(ctx: &'a LintContext<'a>) -> Self {
        Self {
            ctx,
            config: LineFilterConfig::new(),
        }
    }

    /// Skip lines that are part of front matter (YAML/TOML/JSON)
    #[must_use]
    pub(crate) fn skip_front_matter(mut self) -> Self {
        self.config = self.config.skip_front_matter();
        self
    }

    /// Skip lines inside fenced code blocks
    #[must_use]
    pub(crate) fn skip_code_blocks(mut self) -> Self {
        self.config = self.config.skip_code_blocks();
        self
    }

    /// Skip lines inside HTML blocks
    #[must_use]
    pub(crate) fn skip_html_blocks(mut self) -> Self {
        self.config = self.config.skip_html_blocks();
        self
    }

    /// Skip lines inside HTML comments
    #[must_use]
    pub(crate) fn skip_html_comments(mut self) -> Self {
        self.config = self.config.skip_html_comments();
        self
    }

    /// Skip lines inside mkdocstrings blocks
    #[must_use]
    pub(crate) fn skip_mkdocstrings(mut self) -> Self {
        self.config = self.config.skip_mkdocstrings();
        self
    }

    /// Skip lines inside ESM (ECMAScript Module) blocks
    #[must_use]
    pub(crate) fn skip_esm_blocks(mut self) -> Self {
        self.config = self.config.skip_esm_blocks();
        self
    }

    /// Skip lines inside math blocks ($$ ... $$)
    #[must_use]
    pub(crate) fn skip_math_blocks(mut self) -> Self {
        self.config = self.config.skip_math_blocks();
        self
    }

    /// Skip lines inside Quarto div blocks (::: ... :::)
    #[must_use]
    pub(crate) fn skip_quarto_divs(mut self) -> Self {
        self.config = self.config.skip_quarto_divs();
        self
    }

    /// Skip lines containing or inside JSX expressions (MDX: {expression})
    #[must_use]
    pub(crate) fn skip_jsx_expressions(mut self) -> Self {
        self.config = self.config.skip_jsx_expressions();
        self
    }

    /// Skip lines inside MDX comments ({/* ... */})
    #[must_use]
    pub(crate) fn skip_mdx_comments(mut self) -> Self {
        self.config = self.config.skip_mdx_comments();
        self
    }

    /// Skip lines inside Obsidian comments (%%...%%)
    #[must_use]
    pub(crate) fn skip_obsidian_comments(mut self) -> Self {
        self.config = self.config.skip_obsidian_comments();
        self
    }

    /// Skip lines inside PyMdown Blocks (/// ... ///)
    #[must_use]
    pub(crate) fn skip_pymdown_blocks(mut self) -> Self {
        self.config = self.config.skip_pymdown_blocks();
        self
    }

    /// Skip lines inside JSX component blocks (MDX only)
    #[must_use]
    pub(crate) fn skip_jsx_blocks(mut self) -> Self {
        self.config = self.config.skip_jsx_blocks();
        self
    }
}

impl<'a> IntoIterator for FilteredLinesBuilder<'a> {
    type Item = FilteredLine<'a>;
    type IntoIter = FilteredLinesIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        FilteredLinesIter::new(self.ctx, self.config)
    }
}

impl<'a> FilteredLinesExt for LintContext<'a> {
    fn filtered_lines(&self) -> FilteredLinesBuilder<'_> {
        FilteredLinesBuilder::new(self)
    }
}
