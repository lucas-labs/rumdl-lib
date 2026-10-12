use crate::config::MarkdownFlavor;
use crate::utils::mkdocs_html_markdown::MarkdownHtmlTracker;

use super::types::*;

/// Tracks whether we're inside a fenced code block within a MkDocs container.
///
/// MkDocs admonitions, content tabs, and markdown HTML blocks use 4-space indentation
/// which pulldown-cmark misclassifies as indented code blocks. We clear `in_code_block`
/// for container content, but must preserve it for actual fenced code blocks (``` or ~~~)
/// within those containers.
struct FencedCodeTracker {
    in_fenced_code: bool,
    fence_marker: Option<String>,
}

impl FencedCodeTracker {
    fn new() -> Self {
        Self {
            in_fenced_code: false,
            fence_marker: None,
        }
    }

    /// Process a trimmed line and update fenced code state.
    /// Returns true if currently inside a fenced code block.
    fn process_line(&mut self, trimmed: &str) -> bool {
        if !self.in_fenced_code {
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                let fence_char = trimmed.chars().next().unwrap();
                let fence_len = trimmed.chars().take_while(|&c| c == fence_char).count();
                if fence_len >= 3 {
                    self.in_fenced_code = true;
                    self.fence_marker = Some(fence_char.to_string().repeat(fence_len));
                }
            }
            self.in_fenced_code
        } else if let Some(ref marker) = self.fence_marker {
            let fence_char = marker.chars().next().unwrap();
            if trimmed.starts_with(marker.as_str())
                && trimmed
                    .chars()
                    .skip(marker.len())
                    .all(|c| c == fence_char || c.is_whitespace())
            {
                // The closing fence is still part of the code block for the
                // current line, so return true. Subsequent lines will see
                // in_fenced_code = false.
                self.in_fenced_code = false;
                self.fence_marker = None;
                return true;
            }
            true
        } else {
            self.in_fenced_code
        }
    }

    /// Reset state when exiting a container.
    fn reset(&mut self) {
        self.in_fenced_code = false;
        self.fence_marker = None;
    }
}

/// Lines whose leading indentation is container structure rather than code.
///
/// MkDocs admonitions, content tabs and `<div markdown>` blocks hold their
/// content at a 4-space indent, which pulldown-cmark reads as an indented code
/// block. Every flag here is derived from the line text alone, so the answer is
/// available before the parse it corrects, and there is one computation behind
/// both the `LineInfo` flags and the byte ranges built from them.
pub(super) struct ContainerLines {
    /// Line is an admonition marker or admonition content.
    in_admonition: Vec<bool>,
    /// Line is a content-tab marker or tab content.
    in_content_tab: Vec<bool>,
    /// Line is inside a `<div markdown>`-style block.
    in_html_markdown: Vec<bool>,
    /// Line sits in a container where the indentation is structure, so a parse
    /// that read it as an indented code block was wrong. False for a fenced
    /// code block nested in the container, where the content really is code.
    is_container_body: Vec<bool>,
    /// Line opens a container, so it owns the body below it rather than
    /// belonging to the body above it.
    opens_container: Vec<bool>,
    /// Indent of the container that owns this line, or `usize::MAX` for a line
    /// no container owns. An opener owns itself.
    owner_indent: Vec<usize>,
    /// Indent of the innermost admonition or content tab holding this line, so
    /// its body sits four columns further in. Meaningless for a line no such
    /// container holds.
    body_indent: Vec<usize>,
}

impl ContainerLines {
    /// Whether the line's indentation is container structure rather than code.
    pub(super) fn is_container_body(&self, line_index: usize) -> bool {
        self.is_container_body.get(line_index).copied().unwrap_or(false)
    }

    /// Whether the line belongs to a container, nested fenced code included.
    fn in_container(&self, line_index: usize) -> bool {
        self.in_admonition[line_index] || self.in_content_tab[line_index] || self.in_html_markdown[line_index]
    }

