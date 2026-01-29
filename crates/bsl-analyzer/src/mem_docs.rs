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
                    // CRITICAL: LSP uses UTF-16 code units for positions, not byte offsets!
                    // Must convert UTF-16 → byte offsets before using with Rust strings.
                    //
                    // For Cyrillic: "Процедура" = 9 UTF-16 code units, but 18 bytes UTF-8
                    // If we use UTF-16 positions directly, we'll try to split a multibyte char → panic

                    // Convert UTF-16 positions to byte offsets
                    let start_byte_col = data
                        .line_index
                        .utf16_col_to_byte_col(&data.text, range.start.line, range.start.character)
                        .unwrap_or(0);

                    let end_byte_col = data
                        .line_index
                        .utf16_col_to_byte_col(&data.text, range.end.line, range.end.character)
                        .unwrap_or_else(|| {
                            // Fallback: use line length if UTF-16 offset is out of bounds
                            data.line_index.line_len(range.end.line).unwrap_or(0)
                        });

                    // Convert line/col to absolute byte offsets
                    let start_offset = data
                        .line_index
                        .offset(LineCol { line: range.start.line, col: start_byte_col })
                        .unwrap_or(TextSize::from(0));

                    let end_offset = data
                        .line_index
                        .offset(LineCol { line: range.end.line, col: end_byte_col })
                        .unwrap_or(TextSize::from(data.text.len() as u32));

                    let start = usize::from(start_offset);
                    let end = usize::from(end_offset);

                    // Clamp to valid char boundaries to prevent panics
                    // when line_index is slightly out of sync with text
                    let start = data.text.floor_char_boundary(start.min(data.text.len()));
                    let end = data.text.floor_char_boundary(end.min(data.text.len()));

                    // Apply the change
                    data.text.replace_range(start..end, &change.text);
                } else {
                    // Full document sync
                    data.text = change.text;
                }

                // Rebuild line index after each change so subsequent
                // changes in the same batch use correct byte offsets
                data.line_index = LineIndex::new(&data.text);
            }
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

    /// Returns URIs of all tracked documents.
    pub fn uris(&self) -> Vec<Url> {
        self.docs.read().keys().cloned().collect()
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

    #[test]
    fn test_update_incremental_cyrillic() {
        // Regression test for panic with Cyrillic text:
        // LSP sends positions in UTF-16 code units, which must be converted to byte offsets.
        // For Cyrillic, 1 char = 2 bytes UTF-8 but 1 UTF-16 code unit.
        let mut mem_docs = MemDocs::new();
        let uri = Url::parse("file:///test.bsl").unwrap();

        // Initial text: "Процедура Тест()\nКонецПроцедуры"
        // "Процедура" = 9 chars, 18 bytes UTF-8, 9 UTF-16 code units
        mem_docs.insert(uri.clone(), "Процедура Тест()\nКонецПроцедуры".to_string(), 1);

        // Incremental change: Insert "Новая" at position (0, 10) in UTF-16 code units
        // Position 10 = after "Процедура " (space), before "Тест"
        // In bytes: "Процедура " = 18 + 1 = 19 bytes
        // In UTF-16: "Процедура " = 9 + 1 = 10 code units
        let changes = vec![TextDocumentContentChangeEvent {
            range: Some(lsp_types::Range {
                start: lsp_types::Position { line: 0, character: 10 },
                end: lsp_types::Position { line: 0, character: 10 },
            }),
            range_length: Some(0),
            text: "Новая".to_string(),
        }];

        mem_docs.update(&uri, changes);

        // Expected: "Процедура НоваяТест()\nКонецПроцедуры"
        assert_eq!(mem_docs.get(&uri), Some("Процедура НоваяТест()\nКонецПроцедуры".to_string()));
        assert_eq!(mem_docs.get_version(&uri), Some(2));
    }

    #[test]
    fn test_update_incremental_replace_cyrillic() {
        // Test replacing Cyrillic text
        let mut mem_docs = MemDocs::new();
        let uri = Url::parse("file:///test.bsl").unwrap();

        mem_docs.insert(uri.clone(), "Функция Старый()\nКонецФункции".to_string(), 1);

        // Replace "Старый" with "Новый"
        // "Функция " = 7 UTF-16 code units (14 bytes)
        // "Старый" = 6 UTF-16 code units (12 bytes), at position 7..13 in UTF-16
        let changes = vec![TextDocumentContentChangeEvent {
            range: Some(lsp_types::Range {
                start: lsp_types::Position { line: 0, character: 8 },
                end: lsp_types::Position { line: 0, character: 14 },
            }),
            range_length: Some(6),
            text: "Новый".to_string(),
        }];

        mem_docs.update(&uri, changes);

        assert_eq!(mem_docs.get(&uri), Some("Функция Новый()\nКонецФункции".to_string()));
    }

    #[test]
    fn test_update_incremental_multiline_cyrillic() {
        // Test change spanning multiple lines with Cyrillic
        let mut mem_docs = MemDocs::new();
        let uri = Url::parse("file:///test.bsl").unwrap();

        mem_docs.insert(uri.clone(), "Процедура\nТест()\nКонецПроцедуры".to_string(), 1);

        // Replace from end of line 0 to start of line 1
        // Line 0: "Процедура" = 9 UTF-16 code units
        // Replace: position (0,9) to (1,0) with " "
        let changes = vec![TextDocumentContentChangeEvent {
            range: Some(lsp_types::Range {
                start: lsp_types::Position { line: 0, character: 9 },
                end: lsp_types::Position { line: 1, character: 0 },
            }),
            range_length: Some(1),
            text: " ".to_string(),
        }];

        mem_docs.update(&uri, changes);

        assert_eq!(mem_docs.get(&uri), Some("Процедура Тест()\nКонецПроцедуры".to_string()));
    }
}
