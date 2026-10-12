//! Sentence detection utilities
//!
//! This module provides shared functionality for detecting sentence boundaries
//! in markdown text. Used by both text reflow (MD013) and the multiple spaces
//! rule (MD064).
//!
//! Features:
//! - Common abbreviation detection (Mr., Dr., Prof., etc.)
//! - CJK punctuation support (。, ！, ？)
//! - Closing quote detection (straight and curly)
//! - Both forward-looking (reflow) and backward-looking (MD064) sentence detection

use std::collections::HashSet;

/// Default abbreviations that should NOT be treated as sentence endings.
///
/// Only includes abbreviations that:
/// 1. Conventionally ALWAYS have a period in standard writing
/// 2. Are almost always followed by something, not sentence-final
///
/// Does NOT include:
/// - Abbreviations that commonly end sentences (etc., Inc., Ph.D., U.S.)
const DEFAULT_ABBREVIATIONS: &[&str] = &[
    // Titles - always have period, always followed by a name
    "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st",
    // Latin - always written with periods, introduce examples/references
    "i.e", "e.g", // Reference abbreviations - followed by what they refer to
    "vs", "fig", "no", "vol", "ch", "sec", "al",
];

/// Get the effective abbreviations set based on custom additions
/// All abbreviations are normalized to lowercase for case-insensitive matching
/// Custom abbreviations are always merged with built-in defaults
pub(crate) fn get_abbreviations(custom: &Option<Vec<String>>) -> HashSet<String> {
    let mut abbreviations: HashSet<String> = DEFAULT_ABBREVIATIONS.iter().map(|s| s.to_lowercase()).collect();

    // Always extend defaults with custom abbreviations
    // Strip any trailing periods and normalize to lowercase for consistent matching
    if let Some(custom_list) = custom {
        for abbr in custom_list {
            let normalized = abbr.trim_end_matches('.').to_lowercase();
            if !normalized.is_empty() {
                abbreviations.insert(normalized);
            }
        }
    }

    abbreviations
}

/// Check if text ends with a common abbreviation followed by a period
///
/// Abbreviations only count when followed by a period, not ! or ?.
/// This prevents false positives where words ending in abbreviation-like
/// letter sequences (e.g., "paradigms" ending in "ms") are incorrectly
/// detected as abbreviations.
///
/// Examples:
///   - "Dr." -> true (abbreviation)
///   - "Dr?" -> false (question, not abbreviation)
///   - "paradigms." -> false (not in abbreviation list)
///   - "paradigms?" -> false (question mark, not abbreviation)
pub(crate) fn text_ends_with_abbreviation(text: &str, abbreviations: &HashSet<String>) -> bool {
    // Only check if text ends with a period (abbreviations require periods)
    if !text.ends_with('.') {
        return false;
    }

    // Remove the trailing period
    let without_period = text.trim_end_matches('.');

    // Get the last word by splitting on whitespace
    let last_word = without_period.split_whitespace().last().unwrap_or("");

    if last_word.is_empty() {
        return false;
    }

    // Strip leading punctuation (parentheses, brackets, quotes, emphasis markers)
    // that may precede the abbreviation, e.g. "(e.g." or "[i.e."
    let stripped = last_word.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '.');

    // Check the full stripped word first (covers simple cases like "Dr.", "Prof.")
    if abbreviations.contains(&stripped.to_lowercase()) {
        return true;
    }

    // Also check the last hyphen-separated component so that hyphenated place names
    // like "Wrangell-St." are recognized via the "st" abbreviation entry.
    if let Some(after_hyphen) = stripped.rsplit('-').next()
        && !after_hyphen.is_empty()
        && after_hyphen != stripped
    {
        return abbreviations.contains(&after_hyphen.to_lowercase());
    }

    false
}

/// Check if a character is CJK sentence-ending punctuation
/// These include: 。(ideographic full stop), ！(fullwidth exclamation), ？(fullwidth question)
pub(crate) fn is_cjk_sentence_ending(c: char) -> bool {
    matches!(c, '。' | '！' | '？')
}

/// Check if a character is a closing quote mark
/// Includes straight quotes and curly/smart quotes
pub(crate) fn is_closing_quote(c: char) -> bool {
    // " (straight double), ' (straight single), " (U+201D right double), ' (U+2019 right single)
    // » (right guillemet), › (single right guillemet)
    matches!(c, '"' | '\'' | '\u{201D}' | '\u{2019}' | '»' | '›')
}

/// Check if a character is an ASCII closing bracket
pub(crate) fn is_ascii_closing_bracket(c: char) -> bool {
    matches!(c, ')' | ']' | '}')
}

/// Check if a character is a fullwidth or CJK closing bracket
///
/// Covers the fullwidth forms of the ASCII brackets and the corner, lenticular,
/// tortoise-shell and angle brackets CJK text encloses an aside in, together
/// with their halfwidth and vertical presentation forms.
pub(crate) fn is_cjk_closing_bracket(c: char) -> bool {
    matches!(
        c,
        '）' | '］'
            | '｝'
            | '」'
            | '』'
            | '】'
            | '〕'
            | '》'
            | '〉'
            | '〙'
            | '〛'
            | '\u{FF63}'
            | '\u{FE42}'
            | '\u{FE44}'
    )
}

/// Check if a character closes a bracketed aside
///
/// The two sets stay separate above so a caller can stay with ASCII brackets
/// alone where widening the set would change English prose.
pub(crate) fn is_closing_bracket(c: char) -> bool {
    is_ascii_closing_bracket(c) || is_cjk_closing_bracket(c)
}