    /// Whether an admonition or content tab holds this line at a fixed indent.
    fn has_body_indent(&self, line_index: usize) -> bool {
        self.in_admonition[line_index] || self.in_content_tab[line_index]
    }

    /// The parts of a parser-reported code block that really are code.
    ///
    /// A block laid over container content is the parser reading structure as
    /// code. What is left are the lines the container does not hold as
    /// structure, which is exactly the fenced code blocks written inside it.
    pub(super) fn code_line_spans_in(&self, line_range: std::ops::Range<usize>) -> Vec<std::ops::Range<usize>> {
        let start = line_range.start.min(self.is_container_body.len());
        let end = line_range.end.min(self.is_container_body.len());
        let mut spans = Vec::new();
        let mut span_start = None;
        for i in start..end {
            if self.is_container_body[i] {
                if let Some(from) = span_start.take() {
                    spans.push(from..i);
                }
            } else if span_start.is_none() {
                span_start = Some(i);
            }
        }
        if let Some(from) = span_start {
            spans.push(from..end);
        }
        spans
    }

    /// The last line of the container body that starts at `line_index`.
    ///
    /// The body ends where the container ends, or where a sibling container
    /// takes over: an opener at the same or smaller indent belongs to the
    /// container above this one, not to its body. A fenced code block inside
    /// the container does not end it, matching what an unclosed comment hides
    /// at the top level.
    pub(super) fn body_end_line(&self, line_index: usize) -> Option<usize> {
        if !self.is_container_body(line_index) {
            return None;
        }
        let owner = self.owner_indent[line_index];
        let mut end = line_index;
        for i in line_index + 1..self.is_container_body.len() {
            if !self.in_container(i) {
                break;
            }
            if self.opens_container[i] && self.owner_indent[i] <= owner {
                break;
            }
            end = i;
        }
        Some(end)
    }
}

/// Compute the container structure that the line text alone determines.
///
/// Admonitions and content tabs are MkDocs syntax and are only recognized in
/// that flavor. A `markdown` attribute on a block-level HTML element is an
/// unambiguous author-supplied signal, so those blocks are recognized in every
/// flavor - otherwise `rumdl fmt` silently mangles a page whose flavor is unset.
pub(super) fn detect_container_lines(content_lines: &[&str], _flavor: MarkdownFlavor) -> ContainerLines {
    let count = content_lines.len();
    let mut containers = ContainerLines {
        in_admonition: vec![false; count],
        in_content_tab: vec![false; count],
        in_html_markdown: vec![false; count],
        is_container_body: vec![false; count],
        opens_container: vec![false; count],
        owner_indent: vec![usize::MAX; count],
        body_indent: vec![0; count],
    };

    let mut markdown_html_tracker = MarkdownHtmlTracker::new();
    let mut html_markdown_fence = FencedCodeTracker::new();

    for (i, line) in content_lines.iter().enumerate() {
        containers.in_html_markdown[i] = markdown_html_tracker.process_line(line);
        if containers.in_html_markdown[i] {
            let in_fenced = html_markdown_fence.process_line(line.trim());
            if !in_fenced {
                containers.is_container_body[i] = true;
            }
        } else {
            html_markdown_fence.reset();
        }
    }

    restore_indented_code_in_bodies(content_lines, &mut containers);

    containers
}

/// The width of a line's indentation in columns, and the bytes it occupies.
///
/// A tab advances to the next four-column stop, which is how the parser counts
/// one.
fn leading_columns(line: &str) -> (usize, usize) {
    let mut columns = 0;
    let mut width = 0;
    for byte in line.bytes() {
        match byte {
            b' ' => columns += 1,
            b'\t' => columns += 4 - (columns % 4),
            _ => break,
        }
        width += 1;
    }
    (columns, width)
}

