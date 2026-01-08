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
/// # Errors
/// Returns an error if the position is out of bounds.
pub fn offset(line_index: &LineIndex, position: Position) -> Result<TextSize> {
    let line_col = LineCol { line: position.line, col: position.character };

    line_index.offset(line_col).ok_or_else(|| anyhow!("Position out of bounds: {:?}", position))
}

/// Converts an LSP Range to a TextRange.
///
/// # Errors
/// Returns an error if the range is out of bounds.
pub fn text_range(line_index: &LineIndex, range: lsp_types::Range) -> Result<TextRange> {
    let start = offset(line_index, range.start)?;
    let end = offset(line_index, range.end)?;

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
        let result = offset(&line_index, pos).unwrap();
        assert_eq!(result, TextSize::from(6)); // After "hello\n"

        // Position at 'r' in "rust" (line 2, col 0)
        let pos = Position { line: 2, character: 0 };
        let result = offset(&line_index, pos).unwrap();
        assert_eq!(result, TextSize::from(12)); // After "hello\nworld\n"
    }

    #[test]
    fn test_offset_out_of_bounds() {
        let text = "hello";
        let line_index = LineIndex::new(text);

        // Invalid line
        let pos = Position { line: 10, character: 0 };
        assert!(offset(&line_index, pos).is_err());
    }
}
