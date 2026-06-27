pub use text_size::{TextRange, TextSize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    newlines: Box<[TextSize]>,
    len: TextSize,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut newlines = Vec::new();

        for (idx, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                newlines.push(TextSize::from((idx + 1) as u32));
            }
        }

        Self { newlines: newlines.into_boxed_slice(), len: TextSize::of(text) }
    }

    #[inline]
    pub fn len_lines(&self) -> u32 {
        (self.newlines.len() + 1) as u32
    }

    /// Approximate live heap bytes for Salsa's `memory_usage` report: the boxed
    /// `newlines` slice (one `TextSize` per `\n` in the file). The `len` field is
    /// inline and owns no heap.
    pub fn estimated_heap(&self) -> usize {
        self.newlines.len() * std::mem::size_of::<TextSize>()
    }

    pub fn line_col(&self, offset: TextSize) -> LineCol {
        self.try_line_col(offset)
            .unwrap_or_else(|| panic!("offset {:?} exceeds text length {:?}", offset, self.len))
    }

    pub fn try_line_col(&self, offset: TextSize) -> Option<LineCol> {
        if offset > self.len {
            return None;
        }

        let line = self.newlines.partition_point(|&nl| nl <= offset) as u32;

        let line_start = self.line_start(line);
        let col = u32::from(offset) - u32::from(line_start);

        Some(LineCol { line, col })
    }

    #[inline]
    pub fn offset(&self, line_col: LineCol) -> Option<TextSize> {
        let line_start = self.try_line_start(line_col.line)?;
        Some(line_start + TextSize::from(line_col.col))
    }

    #[inline]
    pub fn line_start(&self, line: u32) -> TextSize {
        self.try_line_start(line).expect("line index out of bounds")
    }

    #[inline]
    pub fn try_line_start(&self, line: u32) -> Option<TextSize> {
        if line == 0 {
            Some(TextSize::from(0))
        } else {
            self.newlines.get((line - 1) as usize).copied()
        }
    }

    pub fn line_range(&self, line: u32) -> Option<TextRange> {
        let start = self.try_line_start(line)?;
        let end = if (line as usize) < self.newlines.len() {
            self.newlines[line as usize] - TextSize::from(1)
        } else {
            self.len
        };
        Some(TextRange::new(start, end))
    }

    pub fn line_len(&self, line: u32) -> Option<u32> {
        self.line_range(line).map(|r| u32::from(r.len()))
    }

    #[inline]
    pub fn text_len(&self) -> TextSize {
        self.len
    }

    #[inline]
    pub fn safe_line_str<'a>(&self, text: &'a str, line: u32) -> Option<&'a str> {
        let range = self.line_range(line)?;
        let start: usize = range.start().into();
        let end: usize = range.end().into();

        let start = text.floor_char_boundary(start.min(text.len()));
        let end = text.floor_char_boundary(end.min(text.len()));

        Some(&text[start..end])
    }

    pub fn utf16_len(text: &str, range: TextRange) -> u32 {
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        let end = end.min(text.len());

        let start = text.floor_char_boundary(start);
        let end = text.floor_char_boundary(end);

        text[start..end].encode_utf16().count() as u32
    }

    pub fn utf16_col(&self, text: &str, line: u32, byte_col: u32) -> u32 {
        let Some(line_range) = self.line_range(line) else {
            return 0;
        };

        let line_start: usize = line_range.start().into();
        let col_end = line_start + byte_col as usize;
        let col_end = col_end.min(text.len()).min(line_range.end().into());

        let line_start = text.floor_char_boundary(line_start);
        let col_end = text.floor_char_boundary(col_end);

        text[line_start..col_end].encode_utf16().count() as u32
    }

    pub fn utf16_col_to_byte_col(&self, text: &str, line: u32, utf16_col: u32) -> Option<u32> {
        let line_text = self.safe_line_str(text, line)?;

        let mut utf16_offset = 0u32;
        let mut byte_offset = 0usize;

        for ch in line_text.chars() {
            if utf16_offset >= utf16_col {
                return Some(byte_offset as u32);
            }

            let utf16_len = ch.len_utf16() as u32;
            utf16_offset += utf16_len;
            byte_offset += ch.len_utf8();
        }

        if utf16_offset >= utf16_col {
            Some(byte_offset as u32)
        } else {
            None
        }
    }

    pub fn lines(&self) -> impl Iterator<Item = (u32, TextRange)> + '_ {
        (0..self.len_lines()).filter_map(|line| self.line_range(line).map(|range| (line, range)))
    }
}

pub trait LineIndexExt {
    fn byte_col_to_char_col(&self, text: &str, line: u32, byte_col: u32) -> u32;

    fn char_col_to_byte_col(&self, text: &str, line: u32, char_col: u32) -> Option<u32>;

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

        assert_eq!(index.line_col(TextSize::from(0)), LineCol { line: 0, col: 0 });
        assert_eq!(index.line_col(TextSize::from(5)), LineCol { line: 0, col: 5 });

        assert_eq!(index.line_col(TextSize::from(6)), LineCol { line: 1, col: 0 });
        assert_eq!(index.line_col(TextSize::from(7)), LineCol { line: 1, col: 1 });

        assert_eq!(index.line_col(TextSize::from(12)), LineCol { line: 2, col: 0 });
        assert_eq!(index.line_col(TextSize::from(16)), LineCol { line: 2, col: 4 });
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
        let text = "Привет\nМир";
        let index = LineIndex::new(text);

