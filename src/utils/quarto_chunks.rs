//! Parser for Quarto / RMarkdown executable code chunk metadata.
//!
//! Two label sources, both supported:
//! 1. Inline info string: ` ```{r, label="setup", echo=FALSE} `
//! 2. Hashpipe chunk options inside the block body: `#| label: setup`
//!
//! The inline form supports three shapes:
//! - Bare label as the first positional argument: `{r setup}` or `{r several words}`
//!   (multiple bare words before any `key=value` are treated as a whitespace-
//!   separated label; this is also how the linter detects spaces in labels).
//! - Explicit `label=value`: `{r, label=setup}` or `{r, label="my label"}`.
//! - Mixed forms like `{r setup, echo=FALSE}`.
//!
//! The grammar reflects how knitr/Quarto themselves parse chunk headers. We do
//! not aim for full knitr fidelity; the goal is to recognise the patterns that
//! drive the two lint rules using this helper (MD078, MD079).
