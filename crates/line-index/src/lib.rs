//! Efficient line/column <-> byte offset conversions.
//!
//! This crate provides [`LineIndex`], a data structure that enables O(log n)
//! conversion from byte offsets to line/column positions and O(1) conversion
//! in the reverse direction.
//!
//! # Performance
//!
//! - Construction: O(n) where n is the text length
//! - `line_col(offset)`: O(log n) using binary search
//! - `offset(line_col)`: O(1)
//! - `line_start(line)`: O(1)
//!
//! # Example
//!
//! ```
//! use line_index::{LineIndex, LineCol};
//! use text_size::TextSize;
//!
//! let text = "hello\nworld\n";
//! let index = LineIndex::new(text);
//!
//! // Convert offset to line/col
//! let pos = index.line_col(TextSize::from(7));
//! assert_eq!(pos, LineCol { line: 1, col: 1 }); // 'o' in "world"
//!
//! // Convert line/col back to offset
//! let offset = index.offset(LineCol { line: 1, col: 0 });
//! assert_eq!(offset, Some(TextSize::from(6))); // start of "world"
//! ```

pub use text_size::{TextRange, TextSize};

/// Line and column position (both 0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct LineCol {
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based column (byte offset from line start).
    pub col: u32,
}

/// Maps byte offsets to/from line/column positions.
///
/// The index stores the byte offset of each newline character,
/// enabling efficient lookups in both directions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    /// Byte offsets of newline characters.
    /// `newlines[i]` is the byte offset of the (i+1)-th newline.
    /// Line 0 implicitly starts at offset 0.
    newlines: Box<[TextSize]>,
    /// Total length of the text.
    len: TextSize,
}

impl LineIndex {
    /// Creates a new `LineIndex` for the given text.
    ///
    /// This is an O(n) operation where n is the text length.
    pub fn new(text: &str) -> Self {
        let mut newlines = Vec::new();

        for (idx, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                // Store offset *after* the newline (start of next line)
                newlines.push(TextSize::from((idx + 1) as u32));
            }
        }

