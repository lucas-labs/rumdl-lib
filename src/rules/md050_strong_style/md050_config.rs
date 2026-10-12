use crate::rules::strong_style::StrongStyle;

#[derive(Debug, Clone, PartialEq)]
pub struct MD050Config {
    pub(crate) style: StrongStyle,
}

impl Default for MD050Config {
    fn default() -> Self {
        Self { style: default_style() }
    }
}

fn default_style() -> StrongStyle {
    StrongStyle::Consistent
}
