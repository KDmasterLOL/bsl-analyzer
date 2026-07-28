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

#[test]
fn a_header_leaves_the_word_that_ends_it() {
    // A header waits for the word ending it as surely as a block waits for
    // the word closing it, and a condition never written leaves the parser
    // standing on that word. Consuming it produced a second, false report
    // that the word was missing.
    for (input, separator) in [
        ("Процедура П()\nЕсли Тогда\nКонецЕсли;\nКонецПроцедуры", "Тогда"),
        ("Процедура П()\nЕсли А Тогда\nИначеЕсли Тогда\nКонецЕсли;\nКонецПроцедуры", "Тогда"),
        ("Процедура П()\nПока Цикл\nКонецЦикла;\nКонецПроцедуры", "Цикл"),
        ("Процедура П()\nДля Каждого Э Из Цикл\nКонецЦикла;\nКонецПроцедуры", "Цикл"),
        ("Процедура П()\nДля А = По 10 Цикл\nКонецЦикла;\nКонецПроцедуры", "По"),
    ] {
        assert!(covers(input, true), "`{input}`");
        assert!(
            !swallowed(input, true, separator),
            "`{separator}` was consumed: {:?}",
            error_node_texts(input, true)
        );
        assert_eq!(messages(input, true).len(), 1, "`{input}`: {:?}", messages(input, true));
    }
}

#[test]
fn a_bracketed_list_keeps_its_punctuation() {
    // A comma is how a list reaches its next item and a paren is how it ends,
    // so a rule tripping over one inside an item must leave it. A parameter
    // with no default value used to consume the `)` and the list then reported
    // it missing at the closer of the procedure.
    let input = "Процедура П(А = )\nКонецПроцедуры";
    assert!(covers(input, true));
    assert!(!swallowed(input, true, ")"), "{:?}", error_node_texts(input, true));
    assert_eq!(messages(input, true).len(), 1, "{:?}", messages(input, true));
}

#[test]
fn an_annotation_keeps_the_declaration_it_precedes() {
    // The declaration is what the annotation was attached to, and an
    // unclosed parameter list inside the annotation used to consume the whole
    // of it: four messages and no definition in the tree at all.
    let input = "&Перед(1\nПроцедура П()\nКонецПроцедуры";
    let parse = parser::parse(input);

    assert!(covers(input, true));
    assert!(!swallowed(input, true, "Процедура"), "{:?}", error_node_texts(input, true));
    assert!(
        parse.syntax_node().descendants().any(|n| n.kind() == SyntaxKind::PROCEDURE_DEF),
        "the definition survives: {:#?}",
        parse.errors()
    );
    assert_eq!(messages(input, true).len(), 1, "{:?}", messages(input, true));
}

#[test]
fn a_region_header_keeps_its_then() {
    // The same header rule as `Если`, in the conditional region.
    let input = "#Если Тогда\n#КонецЕсли";
    assert!(covers(input, true));
    assert!(!swallowed(input, true, "Тогда"), "{:?}", error_node_texts(input, true));
    assert_eq!(messages(input, true).len(), 1, "{:?}", messages(input, true));
}

#[test]
fn a_header_keeps_its_last_word_through_the_separator_before_it() {
    // A scope has to outlive the word it protects. `Для А = 1 Цикл` has no
    // `По`, and the expect that says so ran outside the scope keeping `Цикл`,
    // so it took the `Цикл` and the header then reported that same word
    // missing.
    for input in [
        "Процедура П()\nДля А = 1 Цикл\nКонецЦикла;\nКонецПроцедуры",
        "Процедура П()\nДля Каждого Э Цикл\nКонецЦикла;\nКонецПроцедуры",
    ] {
        assert!(covers(input, true), "`{input}`");
        assert!(!swallowed(input, true, "Цикл"), "{:?}", error_node_texts(input, true));
        closer_survives(input, "КонецЦикла");
    }
}

#[test]
fn a_region_that_crosses_a_block_keeps_its_own_closer_only_by_luck() {
    // The scope stack holds constructs that nest. A conditional region and a
    // statement block may cross instead, and then the region returns on a word
    // that is not its own, its scope is dropped, and the `#КонецЕсли` that
    // does belong to it is no longer protected from anything.
    //
    // Pinned as it behaves, which is as it behaved before any of this: the
    // mechanism does not make it worse and cannot make it better. Expressing
    // a scope that outlives the rule that opened it is what would fix it.
    let input = "Процедура П()\nЕсли А Тогда\n#Если Клиент Тогда\n\tИначеЕсли Б Тогда\n#КонецЕсли\n\tКонецЕсли;\nКонецПроцедуры";

    assert!(covers(input, true));
    assert!(swallowed(input, true, "#КонецЕсли"), "{:?}", error_node_texts(input, true));
    assert_eq!(messages(input, true).len(), 2, "{:?}", messages(input, true));
}