        Self { newlines: newlines.into_boxed_slice(), len: TextSize::of(text) }
    }

    /// Returns the total number of lines in the text.
    ///
    /// A text with no newlines has 1 line. A text ending with a newline
    /// has one more line (empty) after the last newline.
    #[inline]
    pub fn len_lines(&self) -> u32 {
        (self.newlines.len() + 1) as u32
    }

    /// Converts a byte offset to a line/column position.
    ///
    /// This is an O(log n) operation using binary search.
    ///
    /// # Panics
    ///
    /// Panics if `offset > text.len()`.
    pub fn line_col(&self, offset: TextSize) -> LineCol {
        assert!(offset <= self.len, "offset {:?} exceeds text length {:?}", offset, self.len);

        // Binary search to find the line containing this offset.
        // We want the largest line index where line_start <= offset.
        let line = self.newlines.partition_point(|&nl| nl <= offset) as u32;

        let line_start = self.line_start(line);
        let col = u32::from(offset) - u32::from(line_start);

        LineCol { line, col }
    }

    /// Converts a line/column position to a byte offset.
    ///
    /// Returns `None` if the line doesn't exist.
    ///
    /// This is an O(1) operation.
    #[inline]
    pub fn offset(&self, line_col: LineCol) -> Option<TextSize> {
        let line_start = self.try_line_start(line_col.line)?;
        Some(line_start + TextSize::from(line_col.col))
    }

    /// Returns the byte offset of the start of the given line.
    ///
    /// # Panics
    ///
    /// Panics if `line >= len_lines()`.
    #[inline]
    pub fn line_start(&self, line: u32) -> TextSize {
        self.try_line_start(line).expect("line index out of bounds")
    }

    /// Returns the byte offset of the start of the given line, or `None` if out of bounds.
    #[inline]
    pub fn try_line_start(&self, line: u32) -> Option<TextSize> {
        if line == 0 {
            Some(TextSize::from(0))
        } else {
            self.newlines.get((line - 1) as usize).copied()
        }
    }

    /// Returns the byte range of the given line (excluding the newline character).
    ///
    /// Returns `None` if the line doesn't exist.
    pub fn line_range(&self, line: u32) -> Option<TextRange> {
        let start = self.try_line_start(line)?;
        let end = if (line as usize) < self.newlines.len() {
            // Exclude the newline character
            self.newlines[line as usize] - TextSize::from(1)
        } else {
            self.len
        };
        Some(TextRange::new(start, end))
    }

    /// Returns the length of the given line in bytes (excluding the newline).
    ///
    /// Returns `None` if the line doesn't exist.
    pub fn line_len(&self, line: u32) -> Option<u32> {
        self.line_range(line).map(|r| u32::from(r.len()))
    }

    /// Returns the total text length.
    #[inline]
    pub fn text_len(&self) -> TextSize {
        self.len
    }

    /// Returns a safe slice of line text with defensive char boundary checks.
    ///
    /// If `LineIndex` is out of sync with `text` (e.g., after edits),
    /// byte ranges may not align with UTF-8 character boundaries.
    /// This method uses `floor_char_boundary` to prevent panics.
    ///
    /// Returns `None` if the line doesn't exist.
    #[inline]
    pub fn safe_line_str<'a>(&self, text: &'a str, line: u32) -> Option<&'a str> {
        let range = self.line_range(line)?;
        let start: usize = range.start().into();
        let end: usize = range.end().into();

        let start = text.floor_char_boundary(start.min(text.len()));
        let end = text.floor_char_boundary(end.min(text.len()));

        Some(&text[start..end])
    }

    /// Calculates the UTF-16 length of text in a given range.
    ///
    /// This is needed for LSP, which uses UTF-16 code units for positions and lengths.
    /// For example:
    /// - Cyrillic "П" = 2 bytes UTF-8 = 1 char = 1 UTF-16 code unit
    /// - Emoji "😀" = 4 bytes UTF-8 = 1 char = 2 UTF-16 code units
    ///
    /// **Defensive**: If the range endpoints are not on character boundaries,
    /// we round them to the nearest character boundaries to avoid panics.
    pub fn utf16_len(text: &str, range: TextRange) -> u32 {
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        let end = end.min(text.len());

        // SAFETY: Ensure both start and end are on char boundaries to avoid panic.
        // If a diagnostic generates an invalid TextRange (not on char boundary),
        // we defensively round to the nearest character boundaries.
        let start = text.floor_char_boundary(start);
        let end = text.floor_char_boundary(end);

        text[start..end].encode_utf16().count() as u32
    }

    /// Converts a byte offset within a line to UTF-16 code unit offset.
    ///
    /// LSP requires positions in UTF-16 code units, not bytes.
    /// This function takes a byte offset from line start and returns the corresponding
    /// UTF-16 code unit offset.
    ///
    /// **Defensive**: If `byte_col` points to the middle of a UTF-8 character,
    /// we round down to the nearest character boundary to avoid panics.
    pub fn utf16_col(&self, text: &str, line: u32, byte_col: u32) -> u32 {
        let Some(line_range) = self.line_range(line) else {
            return 0;
        };

        let line_start: usize = line_range.start().into();
        let col_end = line_start + byte_col as usize;
        let col_end = col_end.min(text.len()).min(line_range.end().into());

        // SAFETY: Ensure both line_start and col_end are on char boundaries to avoid panic.
        // If a diagnostic generates an invalid TextRange (not on char boundary),
        // we defensively round down to the nearest character boundary.
        let line_start = text.floor_char_boundary(line_start);
        let col_end = text.floor_char_boundary(col_end);

        text[line_start..col_end].encode_utf16().count() as u32
    }

    /// Converts a UTF-16 code unit offset to byte offset within a line.
    ///
    /// This is the inverse of `utf16_col()`. It's needed for converting LSP positions
    /// (which use UTF-16) to byte offsets for Rust string operations.
    ///
    /// Returns `None` if:
    /// - The line doesn't exist
    /// - The UTF-16 offset exceeds the line length
    ///
    /// # Example
    ///
    /// ```
    /// use line_index::{LineIndex, LineIndexExt};
    ///
    /// // Cyrillic: "Процедура" = 9 chars, 18 bytes UTF-8, 9 UTF-16 code units
    /// let text = "Процедура Тест";
    /// let index = LineIndex::new(text);
    ///
    /// // UTF-16 position 9 (space after "Процедура") → byte offset 18
    /// assert_eq!(index.utf16_col_to_byte_col(text, 0, 9), Some(18));
    /// ```
    pub fn utf16_col_to_byte_col(&self, text: &str, line: u32, utf16_col: u32) -> Option<u32> {
        let line_text = self.safe_line_str(text, line)?;

        // Iterate through characters, counting UTF-16 code units until we reach utf16_col
        let mut utf16_offset = 0u32;
        let mut byte_offset = 0usize;

        for ch in line_text.chars() {
            if utf16_offset >= utf16_col {
                return Some(byte_offset as u32);
            }

            // Count UTF-16 code units for this character
            // BMP characters (including Cyrillic, Latin, most symbols): 1 code unit
            // Supplementary characters (emoji, rare CJK): 2 code units (surrogate pair)
            let utf16_len = ch.len_utf16() as u32;
            utf16_offset += utf16_len;
            byte_offset += ch.len_utf8();
        }

        // utf16_col is at or past end of line
        if utf16_offset >= utf16_col {
            Some(byte_offset as u32)
        } else {
            // UTF-16 offset exceeds line length
            None
        }
    }

    /// Iterates over all lines, yielding (line_number, line_range) pairs.
    pub fn lines(&self) -> impl Iterator<Item = (u32, TextRange)> + '_ {
        (0..self.len_lines()).filter_map(|line| self.line_range(line).map(|range| (line, range)))
    }
}

