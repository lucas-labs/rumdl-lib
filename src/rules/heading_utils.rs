use std::fmt;
use std::str::FromStr;

/// Represents different styles of Markdown headings
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub enum HeadingStyle {
    Atx,       // # Heading
    AtxClosed, // # Heading #
    Setext1,   // Heading
    // =======
    Setext2, // Heading
    // -------
    Consistent,          // For maintaining consistency with the first found header style
    SetextWithAtx,       // Setext for h1/h2, ATX for h3-h6
    SetextWithAtxClosed, // Setext for h1/h2, ATX closed for h3-h6
}

impl fmt::Display for HeadingStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            HeadingStyle::Atx => "atx",
            HeadingStyle::AtxClosed => "atx-closed",
            HeadingStyle::Setext1 => "setext1",
            HeadingStyle::Setext2 => "setext2",
            HeadingStyle::Consistent => "consistent",
            HeadingStyle::SetextWithAtx => "setext-with-atx",
            HeadingStyle::SetextWithAtxClosed => "setext-with-atx-closed",
        };
        write!(f, "{s}")
    }
}

impl FromStr for HeadingStyle {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_ascii_lowercase().replace('-', "_");
        match normalized.as_str() {
            "atx" => Ok(HeadingStyle::Atx),
            "atx_closed" => Ok(HeadingStyle::AtxClosed),
            "setext1" | "setext" => Ok(HeadingStyle::Setext1),
            "setext2" => Ok(HeadingStyle::Setext2),
            "consistent" => Ok(HeadingStyle::Consistent),
            "setext_with_atx" => Ok(HeadingStyle::SetextWithAtx),
            "setext_with_atx_closed" => Ok(HeadingStyle::SetextWithAtxClosed),
            _ => Err(()),
        }
    }
}

/// Utility functions for working with Markdown headings
pub struct HeadingUtils;

impl HeadingUtils {
    /// Convert a heading to a different style
    pub(crate) fn convert_heading_style(text_content: &str, level: u32, style: HeadingStyle) -> String {
        // Validate heading level
        let level = level.clamp(1, 6);

        if text_content.trim().is_empty() {
            // Empty headings: ATX can be just `##`, Setext requires text so return empty
            return match style {
                HeadingStyle::Atx => "#".repeat(level as usize),
                HeadingStyle::AtxClosed => {
                    let hashes = "#".repeat(level as usize);
                    format!("{hashes} {hashes}")
                }
                HeadingStyle::Setext1 | HeadingStyle::Setext2 => String::new(),
                // These are meta-styles resolved before calling this function
                HeadingStyle::Consistent | HeadingStyle::SetextWithAtx | HeadingStyle::SetextWithAtxClosed => {
                    "#".repeat(level as usize)
                }
            };
        }

        let indentation = text_content
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect::<String>();
        let text_content = text_content.trim();

        match style {
            HeadingStyle::Atx => {
                format!("{}{} {}", indentation, "#".repeat(level as usize), text_content)
            }
            HeadingStyle::AtxClosed => {
                format!(
                    "{}{} {} {}",
                    indentation,
                    "#".repeat(level as usize),
                    text_content,
                    "#".repeat(level as usize)
                )
            }
            HeadingStyle::Setext1 | HeadingStyle::Setext2 => {
                if level > 2 {
                    // Fall back to ATX style for levels > 2
                    format!("{}{} {}", indentation, "#".repeat(level as usize), text_content)
                } else {
                    let underline_char = if level == 1 || style == HeadingStyle::Setext1 {
                        '='
                    } else {
                        '-'
                    };
                    let visible_length = text_content.chars().count();
                    let underline_length = visible_length.max(1); // Ensure at least 1 underline char
                    format!(
                        "{}{}\n{}{}",
                        indentation,
                        text_content,
                        indentation,
                        underline_char.to_string().repeat(underline_length)
                    )
                }
            }
            HeadingStyle::Consistent => {
                // For Consistent style, default to ATX as it's the most commonly used
                format!("{}{} {}", indentation, "#".repeat(level as usize), text_content)
            }
            HeadingStyle::SetextWithAtx => {
                if level <= 2 {
                    // Use Setext for h1/h2
                    let underline_char = if level == 1 { '=' } else { '-' };
                    let visible_length = text_content.chars().count();
                    let underline_length = visible_length.max(1);
                    format!(
                        "{}{}\n{}{}",
                        indentation,
                        text_content,
                        indentation,
                        underline_char.to_string().repeat(underline_length)
                    )
                } else {
                    // Use ATX for h3-h6
                    format!("{}{} {}", indentation, "#".repeat(level as usize), text_content)
                }
            }
            HeadingStyle::SetextWithAtxClosed => {
                if level <= 2 {
                    // Use Setext for h1/h2
                    let underline_char = if level == 1 { '=' } else { '-' };
                    let visible_length = text_content.chars().count();
                    let underline_length = visible_length.max(1);
                    format!(
                        "{}{}\n{}{}",
                        indentation,
                        text_content,
                        indentation,
                        underline_char.to_string().repeat(underline_length)
                    )
                } else {
                    // Use ATX closed for h3-h6
                    format!(
                        "{}{} {} {}",
                        indentation,
                        "#".repeat(level as usize),
                        text_content,
                        "#".repeat(level as usize)
                    )
                }
            }
        }
    }
}
