use crate::lint_context::is_horizontal_rule_content;
/// Rule MD022: Headings should be surrounded by blank lines
///
/// See [docs/md022.md](../../docs/md022.md) for full documentation, configuration, and examples.
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::blank_lines::is_blank_or_comment_only;
use crate::utils::mkdocs_attr_list::is_block_attribute_line;
use crate::utils::pandoc;
use crate::utils::range_utils::calculate_heading_range;

pub(crate) mod md022_config;
use md022_config::MD022Config;

/// Whether a trimmed line begins with a list marker: `-`, `*`, or `+`, or one or more
/// digits followed by `.` or `)`, in each case followed by a space or the end of the line.
///
/// This is a parse-stable substitute for the per-line `list_item` flag when deciding whether
/// a heading is followed by a list. The flag depends on surrounding block context and can flip
/// when a blank line is inserted above the heading, which would make the blank-below fix
/// non-idempotent; the syntactic shape of the following line does not change.
///
/// A spaced thematic break (`* * *`, `- - -`, `- --`) opens with a marker and a space, so it has
/// to be excluded explicitly: the parser reads it as a thematic break rather than a list, and a
/// heading above it needs the same blank line that `***` and `_ _ _` already get.
/// `is_horizontal_rule_content` is what the per-line `is_horizontal_rule` flag is computed from,
/// so the exclusion matches how the rest of rumdl recognizes a break, and it reads the line text
/// alone, which keeps it as parse-stable as the rest of this test.
fn starts_with_list_marker(trimmed: &str) -> bool {
    if is_horizontal_rule_content(trimmed) {
        return false;
    }
    let bytes = trimmed.as_bytes();
    match bytes.first() {
        Some(b'-' | b'*' | b'+') => matches!(bytes.get(1), None | Some(b' ')),
        Some(b'0'..=b'9') => {
            let mut i = 0;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            matches!(bytes.get(i), Some(b'.' | b')')) && matches!(bytes.get(i + 1), None | Some(b' '))
        }
        _ => false,
    }
}

/// Index of the line a heading's text starts on. A heading is recorded on the
/// last line of its text, which for a setext heading is the last line of the
/// paragraph its underline ends.
fn first_text_idx(heading_idx: usize, heading: &crate::lint_context::HeadingInfo) -> usize {
    heading_idx + 1 - heading.text_lines
}

/// Index of the line the document's first heading starts on, or `None` when
/// content the heading cannot open the document over comes first.
///
/// Blank lines, HTML comments, kramdown preamble lines and, in a Pandoc-compatible
/// flavor, div markers are transparent: they do not make the heading below them a
/// heading with content above it. So are the earlier text lines of a setext
/// heading, which are the heading itself rather than content above it.
fn heading_at_start_idx(ctx: &crate::lint_context::LintContext, is_pandoc: bool) -> Option<usize> {
    let mut found_non_transparent = false;
    ctx.lines.iter().enumerate().find_map(|(i, line)| {
        match line.heading.as_deref() {
            Some(heading) if !found_non_transparent => Some(first_text_idx(i, heading)),
            _ => {
                if !line.is_blank && !line.in_html_comment && !line.is_setext_heading_text {
                    let trimmed = line.content(ctx.content).trim();
                    // Check for single-line HTML comments too
                    if is_blank_or_comment_only(trimmed) {
                        // Transparent - HTML comment
                    } else if is_pandoc && (pandoc::is_div_open(trimmed) || pandoc::is_div_close(trimmed)) {
                        // Transparent - Pandoc/Quarto div marker in Pandoc-compatible flavor
                    } else {
                        found_non_transparent = true;
                    }
                }
                None
            }
        }
    })
}

