//! Line-level tokens the Markdown with Gherkin flavor inherits from Gherkin.
//!
//! Gherkin is line-oriented: its parser classifies Markdown content one line at
//! a time. MDG embeds three of its recognized shapes in Markdown, where each
//! one collides with a Markdown construct that would otherwise rewrite or
//! delete it:
//!
//! - a Data Table or Examples row, matched as `/^\s\s\s?\s?\s?\|/`, whose
//!   indentation overlaps the 4-column indented-code threshold;
//! - a tag line, spelled in MDG as backtick-wrapped `@tags` so Gherkin's bare
//!   `@tag` survives as Markdown, which binds to the structure on the very next
//!   line;
//! - a structure heading, `Keyword: name`, whose keyword is a dialect term that
//!   names a structure only when spelled exactly.
//!
//! Each rule decides for itself what to do about a token; this module only says
//! what Gherkin sees.
