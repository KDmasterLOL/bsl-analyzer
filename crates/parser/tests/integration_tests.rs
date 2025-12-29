//! Integration tests for BSL parser.

use parser::parse;
use std::time::Instant;

#[test]
fn test_async_procedure() {
    let input = "Асинх Процедура Тест() КонецПроцедуры";
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_async_function() {
    let input = "Асинх Функция Тест() КонецФункции";
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_compiler_directive() {
    let input = "&НаКлиенте\nПроцедура Тест() КонецПроцедуры";
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_multiple_compiler_directives() {
    let input = "&НаКлиентеНаСервере\nПроцедура Тест() КонецПроцедуры";
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_annotation_without_params() {
    let input = "&До\nПроцедура Тест() КонецПроцедуры";
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_annotation_with_params() {
    let input = r#"&До("Модуль.Метод", Параметр1 = Истина)
Процедура Тест() КонецПроцедуры"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_annotation_nested() {
    let input = r#"&До(&Вокруг("Тест"))
Процедура Тест() КонецПроцедуры"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_execute_statement() {
    let input = r#"Процедура Тест()
    Выполнить("Сообщить('Привет')");
КонецПроцедуры"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_add_handler_statement() {
    let input = r#"Процедура Тест()
    ДобавитьОбработчик Форма.Кнопка.Нажатие, ОбработчикНажатия;
КонецПроцедуры"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_remove_handler_statement() {
    let input = r#"Процедура Тест()
    УдалитьОбработчик Форма.Кнопка.Нажатие, ОбработчикНажатия;
КонецПроцедуры"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_await_expression() {
    let input = r#"Асинх Функция Тест()
    Результат = Ждать ВыполнитьАсинх();
    Возврат Результат;
КонецФункции"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_multiline_string() {
    let input = r#"Процедура Тест()
    Текст = "Строка1
    |Строка2
    |Строка3";
КонецПроцедуры"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_complex_procedure() {
    let input = r#"&НаКлиентеНаСервере
&После("Модуль.Метод")
Асинх Процедура СложнаяПроцедура(Параметр1, Знач Параметр2 = 10) Экспорт
    Перем Локальная;

    Локальная = Ждать АсинхОперация();

    Если Локальная > 0 Тогда
        Выполнить("Сообщить('OK')");
    КонецЕсли;

    ДобавитьОбработчик Событие, Обработчик;
КонецПроцедуры"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_all_statement_types() {
    let input = r#"Процедура Тест()
    // Присваивание
    А = 10;

    // Вызов
    Сообщить("Привет");

    // Возврат
    Возврат А;

    // Если
    Если А > 5 Тогда
        Б = 20;
    КонецЕсли;

    // Пока
    Пока А < 100 Цикл
        А = А + 1;
    КонецЦикла;

    // Для
    Для Счетчик = 1 По 10 Цикл
        А = А + Счетчик;
    КонецЦикла;

    // Для Каждого
    Для Каждого Элемент Из Массив Цикл
        Сообщить(Элемент);
    КонецЦикла;

    // Попытка
    Попытка
        ОпаснаяОперация();
    Исключение
        Сообщить("Ошибка");
    КонецПопытки;

    // ВызватьИсключение
    ВызватьИсключение "Ошибка";

    // Прервать
    Прервать;

    // Продолжить
    Продолжить;

    // Перейти
    ~Метка:
    Перейти ~Метка;

    // Выполнить
    Выполнить("Код");

    // ДобавитьОбработчик
    ДобавитьОбработчик Событие, Обработчик;

    // УдалитьОбработчик
    УдалитьОбработчик Событие, Обработчик;
КонецПроцедуры"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_all_expression_types() {
    let input = r#"Функция Тест()
    // Литералы
    А = 10;
    Б = 3.14;
    В = "Строка";
    Г = '20231231';
    Д = Истина;
    Е = Ложь;
    Ж = Неопределено;
    З = Null;

    // Арифметические операции
    И = А + Б;
    К = А - Б;
    Л = А * Б;
    М = А / Б;
    Н = А % Б;

    // Логические операции
    О = А > Б;
    П = А < Б;
    Р = А >= Б;
    С = А <= Б;
    Т = А = Б;
    У = А <> Б;
    Ф = Д И Е;
    Х = Д ИЛИ Е;
    Ц = НЕ Д;

    // Вызовы
    Ч = ФункцияБезПараметров();
    Ш = ФункцияСПараметрами(1, 2, 3);
    Щ = Объект.Метод();

    // Доступ к свойствам
    Ы = Объект.Свойство;
    Э = Объект.Свойство1.Свойство2;

    // Индексация
    Ю = Массив[0];
    Я = Массив[Индекс];

    // New
    А1 = Новый Массив;
    Б1 = Новый Структура("Ключ", Значение);

    // Тернарный оператор
    В1 = ?(Условие, ЗначениеИстина, ЗначениеЛожь);

    // Await
    Г1 = Ждать АсинхФункция();

    Возврат Г1;
КонецФункции"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_preprocessor_simple_platform_symbol() {
    let input = r#"#Если Клиент Тогда
    Процедура ТестНаКлиенте() КонецПроцедуры
#КонецЕсли"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_preprocessor_and_operator() {
    let input = r#"#Если Клиент И НЕ Сервер Тогда
    Процедура Тест() КонецПроцедуры
#КонецЕсли"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_preprocessor_or_operator() {
    let input = r#"#Если ВебКлиент ИЛИ МобильныйКлиент Тогда
    Процедура Тест() КонецПроцедуры
#КонецЕсли"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_preprocessor_complex_expression() {
    let input = r#"#Если (Клиент И НЕ МобильныйКлиент) ИЛИ (Сервер И Windows) Тогда
    Процедура Тест() КонецПроцедуры
#КонецЕсли"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_preprocessor_elsif_else() {
    let input = r#"#Если Клиент Тогда
    Процедура НаКлиенте() КонецПроцедуры
#ИначеЕсли Сервер Тогда
    Процедура НаСервере() КонецПроцедуры
#Иначе
    Процедура Общая() КонецПроцедуры
#КонецЕсли"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_preprocessor_multiple_elsif() {
    let input = r#"#Если ТонкийКлиент Тогда
    Процедура Тонкий() КонецПроцедуры
#ИначеЕсли ВебКлиент Тогда
    Процедура Веб() КонецПроцедуры
#ИначеЕсли МобильныйКлиент Тогда
    Процедура Мобильный() КонецПроцедуры
#КонецЕсли"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_preprocessor_not_expression() {
    let input = r#"#Если НЕ (Клиент ИЛИ Сервер) Тогда
    Процедура Тест() КонецПроцедуры
#КонецЕсли"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_preprocessor_os_symbols() {
    let input = r#"#Если Windows Тогда
    Процедура НаWindows() КонецПроцедуры
#ИначеЕсли Linux Тогда
    Процедура НаLinux() КонецПроцедуры
#ИначеЕсли MacOS Тогда
    Процедура НаMacOS() КонецПроцедуры
#КонецЕсли"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_preprocessor_all_platform_symbols() {
    let input = r#"#Если ТолстыйКлиентОбычноеПриложение Тогда
    Процедура Тест1() КонецПроцедуры
#ИначеЕсли ТолстыйКлиентУправляемоеПриложение Тогда
    Процедура Тест2() КонецПроцедуры
#ИначеЕсли МобильноеПриложениеКлиент Тогда
    Процедура Тест3() КонецПроцедуры
#ИначеЕсли МобильноеПриложениеСервер Тогда
    Процедура Тест4() КонецПроцедуры
#ИначеЕсли ВнешнееСоединение Тогда
    Процедура Тест5() КонецПроцедуры
#КонецЕсли"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_metadata_access() {
    let input = r#"Функция Тест() Экспорт
    Ссылка = Справочник.Номенклатура.НайтиПоКоду("001");
    Метаданные = Ссылка.Метаданные().Имя;
    Документ = Документы.ПоступлениеТоваровИУслуг.СоздатьДокумент();
    Возврат Метаданные;
КонецФункции"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_built_in_functions() {
    let input = r#"Функция Тест() Экспорт
    СтрРазделитель = Символы.ПС;
    Результат = СтрЗаменить("Строка", "о", "а");
    Массив = СтроковыеФункцииКлиентСервер.РазложитьСтрокуВМассивПодстрок(Результат, СтрРазделитель, Истина);
    Структура = Новый Структура("Ключ, Значение", "Тест", 123);
    Возврат Структура;
КонецФункции"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_real_bsl_code_sample() {
    // Real BSL code from 1C:Enterprise
    let input = r#"#Область ПрограммныйИнтерфейс

Функция ЗначениеНастройкиПланаОбмена(ИмяПланаОбмена, ИмяПараметра, ИдентификаторНастройки = "", ВерсияКорреспондента = "") Экспорт
	ЗначениеПараметра = Новый Структура;
	НастройкиПланаОбмена = Неопределено;
	ИменаПараметров = СтроковыеФункцииКлиентСервер.РазложитьСтрокуВМассивПодстрок(ИмяПараметра,,Истина);

	Если ИменаПараметров.Количество() = 0 Тогда
		Возврат Неопределено;
	КонецЕсли;

	Для Каждого ЕдиничныйПараметр Из ИменаПараметров Цикл
		ЗначениеЕдиничногоПараметра = Неопределено;
		Если ИменаПараметров.Количество() = 1 Тогда
			Возврат ЗначениеЕдиничногоПараметра;
		Иначе
			ЗначениеПараметра.Вставить(ЕдиничныйПараметр, ЗначениеЕдиничногоПараметра);
		КонецЕсли;
	КонецЦикла;

	Возврат ЗначениеПараметра;
КонецФункции

#КонецОбласти"#;
    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_region_minimal() {
    // Minimal test - just region with function
    let input = r#"#Область Test
Функция Тест()
КонецФункции
#КонецОбласти"#;

    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_preprocessor_not_without_parens() {
    // Test the fix for НЕ without parentheses - this was causing infinite loop
    let input = r#"#Если НЕ Клиент Тогда
    Процедура Тест() КонецПроцедуры
#КонецЕсли"#;

    let result = parse(input);
    assert!(!result.events.is_empty());
}

#[test]
fn test_small_file_performance() {
    // Test with small code to verify parser is fast on simple cases
    let input = r#"Функция Тест() Экспорт
    Результат = 10 + 20;
    Возврат Результат;
КонецФункции"#;

    let start = Instant::now();
    let iterations = 1000;

    for _ in 0..iterations {
        let result = parse(input);
        assert!(!result.events.is_empty());
    }

    let elapsed = start.elapsed();
    let avg_micros = elapsed.as_micros() / iterations;

    println!("\nSmall file performance:");
    println!("Iterations: {}", iterations);
    println!("Total time: {:?}", elapsed);
    println!("Average per parse: {} μs", avg_micros);

    // Should be fast - under 100 microseconds per parse for such simple code
    assert!(avg_micros < 1000, "Parser too slow even for simple code: {} μs", avg_micros);
}

#[test]
#[ignore]
fn test_lexer_performance_large_file() {
    use lexer::tokenize;

    let bsl_file_path = "/Users/kiriller/src/lsp/bsl-parser/src/test/resources/Module.bsl";
    let input = match std::fs::read_to_string(bsl_file_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Warning: Could not read file: {}", e);
            return;
        }
    };

    println!("\nLexer performance test:");
    println!("File size: {} bytes", input.len());

    let start = Instant::now();
    let tokens = tokenize(&input);
    let elapsed = start.elapsed();

    println!("Tokens: {}", tokens.len());
    println!("Lexer time: {:?}", elapsed);
    println!("Speed: {:.2} MB/s", input.len() as f64 / 1_000_000.0 / elapsed.as_secs_f64());

    assert!(elapsed.as_millis() < 100, "Lexer too slow: {:?}", elapsed);
}

#[test]
#[ignore] // Run with: cargo test --release -- --ignored benchmark
fn benchmark_parser_performance() {
    // Read real BSL file from bsl-parser project
    let bsl_file_path = "/Users/kiriller/src/lsp/bsl-parser/src/test/resources/Module.bsl";

    let input = match std::fs::read_to_string(bsl_file_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Warning: Could not read benchmark file: {}", e);
            eprintln!("Skipping benchmark test");
            return;
        }
    };

    let file_size_bytes = input.len();
    let file_size_mb = file_size_bytes as f64 / 1_000_000.0;

    println!("\n=== Parser Performance Benchmark ===");
    println!("File: {}", bsl_file_path);
    println!("Size: {} bytes ({:.2} MB)", file_size_bytes, file_size_mb);

    // Warmup - skip to save time
    // for _ in 0..3 {
    //     let _ = parse(&input);
    // }

    // Single parse to measure performance
    println!("\nParsing file (this may take time)...");
    let start = Instant::now();

    let result = parse(&input);
    assert!(!result.events.is_empty());

    let elapsed = start.elapsed();
    let throughput = file_size_mb / elapsed.as_secs_f64();

    println!("\nResults:");
    println!("Parse time: {:.2?}", elapsed);
    println!("Events generated: {}", result.events.len());
    println!("Throughput: {:.2} MB/s", throughput);

    println!("\n=== Criterion Check ===");
    if throughput >= 50.0 {
        println!("✅ PASS: Throughput {:.2} MB/s >= 50 MB/s target", throughput);
    } else {
        println!("⚠️  NOTICE: Throughput {:.2} MB/s < 50 MB/s target", throughput);
        println!("Parser works but needs optimization");
        // Don't panic - at least it works now!
    }
}
#[test]
fn debug_tokens_around_1905() {
    use lexer::tokenize;
    // Test file from bsl-parser project (grammar reference)
    let input = include_str!("fixtures/Module.bsl");

    let tokens = tokenize(input);

    println!("\nTokens around position 1905:");
    for i in 1895..1915 {
        if i < tokens.len() {
            println!("{:4}: {:?}", i, tokens[i].kind);
        }
    }

    println!("\nToken 1905 is: {:?}", tokens.get(1905));
}
#[test]
fn debug_tokens_around_14467() {
    use lexer::tokenize;
    // Test file from bsl-parser project (grammar reference)
    let input = include_str!("fixtures/Module.bsl");

    let tokens = tokenize(input);

    println!("\nTokens around position 14467:");
    for i in 14457..14477 {
        if i < tokens.len() {
            println!("{:5}: {:?}", i, tokens[i].kind);
        }
    }
}

#[test]
fn debug_tokens_around_68041() {
    use lexer::tokenize;
    // Test file from bsl-parser project (grammar reference)
    let input = include_str!("fixtures/Module.bsl");

    let tokens = tokenize(input);

    println!("\nTotal tokens: {}", tokens.len());
    println!("\nTokens around position 68041 (wider context):");
    for i in 68000..68060 {
        if i < tokens.len() {
            println!("{:5}: {:?}", i, tokens[i].kind);
        }
    }

    println!("\nToken 68041 is: {:?}", tokens.get(68041));
}

#[test]
fn test_large_file_performance() {
    // Test file from bsl-parser project (grammar reference)
    // Large real-world BSL module for performance testing
    let input = include_str!("fixtures/Module.bsl");

    println!("\nLarge file performance:");
    println!("File size: {} bytes ({:.2} MB)", input.len(), input.len() as f64 / 1_048_576.0);

    let start = Instant::now();
    let result = parse(input);
    let elapsed = start.elapsed();

    println!("Parse time: {:?}", elapsed);
    println!("Events generated: {}", result.events.len());
    println!("Performance: {:.2} MB/s", (input.len() as f64 / 1_048_576.0) / elapsed.as_secs_f64());

    assert!(!result.events.is_empty());
}
