//! Upward directory traversal with shared stop semantics.
//!
//! Config and project-root discovery all walk from a starting directory toward
//! the filesystem root, probing each directory on the way. The stop conditions
//! are the subtle part and live here, in one place:
//!
//! - **Home boundary (exclusive):** the walk ends *before* yielding the home
//!   directory. A config in `$HOME` is user-level, not a project config, and
//!   must reach the loader only through the user-config fallback.
//! - **Git root (inclusive):** a directory containing `.git` is yielded and
//!   then the walk ends, so a config in the repository root is still found.
//! - **Stop root (inclusive):** an explicit directory (e.g. a project root) is
//!   yielded and then the walk ends.
//! - **Depth cap:** a guard against runaway traversal.
//!
//! Boundary comparisons canonicalize both sides: on Windows the walked path
//! and the boundary can be different representations of the same directory
//! (8.3 short names vs `\\?\` long names), and on Unix symlinks differ.
//! Comparing raw forms would never match and the walk would overshoot. When
//! canonicalization fails (path no longer exists), the raw forms are compared
//! as a fallback. Yielded paths keep their original representation; only the
//! stop checks canonicalize.
