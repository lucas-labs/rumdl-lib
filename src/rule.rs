//!
//! This module defines the Rule trait and related types for implementing linting rules in rumdl.

use dyn_clone::DynClone;
use std::ops::Range;
use thiserror::Error;

use crate::lint_context::LintContext;

// Macro to implement box_clone for Rule implementors
#[macro_export]
macro_rules! impl_rule_clone {
    ($ty:ty) => {
        impl $ty {
            fn box_clone(&self) -> Box<dyn Rule> {
                Box::new(self.clone())
            }
        }
    };
}

#[derive(Debug, Error)]
pub enum LintError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Fix failed: {0}")]
    FixFailed(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Parsing error: {0}")]
    ParsingError(String),
}

pub type LintResult = Result<Vec<LintWarning>, LintError>;

#[derive(Debug, PartialEq, Clone)]
pub struct LintWarning {
    pub(crate) message: String,
    pub line: usize, // 1-indexed start line
    /// 1-indexed start column, measured in **characters** (not bytes).
    /// When deriving a column from a byte offset (regex match, `str::find`,
    /// parser byte offset), convert with `range_utils::byte_to_char_count` or a
    /// character-based range helper. A raw byte offset mis-positions the
    /// highlight on lines containing multi-byte UTF-8.
    pub(crate) column: usize,
    pub(crate) end_line: usize, // 1-indexed end line
    /// 1-indexed end column, measured in **characters** (see `column`). Use
    /// `str::chars().count()`, not `str::len()`, when computing a span width.
    pub(crate) end_column: usize,
    pub(crate) severity: Severity,
    pub fix: Option<Fix>,
    pub(crate) rule_name: Option<String>,
}

/// One atomic fix attached to a `LintWarning`.
///
/// `range`/`replacement` describe the primary edit. `additional_edits`
/// carries any *paired* edits that must apply together with the primary one
/// for the result to be a valid document — for example, MD054's conversion
/// of an inline link to a reference style produces the in-place link rewrite
/// **and** a reference-definition append at end-of-file; applying only one
/// half would leave a dangling reference.
///
/// All fix consumers (the LSP code-action layer, CLI counters, the
/// `apply_warning_fixes` helper) treat the primary edit and the
/// `additional_edits` as a single unit. The field is empty by default so
/// rules that only need a single-location fix can keep using
/// `Fix::new(range, replacement)`; only rules that need multi-location
/// atomicity populate it via `Fix::with_additional_edits(...)`.
///
/// `additional_edits` is intentionally a flat `Vec<Fix>` — nesting beyond
/// one level isn't needed today and would complicate the apply contract.
/// Apply order is "primary first, then additional in their declared order"
/// when offsets are non-overlapping; consumers that batch multiple fixes
/// across warnings still sort by `range.start` descending so earlier offsets
/// remain valid as later edits mutate the buffer.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct Fix {
    pub(crate) range: Range<usize>,
    pub(crate) replacement: String,
    /// Edits applied atomically with the primary `range`/`replacement` pair.
    /// Empty for the common single-edit case. See struct docs for semantics.
    pub(crate) additional_edits: Vec<Fix>,
}

impl Fix {
    /// Construct a single-edit fix. Use this for the overwhelming common case
    /// where a fix is one in-place replacement.
    pub(crate) fn new(range: Range<usize>, replacement: String) -> Self {
        Self {
            range,
            replacement,
            additional_edits: Vec::new(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum Severity {
    Error,
    Warning,
}

/// Type of rule for selective processing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleCategory {
    Heading,
    List,
    CodeBlock,
    Link,
    Image,
    Html,
    Emphasis,
    Whitespace,
    Blockquote,
    Table,
    FrontMatter,
    Other,
}

/// Capability of a rule to fix issues
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixCapability {
    /// Rule can automatically fix all violations it detects
    FullyFixable,
    /// Rule can fix some violations based on context
    ConditionallyFixable,
    /// Rule cannot fix violations (by design)
    Unfixable,
}

pub trait Rule: DynClone + Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn check(&self, ctx: &LintContext) -> LintResult;
    fn fix(&self, ctx: &LintContext) -> Result<String, LintError>;

    /// Check if this rule should quickly skip processing based on content
    fn should_skip(&self, _ctx: &LintContext) -> bool {
        false
    }

    /// Get the category of this rule for selective processing
    fn category(&self) -> RuleCategory {
        RuleCategory::Other // Default implementation returns Other
    }

    /// Whether the content-category prefilter may skip this rule.
    ///
    /// The prefilter reads the document's shape alone: a `Link` rule is skipped
    /// for a document holding no links. A rule whose configuration widens what
    /// it reads answers `false` for that configuration, so that `should_skip`
    /// and `check` decide instead. MD051 and MD057 read frontmatter values on
    /// request, and a document can carry those with no link syntax at all.
    fn skippable_by_category(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn std::any::Any;

    // DocumentStructure has been merged into LintContext - this method is no longer used
    // fn as_maybe_document_structure(&self) -> Option<&dyn MaybeDocumentStructure> {
    //     None
    // }

    /// Declares the fix capability of this rule
    fn fix_capability(&self) -> FixCapability {
        FixCapability::FullyFixable // Safe default for backward compatibility
    }
}

// Implement the cloning logic for the Rule trait object
dyn_clone::clone_trait_object!(Rule);

// Inline config parsing functions are in inline_config.rs.
// Use InlineConfig::from_content() for the full inline configuration system,
// or inline_config::parse_disable_comment/parse_enable_comment for low-level parsing.