/// Give back the container-body lines that really are indented code.
///
/// A container holds its content four columns in, so a body line four columns
/// further still opens an indented code block of its own. Deciding that from the
/// line text would mean re-deriving the list and quote nesting the indent is
/// measured against, so the body is instead pulled back to the column the
/// container gives it and read by the same parser the rest of the document goes
/// through. Only a line the parser calls code loses its body flag, so this can
/// shrink the correction and never grow it.
fn restore_indented_code_in_bodies(content_lines: &[&str], containers: &mut ContainerLines) {
    // Every line the container holds moves to the column the container gives it,
    // since that is the coordinate the rest of the body is measured against.
    let dedentable: Vec<bool> = (0..content_lines.len())
        .map(|i| containers.has_body_indent(i) && !containers.opens_container[i])
        .collect();
    // Of those, the lines this pass decides. A `markdown` attribute says the
    // element's whole body is markdown however far in the author indented it, so
    // those lines keep the reading the container gave them.
    let judged: Vec<bool> = (0..content_lines.len())
        .map(|i| dedentable[i] && containers.is_container_body[i] && !containers.in_html_markdown[i])
        .collect();

    // The parse costs a pass over the document, so it is worth making only for a
    // container that holds something.
    if !(0..content_lines.len()).any(|i| judged[i] && !content_lines[i].trim().is_empty()) {
        return;
    }

    let mut dedented = String::with_capacity(content_lines.iter().map(|line| line.len() + 1).sum());
    let mut line_starts = Vec::with_capacity(content_lines.len());
    for (i, line) in content_lines.iter().enumerate() {
        line_starts.push(dedented.len());
        if containers.opens_container[i] {
            // An opener is structure rather than content, and blanking it keeps
            // the body below it from being read as part of what came before.
        } else if dedentable[i] {
            // Indentation is measured in columns, so a tab is worth what it takes
            // to reach the next stop and the whitespace is rewritten as the spaces
            // the parser would have counted.
            let (columns, width) = leading_columns(line);
            dedented.extend(std::iter::repeat_n(
                ' ',
                columns.saturating_sub(containers.body_indent[i] + 4),
            ));
            dedented.push_str(&line[width..]);
        } else {
            dedented.push_str(line);
        }
        dedented.push('\n');
    }

    let code = crate::utils::code_block_utils::CodeBlockUtils::detect_code_blocks(&dedented);
    for i in 0..content_lines.len() {
        if !judged[i] {
            continue;
        }
        // An indented block starts at the first character of its content rather
        // than at the start of the line, so the line is tested as a range.
        let start = line_starts[i];
        let end = line_starts.get(i + 1).copied().unwrap_or(dedented.len());
        if code.iter().any(|&(from, to)| from < end && start < to) {
            containers.is_container_body[i] = false;
        }
    }
}

/// Apply `<div markdown>`-style HTML block structure to the line info.
///
/// The `markdown` attribute on a block-level HTML element is Python-Markdown's
/// `md_in_html` opt-in and is also used by MkDocs Material for constructs like
/// grid cards.
///
/// Also clears `in_code_block` for container content, which the parser read as
/// an indented code block.
pub(super) fn detect_markdown_html_blocks(lines: &mut [LineInfo], containers: &ContainerLines) {
    for (i, line) in lines.iter_mut().enumerate() {
        line.in_mkdocs_html_markdown = containers.in_html_markdown.get(i).copied().unwrap_or(false);
        if containers.is_container_body(i) {
            line.in_code_block = false;
        }
    }
}

/// What a scan of the content found for Obsidian `%%` comments.
#[derive(Debug, Default)]
pub(super) struct ObsidianCommentScan {
    /// Byte ranges of the comments, in document order. An unclosed comment
    /// still gets a range, running to the end of the document, because that is
    /// how much of the document Obsidian hides.
    pub(crate) ranges: Vec<(usize, usize)>,
}

// ============================================================================
// MyST Markdown Detection
// ============================================================================