/// Extension trait for converting character positions to byte positions.
///
/// BSL uses UTF-8, and some diagnostics need to count characters rather than bytes.
/// This trait provides utilities for such conversions.
pub trait LineIndexExt {
    /// Counts characters in a line up to the given byte column.
    fn byte_col_to_char_col(&self, text: &str, line: u32, byte_col: u32) -> u32;

    /// Converts a character column to a byte column.
    fn char_col_to_byte_col(&self, text: &str, line: u32, char_col: u32) -> Option<u32>;

    /// Returns the length of a line in characters (not bytes).
    fn line_char_len(&self, text: &str, line: u32) -> Option<u32>;
}

impl LineIndexExt for LineIndex {
    fn byte_col_to_char_col(&self, text: &str, line: u32, byte_col: u32) -> u32 {
        let Some(line_range) = self.line_range(line) else {
            return 0;
        };

        let line_start: usize = line_range.start().into();
        let col_end = line_start + byte_col as usize;
        let col_end = col_end.min(text.len());

        // SAFETY: Ensure char boundaries to avoid panic if LineIndex is out of sync
        let line_start = text.floor_char_boundary(line_start);
        let col_end = text.floor_char_boundary(col_end);

        text[line_start..col_end].chars().count() as u32
    }

    fn char_col_to_byte_col(&self, text: &str, line: u32, char_col: u32) -> Option<u32> {
        let line_text = self.safe_line_str(text, line)?;

        let mut byte_col = 0u32;
        for (i, ch) in line_text.chars().enumerate() {
            if i as u32 == char_col {
                return Some(byte_col);
            }
            byte_col += ch.len_utf8() as u32;
        }

        // char_col is at or past end of line
        Some(byte_col)
    }

