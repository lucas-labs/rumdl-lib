use crate::rules::emphasis_style::EmphasisStyle;

#[derive(Debug, Clone, PartialEq)]
pub struct MD049Config {
    pub(crate) style: EmphasisStyle,
}

impl Default for MD049Config {
    fn default() -> Self {
        Self { style: default_style() }
    }
}

fn default_style() -> EmphasisStyle {
    EmphasisStyle::Consistent
}
