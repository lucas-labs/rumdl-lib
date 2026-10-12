use regex::Regex;
use std::sync::LazyLock;

// Better detection of inline code with support for multiple backticks
static INLINE_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(`+)([^`]|[^`].*?[^`])(`+)").unwrap());

/// Represents an emphasis marker found in text
#[derive(Debug, Clone, PartialEq)]
pub struct EmphasisMarker {
    pub(crate) marker_type: u8,  // b'*' or b'_' for faster comparison
    pub(crate) count: u8,        // 1 for single, 2 for double
    pub(crate) start_pos: usize, // Position in the line
}

impl EmphasisMarker {
    #[inline]
    pub(crate) fn end_pos(&self) -> usize {
        self.start_pos + self.count as usize
    }

    #[inline]
    pub(crate) fn as_char(&self) -> char {
        self.marker_type as char
    }
}

/// Represents a complete emphasis span
#[derive(Debug, Clone)]
pub struct EmphasisSpan {
    pub(crate) opening: EmphasisMarker,
    pub(crate) closing: EmphasisMarker,
}

/// Enhanced inline code replacement with optimized performance
/// Replaces inline code with 'X' characters to prevent false positives in emphasis detection
#[inline]
pub(crate) fn replace_inline_code(line: &str) -> String {
    // Quick check: if no backticks, return original
    if !line.contains('`') {
        return line.to_string();
    }

    let mut result = line.to_string();
    let mut offset = 0;

    for cap in INLINE_CODE.captures_iter(line) {
        if let (Some(full_match), Some(_opening), Some(_content), Some(_closing)) =
            (cap.get(0), cap.get(1), cap.get(2), cap.get(3))
        {
            let match_start = full_match.start();
            let match_end = full_match.end();
            // Use 'X' instead of spaces to avoid false positives for "spaces in emphasis"
            let placeholder = "X".repeat(match_end - match_start);

            result.replace_range(match_start + offset..match_end + offset, &placeholder);
            offset += placeholder.len() - (match_end - match_start);
        }
    }

    result
}

/// Optimized emphasis marker parsing using byte iteration
#[inline]
pub(crate) fn find_emphasis_markers(line: &str) -> Vec<EmphasisMarker> {
    // Early return for lines without emphasis markers
    if !line.contains('*') && !line.contains('_') {
        return Vec::new();
    }

    let mut markers = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'*' || byte == b'_' {
            let start_pos = i;
            let mut count = 1u8;

            // Count consecutive markers (limit to avoid overflow)
            while i + (count as usize) < bytes.len() && bytes[i + (count as usize)] == byte && count < 3 {
                count += 1;
            }

            // Only consider single (*) and double (**) markers
            if count == 1 || count == 2 {
                markers.push(EmphasisMarker {
                    marker_type: byte,
                    count,
                    start_pos,
                });
            }

            i += count as usize;
        } else {
            i += 1;
        }
    }

    markers
}

/// Find all emphasis spans in a line, excluding only single emphasis (not strong)
pub(crate) fn find_single_emphasis_spans(line: &str, markers: &[EmphasisMarker]) -> Vec<EmphasisSpan> {
    // Early return for insufficient markers
    if markers.len() < 2 {
        return Vec::new();
    }

    // CommonMark left/right-flanking (whitespace clause): an emphasis opener
    // must not be immediately followed by whitespace, and a closer must not be
    // immediately preceded by it. A marker that can do neither is a literal
    // `*`/`_` (e.g. a list-marker `*`, or a `*` flanked by spaces) and is
    // transparent to delimiter matching. `find_emphasis_spans` (MD037)
    // deliberately keeps such runs so MD037 can flag the spaces inside them;
    // this single-emphasis finder, used only by MD049, must not.
    let bytes = line.as_bytes();
    let is_ws = |b: u8| b == b' ' || b == b'\t';
    let can_open = |m: &EmphasisMarker| {
        let after = m.end_pos();
        after < bytes.len() && !is_ws(bytes[after])
    };
    let can_close = |m: &EmphasisMarker| m.start_pos > 0 && !is_ws(bytes[m.start_pos - 1]);

    let mut spans = Vec::new();
    let mut used_markers = vec![false; markers.len()];

    // Process markers in pairs more efficiently
    for i in 0..markers.len() {
        if used_markers[i] || markers[i].count != 1 || !can_open(&markers[i]) {
            continue;
        }

        let opening = &markers[i];

        // Look for the nearest matching closing marker using optimized search
        for j in (i + 1)..markers.len() {
            if used_markers[j] {
                continue;
            }

            let closing = &markers[j];

            // Quick type and count check - only single emphasis that can close
            if closing.marker_type == opening.marker_type && closing.count == 1 && can_close(closing) {
                let content_start = opening.end_pos();
                let content_end = closing.start_pos;

                if content_end > content_start {
                    let content = &line[content_start..content_end];

                    // Optimized validation checks
                    if is_valid_emphasis_content_fast(content) && is_valid_emphasis_span_fast(line, opening, closing) {
                        // A pairing is only blocked by an intervening *viable*
                        // delimiter (one that can itself open or close); a
                        // transparent literal marker does not interfere, which
                        // lets `*foo * bar*` pair its outer asterisks the way
                        // CommonMark does.
                        let crosses_markers = markers[i + 1..j].iter().any(|marker| {
                            marker.marker_type == opening.marker_type
                                && marker.count == 1
                                && (can_open(marker) || can_close(marker))
                        });

                        if !crosses_markers {
                            spans.push(EmphasisSpan {
                                opening: opening.clone(),
                                closing: closing.clone(),
                            });

                            // Mark both markers as used
                            used_markers[i] = true;
                            used_markers[j] = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    spans
}

/// Fast validation of emphasis span context
#[inline]
fn is_valid_emphasis_span_fast(line: &str, opening: &EmphasisMarker, closing: &EmphasisMarker) -> bool {
    let content_start = opening.end_pos();
    let content_end = closing.start_pos;

    // Content must exist and not be just whitespace
    if content_end <= content_start {
        return false;
    }

    let content = &line[content_start..content_end];
    if content.trim().is_empty() {
        return false;
    }

    // Quick boundary checks using byte indexing
    let bytes = line.as_bytes();

    // Opening should be at start or after valid character
    let valid_opening = opening.start_pos == 0
        || matches!(
            bytes.get(opening.start_pos.saturating_sub(1)),
            Some(&b' ')
                | Some(&b'\t')
                | Some(&b'(')
                | Some(&b'[')
                | Some(&b'{')
                | Some(&b'"')
                | Some(&b'\'')
                | Some(&b'>')
        );

    // Closing should be at end or before valid character
    let valid_closing = closing.end_pos() >= bytes.len()
        || matches!(
            bytes.get(closing.end_pos()),
            Some(&b' ')
                | Some(&b'\t')
                | Some(&b')')
                | Some(&b']')
                | Some(&b'}')
                | Some(&b'"')
                | Some(&b'\'')
                | Some(&b'.')
                | Some(&b',')
                | Some(&b'!')
                | Some(&b'?')
                | Some(&b';')
                | Some(&b':')
                | Some(&b'<')
        );

    valid_opening && valid_closing && !content.contains('\n')
}

/// Fast validation of emphasis content
#[inline]
fn is_valid_emphasis_content_fast(content: &str) -> bool {
    !content.trim().is_empty()
}