///
/// This rule enforces consistent spacing around headings to improve document readability
/// and visual structure.
///
/// ## Purpose
///
/// - **Readability**: Blank lines create visual separation, making headings stand out
/// - **Parsing**: Many Markdown parsers require blank lines around headings for proper rendering
/// - **Consistency**: Creates a uniform document style throughout
/// - **Focus**: Helps readers identify and focus on section transitions
///
/// ## Configuration Options
///
/// The rule supports customizing the number of blank lines required:
///
/// ```yaml
/// MD022:
///   lines_above: 1  # Number of blank lines required above headings (default: 1)
///   lines_below: 1  # Number of blank lines required below headings (default: 1)
/// ```
///
/// ## Examples
///
/// ### Correct (with default configuration)
///
/// ```markdown
/// Regular paragraph text.
///
/// # Heading 1
///
/// Content under heading 1.
///
/// ## Heading 2
///
/// More content here.
/// ```
///
/// ### Incorrect (with default configuration)
///
/// ```markdown
/// Regular paragraph text.
/// # Heading 1
/// Content under heading 1.
/// ## Heading 2
/// More content here.
/// ```
///
/// ## Special Cases
///
/// This rule handles several special cases:
///
/// - **First Heading**: The first heading in a document doesn't require blank lines above
///   if it appears at the very start of the document
/// - **Front Matter**: YAML front matter is detected and skipped
/// - **Code Blocks**: Headings inside code blocks are ignored
/// - **Document Start/End**: Adjusts requirements for headings at the beginning or end of a document
///
/// ## Fix Behavior
///
/// When applying automatic fixes, this rule:
/// - Adds the required number of blank lines above headings
/// - Adds the required number of blank lines below headings
/// - Preserves document structure and existing content
///
/// ## Performance Considerations
///
/// The rule is optimized for performance with:
/// - Efficient line counting algorithms
/// - Proper handling of front matter
/// - Smart code block detection
///
#[derive(Clone, Default)]
pub struct MD022BlanksAroundHeadings {
    config: MD022Config,
}

impl MD022BlanksAroundHeadings {
    /// Fix a document by adding appropriate blank lines around headings
    fn fix_content(&self, ctx: &crate::lint_context::LintContext) -> String {
        // fix() runs on LF text: the CLI and DocumentRun::fix normalise the
        // document before fixing and restore its own line ending afterwards.
        let line_ending = "\n";
        let had_trailing_newline = ctx.content.ends_with('\n');
        let mut result = Vec::new();
        let mut skip_count: usize = 0;

        let heading_at_start_idx = heading_at_start_idx(ctx, false);

        for (i, line_info) in ctx.lines.iter().enumerate() {
            if skip_count > 0 {
                skip_count -= 1;
                continue;
            }
            let line = line_info.content(ctx.content);

            if line_info.in_code_block {
                result.push(line.to_string());
                continue;
            }

            // Check if it's a heading. A setext heading is recorded on the last
            // line of its text, so a span reached at its first line looks ahead
            // for the line carrying it.
            let heading_idx = if line_info.heading.is_some() {
                Some(i)
            } else if line_info.is_setext_heading_text {
                ctx.lines[i..]
                    .iter()
                    .position(|candidate| candidate.heading.is_some())
                    .map(|offset| i + offset)
            } else {
                None
            };

            if let Some(heading_idx) = heading_idx {
                let heading = ctx.lines[heading_idx].heading.as_deref().unwrap();

                // The lines the heading occupies: the text it spans, and the
                // underline below a setext heading
                let heading_end_idx = if matches!(
                    heading.style,
                    crate::lint_context::HeadingStyle::Setext1 | crate::lint_context::HeadingStyle::Setext2
                ) && heading_idx + 1 < ctx.lines.len()
                {
                    heading_idx + 1
                } else {
                    heading_idx
                };

                // This is a heading line (ATX or Setext content)
                let is_first_heading = Some(i) == heading_at_start_idx;
                let heading_level = heading.level as usize;

                // Count existing blank lines above in the result, skipping HTML comments, IAL, and Quarto div markers
                let mut blank_lines_above = 0;
                let mut check_idx = result.len();
                while check_idx > 0 {
                    let prev_line = &result[check_idx - 1];
                    let trimmed = prev_line.trim();
                    if is_blank_or_comment_only(prev_line) {
                        // A line contributing nothing but comments counts as blank (#866)
                        blank_lines_above += 1;
                        check_idx -= 1;
                    } else if is_block_attribute_line(trimmed, ctx.flavor) {
                        // Skip kramdown IAL - they are attached to headings and transparent
                        check_idx -= 1;
                    } else {
                        break;
                    }
                }

                // Determine how many blank lines we need above
                let requirement_above = self.config.lines_above.get_for_level(heading_level);
                let needed_blanks_above = if is_first_heading && self.config.allowed_at_start {
                    0
                } else {
                    requirement_above.required_count().unwrap_or(0)
                };

                // Add missing blank lines above if needed
                while blank_lines_above < needed_blanks_above {
                    result.push(String::new());
                    blank_lines_above += 1;
                }

                // Add the heading's own lines, underline included, so nothing
                // is inserted between two lines of one heading
                for idx in i..=heading_end_idx {
                    result.push(ctx.lines[idx].content(ctx.content).to_string());
                }
                skip_count += heading_end_idx - i; // Skip them in the main loop

                // Determine base index for checking lines below
                let mut effective_end_idx = heading_end_idx;

                // Add any kramdown IAL lines that immediately follow the heading
                // These are part of the heading element and should not be separated
                let mut ial_count = 0;
                while effective_end_idx + 1 < ctx.lines.len() {
                    let next_line = &ctx.lines[effective_end_idx + 1];
                    let next_trimmed = next_line.content(ctx.content).trim();
                    if is_block_attribute_line(next_trimmed, ctx.flavor) {
                        result.push(next_trimmed.to_string());
                        effective_end_idx += 1;
                        ial_count += 1;
                    } else {
                        break;
                    }
                }

                // Now check blank lines below the heading (including underline and IAL)
                let mut blank_lines_below = 0;
                let mut next_content_line_idx = None;
                for j in (effective_end_idx + 1)..ctx.lines.len() {
                    if ctx.lines[j].is_blank || is_blank_or_comment_only(ctx.lines[j].content(ctx.content)) {
                        blank_lines_below += 1;
                    } else {
                        next_content_line_idx = Some(j);
                        break;
                    }
                }

                // Check if the next non-blank line is special (code fence or list item)
                let next_is_special = if let Some(idx) = next_content_line_idx {
                    let next_line = &ctx.lines[idx];
                    let trimmed = next_line.content(ctx.content).trim();
                    next_line.list_item.is_some()
                        || starts_with_list_marker(trimmed)
                        || ((trimmed.starts_with("```") || trimmed.starts_with("~~~"))
                            && (trimmed.len() == 3
                                || (trimmed.len() > 3
                                    && trimmed
                                        .chars()
                                        .nth(3)
                                        .is_some_and(|c| c.is_whitespace() || c.is_alphabetic()))))
                } else {
                    false
                };

                // Add missing blank lines below if needed
                let requirement_below = self.config.lines_below.get_for_level(heading_level);
                // A heading that ends the document has nothing to separate
                // from, and `check` asks for no blank line below it.
                let needed_blanks_below = if next_is_special || next_content_line_idx.is_none() {
                    0
                } else {
                    requirement_below.required_count().unwrap_or(0)
                };
                if blank_lines_below < needed_blanks_below {
                    for _ in 0..(needed_blanks_below - blank_lines_below) {
                        result.push(String::new());
                    }
                }

                // Skip the IAL lines in the main loop since we already added them
                skip_count += ial_count;
            } else {
                // Regular line - just add it
                result.push(line.to_string());
            }
        }

        // `ctx.lines` holds no entry after the final newline, so the file's
        // own final newline is restored after the join, even when the last
        // line is blank.
        let mut joined = result.join(line_ending);
        if had_trailing_newline {
            joined.push_str(line_ending);
        }
        joined
    }
}

