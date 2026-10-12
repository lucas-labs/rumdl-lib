//! MkDocs PyMdown extensions support
//!
//! This module provides support for various PyMdown Markdown extensions
//! commonly used with MkDocs Material:
//!
//! - **InlineHilite**: Inline code highlighting `` `#!python code` ``
//! - **Keys**: Keyboard key notation `++ctrl+alt+delete++`
//! - **Caret**: Superscript and insert `^superscript^` and `^^insert^^`
//! - **Tilde**: Subscript and strikethrough `~subscript~` and `~~strike~~`
//! - **Mark**: Highlight text `==highlighted==`
//! - **SmartSymbols**: Auto-replace symbols `(c)` → `©`
//!
//! ## Architecture
//!
//! All markup detection follows a consistent span-based pattern:
//! 1. `find_*_spans(line) -> Vec<(usize, usize)>` - find byte ranges
//! 2. `is_in_*(line, position) -> bool` - check if position is inside markup
//!
//! For double-takes-precedence patterns (caret: ^^/^, tilde: ~~/~):
//! - Double-delimiter spans are found first
//! - Single-delimiter spans exclude positions inside double spans
//!
//! ## References
//!
//! - [PyMdown Extensions](https://facelessuser.github.io/pymdown-extensions/)

// ============================================================================
// Core span utilities
// ============================================================================

// ============================================================================
// InlineHilite: `#!lang code` syntax for inline code with syntax highlighting
// ============================================================================

// ============================================================================
// Keys: ++key++ syntax for keyboard keys
// ============================================================================

// ============================================================================
// Caret: ^superscript^ and ^^insert^^ syntax
// ============================================================================

// ============================================================================
// Tilde: ~subscript~ and ~~strikethrough~~ syntax
// ============================================================================

// ============================================================================
// Mark: ==highlighted== syntax
// ============================================================================

// ============================================================================
// SmartSymbols: (c), (tm), (r), -->, <--, etc.
// ============================================================================

// ============================================================================
// Combined utilities
// ============================================================================
