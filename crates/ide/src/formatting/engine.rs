//! Formatting engine.
//!
//! Traverses the syntax tree and produces formatted output.

use syntax::{SyntaxNode, TextRange, TextSize};

use super::config::FormattingConfig;

/// Result of formatting operation.
#[derive(Debug, Clone)]
pub struct FormattingResult {
    /// The formatted text.
    pub text: String,
    /// Text edits to transform original to formatted (for minimal diff).
    pub edits: Vec<TextEdit>,
}

/// A single text edit.
#[derive(Debug, Clone)]
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}

/// Formats an entire BSL file.
pub fn format_file(root: &SyntaxNode, config: &FormattingConfig) -> FormattingResult {
    let text = root.text().to_string();
    let formatted = format_text(&text, root, config);
    let edits = compute_edits(&text, &formatted);
    FormattingResult { text: formatted, edits }
}

/// Formats a range within a BSL file via the IR pipeline. Strategy: format
/// the whole document, then slice the result to the line-aligned source
/// range. The IR pipeline preserves line count (each source line maps to
/// one output line) so the slice is well-defined.
pub fn format_range(
    root: &SyntaxNode,
    range: TextRange,
    config: &FormattingConfig,
) -> FormattingResult {
    let text = root.text().to_string();

    let line_ranges = compute_line_ranges(&text);
    if line_ranges.is_empty() {
        return FormattingResult { text: text.clone(), edits: vec![] };
    }

    let range_start = u32::from(range.start()) as usize;
    let range_end = u32::from(range.end()) as usize;
    let start_line =
        line_ranges.iter().position(|(s, e)| range_start >= *s && range_start <= *e).unwrap_or(0);
    let end_line = line_ranges
        .iter()
        .position(|(s, e)| range_end >= *s && range_end <= *e)
        .unwrap_or(line_ranges.len().saturating_sub(1));

    let (src_start, _) = line_ranges[start_line];
    let (_, src_end) = line_ranges[end_line];
    let source_slice = &text[src_start..src_end];

    let formatted_full = format_text_via_ir(&text, root, config);
    let fmt_line_ranges = compute_line_ranges(&formatted_full);
    let (fmt_start, _) = fmt_line_ranges.get(start_line).copied().unwrap_or((src_start, src_end));
    let (_, fmt_end) = fmt_line_ranges.get(end_line).copied().unwrap_or((src_start, src_end));
    let formatted_slice = &formatted_full[fmt_start..fmt_end];

    if formatted_slice == source_slice {
        return FormattingResult { text: source_slice.to_string(), edits: vec![] };
    }
    let actual_range =
        TextRange::new(TextSize::from(src_start as u32), TextSize::from(src_end as u32));
    FormattingResult {
        text: formatted_slice.to_string(),
        edits: vec![TextEdit { range: actual_range, new_text: formatted_slice.to_string() }],
    }
}

/// Computes the byte ranges (start, end) for each line in the text.
/// Returns ranges that include the line content but NOT the newline characters.
fn compute_line_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut line_start = 0;

    for (i, c) in text.char_indices() {
        if c == '\n' {
            // End of line - exclude the newline itself
            // Also handle CRLF: if previous char was \r, exclude it too
            let line_end =
                if i > 0 && text.as_bytes().get(i - 1) == Some(&b'\r') { i - 1 } else { i };
            ranges.push((line_start, line_end));
            line_start = i + 1;
        }
    }

    // Don't forget the last line (if no trailing newline)
    if line_start <= text.len() {
        let line_end = if text.ends_with('\r') { text.len() - 1 } else { text.len() };
        ranges.push((line_start, line_end));
    }

    ranges
}

/// Formats text via the IR pipeline (Phase 2). String literals, BOM, and
/// `+`-style line continuations are preserved by construction. Adds the
/// final newline if [`FormattingConfig::insert_final_newline`] is set.
fn format_text(text: &str, root: &SyntaxNode, config: &FormattingConfig) -> String {
    let mut out = format_text_via_ir(text, root, config);
    if config.insert_final_newline && !out.is_empty() && !out.ends_with('\n') {
        out.push_str(detect_line_ending(text));
    }
    out
}

