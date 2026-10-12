use crate::types::LineLength;
/// Controls how cell text is aligned within padded columns.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ColumnAlign {
    /// Use alignment indicators from delimiter row (`:---`, `:---:`, `---:`)
    #[default]
    Auto,
    /// Force all columns to left-align text
    Left,
    /// Force all columns to center text
    Center,
    /// Force all columns to right-align text
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MD060Config {
    pub enabled: bool,

    pub style: String,

    /// Maximum table width before auto-switching to compact mode.
    ///
    /// - `0` (default): Inherit from MD013's `line-length` setting
    /// - Non-zero: Explicit max width threshold
    ///
    /// When a table's aligned width would exceed this limit, MD060 automatically
    /// uses compact formatting instead (minimal padding) to prevent excessively
    /// long lines. This matches the behavior of Prettier's table formatting.
    ///
    /// # Examples
    ///
    /// ```toml
    /// [MD013]
    /// line-length = 100
    ///
    /// [MD060]
    /// style = "aligned"
    /// max-width = 0  # Uses MD013's line-length (100)
    /// ```
    ///
    /// ```toml
    /// [MD060]
    /// style = "aligned"
    /// max-width = 120  # Explicit threshold, independent of MD013
    /// ```
    pub max_width: LineLength,

    /// Controls how cell text is aligned within the padded column width.
    ///
    /// - `auto` (default): Use alignment indicators from delimiter row (`:---`, `:---:`, `---:`)
    /// - `left`: Force all columns to left-align text
    /// - `center`: Force all columns to center text
    /// - `right`: Force all columns to right-align text
    ///
    /// Only applies when `style = "aligned"` or `style = "aligned-no-space"`.
    ///
    /// # Examples
    ///
    /// ```toml
    /// [MD060]
    /// style = "aligned"
    /// column-align = "center"  # Center all cell text
    /// ```
    pub column_align: ColumnAlign,

    /// Controls alignment specifically for the header row.
    ///
    /// When set, overrides `column-align` for the header row only.
    /// If not set, falls back to `column-align`.
    ///
    /// # Examples
    ///
    /// ```toml
    /// [MD060]
    /// style = "aligned"
    /// column-align-header = "center"  # Center header text
    /// column-align-body = "left"      # Left-align body text
    /// ```
    pub column_align_header: Option<ColumnAlign>,

    /// Controls alignment specifically for body rows (non-header, non-delimiter).
    ///
    /// When set, overrides `column-align` for body rows only.
    /// If not set, falls back to `column-align`.
    ///
    /// # Examples
    ///
    /// ```toml
    /// [MD060]
    /// style = "aligned"
    /// column-align-header = "center"  # Center header text
    /// column-align-body = "left"      # Left-align body text
    /// ```
    pub column_align_body: Option<ColumnAlign>,

    /// Controls whether the last column in body rows is loosely formatted.
    ///
    /// - `false` (default): All columns padded to equal width across all rows.
    /// - `true`: The last column width is capped at the header text width.
    ///   Body cells shorter than the header are padded to the header width.
    ///   Body cells longer than the header extend beyond without padding.
    ///
    /// Only applies when `style = "aligned"` or `style = "aligned-no-space"`.
    ///
    /// # Examples
    ///
    /// ```toml
    /// [MD060]
    /// style = "aligned"
    /// loose-last-column = true
    /// ```
    pub loose_last_column: bool,

    /// Pads the delimiter row's dashes to match header column widths under
    /// `compact` and `tight` styles.
    ///
    /// - `false` (default): delimiter cells use the minimum dash count.
    /// - `true`: delimiter pipe positions align with header pipe positions;
    ///   body rows remain compact/tight and are not padded.
    ///
    /// No effect under `aligned` / `aligned-no-space` (those styles already
    /// align the delimiter row by construction), except when a table exceeds
    /// `max-width` and auto-compacts: the effective output style is then
    /// `compact`, so the delimiter row is aligned to the header column widths.
    ///
    /// Mirrors markdownlint MD060's `aligned_delimiter` option; the snake_case
    /// alias is accepted for cross-tool compatibility.
    pub aligned_delimiter: bool,
}

impl Default for MD060Config {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            style: default_style(),
            max_width: default_max_width(),
            column_align: ColumnAlign::Auto,
            column_align_header: None,
            column_align_body: None,
            loose_last_column: false,
            aligned_delimiter: false,
        }
    }
}

fn default_enabled() -> bool {
    false
}

fn default_style() -> String {
    "any".to_string()
}

fn default_max_width() -> LineLength {
    LineLength::from_const(0) // 0 = inherit from MD013
}