#[test]
fn a_call_keeps_its_closing_paren() {
    // The same rule as a parameter list, in the constructs that had not been
    // given it: an argument list and a parenthesised expression.
    for input in
        ["Процедура П()\nФ(А = )\nКонецПроцедуры", "Процедура П()\nА = (Б = )\nКонецПроцедуры"]
    {
        assert!(covers(input, true), "`{input}`");
        assert!(!swallowed(input, true, ")"), "{:?}", error_node_texts(input, true));
    }
}

#[test]
fn an_annotation_keeps_the_next_link_of_its_chain() {
    // A chain may hold more than one annotation, and a folding marker may sit
    // between them, so the next link is awaited exactly as the declaration is.
    let input = "&Перед(1\n&НаКлиенте\nПроцедура П()\nКонецПроцедуры";
    let parse = parser::parse(input);

    assert!(covers(input, true));
    assert!(!swallowed(input, true, "&НаКлиенте"), "{:?}", error_node_texts(input, true));
    assert!(
        parse.syntax_node().descendants().any(|n| n.kind() == SyntaxKind::COMPILER_DIRECTIVE),
        "the directive survives: {:#?}",
        parse.errors()
    );
}

#[test]
fn a_separator_stops_being_awaited_once_it_is_taken() {
    // `По` is awaited until it is consumed and not after. Keeping it a
    // boundary past its own position leaves a repeated one standing, and the
    // expect for `Цикл` then takes the real `Цикл` instead of it.
    let input = "Процедура П()\nДля А = 1 По По Цикл\nКонецЦикла;\nКонецПроцедуры";

    assert!(covers(input, true));
    assert!(!swallowed(input, true, "Цикл"), "{:?}", error_node_texts(input, true));
    assert_eq!(messages(input, true).len(), 1, "{:?}", messages(input, true));
}

#[test]
fn a_word_inside_a_group_is_not_the_separator_the_header_awaits() {
    // Pinned as it behaves, and as it behaved before any of this. A boundary
    // holds at every depth inside its scope, so a `Тогда` inside parens counts
    // as the one the header is waiting for, and the real one after the group
    // is then taken as an error. Limiting a boundary to the depth it was
    // declared at would fix these two and break the case the whole mechanism
    // exists for — `КонецПроцедуры` reached with a paren still open.
    for input in
        ["#Если (Тогда) Тогда\n#КонецЕсли", "&Перед(Процедура)\nПроцедура П()\nКонецПроцедуры"]
    {
        assert!(covers(input, true), "`{input}`");
        assert_eq!(messages(input, true).len(), 4, "`{input}`: {:?}", messages(input, true));
    }
}

#[test]
fn every_bracketed_construct_keeps_its_punctuation() {
    // Enumerated from the grammar rather than from what a review happened to
    // find: every construct that owns a bracket or a comma says so. The
    // ternary was the worst of them — a missing first operand consumed the
    // comma, both remaining operands and the closing paren, six messages for
    // one gap.
    for input in [
        "Процедура П(А = )\nКонецПроцедуры",
        "Процедура П()\n\tФ(А = );\nКонецПроцедуры",
        "Процедура П()\n\tА = Б[ ];\nКонецПроцедуры",
        "Процедура П()\n\tА = ?(, 1, 2);\nКонецПроцедуры",
        "Процедура П()\n\tВызватьИсключение(, 1);\nКонецПроцедуры",
        "Процедура П()\n\tДобавитьОбработчик , П;\nКонецПроцедуры",
        "Процедура П()\n\tУдалитьОбработчик , П;\nКонецПроцедуры",
        "&Перед(1\nПроцедура П()\nКонецПроцедуры",
    ] {
        assert!(covers(input, true), "`{input}`");

        let eaten: Vec<String> =
            error_node_texts(input, true).into_iter().filter(|t| !t.is_empty()).collect();
        assert!(eaten.is_empty(), "`{input}` consumed {eaten:?}");
        assert!(messages(input, true).len() <= 1, "`{input}`: {:?}", messages(input, true));
    }
}

