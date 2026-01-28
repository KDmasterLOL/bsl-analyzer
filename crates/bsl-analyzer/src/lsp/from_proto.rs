//! Convert LSP protocol types to internal types.
//!
//! This module provides conversions from LSP types (lsp_types) to
//! our internal representation (FileId, TextSize, etc.).

use anyhow::{anyhow, Result};
use ide_db::TextRange;
use line_index::{LineCol, LineIndex, TextSize};
use lsp_types::{Position, Url};
use vfs::FileId;

use crate::global_state::{GlobalState, GlobalStateSnapshot};

/// Converts a URL to a FileId.
///
/// # Errors
/// Returns an error if the URL is invalid or the file is not in VFS.
pub fn file_id(state: &GlobalState, url: &Url) -> Result<FileId> {
    // GlobalState has mutable method, can't use directly
    // We need the file to be already in VFS (from didOpen)
    let path = url.to_file_path().map_err(|_| anyhow!("Invalid file URL: {}", url))?;

    let vfs_path = vfs::VfsPath::new(path);
    let vfs = state.vfs.read();

    vfs.file_id(&vfs_path).ok_or_else(|| anyhow!("File not in VFS: {}", url))
}

/// Converts a URL to a FileId (snapshot version).
///
/// # Errors
/// Returns an error if the URL is invalid or the file is not in VFS.
pub fn file_id_snapshot(snapshot: &GlobalStateSnapshot, url: &Url) -> Result<FileId> {
    snapshot.file_id_for_url(url)
}

/// Converts an LSP Position to a TextSize offset.
///
/// LSP uses UTF-16 code units for character positions, while Rust uses UTF-8 bytes.
/// This function properly converts between the two encodings.
///
/// # Errors
/// Returns an error if the position is out of bounds.
pub fn offset(line_index: &LineIndex, text: &str, position: Position) -> Result<TextSize> {
    // LSP Position.character is UTF-16 code units, but LineCol.col is byte offset.
    // We need to convert UTF-16 → UTF-8 bytes first.
    let byte_col = line_index
        .utf16_col_to_byte_col(text, position.line, position.character)
        .ok_or_else(|| anyhow!("Position out of bounds: {:?}", position))?;

    tracing::info!(
        "from_proto::offset: LSP position={}:{} (UTF-16) → byte_col={}",
        position.line,
        position.character,
        byte_col
    );

    // Get line content to log it
    if let Some(line_text) = line_index.safe_line_str(text, position.line) {
        tracing::info!(
            "Line {} text (first 100 chars): {:?}",
            position.line,
            line_text.chars().take(100).collect::<String>()
        );
    }

    let line_col = LineCol { line: position.line, col: byte_col };

    let result = line_index
        .offset(line_col)
        .ok_or_else(|| anyhow!("Position out of bounds: {:?}", position))?;

    tracing::info!("from_proto::offset: final offset = {:?}", result);

    Ok(result)
}

/// Converts an LSP Range to a TextRange.
///
/// # Errors
/// Returns an error if the range is out of bounds.
pub fn text_range(
    line_index: &LineIndex,
    text: &str,
    range: lsp_types::Range,
) -> Result<TextRange> {
    let start = offset(line_index, text, range.start)?;
    let end = offset(line_index, text, range.end)?;

    Ok(TextRange::new(start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset() {
        let text = "hello\nworld\nrust";
        let line_index = LineIndex::new(text);

        // Position at start of "world" (line 1, col 0)
        let pos = Position { line: 1, character: 0 };
        let result = offset(&line_index, text, pos).unwrap();
        assert_eq!(result, TextSize::from(6)); // After "hello\n"

        // Position at 'r' in "rust" (line 2, col 0)
        let pos = Position { line: 2, character: 0 };
        let result = offset(&line_index, text, pos).unwrap();
        assert_eq!(result, TextSize::from(12)); // After "hello\nworld\n"
    }

    #[test]
    fn test_offset_out_of_bounds() {
        let text = "hello";
        let line_index = LineIndex::new(text);

        // Invalid line
        let pos = Position { line: 10, character: 0 };
        assert!(offset(&line_index, text, pos).is_err());
    }

    #[test]
    fn test_offset_with_cyrillic() {
        // Test UTF-16 → UTF-8 conversion with Cyrillic text
        // "Процедура" = 9 chars, 18 bytes UTF-8, 9 UTF-16 code units
        let text = "Процедура Тест";
        let line_index = LineIndex::new(text);

        // UTF-16 position 9 (space after "Процедура") → byte offset 18
        let pos = Position { line: 0, character: 9 };
        let result = offset(&line_index, text, pos).unwrap();
        assert_eq!(result, TextSize::from(18));

        // UTF-16 position 14 (end of "Тест") → byte offset 27
        let pos = Position { line: 0, character: 14 };
        let result = offset(&line_index, text, pos).unwrap();
        assert_eq!(result, TextSize::from(27));
    }
}
