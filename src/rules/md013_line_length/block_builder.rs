//! Block-level state machine for MD013 reflow.
//!
//! Turns a stream of `LineType` events into a `Vec<Block>`
//! that the reflow renderer can consume. The builder centralises bookkeeping
//! that would otherwise be 20+ scattered locals plus repeated "flush admonition
//! + flush table + flush the active mutex group" sequences in every standalone-line arm.
//!
//! ## State machine layers
//!
//! 1. **Container blocks** that nest other content: `admonition`, `table`.
//!    These flush independently and never coexist.
//! 2. **Mutex group**: at most one of `code`, `html`, `paragraph` is in flight
//!    at a time. [`BlockBuilder::flush_mutex_group`] closes whichever is active.
//! 3. **Standalone lines** (`SemanticLine`, `SnippetLine`, `DivMarker`) flush
//!    everything via [`BlockBuilder::flush_for_new_block`] before pushing themselves.
//!
//! ## Public API
//!
//! Callers feed one `LineType` at a time via the `feed_*`
//! methods, then call [`BlockBuilder::finalize`] to drain remaining state into
//! a `Vec<Block>`. All HTML state-machine bookkeeping is encapsulated inside
//! [`BlockBuilder::feed_content`].

/// A semantic block in a list item, ready for the reflow renderer.
///
/// `Block` is a transport DTO between two collaborators in the same parent
/// module: [`BlockBuilder`] is responsible for *construction* (deciding
/// which variant a stream of `LineType` events
/// produces, and bookkeeping HTML/admonition/table state), and the reflow
/// renderer in `mod.rs` is responsible for *consumption* (pattern
/// matching to apply per-variant rendering rules). The two halves
/// deliberately speak the same tagged-sum-type vocabulary because the
/// rendering logic depends on rendering context (`LineLengthConfig`,
/// `MdConfig`, fix state, the parent line index) that does not belong
/// inside this submodule.
///
/// The encapsulation boundary is therefore "BlockBuilder owns construction;
/// downstream owns consumption", not "Block is opaque". Adding a new
/// variant requires updating both halves.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Block {
    Paragraph(Vec<(String, usize)>),
    Code {
        /// `(content, indent)` pairs preserving original indentation.
        lines: Vec<(String, usize)>,
        /// Whether a blank line preceded this block in the source.
        has_preceding_blank: bool,
    },
    /// A semantic marker (NOTE:, WARNING:, …) preserved on its own line.
    SemanticLine(String),
    /// An MkDocs snippet delimiter (`-8<-`) preserved verbatim with no extra spacing.
    SnippetLine(String),
    /// A Quarto/Pandoc div marker (`:::` opening or closing) preserved verbatim.
    DivMarker(String),
    Html {
        /// HTML lines preserved exactly as-is.
        lines: Vec<String>,
        has_preceding_blank: bool,
    },
    Admonition {
        /// e.g. `!!! note` or `??? warning "Title"`.
        header: String,
        /// Original indent of the header line.
        header_indent: usize,
        /// `(text, original_indent)` pairs for body lines.
        content_lines: Vec<(String, usize)>,
    },
    Table {
        /// `(row_text, original_indent)` pairs preserved verbatim.
        lines: Vec<(String, usize)>,
        has_preceding_blank: bool,
    },
}

/// Block-level HTML tags whose presence triggers HTML block detection.
const BLOCK_LEVEL_TAGS: &[&str] = &[
    "div",
    "details",
    "summary",
    "section",
    "article",
    "header",
    "footer",
    "nav",
    "aside",
    "main",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "td",
    "th",
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd",
    "pre",
    "blockquote",
    "figure",
    "figcaption",
    "form",
    "fieldset",
    "legend",
    "hr",
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "style",
    "script",
    "noscript",
];

