use regex::Regex;
use std::sync::LazyLock;

// Standard front matter delimiter (three dashes)
static STANDARD_FRONT_MATTER_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^---\s*$").unwrap());
static STANDARD_FRONT_MATTER_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^---\s*$").unwrap());

// TOML front matter delimiter (three plus signs)
static TOML_FRONT_MATTER_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\+\+\+\s*$").unwrap());
static TOML_FRONT_MATTER_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\+\+\+\s*$").unwrap());

// JSON front matter delimiter (curly braces)
static JSON_FRONT_MATTER_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\{\s*$").unwrap());
static JSON_FRONT_MATTER_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\}\s*$").unwrap());

// Common malformed front matter (dash space dash dash)
static MALFORMED_FRONT_MATTER_START1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^- --\s*$").unwrap());
static MALFORMED_FRONT_MATTER_END1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^- --\s*$").unwrap());

// Alternate malformed front matter (dash dash space dash)
static MALFORMED_FRONT_MATTER_START2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-- -\s*$").unwrap());
static MALFORMED_FRONT_MATTER_END2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-- -\s*$").unwrap());

// Front matter field pattern
static FRONT_MATTER_FIELD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([^:]+):\s*(.*)$").unwrap());

// TOML field pattern
static TOML_FIELD_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^([^=]+)\s*=\s*"?([^"]*)"?$"#).unwrap());

/// Represents the type of front matter found in a document
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FrontMatterType {
    /// YAML front matter (---)
    Yaml,
    /// TOML front matter (+++)
    Toml,
    /// JSON front matter ({})
    Json,
    /// Malformed front matter
    Malformed,
    /// No front matter
    None,
}

/// Utility functions for detecting and handling front matter in Markdown documents
pub struct FrontMatterUtils;

impl FrontMatterUtils {
    /// Check if a content contains front matter with a specific field
    pub(crate) fn has_front_matter_field(content: &str, field_prefix: &str) -> bool {
        let field_name = field_prefix.trim_end_matches(':');
        Self::get_front_matter_field_value(content, field_name).is_some()
    }

