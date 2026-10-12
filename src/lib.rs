#![warn(clippy::pedantic)]
// Style-only pedantic lints we don't enforce. Each one generated >5 occurrences
// that were either deliberate design choices or too noisy for the value
// delivered. Categories that flag potential bugs stay on.
#![allow(clippy::doc_markdown)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::if_not_else)]
#![allow(clippy::similar_names)]
#![allow(clippy::wildcard_imports)]
#![allow(clippy::case_sensitive_file_extension_comparisons)]
#![allow(clippy::doc_link_with_quotes)]
#![allow(clippy::needless_raw_string_hashes)]
#![allow(clippy::trivially_copy_pass_by_ref)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]
#![allow(clippy::elidable_lifetime_names)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::redundant_else)]
#![allow(clippy::single_match_else)]
#![allow(clippy::needless_continue)]
#![allow(clippy::semicolon_if_nothing_returned)]
#![allow(clippy::ignored_unit_patterns)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::implicit_hasher)]
#![allow(clippy::ref_option)]
#![allow(clippy::struct_field_names)]
#![allow(clippy::unused_self)]
#![allow(clippy::unnested_or_patterns)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::format_push_string)]
// Test smoke-constructors like `let _formatter = Foo;` fire this lint,
// but are acceptable: they verify the type exists without asserting behavior.
#![allow(clippy::no_effect_underscore_binding)]
// Style-only: `Default::default()` vs `T::default()`. Both are readable.
#![allow(clippy::default_trait_access)]
// Style-only: `"".to_string()` vs `String::new()`. Tests favor the former
// for symmetry with non-empty string literals.
#![allow(clippy::manual_string_new)]

mod config;
mod filtered_lines;
mod link_path;
mod lint_context;
mod rule;
mod rules;
mod types;
mod utils;

// what reforge-cli uses, and nothing else
pub use crate::config::MarkdownFlavor;
pub use crate::link_path::link_path_part;
pub use crate::lint_context::LintContext;
pub use crate::rule::{FixCapability, Rule};
pub use crate::rules::{
    CodeFenceStyle, ListStyle, MD001HeadingIncrement, MD004UnorderedListStyle, MD005ListIndent, MD007ULIndent,
    MD009TrailingSpaces, MD010NoHardTabs, MD012NoMultipleBlanks, MD013Config, MD013LineLength, MD018NoMissingSpaceAtx,
    MD020NoMissingSpaceClosedAtx, MD022BlanksAroundHeadings, MD023HeadingStartLeft, MD024NoDuplicateHeading,
    MD027MultipleSpacesBlockquote, MD029OrderedListPrefix, MD030ListMarkerSpace, MD031BlanksAroundFences,
    MD032BlanksAroundLists, MD035HRStyle, MD047SingleTrailingNewline, MD048CodeFenceStyle, MD049EmphasisStyle,
    MD050StrongStyle, MD055TablePipeStyle, MD056TableColumnCount, MD058BlanksAroundTables, MD058Config, MD060Config,
    MD060TableFormat, MD064NoMultipleConsecutiveSpaces, MD065BlanksAroundHorizontalRules, MD069NoDuplicateListMarkers,
    MD071BlankLineAfterFrontmatter, MD075OrphanedTableRows, MD077ListContinuationIndent, UnorderedListStyle,
    emphasis_style::EmphasisStyle, md013_line_length::md013_config::ReflowMode, strong_style::StrongStyle,
};
pub use crate::types::LineLength;
pub use crate::utils::table_utils::TableUtils;
