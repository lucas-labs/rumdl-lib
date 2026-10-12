#[derive(Debug, Clone, PartialEq)]
pub struct MD055Config {
    pub(crate) style: String,
}

impl Default for MD055Config {
    fn default() -> Self {
        Self { style: default_style() }
    }
}

fn default_style() -> String {
    "consistent".to_string()
}