/// If `line` opens a block-level HTML tag (or HTML comment), return the
/// lowercased tag name. Returns `Some("!--")` for the comment sentinel.
fn block_html_opening_tag(line: &str) -> Option<String> {
    let trimmed = line.trim();

    if trimmed.starts_with("<!--") {
        return Some("!--".to_string());
    }

    if trimmed.starts_with('<') && !trimmed.starts_with("</") && !trimmed.starts_with("<!") {
        let after_bracket = &trimmed[1..];
        if let Some(end) = after_bracket.find(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            let tag_name = after_bracket[..end].to_lowercase();
            if BLOCK_LEVEL_TAGS.contains(&tag_name.as_str()) {
                return Some(tag_name);
            }
        }
    }
    None
}

/// Whether `line` is a closing tag for `tag_name`. The sentinel `"!--"`
/// matches the `-->` comment terminator.
fn is_html_closing_tag(line: &str, tag_name: &str) -> bool {
    let trimmed = line.trim();

    if tag_name == "!--" {
        return trimmed.ends_with("-->");
    }

    trimmed.starts_with(&format!("</{tag_name}>"))
        || trimmed.starts_with(&format!("</{tag_name}  "))
        || (trimmed.starts_with("</") && trimmed[2..].trim_start().starts_with(tag_name))
}

/// Whether `line` is a self-closing tag (`<foo/>`).
fn is_self_closing_tag(line: &str) -> bool {
    line.trim().ends_with("/>")
}

/// State machine that consumes line events and produces a `Vec<Block>`.
///
/// Construct via [`BlockBuilder::new_with_start_line`], feed one line at a time via the
/// `feed_*` methods, then call [`BlockBuilder::finalize`] to extract the
/// blocks.
pub(super) struct BlockBuilder {
    blocks: Vec<Block>,

    current_paragraph: Vec<(String, usize)>,
    current_code_block: Vec<(String, usize)>,
    current_html_block: Vec<String>,
    html_tag_stack: Vec<String>,
    current_table: Vec<(String, usize)>,

    in_code: bool,
    in_html_block: bool,
    in_table: bool,
    in_admonition_block: bool,

    admonition_header: Option<(String, usize)>,
    admonition_content: Vec<(String, usize)>,

    had_preceding_blank: bool,
    code_block_has_preceding_blank: bool,
    html_block_has_preceding_blank: bool,
    table_has_preceding_blank: bool,

    current_line: usize,
}

impl BlockBuilder {
    pub(super) fn new_with_start_line(start_line: usize) -> Self {
        Self {
            blocks: Vec::new(),
            current_paragraph: Vec::new(),
            current_code_block: Vec::new(),
            current_html_block: Vec::new(),
            html_tag_stack: Vec::new(),
            current_table: Vec::new(),
            in_code: false,
            in_html_block: false,
            in_table: false,
            in_admonition_block: false,
            admonition_header: None,
            admonition_content: Vec::new(),
            had_preceding_blank: false,
            code_block_has_preceding_blank: false,
            html_block_has_preceding_blank: false,
            table_has_preceding_blank: false,
            current_line: start_line,
        }
    }

    // ------------------------------------------------------------------------
    // Feeders — one per LineType variant. Callers do `match line { ... }` and
    // dispatch to a single feeder method; all state mutation lives here.
    // ------------------------------------------------------------------------

    /// Feed a blank line. Blank lines extend multi-line containers
    /// (admonition, code, html) but terminate tables and paragraphs.
    pub(super) fn feed_blank_line(&mut self) {
        if self.in_admonition_block {
            self.admonition_content.push((String::new(), 0));
        } else if self.in_code {
            self.current_code_block.push((String::new(), 0));
        } else if self.in_html_block {
            self.current_html_block.push(String::new());
        } else if self.in_table {
            self.flush_table();
        } else {
            self.flush_paragraph();
        }
        self.had_preceding_blank = true;
        self.current_line += 1;
    }

