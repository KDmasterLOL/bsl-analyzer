//! Code assists (refactorings) for bsl-analyzer.
//!
//! This crate implements quick fixes and refactorings.

use ide_db::TextRange;

/// An assist (code action).
#[derive(Debug, Clone)]
pub struct Assist {
    pub id: AssistId,
    pub label: String,
    pub group: Option<String>,
    pub source_change: SourceChange,
}

/// Unique identifier for an assist.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssistId(pub &'static str);

/// A source change with edits.
#[derive(Debug, Clone)]
pub struct SourceChange {
    pub edits: Vec<FileEdit>,
}

/// Edits for a single file.
#[derive(Debug, Clone)]
pub struct FileEdit {
    pub file_id: vfs::FileId,
    pub edits: Vec<TextEdit>,
}

/// A single text edit.
#[derive(Debug, Clone)]
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}

// TODO: Implement assists
