//! What error recovery does to the token an enclosing rule was waiting for.
//!
//! A rule deep inside a construct reports an error by consuming the token it
//! tripped over. When that token is the one closing an enclosing block, the
//! enclosing rule never sees it, runs to the end of the file, and reports the
//! closer as missing — while the closer is right there in the text.
//!
//! Every case below is pinned as the parser behaves today. Where today's
//! behaviour is wrong, the assertion says which part of it is wrong; a
//! reader should not take a passing test here as a statement that the
//! behaviour is right.

use syntax::SyntaxKind;

/// The texts of every `ERROR` node, trimmed.
fn error_node_texts(input: &str, bsl: bool) -> Vec<String> {
    let parse = if bsl { parser::parse(input) } else { parser::parse_sdbl(input) };
    parse
        .syntax_node()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::ERROR)
        .map(|n| n.text().to_string().trim().to_string())
        .collect()
}

fn messages(input: &str, bsl: bool) -> Vec<String> {
    let parse = if bsl { parser::parse(input) } else { parser::parse_sdbl(input) };
    parse.errors().iter().map(|e| e.message().to_string()).collect()
}

fn covers(input: &str, bsl: bool) -> bool {
    let parse = if bsl { parser::parse(input) } else { parser::parse_sdbl(input) };
    usize::from(parse.syntax_node().text_range().len()) == input.len()
}

fn swallowed(input: &str, bsl: bool, closer: &str) -> bool {
    error_node_texts(input, bsl).iter().any(|t| t.contains(closer))
}

fn claims_missing(input: &str, bsl: bool, closer: &str) -> bool {
    messages(input, bsl).iter().any(|m| m.contains(closer) && m.contains("конец файла"))
}

// --- BSL: a block closer taken by a recovery inside the block -------------

#[test]
fn an_unclosed_call_costs_a_procedure_its_closer() {
    let input = "Процедура П()\nФ(\nКонецПроцедуры";

    assert!(covers(input, true));

    // Wrong on both counts: `КонецПроцедуры` is not an unexpected token, it
    // is the boundary the argument list had to end before; and it is present
    // in the text, so reporting it missing is a second error about the first.
    assert!(swallowed(input, true, "КонецПроцедуры"), "{:?}", error_node_texts(input, true));
    assert!(claims_missing(input, true, "КонецПроцедуры"), "{:?}", messages(input, true));

    // One typo, three messages, two of them false.
    assert_eq!(messages(input, true).len(), 3, "{:?}", messages(input, true));
}

#[test]
fn an_unfinished_assignment_costs_two_closers() {
    let input = "Процедура П()\nЕсли Истина Тогда\nА =\nКонецЕсли;\nКонецПроцедуры";

    assert!(covers(input, true));

    // The missing right-hand side is one defect. Both closers are in the
    // text, and both are consumed by recovery, so both are then reported
    // missing at end of file.
    for closer in ["КонецЕсли", "КонецПроцедуры"] {
        assert!(swallowed(input, true, closer), "{closer}: {:?}", error_node_texts(input, true));
        assert!(claims_missing(input, true, closer), "{closer}: {:?}", messages(input, true));
    }

    assert_eq!(messages(input, true).len(), 4, "{:?}", messages(input, true));
}

#[test]
fn the_cascade_grows_with_the_depth_of_the_nesting() {
    // Every block between the typo and the end of the file loses its closer,
    // so the count of false reports is the nesting depth. Three here.
    let input =
        "Процедура П()\n\tПопытка\n\t\tА = Ф(\n\tИсключение\n\tКонецПопытки;\nКонецПроцедуры";

    assert!(covers(input, true));

    for closer in ["Исключение", "КонецПопытки", "КонецПроцедуры"]
    {
        assert!(swallowed(input, true, closer), "{closer}: {:?}", error_node_texts(input, true));
        assert!(claims_missing(input, true, closer), "{closer}: {:?}", messages(input, true));
    }

    assert_eq!(messages(input, true).len(), 6, "{:?}", messages(input, true));
}

#[test]
fn a_loop_closer_goes_the_same_way() {
    let input = "Процедура П()\n\tПока Истина Цикл\n\t\tА = Ф(\n\tКонецЦикла;\nКонецПроцедуры";

    assert!(covers(input, true));

    for closer in ["КонецЦикла", "КонецПроцедуры"] {
        assert!(swallowed(input, true, closer), "{closer}: {:?}", error_node_texts(input, true));
        assert!(claims_missing(input, true, closer), "{closer}: {:?}", messages(input, true));
    }
}

// --- SDBL: a clause keyword taken by a recovery inside the clause ---------

fn clause_count(input: &str, kind: SyntaxKind) -> usize {
    parser::parse_sdbl(input).syntax_node().descendants().filter(|n| n.kind() == kind).count()
}

#[test]
fn a_recovery_inside_a_query_takes_the_clause_that_follows_it() {
    // Each of these has one defect inside one part of the query, and in each
    // the clause after it — plainly present in the text — is consumed by the
    // recovery and produces no node.
    let cases: &[(&str, SyntaxKind)] = &[
        ("ВЫБРАТЬ ВЫБОР КОГДА А ТОГДА Б ИЗ Т", SyntaxKind::SDBL_FROM_CLAUSE),
        ("ВЫБРАТЬ () ИЗ Т", SyntaxKind::SDBL_FROM_CLAUSE),
        ("ВЫБРАТЬ А В (1 ИЗ Т", SyntaxKind::SDBL_FROM_CLAUSE),
        ("ВЫБРАТЬ * ИЗ Т ГДЕ А В () УПОРЯДОЧИТЬ ПО Б", SyntaxKind::SDBL_ORDER_CLAUSE),
    ];

    for (input, kind) in cases {
        assert!(covers(input, false), "`{input}`");
        assert_eq!(clause_count(input, *kind), 0, "`{input}` loses its {kind:?}");
    }
}

// --- What a boundary must never do ---------------------------------------

#[test]
fn a_keyword_standing_where_a_name_belongs_is_a_name() {
    // These are legal queries. Whatever protects a clause keyword from being
    // consumed must not reach the position after an explicit `КАК`, where a
    // keyword is an ordinary alias.
    for input in [
        "ВЫБРАТЬ А КАК Итоги ИЗ Т",
        "ВЫБРАТЬ А КАК УНИЧТОЖИТЬ ИЗ Т",
        "ВЫБРАТЬ А ИЗ Т КАК Итоги",
        "ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО Н КАК УНИЧТОЖИТЬ",
    ] {
        let parse = parser::parse_sdbl(input);
        assert!(!parse.has_errors(), "`{input}`: {:#?}", parse.errors());
    }
}
