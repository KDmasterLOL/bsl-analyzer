//! What the SDBL parser does with input the query language does not
//! describe, and with the parts of it that the language describes but
//! the parser used to ignore.
//!
//! Two properties are pinned here. The first is that the syntax tree
//! covers the whole input: nothing the parser refuses may leave without
//! a node and a word. The second is that each tolerance is deliberate —
//! either the language requires the parser to accept it, or it is kept
//! for editor behaviour and named as such.
//!
//! Provenance: `docs/legal/sdbl-clean-room-slice12.md`.

use parser::parse_sdbl;
use syntax::SyntaxKind;

/// The whole input reached the tree and nothing was reported.
fn accepted(input: &str) -> syntax::SyntaxNode {
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    assert_eq!(
        usize::from(root.text_range().len()),
        input.len(),
        "the tree must cover `{input}` completely",
    );
    assert!(!parse.has_errors(), "`{input}` must parse cleanly: {:#?}", parse.errors());
    root
}

/// The whole input reached the tree, and the part the grammar could not
/// place was named in an error.
fn leftover_reported(input: &str, expected: &str) {
    let parse = parse_sdbl(input);
    assert_eq!(
        usize::from(parse.syntax_node().text_range().len()),
        input.len(),
        "the tree must cover `{input}` completely even when it cannot parse it",
    );

    let reported: Vec<_> = parse
        .errors()
        .iter()
        .map(|e| {
            let r = e.range();
            input[usize::from(r.start())..usize::from(r.end())].trim().to_string()
        })
        .collect();
    assert!(
        reported.iter().any(|t| t == expected),
        "expected `{expected}` to be reported for `{input}`; got {reported:?}",
    );
}

fn has_kind(root: &syntax::SyntaxNode, kind: SyntaxKind) -> bool {
    root.descendants().any(|n| n.kind() == kind)
}

// --- The forms the Developer's Reference works through ----------------

#[test]
fn totals_by_hierarchy_worked_example() {
    let root = accepted(
        "ВЫБРАТЬ\n\
         \tДокумент.Номенклатура КАК Номенклатура,\n\
         \tДокумент.Количество КАК Количество\n\
         ИЗ\n\
         \tДокумент.РасходнаяНакладная.Состав КАК Документ\n\
         УПОРЯДОЧИТЬ ПО\n\
         \tДокумент.Номенклатура\n\
         ИТОГИ\n\
         \tСУММА(Количество)\n\
         ПО\n\
         \tНоменклатура ИЕРАРХИЯ",
    );
    assert!(has_kind(&root, SyntaxKind::SDBL_TOTALS_BY));
    assert!(has_kind(&root, SyntaxKind::SDBL_ORDER_CLAUSE));
}

#[test]
fn totals_by_only_hierarchy_worked_example() {
    let root = accepted(
        "ВЫБРАТЬ\n\
         \tДокумент.Номенклатура КАК Номенклатура\n\
         ИЗ\n\
         \tДокумент.РасходнаяНакладная.Состав КАК Документ\n\
         ИТОГИ\n\
         \tСУММА(Количество)\n\
         ПО\n\
         \tНоменклатура ТОЛЬКО ИЕРАРХИЯ",
    );
    assert!(has_kind(&root, SyntaxKind::SDBL_TOTALS_BY));
}

#[test]
fn totals_by_periods_worked_example() {
    let root = accepted(
        "ВЫБРАТЬ\n\
         \tПриходнаяНакладная.Контрагент,\n\
         \tНАЧАЛОПЕРИОДА(ПриходнаяНакладная.Дата, ЧАС) КАК Период,\n\
         \tКОЛИЧЕСТВО(ПриходнаяНакладная.Ссылка) КАК КоличествоПокупок\n\
         ИЗ\n\
         \tДокумент.ПриходнаяНакладная КАК ПриходнаяНакладная\n\
         СГРУППИРОВАТЬ ПО\n\
         \tПриходнаяНакладная.Контрагент,\n\
         \tНАЧАЛОПЕРИОДА(ПриходнаяНакладная.Дата, ЧАС)\n\
         ИТОГИ\n\
         \tСУММА(КоличествоПокупок)\n\
         ПО\n\
         \tПериод ПЕРИОДАМИ(МИНУТА, ДАТАВРЕМЯ(2006,6,28), ДАТАВРЕМЯ(2006,6,28))",
    );
    assert!(has_kind(&root, SyntaxKind::SDBL_TOTALS_BY));
}

