//! What error recovery does to the token an enclosing rule was waiting for.
//!
//! A rule deep inside a construct reports an error by consuming the token it
//! tripped over. When that token is the one closing an enclosing block, the
//! enclosing rule never sees it, runs to the end of the file, and reports the
//! closer as missing — while the closer is right there in the text.
//!
//! A block therefore states the words that close it while it parses what it
//! encloses, and a rule that trips over one of them reports the trip and
//! leaves the word alone. What these tests hold to is that the closer stays
//! in the tree where the text put it, and that the count of messages does not
//! grow with the depth of the nesting.

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

// --- BSL: a block closer is not a recovery's to take ---------------------

/// Nothing consumed the closer, and nothing claimed it was missing.
fn closer_survives(input: &str, closer: &str) {
    assert!(covers(input, true), "`{input}`");
    assert!(
        !swallowed(input, true, closer),
        "`{closer}` was consumed: {:?}",
        error_node_texts(input, true)
    );
    assert!(
        !claims_missing(input, true, closer),
        "`{closer}` was reported missing while present: {:?}",
        messages(input, true)
    );
}

#[test]
fn an_unclosed_call_leaves_a_procedure_its_closer() {
    let input = "Процедура П()\nФ(\nКонецПроцедуры";

    closer_survives(input, "КонецПроцедуры");

    // Two messages about the one gap, both standing at it. One is the
    // argument that was never written, the other the paren that never closed.
    assert_eq!(messages(input, true).len(), 2, "{:?}", messages(input, true));
}

#[test]
fn an_unfinished_assignment_leaves_both_closers() {
    let input = "Процедура П()\nЕсли Истина Тогда\nА =\nКонецЕсли;\nКонецПроцедуры";

    for closer in ["КонецЕсли", "КонецПроцедуры"] {
        closer_survives(input, closer);
    }

    // The missing right-hand side, and nothing else.
    assert_eq!(messages(input, true).len(), 1, "{:?}", messages(input, true));
}

#[test]
fn the_count_of_messages_does_not_grow_with_the_nesting() {
    // Three blocks stand between the typo and the end of the file. Each used
    // to lose its closer and be reported unclosed, so the false reports came
    // one per level; the depth now costs nothing.
    let input =
        "Процедура П()\n\tПопытка\n\t\tА = Ф(\n\tИсключение\n\tКонецПопытки;\nКонецПроцедуры";

    for closer in ["Исключение", "КонецПопытки", "КонецПроцедуры"]
    {
        closer_survives(input, closer);
    }

    assert_eq!(messages(input, true).len(), 2, "{:?}", messages(input, true));
}

#[test]
fn a_loop_closer_stays_the_loop_closer() {
    let input = "Процедура П()\n\tПока Истина Цикл\n\t\tА = Ф(\n\tКонецЦикла;\nКонецПроцедуры";

    for closer in ["КонецЦикла", "КонецПроцедуры"] {
        closer_survives(input, closer);
    }

    assert_eq!(messages(input, true).len(), 2, "{:?}", messages(input, true));
}

#[test]
fn a_statement_list_ends_at_a_closer_further_out() {
    // A boundary is a promise that some rule will not advance, and a list
    // waiting only for its own terminator waits for a token nothing will
    // reach. These inputs each end a block without its own closer, and the
    // parse has to finish rather than spin.
    for input in [
        "Процедура П()\n\tПока Истина Цикл\n\t\tА = 1;\nКонецПроцедуры",
        "Процедура П()\n\tЕсли Истина Тогда\n\t\tА = 1;\nКонецПроцедуры",
        "Процедура П()\n\tПопытка\n\t\tА = 1;\nКонецПроцедуры",
        "Процедура П()\n\tДля Каждого Э Из К Цикл\n\t\tА = 1;\nКонецПроцедуры",
    ] {
        assert!(covers(input, true), "`{input}`");
        closer_survives(input, "КонецПроцедуры");
    }
}

#[test]
fn what_the_boundary_costs_when_the_closer_is_not_stranded() {
    // The same rule cuts the other way. Here `Иначе` is a stray word, not a
    // closer anyone is waiting for in the text — the loop's `КонецЦикла` is
    // right there — but an enclosing `Если` is open, so the list ends and the
    // loop reports a closer that is present. Two messages where one would do,
    // and the first of them false.
    //
    // Telling the two cases apart needs to know whether the block's own
    // closer appears later, which is the whole rest of the input. The trade
    // was made the other way on measurement: this shape costs one message on
    // one module in a production configuration of 21 114, and the shape it
    // replaces costs one false message per level of nesting on every
    // half-written block.
    let input =
        "Процедура П()\n\tЕсли А Тогда\n\t\tДля Каждого Э Из К Цикл\n\t\t\tИначе\n\t\tКонецЦикла;\n\tКонецЕсли;\nКонецПроцедуры";

    assert!(covers(input, true));
    assert_eq!(messages(input, true).len(), 2, "{:?}", messages(input, true));
    assert!(
        messages(input, true).iter().any(|m| m.contains("КонецЦикла")),
        "{:?}",
        messages(input, true)
    );
}

#[test]
fn a_region_crossing_a_block_does_not_hang() {
    // A `#Если` may open inside `Если` and the `ИначеЕсли` closing that `Если`
    // may stand inside the region. The region's content must end there: rules
    // inside it will not consume an enclosing closer, so a region waiting only
    // for `#КонецЕсли` waits for a token nothing reaches.
    let input = "Процедура П()\n\tЕсли А Тогда\n#Если Клиент Тогда\n\tИначеЕсли Б Тогда\n#КонецЕсли\n\tКонецЕсли;\nКонецПроцедуры";

    assert!(covers(input, true));
    closer_survives(input, "КонецПроцедуры");
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
