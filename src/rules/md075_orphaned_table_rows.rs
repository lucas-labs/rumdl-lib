use std::collections::HashSet;

use super::md060_table_format::MD060TableFormat;
use crate::rule::{Fix, FixCapability, LintError, LintResult, LintWarning, Rule, RuleCategory, Severity};
use crate::utils::blockquote::strip_blockquote_prefix;
use crate::utils::ensure_consistent_line_endings;
use crate::utils::fix_utils::apply_warning_fixes;
use crate::utils::table_utils::TableUtils;

/// Rule MD075: Orphaned table rows / headerless tables
///
/// See [docs/md075.md](../../docs/md075.md) for full documentation and examples.
///
/// Detects two cases:
/// 1. Pipe-delimited rows separated from a preceding table by blank lines (auto-fixable)
/// 2. Standalone pipe-formatted rows without a table header/delimiter (warn only)
#[derive(Clone)]
pub struct MD075OrphanedTableRows {
    md060_formatter: MD060TableFormat,
}

/// Represents a group of orphaned rows after a table (Case 1)
struct OrphanedGroup {
    /// Start line of the preceding table block (0-indexed)
    table_start: usize,
    /// End line of the preceding table block (0-indexed)
    table_end: usize,
    /// Expected table column count derived from the original table header
    expected_columns: usize,
    /// First blank line separating orphaned rows from the table
    blank_start: usize,
    /// Last blank line before the orphaned rows
    blank_end: usize,
    /// The orphaned row lines (0-indexed)
    row_lines: Vec<usize>,
}

/// Represents standalone headerless pipe content (Case 2)
struct HeaderlessGroup {
    /// The first line of the group (0-indexed)
    start_line: usize,
    /// All lines in the group (0-indexed)
    lines: Vec<usize>,
}

impl MD075OrphanedTableRows {
    /// Check if a line should be skipped (frontmatter, code block, HTML, ESM, mkdocstrings, math)
    fn should_skip_line(&self, ctx: &crate::lint_context::LintContext, line_idx: usize) -> bool {
        if let Some(line_info) = ctx.lines.get(line_idx) {
            line_info.in_front_matter
                || line_info.in_code_block
                || line_info.in_html_block
                || line_info.in_html_comment
                || false
                || false
                || line_info.in_mkdocstrings
                || line_info.in_math_block
        } else {
            false
        }
    }

    /// Check if a line is a potential table row, handling blockquote prefixes
    fn is_table_row_line(&self, line: &str, flavor: crate::config::MarkdownFlavor) -> bool {
        let content = strip_blockquote_prefix(line);
        TableUtils::is_potential_table_row_with_flavor(content, flavor)
    }

    /// Check if a line is a delimiter row, handling blockquote prefixes
    fn is_delimiter_line(&self, line: &str) -> bool {
        let content = strip_blockquote_prefix(line);
        TableUtils::is_delimiter_row(content)
    }

    /// Check if a line is blank (including blockquote continuation lines like ">")
    fn is_blank_line(line: &str) -> bool {
        crate::utils::regex_cache::is_blank_in_blockquote_context(line)
    }

    /// Heuristic to detect templating syntax (Liquid/Jinja-style markers).
    fn contains_template_marker(line: &str) -> bool {
        let trimmed = line.trim();
        trimmed.contains("{%")
            || trimmed.contains("%}")
            || trimmed.contains("{{")
            || trimmed.contains("}}")
            || trimmed.contains("{#")
            || trimmed.contains("#}")
    }

    /// Detect lines that are pure template directives (e.g., `{% data ... %}`).
    fn is_template_directive_line(line: &str) -> bool {
        let trimmed = line.trim();
        (trimmed.starts_with("{%")
            || trimmed.starts_with("{%-")
            || trimmed.starts_with("{{")
            || trimmed.starts_with("{{-"))
            && (trimmed.ends_with("%}")
                || trimmed.ends_with("-%}")
                || trimmed.ends_with("}}")
                || trimmed.ends_with("-}}"))
    }

    /// Pipe-bearing lines with template markers are often generated fragments, not literal tables.
    fn is_templated_pipe_line(line: &str) -> bool {
        let content = strip_blockquote_prefix(line).trim();
        content.contains('|') && Self::contains_template_marker(content)
    }

