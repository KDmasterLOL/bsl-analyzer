use anyhow::{anyhow, bail, Result};
use ide::TextRange;
use line_index::{LineCol, LineIndex, TextSize};
use lsp_types::{Position, Url};
use vfs::FileId;

use crate::global_state::{GlobalState, GlobalStateSnapshot};
use crate::lsp::PositionEncoding;

pub fn file_id(state: &GlobalState, url: &Url) -> Result<FileId> {
    let path = url.to_file_path().map_err(|_| anyhow!("Invalid file URL: {}", url))?;

    let vfs_path = vfs::VfsPath::new(path);
    let vfs = state.vfs.read();

    vfs.file_id(&vfs_path).ok_or_else(|| anyhow!("File not in VFS: {}", url))
}

pub fn file_id_snapshot(snapshot: &GlobalStateSnapshot, url: &Url) -> Result<FileId> {
    snapshot.file_id_for_url(url)
}

pub fn offset(line_index: &LineIndex, text: &str, position: Position) -> Result<TextSize> {
    offset_with_encoding(line_index, text, position, PositionEncoding::Utf16)
}

/// The byte offset of an LSP position, or an error when the position names no
/// place in `text`.
///
/// Both bounds are proven here and nowhere else: [`LineIndex::offset`] validates
/// the line only and adds the column to its start unchecked, so an over-long
/// column would silently resolve into a later line and a column splitting a
/// character would resolve to an offset no `&str[..]` accepts. The UTF-16 path
/// cannot produce either — it walks whole characters — but the UTF-8 path takes
/// the column as the client counted it.
pub fn offset_with_encoding(
    line_index: &LineIndex,
    text: &str,
    position: Position,
    encoding: PositionEncoding,
) -> Result<TextSize> {
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

    if !text.is_char_boundary(usize::from(offset)) {
        bail!(
            "position {:?} resolves to non-character boundary byte offset {}",
            position,
            u32::from(offset)
        );
    }

    tracing::trace!(?position, ?encoding, byte_col, ?offset, "from_proto::offset");

    Ok(offset)
}

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
    fn utf8_column_inside_a_multibyte_char_is_rejected() {
        let text = "Процедура Тест";
        let line_index = LineIndex::new(text);

        // Byte column 1 sits inside 'П' (bytes 0..2). Resolving it would hand
        // every downstream slice an offset no `&str[..]` accepts.
        let pos = Position { line: 0, character: 1 };
        let result = offset_with_encoding(&line_index, text, pos, PositionEncoding::Utf8);

        assert!(result.is_err(), "got {result:?}");
    }

    #[test]
    fn utf8_column_past_the_line_end_is_rejected() {
        let text = "Процедура\nТест";
        let line_index = LineIndex::new(text);

        // Line 0 is 18 bytes long; an over-long column used to spill silently
        // into the following line instead of being reported out of bounds.
        let pos = Position { line: 0, character: 40 };
        let result = offset_with_encoding(&line_index, text, pos, PositionEncoding::Utf8);

        assert!(result.is_err(), "got {result:?}");
    }

    #[test]
    fn utf8_byte_column_on_a_boundary_resolves() {
        let text = "Процедура Тест";
        let line_index = LineIndex::new(text);

        let pos = Position { line: 0, character: 18 };
        let result = offset_with_encoding(&line_index, text, pos, PositionEncoding::Utf8).unwrap();

        assert_eq!(result, TextSize::from(18));
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
