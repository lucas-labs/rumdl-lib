pub mod types;
pub(crate) use heading_detection::is_paragraph_text_line;
pub use types::*;

mod bracket_math;
mod element_parsers;
mod flavor_detection;
mod heading_detection;
mod line_computation;
mod link_parser;
mod list_blocks;

use crate::config::MarkdownFlavor;
use crate::rules::front_matter_utils::FrontMatterUtils;
use crate::utils::code_block_utils::{CodeBlockDetail, CodeBlockUtils};
use crate::utils::range_utils::byte_to_char_count;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;

/// Names a section of the parse; the name only documents the code.
macro_rules! profile_section {
    ($name:expr, $profile:expr, $code:expr) => {{ $code }};
}

fn build_commonmark_ordered_lists(
    lines: &[LineInfo],
    line_to_list: &crate::utils::code_block_utils::LineToListMap,
    list_start_values: &crate::utils::code_block_utils::ListStartValues,
) -> Vec<CommonMarkOrderedListInfo> {
    let mut grouped_lines: HashMap<usize, Vec<usize>> = HashMap::new();

    for (&line_num, &list_id) in line_to_list {
        let is_ordered_item = line_num
            .checked_sub(1)
            .and_then(|index| lines.get(index))
            .and_then(|line| line.list_item.as_deref())
            .is_some_and(|item| item.is_ordered);
        if is_ordered_item {
            grouped_lines.entry(list_id).or_default().push(line_num);
        }
    }

    let mut lists: Vec<_> = grouped_lines
        .into_iter()
        .map(|(list_id, mut item_lines)| {
            item_lines.sort_unstable();
            CommonMarkOrderedListInfo {
                start_value: list_start_values.get(&list_id).copied().unwrap_or(1),
                item_lines,
            }
        })
        .collect();
    lists.sort_by_key(|list| list.item_lines.first().copied().unwrap_or(0));
    lists
}

/// Grouped byte ranges for skip context detection
/// Used to reduce parameter count in internal functions
pub(super) struct SkipByteRanges<'a> {
    pub(super) html_comment_ranges: &'a [crate::utils::skip_context::ByteRange],
    pub(super) autodoc_ranges: &'a [crate::utils::skip_context::ByteRange],
}

use std::sync::{Arc, OnceLock};

/// Map from line byte offset to list item data: (is_ordered, marker, marker_column, content_column, number)
pub(super) type ListItemMap = std::collections::HashMap<usize, (bool, String, usize, usize, Option<usize>)>;

/// Type alias for byte ranges used in JSX expression and MDX comment detection
pub(super) type ByteRanges = Vec<(usize, usize)>;

pub struct LintContext<'a> {
    pub(crate) content: &'a str,
    content_lines: Vec<&'a str>, // Pre-split lines from content (avoids repeated allocations)
    pub(crate) line_offsets: Vec<usize>,
    pub(crate) code_blocks: Vec<(usize, usize)>, // Cached code block ranges (not including inline code spans)
    pub(crate) code_block_details: Vec<CodeBlockDetail>, // Per-block metadata (fenced/indented, info string)
    pub(crate) strong_spans: Vec<crate::utils::code_block_utils::StrongSpanDetail>, // Pre-computed strong emphasis spans
    line_to_list: crate::utils::code_block_utils::LineToListMap, // Private CommonMark membership input
    list_start_values: crate::utils::code_block_utils::ListStartValues, // Private CommonMark start-value input
    definition_lists: DefinitionListLines,                       // Parser-detected definition lists, in lines
    commonmark_ordered_lists_cache: OnceLock<Vec<CommonMarkOrderedListInfo>>, // Lazy source-ordered view
    pub lines: Vec<LineInfo>,                                    // Pre-computed line information
    blockquote_headings: Vec<Option<Box<HeadingInfo>>>,          // Container headings, parallel to `lines`
    links: Vec<ParsedLink<'a>>,                                  // Pre-parsed links
    images: Vec<ParsedImage<'a>>,                                // Pre-parsed images
    reference_defs: Vec<ReferenceDef>,                           // Reference definitions
    code_spans_cache: OnceLock<Arc<Vec<CodeSpan>>>,              // Lazy-loaded inline code spans
    math_spans_cache: OnceLock<Arc<Vec<MathSpan>>>,              // Lazy-loaded math spans ($...$ and $$...$$)
    bracket_math_cache: OnceLock<bracket_math::BracketDisplayMathLines>,
    math_byte_ranges_cache: OnceLock<Vec<(usize, usize)>>, // Lazy-loaded math byte ranges for is_in_math_context
    pub(crate) list_blocks: Vec<ListBlock>,                // Pre-parsed list blocks
    pub(crate) char_frequency: CharFrequency,              // Character frequency analysis
    html_tags_cache: OnceLock<Arc<Vec<HtmlTag>>>,          // Lazy-loaded HTML tags
    jsx_component_tags_cache: OnceLock<Arc<Vec<HtmlTag>>>, // Lazy-loaded JSX component tags (shares the html_tags parse)
    html_comment_ranges: Vec<crate::utils::skip_context::ByteRange>, // Pre-computed HTML comment ranges
    pub table_blocks: Vec<crate::utils::table_utils::TableBlock>, // Pre-computed table blocks
    line_index: crate::utils::range_utils::LineIndex<'a>,  // Pre-computed source-location index
    pub flavor: MarkdownFlavor,                            // Markdown flavor being used
    colon_fence_details: Vec<CodeBlockDetail>, // Pre-computed Azure DevOps colon code fences (:::lang ... :::)
    obsidian_comment_ranges: Vec<(usize, usize)>, // Pre-computed Obsidian comment ranges (%%...%%)
    lazy_cont_lines_cache: OnceLock<Arc<Vec<LazyContLine>>>, // Lazy-loaded lazy continuation lines
    front_matter_end: usize,                   // 1-indexed line where front matter ends, 0 if none
}