    /// Feed a content (CommonMark text) line. Encapsulates the HTML
    /// state machine: detects block-level HTML opens/closes and routes
    /// the line into the appropriate buffer.
    pub(super) fn feed_content(&mut self, content: &str) {
        self.flush_admonition_and_table();
        if self.in_html_block {
            self.extend_html_block(content);
        } else if let Some(tag_name) = block_html_opening_tag(content) {
            self.start_html_block(content, tag_name);
        } else {
            self.append_to_paragraph(content);
        }
        self.had_preceding_blank = false;
        self.current_line += 1;
    }

    /// Feed a fenced or indented code block line.
    pub(super) fn feed_code_line(&mut self, content: &str, indent: usize) {
        self.flush_admonition_and_table();
        self.flush_html();
        if !self.in_code {
            self.flush_paragraph();
            self.in_code = true;
            self.code_block_has_preceding_blank = self.had_preceding_blank;
        }
        self.current_code_block.push((content.to_string(), indent));
        self.had_preceding_blank = false;
        self.current_line += 1;
    }

    /// Feed a standalone semantic marker (NOTE:, WARNING:, …).
    pub(super) fn feed_semantic_line(&mut self, content: &str) {
        self.flush_for_new_block();
        self.blocks.push(Block::SemanticLine(content.to_string()));
        self.had_preceding_blank = false;
        self.current_line += 1;
    }

    /// Feed an MkDocs snippet delimiter (`-8<-`).
    pub(super) fn feed_snippet_line(&mut self, content: &str) {
        self.flush_for_new_block();
        self.blocks.push(Block::SnippetLine(content.to_string()));
        self.had_preceding_blank = false;
        self.current_line += 1;
    }

    /// Feed a Quarto/Pandoc div marker (`:::` opening or closing).
    pub(super) fn feed_div_marker(&mut self, content: &str) {
        self.flush_for_new_block();
        self.blocks.push(Block::DivMarker(content.to_string()));
        self.had_preceding_blank = false;
        self.current_line += 1;
    }

    /// Feed an admonition header. Starts a fresh admonition container.
    pub(super) fn feed_admonition_header(&mut self, header_text: &str, indent: usize) {
        self.flush_for_new_block();
        self.in_admonition_block = true;
        self.admonition_header = Some((header_text.to_string(), indent));
        self.admonition_content.clear();
        self.had_preceding_blank = false;
        self.current_line += 1;
    }

    /// Feed an admonition body line. If no admonition header is in scope this
    /// degrades to appending the line to the current paragraph rather than
    /// silently dropping it.
    pub(super) fn feed_admonition_content(&mut self, content: &str, indent: usize) {
        if self.in_admonition_block {
            self.admonition_content.push((content.to_string(), indent));
        } else {
            self.append_to_paragraph(content);
        }
        self.had_preceding_blank = false;
        self.current_line += 1;
    }

    /// Feed a GFM table row. Tables flush admonition (peer container) and the
    /// mutex group, but extend themselves rather than flushing.
    pub(super) fn feed_table_line(&mut self, content: &str, indent: usize) {
        self.flush_admonition();
        self.flush_mutex_group();
        if !self.in_table {
            self.in_table = true;
            self.table_has_preceding_blank = self.had_preceding_blank;
        }
        self.current_table.push((content.to_string(), indent));
        self.had_preceding_blank = false;
        self.current_line += 1;
    }

    /// Drain remaining state and return the accumulated blocks.
    pub(super) fn finalize(mut self) -> Vec<Block> {
        self.flush_admonition();
        self.flush_table();
        if self.in_code && !self.current_code_block.is_empty() {
            self.blocks.push(Block::Code {
                lines: self.current_code_block,
                has_preceding_blank: self.code_block_has_preceding_blank,
            });
        }
        if self.in_html_block && !self.current_html_block.is_empty() {
            self.blocks.push(Block::Html {
                lines: self.current_html_block,
                has_preceding_blank: self.html_block_has_preceding_blank,
            });
        }
        if !self.current_paragraph.is_empty() {
            self.blocks.push(Block::Paragraph(self.current_paragraph));
        }
        self.blocks
    }

