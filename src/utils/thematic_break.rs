/// Check if a line is a thematic break (horizontal rule).
///
/// Matches two forms:
/// 1. Compact: 3+ of the same marker (-, *, _) with optional trailing whitespace
///    Examples: `---`, `****`, `_____`
/// 2. Spaced: 3+ single markers separated by whitespace
///    Examples: `- - -`, `* * *`, `_  _  _`
///
/// Per CommonMark, up to 3 spaces of leading indentation are allowed; 4 or
/// more spaces mark an indented code block, so such lines are not thematic
/// breaks.
///
/// Does NOT match multi-char groups with spaces (e.g., `---- ------`)
/// even though CommonMark treats them as valid thematic breaks. This avoids
/// false positives on table separators in unformatted output.
pub(crate) fn is_thematic_break(line: &str) -> bool {
    // Strip CRLF but preserve leading indent so the indent check is meaningful.
    let line = line.strip_suffix('\r').unwrap_or(line);
    let leading_spaces = line.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces >= 4 {
        return false;
    }
    let line = line[leading_spaces..].trim_end();
    if line.len() < 3 {
        return false;
    }

    let first = line.as_bytes()[0];
    if first != b'-' && first != b'*' && first != b'_' {
        return false;
    }

    // Check if all non-whitespace chars are the same marker
    let non_ws_count = line.bytes().filter(|&b| !b.is_ascii_whitespace()).count();
    let marker_only = line.bytes().all(|b| b == first || b.is_ascii_whitespace());

    if !marker_only || non_ws_count < 3 {
        return false;
    }

    // Check for spaces between markers (not just trailing)
    let has_internal_space = line.trim_end().as_bytes().iter().any(|&b| b.is_ascii_whitespace());

    if !has_internal_space {
        // Compact form: markers only (with optional trailing whitespace)
        true
    } else {
        // Spaced form: single markers separated by whitespace
        // Each non-whitespace char must be the marker, and markers must not be adjacent
        let mut marker_count = 0;
        let mut prev_was_marker = false;

        for c in line.chars() {
            if c as u32 == first as u32 {
                if prev_was_marker {
                    return false; // Adjacent markers in spaced form
                }
                marker_count += 1;
                prev_was_marker = true;
            } else if c.is_whitespace() {
                prev_was_marker = false;
            } else {
                return false; // Wrong character
            }
        }

        marker_count >= 3
    }
}