impl<'a> LintContext<'a> {
    pub fn new(content: &'a str, flavor: MarkdownFlavor, _source_file: Option<PathBuf>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let line_offsets = profile_section!("Line offsets", profile, {
            let mut offsets = vec![0];
            for (i, c) in content.char_indices() {
                if c == '\n' {
                    offsets.push(i + 1);
                }
            }
            offsets
        });

        // Compute content_lines once for all functions that need it
        let content_lines: Vec<&str> = content.lines().collect();

        // Detect front matter boundaries once for all functions that need it.
        // This is the single allowed call site; rules read the cached value
        // via front_matter_end_line().
        #[allow(clippy::disallowed_methods)]
        let front_matter_end = FrontMatterUtils::get_front_matter_end_line(content);

        // Detect code blocks and code spans once and cache them
        let parse_result = profile_section!(
            "Code blocks",
            profile,
            CodeBlockUtils::detect_code_blocks_and_spans(content)
        );
        let mut code_blocks = parse_result.code_blocks;
        let code_span_ranges = parse_result.code_spans;
        let code_block_details = parse_result.code_block_details;
        let strong_spans = parse_result.strong_spans;
        let line_to_list = parse_result.line_to_list;
        let list_start_values = parse_result.list_start_values;
        let html_blocks = parse_result.html_blocks;
        let definition_lists = DefinitionListLines::new(
            &line_offsets,
            &parse_result.definition_items,
            &parse_result.definition_terms,
            &parse_result.definition_texts,
        );

        // Container structure the parser cannot see. Computed from the line text
        // alone, so it is available here, before the line info it corrects.
        let containers = profile_section!(
            "Container lines",
            profile,
            flavor_detection::detect_container_lines(&content_lines, flavor)
        );

        // Pre-compute HTML comment ranges ONCE for all operations.
        // Code-span and code-block ranges are passed so `<!--`/`-->` inside code
        // are treated as literal text, not comment delimiters that could pair
        // across code regions on different lines. An indented block over
        // container content is the parser reading a MkDocs admonition or a
        // `<div markdown>` body as code, and a comment written there is a real
        // comment, so only the parts of such a block that a fence really does
        // hold as code are kept.
        let comment_code_block_ranges: Vec<(usize, usize)> = code_block_details
            .iter()
            .flat_map(|detail| {
                if detail.is_fenced {
                    return vec![(detail.start, detail.end)];
                }
                let start_line = line_offsets
                    .partition_point(|&offset| offset <= detail.start)
                    .saturating_sub(1);
                let end_line = line_offsets.partition_point(|&offset| offset < detail.end);
                containers
                    .code_line_spans_in(start_line..end_line)
                    .into_iter()
                    .map(|span| {
                        let start = line_offsets[span.start].max(detail.start);
                        let end = line_offsets
                            .get(span.end)
                            .copied()
                            .unwrap_or(content.len())
                            .min(detail.end);
                        (start, end)
                    })
                    .collect()
            })
            .collect();
        // Front matter is data, not markdown: a `<!--` in a YAML value would
        // otherwise pair with a `-->` in the body and hide everything between
        // them from every rule. `front_matter_end` is the 1-indexed closing
        // delimiter line, so the body starts at the line after it, and a
        // document without front matter starts at byte 0.
        let body_start = line_offsets.get(front_matter_end).copied().unwrap_or(content.len());
        let html_comment_scan = profile_section!(
            "HTML comment ranges",
            profile,
            crate::utils::skip_context::scan_html_comments(
                content,
                &code_span_ranges,
                &comment_code_block_ranges,
                body_start
            )
        );
        let mut html_comment_ranges = html_comment_scan.ranges;
        let unterminated_html_comment = html_comment_scan.unterminated;

        // Pre-compute autodoc block ranges (avoids O(n^2) scaling)
        // Detected for all flavors except AzureDevOps, where `:::` denotes code fences
        // rather than autodoc directives.
        let autodoc_ranges = profile_section!("Autodoc block ranges", profile, {
            crate::utils::mkdocstrings_refs::detect_autodoc_block_ranges(content, flavor)
        });

        // Pre-compute line information AND emphasis spans (without headings/blockquotes yet)
        // Emphasis spans are captured during the same pulldown-cmark parse as list detection
        let skip_ranges = SkipByteRanges {
            html_comment_ranges: &html_comment_ranges,
            autodoc_ranges: &autodoc_ranges,
        };
        let (mut lines, _emphasis_spans) = profile_section!(
            "Basic line info",
            profile,
            line_computation::compute_basic_line_info(
                content,
                &content_lines,
                &line_offsets,
                &code_blocks,
                flavor,
                &skip_ranges,
                front_matter_end,
            )
        );

        // Detect HTML blocks BEFORE heading detection
        profile_section!(
            "HTML blocks",
            profile,
            heading_detection::detect_html_blocks(content, &mut lines)
        );

        // Detect JSX expressions and MDX comments in MDX files
        let (_jsx_expression_ranges, _mdx_comment_ranges): (ByteRanges, ByteRanges) = (Vec::new(), Vec::new());

        // Detect `<div markdown>`-style HTML blocks (grid cards, etc.) regardless of flavor.
        // The `markdown` attribute is an explicit, author-supplied signal; recognizing it
        // in all flavors keeps `rumdl fmt` from mangling Material grid cards when the
        // MkDocs flavor isn't active.
        profile_section!(
            "Markdown-in-HTML blocks",
            profile,
            flavor_detection::detect_markdown_html_blocks(&mut lines, &containers)
        );

        // Detect footnote definitions and correct false code block detection.
        // With ENABLE_FOOTNOTES, pulldown-cmark correctly parses multi-line
        // footnotes, but the code block detector may still mark 4-space-indented
        // footnote continuation lines as indented code blocks.
        profile_section!(
            "Footnote definitions",
            profile,
            detect_footnote_definitions(content, &mut lines, &line_offsets)
        );

        // Filter code_blocks to remove false positives from footnote continuation content.
        // Same pattern as MkDocs/JSX corrections below.
        {
            let mut new_code_blocks = Vec::with_capacity(code_blocks.len());
            for &(start, end) in &code_blocks {
                let start_line = line_offsets
                    .partition_point(|&offset| offset <= start)
                    .saturating_sub(1);
                let end_line = line_offsets.partition_point(|&offset| offset < end).min(lines.len());

                let mut sub_start: Option<usize> = None;
                for (i, &offset) in line_offsets[start_line..end_line]
                    .iter()
                    .enumerate()
                    .map(|(j, o)| (j + start_line, o))
                {
                    let is_real_code = lines.get(i).is_some_and(|info| info.in_code_block);
                    if is_real_code && sub_start.is_none() {
                        let byte_start = if i == start_line { start } else { offset };
                        sub_start = Some(byte_start);
                    } else if !is_real_code && sub_start.is_some() {
                        new_code_blocks.push((sub_start.unwrap(), offset));
                        sub_start = None;
                    }
                }
                if let Some(s) = sub_start {
                    new_code_blocks.push((s, end));
                }
            }
            code_blocks = new_code_blocks;
        }

        // Filter code_blocks to remove false positives from MkDocs admonition/tab content
        // and `<div markdown>` HTML blocks (grid cards).
        // pulldown-cmark treats 4-space-indented content as indented code blocks, but inside
        // these containers this is regular markdown content. detect_mkdocs_line_info and
        // detect_markdown_html_blocks already corrected LineInfo.in_code_block for these lines,
        // but the code_blocks byte ranges are still stale. We split ranges rather than using
        // all-or-nothing removal, so fenced code blocks within the containers are preserved.
        let has_markdown_html = lines.iter().any(|l| l.in_mkdocs_html_markdown);
        if has_markdown_html {
            let mut new_code_blocks = Vec::with_capacity(code_blocks.len());
            for &(start, end) in &code_blocks {
                let start_line = line_offsets
                    .partition_point(|&offset| offset <= start)
                    .saturating_sub(1);
                let end_line = line_offsets.partition_point(|&offset| offset < end).min(lines.len());

                // Walk lines in this range, collecting sub-ranges where in_code_block is true
                let mut sub_start: Option<usize> = None;
                for (i, &offset) in line_offsets[start_line..end_line]
                    .iter()
                    .enumerate()
                    .map(|(j, o)| (j + start_line, o))
                {
                    let is_real_code = lines.get(i).is_some_and(|info| info.in_code_block);
                    if is_real_code && sub_start.is_none() {
                        let byte_start = if i == start_line { start } else { offset };
                        sub_start = Some(byte_start);
                    } else if !is_real_code && sub_start.is_some() {
                        new_code_blocks.push((sub_start.unwrap(), offset));
                        sub_start = None;
                    }
                }
                if let Some(s) = sub_start {
                    new_code_blocks.push((s, end));
                }
            }
            code_blocks = new_code_blocks;
        }

        // Filter code_blocks for MDX JSX blocks (same pattern as MkDocs above).
        // detect_jsx_blocks already corrected LineInfo.in_code_block for indented content
        // inside JSX component blocks, but code_blocks byte ranges need updating too.

        // Detect Azure DevOps colon code fences and extend code_blocks so that
        // all byte-range consumers correctly skip their content.
        let colon_fence_details: Vec<CodeBlockDetail> = Vec::new();

        // Filter code_blocks to remove false positives from MyST content-bearing directives.
        // Same pattern as MkDocs admonition filtering.

        // Layer 1: Sanitize content-derived fields inside kramdown extension blocks
        // so downstream heading detection and collection builders never see them.
        // This must run BEFORE detect_headings_and_blockquotes to prevent headings
        // from being populated inside extension blocks.

        // Detect Obsidian comments (%%...%%) in Obsidian flavor
        let obsidian_comment_scan = flavor_detection::ObsidianCommentScan::default();
        let obsidian_comment_ranges = obsidian_comment_scan.ranges;

        // An Obsidian comment hides the text it wraps, so a `<!--` inside one is
        // not a comment opener. The HTML scan cannot know that yet - detecting
        // Obsidian comments needs its ranges - so the opener it reported is
        // re-resolved here, now that the comments that hide it are known.
        let unterminated_html_comment = crate::utils::skip_context::unterminated_html_comment_outside(
            unterminated_html_comment,
            &obsidian_comment_ranges,
            content,
            &code_span_ranges,
            &comment_code_block_ranges,
            body_start,
        );

        // An unclosed `<!--` that opens an HTML block comments out the rest of
        // that block, so the text below it is not content any rule should judge.
        // Without this the block-structure rules and the comment-aware rules
        // disagree about the same lines: the parser reports no list inside the
        // block, while a bare URL there is still flagged.
        //
        // The opener stays reported either way. This governs what the rest of
        // the linter sees, not whether the missing closer is raised.
        //
        // It waits for the re-resolution above because an opener a `%%` pair
        // hides is not an opener, and giving that one a range would hide the
        // rest of the note from every rule.
        if let Some(range) = unterminated_html_comment.and_then(|opener| {
            crate::utils::skip_context::unterminated_comment_range(opener, &html_blocks)
                .or_else(|| container_comment_range(opener, &containers, &lines, content))
        }) {
            // Every complete comment starts before the unclosed opener, so this
            // keeps the ranges sorted for the binary searches over them.
            html_comment_ranges.push(range);

            // The line flags are computed before the Obsidian comments are
            // known, so they predate this range. Recomputing them through the
            // same helper keeps `is_in_html_comment` and the per-line flag
            // answering alike, which is the agreement this range exists to
            // create.
            for line in &mut lines {
                let text = line.content(content);
                let content_start = line.byte_offset + line.indent;
                let content_end = line.byte_offset + text.trim_end().len();
                line.in_html_comment = crate::utils::skip_context::is_line_entirely_in_html_comment(
                    &html_comment_ranges,
                    content_start,
                    content_end,
                );
            }
        }

        // Run pulldown-cmark parse for links, images, and link byte ranges in a single pass.
        // Link byte ranges are needed for heading detection; links/images are finalized later
        // after code_spans are available.
        let pulldown_result = profile_section!(
            "Links, images & link ranges",
            profile,
            link_parser::parse_links_images_pulldown(content, &lines, &code_blocks, flavor, &html_comment_ranges)
        );

        heading_detection::mark_parser_html_blocks(content, &mut lines, &html_blocks, flavor);

        // Now detect headings and blockquotes
        let mut blockquote_headings = profile_section!(
            "Headings & blockquotes",
            profile,
            heading_detection::detect_headings_and_blockquotes(
                &content_lines,
                &mut lines,
                flavor,
                &html_comment_ranges,
                &html_blocks,
                &code_blocks,
                &code_span_ranges,
                &pulldown_result.link_byte_ranges,
                front_matter_end,
                None,
            )
        );

        // Clear headings that were detected inside kramdown extension blocks
        for _line in &mut lines {}
        for (_line, _heading) in lines.iter().zip(&mut blockquote_headings) {}

        // A run of `-`, `*` or `_` is a thematic break only because of the block it
        // sits in, and that block is known only now: the passes above are what mark
        // an HTML comment, an HTML block, a math block, an MDX or Obsidian comment,
        // and the colon fences a flavor reads as code. The flag was computed from the
        // line text before any of them ran, so it is settled here against the answers
        // they produced, the way the kramdown sanitization above settles its own.
        //
        // Left alone deliberately: containers whose body IS markdown (Pandoc divs,
        // MkDocs admonitions and tabs, PyMdown blocks, MyST directives) render a
        // thematic break written in them.
        for line in &mut lines {
            if line.is_horizontal_rule
                && (line.in_code_block || line.in_html_block || line.in_html_comment || line.in_math_block)
            {
                line.is_horizontal_rule = false;
            }
        }

        // Parse code spans early so we can exclude them from link/image parsing
        let code_spans = profile_section!(
            "Code spans",
            profile,
            element_parsers::build_code_spans_from_ranges(content, &lines, &code_span_ranges)
        );

        // Supplement code spans for MkDocs container content that pulldown-cmark missed.
        // pulldown-cmark treats 4-space-indented MkDocs content as indented code blocks,
        // so backtick code spans within admonitions/tabs/markdown HTML are invisible to it.

        // Supplement code spans for MDX JSX component body content that pulldown-cmark missed.
        // pulldown-cmark treats JSX component opening tags (e.g. `<ParamField>`) as HTML block
        // starters, so backtick code spans within component bodies are invisible to the initial
        // parse.

        // Mark lines that are continuations of multi-line code spans
        // This is needed for parse_list_blocks to correctly handle list items with multi-line code spans
        for span in &code_spans {
            if span.end_line > span.line {
                // Mark lines after the first line as continuations
                for line_num in (span.line + 1)..=span.end_line {
                    if let Some(line_info) = lines.get_mut(line_num - 1) {
                        line_info.in_code_span_continuation = true;
                    }
                }
            }
        }

        // Finalize links and images: filter by code_spans and run regex fallbacks
        let (links, images, broken_links, _footnote_refs) = profile_section!(
            "Links & images finalize",
            profile,
            link_parser::finalize_links_and_images(
                content,
                &lines,
                flavor,
                &link_parser::LinkExclusions {
                    code_blocks: &code_blocks,
                    code_spans: &code_spans,
                    html_comment_ranges: &html_comment_ranges,
                },
                pulldown_result,
            )
        );

        let reference_defs = profile_section!("Reference defs", profile, {
            link_parser::parse_reference_defs(content, &lines)
        });

        let list_blocks = profile_section!("List blocks", profile, list_blocks::parse_list_blocks(content, &lines));

        // Compute character frequency for fast content analysis
        let char_frequency = profile_section!(
            "Char frequency",
            profile,
            line_computation::compute_char_frequency(content)
        );

        // Pre-compute table blocks for rules that need them (MD013, MD055, MD056, MD058, MD060)
        let table_blocks = profile_section!(
            "Table blocks",
            profile,
            crate::utils::table_utils::TableUtils::find_table_blocks_with_code_info(
                content,
                &code_blocks,
                &code_spans,
                &html_comment_ranges,
                flavor,
            )
        );

        // Layer 2: Filter pre-computed collections to exclude items inside kramdown extension blocks.
        // Rules that iterate these collections automatically skip kramdown content.
        let links = links
            .into_iter()
            .filter(|link| !lines.get(link.line - 1).is_some_and(|_l| false))
            .collect::<Vec<_>>();
        let images = images
            .into_iter()
            .filter(|img| !lines.get(img.line - 1).is_some_and(|_l| false))
            .collect::<Vec<_>>();
        let _broken_links = broken_links
            .into_iter()
            .filter(|bl| {
                // BrokenLinkInfo has span but no line field; find line from byte offset
                let line_idx = line_offsets
                    .partition_point(|&offset| offset <= bl.span.start)
                    .saturating_sub(1);
                !lines.get(line_idx).is_some_and(|_l| false)
            })
            .collect::<Vec<_>>();
        let reference_defs = reference_defs
            .into_iter()
            .filter(|def| !lines.get(def.line - 1).is_some_and(|_l| false))
            .collect::<Vec<_>>();
        let list_blocks = list_blocks
            .into_iter()
            .filter(|block| !lines.get(block.start_line - 1).is_some_and(|_l| false))
            .collect::<Vec<_>>();
        let table_blocks = table_blocks
            .into_iter()
            .filter(|block| {
                // TableBlock.start_line is 0-indexed
                !lines.get(block.start_line).is_some_and(|_l| false)
            })
            .collect::<Vec<_>>();

        // Mark lines covered by a list or table block so is_in_list_block /
        // is_in_table_block are O(1) reads (mirrors in_html_block) instead of
        // scanning the whole block vector on every call.
        for block in &list_blocks {
            // ListBlock line numbers are 1-indexed.
            for line_num in block.start_line..=block.end_line {
                if let Some(li) = lines.get_mut(line_num - 1) {
                    li.in_list_block = true;
                }
            }
        }
        for block in &table_blocks {
            // TableBlock line numbers are 0-indexed.
            for idx in block.start_line..=block.end_line {
                if let Some(li) = lines.get_mut(idx) {
                    li.in_table_block = true;
                }
            }
        }

        // Reuse already-computed line_offsets and code_blocks instead of re-detecting
        let line_index = profile_section!(
            "Line index",
            profile,
            crate::utils::range_utils::LineIndex::with_line_starts_and_code_blocks(
                content,
                line_offsets.clone(),
                &code_blocks,
            )
        );

        // Pre-compute Hugo/Quarto shortcode ranges ({{< ... >}} and {{% ... %}})
        let _shortcode_ranges = profile_section!("Shortcode ranges", profile, {
            use crate::utils::regex_cache::HUGO_SHORTCODE_REGEX;
            let mut ranges = Vec::new();
            for mat in HUGO_SHORTCODE_REGEX.find_iter(content) {
                ranges.push((mat.start(), mat.end()));
            }
            ranges
        });

        Self {
            content,
            content_lines,
            line_offsets,
            code_blocks,
            code_block_details,
            strong_spans,
            line_to_list,
            list_start_values,
            definition_lists,
            commonmark_ordered_lists_cache: OnceLock::new(),
            lines,
            blockquote_headings,
            links,
            images,
            reference_defs,
            code_spans_cache: OnceLock::from(Arc::new(code_spans)),
            math_spans_cache: OnceLock::new(), // Lazy-loaded on first access
            bracket_math_cache: OnceLock::new(),
            math_byte_ranges_cache: OnceLock::new(), // Lazy-loaded on first access
            list_blocks,
            char_frequency,
            html_tags_cache: OnceLock::new(),
            jsx_component_tags_cache: OnceLock::new(),
            html_comment_ranges,
            table_blocks,
            line_index,
            flavor,
            colon_fence_details,
            obsidian_comment_ranges,
            lazy_cont_lines_cache: OnceLock::new(),
            front_matter_end,
        }
    }

    /// The 1-indexed line number where front matter ends (the closing
    /// delimiter line), or 0 when the document has no front matter.
    /// Computed once in `new()`; rules must use this instead of re-scanning
    /// the content with `FrontMatterUtils`.
    pub(crate) fn front_matter_end_line(&self) -> usize {
        self.front_matter_end
    }

    /// Binary search for whether `pos` falls inside any range in a sorted, non-overlapping
    /// slice of `(start, end)` byte ranges. O(log n) instead of O(n).
    #[inline]
    fn binary_search_ranges(ranges: &[(usize, usize)], pos: usize) -> bool {
        // Find the rightmost range whose start <= pos
        let idx = ranges.partition_point(|&(start, _)| start <= pos);
        // If idx == 0, no range starts at or before pos
        idx > 0 && pos < ranges[idx - 1].1
    }

    /// Check if `pos` is inside any link byte range. O(log n).
    pub(crate) fn is_in_link(&self, pos: usize) -> bool {
        self.link_containing(pos).is_some() || self.image_containing(pos).is_some() || self.is_in_reference_def(pos)
    }

    /// Azure DevOps colon code fences (`:::lang … :::`), each with its byte range
    /// and the opener's info string. These are detected outside the CommonMark
    /// parse, so they never appear in `code_block_details`. Empty for all other
    /// flavors.
    pub(crate) fn colon_fence_details(&self) -> &[CodeBlockDetail] {
        &self.colon_fence_details
    }

    /// Get pre-split content lines, avoiding repeated `content.lines().collect()` allocations.
    ///
    /// Lines are 0-indexed (line 0 corresponds to line number 1 in the document).
    pub fn raw_lines(&self) -> &[&'a str] {
        &self.content_lines
    }

    /// Get code spans - computed lazily on first access
    pub(crate) fn code_spans(&self) -> Arc<Vec<CodeSpan>> {
        Arc::clone(
            self.code_spans_cache
                .get_or_init(|| Arc::new(element_parsers::parse_code_spans(self.content, &self.lines))),
        )
    }

    /// Math byte ranges (`$...$` inline and `$$...$$` display), computed once and
    /// cached. Used by `is_in_math_context`; without the cache that helper
    /// rescanned the whole document on every call.
    pub(crate) fn math_byte_ranges(&self) -> &[(usize, usize)] {
        self.math_byte_ranges_cache
            .get_or_init(|| crate::utils::skip_context::math_byte_ranges(self.content))
    }

    /// Get math spans - computed lazily on first access
    pub(crate) fn math_spans(&self) -> Arc<Vec<MathSpan>> {
        Arc::clone(
            self.math_spans_cache
                .get_or_init(|| Arc::new(element_parsers::parse_math_spans(self.content, &self.lines))),
        )
    }

    /// Standalone TeX bracket display math, read only by MD013 when opted in.
    pub(crate) fn bracket_display_math_lines(&self) -> &bracket_math::BracketDisplayMathLines {
        self.bracket_math_cache
            .get_or_init(|| bracket_math::parse(self.content, &self.lines, &self.code_spans(), &self.list_blocks))
    }

    /// Check if a byte position is inside an Obsidian comment
    ///
    /// Returns false for non-Obsidian flavors.
    pub(crate) fn is_in_obsidian_comment(&self, byte_pos: usize) -> bool {
        Self::binary_search_ranges(&self.obsidian_comment_ranges, byte_pos)
    }

    /// Drop tags that live inside kramdown extension blocks, preserving order.
    fn filter_kramdown_tags(&self, tags: Vec<HtmlTag>) -> Vec<HtmlTag> {
        tags.into_iter()
            .filter(|tag| !self.lines.get(tag.line - 1).is_some_and(|_l| false))
            .collect()
    }

    /// Get HTML tags - computed lazily on first access.
    ///
    /// JSX component tags (e.g. `<Card .../>`) are excluded so HTML-specific rules
    /// keep ignoring them; use [`Self::jsx_component_tags`] to access those. The
    /// single underlying parse populates both caches at once.
    pub(crate) fn html_tags(&self) -> Arc<Vec<HtmlTag>> {
        Arc::clone(self.html_tags_cache.get_or_init(|| {
            let (html_tags, jsx_component_tags) =
                element_parsers::parse_html_tags(self.content, &self.lines, &self.code_blocks, self.flavor);
            // Populate the JSX-component cache from the same parse so it is built once.
            let _ = self
                .jsx_component_tags_cache
                .set(Arc::new(self.filter_kramdown_tags(jsx_component_tags)));
            Arc::new(self.filter_kramdown_tags(html_tags))
        }))
    }

    /// Get lazy continuation lines - computed lazily on first access
    pub(crate) fn lazy_continuation_lines(&self) -> Arc<Vec<LazyContLine>> {
        Arc::clone(self.lazy_cont_lines_cache.get_or_init(|| {
            Arc::new(element_parsers::detect_lazy_continuation_lines(
                self.content,
                &self.lines,
                &self.line_offsets,
            ))
        }))
    }

    /// Map a byte offset to (line, column).
    ///
    /// The column is a 1-indexed *character* offset within the line (rumdl's
    /// diagnostic convention), not a byte offset, so it is correct on lines
    /// containing multi-byte UTF-8 characters.
    pub(crate) fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        match self.line_offsets.binary_search(&offset) {
            Ok(line) => (line + 1, 1),
            Err(line) => {
                let line_start = self.line_offsets.get(line.wrapping_sub(1)).copied().unwrap_or(0);
                // Convert the byte offset within the line to a character column.
                let col = byte_to_char_count(&self.content[line_start..], offset.saturating_sub(line_start));
                (line, col)
            }
        }
    }

    /// Return the byte offset at which a 1-indexed source line starts.
    ///
    /// This is the inverse-facing half of [`Self::offset_to_line_col`]. Keeping
    /// both conversions on the document prevents rules from depending on the
    /// line-index representation or reconstructing it independently.
    pub(crate) fn line_start_byte(&self, line_number: usize) -> Option<usize> {
        self.line_index.get_line_start_byte(line_number)
    }

    /// Return an empty byte range at a 1-indexed line and character column.
    ///
    /// Columns are character offsets, not UTF-8 byte offsets. Positions past
    /// the end of a line clamp to the end of its content; missing lines clamp to
    /// the end of the document.
    pub(crate) fn line_column_byte_range(&self, line_number: usize, column: usize) -> Range<usize> {
        self.line_index.line_col_to_byte_range(line_number, column)
    }

    /// Return a byte range beginning at a 1-indexed line and character column.
    ///
    /// `length` is measured in characters. The result never crosses the line's
    /// content boundary and excludes its line ending.
    pub(crate) fn line_column_byte_range_with_length(
        &self,
        line_number: usize,
        column: usize,
        length: usize,
    ) -> Range<usize> {
        self.line_index
            .line_col_to_byte_range_with_length(line_number, column, length)
    }

    /// Return the byte range of a complete 1-indexed line, including its line
    /// ending when one is present.
    pub(crate) fn whole_line_byte_range(&self, line_number: usize) -> Range<usize> {
        self.line_index.whole_line_range(line_number)
    }

    /// Return the byte range between two 1-indexed character columns on a line.
    ///
    /// The range excludes the line ending and clamps both columns to valid
    /// character boundaries in the line content.
    pub(crate) fn line_text_byte_range(
        &self,
        line_number: usize,
        start_column: usize,
        end_column: usize,
    ) -> Range<usize> {
        self.line_index.line_text_range(line_number, start_column, end_column)
    }

    /// Return the byte range of a 1-indexed line's content, excluding its line
    /// ending.
    pub(crate) fn line_content_byte_range(&self, line_number: usize) -> Range<usize> {
        self.line_index.line_content_range(line_number)
    }

    /// Return the byte range spanning complete 1-indexed lines, inclusive.
    pub(crate) fn line_span_byte_range(&self, start_line: usize, end_line: usize) -> Range<usize> {
        self.line_index.multi_line_range(start_line, end_line)
    }

    /// Check if a position is within a code block or code span. O(log n).
    pub(crate) fn is_in_code_block_or_span(&self, pos: usize) -> bool {
        // Check code blocks first (already uses binary search internally)
        if CodeBlockUtils::is_in_code_block_or_span(&self.code_blocks, pos) {
            return true;
        }

        // Check inline code spans via binary search
        self.is_byte_offset_in_code_span(pos)
    }

    /// Get line information by line number (1-indexed)
    pub fn line_info(&self, line_num: usize) -> Option<&LineInfo> {
        if line_num > 0 {
            self.lines.get(line_num - 1)
        } else {
            None
        }
    }

    /// Parsed links in document order.
    pub fn links(&self) -> &[ParsedLink<'a>] {
        &self.links
    }

    /// Parsed images in document order.
    pub fn images(&self) -> &[ParsedImage<'a>] {
        &self.images
    }

    /// Parsed reference definitions in document order.
    pub fn reference_definitions(&self) -> &[ReferenceDef] {
        &self.reference_defs
    }

    /// Links whose opening delimiter starts on `line_number` (1-indexed).
    pub(crate) fn links_on_line(&self, line_number: usize) -> &[ParsedLink<'a>] {
        let start = self.links.partition_point(|link| link.line < line_number);
        let end = self.links.partition_point(|link| link.line <= line_number);
        &self.links[start..end]
    }

    /// Images whose opening delimiter starts on `line_number` (1-indexed).
    pub(crate) fn images_on_line(&self, line_number: usize) -> &[ParsedImage<'a>] {
        let start = self.images.partition_point(|image| image.line < line_number);
        let end = self.images.partition_point(|image| image.line <= line_number);
        &self.images[start..end]
    }

    /// Find the link that starts at an exact byte offset. O(log n).
    pub(crate) fn link_starting_at(&self, byte_offset: usize) -> Option<&ParsedLink<'a>> {
        self.links
            .binary_search_by_key(&byte_offset, |link| link.byte_offset)
            .ok()
            .map(|index| &self.links[index])
    }

    /// Find the image that starts at an exact byte offset. O(log n).
    pub(crate) fn image_starting_at(&self, byte_offset: usize) -> Option<&ParsedImage<'a>> {
        self.images
            .binary_search_by_key(&byte_offset, |image| image.byte_offset)
            .ok()
            .map(|index| &self.images[index])
    }

    /// Find the parsed link containing `byte_offset`. O(log n).
    pub(crate) fn link_containing(&self, byte_offset: usize) -> Option<&ParsedLink<'a>> {
        let index = self.links.partition_point(|link| link.byte_offset <= byte_offset);
        self.links
            .get(index.checked_sub(1)?)
            .filter(|link| byte_offset < link.byte_end)
    }

    /// Find the parsed image containing `byte_offset`. O(log n).
    pub(crate) fn image_containing(&self, byte_offset: usize) -> Option<&ParsedImage<'a>> {
        let index = self.images.partition_point(|image| image.byte_offset <= byte_offset);
        self.images
            .get(index.checked_sub(1)?)
            .filter(|image| byte_offset < image.byte_end)
    }

    /// Check if a line is part of a list block
    pub(crate) fn is_in_list_block(&self, line_num: usize) -> bool {
        if line_num == 0 || line_num > self.lines.len() {
            return false;
        }
        self.lines[line_num - 1].in_list_block
    }

    /// Check if a line is inside a definition list, as the parser reads it
    ///
    /// Terms count, and so does everything nested in a definition, but the blank
    /// lines between items do not. A colon touching its text (`:warning:`) opens
    /// no definition here, though pulldown-cmark reads one, so its term is free
    /// text even between two items that do count. This is the parser's answer for every flavor,
    /// unlike `LineInfo::in_definition_list`, which only MkDocs populates.
    pub(crate) fn is_in_definition_list(&self, line_num: usize) -> bool {
        let lists = &self.definition_lists.outer;
        let idx = lists.partition_point(|&(start, _)| start <= line_num);
        idx > 0 && line_num <= lists[idx - 1].1
    }

    /// Check if a line is part of a definition list term
    pub(crate) fn is_definition_term(&self, line_num: usize) -> bool {
        let terms = &self.definition_lists.terms;
        let idx = terms.partition_point(|&(_, end)| end < line_num);
        terms.get(idx).is_some_and(|&(start, _)| start <= line_num)
    }

    /// The definition text containing a line, if any
    pub(crate) fn definition_text_at(&self, line_num: usize) -> Option<&DefinitionText> {
        let texts = &self.definition_lists.texts;
        let idx = texts.partition_point(|text| text.end_line < line_num);
        texts.get(idx).filter(|text| text.start_line <= line_num)
    }

    /// Check if a byte offset is within a code span. O(log n).
    #[inline]
    pub(crate) fn is_byte_offset_in_code_span(&self, byte_offset: usize) -> bool {
        let code_spans = self.code_spans();
        let idx = code_spans.partition_point(|span| span.byte_offset <= byte_offset);
        idx > 0 && byte_offset < code_spans[idx - 1].byte_end
    }

    /// Check if a byte position is within a reference definition. O(log n).
    #[inline]
    pub(crate) fn is_in_reference_def(&self, byte_pos: usize) -> bool {
        let idx = self.reference_defs.partition_point(|rd| rd.byte_offset <= byte_pos);
        idx > 0 && byte_pos < self.reference_defs[idx - 1].byte_end
    }

    /// Check if a byte position is within an HTML comment. O(log n).
    #[inline]
    pub(crate) fn is_in_html_comment(&self, byte_pos: usize) -> bool {
        let idx = self.html_comment_ranges.partition_point(|r| r.start <= byte_pos);
        idx > 0 && byte_pos < self.html_comment_ranges[idx - 1].end
    }

    /// Check if a byte position is within an HTML tag (including multiline tags).
    /// Uses the pre-parsed html_tags which correctly handles tags spanning multiple lines. O(log n).
    #[inline]
    pub(crate) fn is_in_html_tag(&self, byte_pos: usize) -> bool {
        let tags = self.html_tags();
        let idx = tags.partition_point(|tag| tag.byte_offset <= byte_pos);
        idx > 0 && byte_pos < tags[idx - 1].byte_end
    }

    /// Check if content has any instances of a specific character (fast)
    pub(crate) fn has_char(&self, ch: char) -> bool {
        match ch {
            '#' => self.char_frequency.hash_count > 0,
            '*' => self.char_frequency.asterisk_count > 0,
            '_' => self.char_frequency.underscore_count > 0,
            '-' => self.char_frequency.hyphen_count > 0,
            '+' => self.char_frequency.plus_count > 0,
            '>' => self.char_frequency.gt_count > 0,
            '|' => self.char_frequency.pipe_count > 0,
            '[' => self.char_frequency.bracket_count > 0,
            '`' => self.char_frequency.backtick_count > 0,
            '<' => self.char_frequency.lt_count > 0,
            '!' => self.char_frequency.exclamation_count > 0,
            '\n' => self.char_frequency.newline_count > 0,
            _ => self.content.contains(ch), // Fallback for other characters
        }
    }

    /// Get count of a specific character (fast)
    pub(crate) fn char_count(&self, ch: char) -> usize {
        match ch {
            '#' => self.char_frequency.hash_count,
            '*' => self.char_frequency.asterisk_count,
            '_' => self.char_frequency.underscore_count,
            '-' => self.char_frequency.hyphen_count,
            '+' => self.char_frequency.plus_count,
            '>' => self.char_frequency.gt_count,
            '|' => self.char_frequency.pipe_count,
            '[' => self.char_frequency.bracket_count,
            '`' => self.char_frequency.backtick_count,
            '<' => self.char_frequency.lt_count,
            '!' => self.char_frequency.exclamation_count,
            '\n' => self.char_frequency.newline_count,
            _ => self.content.matches(ch).count(), // Fallback for other characters
        }
    }

    /// Check if content likely contains headings (fast). A setext underline can
    /// be a single `-` or `=`, so one of either may make a heading.
    pub(crate) fn likely_has_headings(&self) -> bool {
        self.char_frequency.hash_count > 0 || self.char_frequency.hyphen_count > 0 || self.content.contains('=')
    }

    /// Check if content likely contains unordered lists (fast). Only bullet
    /// characters are counted, so an ordered-only document answers false;
    /// rules about ordered lists read `commonmark_ordered_lists` instead.
    pub(crate) fn likely_has_lists(&self) -> bool {
        self.char_frequency.asterisk_count > 0
            || self.char_frequency.hyphen_count > 0
            || self.char_frequency.plus_count > 0
    }

    /// Check if content likely contains emphasis (fast)
    pub(crate) fn likely_has_emphasis(&self) -> bool {
        self.char_frequency.asterisk_count > 1 || self.char_frequency.underscore_count > 1
    }

    /// Check if content likely contains tables (fast)
    pub(crate) fn likely_has_tables(&self) -> bool {
        self.char_frequency.pipe_count > 2
    }

    /// Check if content likely contains blockquotes (fast)
    pub(crate) fn likely_has_blockquotes(&self) -> bool {
        self.char_frequency.gt_count > 0
    }

    /// Check if content likely contains code (fast)
    pub(crate) fn likely_has_code(&self) -> bool {
        self.char_frequency.backtick_count > 0
    }

    /// Get the blockquote prefix for inserting a blank line at the given line index.
    /// Returns the prefix without trailing content (e.g., ">" or ">>").
    /// This is needed because blank lines inside blockquotes must preserve the blockquote structure.
    /// Returns an empty string if the line is not inside a blockquote.
    pub(crate) fn blockquote_prefix_for_blank_line(&self, line_idx: usize) -> String {
        if let Some(line_info) = self.lines.get(line_idx)
            && let Some(ref bq) = line_info.blockquote
        {
            bq.prefix.trim_end().to_string()
        } else {
            String::new()
        }
    }

    /// Find the line index for a given byte offset using binary search.
    /// Returns (line_index, line_number, column) where:
    /// - line_index is the 0-based index in the lines array
    /// - line_number is the 1-based line number
    /// - column is the 0-based *character* offset within that line
    ///
    /// The column is a character offset rather than a byte offset so that the
    /// `start_col`/`end_col` it feeds into match rumdl's diagnostic convention
    /// (columns are character positions). On lines with multi-byte UTF-8
    /// characters the two differ; reporting bytes would mis-position highlights.
    #[inline]
    fn find_line_for_offset(lines: &[LineInfo], content: &str, byte_offset: usize) -> (usize, usize, usize) {
        // Binary search to find the line containing this byte offset
        let idx = match lines.binary_search_by(|line| {
            if byte_offset < line.byte_offset {
                std::cmp::Ordering::Greater
            } else if byte_offset > line.byte_offset + line.byte_len {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        }) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };

        let line = &lines[idx];
        let line_num = idx + 1;
        let byte_col = byte_offset.saturating_sub(line.byte_offset);
        // Convert the byte offset within the line to a 0-based character column.
        // `byte_to_char_count` returns a 1-based value, so subtract 1.
        let col = byte_to_char_count(line.content(content), byte_col) - 1;

        (idx, line_num, col)
    }

    /// Check if a byte offset is within a code span using binary search
    #[inline]
    fn is_offset_in_code_span(code_spans: &[CodeSpan], offset: usize) -> bool {
        // Since spans are sorted by byte_offset, use partition_point for binary search
        let idx = code_spans.partition_point(|span| span.byte_offset <= offset);

        // Check the span that starts at or before our offset
        if idx > 0 {
            let span = &code_spans[idx - 1];
            if offset >= span.byte_offset && offset < span.byte_end {
                return true;
            }
        }

        false
    }

    /// Get an iterator over valid headings (skipping invalid ones like `#NoSpace`)
    ///
    /// Valid headings have proper spacing after the `#` markers (or are level > 1).
    /// This is the standard iterator for rules that need to process headings.
    ///
    /// # Examples
    ///
    /// ```
    /// use rumdl_lib::lint_context::LintContext;
    /// use rumdl_lib::config::MarkdownFlavor;
    ///
    /// let content = "# Valid Heading\n#NoSpace\n## Another Valid";
    /// let ctx = LintContext::new(content, MarkdownFlavor::Standard, None);
    ///
    /// for heading in ctx.valid_headings() {
    ///     println!("Line {}: {} (level {})", heading.line_num, heading.heading.text, heading.heading.level);
    /// }
    /// // Only prints valid headings, skips `#NoSpace`
    /// ```
    #[must_use]
    pub(crate) fn valid_headings(&self) -> ValidHeadingsIter<'_> {
        ValidHeadingsIter::new(&self.lines)
    }

    /// Check if the document contains any CommonMark headings
    ///
    /// A line like `#NoSpace` is paragraph text and does not count.
    #[must_use]
    pub(crate) fn has_valid_headings(&self) -> bool {
        self.lines.iter().any(|line| line.heading.is_some())
    }

    /// Return the parsed list item on a 1-indexed source line, if any.
    #[must_use]
    pub(crate) fn list_item_on_line(&self, line_num: usize) -> Option<ParsedListItem<'_>> {
        let line_info = self.lines.get(line_num.checked_sub(1)?)?;
        Some(ParsedListItem::new(
            line_num,
            line_info.list_item.as_deref()?,
            line_info,
        ))
    }

    /// Borrow the document's parsed list blocks and their item iterators.
    #[must_use]
    pub(crate) fn parsed_list_blocks(&self) -> ParsedListBlocks<'_> {
        ParsedListBlocks::new(&self.list_blocks, &self.lines)
    }

    /// The item lines of `block` grouped into the lists they form, one group
    /// per list as CommonMark nests them, in source order, so siblings can be
    /// compared without the nested items that sit between them.
    #[must_use]
    pub(crate) fn list_block_item_groups(&self, block: &ListBlock) -> Vec<Vec<usize>> {
        list_blocks::item_lines_by_list(self.content, &self.lines, block)
    }

    /// Whether the document contains any parsed list items.
    #[must_use]
    pub(crate) fn has_list_items(&self) -> bool {
        self.lines.iter().any(|line| line.list_item.is_some())
    }

    /// Whether the document contains any parsed unordered-list items.
    #[must_use]
    pub(crate) fn has_unordered_list_items(&self) -> bool {
        self.lines
            .iter()
            .any(|line| line.list_item.as_ref().is_some_and(|item| !item.is_ordered))
    }

    /// Borrow ordered lists using the membership and start values determined by CommonMark.
    #[must_use]
    pub(crate) fn commonmark_ordered_lists(&self) -> CommonMarkOrderedLists<'_> {
        let lists = self
            .commonmark_ordered_lists_cache
            .get_or_init(|| build_commonmark_ordered_lists(&self.lines, &self.line_to_list, &self.list_start_values));
        CommonMarkOrderedLists::new(lists, &self.lines)
    }

    /// Return the parsed heading on a 1-indexed source line, if any.
    #[must_use]
    pub(crate) fn heading_on_line(&self, line_num: usize) -> Option<ParsedHeading<'_>> {
        let idx = line_num.checked_sub(1)?;
        let line_info = self.lines.get(idx)?;
        let (heading, _blockquote_depth) = match line_info.heading.as_deref() {
            Some(heading) => (heading, 0),
            None => (
                self.blockquote_headings.get(idx)?.as_deref()?,
                line_info.blockquote.as_ref().map_or(0, |bq| bq.nesting_level),
            ),
        };
        Some(ParsedHeading {
            heading,
            text_line_infos: &self.lines[idx + 1 - heading.text_lines..=idx],
        })
    }
}

