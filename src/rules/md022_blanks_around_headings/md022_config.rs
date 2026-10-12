use crate::types::NonNegativeUsize;

/// Represents the blank-line requirement for a heading level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingBlankRequirement {
    /// Require an exact minimum count of blank lines (0 or greater).
    /// Stored as `NonNegativeUsize` to forbid negative values while allowing `0`.
    Limited(NonNegativeUsize),
}

impl HeadingBlankRequirement {
    pub(crate) fn limited(value: usize) -> Self {
        HeadingBlankRequirement::Limited(NonNegativeUsize::new(value))
    }

    pub(crate) fn required_count(&self) -> Option<usize> {
        match self {
            HeadingBlankRequirement::Limited(value) => Some(value.get()),
        }
    }
}

impl Default for HeadingBlankRequirement {
    fn default() -> Self {
        HeadingBlankRequirement::limited(1)
    }
}

/// Configuration for blank lines that can be specified per heading level or globally
#[derive(Debug, Clone, PartialEq)]
pub enum HeadingLevelConfig {
    /// Same value for all heading levels
    Scalar(HeadingBlankRequirement),
}

impl HeadingLevelConfig {
    /// Get the configured value for a specific heading level (1-6)
    pub(crate) fn get_for_level(&self, _level: usize) -> &HeadingBlankRequirement {
        match self {
            HeadingLevelConfig::Scalar(value) => value,
        }
    }
}

impl Default for HeadingLevelConfig {
    fn default() -> Self {
        HeadingLevelConfig::Scalar(HeadingBlankRequirement::default())
    }
}

/// Configuration for MD022 (Headings should be surrounded by blank lines)
#[derive(Debug, Clone, PartialEq)]
pub struct MD022Config {
    /// Number of blank lines required above headings (default: 1 for all levels)
    /// Can be a single integer (applies to all levels) or an array of 6 integers (one per level 1-6)
    pub(crate) lines_above: HeadingLevelConfig,

    /// Number of blank lines required below headings (default: 1 for all levels)
    /// Can be a single integer (applies to all levels) or an array of 6 integers (one per level 1-6)
    pub(crate) lines_below: HeadingLevelConfig,

    /// Whether the first heading can be at the start of the document (default: true)
    pub(crate) allowed_at_start: bool,
}

fn default_lines_above() -> HeadingLevelConfig {
    HeadingLevelConfig::default()
}

fn default_lines_below() -> HeadingLevelConfig {
    HeadingLevelConfig::default()
}

fn default_allowed_at_start() -> bool {
    true
}

impl Default for MD022Config {
    fn default() -> Self {
        Self {
            lines_above: default_lines_above(),
            lines_below: default_lines_below(),
            allowed_at_start: default_allowed_at_start(),
        }
    }
}