#[test]
fn totals_by_overall_worked_example() {
    accepted(
        "ВЫБРАТЬ\n\
         \tДокумент.Номенклатура,\n\
         \tДокумент.Количество КАК Количество\n\
         ИЗ\n\
         \tДокумент.РасходнаяНакладная.Состав КАК Документ\n\
         ИТОГИ\n\
         \tСУММА(Количество)\n\
         ПО\n\
         \tОБЩИЕ",
    );
}

#[test]
fn totals_by_without_an_aggregate_list() {
    // The list may be omitted, in which case it is derived from the
    // selection's aggregate fields.
    accepted(
        "ВЫБРАТЬ\n\
         \tДокумент.Номенклатура КАК Номенклатура,\n\
         \tСУММА(Документ.Количество) КАК Количество\n\
         ИЗ\n\
         \tДокумент.РасходнаяНакладная.Состав КАК Документ\n\
         СГРУППИРОВАТЬ ПО\n\
         \tДокумент.Номенклатура\n\
         ИТОГИ ПО\n\
         \tОБЩИЕ,\n\
         \tНоменклатура",
    );
}

#[test]
fn order_by_descending_worked_example() {
    accepted(
        "ВЫБРАТЬ ПЕРВЫЕ 5\n\
         \tНоменклатура.Наименование,\n\
         \tНоменклатура.ЗакупочнаяЦена\n\
         ИЗ\n\
         \tСправочник.Номенклатура КАК Номенклатура\n\
         УПОРЯДОЧИТЬ ПО\n\
         \tНоменклатура.ЗакупочнаяЦена УБЫВ",
    );
}

// --- The closed lists the rules give ---------------------------------

#[test]
fn every_period_name_is_taken() {
    for period in [
        "СЕКУНДА",
        "МИНУТА",
        "ЧАС",
        "ДЕНЬ",
        "НЕДЕЛЯ",
        "МЕСЯЦ",
        "КВАРТАЛ",
        "ГОД",
        "ДЕКАДА",
        "ПОЛУГОДИЕ",
    ] {
        accepted(&format!("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ({period})"));
    }
}

#[test]
fn periods_takes_none_one_or_two_boundaries() {
    accepted("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ)");
    accepted("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ, &Начало)");
    accepted("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ, &Начало, &Конец)");
    accepted(
        "ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ, ДАТАВРЕМЯ(2006,1,1), ДАТАВРЕМЯ(2006,12,31))",
    );
}

#[test]
fn an_ordering_field_takes_its_four_orderings() {
    for order in ["ВОЗР", "УБЫВ", "ИЕРАРХИЯ", "ИЕРАРХИЯ УБЫВ"] {
        accepted(&format!("ВЫБРАТЬ А ИЗ Т УПОРЯДОЧИТЬ ПО Н {order}"));
    }
}

#[test]
fn a_control_point_takes_its_alias() {
    accepted("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО Н КАК Группа");
    accepted("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО Н Группа");
    accepted("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО Н ИЕРАРХИЯ КАК Группа");
    accepted("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ) КАК Группа");
}

#[test]
fn modifiers_survive_across_a_list_of_control_points() {
    accepted("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО Н ИЕРАРХИЯ, П ПЕРИОДАМИ(ДЕНЬ), К КАК Группа");
}

// --- Tolerances kept on purpose ---------------------------------------

#[test]
fn a_word_order_the_rules_do_not_list_is_still_taken() {
    // Not among the four orderings; a plausible slip, and rejecting it
    // would buy nothing.
    accepted("ВЫБРАТЬ А ИЗ Т УПОРЯДОЧИТЬ ПО Н УБЫВ ИЕРАРХИЯ");
}

#[test]
fn a_clause_keyword_without_its_body_stays_quiet() {
    // Mid-typing. The mandatory BY has not been reached yet, and saying
    // so on every keystroke would be noise.
    for input in [
        "ВЫБРАТЬ А ИЗ Т СГРУППИРОВАТЬ",
        "ВЫБРАТЬ А ИЗ Т УПОРЯДОЧИТЬ",
        "ВЫБРАТЬ А ИЗ Т ИНДЕКСИРОВАТЬ",
        "ВЫБРАТЬ А ИЗ Т ИТОГИ",
        "ВЫБРАТЬ А ИЗ Т ДЛЯ",
        "ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ",
    ] {
        accepted(input);
    }
}

