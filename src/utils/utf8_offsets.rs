//! UTF-8 byte offset to character offset conversion utilities.
//!
//! JavaScript uses UTF-16 code units for string indexing, while Rust uses bytes.
//! This module provides functions to convert between byte offsets and character
//! offsets for proper interoperability with JavaScript/WASM environments.