/// Core IR pipeline shared by `format_text` and `format_range`. Skips
/// `insert_final_newline` so that callers can slice the output without
/// shifting line indices.
fn format_text_via_ir(text: &str, root: &SyntaxNode, config: &FormattingConfig) -> String {
    let ir = super::ir::Ir::build(root);
    let decisions = super::ir::apply_policy(&ir, config, 0);
    let line_ending = detect_line_ending(text);
    let mut out = super::ir::render_with_line_ending(&ir, &decisions, config, line_ending);
    if config.trim_trailing_whitespace {
        out = trim_trailing_whitespace_per_line(&out, line_ending);
    }
    out
}

/// Strips trailing horizontal whitespace from each line. Blank lines —
/// whose *only* content is leading whitespace — are kept verbatim so
/// re-indented blank lines inside a block don't collapse to width zero.
fn trim_trailing_whitespace_per_line(s: &str, line_ending: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut first = true;
    for line in s.split(line_ending) {
        if !first {
            result.push_str(line_ending);
        }
        first = false;
        if line.chars().all(|c| c == ' ' || c == '\t') {
            // Blank-but-indented line: keep the indent as content.
            result.push_str(line);
        } else {
            result.push_str(line.trim_end_matches([' ', '\t']));
        }
    }
    result
}