    fn line_char_len(&self, text: &str, line: u32) -> Option<u32> {
        let line_text = self.safe_line_str(text, line)?;

        Some(line_text.chars().count() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_text() {
        let index = LineIndex::new("");
        assert_eq!(index.len_lines(), 1);
        assert_eq!(index.line_col(TextSize::from(0)), LineCol { line: 0, col: 0 });
    }

    #[test]
    fn test_single_line() {
        let index = LineIndex::new("hello");
        assert_eq!(index.len_lines(), 1);
        assert_eq!(index.line_col(TextSize::from(0)), LineCol { line: 0, col: 0 });
        assert_eq!(index.line_col(TextSize::from(3)), LineCol { line: 0, col: 3 });
        assert_eq!(index.line_col(TextSize::from(5)), LineCol { line: 0, col: 5 });
    }

    #[test]
    fn test_multiple_lines() {
        let text = "hello\nworld\ntest";
        let index = LineIndex::new(text);

        assert_eq!(index.len_lines(), 3);

        // Line 0: "hello"
        assert_eq!(index.line_col(TextSize::from(0)), LineCol { line: 0, col: 0 });
        assert_eq!(index.line_col(TextSize::from(5)), LineCol { line: 0, col: 5 }); // '\n'

        // Line 1: "world"
        assert_eq!(index.line_col(TextSize::from(6)), LineCol { line: 1, col: 0 }); // 'w'
        assert_eq!(index.line_col(TextSize::from(7)), LineCol { line: 1, col: 1 }); // 'o'

        // Line 2: "test"
        assert_eq!(index.line_col(TextSize::from(12)), LineCol { line: 2, col: 0 }); // 't'
        assert_eq!(index.line_col(TextSize::from(16)), LineCol { line: 2, col: 4 });
        // end
    }

    #[test]
    fn test_trailing_newline() {
        let text = "hello\nworld\n";
        let index = LineIndex::new(text);

        assert_eq!(index.len_lines(), 3);
        assert_eq!(index.line_col(TextSize::from(12)), LineCol { line: 2, col: 0 });
    }

    #[test]
    fn test_offset_roundtrip() {
        let text = "hello\nworld\ntest";
        let index = LineIndex::new(text);

        for offset in 0..=text.len() {
            let offset = TextSize::from(offset as u32);
            let line_col = index.line_col(offset);
            let recovered = index.offset(line_col).unwrap();
            assert_eq!(offset, recovered, "roundtrip failed for offset {:?}", offset);
        }
    }

    #[test]
    fn test_line_start() {
        let text = "hello\nworld\ntest";
        let index = LineIndex::new(text);

        assert_eq!(index.line_start(0), TextSize::from(0));
        assert_eq!(index.line_start(1), TextSize::from(6));
        assert_eq!(index.line_start(2), TextSize::from(12));
    }

    #[test]
    fn test_line_range() {
        let text = "hello\nworld\ntest";
        let index = LineIndex::new(text);

        assert_eq!(index.line_range(0), Some(TextRange::new(TextSize::from(0), TextSize::from(5))));
        assert_eq!(
            index.line_range(1),
            Some(TextRange::new(TextSize::from(6), TextSize::from(11)))
        );
        assert_eq!(
            index.line_range(2),
            Some(TextRange::new(TextSize::from(12), TextSize::from(16)))
        );
        assert_eq!(index.line_range(3), None);
    }

    #[test]
    fn test_line_len() {
        let text = "hello\nworld\ntest";
        let index = LineIndex::new(text);

        assert_eq!(index.line_len(0), Some(5));
        assert_eq!(index.line_len(1), Some(5));
        assert_eq!(index.line_len(2), Some(4));
        assert_eq!(index.line_len(3), None);
    }

    #[test]
    fn test_utf8_text() {
        // Cyrillic: "Привет" (12 bytes, 6 chars) + "\n" + "Мир" (6 bytes, 3 chars)
        let text = "Привет\nМир";
        let index = LineIndex::new(text);

        assert_eq!(index.len_lines(), 2);

        // Line 0: "Привет" (12 bytes)
        assert_eq!(index.line_col(TextSize::from(0)), LineCol { line: 0, col: 0 });
        assert_eq!(index.line_col(TextSize::from(2)), LineCol { line: 0, col: 2 }); // after 'П' (2 bytes)
        assert_eq!(index.line_col(TextSize::from(12)), LineCol { line: 0, col: 12 }); // '\n'

        // Line 1: "Мир" starts at byte 13
        assert_eq!(index.line_col(TextSize::from(13)), LineCol { line: 1, col: 0 });
    }

    #[test]
    fn test_char_column_conversion() {
        // "Привет" = 6 chars, 12 bytes
        let text = "Привет\nМир";
        let index = LineIndex::new(text);

        // Line 0: byte col 12 = char col 6
        assert_eq!(index.byte_col_to_char_col(text, 0, 12), 6);
        assert_eq!(index.byte_col_to_char_col(text, 0, 2), 1); // 'П' is 2 bytes

        // Reverse
        assert_eq!(index.char_col_to_byte_col(text, 0, 6), Some(12));
        assert_eq!(index.char_col_to_byte_col(text, 0, 1), Some(2));
    }

    #[test]
    fn test_line_char_len() {
        let text = "Привет\nWorld";
        let index = LineIndex::new(text);

        // "Привет" = 6 chars (12 bytes)
        assert_eq!(index.line_char_len(text, 0), Some(6));
        // "World" = 5 chars (5 bytes)
        assert_eq!(index.line_char_len(text, 1), Some(5));
    }

    #[test]
    fn test_lines_iterator() {
        let text = "hello\nworld\ntest";
        let index = LineIndex::new(text);

        let lines: Vec<_> = index.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], (0, TextRange::new(TextSize::from(0), TextSize::from(5))));
        assert_eq!(lines[1], (1, TextRange::new(TextSize::from(6), TextSize::from(11))));
        assert_eq!(lines[2], (2, TextRange::new(TextSize::from(12), TextSize::from(16))));
    }

