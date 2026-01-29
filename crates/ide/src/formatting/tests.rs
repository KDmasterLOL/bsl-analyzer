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