#[test]
fn an_unbalanced_extension_brace_stays_quiet() {
    // A query assembled at runtime can carry half a brace region.
    accepted("ВЫБРАТЬ Т.Поле ИЗ Справочник.Т КАК Т {ГДЕ Т.Поле");
}

#[test]
fn comments_are_trivia() {
    accepted("// только комментарий");
    accepted("ВЫБРАТЬ А // поле\nИЗ Т");
    accepted("ВЫБРАТЬ А ИЗ Т // хвост");
}

#[test]
fn a_bad_field_does_not_cost_the_rest_of_the_list() {
    let parse = parse_sdbl("ВЫБРАТЬ А, Б В Г Д, Е ИЗ Т");
    let root = parse.syntax_node();
    assert_eq!(usize::from(root.text_range().len()), "ВЫБРАТЬ А, Б В Г Д, Е ИЗ Т".len());
    assert!(has_kind(&root, SyntaxKind::SDBL_FROM_CLAUSE), "the FROM clause is still found");
}

// --- Nothing leaves without a word ------------------------------------

#[test]
fn a_misplaced_clause_is_named() {
    leftover_reported("ВЫБРАТЬ А ГДЕ А=1 ИЗ Т", "ИЗ Т");
    leftover_reported("ВЫБРАТЬ А ИЗ Т ПОМЕСТИТЬ ВТ", "ПОМЕСТИТЬ ВТ");
    leftover_reported("ВЫБРАТЬ А ИЗ Т СГРУППИРОВАТЬ ПО Н ГДЕ А=1", "ГДЕ А=1");
}

#[test]
fn a_following_query_is_never_swallowed() {
    // The leftover ends at the separator, so a bad query costs its own
    // parse and nothing else in the package.
    let input = "ВЫБРАТЬ А ИЗ Т ГДЕ А = 1 ФУНК(Х); ВЫБРАТЬ 2 ИЗ У; ВЫБРАТЬ 3 ИЗ В";
    leftover_reported(input, "ФУНК(Х)");
    let root = parse_sdbl(input).syntax_node();
    assert_eq!(
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_QUERY).count(),
        3,
        "one bad member of a package must not cost the others their parse",
    );
}

#[test]
fn the_mandatory_parts_of_a_periods_modifier_are_reported() {
    // Every part the rule marks mandatory, and the alternative it is not
    // allowed to combine with. All are reported; none costs the text.
    for (input, what) in [
        ("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ КАК Г", "no opening paren"),
        ("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ()", "no period name"),
        ("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(СМЕНА)", "not one of the ten"),
        ("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ,)", "boundary missing"),
        ("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО Н ИЕРАРХИЯ ПЕРИОДАМИ(ДЕНЬ)", "exclusive with hierarchy"),
        ("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО Н ТОЛЬКО", "ONLY without HIERARCHY"),
    ] {
        let parse = parse_sdbl(input);
        assert!(parse.has_errors(), "{what}: `{input}` must be reported");
        assert_eq!(
            usize::from(parse.syntax_node().text_range().len()),
            input.len(),
            "{what}: reporting must not cost the text",
        );
    }
}

#[test]
fn an_explicit_alias_commits_to_a_name() {
    // `КАК` written out means a name follows; its absence is reported.
    let parse = parse_sdbl("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО Н КАК, П");
    assert!(parse.has_errors());

    // But a name that happens to spell a keyword is still a name, as it is
    // everywhere else an explicit alias is parsed.
    accepted("SELECT A FROM T TOTALS SUM(A) BY N AS Inner");
    accepted("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО Н КАК Итоги");
}

#[test]
fn a_stray_comma_costs_only_its_own_field() {
    // A separator with nothing before it is a list with something wrong in
    // it, not an absent list; the clauses after it still parse.
    let input = "ВЫБРАТЬ , А ИЗ Т";
    let parse = parse_sdbl(input);
    assert!(parse.has_errors());
    assert_eq!(usize::from(parse.syntax_node().text_range().len()), input.len());
    assert_eq!(
        parse
            .syntax_node()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE)
            .count(),
        1,
    );
}

#[test]
fn the_leftover_is_a_node_not_just_a_message() {
    let parse = parse_sdbl("ВЫБРАТЬ А ГДЕ А=1 ИЗ Т");
    assert!(
        has_kind(&parse.syntax_node(), SyntaxKind::ERROR),
        "the unparsed remainder must be reachable in the tree, not only in the error list",
    );
}

