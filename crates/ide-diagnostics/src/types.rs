use crate::DiagnosticCode;
use ide_db::TextRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
    pub severity: Severity,
    pub range: TextRange,
    pub tags: Vec<DiagnosticTag>,
    pub fixes: Vec<Fix>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiagnosticOutput {
    pub code: String,

    pub message: String,

    pub severity: String,

    pub start_line: usize,

    pub start_column: usize,

    pub end_line: usize,

    pub end_column: usize,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl Diagnostic {
    pub fn to_output(&self, file_text: &str) -> DiagnosticOutput {
        let line_index = line_index::LineIndex::new(file_text);
        self.to_output_with_index(file_text, &line_index)
    }

    pub fn to_output_with_index(
        &self,
        file_text: &str,
        line_index: &line_index::LineIndex,
    ) -> DiagnosticOutput {
        use line_index::LineIndexExt;

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

        let start = line_index.line_col(self.range.start());
        let end = line_index.line_col(self.range.end());

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Blocker,
    Critical,
    Major,
    Error,
    Warning,
    Information,
    Hint,
}

impl Severity {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticTag {
    Unnecessary,
    Deprecated,
}

impl DiagnosticTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unnecessary => "Unnecessary",
            Self::Deprecated => "Deprecated",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub label: String,
    pub edits: Vec<TextEdit>,
    /// Whether this fix may be applied unattended as part of a `source.fixAll`
    /// batch. Safe means the edit is deterministic, semantics-preserving, and
    /// reference-safe when applied together with every other occurrence in the
    /// file. Fixes that offer the user a choice, or that rename an identifier
    /// whose other references the batch cannot reach, stay opt-in quick fixes.
    pub safe_for_fix_all: bool,
}

impl Fix {
    /// A fix eligible for `source.fixAll`: deterministic, semantics-preserving,
    /// and reference-safe as an unattended batch edit.
    pub fn safe(label: impl Into<String>, edits: Vec<TextEdit>) -> Self {
        Self { label: label.into(), edits, safe_for_fix_all: true }
    }

    /// A fix that stays an explicit, opt-in quick fix and is excluded from
    /// `source.fixAll` — it offers a choice or renames a symbol whose remaining
    /// references a file-local batch cannot keep in sync.
    pub fn manual(label: impl Into<String>, edits: Vec<TextEdit>) -> Self {
        Self { label: label.into(), edits, safe_for_fix_all: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}
