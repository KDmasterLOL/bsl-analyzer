//! Conversion from LSP protocol types to internal types.

use anyhow::{anyhow, Result};
use ide::TextRange;
use line_index::{LineCol, LineIndex, TextSize};
use lsp_types::{Position, Url};
use vfs::FileId;

use crate::global_state::{GlobalState, GlobalStateSnapshot};
use crate::lsp::PositionEncoding;

/// Converts a URL to a FileId.
pub fn file_id(state: &GlobalState, url: &Url) -> Result<FileId> {
    let path = url.to_file_path().map_err(|_| anyhow!("Invalid file URL: {}", url))?;

    let vfs_path = vfs::VfsPath::new(path);
    let vfs = state.vfs.read();

    vfs.file_id(&vfs_path).ok_or_else(|| anyhow!("File not in VFS: {}", url))
}

/// Converts a URL to a FileId from a frozen snapshot.
pub fn file_id_snapshot(snapshot: &GlobalStateSnapshot, url: &Url) -> Result<FileId> {
    snapshot.file_id_for_url(url)
}

/// Converts a UTF-16 LSP position to a text offset.
///
/// Use [`offset_with_encoding`] when the client negotiated another encoding.
pub fn offset(line_index: &LineIndex, text: &str, position: Position) -> Result<TextSize> {
    offset_with_encoding(line_index, text, position, PositionEncoding::Utf16)
}

pub fn offset_with_encoding(
    line_index: &LineIndex,
    text: &str,
    position: Position,
    encoding: PositionEncoding,
) -> Result<TextSize> {
    if encoding == PositionEncoding::Utf8 {
        let line_col = LineCol { line: position.line, col: position.character };
        return line_index
            .offset(line_col)
            .ok_or_else(|| anyhow!("Position out of bounds: {:?}", position));
    }

    let byte_col = line_index
        .utf16_col_to_byte_col(text, position.line, position.character)
        .ok_or_else(|| anyhow!("Position out of bounds: {:?}", position))?;

    tracing::trace!(
        "from_proto::offset: LSP position={}:{} (UTF-16) -> byte_col={}",
        position.line,
        position.character,
        byte_col
    );

    if let Some(line_text) = line_index.safe_line_str(text, position.line) {
        tracing::trace!(
            "Line {} text (first 100 chars): {:?}",
            position.line,
            line_text.chars().take(100).collect::<String>()
        );
    }

    let line_col = LineCol { line: position.line, col: byte_col };

    let result = line_index
        .offset(line_col)
        .ok_or_else(|| anyhow!("Position out of bounds: {:?}", position))?;

    tracing::trace!("from_proto::offset: final offset = {:?}", result);

    Ok(result)
}

/// Converts a UTF-16 LSP range to a text range.
pub fn text_range(
    line_index: &LineIndex,
    text: &str,
    range: lsp_types::Range,
) -> Result<TextRange> {
    text_range_with_encoding(line_index, text, range, PositionEncoding::Utf16)
}

pub fn text_range_with_encoding(
    line_index: &LineIndex,
    text: &str,
    range: lsp_types::Range,
    encoding: PositionEncoding,
) -> Result<TextRange> {
    let start = offset_with_encoding(line_index, text, range.start, encoding)?;
    let end = offset_with_encoding(line_index, text, range.end, encoding)?;

    Ok(TextRange::new(start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset() {
        let text = "hello\nworld\nrust";
        let line_index = LineIndex::new(text);

        let pos = Position { line: 1, character: 0 };
        let result = offset(&line_index, text, pos).unwrap();
        assert_eq!(result, TextSize::from(6));

        let pos = Position { line: 2, character: 0 };
        let result = offset(&line_index, text, pos).unwrap();
        assert_eq!(result, TextSize::from(12));
    }

    #[test]
    fn test_offset_out_of_bounds() {
        let text = "hello";
        let line_index = LineIndex::new(text);

        let pos = Position { line: 10, character: 0 };
        assert!(offset(&line_index, text, pos).is_err());
    }

    #[test]
    fn test_offset_with_cyrillic() {
        let text = "Процедура Тест";
        let line_index = LineIndex::new(text);

        let pos = Position { line: 0, character: 9 };
        let result = offset(&line_index, text, pos).unwrap();
        assert_eq!(result, TextSize::from(18));

        let pos = Position { line: 0, character: 14 };
        let result = offset(&line_index, text, pos).unwrap();
        assert_eq!(result, TextSize::from(27));
    }
}