        assert_eq!(index.len_lines(), 2);

        assert_eq!(index.line_col(TextSize::from(0)), LineCol { line: 0, col: 0 });
        assert_eq!(index.line_col(TextSize::from(2)), LineCol { line: 0, col: 2 });
        assert_eq!(index.line_col(TextSize::from(12)), LineCol { line: 0, col: 12 });
        assert_eq!(index.line_col(TextSize::from(13)), LineCol { line: 1, col: 0 });
    }

    #[test]
    fn test_char_column_conversion() {
        let text = "Привет\nМир";
        let index = LineIndex::new(text);

        assert_eq!(index.byte_col_to_char_col(text, 0, 12), 6);
        assert_eq!(index.byte_col_to_char_col(text, 0, 2), 1);
        assert_eq!(index.char_col_to_byte_col(text, 0, 6), Some(12));
        assert_eq!(index.char_col_to_byte_col(text, 0, 1), Some(2));
    }

    #[test]
    fn test_line_char_len() {
        let text = "Привет\nWorld";
        let index = LineIndex::new(text);

        assert_eq!(index.line_char_len(text, 0), Some(6));
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
        let text = "hello";
        let range = TextRange::new(TextSize::from(0), TextSize::from(5));
        assert_eq!(LineIndex::utf16_len(text, range), 5);

        let text = "ПрограммныйИнтерфейс";
        let range = TextRange::new(TextSize::from(0), TextSize::from(40));
        assert_eq!(LineIndex::utf16_len(text, range), 20);

        let text = "Функция";
        let range = TextRange::new(TextSize::from(0), TextSize::from(14));
        assert_eq!(LineIndex::utf16_len(text, range), 7);

        let text = "hello😀world";
        let range = TextRange::new(TextSize::from(5), TextSize::from(9));
        assert_eq!(LineIndex::utf16_len(text, range), 2);
    }

    #[test]
    fn test_utf16_col() {
        let text = "    Перем ЛокальнаяПеременная;";
        let index = LineIndex::new(text);

        assert_eq!(index.utf16_col(text, 0, 4), 4);
        assert_eq!(index.utf16_col(text, 0, 14), 9);
        assert_eq!(index.utf16_col(text, 0, 15), 10);

        let text = "Привет Мир";
        let index = LineIndex::new(text);

        assert_eq!(index.utf16_col(text, 0, 12), 6);
        assert_eq!(index.utf16_col(text, 0, 13), 7);
    }

    #[test]
    fn test_utf16_col_invalid_offset() {
        let text = "ВладимирБондаревский";
        let index = LineIndex::new(text);

        let utf16_col = index.utf16_col(text, 0, 1);
        assert_eq!(utf16_col, 0, "Should round down to char boundary");

        let utf16_col = index.utf16_col(text, 0, 3);
        assert_eq!(utf16_col, 1, "Should round down to start of 'л'");
    }

    #[test]
    fn test_utf16_len_invalid_range() {
        let text = "ПрограммныйИнтерфейс";

        let valid_range = TextRange::new(0.into(), 4.into());
        assert_eq!(LineIndex::utf16_len(text, valid_range), 2);

        let invalid_range = TextRange::new(0.into(), 3.into());
        let len = LineIndex::utf16_len(text, invalid_range);
        assert_eq!(len, 1, "Should handle invalid range endpoint");
    }

    #[test]
    fn test_utf16_col_to_byte_col_ascii() {
        let text = "hello world";
        let index = LineIndex::new(text);

        assert_eq!(index.utf16_col_to_byte_col(text, 0, 0), Some(0));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 6), Some(6));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 11), Some(11));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 12), None);
    }

    #[test]
    fn test_utf16_col_to_byte_col_cyrillic() {
        let text = "Процедура Тест";
        let index = LineIndex::new(text);

        assert_eq!(index.utf16_col_to_byte_col(text, 0, 0), Some(0));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 1), Some(2));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 9), Some(18));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 10), Some(19));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 14), Some(27));
    }

    #[test]
    fn test_utf16_col_to_byte_col_mixed() {
        let text = "Функция Test";
        let index = LineIndex::new(text);

        assert_eq!(index.utf16_col_to_byte_col(text, 0, 0), Some(0));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 7), Some(14));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 8), Some(15));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 12), Some(19));
    }

    #[test]
    fn test_utf16_col_to_byte_col_multiline() {
        let text = "Процедура\nТест()";
        let index = LineIndex::new(text);

        assert_eq!(index.utf16_col_to_byte_col(text, 0, 0), Some(0));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 9), Some(18));

        assert_eq!(index.utf16_col_to_byte_col(text, 1, 0), Some(0));
        assert_eq!(index.utf16_col_to_byte_col(text, 1, 4), Some(8));
        assert_eq!(index.utf16_col_to_byte_col(text, 1, 6), Some(10));
    }

    #[test]
    fn test_utf16_col_to_byte_col_emoji() {
        let text = "hello😀world";
        let index = LineIndex::new(text);

        assert_eq!(index.utf16_col_to_byte_col(text, 0, 0), Some(0));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 5), Some(5));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 7), Some(9));
        assert_eq!(index.utf16_col_to_byte_col(text, 0, 12), Some(14));
    }

    #[test]
    fn test_utf16_col_to_byte_col_roundtrip() {
        let text = "Процедура Тест";
        let index = LineIndex::new(text);

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
