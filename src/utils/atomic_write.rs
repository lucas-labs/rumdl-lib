//! Atomic file writes for the `--fix` / `fmt` path.
//!
//! Overwriting a user's Markdown file in place with `std::fs::write` opens it
//! with `O_TRUNC`: the moment the call starts, the original bytes are gone, and
//! if the process is killed, the disk fills, or the write errors partway
//! through, the file is left truncated with no way to recover the original.
//! For a linter that edits source files this is data loss.
//!
//! `write_atomically` instead writes the new content to a sibling temp file on
//! the same filesystem and renames it over the target. `rename(2)` is atomic, so
//! a concurrent reader (or a crash) sees either the complete old file or the
//! complete new file, never a partial one, and a failed write leaves the
//! original untouched.
