//! In-memory document tracking for LSP.
//!
//! This module provides `MemDocs`, which tracks the state of documents
//! opened in the editor. It handles:
//! - Document lifecycle (open, change, close)
//! - Incremental text changes
//! - Version tracking

use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use line_index::{LineCol, LineIndex};
use lsp_types::{TextDocumentContentChangeEvent, Url};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;

use crate::lsp::PositionEncoding;

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

/// Immutable snapshot of a single document at request-dispatch time.
///
/// Created by `MemDocs::freeze()` and exposed via accessor methods so the
/// internal `DocumentData` layout stays private.
#[derive(Debug, Clone)]
pub struct FrozenDocument {
    text: String,
    version: i32,
    line_index: LineIndex,
}

impl FrozenDocument {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn version(&self) -> i32 {
        self.version
    }

    pub fn line_index(&self) -> &LineIndex {
        &self.line_index
    }
}

/// Frozen view of `MemDocs` at request-dispatch time.
///
/// Built on the main thread by `MemDocs::freeze()` and shared to background
/// workers by value (Arc clone is cheap). Workers never observe edits that
/// arrive after the freeze, so their source view is consistent with whatever
/// Salsa snapshot was captured alongside.
#[derive(Debug, Clone, Default)]
pub struct FrozenMemDocs {
    docs: Arc<FxHashMap<Url, FrozenDocument>>,
}

impl FrozenMemDocs {
    pub fn get(&self, uri: &Url) -> Option<&FrozenDocument> {
        self.docs.get(uri)
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
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

    /// Applies incremental changes to a document using LSP's default UTF-16 coordinates.
    ///
    /// This is called on `textDocument/didChange`.
    /// Supports both full document sync and incremental changes.
    pub fn update(&mut self, uri: &Url, changes: Vec<TextDocumentContentChangeEvent>) {
        if let Err(err) = self.update_with_encoding(uri, changes, PositionEncoding::Utf16) {
            tracing::error!(%uri, error = %err, "failed to apply document changes");
        }
    }

    /// Applies incremental changes to a document using the negotiated LSP position encoding.
    pub fn update_with_encoding(
        &mut self,
        uri: &Url,
        changes: Vec<TextDocumentContentChangeEvent>,
        encoding: PositionEncoding,
    ) -> Result<()> {
        let mut docs = self.docs.write();

        if let Some(data) = docs.get_mut(uri) {
            for change in changes {
                if let Some(range) = change.range {
                    let start = lsp_position_to_offset(
                        &data.line_index,
                        &data.text,
                        range.start,
                        encoding,
                    )?;
                    let end =
                        lsp_position_to_offset(&data.line_index, &data.text, range.end, encoding)?;

                    data.text.replace_range(start..end, &change.text);
                } else {
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

        Ok(())
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

    /// Deep-clone the current document state into an immutable snapshot.
    ///
    /// Paid on the main thread per request that needs a background snapshot.
    /// Cost is O(open_documents * text_size); acceptable for typical LSP
    /// workloads (tens of open files).
    pub fn freeze(&self) -> FrozenMemDocs {
        let live = self.docs.read();
        let frozen: FxHashMap<Url, FrozenDocument> = live
            .iter()
            .map(|(uri, data)| {
                (
                    uri.clone(),
                    FrozenDocument {
                        text: data.text.clone(),
                        version: data.version,
                        line_index: data.line_index.clone(),
                    },
                )
            })
            .collect();
        FrozenMemDocs { docs: Arc::new(frozen) }
    }
}

fn lsp_position_to_offset(
    line_index: &LineIndex,
    text: &str,
    position: lsp_types::Position,
    encoding: PositionEncoding,
) -> Result<usize> {
    let line_len = line_index
        .line_len(position.line)
        .ok_or_else(|| anyhow!("line {} is out of bounds", position.line))?;

    let byte_col = match encoding {
        PositionEncoding::Utf8 => position.character,
        PositionEncoding::Utf16 => line_index
            .utf16_col_to_byte_col(text, position.line, position.character)
            .ok_or_else(|| {
                anyhow!(
                    "UTF-16 column {} is out of bounds on line {}",
                    position.character,
                    position.line
                )
            })?,
    };

    if byte_col > line_len {
        bail!(
            "column {} is out of bounds on line {} with length {} bytes",
            byte_col,
            position.line,
            line_len
        );
    }

    let offset = line_index
        .offset(LineCol { line: position.line, col: byte_col })
        .ok_or_else(|| anyhow!("position {:?} is out of bounds", position))?;
    let offset = usize::from(offset);

    if !text.is_char_boundary(offset) {
        bail!("position {:?} resolves to non-character boundary byte offset {}", position, offset);
    }

    Ok(offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use line_index::TextSize;

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
    fn test_update_incremental_cyrillic_utf8_encoding() {
        let mut mem_docs = MemDocs::new();
        let uri = Url::parse("file:///test.bsl").unwrap();

        mem_docs.insert(uri.clone(), "Процедура Тест()\nКонецПроцедуры".to_string(), 1);

        let changes = vec![TextDocumentContentChangeEvent {
            range: Some(lsp_types::Range {
                start: lsp_types::Position { line: 0, character: 19 },
                end: lsp_types::Position { line: 0, character: 19 },
            }),
            range_length: Some(0),
            text: "Новая".to_string(),
        }];

        mem_docs.update_with_encoding(&uri, changes, PositionEncoding::Utf8).unwrap();

        assert_eq!(mem_docs.get(&uri), Some("Процедура НоваяТест()\nКонецПроцедуры".to_string()));
        assert_eq!(mem_docs.get_version(&uri), Some(2));
    }

    #[test]
    fn test_update_incremental_utf8_rejects_non_char_boundary() {
        let mut mem_docs = MemDocs::new();
        let uri = Url::parse("file:///test.bsl").unwrap();

        mem_docs.insert(uri.clone(), "Функция Старый()\nКонецФункции".to_string(), 1);

        let changes = vec![TextDocumentContentChangeEvent {
            range: Some(lsp_types::Range {
                start: lsp_types::Position { line: 0, character: 1 },
                end: lsp_types::Position { line: 0, character: 1 },
            }),
            range_length: Some(0),
            text: "X".to_string(),
        }];

        let result = mem_docs.update_with_encoding(&uri, changes, PositionEncoding::Utf8);

        assert!(result.is_err());
        assert_eq!(mem_docs.get(&uri), Some("Функция Старый()\nКонецФункции".to_string()));
        assert_eq!(mem_docs.get_version(&uri), Some(1));
    }

    #[test]
    fn freeze_is_independent_of_mutation() {
        let mut mem_docs = MemDocs::new();
        let uri = Url::parse("file:///test.bsl").unwrap();
        mem_docs.insert(uri.clone(), "original".to_string(), 1);

        let frozen = mem_docs.freeze();

        let changes = vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "mutated".to_string(),
        }];
        mem_docs.update(&uri, changes);

        assert_eq!(frozen.get(&uri).map(|d| d.text()), Some("original"));
        assert_eq!(frozen.get(&uri).map(|d| d.version()), Some(1));
        assert_eq!(mem_docs.get(&uri), Some("mutated".to_string()));
        assert_eq!(mem_docs.get_version(&uri), Some(2));

        mem_docs.remove(&uri);
        assert!(frozen.get(&uri).is_some(), "frozen view must survive removal");
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
