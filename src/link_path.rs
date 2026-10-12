//! the file path a link destination names.

/// Convert a hex digit character to its numeric value (0-15)
fn hex_digit_to_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// URL-decode a string, handling percent-encoded characters.
///
/// Returns the decoded string, or the original if the decoded bytes are not
/// UTF-8. A `%` not followed by two hex digits is kept as written. Used both to
/// match URL-encoded CJK fragments against raw anchors and to turn a link's
/// path into the file name it spells.
fn url_decode(s: &str) -> String {
    // Fast path: no percent signs means no encoding
    if !s.contains('%') {
        return s.to_string();
    }

    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            // Try to parse the two hex digits following %
            let hex1 = bytes[i + 1];
            let hex2 = bytes[i + 2];
            if let (Some(d1), Some(d2)) = (hex_digit_to_value(hex1), hex_digit_to_value(hex2)) {
                result.push(d1 * 16 + d2);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }

    // Convert to UTF-8, falling back to original if invalid
    String::from_utf8(result).unwrap_or_else(|_| s.to_string())
}

/// Strip query parameters and fragments from a URL path
/// Returns the path portion before `?` or `#`
fn strip_query_and_fragment(url: &str) -> &str {
    let query_pos = url.find('?');
    let fragment_pos = url.find('#');

    match (query_pos, fragment_pos) {
        (Some(q), Some(f)) => &url[..q.min(f)],
        (Some(q), None) => &url[..q],
        (None, Some(f)) => &url[..f],
        (None, None) => url,
    }
}

/// The file path a link destination spells: the destination without its query
/// string or fragment, percent-decoded.
///
/// A destination is a URL, so `guide%20one.md` names the file `guide one.md`,
/// and a query or fragment is never part of a file name. Every question about
/// which file a link names starts from this, so the answer cannot depend on
/// which rule asked.
pub fn link_path_part(url: &str) -> String {
    url_decode(strip_query_and_fragment(url))
}