    /// Get the value of a specific front matter field
    pub(crate) fn get_front_matter_field_value<'a>(content: &'a str, field_name: &str) -> Option<&'a str> {
        let lines: Vec<&'a str> = content.lines().collect();
        if lines.len() < 3 {
            return None;
        }

        let front_matter_type = Self::detect_front_matter_type(content);
        if front_matter_type == FrontMatterType::None {
            return None;
        }

        let front_matter = Self::extract_front_matter(content);
        for line in front_matter {
            let line = line.trim();
            match front_matter_type {
                FrontMatterType::Toml => {
                    // Handle TOML-style fields (key = value)
                    if let Some(captures) = TOML_FIELD_PATTERN.captures(line) {
                        let key = captures.get(1).unwrap().as_str().trim();
                        if key == field_name {
                            let value = captures.get(2).unwrap().as_str();
                            return Some(value);
                        }
                    }
                }
                _ => {
                    // Handle YAML/JSON-style fields (key: value)
                    if let Some(captures) = FRONT_MATTER_FIELD.captures(line) {
                        let mut key = captures.get(1).unwrap().as_str().trim();

                        // Strip quotes from the key if present (for JSON-style fields in any format)
                        if key.starts_with('"') && key.ends_with('"') && key.len() >= 2 {
                            key = &key[1..key.len() - 1];
                        }

                        if key == field_name {
                            let value = captures.get(2).unwrap().as_str().trim();
                            // Strip quotes if present
                            if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                                return Some(&value[1..value.len() - 1]);
                            }
                            return Some(value);
                        }
                    }
                }
            }
        }

        None
    }

    /// Extract the front matter content as a vector of lines
    pub(crate) fn extract_front_matter<'a>(content: &'a str) -> Vec<&'a str> {
        let lines: Vec<&'a str> = content.lines().collect();
        if lines.len() < 3 {
            return Vec::new();
        }

        let front_matter_type = Self::detect_front_matter_type(content);
        if front_matter_type == FrontMatterType::None {
            return Vec::new();
        }

        let mut front_matter = Vec::new();
        let mut in_front_matter = false;

        for (i, line) in lines.iter().enumerate() {
            match front_matter_type {
                FrontMatterType::Yaml => {
                    if i == 0 && STANDARD_FRONT_MATTER_START.is_match(line) {
                        in_front_matter = true;
                        continue;
                    } else if STANDARD_FRONT_MATTER_END.is_match(line) && in_front_matter && i > 0 {
                        break;
                    }
                }
                FrontMatterType::Toml => {
                    if i == 0 && TOML_FRONT_MATTER_START.is_match(line) {
                        in_front_matter = true;
                        continue;
                    } else if TOML_FRONT_MATTER_END.is_match(line) && in_front_matter && i > 0 {
                        break;
                    }
                }
                FrontMatterType::Json => {
                    if i == 0 && JSON_FRONT_MATTER_START.is_match(line) {
                        in_front_matter = true;
                        continue;
                    } else if JSON_FRONT_MATTER_END.is_match(line) && in_front_matter && i > 0 {
                        break;
                    }
                }
                FrontMatterType::Malformed => {
                    if i == 0
                        && (MALFORMED_FRONT_MATTER_START1.is_match(line)
                            || MALFORMED_FRONT_MATTER_START2.is_match(line))
                    {
                        in_front_matter = true;
                        continue;
                    } else if (MALFORMED_FRONT_MATTER_END1.is_match(line) || MALFORMED_FRONT_MATTER_END2.is_match(line))
                        && in_front_matter
                        && i > 0
                    {
                        break;
                    }
                }
                FrontMatterType::None => break,
            }

            if in_front_matter {
                front_matter.push(*line);
            }
        }

        front_matter
    }

    /// Detect the type of front matter in the content
    pub(crate) fn detect_front_matter_type(content: &str) -> FrontMatterType {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return FrontMatterType::None;
        }

        let first_line = lines[0];

        if STANDARD_FRONT_MATTER_START.is_match(first_line) {
            // Check if there's a closing marker
            for line in lines.iter().skip(1) {
                if STANDARD_FRONT_MATTER_END.is_match(line) {
                    return FrontMatterType::Yaml;
                }
            }
        } else if TOML_FRONT_MATTER_START.is_match(first_line) {
            // Check if there's a closing marker
            for line in lines.iter().skip(1) {
                if TOML_FRONT_MATTER_END.is_match(line) {
                    return FrontMatterType::Toml;
                }
            }
        } else if JSON_FRONT_MATTER_START.is_match(first_line) {
            // Check if there's a closing marker
            for line in lines.iter().skip(1) {
                if JSON_FRONT_MATTER_END.is_match(line) {
                    return FrontMatterType::Json;
                }
            }
        } else if MALFORMED_FRONT_MATTER_START1.is_match(first_line)
            || MALFORMED_FRONT_MATTER_START2.is_match(first_line)
        {
            // Check if there's a closing marker
            for line in lines.iter().skip(1) {
                if MALFORMED_FRONT_MATTER_END1.is_match(line) || MALFORMED_FRONT_MATTER_END2.is_match(line) {
                    return FrontMatterType::Malformed;
                }
            }
        }

        FrontMatterType::None
    }

    /// Get the line number where front matter ends (or 0 if no front matter)
    ///
    /// Re-scans the whole content. `LintContext::new` calls this once per
    /// document and caches the result; everything downstream of a
    /// `LintContext` must read `ctx.front_matter_end_line()` instead
    /// (enforced via `disallowed-methods` in clippy.toml).
    pub(crate) fn get_front_matter_end_line(content: &str) -> usize {
        let lines: Vec<&str> = content.lines().collect();
        if lines.len() < 3 {
            return 0;
        }

        let front_matter_type = Self::detect_front_matter_type(content);
        if front_matter_type == FrontMatterType::None {
            return 0;
        }

        let mut in_front_matter = false;

        for (i, line) in lines.iter().enumerate() {
            match front_matter_type {
                FrontMatterType::Yaml => {
                    if i == 0 && STANDARD_FRONT_MATTER_START.is_match(line) {
                        in_front_matter = true;
                    } else if STANDARD_FRONT_MATTER_END.is_match(line) && in_front_matter && i > 0 {
                        return i + 1;
                    }
                }
                FrontMatterType::Toml => {
                    if i == 0 && TOML_FRONT_MATTER_START.is_match(line) {
                        in_front_matter = true;
                    } else if TOML_FRONT_MATTER_END.is_match(line) && in_front_matter && i > 0 {
                        return i + 1;
                    }
                }
                FrontMatterType::Json => {
                    if i == 0 && JSON_FRONT_MATTER_START.is_match(line) {
                        in_front_matter = true;
                    } else if JSON_FRONT_MATTER_END.is_match(line) && in_front_matter && i > 0 {
                        return i + 1;
                    }
                }
                FrontMatterType::Malformed => {
                    if i == 0
                        && (MALFORMED_FRONT_MATTER_START1.is_match(line)
                            || MALFORMED_FRONT_MATTER_START2.is_match(line))
                    {
                        in_front_matter = true;
                    } else if (MALFORMED_FRONT_MATTER_END1.is_match(line) || MALFORMED_FRONT_MATTER_END2.is_match(line))
                        && in_front_matter
                        && i > 0
                    {
                        return i + 1;
                    }
                }
                FrontMatterType::None => return 0,
            }
        }

        0
    }
}
