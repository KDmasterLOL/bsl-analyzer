use syntax::{SyntaxNode, TextRange, TextSize};

use super::config::FormattingConfig;

#[derive(Debug, Clone)]
pub struct FormattingResult {
    pub text: String,
    pub edits: Vec<TextEdit>,
}

#[derive(Debug, Clone)]
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}

pub fn format_file(root: &SyntaxNode, config: &FormattingConfig) -> FormattingResult {
    let source = root.text().to_string();
    let (text, edits) = render_full(root, config, &source);
    FormattingResult { text, edits: convert_edits(edits) }
}

pub fn format_range(
    root: &SyntaxNode,
    range: TextRange,
    config: &FormattingConfig,
) -> FormattingResult {
    let source = root.text().to_string();

    let range_config = FormattingConfig { insert_final_newline: false, ..config.clone() };
    let (formatted_full, all_edits) = render_full(root, &range_config, &source);

    let line_ranges = compute_line_ranges(&source);
    if line_ranges.is_empty() {
        return FormattingResult { text: source, edits: vec![] };
    }

    let range_start = u32::from(range.start()) as usize;
    let range_end = u32::from(range.end()) as usize;
    let start_line = line_for_offset(&source, range_start);
    let end_line = line_for_offset(&source, range_end);
    let last_idx = line_ranges.len().saturating_sub(1);
    let start_line = start_line.min(last_idx);
    let end_line = end_line.min(last_idx).max(start_line);

    let (src_start, _) = line_ranges[start_line];
    let (_, src_end) = line_ranges[end_line];
    let span = TextRange::new(TextSize::from(src_start as u32), TextSize::from(src_end as u32));

    let edits: Vec<TextEdit> = all_edits
        .into_iter()
        .filter(|e| ranges_overlap(e.range, span))
        .map(|e| TextEdit { range: e.range, new_text: e.new_text })
        .collect();

    let fmt_line_ranges = compute_line_ranges(&formatted_full);
    let (fmt_start, _) = fmt_line_ranges.get(start_line).copied().unwrap_or((src_start, src_end));
    let (_, fmt_end) = fmt_line_ranges.get(end_line).copied().unwrap_or((src_start, src_end));
    let formatted_slice = formatted_full[fmt_start..fmt_end].to_string();

    FormattingResult { text: formatted_slice, edits }
}

fn render_full(
    root: &SyntaxNode,
    config: &FormattingConfig,
    source: &str,
) -> (String, Vec<super::ir::GapEdit>) {
    let ir = super::ir::Ir::build(root);
    let decisions = super::ir::apply_policy(&ir, config, 0);
    let line_ending = detect_line_ending(source);
    super::ir::render_full(&ir, &decisions, config, line_ending, config.insert_final_newline)
}

fn convert_edits(edits: Vec<super::ir::GapEdit>) -> Vec<TextEdit> {
    edits.into_iter().map(|e| TextEdit { range: e.range, new_text: e.new_text }).collect()
}

fn line_for_offset(text: &str, offset: usize) -> usize {
    let bounded = offset.min(text.len());
    text.as_bytes()[..bounded].iter().filter(|&&b| b == b'\n').count()
}

fn ranges_overlap(a: TextRange, b: TextRange) -> bool {
    a.start() < b.end() && b.start() < a.end()
        || a.start() == a.end() && b.start() <= a.start() && a.start() <= b.end()
        || b.start() == b.end() && a.start() <= b.start() && b.start() <= a.end()
}

fn compute_line_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut line_start = 0;

    for (i, c) in text.char_indices() {
        if c == '\n' {
            let line_end =
                if i > 0 && text.as_bytes().get(i - 1) == Some(&b'\r') { i - 1 } else { i };
            ranges.push((line_start, line_end));
            line_start = i + 1;
        }
    }

    if line_start <= text.len() {
        let line_end = if text.ends_with('\r') { text.len() - 1 } else { text.len() };
        ranges.push((line_start, line_end));
    }

    ranges
}

fn detect_line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
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
        let expected = "#Область Тест\nА = 1;\n#КонецОбласти\n";
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
        let code = "Процедура Тест()\n\n\tА = 1;\n\nКонецПроцедуры";
        let formatted = format(code);
        assert_eq!(formatted, "Процедура Тест()\n\t\n\tА = 1;\n\t\nКонецПроцедуры\n");
    }

    #[test]
    fn test_range_formatting_middle_lines() {
        let code = "Процедура Тест()\n    А = 1;\n    Б = 2;\nКонецПроцедуры";
        let parsed = parser::parse(code);
        let root = parsed.syntax_node();
        let config = FormattingConfig::default();

        let line1_start = "Процедура Тест()\n".len() as u32;
        let range = TextRange::new(TextSize::from(line1_start), TextSize::from(line1_start + 20));
        let result = format_range(&root, range, &config);

        assert!(result.text.contains('\t'), "Should use tabs: {:?}", result.text);
        assert!(!result.edits.is_empty(), "Should have edits");
    }

    #[test]
    fn test_range_formatting_preserves_surrounding() {
        let code = "Процедура Тест()\n    А = 1;\nКонецПроцедуры";
        let parsed = parser::parse(code);
        let root = parsed.syntax_node();
        let config = FormattingConfig::default();

        let header = "Процедура Тест()";
        let line1_start = "Процедура Тест()\n".len() as u32;
        let range = TextRange::new(TextSize::from(line1_start), TextSize::from(line1_start + 10));
        let result = format_range(&root, range, &config);

        for edit in &result.edits {
            assert!(
                u32::from(edit.range.end()) <= header.len() as u32
                    || u32::from(edit.range.start()) >= header.len() as u32,
                "edit {:?} straddles the header content",
                edit
            );
            if u32::from(edit.range.start()) == header.len() as u32 {
                assert!(
                    edit.new_text.starts_with('\n'),
                    "edit at line boundary must preserve newline: {:?}",
                    edit
                );
            }
        }
    }

    #[test]
    fn test_compute_line_ranges() {
        let ranges = compute_line_ranges("line0\nline1\nline2");
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], (0, 5));
        assert_eq!(ranges[1], (6, 11));
        assert_eq!(ranges[2], (12, 17));

        let ranges = compute_line_ranges("line0\r\nline1\r\nline2");
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], (0, 5));
        assert_eq!(ranges[1], (7, 12));
        assert_eq!(ranges[2], (14, 19));
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

        assert!(format_time.as_millis() < 1000, "Formatting took too long: {:?}", format_time);
    }

    #[test]
    fn test_format_large_file_performance() {
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

        let start = std::time::Instant::now();
        let range = TextRange::new(TextSize::from(5000), TextSize::from(6000));
        let _result = format_range(&root, range, &config);
        let range_time = start.elapsed();
        println!("Range format time: {:?}", range_time);

        assert!(format_time.as_millis() < 5000, "Full format took too long: {:?}", format_time);
        assert!(range_time.as_millis() < 1000, "Range format took too long: {:?}", range_time);
    }
}
