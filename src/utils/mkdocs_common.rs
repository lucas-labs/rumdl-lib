//! Common utilities and constants for MkDocs pattern detection
//!
//! This module provides shared functionality used across all MkDocs feature
//! detection modules to reduce code duplication and improve maintainability.

/// Extract indentation from a line (counts spaces and tabs)
pub(crate) fn get_line_indent(line: &str) -> usize {
    line.chars()
        .take_while(|&c| c == ' ' || c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 }) // Treat tabs as 4 spaces
        .sum()
}
