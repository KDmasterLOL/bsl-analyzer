mod event;
// Объявлен ДО грамматики и с `macro_use`: макрос `T![…]` порождается другим
// макросом, а такой `macro_export` внутри своего крейта по пути `crate::T`
// недоступен — язык это прямо запрещает. Остаётся текстовая область
// видимости, а она идёт по порядку объявления модулей.
#[macro_use]
mod parser;
pub mod grammar;
mod sdbl_token_converter;
mod sink;
mod syntax_kind;

use lexer::tokenize;

/// Алфавит грамматики выходит наружу вынужденно, а не по широте API: макрос
/// `T![…]` разворачивается в коде вызывающего, и путь к типу обязан
/// разрешаться снаружи крейта — в том числе в `compile_fail`-доктестах,
/// которые собираются как отдельный крейт.
pub use crate::parser::input::Sig;
/// Путь `parser::token_set::TokenSet` сохраняется после переезда модуля внутрь
/// `parser`: переезд нужен видимости `Sig::kind`, а не поверхности крейта.
pub use crate::parser::token_set;
pub use crate::parser::Parser;

pub fn parse(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    let tokens = tokenize(input);
    let mut p = Parser::new(&tokens);
    grammar::source_file(&mut p);
    let events = p.finish();

    let sink = sink::Sink::new(&tokens);
    let builder = sink.finish(events);
    builder.finish()
}

pub fn parse_with_shared_cache(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    let tokens = tokenize(input);
    let mut p = Parser::new(&tokens);
    grammar::source_file(&mut p);
    let events = p.finish();

    syntax::with_shared_node_cache(|cache| {
        let sink = sink::Sink::with_cache(&tokens, cache);
        let builder = sink.finish(events);
        builder.finish()
    })
}

pub fn parse_sdbl(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    use lexer::sdbl::tokenize_sdbl;

    let sdbl_tokens = tokenize_sdbl(input);

    let tokens = sdbl_token_converter::convert_sdbl_tokens(&sdbl_tokens);

    let mut p = parser::Parser::new(&tokens);
    p.set_grammar_boundary(grammar::sdbl::at_query_boundary);
    grammar::sdbl::query_package(&mut p);
    let events = p.finish();

    let sink = sink::Sink::new(&tokens);
    let builder = sink.finish(events);
    builder.finish()
}