/// Check if a character is an opening quote mark
/// Includes straight quotes and curly/smart quotes
pub(crate) fn is_opening_quote(c: char) -> bool {
    // " (straight double), ' (straight single), " (U+201C left double), ' (U+2018 left single)
    // « (left guillemet), ‹ (single left guillemet)
    matches!(c, '"' | '\'' | '\u{201C}' | '\u{2018}' | '«' | '‹')
}

/// Check if a character is a CJK character (Chinese, Japanese, Korean)
pub(crate) fn is_cjk_char(c: char) -> bool {
    // CJK Unified Ideographs and common extensions
    matches!(c,
        '\u{4E00}'..='\u{9FFF}' |   // CJK Unified Ideographs
        '\u{3400}'..='\u{4DBF}' |   // CJK Unified Ideographs Extension A
        '\u{3040}'..='\u{309F}' |   // Hiragana
        '\u{30A0}'..='\u{30FF}' |   // Katakana
        '\u{AC00}'..='\u{D7AF}'     // Hangul Syllables
    )
}

/// Check if a character is closing punctuation that can follow sentence-ending punctuation
/// This includes closing quotes, parentheses, and brackets
fn is_trailing_close_punctuation(c: char) -> bool {
    is_closing_quote(c) || is_ascii_closing_bracket(c)
}

/// Check if multiple spaces occur immediately after sentence-ending punctuation.
/// This is a backward-looking check used by MD064.
///
/// Returns true if the character(s) immediately before `match_start` constitute
/// a sentence ending, supporting the traditional two-space-after-sentence convention.
///
/// Recognized sentence-ending patterns:
/// - Direct punctuation: `.`, `!`, `?`, `。`, `！`, `？`
/// - With closing quotes: `."`, `!"`, `?"`, `.'`, `!'`, `?'`, `."`, `?"`, `!"`
/// - With closing parenthesis: `.)`, `!)`, `?)`
/// - With closing bracket: `.]`, `!]`, `?]`
/// - Ellipsis: `...`
/// - Combinations: `.")`  (quote then paren), `?')`
///
/// Does NOT treat as sentence ending:
/// - Abbreviations: `Dr.`, `Mr.`, `Prof.`, etc. (when detectable)
/// - Single letters followed by period: `A.` (likely initials or list markers)
pub(crate) fn is_after_sentence_ending(text: &str, match_start: usize) -> bool {
    is_after_sentence_ending_with_abbreviations(text, match_start, &get_abbreviations(&None))
}

/// Check if multiple spaces occur immediately after sentence-ending punctuation,
/// with a custom abbreviations set.
///
/// Note: `match_start` is a byte position (from regex). This function handles
/// multi-byte UTF-8 characters correctly by working with character iterators.
fn is_after_sentence_ending_with_abbreviations(
    text: &str,
    match_start: usize,
    abbreviations: &HashSet<String>,
) -> bool {
    if match_start == 0 || match_start > text.len() {
        return false;
    }

    // Safely get the portion of the text before the spaces
    // match_start is a byte position, so we need to ensure it's a valid char boundary
    let Some(before) = text.get(..match_start) else {
        return false; // Invalid byte position
    };

    // Collect chars for iteration (we need random access for some checks)
    let chars: Vec<char> = before.chars().collect();
    if chars.is_empty() {
        return false;
    }

    let mut idx = chars.len() - 1;

    // Skip through any trailing closing punctuation (quotes, parens, brackets)
    // These can appear after the sentence-ending punctuation
    // e.g., `sentence."  Next` or `sentence.)  Next` or `sentence.")`
    while idx > 0 && is_trailing_close_punctuation(chars[idx]) {
        idx -= 1;
    }

    // Now check if we're at sentence-ending punctuation
    let current = chars[idx];

    // Check for CJK sentence-ending punctuation
    if is_cjk_sentence_ending(current) {
        return true;
    }

    // Direct sentence-ending punctuation (! and ?)
    if current == '!' || current == '?' {
        return true;
    }

    // Period - need more careful handling
    if current == '.' {
        // Check for ellipsis (...) - always a valid sentence ending
        if idx >= 2 && chars[idx - 1] == '.' && chars[idx - 2] == '.' {
            return true;
        }

        // Build the text before the period by collecting chars up to idx
        // (not including the period itself)
        let text_before_period: String = chars[..idx].iter().collect();

        // Check if this is an abbreviation
        if text_ends_with_abbreviation(&format!("{text_before_period}."), abbreviations) {
            return false;
        }

        // Check what comes before the period
        if idx > 0 {
            let prev = chars[idx - 1];

            // Single letter before period - likely initial or list marker, not sentence
            // e.g., "A." "B." but allow "a." at end of sentence
            if prev.is_ascii_uppercase() {
                // Check if it's preceded by whitespace or start of text (isolated initial)
                if idx >= 2 {
                    if chars[idx - 2].is_whitespace() {
                        // "word A." - isolated initial, not sentence ending
                        return false;
                    }
                } else {
                    // "A." at start - not a sentence ending
                    return false;
                }
            }

            // If previous char is alphanumeric, closing quote/paren, or markdown inline delimiters, treat as sentence end
            // Markdown inline elements that can end before punctuation:
            // - `)` `]` - links, images, footnote refs
            // - `` ` `` - inline code
            // - `*` `_` - emphasis/bold
            // - `~` - strikethrough
            // - `=` - highlight (extended markdown)
            // - `^` - superscript (extended markdown)
            if prev.is_alphanumeric()
                || is_closing_quote(prev)
                || matches!(prev, ')' | ']' | '`' | '*' | '_' | '~' | '=' | '^')
                || is_cjk_char(prev)
            {
                return true;
            }
        }

        // Period at start or after non-word char - not a sentence ending
        return false;
    }

    false
}
