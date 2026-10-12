//! PyMdown Extensions Blocks detection utilities
//!
//! This module provides detection for PyMdown Extensions "Blocks" syntax which uses
//! `///` markers to create structured content blocks.
//!
//! Common patterns:
//! - `/// caption` - Caption block for figures/tables
//! - `/// details | Summary title` - Collapsible content
//! - `/// admonition | Title` - Admonition with custom title
//! - `/// html | div` - HTML wrapper block
//! - `///` - Closing marker
//!
//! Blocks can have YAML options indented 4 spaces after the header line:
//! ```text
//! /// caption
//!     attrs: {id: my-id}
//! Caption text
//! ///
//! ```
//!
//! Supported block types: caption, figure-caption, details, admonition, html, definition, tab
