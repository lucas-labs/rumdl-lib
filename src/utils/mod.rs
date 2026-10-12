//!
//! Shared utilities for rumdl, including document structure analysis, code block handling, regex helpers, and string extensions.
//! Provides reusable traits and functions for rule implementations and core linter logic.

pub mod anchor_styles;
pub mod atomic_write;
pub mod blank_lines;
pub mod blockquote;
pub mod code_block_utils;
pub mod emphasis_utils;
pub mod fix_utils;
pub(crate) mod gh_aw;
pub(crate) mod hard_break;
pub mod header_id_utils;
pub mod html_block;
pub mod html_elements;
pub mod jinja_utils;
pub mod kramdown_utils;
pub mod line_ending;
pub(crate) mod list_fix_guard;
pub mod list_indent_shift;
pub mod mdg;
pub mod mkdocs_admonitions;
pub mod mkdocs_attr_list;
pub mod mkdocs_common;
pub mod mkdocs_config;
pub mod mkdocs_critic;
pub mod mkdocs_definition_lists;
pub mod mkdocs_extensions;
pub mod mkdocs_footnotes;
pub mod mkdocs_html_markdown;
pub mod mkdocs_icons;
pub mod mkdocs_patterns;
pub mod mkdocs_snippets;
pub mod mkdocs_tabs;
pub mod mkdocstrings_refs;
pub mod obsidian_config;
pub mod obsidian_tag;
pub mod pandoc;
pub mod parser_options;
pub mod project_root;
pub mod pymdown_blocks;
pub mod quarto_chunks;
pub mod range_utils;
pub mod regex_cache;
pub mod sentence_utils;
pub mod skip_context;
pub mod table_utils;
pub mod text_reflow;
pub mod thematic_break;
pub mod unicode;
pub mod upward_walk;
pub mod utf8_offsets;

pub(crate) use line_ending::{detect_line_ending, ensure_consistent_line_endings};
pub(crate) use parser_options::rumdl_parser_options;

/// Calculate the visual indentation width of a string, expanding tabs to spaces.
///
/// Per CommonMark, tabs expand to the next tab stop (columns 4, 8, 12, ...).
pub(crate) fn calculate_indentation_width(indent_str: &str, tab_width: usize) -> usize {
    let mut width = 0;
    for ch in indent_str.chars() {
        if ch == '\t' {
            width = ((width / tab_width) + 1) * tab_width;
        } else if ch == ' ' {
            width += 1;
        } else {
            break;
        }
    }
    width
}

/// Calculate the visual indentation width using default tab width of 4
pub(crate) fn calculate_indentation_width_default(indent_str: &str) -> usize {
    calculate_indentation_width(indent_str, 4)
}

/// Check if a line consists only of a template directive with no surrounding text.
///
/// Detects template syntax used in static site generators:
/// - Handlebars/mdBook/Mustache: `{{...}}`
/// - Jinja2/Liquid/Jekyll: `{%...%}`
/// - Hugo shortcodes: `{{<...>}}` or `{{%...%}}`
///
/// Template directives are preprocessor instructions that should not be merged
/// into surrounding paragraphs during reflow.
pub(crate) fn is_template_directive_only(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    (trimmed.starts_with("{{") && trimmed.ends_with("}}")) || (trimmed.starts_with("{%") && trimmed.ends_with("%}"))
}