#[test]
fn coverage_holds_across_the_shapes_the_parser_owns() {
    for input in [
        "ВЫБРАТЬ * ИЗ Справочник.Товары",
        "ВЫБРАТЬ РАЗЛИЧНЫЕ ПЕРВЫЕ 10 Т.Код КАК К ИЗ Справочник.Товары КАК Т",
        "ВЫБРАТЬ А ПОМЕСТИТЬ Врем ИЗ Т; УНИЧТОЖИТЬ Врем",
        "ВЫБРАТЬ А ИЗ Т ЛЕВОЕ СОЕДИНЕНИЕ У ПО Т.А = У.А",
        "ВЫБРАТЬ А ИЗ Т ОБЪЕДИНИТЬ ВСЕ ВЫБРАТЬ Б ИЗ У",
        "ВЫБРАТЬ Т.Номенклатура ИЗ РегистрНакопления.Т.Остатки(&Период, ) КАК Т",
        "ВЫБРАТЬ Т.Ссылка ИЗ Справочник.Товары КАК Т {ГДЕ Т.Поле}",
        "ВЫБРАТЬ А ИЗ Т ГДЕ А В (ВЫБРАТЬ Б ИЗ У)",
        "ВЫБРАТЬ А ИЗ Т АВТОУПОРЯДОЧИВАНИЕ",
        "ВЫБРАТЬ А ИЗ Т ДЛЯ ИЗМЕНЕНИЯ Т",
        "ВЫБРАТЬ А ПОМЕСТИТЬ Врем ИЗ Т ИНДЕКСИРОВАТЬ ПО А",
    ] {
        accepted(input);
    }
}

#[test]
fn a_bad_member_keeps_the_next_query_even_when_it_ate_the_separator() {
    // Several recovery paths report by bumping a token, and the token can be
    // the separator. The package loop must not depend on the separator
    // surviving, or "one bad member costs only itself" holds by luck.
    // `SDBL_QUERY` counts every `ВЫБРАТЬ`, nested ones included; the DROP
    // case is counted by its own node because it has no `ВЫБРАТЬ` of its own.
    for (input, kind, wanted) in [
        ("УНИЧТОЖИТЬ ; ВЫБРАТЬ A ИЗ T", SyntaxKind::SDBL_QUERY, 1),
        ("УНИЧТОЖИТЬ ; ВЫБРАТЬ A ИЗ T", SyntaxKind::SDBL_DROP_QUERY, 1),
        ("ВЫБРАТЬ ПЕРВЫЕ ; ВЫБРАТЬ Б ИЗ У", SyntaxKind::SDBL_QUERY, 2),
        ("ВЫБРАТЬ А ИЗ ; ВЫБРАТЬ Б ИЗ У", SyntaxKind::SDBL_QUERY, 2),
        ("ВЫБРАТЬ (ВЫБРАТЬ ); ВЫБРАТЬ А ИЗ Т", SyntaxKind::SDBL_QUERY, 3),
    ] {
        let parse = parse_sdbl(input);
        assert_eq!(
            usize::from(parse.syntax_node().text_range().len()),
            input.len(),
            "`{input}` must be covered completely",
        );
        let found = parse.syntax_node().descendants().filter(|n| n.kind() == kind).count();
        assert_eq!(found, wanted, "`{input}`: {kind:?} count");
    }
}

#[test]
fn a_period_boundary_is_a_date_or_a_parameter() {
    accepted("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ, &Н, &К)");
    accepted(
        "ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(МИНУТА, ДАТАВРЕМЯ(2006,6,28), ДАТАВРЕМЯ(2006,6,28))",
    );
    for bad in [
        "ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ, 42)",
        "ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ, \"текст\")",
        "ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ, Поле.Реквизит)",
    ] {
        let parse = parse_sdbl(bad);
        assert!(parse.has_errors(), "`{bad}`: the rule allows only a date or a parameter");
        assert_eq!(usize::from(parse.syntax_node().text_range().len()), bad.len());
    }
}

#[test]
fn only_the_descending_direction_pairs_with_hierarchy() {
    accepted("ВЫБРАТЬ А ИЗ Т УПОРЯДОЧИТЬ ПО Н УБЫВ ИЕРАРХИЯ");
    accepted("ВЫБРАТЬ А ИЗ Т УПОРЯДОЧИТЬ ПО Н ИЕРАРХИЯ УБЫВ");
    for bad in [
        "ВЫБРАТЬ А ИЗ Т УПОРЯДОЧИТЬ ПО Н ВОЗР ИЕРАРХИЯ",
        "ВЫБРАТЬ А ИЗ Т УПОРЯДОЧИТЬ ПО Н ИЕРАРХИЯ ВОЗР",
    ] {
        assert!(parse_sdbl(bad).has_errors(), "`{bad}` is not one of the four orderings");
        assert_eq!(uncovered_len(bad), 0);
    }
}

