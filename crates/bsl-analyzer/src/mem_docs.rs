//! In-memory document tracking for LSP.
//!
//! This module provides `MemDocs`, which tracks the state of documents
//! opened in the editor. It handles:
//! - Document lifecycle (open, change, close)
//! - Incremental text changes
//! - Version tracking

use std::sync::Arc;

use line_index::{LineCol, LineIndex, TextSize};
use lsp_types::{TextDocumentContentChangeEvent, Url};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;

/// In-memory document storage.
///
/// Tracks documents that are currently opened in the editor.
/// Each document has:
/// - Full text content
/// - Version number (for change tracking)
/// - LineIndex for position conversions
#[derive(Debug, Clone, Default)]
pub struct MemDocs {
    docs: Arc<RwLock<FxHashMap<Url, DocumentData>>>,
}

/// Data for a single document.
#[derive(Debug, Clone)]
struct DocumentData {
    /// Full text of the document.
    text: String,
    /// LSP version number (incremented on each change).
    version: i32,
    /// Line index for offset <-> line/col conversions.
    line_index: LineIndex,
}

impl MemDocs {
    /// Creates a new empty MemDocs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or updates a document.
    ///
    /// This is called on `textDocument/didOpen`.
    pub fn insert(&mut self, uri: Url, text: String, version: i32) {
        let line_index = LineIndex::new(&text);
        let data = DocumentData { text, version, line_index };
        self.docs.write().insert(uri, data);
    }

    /// Applies incremental changes to a document.
    ///
    /// This is called on `textDocument/didChange`.
    /// Supports both full document sync and incremental changes.
    pub fn update(&mut self, uri: &Url, changes: Vec<TextDocumentContentChangeEvent>) {
        let mut docs = self.docs.write();

        if let Some(data) = docs.get_mut(uri) {
            for change in changes {
                if let Some(range) = change.range {
                    // Incremental change
                    let start_offset = data
                        .line_index
                        .offset(LineCol { line: range.start.line, col: range.start.character })
                        .unwrap_or(TextSize::from(0));

                    let end_offset = data
                        .line_index
                        .offset(LineCol { line: range.end.line, col: range.end.character })
                        .unwrap_or(TextSize::from(data.text.len() as u32));

                    let start = usize::from(start_offset);
                    let end = usize::from(end_offset);

                    // Apply the change
                    data.text.replace_range(start..end, &change.text);
                } else {
                    // Full document sync
                    data.text = change.text;
                }
            }

            // Rebuild line index after changes
            data.line_index = LineIndex::new(&data.text);
            data.version += 1;
        } else {
            tracing::warn!("Attempted to update non-existent document: {}", uri);
        }
    }

    /// Removes a document.
    ///
    /// This is called on `textDocument/didClose`.
    pub fn remove(&mut self, uri: &Url) {
        self.docs.write().remove(uri);
    }

    /// Gets the full text of a document.
    pub fn get(&self, uri: &Url) -> Option<String> {
        self.docs.read().get(uri).map(|data| data.text.clone())
    }

    /// Gets the version of a document.
    pub fn get_version(&self, uri: &Url) -> Option<i32> {
        self.docs.read().get(uri).map(|data| data.version)
    }

    /// Gets the line index for a document.
    pub fn get_line_index(&self, uri: &Url) -> Option<LineIndex> {
        self.docs.read().get(uri).map(|data| data.line_index.clone())
    }

    /// Checks if a document is tracked.
    pub fn contains(&self, uri: &Url) -> bool {
        self.docs.read().contains_key(uri)
    }

    /// Gets the number of tracked documents.
    pub fn len(&self) -> usize {
        self.docs.read().len()
    }

    /// Checks if there are no tracked documents.
    pub fn is_empty(&self) -> bool {
        self.docs.read().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_get() {
        let mut mem_docs = MemDocs::new();
        let uri = Url::parse("file:///test.bsl").unwrap();
        let text = "Процедура Тест()\nКонецПроцедуры".to_string();

        mem_docs.insert(uri.clone(), text.clone(), 1);

        assert_eq!(mem_docs.get(&uri), Some(text));
        assert_eq!(mem_docs.get_version(&uri), Some(1));
        assert!(mem_docs.contains(&uri));
    }

    #[test]
    fn test_update_full() {
        let mut mem_docs = MemDocs::new();
        let uri = Url::parse("file:///test.bsl").unwrap();

        mem_docs.insert(uri.clone(), "old text".to_string(), 1);

        let changes = vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "new text".to_string(),
        }];

        mem_docs.update(&uri, changes);

        assert_eq!(mem_docs.get(&uri), Some("new text".to_string()));
        assert_eq!(mem_docs.get_version(&uri), Some(2));
    }

    #[test]
    fn test_update_incremental() {
        let mut mem_docs = MemDocs::new();
        let uri = Url::parse("file:///test.bsl").unwrap();

        mem_docs.insert(uri.clone(), "hello world".to_string(), 1);

        // Replace "world" with "rust"
        let changes = vec![TextDocumentContentChangeEvent {
            range: Some(lsp_types::Range {
                start: lsp_types::Position { line: 0, character: 6 },
                end: lsp_types::Position { line: 0, character: 11 },
            }),
            range_length: Some(5),
            text: "rust".to_string(),
        }];

        mem_docs.update(&uri, changes);

        assert_eq!(mem_docs.get(&uri), Some("hello rust".to_string()));
        assert_eq!(mem_docs.get_version(&uri), Some(2));
    }

    #[test]
    fn test_remove() {
        let mut mem_docs = MemDocs::new();
        let uri = Url::parse("file:///test.bsl").unwrap();

        mem_docs.insert(uri.clone(), "test".to_string(), 1);
        assert!(mem_docs.contains(&uri));

        mem_docs.remove(&uri);
        assert!(!mem_docs.contains(&uri));
        assert_eq!(mem_docs.get(&uri), None);
    }

    #[test]
    fn test_line_index() {
        let mut mem_docs = MemDocs::new();
        let uri = Url::parse("file:///test.bsl").unwrap();
        let text = "line1\nline2\nline3".to_string();

        mem_docs.insert(uri.clone(), text, 1);

        let line_index = mem_docs.get_line_index(&uri).unwrap();

        // Test line_col for offset 6 ('l' in "line2")
        let pos = line_index.line_col(TextSize::from(6));
        assert_eq!(pos, LineCol { line: 1, col: 0 });
    }
}
