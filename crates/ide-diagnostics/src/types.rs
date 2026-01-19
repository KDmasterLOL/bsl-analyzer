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

    /// Start line (0-based).
    pub start_line: usize,

    /// Start column (0-based).
    pub start_column: usize,

    /// End line (0-based).
    pub end_line: usize,

    /// End column (0-based).
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
    pub fn to_output(&self, file_text: &str) -> DiagnosticOutput {
        use line_index::LineIndex;

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

        DiagnosticOutput {
            code: self.code.as_str().to_string(),
            message: self.message.clone(),
            severity: self.severity.as_str().to_string(),
            start_line: start.line as usize,
            start_column: start.col as usize,
            end_line: end.line as usize,
            end_column: end.col as usize,
            tags: self.tags.iter().map(|tag| tag.as_str().to_string()).collect(),
        }
    }
}

/// Diagnostic severity.
/// Matches bsl-language-server severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Blocker,     // Highest severity (Java: BLOCKER)
    Critical,    // Critical issues (Java: CRITICAL)
    Major,       // Significant issues (Java: MAJOR)
    Error,       // General errors
    Warning,     // Minor issues (Java: MINOR)
    Information, // Informational (Java: INFO)
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