    // ------------------------------------------------------------------------
    // Internal flush primitives. Each `flush_*` is a no-op when the
    // corresponding buffer is empty / inactive, so callers can compose them
    // freely without explicit guards.
    // ------------------------------------------------------------------------

    fn flush_admonition(&mut self) {
        if self.in_admonition_block {
            if let Some((h, hi)) = self.admonition_header.take() {
                self.blocks.push(Block::Admonition {
                    header: h,
                    header_indent: hi,
                    content_lines: std::mem::take(&mut self.admonition_content),
                });
            }
            self.in_admonition_block = false;
        }
    }

    fn flush_table(&mut self) {
        if self.in_table {
            self.blocks.push(Block::Table {
                lines: std::mem::take(&mut self.current_table),
                has_preceding_blank: self.table_has_preceding_blank,
            });
            self.in_table = false;
        }
    }

    fn flush_admonition_and_table(&mut self) {
        self.flush_admonition();
        self.flush_table();
    }

    fn flush_code(&mut self) {
        if self.in_code {
            self.blocks.push(Block::Code {
                lines: std::mem::take(&mut self.current_code_block),
                has_preceding_blank: self.code_block_has_preceding_blank,
            });
            self.in_code = false;
        }
    }

    fn flush_html(&mut self) {
        if self.in_html_block {
            self.blocks.push(Block::Html {
                lines: std::mem::take(&mut self.current_html_block),
                has_preceding_blank: self.html_block_has_preceding_blank,
            });
            self.html_tag_stack.clear();
            self.in_html_block = false;
        }
    }

    fn flush_paragraph(&mut self) {
        if !self.current_paragraph.is_empty() {
            self.blocks
                .push(Block::Paragraph(std::mem::take(&mut self.current_paragraph)));
        }
    }

    /// Flush whichever of code / html / paragraph is currently in flight.
    /// They are mutually exclusive by construction, so the if/else chain
    /// preserves the original semantics exactly.
    fn flush_mutex_group(&mut self) {
        if self.in_code {
            self.flush_code();
        } else if self.in_html_block {
            self.flush_html();
        } else {
            self.flush_paragraph();
        }
    }

    /// Flush all in-flight state to start a fresh standalone block.
    /// Used by every LineType that introduces a new top-level block.
    fn flush_for_new_block(&mut self) {
        self.flush_admonition_and_table();
        self.flush_mutex_group();
    }

    // ------------------------------------------------------------------------
    // HTML state-machine internals. These are private to the module so the
    // tag-stack invariant ("only ever modified by these helpers") is local.
    // ------------------------------------------------------------------------

    fn extend_html_block(&mut self, content: &str) {
        self.current_html_block.push(content.to_string());

        // Track HTML block boundaries via the tag stack: a closing tag for
        // the topmost element pops the stack; nested opening tags push.
        let last_tag = match self.html_tag_stack.last() {
            Some(t) => t.clone(),
            None => return,
        };
        if is_html_closing_tag(content, &last_tag) {
            self.html_tag_stack.pop();
            if self.html_tag_stack.is_empty() {
                self.flush_html();
            }
        } else if let Some(new_tag) = block_html_opening_tag(content)
            && !is_self_closing_tag(content)
        {
            self.html_tag_stack.push(new_tag);
        }
    }

    fn start_html_block(&mut self, content: &str, tag_name: String) {
        // Starting a new HTML block: flush whichever of code / paragraph
        // is active first (admonition + table already flushed by caller).
        if self.in_code {
            self.flush_code();
        } else {
            self.flush_paragraph();
        }
        self.in_html_block = true;
        self.html_block_has_preceding_blank = self.had_preceding_blank;
        self.current_html_block.push(content.to_string());
        if is_self_closing_tag(content) {
            self.flush_html();
        } else {
            self.html_tag_stack.push(tag_name);
        }
    }

    fn append_to_paragraph(&mut self, content: &str) {
        if self.in_code {
            self.flush_code();
        }
        self.current_paragraph.push((content.to_string(), self.current_line));
    }
}
