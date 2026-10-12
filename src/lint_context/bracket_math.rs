//! Standalone TeX `\[ ... \]` display math for MD013's opt-in policy.
//!
//! This is deliberately line-oriented. A delimiter must occupy its own line
//! (apart from list and blockquote structure), and an opener is only protected
//! when a matching closer exists. Ordinary escaped brackets stay Markdown.

use super::{CodeSpan, LineInfo, ListBlock};

#[derive(Default)]
pub(crate) struct BracketDisplayMathLines {
    pub(crate) multiline: Vec<bool>,
    pub(crate) standalone: Vec<bool>,
}

fn body<'a>(line: &'a LineInfo, content: &'a str, in_code_span: bool) -> Option<&'a str> {
    if in_code_span || line.in_code_block || line.in_front_matter || line.in_html_block || line.in_html_comment {
        return None;
    }

    let raw = line.content(content);
    let mut text = line
        .blockquote
        .as_ref()
        .map_or(raw, |quote| quote.content.as_str())
        .trim_start();

    // An opener can be the content of a list marker. Continuation lines only
    // have indentation, which trim_start above already removed.
    if line.list_item.is_some() {
        let marker_end = if text.starts_with(['-', '*', '+']) {
            Some(1)
        } else {
            let digits = text.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && matches!(text.as_bytes().get(digits), Some(b'.' | b')')) {
                Some(digits + 1)
            } else {
                None
            }
        };
        if let Some(end) = marker_end
            && text.as_bytes().get(end).is_some_and(u8::is_ascii_whitespace)
        {
            text = text[end..].trim_start();
        }
    }

    Some(text.trim_end())
}

pub(crate) fn parse(
    content: &str,
    lines: &[LineInfo],
    code_spans: &[CodeSpan],
    list_blocks: &[ListBlock],
) -> BracketDisplayMathLines {
    let mut result = BracketDisplayMathLines {
        multiline: vec![false; lines.len()],
        standalone: vec![false; lines.len()],
    };
    let mut code_span_lines = vec![false; lines.len()];
    for span in code_spans {
        if let Some(touched) = code_span_lines.get_mut(span.line.saturating_sub(1)..span.end_line) {
            touched.fill(true);
        }
    }
    let bodies: Vec<_> = lines
        .iter()
        .zip(code_span_lines)
        .map(|(line, in_code_span)| body(line, content, in_code_span))
        .collect();
    let mut opener = None;

    for (index, candidate) in bodies.iter().enumerate() {
        // A fenced block, HTML block, or code span breaks the candidate math
        // block. Otherwise a later closing delimiter could protect unrelated
        // Markdown (and exempt it from MD013's width check).
        let Some(text) = candidate else {
            opener = None;
            continue;
        };
        let is_open = *text == r"\[";
        let is_close = *text == r"\]";
        if !is_open && !is_close {
            if opener.is_none() && text.starts_with(r"\[") && text.ends_with(r"\]") {
                result.standalone[index] = true;
            }
            continue;
        }

        let quote_depth = lines[index].blockquote.as_ref().map_or(0, |quote| quote.nesting_level);
        let line_num = index + 1;
        let list_id = list_blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| (block.start_line..=block.end_line).contains(&line_num))
            .max_by_key(|(_, block)| (block.nesting_level, block.start_line))
            .map(|(id, _)| id);
        if is_open {
            opener = Some((index, quote_depth, list_id));
        } else if is_close && opener.is_some_and(|(_, depth, list)| depth == quote_depth && list == list_id) {
            let (start, _, _) = opener.take().unwrap();
            result.multiline[start..=index].fill(true);
        }
    }

    result
}
