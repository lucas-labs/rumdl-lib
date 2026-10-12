use crate::rules::code_fence_utils::CodeFenceStyle;

#[derive(Debug, Clone, PartialEq)]
pub struct MD048Config {
    pub(crate) style: CodeFenceStyle,
}

impl Default for MD048Config {
    fn default() -> Self {
        Self { style: default_style() }
    }
}

fn default_style() -> CodeFenceStyle {
    CodeFenceStyle::Consistent
}
