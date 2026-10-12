//! The Obsidian tag token (`#tag`).
//!
//! Rules that meet a `#` have to tell a tag from an ATX heading marker or an
//! issue reference, and they have to agree on the answer, so the definition
//! lives here once.

use regex::Regex;
use std::sync::LazyLock;

/// A tag must contain at least one non-numerical character, wherever it sits:
/// `#1984` is not a tag, `#y1984` and `#3d_printing` are. A leading run of
/// digits is therefore allowed as long as a non-numerical tag character follows
/// it. That character may be a letter, a combining mark, an emoji or one of the
/// three punctuation characters Obsidian lists (`_`, `-`, `/`), but not
/// punctuation in general: `#37.` and `#42,` are issue references, not tags.
/// The token runs to the next whitespace.
pub const TAG_PATTERN_STR: &str = r"^#(?:[^\d\s#]|\d+[\p{L}\p{M}\p{So}_/-])[^\s#]*(?:\s|$)";

/// [`TAG_PATTERN_STR`] compiled. The pattern is anchored, so the text handed to
/// it starts at the `#`.
pub static TAG_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(TAG_PATTERN_STR).expect("tag pattern is a valid regex"));