impl Rule for MD022BlanksAroundHeadings {
    fn name(&self) -> &'static str {
        "MD022"
    }

    fn description(&self) -> &'static str {
        "Headings should be surrounded by blank lines"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut result = Vec::new();

        // Skip if empty document
        if ctx.lines.is_empty() {
            return Ok(result);
        }

        // Fix replacements are written for LF text; where warnings are collected,
        // conform_fix_line_endings rewrites them for a CRLF document.
        let line_ending = "\n";

        let heading_at_start_idx = heading_at_start_idx(ctx, false);

        // Collect all headings first to batch process
        let mut heading_violations = Vec::new();
        let mut processed_headings = std::collections::HashSet::new();

        for (line_num, line_info) in ctx.lines.iter().enumerate() {
            // Skip if already processed or not a heading
            if processed_headings.contains(&line_num) || line_info.heading.is_none() {
                continue;
            }

            // Skip headings inside PyMdown blocks (/// ... ///) - MkDocs flavor only

            let heading = line_info.heading.as_ref().unwrap();

            let heading_level = heading.level as usize;

            // Note: Setext underline lines have heading=None, so they're already
            // skipped by the check at line 351. No additional check needed here.

            processed_headings.insert(line_num);

            // What sits above the heading sits above the line its text starts
            // on, which for a setext heading is the first line of the paragraph
            // its underline ends
            let first_idx = first_text_idx(line_num, heading);

            // Check if this heading is at document start
            let is_first_heading = Some(first_idx) == heading_at_start_idx;

            // Get configured blank line requirements for this heading level
            let required_above_count = self.config.lines_above.get_for_level(heading_level).required_count();
            let required_below_count = self.config.lines_below.get_for_level(heading_level).required_count();

            // Count blank lines above if needed
            let should_check_above = required_above_count.is_some()
                && first_idx > 0
                && (!is_first_heading || !self.config.allowed_at_start)
                && !(false);
            if should_check_above {
                let mut blank_lines_above = 0;
                let mut hit_frontmatter_end = false;
                for j in (0..first_idx).rev() {
                    let line_content = ctx.lines[j].content(ctx.content);
                    let trimmed = line_content.trim();
                    if ctx.lines[j].is_blank || is_blank_or_comment_only(line_content) {
                        // A line contributing nothing but comments separates the blocks
                        // around it the way an empty one does (#866)
                        blank_lines_above += 1;
                    } else if ctx.lines[j].in_html_comment {
                        // Skip the interior of a multi-line comment - transparent for blank line counting
                        continue;
                    } else if is_block_attribute_line(trimmed, ctx.flavor) {
                        // Skip kramdown IAL - they are attached to headings and transparent for blank line counting
                        continue;
                    } else if ctx.lines[j].in_front_matter {
                        // Skip frontmatter - first heading after frontmatter doesn't need blank line above
                        // Note: We only check in_front_matter flag, NOT the string "---", because
                        // a standalone "---" is a horizontal rule and should NOT exempt headings
                        // from requiring blank lines above
                        hit_frontmatter_end = true;
                        break;
                    } else {
                        break;
                    }
                }
                let required = required_above_count.unwrap();
                if !hit_frontmatter_end && blank_lines_above < required {
                    let needed_blanks = required - blank_lines_above;
                    heading_violations.push((line_num, first_idx, "above", needed_blanks, heading_level));
                }
            }

            // Determine the effective last line of the heading
            let mut effective_last_line = if matches!(
                heading.style,
                crate::lint_context::HeadingStyle::Setext1 | crate::lint_context::HeadingStyle::Setext2
            ) {
                line_num + 1 // For Setext, include the underline
            } else {
                line_num
            };

            // Extend effective_last_line to include any kramdown IAL lines immediately following
            // IAL lines like `{: .class #id}` are part of the heading element
            while effective_last_line + 1 < ctx.lines.len() {
                let next_line = &ctx.lines[effective_last_line + 1];
                let next_trimmed = next_line.content(ctx.content).trim();
                if is_block_attribute_line(next_trimmed, ctx.flavor) {
                    effective_last_line += 1;
                } else {
                    break;
                }
            }

            // Check blank lines below
            if effective_last_line < ctx.lines.len() - 1 {
                // Find next non-blank line, skipping transparent elements (blank lines, HTML comments, Quarto div markers)
                let mut next_non_blank_idx = effective_last_line + 1;
                while next_non_blank_idx < ctx.lines.len() {
                    let check_line = &ctx.lines[next_non_blank_idx];
                    let _check_trimmed = check_line.content(ctx.content).trim();
                    if check_line.is_blank {
                        next_non_blank_idx += 1;
                    } else if check_line.in_html_comment || is_blank_or_comment_only(check_line.content(ctx.content)) {
                        // Skip HTML comments - they are transparent for blank line counting
                        next_non_blank_idx += 1;
                    } else {
                        break;
                    }
                }

                // If we've reached end of document (after skipping transparent elements), no blank needed
                if next_non_blank_idx >= ctx.lines.len() {
                    // End of document - no blank line needed after heading
                    continue;
                }

                // Check if next line is a code fence or list item
                let next_line_is_special = {
                    let next_line = &ctx.lines[next_non_blank_idx];
                    let next_trimmed = next_line.content(ctx.content).trim();

                    // Check for code fence
                    let is_code_fence = (next_trimmed.starts_with("```") || next_trimmed.starts_with("~~~"))
                        && (next_trimmed.len() == 3
                            || (next_trimmed.len() > 3
                                && next_trimmed
                                    .chars()
                                    .nth(3)
                                    .is_some_and(|c| c.is_whitespace() || c.is_alphabetic())));

                    // Check for list item. The parser's `list_item` flag for a following
                    // marker is not parse-stable here: a sibling list directly above the
                    // heading can leak list context onto this line, so inserting the blank
                    // line above the heading flips the flag (e.g. `2. ` counts as a list
                    // item in one pass and not the next). Detect the marker syntactically as
                    // well, so the guard - and therefore the fix - stays idempotent.
                    let is_list_item = next_line.list_item.is_some() || starts_with_list_marker(next_trimmed);

                    is_code_fence || is_list_item
                };

                // Only generate warning if next line is NOT a code fence or list item
                if !next_line_is_special && let Some(required) = required_below_count {
                    // Count blank lines below (counting only blank lines, not skipped transparent lines)
                    let mut blank_lines_below = 0;
                    for k in (effective_last_line + 1)..next_non_blank_idx {
                        // A line contributing nothing but comments counts as blank (#866)
                        if ctx.lines[k].is_blank || is_blank_or_comment_only(ctx.lines[k].content(ctx.content)) {
                            blank_lines_below += 1;
                        }
                    }

                    if blank_lines_below < required {
                        let needed_blanks = required - blank_lines_below;
                        heading_violations.push((line_num, first_idx, "below", needed_blanks, heading_level));
                    }
                }
            }
        }

        // Generate warnings for all violations
        for (heading_line, first_line, position, needed_blanks, heading_level) in heading_violations {
            let line_info = &ctx.lines[heading_line];

            // Calculate precise character range for the heading
            let (start_line, start_col, end_line, end_col) =
                calculate_heading_range(first_line + 1, heading_line + 1, line_info.content(ctx.content));

            // Each requirement is resolved inside the arm that uses it. A
            // requirement can be unlimited (a negative config value such as
            // `lines_above: -1`), in which case it has no required count and
            // never produces a violation for its own position. Resolving both
            // up front panicked whenever one position was unlimited and the
            // other reported.
            let (message, insertion_point) = match position {
                "above" => {
                    let Some(required_above_count) =
                        self.config.lines_above.get_for_level(heading_level).required_count()
                    else {
                        continue;
                    };
                    (
                        format!(
                            "Expected {} blank {} above heading",
                            required_above_count,
                            if required_above_count == 1 { "line" } else { "lines" }
                        ),
                        first_line, // Insert before the line the heading text starts on
                    )
                }
                "below" => {
                    let Some(required_below_count) =
                        self.config.lines_below.get_for_level(heading_level).required_count()
                    else {
                        continue;
                    };
                    // For Setext headings, insert after the underline
                    let insert_after = if line_info.heading.as_ref().is_some_and(|h| {
                        matches!(
                            h.style,
                            crate::lint_context::HeadingStyle::Setext1 | crate::lint_context::HeadingStyle::Setext2
                        )
                    }) {
                        heading_line + 2
                    } else {
                        heading_line + 1
                    };

                    (
                        format!(
                            "Expected {} blank {} below heading",
                            required_below_count,
                            if required_below_count == 1 { "line" } else { "lines" }
                        ),
                        insert_after,
                    )
                }
                _ => continue,
            };

            // Calculate byte range for insertion
            let byte_range = if insertion_point == 0 && position == "above" {
                // Insert at beginning of document (only for "above" case at line 0)
                0..0
            } else if position == "above" && insertion_point > 0 {
                // For "above", insert at the start of the heading line
                ctx.lines[insertion_point].byte_offset..ctx.lines[insertion_point].byte_offset
            } else if position == "below" && insertion_point - 1 < ctx.lines.len() {
                // For "below", insert after the line
                let line_idx = insertion_point - 1;
                let line_end_offset = if line_idx + 1 < ctx.lines.len() {
                    ctx.lines[line_idx + 1].byte_offset
                } else {
                    ctx.content.len()
                };
                line_end_offset..line_end_offset
            } else {
                // Insert at end of file
                let content_len = ctx.content.len();
                content_len..content_len
            };

            result.push(LintWarning {
                rule_name: Some(self.name().to_string()),
                message,
                line: start_line,
                column: start_col,
                end_line,
                end_column: end_col,
                severity: Severity::Warning,
                fix: Some(Fix::new(byte_range, line_ending.repeat(needed_blanks))),
            });
        }

        Ok(result)
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        if ctx.content.is_empty() {
            return Ok(ctx.content.to_string());
        }

        // Use a consolidated fix that avoids adding multiple blank lines
        let fixed = self.fix_content(ctx);

        Ok(fixed)
    }

    /// Get the category of this rule for selective processing
    fn category(&self) -> RuleCategory {
        RuleCategory::Heading
    }

    /// Check if this rule should be skipped
    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Fast path: check if document likely has headings
        if ctx.content.is_empty() || !ctx.likely_has_headings() {
            return true;
        }
        // Verify headings actually exist
        ctx.lines.iter().all(|line| line.heading.is_none())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