#[test]
fn a_construct_claims_only_the_punctuation_it_consumes() {
    // The mirror of the test above, and the price of stating the class with
    // one shared predicate: a construct that also claimed its neighbour's
    // separator left behind a token no rule would take, and then spent its
    // own closer on it — three messages for one stray bracket.
    for input in [
        "Процедура П()\n\tА = Б[,];\nКонецПроцедуры",
        "Процедура П()\n\tА = Б[)];\nКонецПроцедуры",
        "Процедура П()\n\tА = (]);\nКонецПроцедуры",
        "Процедура П()\n\tФ(]);\nКонецПроцедуры",
        "Процедура П()\n\tА = ?(],1,2);\nКонецПроцедуры",
        "Процедура П()\n\tВызватьИсключение(]);\nКонецПроцедуры",
    ] {
        assert!(covers(input, true), "`{input}`");
        assert_eq!(messages(input, true).len(), 1, "`{input}`: {:?}", messages(input, true));
    }
}

#[test]
fn the_opening_paren_never_written_does_not_cost_the_separator() {
    // A ternary owns its commas from the `?` on, not from the paren onwards:
    // the first way to land on one is the paren missing.
    let input = "Процедура П()\n\tА = ?, 1, 2);\nКонецПроцедуры";
    assert!(covers(input, true), "`{input}`");
    let eaten: Vec<String> =
        error_node_texts(input, true).into_iter().filter(|t| !t.is_empty()).collect();
    assert!(eaten.is_empty(), "consumed {eaten:?}");
    assert_eq!(messages(input, true).len(), 2, "{:?}", messages(input, true));
}

// --- SDBL: a clause keyword taken by a recovery inside the clause ---------

fn clause_count(input: &str, kind: SyntaxKind) -> usize {
    parser::parse_sdbl(input).syntax_node().descendants().filter(|n| n.kind() == kind).count()
}

#[test]
fn a_recovery_inside_a_query_leaves_the_clause_that_follows_it() {
    // Each of these has one defect inside one part of the query, and the
    // clause after it used to be consumed by the recovery and produce no
    // node — so a consumer could not see a source clause that is plainly
    // written.
    let cases: &[(&str, SyntaxKind)] = &[
        ("ВЫБРАТЬ ВЫБОР КОГДА А ТОГДА Б ИЗ Т", SyntaxKind::SDBL_FROM_CLAUSE),
        ("ВЫБРАТЬ () ИЗ Т", SyntaxKind::SDBL_FROM_CLAUSE),
        ("ВЫБРАТЬ А В (1 ИЗ Т", SyntaxKind::SDBL_FROM_CLAUSE),
        ("ВЫБРАТЬ * ИЗ Т ГДЕ А В () УПОРЯДОЧИТЬ ПО Б", SyntaxKind::SDBL_ORDER_CLAUSE),
        ("ВЫБРАТЬ А В (1 ИЗ Т ГДЕ Б", SyntaxKind::SDBL_WHERE_CLAUSE),
    ];

    for (input, kind) in cases {
        assert!(covers(input, false), "`{input}`");
        assert_eq!(clause_count(input, *kind), 1, "`{input}` keeps its {kind:?}");
    }
}

#[test]
fn an_operand_never_written_still_costs_the_clause_after_it() {
    // The other half of the same defect, and not fixed here. A rule with an
    // operand it must have reports nothing at all — it reads the next word as
    // that operand — and since every keyword of this language arrives as an
    // identifier, the word it reads can be the clause keyword that follows.
    //
    // Refusing it needs the position, not the word: `ИЗ` opens a clause where
    // a clause may start and names a source where a name may stand. Pinned as
    // it behaves so that the remaining half is visible rather than implied.
    for input in ["ВЫБРАТЬ А + ИЗ Т", "ВЫБРАТЬ А ССЫЛКА ИЗ Т", "ВЫБРАТЬ А ПОДОБНО ИЗ Т"]
    {
        assert!(covers(input, false), "`{input}`");
        assert_eq!(clause_count(input, SyntaxKind::SDBL_FROM_CLAUSE), 0, "`{input}`");
    }
}

#[test]
fn the_clauses_after_the_body_are_inside_the_boundary_too() {
    // `АВТОУПОРЯДОЧИВАНИЕ`, `УПОРЯДОЧИТЬ` and `ИТОГИ` may come in any order
    // and are parsed after the body rather than in it, so a boundary stated
    // only for the body leaves them outside it.
    let cases: &[(&str, SyntaxKind)] = &[
        ("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО () АВТОУПОРЯДОЧИВАНИЕ", SyntaxKind::SDBL_AUTOORDER),
        (
            "ВЫБРАТЬ А ИЗ Т АВТОУПОРЯДОЧИВАНИЕ УПОРЯДОЧИТЬ ПО () ИТОГИ ПО А",
            SyntaxKind::SDBL_TOTALS_BY,
        ),
    ];

    for (input, kind) in cases {
        assert!(covers(input, false), "`{input}`");
        assert_eq!(clause_count(input, *kind), 1, "`{input}` keeps its {kind:?}");
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
