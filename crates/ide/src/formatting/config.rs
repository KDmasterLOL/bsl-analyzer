//! Formatting configuration.

/// Configuration for BSL code formatting.
#[derive(Debug, Clone)]
pub struct FormattingConfig {
    /// Use tabs for indentation (true) or spaces (false).
    pub use_tabs: bool,

    /// Size of one indentation level.
    /// If `use_tabs` is true, this is the number of tabs (usually 1).
    /// If `use_tabs` is false, this is the number of spaces (usually 4).
    pub indent_size: u32,

    /// Additional indent for continuation lines (multi-line expressions).
    pub continuation_indent: u32,

    /// Add space after comma in argument lists.
    pub space_after_comma: bool,

    /// Add spaces around assignment operator (=).
    pub space_around_assignment: bool,

    /// Add spaces around binary operators (+, -, *, /, etc.).
    pub space_around_binary_ops: bool,

    /// Remove trailing whitespace from lines.
    pub trim_trailing_whitespace: bool,

    /// Ensure file ends with a newline.
    pub insert_final_newline: bool,
}

impl Default for FormattingConfig {
    fn default() -> Self {
        Self {
            use_tabs: true,
            indent_size: 1,
            continuation_indent: 1,
            space_after_comma: true,
            space_around_assignment: true,
            space_around_binary_ops: true,
            trim_trailing_whitespace: true,
            insert_final_newline: true,
        }
    }
}

impl FormattingConfig {
    /// Creates a new FormattingConfig with spaces instead of tabs.
    pub fn with_spaces(spaces_per_indent: u32) -> Self {
        Self { use_tabs: false, indent_size: spaces_per_indent, ..Default::default() }
    }

    /// Returns the string used for one indentation level.
    pub fn indent_str(&self) -> String {
        if self.use_tabs {
            "\t".repeat(self.indent_size as usize)
        } else {
            " ".repeat(self.indent_size as usize)
        }
    }

    /// Returns indentation string for the given level.
    pub fn indent_for_level(&self, level: u32) -> String {
        if self.use_tabs {
            "\t".repeat(level as usize)
        } else {
            " ".repeat((level * self.indent_size) as usize)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = FormattingConfig::default();
        assert!(config.use_tabs);
        assert_eq!(config.indent_size, 1);
        assert_eq!(config.indent_str(), "\t");
    }

    #[test]
    fn test_spaces_config() {
        let config = FormattingConfig::with_spaces(4);
        assert!(!config.use_tabs);
        assert_eq!(config.indent_size, 4);
        assert_eq!(config.indent_str(), "    ");
        assert_eq!(config.indent_for_level(2), "        ");
    }
}
