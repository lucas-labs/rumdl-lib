//! Python-Markdown anchor generation (used by MkDocs)
//!
//! This module implements the exact slugify algorithm from Python-Markdown's
//! `toc` extension, which is the default used by MkDocs.
//!
//! Algorithm (from markdown/extensions/toc.py):
//! 1. NFKD Unicode normalization, then encode to ASCII (ignore errors)
//! 2. Remove all characters except `\w`, `\s`, and `-`
//! 3. Lowercase and strip leading/trailing whitespace
//! 4. Collapse consecutive hyphens and whitespace into a single separator
//!
//! Verified against Python-Markdown 3.x via:
//! ```bash
//! uv run --with markdown python3 -c "
//! from markdown.extensions.toc import slugify
//! print(slugify('Your Heading', '-'))
//! "
//! ```
