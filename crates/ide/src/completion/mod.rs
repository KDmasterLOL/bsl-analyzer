//! Code completion for BSL and SDBL.
//!
//! This module provides completion suggestions for:
//! - SDBL queries (FROM clause, metadata objects)
//! - BSL code (variables, functions, etc.) - TODO

mod sdbl_completion;

use ide_db::RootDatabase;
use std::path::PathBuf;
use syntax::TextSize;
use vfs::FileId;

/// Position in file for completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionPosition {
    pub file_id: FileId,
    pub offset: TextSize,
    pub workspace_root: Option<PathBuf>,
}

/// A single completion item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// Label shown to user (e.g., "Валюты")
    pub label: String,

    /// Detail shown after label (e.g., "Справочник")
    pub detail: Option<String>,

    /// Kind of completion (Class, Module, Field, etc.)
    pub kind: CompletionItemKind,

    /// Text to insert (usually same as label)
    pub insert_text: String,

    /// Documentation (optional)
    pub documentation: Option<String>,
}

/// Kind of completion item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionItemKind {
    /// Metadata object type (Справочник, Документ)
    MdoType,
    /// Metadata object instance (Валюты, Контрагенты)
    MdoObject,
    /// Field/property
    Field,
    /// Function
    Function,
    /// Keyword
    Keyword,
}

/// Main completion entry point.
///
/// Returns completion suggestions at the given position.
///
/// # Arguments
///
/// * `db` - Root database with file contents and metadata
/// * `position` - File and offset where completion is requested
///
/// # Returns
///
/// List of completion items appropriate for the position.
pub fn completions(db: &dyn RootDatabase, position: CompletionPosition) -> Vec<CompletionItem> {
    let _p = tracing::info_span!("completions", ?position).entered();

    // Try SDBL completion first
    if let Some(items) = sdbl_completion::sdbl_completions(db, position) {
        tracing::debug!(items = items.len(), "returning SDBL completions");
        return items;
    }

    // TODO: BSL completion (variables, functions, etc.)
    tracing::trace!("no completions available");

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_item_creation() {
        let item = CompletionItem {
            label: "Валюты".to_string(),
            detail: Some("Справочник".to_string()),
            kind: CompletionItemKind::MdoObject,
            insert_text: "Валюты".to_string(),
            documentation: None,
        };

        assert_eq!(item.label, "Валюты");
        assert_eq!(item.detail, Some("Справочник".to_string()));
        assert_eq!(item.kind, CompletionItemKind::MdoObject);
    }
}
