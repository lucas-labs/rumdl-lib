use crate::filtered_lines::FilteredLinesExt;
use crate::rule::{Fix, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
/// Rule MD010: No tabs
///
/// See [docs/md010.md](../../docs/md010.md) for full documentation, configuration, and examples.
use crate::utils::range_utils::calculate_match_range;

pub mod md010_config;
pub use md010_config::MD010Config;

/// Rule MD010: Hard tabs
#[derive(Clone, Default)]
pub struct MD010NoHardTabs {
    config: MD010Config,
}

impl MD010NoHardTabs {
    fn count_leading_tabs(line: &str) -> usize {
        let mut count = 0;
        for c in line.chars() {
            if c == '\t' {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// The language an info string declares, or `""` when it declares none.
    ///
    /// Normally that is the first whitespace-separated word, so
    /// ```` ```makefile title="Makefile" ```` gives `makefile`. Two flavors write
    /// a language inside braces instead, and each is read only where it is real
    /// syntax: a Pandoc code-attribute block names it as the first `.class`
    /// (`{#id .makefile}` gives `makefile`), and a Quarto executable chunk names
    /// its engine (`{r, echo=FALSE}` gives `r`).
    fn fence_language(info_string: &str, _flavor: crate::config::MarkdownFlavor) -> std::borrow::Cow<'_, str> {
        let info = info_string.trim();

        std::borrow::Cow::Borrowed(info.split_whitespace().next().unwrap_or(""))
    }

    /// Lines (1-indexed) belonging to a code block whose language is listed in
    /// `ignore-code-languages`.
    ///
    /// Covers CommonMark fences plus the Azure DevOps colon fences the parser
    /// never sees. An indented code block has no info string and can never match.
    fn ignored_language_lines(&self, ctx: &crate::lint_context::LintContext) -> std::collections::HashSet<usize> {
        let mut ignored = std::collections::HashSet::new();

        for detail in ctx.code_block_details.iter().chain(ctx.colon_fence_details()) {
            let label = Self::fence_language(&detail.info_string, ctx.flavor);
            if label.is_empty()
                || !self
                    .config
                    .ignore_code_languages
                    .iter()
                    .any(|listed| listed.eq_ignore_ascii_case(label.as_ref()))
            {
                continue;
            }

            let start_line = ctx
                .line_offsets
                .partition_point(|&off| off <= detail.start)
                .saturating_sub(1);
            let end_byte = detail.end.saturating_sub(1);
            let end_line = ctx
                .line_offsets
                .partition_point(|&off| off <= end_byte)
                .saturating_sub(1);
            for line in start_line..=end_line {
                ignored.insert(line + 1);
            }
        }

        ignored
    }

    /// Runs of consecutive tabs as `(start, end)` byte offsets into `line`.
    fn find_and_group_tabs(line: &str) -> Vec<(usize, usize)> {
        let mut groups = Vec::new();
        let mut current_group_start: Option<usize> = None;
        let mut last_tab_pos = 0;

        for (i, byte) in line.bytes().enumerate() {
            if byte == b'\t' {
                if let Some(start) = current_group_start {
                    // We're in a group - check if this tab is consecutive
                    if i == last_tab_pos + 1 {
                        // Consecutive tab, continue the group
                        last_tab_pos = i;
                    } else {
                        // Gap found, save current group and start new one
                        groups.push((start, last_tab_pos + 1));
                        current_group_start = Some(i);
                        last_tab_pos = i;
                    }
                } else {
                    // Start a new group
                    current_group_start = Some(i);
                    last_tab_pos = i;
                }
            }
        }

        // Add the last group if there is one
        if let Some(start) = current_group_start {
            groups.push((start, last_tab_pos + 1));
        }

        groups
    }
}

impl Rule for MD010NoHardTabs {
    fn name(&self) -> &'static str {
        "MD010"
    }

    fn description(&self) -> &'static str {
        "No tabs"
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let mut warnings = Vec::new();

        let mut filtered = ctx
            .filtered_lines()
            .skip_front_matter()
            .skip_html_comments()
            .skip_mdx_comments()
            .skip_html_blocks()
            .skip_pymdown_blocks()
            .skip_mkdocstrings()
            .skip_esm_blocks();

        if !self.config.code_blocks {
            filtered = filtered.skip_code_blocks();
        }

        // Testing code-blocks here only avoids building a set that cannot matter:
        // with it false the walk has already dropped every code block line.
        let ignored_lines = if self.config.code_blocks && !self.config.ignore_code_languages.is_empty() {
            self.ignored_language_lines(ctx)
        } else {
            std::collections::HashSet::new()
        };

        for filtered_line in filtered {
            if ignored_lines.contains(&filtered_line.line_num) {
                continue;
            }
            let line_num = filtered_line.line_num - 1;
            let line = filtered_line.content;

            // Process tabs directly without intermediate collection
            let tab_groups = Self::find_and_group_tabs(line);
            if tab_groups.is_empty() {
                continue;
            }

            let leading_tabs = Self::count_leading_tabs(line);
            let line_start = ctx.line_content_byte_range(line_num + 1).start;

            // Generate warning for each group of consecutive tabs
            for (start_pos, end_pos) in tab_groups {
                let tab_count = end_pos - start_pos;
                let is_leading = start_pos < leading_tabs;

                // Calculate precise character range for the tab group
                let (start_line, start_col, end_line, end_col) =
                    calculate_match_range(line_num + 1, line, start_pos, tab_count);

                let message = if line.trim().is_empty() {
                    if tab_count == 1 {
                        "Empty line contains tab".to_string()
                    } else {
                        format!("Empty line contains {tab_count} tabs")
                    }
                } else if is_leading {
                    if tab_count == 1 {
                        format!(
                            "Found leading tab, use {} spaces instead",
                            self.config.spaces_per_tab.get()
                        )
                    } else {
                        format!(
                            "Found {} leading tabs, use {} spaces instead",
                            tab_count,
                            tab_count * self.config.spaces_per_tab.get()
                        )
                    }
                } else if tab_count == 1 {
                    "Found tab for alignment, use spaces instead".to_string()
                } else {
                    format!("Found {tab_count} tabs for alignment, use spaces instead")
                };

                warnings.push(LintWarning {
                    rule_name: Some(self.name().to_string()),
                    line: start_line,
                    column: start_col,
                    end_line,
                    end_column: end_col,
                    message,
                    severity: Severity::Warning,
                    fix: Some(Fix::new(
                        line_start + start_pos..line_start + end_pos,
                        " ".repeat(tab_count * self.config.spaces_per_tab.get()),
                    )),
                });
            }
        }

        Ok(warnings)
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        if self.should_skip(ctx) {
            return Ok(ctx.content.to_string());
        }
        let warnings = self.check(ctx)?;
        if warnings.is_empty() {
            return Ok(ctx.content.to_string());
        }
        let warnings = warnings;
        crate::utils::fix_utils::apply_warning_fixes(ctx.content, &warnings)
            .map_err(crate::rule::LintError::InvalidInput)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Skip if content is empty or has no tabs
        ctx.content.is_empty() || !ctx.has_char('\t')
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Whitespace
    }
}