    /// Row-like line with pipes that is not itself a valid table row, often used
    /// as an in-table section divider (for example: `Search||`).
    fn is_sparse_table_row_hint(line: &str) -> bool {
        let content = strip_blockquote_prefix(line).trim();
        if content.is_empty()
            || !content.contains('|')
            || Self::contains_template_marker(content)
            || TableUtils::is_delimiter_row(content)
            || TableUtils::is_potential_table_row(content)
        {
            return false;
        }

        let has_edge_pipe = content.starts_with('|') || content.ends_with('|');
        let has_repeated_pipe = content.contains("||");
        let non_empty_parts = content.split('|').filter(|part| !part.trim().is_empty()).count();

        non_empty_parts >= 1 && (has_edge_pipe || has_repeated_pipe)
    }

    /// Headerless groups after a sparse row that is itself inside a table context
    /// are likely false positives caused by parser table-block boundaries.
    fn preceded_by_sparse_table_context(content_lines: &[&str], start_line: usize) -> bool {
        let mut idx = start_line;
        while idx > 0 {
            idx -= 1;
            let content = strip_blockquote_prefix(content_lines[idx]).trim();
            if content.is_empty() {
                continue;
            }

            if !Self::is_sparse_table_row_hint(content) {
                return false;
            }

            let mut scan = idx;
            while scan > 0 {
                scan -= 1;
                let prev = strip_blockquote_prefix(content_lines[scan]).trim();
                if prev.is_empty() {
                    break;
                }
                if TableUtils::is_delimiter_row(prev) {
                    return true;
                }
            }

            return false;
        }

        false
    }

    /// Headerless rows immediately following a template directive are likely generated table fragments.
    fn preceded_by_template_directive(content_lines: &[&str], start_line: usize) -> bool {
        let mut idx = start_line;
        while idx > 0 {
            idx -= 1;
            let content = strip_blockquote_prefix(content_lines[idx]).trim();
            if content.is_empty() {
                continue;
            }

            return Self::is_template_directive_line(content);
        }

        false
    }

    /// Count visual indentation width where tab is treated as 4 spaces.
    fn indentation_width(line: &str) -> usize {
        let mut width = 0;
        for b in line.bytes() {
            match b {
                b' ' => width += 1,
                b'\t' => width += 4,
                _ => break,
            }
        }
        width
    }

    /// Count blockquote nesting depth for context matching.
    fn blockquote_depth(line: &str) -> usize {
        let (prefix, _) = TableUtils::extract_blockquote_prefix(line);
        prefix.bytes().filter(|&b| b == b'>').count()
    }

    /// Check whether a Case 1 candidate has at least one outer table pipe.
    ///
    /// An outer pipe preserves supported table styles while excluding bare pipe-delimited
    /// prose that the formatter could otherwise merge into the preceding table.
    fn has_outer_table_pipe(line: &str) -> bool {
        let candidate = strip_blockquote_prefix(line).trim();
        candidate.starts_with('|') || candidate.ends_with('|')
    }

    /// Ensure candidate orphan rows are in the same render context as the table.
    ///
    /// This prevents removing blank lines across boundaries where merging is invalid,
    /// such as table -> blockquote row transitions or list-context changes.
    fn row_matches_table_context(
        &self,
        table_block: &crate::utils::table_utils::TableBlock,
        content_lines: &[&str],
        row_idx: usize,
    ) -> bool {
        let table_start_line = content_lines[table_block.start_line];
        let candidate_line = content_lines[row_idx];

        if Self::blockquote_depth(table_start_line) != Self::blockquote_depth(candidate_line) {
            return false;
        }

        let (_, candidate_after_blockquote) = TableUtils::extract_blockquote_prefix(candidate_line);
        let (candidate_list_prefix, _, _) = TableUtils::extract_list_prefix(candidate_after_blockquote);
        let candidate_indent = Self::indentation_width(candidate_after_blockquote);

        if let Some(list_ctx) = &table_block.list_context {
            // Table continuation rows in lists must stay continuation rows, not new list items.
            if !candidate_list_prefix.is_empty() {
                return false;
            }
            candidate_indent >= list_ctx.content_indent && candidate_indent < list_ctx.content_indent + 4
        } else {
            // Avoid crossing into list/code contexts for non-list tables.
            candidate_list_prefix.is_empty() && candidate_indent < 4
        }
    }

