//! Tests for BSL formatting.

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
    // Empty lines should have indent to match 1C Configurator behavior
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

    // Should have no edits if already formatted
    // Note: We check text equality since the algorithm may produce equivalent output
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
    // Multiline strings with | continuation marker should NOT get extra indent
    // The | marker should stay at the same level as the opening quote
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
    // Statement without semicolon should NOT cause КонецПроцедуры to be indented
    // Empty lines inside procedure get indent (1C Configurator behavior)
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

// ---------------------------------------------------------------------------
// Regression tests derived from real-world ObjectModule.bsl breakage.
//
// Each `#[ignore]` marks a currently-broken behavior. The `expect![]` block
// captures the DESIRED output. Remove the `#[ignore]` once the formatter is
// fixed. Run the full set with:
//
//     cargo test -p ide formatting -- --ignored
//
// Design decisions backing these expectations:
//   * String literal contents are NEVER edited by the formatter (incl. `|`
//     continuations of SDBL queries).
//   * `+`-prefixed line continuations are preserved as the user wrote them
//     (no active reflow, no space loss after the operator).
//   * Trailing inline comments collapse leading whitespace to a single space.
// ---------------------------------------------------------------------------

#[test]
fn regression_bom_first_line_preserved() {
    check(
        "\u{FEFF}//comment\nПерем А Экспорт;\n",
        expect![[r#"
            ﻿//comment
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
    // The string content — including the `|` continuation lines — must be
    // emitted byte-for-byte. Only the surrounding statement gets re-indented.
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
    // The newline before `+` is user-authored. Formatter must not collapse it,
    // must not glue `+":"`, must preserve the space after the operator.
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
    // Baseline: minimal Попытка/Исключение indents correctly. The real-world
    // ObjectModule.bsl bug — body losing one indent level — needs a more
    // complex reproducer (nested if/else inside try). Add when isolated.
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
    // Each comma in an argument list is followed by exactly one space, even if
    // the next token is another comma (skipped default parameter).
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
    // Baseline (currently correct): any run of whitespace between an end
    // keyword and a trailing `//` comment collapses to one space.
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

// ----- CRLF line-ending tests -----
//
// The formatter detects the source line ending and emits synthesized
// newlines (re-indentation, block boundaries) using the same. Tests below
// use literal `\r\n` strings rather than `expect!` to avoid raw-string
// escaping noise; the assertions are exact equality on the bytes.

fn format_crlf(input: &str) -> String {
    let parsed = parser::parse(input);
    let root = parsed.syntax_node();
    let config = FormattingConfig::default();
    format_file(&root, &config).text
}

#[test]
fn crlf_simple_procedure_preserved() {
    let src = "Процедура Тест()\r\nКонецПроцедуры";
    let expected = "Процедура Тест()\r\nКонецПроцедуры\r\n";
    assert_eq!(format_crlf(src), expected);
}

#[test]
fn crlf_body_reindent_uses_crlf() {
    // Indent inserted by the policy must use the source's line ending.
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
    // The lexer's `//[^\n]*` regex eats the `\r` into the COMMENT token in
    // CRLF files. `Ir::build` strips it; the rendered comment must not
    // carry the spurious `\r`, and the line ending stays `\r\n`.
    let src = "Функция Ф()\r\n\tВозврат 1;\r\nКонецФункции\t\t// trailing\r\n";
    let expected = "Функция Ф()\r\n\tВозврат 1;\r\nКонецФункции // trailing\r\n";
    assert_eq!(format_crlf(src), expected);
}

#[test]
fn crlf_multiline_string_literal_preserved() {
    // String content carries its own `\r\n` separators — they must round-
    // trip byte-for-byte (LITERAL coalescing keeps the atom opaque).
    let src = "А = \"ВЫБРАТЬ\r\n|\tX.A\r\n|ИЗ\r\n|\tT КАК X\";\r\n";
    assert_eq!(format_crlf(src), src);
}

#[test]
fn crlf_bom_preserved() {
    let src = "\u{FEFF}// header\r\nПроцедура Т()\r\nКонецПроцедуры\r\n";
    assert_eq!(format_crlf(src), src);
}

#[test]
fn crlf_and_lf_parity_modulo_line_ending() {
    // Formatting parity: replacing CRLF with LF in the source yields LF
    // output that mirrors the CRLF output line-for-line.
    let src_lf = "Процедура Т()\nЕсли А Тогда\nБ = 1;\nИначе\nВ = 2;\nКонецЕсли;\nКонецПроцедуры";
    let src_crlf = src_lf.replace('\n', "\r\n");
    let out_lf = format_crlf(src_lf);
    let out_crlf = format_crlf(&src_crlf);
    assert_eq!(out_crlf, out_lf.replace('\n', "\r\n"));
}