pub fn parse_sdbl_with_shared_cache(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    use lexer::sdbl::tokenize_sdbl;

    let sdbl_tokens = tokenize_sdbl(input);
    let tokens = sdbl_token_converter::convert_sdbl_tokens(&sdbl_tokens);

    let mut p = parser::Parser::new(&tokens);
    p.set_grammar_boundary(grammar::sdbl::at_query_boundary);
    grammar::sdbl::query_package(&mut p);
    let events = p.finish();

    syntax::with_shared_node_cache(|cache| {
        let sink = sink::Sink::with_cache(&tokens, cache);
        let builder = sink.finish(events);
        builder.finish()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let parse = parse("Процедура Тест() КонецПроцедуры");
        assert!(!parse.has_errors());
        let root = parse.syntax_node();
        assert_eq!(root.kind(), syntax::SyntaxKind::SOURCE_FILE);
    }

    #[test]
    fn test_parse_with_trivia() {
        let parse = parse("// Comment\nПроцедура Тест()\nКонецПроцедуры");
        assert!(!parse.has_errors());
    }

    fn assert_parses(code: &str) {
        let parse = parse(code);
        if parse.has_errors() {
            for e in parse.errors() {
                eprintln!("parse error: {e:?}");
            }
            panic!("valid 1C code must parse without errors:\n{code}");
        }
    }

    // Region directives are flat folding markers and may cross control-flow
    // boundaries without nesting. The parser must accept such overlap.

    #[test]
    fn region_end_inside_if_body_before_endif() {
        assert_parses(
            "Процедура П(Парам) Экспорт\n\t#Область Р\n\tЕсли Истина Тогда\n\t\tА = 1;\n\t#КонецОбласти\n\tКонецЕсли;\nКонецПроцедуры\n",
        );
    }

    #[test]
    fn region_markers_between_branch_and_elsif() {
        assert_parses(
            "Процедура П(Парам) Экспорт\n\tЕсли А Тогда\n\t\tБ = 1;\n\t#КонецОбласти\n\t#Область Р2\n\tИначеЕсли В Тогда\n\t\tГ = 2;\n\tКонецЕсли;\nКонецПроцедуры\n",
        );
    }

    #[test]
    fn region_end_before_else() {
        assert_parses(
            "Процедура П() Экспорт\n\tЕсли А Тогда\n\t\tБ = 1;\n\t#КонецОбласти\n\tИначе\n\t\tВ = 2;\n\tКонецЕсли;\nКонецПроцедуры\n",
        );
    }

    #[test]
    fn region_end_inside_while_body() {
        assert_parses(
            "Процедура П() Экспорт\n\t#Область Р\n\tПока А Цикл\n\t\tБ = 1;\n\t#КонецОбласти\n\tКонецЦикла;\nКонецПроцедуры\n",
        );
    }

    #[test]
    fn region_end_inside_for_each_body() {
        assert_parses(
            "Процедура П() Экспорт\n\t#Область Р\n\tДля Каждого Э Из К Цикл\n\t\tБ = 1;\n\t#КонецОбласти\n\tКонецЦикла;\nКонецПроцедуры\n",
        );
    }

    #[test]
    fn region_end_inside_try_body() {
        assert_parses(
            "Процедура П() Экспорт\n\t#Область Р\n\tПопытка\n\t\tБ = 1;\n\t#КонецОбласти\n\tИсключение\n\t\tВ = 2;\n\tКонецПопытки;\nКонецПроцедуры\n",
        );
    }

    #[test]
    fn region_english_aliases_cross_if() {
        assert_parses(
            "Procedure P() Export\n\t#Region R\n\tIf A Then\n\t\tB = 1;\n\t#EndRegion\n\tEndIf;\nEndProcedure\n",
        );
    }

    #[test]
    fn region_wrapping_procedures_still_parses() {
        assert_parses(
            "#Область ПрограммныйИнтерфейс\nПроцедура Тест1() Экспорт\nКонецПроцедуры\n\nПроцедура Тест2() Экспорт\nКонецПроцедуры\n#КонецОбласти\n",
        );
    }

    #[test]
    fn region_markers_inside_preproc_if() {
        assert_parses(
            "Процедура П() Экспорт\n#Если Сервер Тогда\n\t#Область Р\n\tА = 1;\n\t#КонецОбласти\n#КонецЕсли\nКонецПроцедуры\n",
        );
    }

    #[test]
    fn region_unpaired_start_parses() {
        assert_parses("#Область Р\nПроцедура Тест()\nКонецПроцедуры\n");
    }

    #[test]
    fn region_unpaired_end_parses() {
        assert_parses("Процедура Тест()\nКонецПроцедуры\n#КонецОбласти\n");
    }

    // A region directive may sit between an annotation/compiler directive and
    // the declaration it applies to; the annotation still binds to the
    // declaration and the marker must not derail the parse.

    #[test]
    fn region_between_directive_and_var_parses() {
        assert_parses("&НаКлиенте\n#Область ОписаниеПеременных\n\nПерем П;\n#КонецОбласти\n");
    }

    #[test]
    fn region_between_directive_and_procedure_parses() {
        assert_parses(
            "&НаСервере\n#Область Р\nПроцедура Тест() Экспорт\nКонецПроцедуры\n#КонецОбласти\n",
        );
    }

    #[test]
    fn region_end_between_directive_and_function_parses() {
        assert_parses("&НаКлиенте\n#КонецОбласти\n#Область Р2\nФункция Ф()\nКонецФункции\n");
    }

    // Date literals carry optional separators and a time component; assignments
    // using them must parse cleanly (see lexer `Date` token).

    #[test]
    fn iso_datetime_literal_assignment_parses() {
        assert_parses("Процедура П()\n\tВремяНачала = '0001-01-01 09:00:00';\nКонецПроцедуры\n");
    }

    #[test]
    fn digits_spaced_time_literal_assignment_parses() {
        assert_parses("Процедура П()\n\tВремяНачала = '00010101 22:00';\nКонецПроцедуры\n");
    }

    #[test]
    #[ignore]
    fn test_event_balance_large_file() {
        use crate::event::Event;

        let input = include_str!("../tests/fixtures/Module.bsl");
        let tokens = tokenize(input);

        let mut p = Parser::new(&tokens);
        grammar::source_file(&mut p);
        let events = p.finish();

        let start_count = events.iter().filter(|e| matches!(e, Event::Start { .. })).count();
        let finish_count = events.iter().filter(|e| matches!(e, Event::Finish)).count();

        eprintln!("\n=== Event Balance Analysis ===");
        eprintln!("File size: {} bytes", input.len());
        eprintln!("Total events: {}", events.len());
        eprintln!("Start events: {}", start_count);
        eprintln!("Finish events: {}", finish_count);
        eprintln!("Balance: {} (should be 0)", start_count as i32 - finish_count as i32);

        assert_eq!(
            start_count, finish_count,
            "Events must be balanced! Start: {}, Finish: {}",
            start_count, finish_count
        );
    }

    /// Событий ровно столько, сколько значимых лексем в области разбора.
    ///
    /// Промежутки событий не имеют: их проматывает сток. Считается по обеим
    /// фикстурам и по обоим языкам, потому что счётчик, сверенный с самим
    /// собой на одном входе, ничего не сторожит.
    #[test]
    fn one_token_event_per_significant_lexeme() {
        use crate::event::Event;

        let bsl = include_str!("../tests/fixtures/Module.bsl");
        let sdbl = include_str!("../tests/fixtures/user_query_with_highlighting_issue.sdbl");

        let tokens = tokenize(bsl);
        let mut p = Parser::new(&tokens);
        grammar::source_file(&mut p);
        let bsl_events = p.finish();

        let sdbl_tokens =
            sdbl_token_converter::convert_sdbl_tokens(&lexer::sdbl::tokenize_sdbl(sdbl));
        let mut p = Parser::new(&sdbl_tokens);
        p.set_grammar_boundary(grammar::sdbl::at_query_boundary);
        grammar::sdbl::query_package(&mut p);
        let sdbl_events = p.finish();

        for (name, tokens, events) in
            [("BSL", &tokens, &bsl_events), ("SDBL", &sdbl_tokens, &sdbl_events)]
        {
            let significant = tokens.iter().filter(|t| !t.kind.is_trivia()).count();
            let emitted = events.iter().filter(|e| matches!(e, Event::Token { .. })).count();
            assert_eq!(
                emitted, significant,
                "{name}: событий {emitted}, значимых лексем {significant}"
            );
            assert!(
                tokens.len() > significant,
                "{name}: во входе нет тривии, и счёт совпал бы при любой реализации"
            );
        }
    }

    #[test]
    fn test_parse_sdbl_entry_point() {
        let parse = parse_sdbl("SELECT Name FROM Catalog.Products");

        if parse.has_errors() {
            for error in parse.errors() {
                eprintln!("Parse error: {:?}", error);
            }
        }

        let root = parse.syntax_node();
        assert_eq!(root.kind(), syntax::SyntaxKind::SDBL_QUERY_PACKAGE);
    }
}
