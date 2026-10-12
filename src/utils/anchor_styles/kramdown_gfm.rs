//! Kramdown with GFM input anchor generation
//!
//! This module implements the exact anchor generation algorithm used by kramdown
//! when configured with GFM input mode. This is the default configuration used by
//! Jekyll and GitHub Pages.
//!
//! Algorithm verified against official kramdown Ruby gem (2.5.1) with GFM input:
//! 1. Input validation and normalization
//! 2. Remove markdown formatting (emphasis, code, links - keep only link text)
//! 3. Symbol replacements (arrows with specific hyphen counts)
//! 4. Character filtering (ASCII letters/digits, common Unicode letters, spaces, hyphens, underscores)
//! 5. Trim to first letter (but preserve number-only headings)
//! 6. Convert spaces to hyphens, consolidate multiple hyphens
//! 7. Remove leading and trailing hyphens