    #[test]
    fn test_utf16_len() {
        // ASCII: 1 byte = 1 UTF-16 code unit
        let text = "hello";
        let range = TextRange::new(TextSize::from(0), TextSize::from(5));
        assert_eq!(LineIndex::utf16_len(text, range), 5);

        // Cyrillic: 2 bytes = 1 UTF-16 code unit
        // "ПрограммныйИнтерфейс" = 40 bytes, 20 chars, 20 UTF-16 code units
        let text = "ПрограммныйИнтерфейс";
        let range = TextRange::new(TextSize::from(0), TextSize::from(40));
        assert_eq!(LineIndex::utf16_len(text, range), 20);

        // Mixed: "Функция" = 14 bytes, 7 chars, 7 UTF-16 code units
        let text = "Функция";
        let range = TextRange::new(TextSize::from(0), TextSize::from(14));
        assert_eq!(LineIndex::utf16_len(text, range), 7);

        // Emoji: "😀" = 4 bytes, 1 char, 2 UTF-16 code units (surrogate pair)
        let text = "hello😀world";
        let range = TextRange::new(TextSize::from(5), TextSize::from(9)); // just the emoji
        assert_eq!(LineIndex::utf16_len(text, range), 2);
    }

    #[test]
    fn test_utf16_col() {
        // "    Перем ЛокальнаяПеременная;" - example from user
        // "Перем" starts at byte 4, which is UTF-16 position 4 (leading spaces are ASCII)
        // "ЛокальнаяПеременная" starts at byte 15 (4 spaces + "Перем" (10 bytes) + 1 space)
        let text = "    Перем ЛокальнаяПеременная;";
        let index = LineIndex::new(text);

        // ASCII spaces: byte 4 = UTF-16 position 4
        assert_eq!(index.utf16_col(text, 0, 4), 4);

        // After "Перем" (10 bytes = 5 chars): byte 14 = UTF-16 position 9 (4 spaces + 5 chars)
        assert_eq!(index.utf16_col(text, 0, 14), 9);

        // Start of "ЛокальнаяПеременная": byte 15 = UTF-16 position 10 (4 + 5 + 1 space)
        assert_eq!(index.utf16_col(text, 0, 15), 10);

        // Cyrillic: "Привет Мир"
        // "Привет" = 12 bytes, 6 chars, 6 UTF-16 code units
        let text = "Привет Мир";
        let index = LineIndex::new(text);

        // After "Привет": byte 12 = UTF-16 position 6
        assert_eq!(index.utf16_col(text, 0, 12), 6);

        // After "Привет ": byte 13 = UTF-16 position 7
        assert_eq!(index.utf16_col(text, 0, 13), 7);
    }

    #[test]
    fn test_utf16_col_invalid_offset() {
        // Regression test for crash: "byte index is not a char boundary"
        // This happened when a diagnostic had a TextRange with endpoints not on char boundaries.
        //
        // Text: "ВладимирБондаревский"
        // 'В' occupies bytes 0..2
        // Asking for utf16_col with byte_col=1 (middle of 'В') should not panic.
        let text = "ВладимирБондаревский";
        let index = LineIndex::new(text);

        // byte_col=1 is in the middle of 'В' (bytes 0..2)
        // Should defensively round down to byte 0 and return UTF-16 position 0
        let utf16_col = index.utf16_col(text, 0, 1);
        assert_eq!(utf16_col, 0, "Should round down to char boundary");

        // byte_col=3 is in the middle of 'л' (bytes 2..4)
        // Should round down to byte 2 (start of 'л') and return UTF-16 position 1
        let utf16_col = index.utf16_col(text, 0, 3);
        assert_eq!(utf16_col, 1, "Should round down to start of 'л'");
    }

    #[test]
    fn test_utf16_len_invalid_range() {
        // Regression test for crash with invalid TextRange endpoints
        let text = "ПрограммныйИнтерфейс";

        // Valid range
        let valid_range = TextRange::new(0.into(), 4.into()); // "Пр" (2 chars, 4 bytes)
        assert_eq!(LineIndex::utf16_len(text, valid_range), 2);

        // Invalid range: end=3 is in the middle of 'о' (bytes 2..4)
        // Should defensively round down to byte 2
        let invalid_range = TextRange::new(0.into(), 3.into());
        let len = LineIndex::utf16_len(text, invalid_range);
        // Should return 1 (just 'П'), since 3 rounds down to 2
        assert_eq!(len, 1, "Should handle invalid range endpoint");
    }

