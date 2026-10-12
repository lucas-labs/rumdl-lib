/// MkDocs emoji and icon shortcode detection.
///
/// Supports the MkDocs Material emoji/icons extension and standard GitHub-style
/// emoji shortcodes, exposing just enough surface for `skip_context` to
/// determine whether a given position falls inside one.
///
/// ## References
///
/// - [MkDocs Material Icons](https://squidfunk.github.io/mkdocs-material/reference/icons-emojis/)
/// - [Python-Markdown Emoji](https://facelessuser.github.io/pymdown-extensions/extensions/emoji/)
use regex::Regex;
use std::sync::LazyLock;

/// Pattern to match MkDocs icon shortcodes like `:material-check:`,
/// `:octicons-mark-github-16:`, or `:fontawesome-brands-github:`.
static ICON_SHORTCODE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":([a-z][a-z0-9_]*(?:-[a-z0-9_]+)+):").unwrap());

/// Pattern to match standard emoji shortcodes like `:smile:` or `:+1:`.
static EMOJI_SHORTCODE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":([a-zA-Z0-9_+-]+):").unwrap());

/// Check if a position in a line is within any emoji/icon shortcode.
pub(crate) fn is_in_any_shortcode(line: &str, position: usize) -> bool {
    if !line.contains(':') {
        return false;
    }

    for m in ICON_SHORTCODE_PATTERN.find_iter(line) {
        if m.start() <= position && position < m.end() {
            return true;
        }
    }

    for m in EMOJI_SHORTCODE_PATTERN.find_iter(line) {
        if m.start() <= position && position < m.end() {
            return true;
        }
    }

    false
}