/// The range an unclosed `<!--` hides when it opens a block the parser missed.
///
/// A MkDocs admonition or a `<div markdown>` body is rendered as markdown in its
/// own right, so a `<!--` starting one of its lines opens an HTML block there
/// just as it would at the top level. The parser has no notion of either
/// container, reads the body as indented code or as a lazy paragraph
/// continuation, and so reports no block for the opener to run to the end of.
///
/// The block ends where the container's body ends, which is what CommonMark
/// gives an unclosed comment in any other container. An opener that is not the
/// first thing on its line is inline HTML and opens nothing, here as anywhere.
fn container_comment_range(
    opener: usize,
    containers: &flavor_detection::ContainerLines,
    lines: &[types::LineInfo],
    content: &str,
) -> Option<crate::utils::skip_context::ByteRange> {
    let line_index = lines
        .partition_point(|line| line.byte_offset <= opener)
        .checked_sub(1)?;
    let line = lines.get(line_index)?;
    if line.byte_offset + line.indent != opener {
        return None;
    }
    if !containers.is_container_body(line_index) {
        return None;
    }
    let end_line = lines.get(containers.body_end_line(line_index)?)?;
    Some(crate::utils::skip_context::ByteRange {
        start: opener,
        end: (end_line.byte_offset + end_line.byte_len).min(content.len()),
    })
}