    /// Detect Case 1: Orphaned rows after existing tables
    fn detect_orphaned_rows(
        &self,
        ctx: &crate::lint_context::LintContext,
        content_lines: &[&str],
        table_line_set: &HashSet<usize>,
    ) -> Vec<OrphanedGroup> {
        let mut groups = Vec::new();

        for table_block in &ctx.table_blocks {
            let end = table_block.end_line;
            let header_content =
                TableUtils::extract_table_row_content(content_lines[table_block.start_line], table_block, 0);
            let expected_columns = TableUtils::count_cells_with_flavor(header_content, ctx.flavor);

            // Scan past end of table for blank lines followed by pipe rows
            let mut i = end + 1;
            let mut blank_start = None;
            let mut blank_end = None;

            // Find blank lines after the table
            while i < content_lines.len() {
                if self.should_skip_line(ctx, i) {
                    break;
                }
                if Self::is_blank_line(content_lines[i]) {
                    if blank_start.is_none() {
                        blank_start = Some(i);
                    }
                    blank_end = Some(i);
                    i += 1;
                } else {
                    break;
                }
            }

            // If no blank lines found, no orphan scenario
            let (Some(bs), Some(be)) = (blank_start, blank_end) else {
                continue;
            };

            // Now check if the lines after the blanks are pipe rows not in any table
            let mut orphan_rows = Vec::new();
            let mut j = be + 1;
            while j < content_lines.len() {
                if self.should_skip_line(ctx, j) {
                    break;
                }
                if table_line_set.contains(&j) {
                    break;
                }
                if Self::has_outer_table_pipe(content_lines[j])
                    && self.is_table_row_line(content_lines[j], ctx.flavor)
                    && self.row_matches_table_context(table_block, content_lines, j)
                {
                    orphan_rows.push(j);
                    j += 1;
                } else {
                    break;
                }
            }

            if !orphan_rows.is_empty() {
                groups.push(OrphanedGroup {
                    table_start: table_block.start_line,
                    table_end: table_block.end_line,
                    expected_columns,
                    blank_start: bs,
                    blank_end: be,
                    row_lines: orphan_rows,
                });
            }
        }

        groups
    }

    /// Detect pipe rows that directly continue a parsed table block but may not be
    /// recognized by `table_blocks` (for example rows with inline fence markers).
    ///
    /// These rows should not be treated as standalone headerless tables (Case 2).
    fn detect_table_continuation_rows(
        &self,
        ctx: &crate::lint_context::LintContext,
        content_lines: &[&str],
        table_line_set: &HashSet<usize>,
    ) -> HashSet<usize> {
        let mut continuation_rows = HashSet::new();

        for table_block in &ctx.table_blocks {
            let mut i = table_block.end_line + 1;
            while i < content_lines.len() {
                if self.should_skip_line(ctx, i) || table_line_set.contains(&i) {
                    break;
                }
                if self.is_table_row_line(content_lines[i], ctx.flavor)
                    && self.row_matches_table_context(table_block, content_lines, i)
                {
                    continuation_rows.insert(i);
                    i += 1;
                } else {
                    break;
                }
            }
        }

        continuation_rows
    }

