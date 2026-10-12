//! Project root discovery for resolving project-relative paths.
//!
//! Walks up the directory tree from a starting point looking for a project
//! marker (`.git`, `.rumdl.toml`, `pyproject.toml`, or `.markdownlint.json`).
//! When a marker is found, its containing directory is returned as the project
//! root. When no marker is found, the start directory is returned as a
//! sensible fallback. The result is canonicalized when possible so callers
//! get a stable, symlink-resolved path.
