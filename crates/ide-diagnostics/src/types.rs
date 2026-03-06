//! Diagnostic types and structs.

use crate::DiagnosticCode;
use ide_db::TextRange;

/// A diagnostic produced by the analyzer (internal representation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
    pub severity: Severity,
    pub range: TextRange,
    pub tags: Vec<DiagnosticTag>,
    pub fixes: Vec<Fix>,
}

/// Diagnostic output DTO for external consumption (reports, CLI, etc.).
///
/// This is the public-facing format with line/column positions instead of byte offsets.
/// Used by:
/// - Streaming mode results
/// - Reporter system (JSON, SARIF, console)
/// - CLI output
///
/// ## Architecture
/// This follows the DTO (Data Transfer Object) pattern:
/// - Internal representation: `Diagnostic` with `TextRange` (byte offsets)
/// - External representation: `DiagnosticOutput` with line/column positions
///
/// Conversion happens in the domain layer via `Diagnostic::to_output()`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiagnosticOutput {
    /// Diagnostic code (e.g., "LineLength", "BadWords").
    pub code: String,

    /// Human-readable message.
    pub message: String,

    /// Severity level (e.g., "Warning", "Error").
    pub severity: String,

    /// Start line (0-based, LSP compatible).
    pub start_line: usize,

    /// Start column (0-based, LSP compatible).
    pub start_column: usize,

    /// End line (0-based, LSP compatible).
    pub end_line: usize,

    /// End column (0-based, LSP compatible).
    pub end_column: usize,

    /// Diagnostic tags (e.g., "Unnecessary", "Deprecated").
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl Diagnostic {
    /// Convert to output DTO with line/column positions.
    ///
    /// This method performs the conversion from internal representation (TextRange)
    /// to external output format (line/column). Requires file text to build LineIndex.
    ///
    /// **Important**: Column positions are converted from byte offsets to character positions.
    /// This is necessary because external tools (SonarQube, editors) expect character positions,
    /// while internal TextRange uses byte offsets. For Cyrillic text, 1 char = 2 bytes in UTF-8.
    pub fn to_output(&self, file_text: &str) -> DiagnosticOutput {
        use line_index::{LineIndex, LineIndexExt};

        let file_len = file_text.len();
        let range_start: u32 = self.range.start().into();
        let range_end: u32 = self.range.end().into();

        if range_start as usize > file_len || range_end as usize > file_len {
            panic!(
                "BUG in diagnostic handler '{}': Invalid TextRange [{}, {}) exceeds file length {}\n\
                 Diagnostic message: '{}'\n\
                 This is a bug in the diagnostic handler that created this Diagnostic.\n\
                 The handler must ensure TextRange is within file bounds.",
                self.code.as_str(),
                range_start,
                range_end,
                file_len,
                self.message
            );
        }

        let line_index = LineIndex::new(file_text);

        let start = line_index.line_col(self.range.start());
        let end = line_index.line_col(self.range.end());

        // Convert byte columns to character columns for external tools.
        // Internal TextRange uses byte offsets, but SonarQube/editors expect character positions.
        let start_char_col = line_index.byte_col_to_char_col(file_text, start.line, start.col);
        let end_char_col = line_index.byte_col_to_char_col(file_text, end.line, end.col);

        DiagnosticOutput {
            code: self.code.as_str().to_string(),
            message: self.message.clone(),
            severity: self.severity.as_str().to_string(),
            start_line: start.line as usize,
            start_column: start_char_col as usize,
            end_line: end.line as usize,
            end_column: end_char_col as usize,
            tags: self.tags.iter().map(|tag| tag.as_str().to_string()).collect(),
        }
    }
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Blocker,     // Highest severity
    Critical,    // Critical issues
    Major,       // Significant issues
    Error,       // General errors
    Warning,     // Minor issues
    Information, // Informational
    Hint,        // Lowest severity
}

impl Severity {
    /// Returns string representation for output.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Blocker => "Blocker",
            Self::Critical => "Critical",
            Self::Major => "Major",
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::Information => "Information",
            Self::Hint => "Hint",
        }
    }
}

/// Diagnostic tag for special handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticTag {
    Unnecessary,
    Deprecated,
}

impl DiagnosticTag {
    /// Returns string representation for output.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unnecessary => "Unnecessary",
            Self::Deprecated => "Deprecated",
        }
    }
}

/// A quick fix for a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub label: String,
    pub edits: Vec<TextEdit>,
}

/// A text edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}