    /// Detect Case 2: Standalone headerless pipe content
    fn detect_headerless_tables(
        &self,
        ctx: &crate::lint_context::LintContext,
        content_lines: &[&str],
        table_line_set: &HashSet<usize>,
        orphaned_line_set: &HashSet<usize>,
        continuation_line_set: &HashSet<usize>,
    ) -> Vec<HeaderlessGroup> {
        if self.is_probable_headerless_fragment_file(ctx, content_lines) {
            return Vec::new();
        }

        let mut groups = Vec::new();
        let mut i = 0;

        while i < content_lines.len() {
            // Skip lines in skip contexts, existing tables, or orphaned groups
            if self.should_skip_line(ctx, i)
                || table_line_set.contains(&i)
                || orphaned_line_set.contains(&i)
                || continuation_line_set.contains(&i)
            {
                i += 1;
                continue;
            }

            // Look for consecutive pipe rows
            if self.is_table_row_line(content_lines[i], ctx.flavor) {
                if Self::is_templated_pipe_line(content_lines[i]) {
                    i += 1;
                    continue;
                }

                // Suppress headerless detection for likely template-generated table fragments.
                if Self::preceded_by_template_directive(content_lines, i) {
                    i += 1;
                    while i < content_lines.len()
                        && !self.should_skip_line(ctx, i)
                        && !table_line_set.contains(&i)
                        && !orphaned_line_set.contains(&i)
                        && !continuation_line_set.contains(&i)
                        && self.is_table_row_line(content_lines[i], ctx.flavor)
                    {
                        i += 1;
                    }
                    continue;
                }

                // Suppress headerless detection for rows that likely continue an
                // existing table through sparse section-divider rows.
                if Self::preceded_by_sparse_table_context(content_lines, i) {
                    i += 1;
                    while i < content_lines.len()
                        && !self.should_skip_line(ctx, i)
                        && !table_line_set.contains(&i)
                        && !orphaned_line_set.contains(&i)
                        && !continuation_line_set.contains(&i)
                        && self.is_table_row_line(content_lines[i], ctx.flavor)
                    {
                        i += 1;
                    }
                    continue;
                }

                let start = i;
                let mut group_lines = vec![i];
                i += 1;

                while i < content_lines.len()
                    && !self.should_skip_line(ctx, i)
                    && !table_line_set.contains(&i)
                    && !orphaned_line_set.contains(&i)
                    && !continuation_line_set.contains(&i)
                    && self.is_table_row_line(content_lines[i], ctx.flavor)
                {
                    if Self::is_templated_pipe_line(content_lines[i]) {
                        break;
                    }
                    group_lines.push(i);
                    i += 1;
                }

                // Need at least 2 consecutive pipe rows to flag
                if group_lines.len() >= 2 {
                    // Check that none of these lines is a delimiter row that would make
                    // them a valid table header+delimiter combination
                    let has_delimiter = group_lines
                        .iter()
                        .any(|&idx| self.is_delimiter_line(content_lines[idx]));

                    if !has_delimiter {
                        // Verify consistent column count
                        let first_content = strip_blockquote_prefix(content_lines[group_lines[0]]);
                        let first_count = TableUtils::count_cells_with_flavor(first_content, ctx.flavor);
                        let consistent = group_lines.iter().all(|&idx| {
                            let content = strip_blockquote_prefix(content_lines[idx]);
                            TableUtils::count_cells_with_flavor(content, ctx.flavor) == first_count
                        });

                        if consistent && first_count > 0 {
                            groups.push(HeaderlessGroup {
                                start_line: start,
                                lines: group_lines,
                            });
                        }
                    }
                }
            } else {
                i += 1;
            }
        }

        groups
    }

    /// Some repositories store reusable table-row snippets as standalone files
    /// (headerless by design). Suppress Case 2 warnings for those fragment files.
    fn is_probable_headerless_fragment_file(
        &self,
        ctx: &crate::lint_context::LintContext,
        content_lines: &[&str],
    ) -> bool {
        if !ctx.table_blocks.is_empty() {
            return false;
        }

        let mut row_count = 0usize;

        for (idx, line) in content_lines.iter().enumerate() {
            if self.should_skip_line(ctx, idx) {
                continue;
            }

            let content = strip_blockquote_prefix(line).trim();
            if content.is_empty() {
                continue;
            }

            if Self::is_template_directive_line(content) {
                continue;
            }

            if TableUtils::is_delimiter_row(content) {
                return false;
            }

            // Allow inline template gate rows like `| {% ifversion ... %} |`.
            if Self::contains_template_marker(content) && content.contains('|') {
                continue;
            }

            if self.is_table_row_line(content, ctx.flavor) {
                let cols = TableUtils::count_cells_with_flavor(content, ctx.flavor);
                // Require 3+ columns to avoid suppressing common 2-column headerless issues.
                if cols < 3 {
                    return false;
                }
                row_count += 1;
                continue;
            }

            return false;
        }

        row_count >= 2
    }

    /// Build fix edit for a single orphaned-row group by replacing the local table block.
    fn build_orphan_group_fix(
        &self,
        ctx: &crate::lint_context::LintContext,
        content_lines: &[&str],
        group: &OrphanedGroup,
    ) -> Result<Option<Fix>, LintError> {
        if group.row_lines.is_empty() {
            return Ok(None);
        }

        let last_orphan = *group
            .row_lines
            .last()
            .expect("row_lines is non-empty after early return");

        // Be conservative: only auto-merge when orphan rows match original table width.
        let has_column_mismatch = group
            .row_lines
            .iter()
            .any(|&idx| TableUtils::count_cells_with_flavor(content_lines[idx], ctx.flavor) != group.expected_columns);
        if has_column_mismatch {
            return Ok(None);
        }

        let replacement_range = ctx.line_span_byte_range(group.table_start + 1, last_orphan + 1);
        let original_block = &ctx.content[replacement_range.clone()];
        let block_has_trailing_newline = original_block.ends_with('\n');

        let mut merged_table_lines: Vec<&str> = (group.table_start..=group.table_end)
            .map(|idx| content_lines[idx])
            .collect();
        merged_table_lines.extend(group.row_lines.iter().map(|&idx| content_lines[idx]));

        let mut merged_block = merged_table_lines.join("\n");
        if block_has_trailing_newline {
            merged_block.push('\n');
        }

        let block_ctx = crate::lint_context::LintContext::new(&merged_block, ctx.flavor, None);
        let mut normalized_block = self.md060_formatter.fix(&block_ctx)?;

        if !block_has_trailing_newline {
            normalized_block = normalized_block.trim_end_matches('\n').to_string();
        } else if !normalized_block.ends_with('\n') {
            normalized_block.push('\n');
        }

        let replacement = ensure_consistent_line_endings(original_block, &normalized_block);

        if replacement == original_block {
            Ok(None)
        } else {
            Ok(Some(Fix::new(replacement_range, replacement)))
        }
    }
}