/// Parser-detected definition lists, converted from byte ranges to lines
#[derive(Debug)]
struct DefinitionListLines {
    /// Items of outermost lists as inclusive 1-indexed line ranges, sorted and
    /// disjoint, nested items merged into the item holding them
    outer: Vec<(usize, usize)>,
    /// Terms as inclusive 1-indexed line ranges, sorted and disjoint
    terms: Vec<(usize, usize)>,
    /// Definition texts, sorted and disjoint
    texts: Vec<DefinitionText>,
}

impl DefinitionListLines {
    fn new(
        line_offsets: &[usize],
        items: &[(usize, usize)],
        terms: &[(usize, usize)],
        texts: &[crate::utils::code_block_utils::DefinitionTextDetail],
    ) -> Self {
        // A range's end is exclusive and can sit just past the newline ending
        // its last line, so the last line is the one holding the byte before it.
        let line_of = |byte: usize| line_offsets.partition_point(|&offset| offset <= byte).max(1);
        let last_line_of = |start: usize, end: usize| line_of(end.saturating_sub(1).max(start));

        // Items arrive in document order, a nested item after the item holding
        // it, so each merges into the last range when it starts inside it.
        let mut outer: Vec<(usize, usize)> = Vec::new();
        for &(start, end) in items {
            let (start_line, end_line) = (line_of(start), last_line_of(start, end));
            match outer.last_mut() {
                Some(last) if start_line <= last.1 => last.1 = last.1.max(end_line),
                _ => outer.push((start_line, end_line)),
            }
        }
        let terms = terms
            .iter()
            .map(|&(start, end)| (line_of(start), last_line_of(start, end)))
            .collect();

        let texts = texts
            .iter()
            .map(|text| {
                let start_line = line_of(text.start);
                let line_start = line_offsets[start_line - 1];
                let on_marker_line = line_of(text.definition_start) == start_line;
                DefinitionText {
                    start_line,
                    end_line: last_line_of(text.start, text.end),
                    marker_prefix_len: on_marker_line.then(|| text.start - line_start),
                }
            })
            .collect();

        Self { outer, terms, texts }
    }
}

