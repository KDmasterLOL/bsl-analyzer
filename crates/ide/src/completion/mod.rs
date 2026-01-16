//! Code completion for BSL and SDBL.
//!
//! This module provides completion suggestions for:
//! - SDBL queries (FROM clause, metadata objects)
//! - Platform methods (after DOT operator)
//! - BSL code (variables, functions, etc.) - TODO

mod bsl_completion;
mod platform_completion;
mod sdbl;

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

    /// Sort text (for ordering in completion list)
    pub sort_text: Option<String>,

    /// Filter text (for filtering when user types)
    pub filter_text: Option<String>,
}

impl CompletionItem {
    /// Create a simple completion item with defaults for optional fields.
    pub fn simple(label: String, kind: CompletionItemKind, insert_text: String) -> Self {
        Self {
            label,
            kind,
            detail: None,
            documentation: None,
            insert_text,
            sort_text: None,
            filter_text: None,
        }
    }
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
    /// Method (platform or user-defined)
    Method,
    /// Keyword
    Keyword,
    /// Constant (ПустаяСсылка, predefined items)
    Constant,
    /// Enumeration member (enum value)
    EnumMember,
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

    // Try SDBL completion first (new Clean Architecture implementation)
    if let Some(items) = sdbl::sdbl_completions(db, position.clone()) {
        tracing::debug!(items = items.len(), "returning SDBL completions");
        return items;
    }

    // Try platform method completion
    if let Some(items) = platform_completion::platform_completions(db, position.clone()) {
        tracing::debug!(items = items.len(), "returning platform method completions");
        return items;
    }

    // Try BSL completion (global functions, keywords, etc.)
    if let Some(items) = bsl_completion::bsl_completions(db, position.clone()) {
        tracing::debug!(items = items.len(), "returning BSL completions");
        return items;
    }

    // TODO: BSL user-defined symbols (variables, local functions, etc.)
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
            sort_text: None,
            filter_text: None,
        };

        assert_eq!(item.label, "Валюты");
        assert_eq!(item.detail, Some("Справочник".to_string()));
        assert_eq!(item.kind, CompletionItemKind::MdoObject);
    }

    #[test]
    fn test_completion_item_simple() {
        let item = CompletionItem::simple(
            "Test".to_string(),
            CompletionItemKind::Field,
            "Test".to_string(),
        );

        assert_eq!(item.label, "Test");
        assert_eq!(item.kind, CompletionItemKind::Field);
        assert_eq!(item.detail, None);
        assert_eq!(item.sort_text, None);
        assert_eq!(item.filter_text, None);
    }
}