impl Default for MD075OrphanedTableRows {
    fn default() -> Self {
        Self {
            // MD075 should normalize merged rows even when MD060 is not explicitly enabled.
            md060_formatter: MD060TableFormat::new(true, "aligned".to_string()),
        }
    }
}

impl Rule for MD075OrphanedTableRows {
    fn name(&self) -> &'static str {
        "MD075"
    }

    fn description(&self) -> &'static str {
        "Orphaned table rows or headerless pipe content"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Table
    }

    fn should_skip(&self, ctx: &crate::lint_context::LintContext) -> bool {
        // Need at least 2 pipe characters for two minimal rows like:
        // a | b
        // c | d
        ctx.char_count('|') < 2
    }

    fn check(&self, ctx: &crate::lint_context::LintContext) -> LintResult {
        let content_lines = ctx.raw_lines();
        let mut warnings = Vec::new();

        // Build set of all lines belonging to existing table blocks
        let mut table_line_set = HashSet::new();
        for table_block in &ctx.table_blocks {
            for line_idx in table_block.start_line..=table_block.end_line {
                table_line_set.insert(line_idx);
            }
        }

        // Case 1: Orphaned rows after tables
        let orphaned_groups = self.detect_orphaned_rows(ctx, content_lines, &table_line_set);
        let orphan_group_fixes: Vec<Option<Fix>> = orphaned_groups
            .iter()
            .map(|group| self.build_orphan_group_fix(ctx, content_lines, group))
            .collect::<Result<Vec<_>, _>>()?;
        let mut orphaned_line_set = HashSet::new();
        for group in &orphaned_groups {
            for &line_idx in &group.row_lines {
                orphaned_line_set.insert(line_idx);
            }
            // Also mark blank lines as part of the orphan group for dedup
            for line_idx in group.blank_start..=group.blank_end {
                orphaned_line_set.insert(line_idx);
            }
        }
        let continuation_line_set = self.detect_table_continuation_rows(ctx, content_lines, &table_line_set);

        for (group, group_fix) in orphaned_groups.iter().zip(orphan_group_fixes.iter()) {
            let first_orphan = group.row_lines[0];
            let last_orphan = *group.row_lines.last().unwrap();
            let num_blanks = group.blank_end - group.blank_start + 1;

            warnings.push(LintWarning {
                rule_name: Some(self.name().to_string()),
                message: format!("Orphaned table row(s) separated from preceding table by {num_blanks} blank line(s)"),
                line: first_orphan + 1,
                column: 1,
                end_line: last_orphan + 1,
                end_column: content_lines[last_orphan].chars().count() + 1,
                severity: Severity::Warning,
                fix: group_fix.clone(),
            });
        }

        // Case 2: Headerless pipe content
        let headerless_groups = self.detect_headerless_tables(
            ctx,
            content_lines,
            &table_line_set,
            &orphaned_line_set,
            &continuation_line_set,
        );

        for group in &headerless_groups {
            let start = group.start_line;
            let end = *group.lines.last().unwrap();

            warnings.push(LintWarning {
                rule_name: Some(self.name().to_string()),
                message: "Pipe-formatted rows without a table header/delimiter row".to_string(),
                line: start + 1,
                column: 1,
                end_line: end + 1,
                end_column: content_lines[end].chars().count() + 1,
                severity: Severity::Warning,
                fix: None,
            });
        }

        Ok(warnings)
    }

    fn fix(&self, ctx: &crate::lint_context::LintContext) -> Result<String, LintError> {
        let warnings = self.check(ctx)?;
        let warnings = warnings;
        if warnings.iter().all(|warning| warning.fix.is_none()) {
            return Ok(ctx.content.to_string());
        }

        apply_warning_fixes(ctx.content, &warnings).map_err(LintError::FixFailed)
    }

    fn fix_capability(&self) -> FixCapability {
        FixCapability::ConditionallyFixable
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
