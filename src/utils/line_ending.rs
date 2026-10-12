use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
    Mixed,
}

pub(crate) fn detect_line_ending_enum(content: &str) -> LineEnding {
    let bytes = content.as_bytes();
    let mut has_crlf = false;
    let mut has_standalone_lf = false;
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            has_crlf = true;
            i += 2;
        } else if bytes[i] == b'\n' {
            has_standalone_lf = true;
            i += 1;
        } else {
            i += 1;
        }
        // Early exit once both types are found
        if has_crlf && has_standalone_lf {
            return LineEnding::Mixed;
        }
    }

    match (has_crlf, has_standalone_lf) {
        (true, true) => LineEnding::Mixed,
        (true, false) => LineEnding::Crlf,
        (false, _) => LineEnding::Lf,
    }
}

pub(crate) fn detect_line_ending(content: &str) -> &'static str {
    // Compatibility function matching the old signature
    let crlf_count = content.matches("\r\n").count();
    let lf_count = content.matches('\n').count() - crlf_count;

    if crlf_count > lf_count { "\r\n" } else { "\n" }
}

pub(crate) fn normalize_line_ending<'a>(content: &'a str, target: LineEnding) -> Cow<'a, str> {
    match target {
        LineEnding::Lf => {
            if !content.contains('\r') {
                Cow::Borrowed(content)
            } else {
                Cow::Owned(content.replace("\r\n", "\n"))
            }
        }
        LineEnding::Crlf => {
            // First normalize everything to LF, then convert to CRLF
            let normalized = content.replace("\r\n", "\n");
            Cow::Owned(normalized.replace('\n', "\r\n"))
        }
        LineEnding::Mixed => Cow::Borrowed(content),
    }
}

pub(crate) fn ensure_consistent_line_endings(original: &str, modified: &str) -> String {
    let original_ending = detect_line_ending_enum(original);

    // For mixed line endings, normalize to the most common one (like detect_line_ending does)
    let target_ending = if original_ending == LineEnding::Mixed {
        // Use the same logic as detect_line_ending: prefer the more common one
        let crlf_count = original.matches("\r\n").count();
        let lf_count = original.matches('\n').count() - crlf_count;
        if crlf_count > lf_count {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        }
    } else {
        original_ending
    };

    let modified_ending = detect_line_ending_enum(modified);

    if target_ending != modified_ending {
        normalize_line_ending(modified, target_ending).into_owned()
    } else {
        modified.to_string()
    }
}