#[test]
fn a_missing_hierarchy_does_not_also_report_a_conflict_with_it() {
    // `ТОЛЬКО ПЕРИОДАМИ(…)` is one mistake, not two: the alternative was
    // never taken, so nothing can conflict with it.
    let parse = parse_sdbl("ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО Н ТОЛЬКО ПЕРИОДАМИ(ДЕНЬ)");
    assert_eq!(parse.errors().len(), 1, "got: {:#?}", parse.errors());
}

fn uncovered_len(input: &str) -> usize {
    input.len() - usize::from(parse_sdbl(input).syntax_node().text_range().len())
}

#[test]
fn a_package_says_when_its_members_are_not_separated() {
    // Recognising a query where a separator should be is what keeps the rest
    // of the package parseable; saying nothing about it would turn one bad
    // package into two good queries.
    let run_on = "ВЫБРАТЬ А ИЗ Т ВЫБРАТЬ Б ИЗ У";
    let parse = parse_sdbl(run_on);
    assert!(parse.has_errors(), "a missing separator must be reported");
    assert_eq!(
        parse.syntax_node().descendants().filter(|n| n.kind() == SyntaxKind::SDBL_QUERY).count(),
        2,
        "and both queries must still be parsed",
    );

    // A member missing between two separators is reported too.
    assert!(parse_sdbl("ВЫБРАТЬ А ИЗ Т;;ВЫБРАТЬ Б ИЗ У").has_errors());

    // A separator at the end is not a missing member.
    accepted("ВЫБРАТЬ А ИЗ Т;");
    accepted("ВЫБРАТЬ А ИЗ Т ОБЪЕДИНИТЬ ВЫБРАТЬ Б ИЗ У");
}

#[test]
fn a_closing_paren_ends_a_missing_top_count() {
    // Inside a subquery the paren is structure, not a count. Consuming it
    // would cost the subquery its closing paren and the outer query its
    // shape.
    let input = "ВЫБРАТЬ (ВЫБРАТЬ ПЕРВЫЕ)";
    let parse = parse_sdbl(input);
    assert_eq!(usize::from(parse.syntax_node().text_range().len()), input.len());
    assert!(
        !parse.errors().iter().any(|e| e.message().contains("Ожидалось ')'")),
        "the paren is still there, so nothing may ask for another: {:#?}",
        parse.errors(),
    );
}

#[test]
fn a_separator_survives_the_recovery_that_reports_next_to_it() {
    // Reporting is implemented as taking the offending token, and for a
    // while the offending token could be the separator. Then the package
    // loop either blamed the next query for a separator that was there, or
    // let a recovery keep parsing straight through it.
    let after_drop = parse_sdbl("УНИЧТОЖИТЬ ; ВЫБРАТЬ A ИЗ T");
    assert!(
        !after_drop.errors().iter().any(|e| e.message().contains("разделитель")),
        "the separator is present, so nothing may report it missing: {:#?}",
        after_drop.errors(),
    );
    assert!(parse_sdbl("УНИЧТОЖИТЬ ;; ВЫБРАТЬ A ИЗ T")
        .errors()
        .iter()
        .any(|e| e.message().contains("между разделителями")));

    let after_join = parse_sdbl("ВЫБРАТЬ A ИЗ T СОЕДИНЕНИЕ U ; ВЫБРАТЬ B ИЗ V");
    assert_eq!(
        after_join
            .syntax_node()
            .children()
            .filter(|n| n.kind() == SyntaxKind::SDBL_SELECT_QUERY)
            .count(),
        2,
        "a recovery that reports mid-clause must not swallow the next member",
    );
}

#[test]
fn the_drain_does_not_promote_a_nested_query() {
    // A query start inside a paren group belongs to that group. Promoting it
    // would hand the lowerer a subquery dressed as a package member.
    let input = "ВЫБРАТЬ А ИЗ Т 42 (ВЫБРАТЬ Б ИЗ У)";
    let parse = parse_sdbl(input);
    assert_eq!(usize::from(parse.syntax_node().text_range().len()), input.len());
    assert_eq!(
        parse
            .syntax_node()
            .children()
            .filter(|n| n.kind() == SyntaxKind::SDBL_SELECT_QUERY)
            .count(),
        1,
    );
}