    #[test]
    fn test_utf16_col_to_byte_col_ascii() {
        // ASCII: 1 byte = 1 UTF-16 code unit
        let text = "hello world";
        let index = LineIndex::new(text);

        // Position 0 → byte 0
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 0), Some(0));
        // Position 6 (space) → byte 6
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 6), Some(6));
        // Position 11 (end) → byte 11
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 11), Some(11));
        // Position 12 (past end) → None
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 12), None);
    }

    #[test]
    fn test_utf16_col_to_byte_col_cyrillic() {
        // Cyrillic: 2 bytes UTF-8 = 1 UTF-16 code unit
        // "Процедура Тест" = 14 chars, 27 bytes UTF-8, 14 UTF-16 code units
        let text = "Процедура Тест";
        let index = LineIndex::new(text);

        // Position 0 → byte 0
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 0), Some(0));
        // Position 1 ('р' after 'П') → byte 2
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 1), Some(2));
        // Position 9 (space after "Процедура") → byte 18
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 9), Some(18));
        // Position 10 ('Т' in "Тест") → byte 19 (start of 'Т')
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 10), Some(19));
        // Position 14 (end) → byte 27
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 14), Some(27));
    }

    #[test]
    fn test_utf16_col_to_byte_col_mixed() {
        // Mixed ASCII and Cyrillic
        // "Функция Test" = "Функция" (14 bytes, 7 chars) + " Test" (5 bytes, 5 chars)
        let text = "Функция Test";
        let index = LineIndex::new(text);

        // Position 0 → byte 0
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 0), Some(0));
        // Position 7 (space after "Функция") → byte 14
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 7), Some(14));
        // Position 8 ('T') → byte 15
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 8), Some(15));
        // Position 12 (end) → byte 19
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 12), Some(19));
    }

    #[test]
    fn test_utf16_col_to_byte_col_multiline() {
        // Test with multiple lines
        let text = "Процедура\nТест()";
        let index = LineIndex::new(text);

        // Line 0: "Процедура" = 9 UTF-16 code units, 18 bytes
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 0), Some(0));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 9), Some(18));

        // Line 1: "Тест()" = 6 UTF-16 code units (4 Cyrillic + 2 ASCII), 10 bytes
        assert_eq!(index.utf16_col_to_byte_col(text, 1, 0), Some(0));
        assert_eq!(index.utf16_col_to_byte_col(text, 1, 4), Some(8)); // after "Тест"
        assert_eq!(index.utf16_col_to_byte_col(text, 1, 6), Some(10)); // after "()"
    }

    #[test]
    fn test_utf16_col_to_byte_col_emoji() {
        // Emoji: 4 bytes UTF-8 = 2 UTF-16 code units (surrogate pair)
        let text = "hello😀world";
        let index = LineIndex::new(text);

        // Position 0 → byte 0
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 0), Some(0));
        // Position 5 (before emoji) → byte 5
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 5), Some(5));
        // Position 7 (after emoji, 2 UTF-16 code units) → byte 9 (5 + 4)
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 7), Some(9));
        // Position 12 (end) → byte 14
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 12), Some(14));
    }

    #[test]
    fn test_utf16_col_to_byte_col_roundtrip() {
        // Verify roundtrip: byte col → UTF-16 col → byte col
        // IMPORTANT: Only test on valid char boundaries!
        let text = "Процедура Тест";
        let index = LineIndex::new(text);

        // Valid char boundaries: 0, 2, 4, 6, 8, 10, 12, 14, 16, 18 (П,р,о,ц,е,д,у,р,а, )
        // and 19, 21, 23, 25, 27 (Т,е,с,т,end)
        for byte_col in [0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 19, 21, 23, 25, 27] {
            let utf16_col = index.utf16_col(text, 0, byte_col);
            let recovered = index.utf16_col_to_byte_col(text, 0, utf16_col);
            assert_eq!(
                recovered,
                Some(byte_col),
                "roundtrip failed for byte_col {}: utf16_col={}, recovered={:?}",
                byte_col,
                utf16_col,
                recovered
            );
        }
    }
}
