//! Shared HTML block-start classification.
//!
//! A line whose first tag names one of these elements starts an HTML block in
//! rumdl's parser (`lint_context::heading_detection::detect_html_blocks`).
//! Reflow consults the same predicate so a wrapped line can never introduce a
//! construct the parser would classify differently: the two must agree, and
//! sharing the list keeps them from drifting apart.

/// Type-1 tags per CommonMark: blank lines inside these blocks do not
/// terminate them — only a matching end tag (or EOF) does.
pub const TYPE_1_BLOCK_ELEMENTS: &[&str] = &["pre", "script", "style", "textarea"];

/// HTML elements whose tags open an HTML block at line start (CommonMark
/// type-1 and type-6 conditions, as recognized by rumdl's parser).
pub const BLOCK_ELEMENTS: &[&str] = &[
    "address",
    "article",
    "aside",
    "audio",
    "blockquote",
    "canvas",
    "details",
    "dialog",
    "dd",
    "div",
    "dl",
    "dt",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "iframe",
    "li",
    "main",
    "menu",
    "nav",
    "noscript",
    "object",
    "ol",
    "p",
    "picture",
    "pre",
    "script",
    "search",
    "section",
    "source",
    "style",
    "summary",
    "svg",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "tr",
    "track",
    "ul",
    "video",
];

/// If `trimmed` (a line with leading whitespace already stripped) opens an
/// HTML block per rumdl's parser, return the lowercased tag name and whether
/// it is a closing tag. Returns `None` for text, autolinks, and inline-level
/// tags (`<span>`, `<b>`, ...), which cannot interrupt a paragraph.
///
/// The tag name has to end the way CommonMark's start conditions require:
/// whitespace, the end of the line, `>` or `/>` may follow it, so `<div.class>`
/// or `<p,` is text that happens to begin with a block element's name.
pub(crate) fn parse_html_block_start(trimmed: &str) -> Option<(String, bool)> {
    let after_bracket = trimmed.strip_prefix('<')?;
    if after_bracket.is_empty() {
        return None;
    }
    let is_closing = after_bracket.starts_with('/');
    let tag_start = if is_closing { &after_bracket[1..] } else { after_bracket };

    let tag_name = tag_start
        .chars()
        .take_while(|c| c.is_ascii_alphabetic() || *c == '-' || c.is_ascii_digit())
        .collect::<String>()
        .to_lowercase();

    let rest = &tag_start[tag_name.len()..];
    let terminated =
        rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_whitespace() || c == '>') || rest.starts_with("/>");

    if terminated && !tag_name.is_empty() && BLOCK_ELEMENTS.contains(&tag_name.as_str()) {
        Some((tag_name, is_closing))
    } else {
        None
    }
}

/// Whether `trimmed` (a line with leading whitespace already stripped) opens
/// an HTML block that no tag name identifies: a comment, a processing
/// instruction, a declaration or a CDATA section (CommonMark start conditions
/// 2 to 5). Each interrupts a paragraph like a block-level tag does, and none
/// of them is a tag `parse_html_block_start` can name, so a caller asking
/// "does this line start a block?" needs both.
pub(crate) fn opens_untagged_html_block(trimmed: &str) -> bool {
    let Some(after_bracket) = trimmed.strip_prefix('<') else {
        return false;
    };
    after_bracket.starts_with("!--")
        || after_bracket.starts_with('?')
        || after_bracket.starts_with("![CDATA[")
        || after_bracket
            .strip_prefix('!')
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_alphabetic()))
}
