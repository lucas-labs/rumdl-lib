//! Pandoc Markdown syntax detection.
//!
//! This module provides detection for Pandoc Markdown constructs that affect
//! rumdl rule output: fenced divs (`:::`), attribute lists (`{#id .class}`),
//! citations (`[@key]`), bracketed spans (`[text]{.class}`), and other
//! Pandoc-specific syntax.
//!
//! Pandoc is the foundation; the Quarto flavor extends it with Quarto-only
//! syntax (executable code blocks, shortcodes, cell options) elsewhere in
//! the codebase. Anything that's pure Pandoc lives here.
//!
//! Common patterns this module handles:
//! - `::: {.callout-note}` — fenced div with class
//! - `::: {#myid .class}` — generic div with id and class
//! - `:::` — closing marker
//! - `{#id .class key="value"}` — Pandoc attribute lists
//! - `@key`, `[@key]`, `[-@key]`, `[@a; @b]` — citations

use regex::Regex;
use std::sync::LazyLock;

/// Pattern to match div opening markers
/// Matches: ::: {.class}, ::: {#id .class}, ::: classname, etc.
/// Does NOT match a closing ::: on its own
static DIV_OPEN_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\s*):::\s*(?:\{[^}]+\}|\S+)").unwrap());

/// Pattern to match div closing markers
/// Matches: ::: (with optional whitespace before and after)
static DIV_CLOSE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\s*):::\s*$").unwrap());

/// Check if a line is a div opening marker
pub(crate) fn is_div_open(line: &str) -> bool {
    DIV_OPEN_PATTERN.is_match(line)
}

/// Check if a line is a div closing marker (just `:::`)
pub(crate) fn is_div_close(line: &str) -> bool {
    DIV_CLOSE_PATTERN.is_match(line)
}

// ============================================================================
// Citation Support
// ============================================================================
//
// Pandoc citation syntax:
// - Inline citation: @smith2020
// - Parenthetical citation: [@smith2020]
// - Suppress author: [-@smith2020]
// - With locator: [@smith2020, p. 10]
// - Multiple citations: [@smith2020; @jones2021]
// - With prefix: [see @smith2020]
//
// Citation keys must start with a letter, digit, or underscore, and may contain
// alphanumerics, underscores, hyphens, periods, and colons.

// ============================================================================
// Inline Footnote Support
// ============================================================================
//
// Pandoc inline footnote syntax: ^[footnote text]
//
// The `^` must not be preceded by `!` (image) or by a word character
// (superscript syntax: `2^10^`). The footnote body extends to the first
// unescaped `]`; nested brackets are not supported in this detector.

// ============================================================================
// Subscript and Superscript Support
// ============================================================================
//
// Pandoc `subscript` extension: `~x~` where x contains no whitespace or `~`.
// Pandoc `superscript` extension: `^x^` where x contains no whitespace or `^`.
//
// These are distinct from GFM strikethrough (`~~text~~`) and Pandoc inline
// footnotes (`^[...]`). The disambiguation rule for subscript is: reject any
// match where the opening or closing `~` is immediately adjacent to another `~`
// (which would make it GFM strikethrough). For superscript, reject matches
// where a `^` neighbour would form `^^`.

// ============================================================================
// Inline Code Attribute Support
// ============================================================================
//
// Pandoc `inline_code_attributes` extension: `` `code`{.lang} ``
//
// The attribute block must immediately follow the closing backtick of the
// inline code span. Only the `{...}` part is captured; the backtick span
// itself is already handled by the standard code-span detector.

// ============================================================================
// Example List Support
// ============================================================================
//
// Pandoc `example_lists` extension:
// - Line-start marker: `(@)` or `(@label)` followed by whitespace
// - Inline reference: `(@label)` appearing mid-paragraph (not at line start)
//
// Example keys contain letters, digits, underscores, and hyphens.
// The anonymous form `(@)` is valid as a marker but cannot appear as a reference
// (references require a label to be named).

// ============================================================================
// Bracketed Span Support
// ============================================================================
//
// Pandoc `bracketed_spans` extension: `[text]{attrs}` where attrs is a
// non-empty Pandoc attribute block.
//
// Distinguished from `[text](url)` (link) and `[text][ref]` (reference link)
// by requiring `]{` immediately adjacent — the `{` must directly follow `]`
// with no intervening characters.

// ============================================================================
// Line Block Support
// ============================================================================
//
// Pandoc `line_blocks` extension: a contiguous run of lines starting with `| `
// (pipe space). Each line in a line block is rendered as a separate line of
// verse or address. Continuation lines — indented, non-empty, not starting
// with `|` — extend the immediately preceding block line.
//
// Distinguished from pipe tables: a line whose trimmed form ends with `|`
// (i.e. `| col1 | col2 |`) is a table row, not a line block entry.

// ============================================================================
// Pipe-Table Caption Support
// ============================================================================
//
// Pandoc `table_captions` extension: a `: caption text` line that appears
// adjacent to a pipe table, separated by exactly one blank line (either
// above or below). Without the blank-line adjacency to a pipe table, a
// `: text` line is a definition-list value and must NOT be matched here.
//
// Matching rule:
//   caption_below: caption at line i, blank at i+1, pipe-table row at i+2
//   caption_above: pipe-table row at i-2, blank at i-1, caption at i

// ============================================================================
// YAML Metadata Block Support
// ============================================================================
//
// Pandoc `yaml_metadata_block` extension: one or more `---`-delimited YAML
// blocks anywhere in the document. Unlike standard frontmatter (single block
// at file start), Pandoc allows:
//   - Multiple blocks per document
//   - `---` opener
//   - Either `---` or `...` as the closer
//   - Opener must be at start-of-file OR immediately after a blank line
//   - Unterminated openers are skipped

// ============================================================================
// Grid Table Support
// ============================================================================
//
// Pandoc `grid_tables` extension: a contiguous block of lines where the
// first line is a `+---+---+` border row (`+` corners, `-` or `=` between),
// followed by alternating content rows (`| ... | ... |`) and border rows
// (`+---+---+` or `+===+===+`), ending with a closing border row.
// At least one content row is required for a valid grid table.

// ============================================================================
// Multi-line Table Support
// ============================================================================
//
// Pandoc `multiline_tables` extension: a block whose column widths are declared
// by an underline row of dashes-separated-by-spaces (MULTI_LINE_UNDERLINE), with
// an optional top-border and a mandatory closing solid-dash row (MULTI_LINE_BORDER).
// The header line immediately precedes the underline row.