/// Detect footnote definitions and mark their continuation lines.
///
/// Uses pulldown-cmark to find footnote definition ranges and fenced code
/// blocks within them, then:
/// 1. Sets `in_footnote_definition = true` on all lines within
/// 2. Clears `in_code_block = false` on continuation lines that were
///    misidentified as indented code blocks (but preserves real fenced
///    code blocks within footnotes)
fn detect_footnote_definitions(content: &str, lines: &mut [types::LineInfo], line_offsets: &[usize]) {
    use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

    let options = crate::utils::rumdl_parser_options();
    let parser = Parser::new_ext(content, options).into_offset_iter();

    // Collect footnote ranges and fenced code block ranges within them
    let mut footnote_ranges: Vec<(usize, usize)> = Vec::new();
    let mut fenced_code_ranges: Vec<(usize, usize)> = Vec::new();
    let mut in_footnote = false;

    for (event, range) in parser {
        match event {
            Event::Start(Tag::FootnoteDefinition(_)) => {
                in_footnote = true;
                footnote_ranges.push((range.start, range.end));
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                in_footnote = false;
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))) if in_footnote => {
                fenced_code_ranges.push((range.start, range.end));
            }
            _ => {}
        }
    }

    let byte_to_line = |byte_offset: usize| -> usize {
        line_offsets
            .partition_point(|&offset| offset <= byte_offset)
            .saturating_sub(1)
    };

    // Mark footnote definition lines
    for &(start, end) in &footnote_ranges {
        let start_line = byte_to_line(start);
        let end_line = line_offsets.partition_point(|&offset| offset < end).min(lines.len());

        for line in &mut lines[start_line..end_line] {
            line.in_footnote_definition = true;
            line.in_code_block = false;
        }
    }

    // Restore in_code_block for fenced code blocks within footnotes
    for &(start, end) in &fenced_code_ranges {
        let start_line = byte_to_line(start);
        let end_line = line_offsets.partition_point(|&offset| offset < end).min(lines.len());

        for line in &mut lines[start_line..end_line] {
            line.in_code_block = true;
        }
    }
}
