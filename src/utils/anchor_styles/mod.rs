//! Anchor generation styles for different Markdown platforms
//!
//! This module provides different anchor generation implementations that match
//! the behavior of various Markdown platforms:
//!
//! - **GitHub**: GitHub.com's official anchor generation algorithm
//! - **KramdownGfm**: Kramdown with GFM input (used by Jekyll/GitHub Pages)
//! - **Kramdown**: Pure kramdown without GFM extensions
//!
//! Each style is implemented in a separate module with comprehensive tests
//! verified against the official tools/platforms.
//!
//! Common utilities are shared via the `common` module to avoid duplication.

pub mod common;
pub mod github;
pub mod kramdown;
pub mod kramdown_gfm; // Renamed from jekyll for clarity
pub mod python_markdown;