/// Detects the line ending style used in the text.
/// Returns "\r\n" for CRLF, "\n" for LF.
fn detect_line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Computes minimal text edits between original and formatted text.
fn compute_edits(original: &str, formatted: &str) -> Vec<TextEdit> {
    if original == formatted {
        return vec![];
    }

    // Simple approach: find differing ranges
    let orig_lines: Vec<&str> = original.lines().collect();
    let fmt_lines: Vec<&str> = formatted.lines().collect();

    let mut edits = Vec::new();
    let mut offset = 0u32;

    let max_lines = orig_lines.len().max(fmt_lines.len());

    for i in 0..max_lines {
        let orig_line = orig_lines.get(i).copied().unwrap_or("");
        let fmt_line = fmt_lines.get(i).copied().unwrap_or("");

        if orig_line != fmt_line {
            let line_start = TextSize::from(offset);
            let line_end = TextSize::from(offset + orig_line.len() as u32);

            edits.push(TextEdit {
                range: TextRange::new(line_start, line_end),
                new_text: fmt_line.to_string(),
            });
        }

        offset += orig_line.len() as u32 + 1; // +1 for newline
    }

    // Handle trailing newline difference
    let orig_has_final_nl = original.ends_with('\n');
    let fmt_has_final_nl = formatted.ends_with('\n');

    if orig_has_final_nl != fmt_has_final_nl {
        if fmt_has_final_nl && !orig_has_final_nl {
            // Add newline at end
            let end = TextSize::from(original.len() as u32);
            edits.push(TextEdit { range: TextRange::new(end, end), new_text: "\n".to_string() });
        } else if !fmt_has_final_nl && orig_has_final_nl {
            // Remove newline at end
            let start = TextSize::from((original.len() - 1) as u32);
            let end = TextSize::from(original.len() as u32);
            edits.push(TextEdit { range: TextRange::new(start, end), new_text: String::new() });
        }
    }

    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(code: &str) -> String {
        let parsed = parser::parse(code);
        let root = parsed.syntax_node();
        let config = FormattingConfig::default();
        format_file(&root, &config).text
    }

    #[test]
    fn test_simple_procedure() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let formatted = format(code);
        assert_eq!(formatted, "Процедура Тест()\nКонецПроцедуры\n");
    }

    #[test]
    fn test_procedure_with_body() {
        let code = "Процедура Тест()\nА = 1;\nКонецПроцедуры";
        let formatted = format(code);
        assert_eq!(formatted, "Процедура Тест()\n\tА = 1;\nКонецПроцедуры\n");
    }

    #[test]
    fn test_nested_if() {
        let code = "Процедура Тест()\nЕсли А Тогда\nБ = 1;\nКонецЕсли;\nКонецПроцедуры";
        let formatted = format(code);
        let expected =
            "Процедура Тест()\n\tЕсли А Тогда\n\t\tБ = 1;\n\tКонецЕсли;\nКонецПроцедуры\n";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_if_else() {
        let code = "Если А Тогда\nБ = 1;\nИначе\nВ = 2;\nКонецЕсли;";
        let formatted = format(code);
        let expected = "Если А Тогда\n\tБ = 1;\nИначе\n\tВ = 2;\nКонецЕсли;\n";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_try_except() {
        let code = "Попытка\nА = 1;\nИсключение\nБ = 2;\nКонецПопытки;";
        let formatted = format(code);
        let expected = "Попытка\n\tА = 1;\nИсключение\n\tБ = 2;\nКонецПопытки;\n";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_region() {
        let code = "#Область Тест\nА = 1;\n#КонецОбласти";
        let formatted = format(code);
        let expected = "#Область Тест\n\tА = 1;\n#КонецОбласти\n";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_trim_trailing_whitespace() {
        let code = "Процедура Тест()   \nКонецПроцедуры  ";
        let formatted = format(code);
        assert_eq!(formatted, "Процедура Тест()\nКонецПроцедуры\n");
    }

    #[test]
    fn test_empty_lines() {
        // Empty lines should have indent to match 1C Configurator behavior
        let code = "Процедура Тест()\n\n\tА = 1;\n\nКонецПроцедуры";
        let formatted = format(code);
        assert_eq!(formatted, "Процедура Тест()\n\t\n\tА = 1;\n\t\nКонецПроцедуры\n");
    }

    #[test]
    fn test_compute_edits_no_change() {
        let edits = compute_edits("hello", "hello");
        assert!(edits.is_empty());
    }

    #[test]
    fn test_compute_edits_with_change() {
        let edits = compute_edits("hello", "world");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "world");
    }

    #[test]
    fn test_range_formatting_middle_lines() {
        // Test range formatting of middle lines in a procedure
        // "Процедура Тест()" = 29 bytes (UTF-8), + \n = 30 bytes
        let code = "Процедура Тест()\n    А = 1;\n    Б = 2;\nКонецПроцедуры";
        let parsed = parser::parse(code);
        let root = parsed.syntax_node();
        let config = FormattingConfig::default();

        // Format lines 1-2 (А = 1; and Б = 2;)
        // Line 1 starts at byte 30
        let line1_start = "Процедура Тест()\n".len() as u32;
        let range = TextRange::new(TextSize::from(line1_start), TextSize::from(line1_start + 20));
        let result = format_range(&root, range, &config);

        // Should format with tabs, not 4 spaces
        assert!(result.text.contains('\t'), "Should use tabs: {:?}", result.text);
        assert!(!result.edits.is_empty(), "Should have edits");
    }

    #[test]
    fn test_range_formatting_preserves_surrounding() {
        // Ensure range formatting doesn't corrupt surrounding text
        let code = "Процедура Тест()\n    А = 1;\nКонецПроцедуры";
        let parsed = parser::parse(code);
        let root = parsed.syntax_node();
        let config = FormattingConfig::default();

        // Format only line 1 (А = 1;)
        let line1_start = "Процедура Тест()\n".len() as u32;
        let range = TextRange::new(TextSize::from(line1_start), TextSize::from(line1_start + 10));
        let result = format_range(&root, range, &config);

        // Check that the edit range is correct
        if !result.edits.is_empty() {
            let edit = &result.edits[0];
            // The edit should only cover the selected line, not touch header
            assert!(
                u32::from(edit.range.start()) >= line1_start,
                "Edit should not touch header, got start: {}, expected >= {}",
                u32::from(edit.range.start()),
                line1_start
            );
        }
    }

    #[test]
    fn test_compute_line_ranges() {
        // Test LF line endings
        let ranges = compute_line_ranges("line0\nline1\nline2");
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], (0, 5)); // "line0"
        assert_eq!(ranges[1], (6, 11)); // "line1"
        assert_eq!(ranges[2], (12, 17)); // "line2"

        // Test CRLF line endings
        let ranges = compute_line_ranges("line0\r\nline1\r\nline2");
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], (0, 5)); // "line0" (excluding \r)
        assert_eq!(ranges[1], (7, 12)); // "line1"
        assert_eq!(ranges[2], (14, 19)); // "line2"
    }

    #[test]
    fn test_format_real_code_performance() {
        let code = r#"// NIA 01.06.2023 АРМ Управления неликвидами - СР--0018026.
// Запускаем перевод номенклатуры в неликвид и в исходную
//
Процедура ПереводНоменклатурыВНеликвидИВИсходную() Экспорт

    Организация = Справочники.Организации.НайтиПоРеквизиту("ИНН", "4802024282"); // ООО Прайм Топ
    // ++ NIA 30.01.2024 Регистр_ Склады для анализа по сроку годности; правка рег. задания по переводу в неликвид по истечению срока годности - СР--0022920.
    // получаем список складов по которым будет производится поиск номенклатуры для перевода в неликвид
    СписокСкладов = ПолучитьСписокСкладов();
    // -- NIA 30.01.2024 СР--0022920.
    // ++ NIA 17.04.2024 Регламентное задание перевод в неликвид по сроку годности - СР--0024089.
    ДатаНачалаВыполненияРегламентногоЗадания = ТекущаяДата();
    // -- NIA 17.04.2024 СР--0024089.
    ПереводВНеликвидНоменклатуру(Организация, СписокСкладов, ДатаНачалаВыполненияРегламентногоЗадания);
    ПереводИзНеликвидНоменклатуры(Организация, СписокСкладов, ДатаНачалаВыполненияРегламентногоЗадания);

КонецПроцедуры

// NIA 01.06.2023 АРМ Управления неликвидами - СР--0018026.
// Запускаем перевод номенклатуры в неликвид
//
Процедура ПереводВНеликвидНоменклатуру(Организация, СписокСкладов, ДатаНачалаВыполненияРегламентногоЗадания)

    ОстаткиПоНоменклатуре = ПолучитьОстаткиПоНоменклатуре(Организация, "ПеревестиВНеликвид", СписокСкладов);
    // ++ PIV 08.09.2025 Автоматический перевод статуса "Нетарный остаток" партии если другая часть партии переводится в ГО - СР--0030729.
    СоздатьДокументыРегламентноеЗадание(Организация, ОстаткиПоНоменклатуре, "ПеревестиВНеликвид", ДатаНачалаВыполненияРегламентногоЗадания, СписокСкладов);
    // -- PIV 08.09.2025 СР--0030729.

КонецПроцедуры

// NIA 01.06.2023 АРМ Управления неликвидами - СР--0018026.
// Запускаем перевод номенклатуры в неликвид
//
Процедура ПереводИзНеликвидНоменклатуры(Организация, СписокСкладов, ДатаНачалаВыполненияРегламентногоЗадания)

    ОстаткиПоНоменклатуре = ПолучитьОстаткиПоНоменклатуре(Организация, "ПеревестиВИсходную", СписокСкладов);
    СоздатьДокументыРегламентноеЗадание(Организация, ОстаткиПоНоменклатуре, "ПеревестиВИсходную", ДатаНачалаВыполненияРегламентногоЗадания);

КонецПроцедуры"#;

        let start = std::time::Instant::now();
        let parsed = parser::parse(code);
        let parse_time = start.elapsed();

        let root = parsed.syntax_node();
        let config = FormattingConfig::default();

        let start = std::time::Instant::now();
        let result = format_file(&root, &config);
        let format_time = start.elapsed();

        println!("Parse time: {:?}", parse_time);
        println!("Format time: {:?}", format_time);
        println!("Code: {} bytes, {} lines", code.len(), code.lines().count());
        println!("Result: {} bytes", result.text.len());

        // Should complete in reasonable time
        assert!(format_time.as_millis() < 1000, "Formatting took too long: {:?}", format_time);
    }

    #[test]
    fn test_format_large_file_performance() {
        // Generate a large file (100 procedures ~ 900 lines)
        let mut code = String::new();
        for i in 0..100 {
            code.push_str(&format!(
                r#"
Процедура Тест{}() Экспорт
    А = Справочники.Организации.НайтиПоРеквизиту("ИНН", "4802024282");
    // Комментарий номер {}
    Б = ПолучитьСписокСкладов();
    В = ТекущаяДата();
    Тест(А, Б, В);
    ЕщёОдинТест(А, Б, В, Г, Д, Е);
КонецПроцедуры
"#,
                i, i
            ));
        }

        println!("Generated file: {} bytes, {} lines", code.len(), code.lines().count());

        let start = std::time::Instant::now();
        let parsed = parser::parse(&code);
        let parse_time = start.elapsed();
        println!("Parse time: {:?}", parse_time);

        let root = parsed.syntax_node();
        let config = FormattingConfig::default();

        let start = std::time::Instant::now();
        let result = format_file(&root, &config);
        let format_time = start.elapsed();
        println!("Full file format time: {:?}", format_time);
        println!("Result: {} bytes", result.text.len());

        // Test range formatting (just ~1000 bytes in the middle)
        let start = std::time::Instant::now();
        let range = TextRange::new(TextSize::from(5000), TextSize::from(6000));
        let _result = format_range(&root, range, &config);
        let range_time = start.elapsed();
        println!("Range format time: {:?}", range_time);

        assert!(format_time.as_millis() < 5000, "Full format took too long: {:?}", format_time);
        assert!(range_time.as_millis() < 1000, "Range format took too long: {:?}", range_time);
    }
}
