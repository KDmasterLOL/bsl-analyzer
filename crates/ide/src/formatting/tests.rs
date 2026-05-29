use expect_test::{expect, Expect};

use super::config::FormattingConfig;
use super::engine::format_file;

fn check(input: &str, expected: Expect) {
    let parsed = parser::parse(input);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    let result = format_file(&root, &config);
    expected.assert_eq(&result.text);
}

fn check_with_spaces(input: &str, expected: Expect) {
    let parsed = parser::parse(input);
    let root = parsed.syntax_node();
    let config = FormattingConfig::with_spaces(4);
    let result = format_file(&root, &config);
    expected.assert_eq(&result.text);
}

#[test]
fn test_empty_file() {
    check("", expect![[""]]);
}

#[test]
fn test_simple_procedure() {
    check(
        "Процедура Тест()
КонецПроцедуры",
        expect![[r#"
            Процедура Тест()
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn test_procedure_with_body() {
    check(
        "Процедура Тест()
А = 1;
Б = 2;
КонецПроцедуры",
        expect![[r#"
            Процедура Тест()
            	А = 1;
            	Б = 2;
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn test_procedure_english() {
    check(
        "Procedure Test()
A = 1;
EndProcedure",
        expect![[r#"
            Procedure Test()
            	A = 1;
            EndProcedure
        "#]],
    );
}

#[test]
fn test_function() {
    check(
        "Функция Тест()
Возврат 1;
КонецФункции",
        expect![[r#"
            Функция Тест()
            	Возврат 1;
            КонецФункции
        "#]],
    );
}

#[test]
fn test_if_statement() {
    check(
        "Если А Тогда
Б = 1;
КонецЕсли;",
        expect![[r#"
            Если А Тогда
            	Б = 1;
            КонецЕсли;
        "#]],
    );
}

#[test]
fn test_if_else() {
    check(
        "Если А Тогда
Б = 1;
Иначе
В = 2;
КонецЕсли;",
        expect![[r#"
            Если А Тогда
            	Б = 1;
            Иначе
            	В = 2;
            КонецЕсли;
        "#]],
    );
}

#[test]
fn test_if_elsif_else() {
    check(
        "Если А Тогда
Б = 1;
ИначеЕсли В Тогда
Г = 2;
Иначе
Д = 3;
КонецЕсли;",
        expect![[r#"
            Если А Тогда
            	Б = 1;
            ИначеЕсли В Тогда
            	Г = 2;
            Иначе
            	Д = 3;
            КонецЕсли;
        "#]],
    );
}

#[test]
fn test_nested_if() {
    check(
        "Если А Тогда
Если Б Тогда
В = 1;
КонецЕсли;
КонецЕсли;",
        expect![[r#"
            Если А Тогда
            	Если Б Тогда
            		В = 1;
            	КонецЕсли;
            КонецЕсли;
        "#]],
    );
}

#[test]
fn test_for_loop() {
    check(
        "Для Сч = 1 По 10 Цикл
А = Сч;
КонецЦикла;",
        expect![[r#"
            Для Сч = 1 По 10 Цикл
            	А = Сч;
            КонецЦикла;
        "#]],
    );
}

#[test]
fn test_for_each_loop() {
    check(
        "Для Каждого Элемент Из Коллекция Цикл
Обработать(Элемент);
КонецЦикла;",
        expect![[r#"
            Для Каждого Элемент Из Коллекция Цикл
            	Обработать(Элемент);
            КонецЦикла;
        "#]],
    );
}

#[test]
fn test_while_loop() {
    check(
        "Пока А < 10 Цикл
А = А + 1;
КонецЦикла;",
        expect![[r#"
            Пока А < 10 Цикл
            	А = А + 1;
            КонецЦикла;
        "#]],
    );
}

#[test]
fn test_try_except() {
    check(
        "Попытка
А = 1 / 0;
Исключение
ОписаниеОшибки();
КонецПопытки;",
        expect![[r#"
            Попытка
            	А = 1 / 0;
            Исключение
            	ОписаниеОшибки();
            КонецПопытки;
        "#]],
    );
}

#[test]
fn test_region() {
    check(
        "#Область Инициализация
А = 1;
Б = 2;
#КонецОбласти",
        expect![[r#"
            #Область Инициализация
            А = 1;
            Б = 2;
            #КонецОбласти
        "#]],
    );
}

#[test]
fn test_preprocessor_if() {
    check(
        "#Если Сервер Тогда
А = 1;
#КонецЕсли",
        expect![[r#"
            #Если Сервер Тогда
            А = 1;
            #КонецЕсли
        "#]],
    );
}

#[test]
fn test_preprocessor_if_else() {
    check(
        "#Если Сервер Тогда
А = 1;
#Иначе
Б = 2;
#КонецЕсли",
        expect![[r#"
            #Если Сервер Тогда
            А = 1;
            #Иначе
            Б = 2;
            #КонецЕсли
        "#]],
    );
}

#[test]
fn test_procedure_in_region() {
    check(
        "#Область ПрограммныйИнтерфейс
Процедура Тест()
А = 1;
КонецПроцедуры
#КонецОбласти",
        expect![[r#"
            #Область ПрограммныйИнтерфейс
            Процедура Тест()
            	А = 1;
            КонецПроцедуры
            #КонецОбласти
        "#]],
    );
}

#[test]
fn test_complex_nesting() {
    check(
        "Процедура Тест()
Если А Тогда
Для Сч = 1 По 10 Цикл
Попытка
Б = Сч;
Исключение
В = 0;
КонецПопытки;
КонецЦикла;
КонецЕсли;
КонецПроцедуры",
        expect![[r#"
            Процедура Тест()
            	Если А Тогда
            		Для Сч = 1 По 10 Цикл
            			Попытка
            				Б = Сч;
            			Исключение
            				В = 0;
            			КонецПопытки;
            		КонецЦикла;
            	КонецЕсли;
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn test_trailing_whitespace_removal() {
    check(
        "Процедура Тест()
А = 1;
КонецПроцедуры  ",
        expect![[r#"
            Процедура Тест()
            	А = 1;
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn test_preserve_empty_lines() {
    check(
        "Процедура Тест()

А = 1;

КонецПроцедуры",
        expect![[r#"
            Процедура Тест()
	
            	А = 1;
	
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn test_spaces_instead_of_tabs() {
    check_with_spaces(
        "Процедура Тест()
А = 1;
КонецПроцедуры",
        expect![[r#"
            Процедура Тест()
                А = 1;
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn test_already_formatted() {
    let input = "Процедура Тест()
\tА = 1;
КонецПроцедуры
";
    let parsed = parser::parse(input);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    let result = format_file(&root, &config);

    assert_eq!(result.text, input);
}

#[test]
fn test_comment_preservation() {
    check(
        "// Комментарий
Процедура Тест()
// Внутренний комментарий
А = 1; // Строчный комментарий
КонецПроцедуры",
        expect![[r#"
            // Комментарий
            Процедура Тест()
            	// Внутренний комментарий
            	А = 1; // Строчный комментарий
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn test_multiline_string() {
    check(
        r#"Текст = "Строка 1
|Строка 2
|Строка 3";"#,
        expect![[r#"
            Текст = "Строка 1
            |Строка 2
            |Строка 3";
        "#]],
    );
}

#[test]
fn test_procedure_statement_without_semicolon() {
    check(
        "Процедура Тест() Экспорт

    ОстаткиПоНоменклатуре = Получить();
    СоздатьДокументы(ОстаткиПоНоменклатуре)

КонецПроцедуры",
        expect![[r#"
            Процедура Тест() Экспорт
	
            	ОстаткиПоНоменклатуре = Получить();
            	СоздатьДокументы(ОстаткиПоНоменклатуре)
	
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn regression_bom_first_line_preserved() {
    check(
        "\u{FEFF}// comment\nПерем А Экспорт;\n",
        expect![[r#"
            ﻿// comment
            Перем А Экспорт;
        "#]],
    );
}

#[test]
fn regression_no_space_before_call_paren() {
    check(
        "Процедура Т()
А = З.Выполнить().Выбрать();
КонецПроцедуры
",
        expect![[r#"
            Процедура Т()
            	А = З.Выполнить().Выбрать();
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn regression_no_space_before_index() {
    check(
        "Процедура Т()
А = ТЗ[0].Состояние;
КонецПроцедуры
",
        expect![[r#"
            Процедура Т()
            	А = ТЗ[0].Состояние;
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn regression_multiline_string_literal_preserved() {
    check(
        "Процедура Т()
А = \"ВЫБРАТЬ
|	X.A
|ИЗ
|	T КАК X\";
КонецПроцедуры
",
        expect![[r#"
            Процедура Т()
            	А = "ВЫБРАТЬ
            |	X.A
            |ИЗ
            |	T КАК X";
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn regression_binary_plus_line_continuation_preserved() {
    check(
        "Процедура Т()
	а = \"foo\"
		+ \": \" + б;
КонецПроцедуры
",
        expect![[r#"
            Процедура Т()
            	а = "foo"
            		+ ": " + б;
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn regression_try_except_body_indent() {
    check(
        "Процедура Т()
Попытка
А = 1;
Исключение
Б = 2;
КонецПопытки;
КонецПроцедуры
",
        expect![[r#"
            Процедура Т()
            	Попытка
            		А = 1;
            	Исключение
            		Б = 2;
            	КонецПопытки;
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn regression_empty_default_args_keep_spaces() {
    check(
        "Процедура Т()
Соединение = Новый HTTPСоединение(Сервер, Порт, , , , 60, ssl);
КонецПроцедуры
",
        expect![[r#"
            Процедура Т()
            	Соединение = Новый HTTPСоединение(Сервер, Порт, , , , 60, ssl);
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn regression_trailing_inline_comment_single_space() {
    check(
        "Функция Ф()
	Возврат 1;
КонецФункции\t\t// trailing
",
        expect![[r#"
            Функция Ф()
            	Возврат 1;
            КонецФункции // trailing
        "#]],
    );
}

#[test]
fn regression_multiline_literal_on_assignment_rhs_reindented() {
    let input = "Процедура Т()\n А =\n \"ВЫБРАТЬ\n |X\";\nКонецПроцедуры\n";
    let parsed = parser::parse(input);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    let output = format_file(&root, &config).text;

    let lines: Vec<&str> = output.lines().collect();
    let quote_line = lines
        .iter()
        .find(|l| l.trim_start().starts_with("\"ВЫБРАТЬ"))
        .expect("formatter dropped the literal");
    let pipe_line =
        lines.iter().find(|l| l.trim_start().starts_with("|")).expect("missing `|` continuation");
    assert!(
        quote_line.starts_with("\t\t") && !quote_line.starts_with("\t\t\t"),
        "opening `\"` must sit at body+1 (#std444 п. 3.1); got {:?}",
        quote_line
    );
    assert!(
        pipe_line.starts_with("\t\t|") && !pipe_line.starts_with("\t\t\t"),
        "`|` continuation must align with opening `\"`; got {:?}",
        pipe_line
    );
}

#[test]
fn regression_multiline_literal_after_plus_continuation_reindented() {
    let input = "Процедура Т()\n А = Б +\n \"ВЫБРАТЬ\n |X\";\nКонецПроцедуры\n";
    let parsed = parser::parse(input);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    let output = format_file(&root, &config).text;

    let lines: Vec<&str> = output.lines().collect();
    let quote_line = lines
        .iter()
        .find(|l| l.trim_start().starts_with("\"ВЫБРАТЬ"))
        .expect("formatter dropped the literal");
    let pipe_line =
        lines.iter().find(|l| l.trim_start().starts_with("|")).expect("missing `|` continuation");
    assert!(
        quote_line.starts_with("\t\t") && !quote_line.starts_with("\t\t\t"),
        "opening `\"` after `+\\n` must sit at body+1 (#std444 п. 3.3); got {:?}",
        quote_line
    );
    assert!(
        pipe_line.starts_with("\t\t|") && !pipe_line.starts_with("\t\t\t"),
        "`|` continuation after `+\\n` must align with opening `\"`; got {:?}",
        pipe_line
    );
}

#[test]
fn regression_multiline_literal_same_line_keeps_pipe_at_source_column() {
    let input = "Процедура Т()\n\tА = \"ВЫБРАТЬ\n|X\";\nКонецПроцедуры\n";
    let parsed = parser::parse(input);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    let output = format_file(&root, &config).text;

    let pipe_line =
        output.lines().find(|l| l.trim_start().starts_with("|")).expect("missing `|` continuation");
    assert!(
        pipe_line.starts_with("|"),
        "`|` must stay at column 0 when literal opens on the same line as `=`; got {:?}",
        pipe_line
    );
}

#[test]
fn regression_multiline_literal_as_call_arg_preserves_user_indent() {
    let input = "Процедура Т()\n\tОбработать(\n\t\t\"ВЫБРАТЬ\n\t\t|*\n\t\t|ИЗ Таблица\");\nКонецПроцедуры\n";
    let parsed = parser::parse(input);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    let output = format_file(&root, &config).text;

    let quote_line = output
        .lines()
        .find(|l| l.trim_start().starts_with("\"ВЫБРАТЬ"))
        .expect("formatter dropped the literal");
    assert!(
        quote_line.starts_with("\t\t"),
        "user-authored argument indent must be preserved; got {:?}",
        quote_line
    );
}

#[test]
fn regression_multiline_literal_as_second_call_arg_preserves_user_indent() {
    let input = "Процедура Т()\n\tОбработать(Первый,\n\t\t\"ВЫБРАТЬ\n\t\t|*\n\t\t|ИЗ Таблица\");\nКонецПроцедуры\n";
    let parsed = parser::parse(input);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    let output = format_file(&root, &config).text;

    let quote_line = output
        .lines()
        .find(|l| l.trim_start().starts_with("\"ВЫБРАТЬ"))
        .expect("formatter dropped the literal");
    assert!(
        quote_line.starts_with("\t\t"),
        "user-authored argument indent must be preserved across COMMA; got {:?}",
        quote_line
    );
}

#[test]
fn regression_comment_spacing_normalized() {
    check(
        "//заголовок\nПроцедура Т()\n\tА = 1; //коммент\nКонецПроцедуры\n",
        expect![[r#"
            // заголовок
            Процедура Т()
            	А = 1; // коммент
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn regression_comment_spacing_preserves_existing_whitespace() {
    check(
        "//   double-space\nПроцедура Т()\n\tА = 1; //\tafter-tab\nКонецПроцедуры\n",
        expect![[r#"
            //   double-space
            Процедура Т()
            	А = 1; //	after-tab
            КонецПроцедуры
        "#]],
    );
}

#[test]
fn regression_comment_spacing_empty_comment_untouched() {
    check(
        "//\nПроцедура Т()\nКонецПроцедуры\n",
        expect![[r#"
            //
            Процедура Т()
            КонецПроцедуры
        "#]],
    );
}

fn format_crlf(input: &str) -> String {
    let parsed = parser::parse(input);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    format_file(&root, &config).text
}

fn apply_edits(source: &str, edits: &[super::engine::TextEdit]) -> String {
    let mut sorted: Vec<_> = edits.iter().collect();
    sorted.sort_by_key(|e| u32::from(e.range.start()));
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0usize;
    for edit in sorted {
        let start = u32::from(edit.range.start()) as usize;
        let end = u32::from(edit.range.end()) as usize;
        assert!(start >= cursor, "overlapping edits not supported in test");
        out.push_str(&source[cursor..start]);
        out.push_str(&edit.new_text);
        cursor = end;
    }
    out.push_str(&source[cursor..]);
    out
}

#[test]
fn edit_path_matches_render_path_lf() {
    let src = "Процедура Т()\nА=1;\nКонецПроцедуры";
    let parsed = parser::parse(src);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    let result = super::engine::format_file(&root, &config);
    assert_eq!(apply_edits(src, &result.edits), result.text);
}

#[test]
fn edit_path_matches_render_path_crlf_with_trailing_comment() {
    let src = "Функция Ф()\r\n\tВозврат 1;\r\nКонецФункции\t\t// trailing\r\n";
    let parsed = parser::parse(src);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    let result = super::engine::format_file(&root, &config);
    let applied = apply_edits(src, &result.edits);
    assert_eq!(applied, result.text, "edit path diverges from render path");
    assert!(
        !applied.contains("\r\r"),
        "applying edits introduced a doubled carriage return: {applied:?}"
    );
}

#[test]
fn crlf_simple_procedure_preserved() {
    let src = "Процедура Тест()\r\nКонецПроцедуры";
    let expected = "Процедура Тест()\r\nКонецПроцедуры\r\n";
    assert_eq!(format_crlf(src), expected);
}

#[test]
fn crlf_body_reindent_uses_crlf() {
    let src = "Процедура Тест()\r\nА = 1;\r\nКонецПроцедуры";
    let expected = "Процедура Тест()\r\n\tА = 1;\r\nКонецПроцедуры\r\n";
    assert_eq!(format_crlf(src), expected);
}

#[test]
fn crlf_blank_lines_inside_body() {
    let src = "Процедура Тест()\r\n\r\nА = 1;\r\n\r\nКонецПроцедуры";
    let expected = "Процедура Тест()\r\n\t\r\n\tА = 1;\r\n\t\r\nКонецПроцедуры\r\n";
    assert_eq!(format_crlf(src), expected);
}

#[test]
fn crlf_trailing_inline_comment_no_trailing_cr() {
    let src = "Функция Ф()\r\n\tВозврат 1;\r\nКонецФункции\t\t// trailing\r\n";
    let expected = "Функция Ф()\r\n\tВозврат 1;\r\nКонецФункции // trailing\r\n";
    assert_eq!(format_crlf(src), expected);
}

#[test]
fn crlf_multiline_string_literal_preserved() {
    let src = "А = \"ВЫБРАТЬ\r\n|\tX.A\r\n|ИЗ\r\n|\tT КАК X\";\r\n";
    assert_eq!(format_crlf(src), src);
}

#[test]
fn crlf_bom_preserved() {
    let src = "\u{FEFF}// header\r\nПроцедура Т()\r\nКонецПроцедуры\r\n";
    assert_eq!(format_crlf(src), src);
}

fn format_full_lines(input: &str) -> Vec<String> {
    let parsed = parser::parse(input);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    let text = super::engine::format_file(&root, &config).text;
    text.split_inclusive('\n').map(|s| s.to_string()).collect()
}

fn format_range_lines(input: &str, start_line: usize, end_line: usize) -> String {
    use syntax::{TextRange, TextSize};
    let parsed = parser::parse(input);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();

    let line_starts: Vec<u32> = std::iter::once(0u32)
        .chain(input.char_indices().filter(|(_, c)| *c == '\n').map(|(i, _)| (i + 1) as u32))
        .collect();
    let start = line_starts[start_line];
    let end = line_starts.get(end_line + 1).copied().unwrap_or(input.len() as u32);
    let range = TextRange::new(TextSize::from(start), TextSize::from(end.saturating_sub(1)));
    super::engine::format_range(&root, range, &config).text
}

#[test]
fn range_parity_middle_line() {
    let src = "Процедура Т()\nА=1;\nБ=2;\nКонецПроцедуры";
    let full = format_full_lines(src);
    let slice = format_range_lines(src, 1, 1);
    assert_eq!(slice, full[1].trim_end_matches('\n'));
}

#[test]
fn range_parity_multi_line_span() {
    let src = "Процедура Т()\nА=1;\nБ=2;\nВ=3;\nКонецПроцедуры";
    let full = format_full_lines(src);
    let slice = format_range_lines(src, 1, 3);
    let expected = format!("{}{}{}", full[1], full[2], full[3].trim_end_matches('\n'));
    assert_eq!(slice, expected);
}

#[test]
fn range_parity_header_line() {
    let src = "Процедура Т()\nА=1;\nКонецПроцедуры";
    let full = format_full_lines(src);
    let slice = format_range_lines(src, 0, 0);
    assert_eq!(slice, full[0].trim_end_matches('\n'));
}

#[test]
fn range_full_file_matches_format_file_sans_final_newline() {
    let src = "Процедура Т()\nА=1;\nКонецПроцедуры";
    let parsed = parser::parse(src);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    let full = super::engine::format_file(&root, &config).text;
    let last_line = src.matches('\n').count();
    let slice = format_range_lines(src, 0, last_line);
    assert_eq!(slice, full.trim_end_matches('\n'));
}

#[test]
fn range_unaligned_offset_snaps_to_lines() {
    use syntax::{TextRange, TextSize};
    let src = "Процедура Т()\nА=1;\nБ=2;\nКонецПроцедуры";
    let parsed = parser::parse(src);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    let line1_start = "Процедура Т()\n".len() as u32;
    let range = TextRange::new(TextSize::from(line1_start + 1), TextSize::from(line1_start + 2));
    let out = super::engine::format_range(&root, range, &config).text;
    assert_eq!(out, "\tА = 1;");
}

#[test]
fn range_end_on_newline_does_not_leak_edits_to_eof() {
    use syntax::{TextRange, TextSize};
    let mut src = String::new();
    src.push_str("Процедура Т()\r\n");
    for i in 0..10 {
        src.push_str(&format!("\tА{i} = 1;\r\n"));
    }
    let mid_start = src.len();
    for i in 0..5 {
        src.push_str(&format!("Б{i}=2;\r\n"));
    }
    let mid_end = src.len();
    for i in 0..10 {
        src.push_str(&format!("\tВ{i} = 3;\r\n"));
    }
    src.push_str("КонецПроцедуры\r\n");

    let parsed = parser::parse(&src);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();

    let range =
        TextRange::new(TextSize::from(mid_start as u32), TextSize::from((mid_end - 1) as u32));
    let result = super::engine::format_range(&root, range, &config);

    for edit in &result.edits {
        let start = u32::from(edit.range.start()) as usize;
        let end = u32::from(edit.range.end()) as usize;
        assert!(
            end <= mid_end + 2,
            "edit {:?} reaches past the requested span (mid_end={mid_end})",
            edit
        );
        assert!(
            start + 1 >= mid_start,
            "edit {:?} starts before the requested span (mid_start={mid_start})",
            edit
        );
    }
}

#[test]
fn range_idempotent_on_already_formatted() {
    let src = "Процедура Т()\n\tА = 1;\nКонецПроцедуры\n";
    let parsed = parser::parse(src);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    use syntax::{TextRange, TextSize};
    let line1_start = "Процедура Т()\n".len() as u32;
    let range = TextRange::new(TextSize::from(line1_start), TextSize::from(line1_start + 8));
    let result = super::engine::format_range(&root, range, &config);
    assert!(result.edits.is_empty(), "idempotent range should yield no edits: {:?}", result);
}

#[test]
fn crlf_and_lf_parity_modulo_line_ending() {
    let src_lf = "Процедура Т()\nЕсли А Тогда\nБ = 1;\nИначе\nВ = 2;\nКонецЕсли;\nКонецПроцедуры";
    let src_crlf = src_lf.replace('\n', "\r\n");
    let out_lf = format_crlf(src_lf);
    let out_crlf = format_crlf(&src_crlf);
    assert_eq!(out_crlf, out_lf.replace('\n', "\r\n"));
}
