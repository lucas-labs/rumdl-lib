//! MkDocs/Python-Markdown Definition Lists extension support
//!
//! This module provides support for the Python-Markdown Definition Lists extension,
//! which allows creating definition lists with terms and their definitions.
//!
//! ## Syntax
//!
//! ```markdown
//! Term 1
//! :   Definition for term 1
//!
//! Term 2
//! :   Definition for term 2
//!     Continuation of the definition
//!
//! Term with multiple definitions
//! :   First definition
//! :   Second definition
//! ```
//!
//! ## Format Requirements
//!
//! - Term appears on its own line (no leading whitespace required for the term)
//! - Definition starts with `:` followed by whitespace (typically 3 spaces after `:`)
//! - Multiple definitions for a term use separate `:` lines
//! - Continuation lines are indented (typically 4 spaces)
//!
//! ## MkDocs Material Specifics
//!
//! MkDocs Material supports definition lists via the Python-Markdown Definition Lists extension,
//! which is enabled by default in the Material theme.
//!
//! ## References
//!
//! - [Python-Markdown Definition Lists](https://python-markdown.github.io/extensions/definition_lists/)
//! - [MkDocs Material - Lists](https://squidfunk.github.io/mkdocs-material/reference/lists/#using-definition-lists)
