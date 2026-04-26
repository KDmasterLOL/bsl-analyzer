//! Slice 11 clean-room acceptance suite — clauses after FROM.
//!
//! These tests are the spec-driven acceptance gate for the Slice 11
//! clean-room rewrite of the SDBL post-FROM clause family
//! (WHERE / GROUP BY / HAVING / ORDER BY / AUTOORDER / TOTALS BY /
//! FOR UPDATE / INDEX BY). Each test cites either an ITS pubqlang
//! chapter / line range, a §section of the SELECT mini-spec at
//! `docs/legal/sdbl-select-mini-spec.md`, or a §invariant of the
//! Slice 11 attestation at `docs/legal/sdbl-clean-room-slice11.md`.
//!
//! Authored under the clean-room discipline documented in
//! `docs/legal/sdbl-clean-room-slices.md` — `../bsl-parser/*` was
//! not consulted; ITS chapter regions read directly via the local
//! dump path `/home/itrous/src/tools_migration/its/dump/html/`
//! (chapters 16, 17, 22, 23, 24, 27, 34, 35, 39).

use parser::parse_sdbl;
use syntax::SyntaxKind;

fn assert_clean(input: &str) -> syntax::SyntaxNode {
    let parse = parse_sdbl(input);
    assert!(
        !parse.has_errors(),
        "Expected clean parse for `{}`; got errors: {:#?}",
        input,
        parse.errors(),
    );
    let root = parse.syntax_node();
    let error_descendants: Vec<_> =
        root.descendants().filter(|n| n.kind() == SyntaxKind::ERROR).collect();
    assert!(
        error_descendants.is_empty(),
        "Expected no ERROR recovery nodes for `{}`; got: {:#?}",
        input,
        error_descendants,
    );
    root
}

fn find_kind(root: &syntax::SyntaxNode, kind: SyntaxKind) -> syntax::SyntaxNode {
    root.descendants()
        .find(|n| n.kind() == kind)
        .unwrap_or_else(|| panic!("Tree must contain {:?}; got: {:#?}", kind, root))
}

// ============================================================
// §WHERE — ITS pubqlang/22 §Условие отбора;
// SELECT mini-spec §WHERE.
// ============================================================

/// ITS chapter 22 canonical Russian form: `ВЫБРАТЬ ... ГДЕ
/// <условие>`. Source: `chapter_022.html:15` —
/// `Условие отбора данных из таблицы задается после ключевого
/// слова ГДЕ`.
#[test]
fn test_slice11_where_canonical_ru() {
    let root = assert_clean("ВЫБРАТЬ Наименование ИЗ Справочник.Контрагенты ГДЕ Активен = ИСТИНА");
    find_kind(&root, SyntaxKind::SDBL_WHERE_CLAUSE);
}

/// English-bilingual canonical form. Bilingual WHERE/ГДЕ
/// attested in lexer Slice 2 attestation
/// `docs/legal/sdbl-clean-room-slice2.md` §clause starters;
/// SELECT mini-spec §WHERE.
#[test]
fn test_slice11_where_canonical_en() {
    let root = assert_clean("SELECT Name FROM Catalog.Counterparties WHERE Active = TRUE");
    find_kind(&root, SyntaxKind::SDBL_WHERE_CLAUSE);
}

/// §AST-shape invariant: `SdblWhereClause` direct child is one of
/// 9 expression NodeKinds (Slice 10a wraps in `SdblLogicalOrExpr`).
/// Consumer at `crates/sdbl-hir/src/lower/clauses.rs:28-41` reads
/// the first matching child.
#[test]
fn test_slice11_where_logical_or_expr_direct_child() {
    let root = assert_clean("ВЫБРАТЬ * ИЗ Т ГДЕ A = 1 ИЛИ B = 2");
    let where_clause = find_kind(&root, SyntaxKind::SDBL_WHERE_CLAUSE);
    let direct_kinds: Vec<_> = where_clause.children().map(|c| c.kind()).collect();
    assert!(
        direct_kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR),
        "SdblWhereClause must have SdblLogicalOrExpr as a direct \
         child (Slice 10a wrapping contract). Got: {:?}",
        direct_kinds,
    );
}

/// §IDE-recovery / §Child-attachment invariant #2: KW_OR token
/// reachable from `SdblWhereClause` via recursive walk excluding
/// subqueries. Consumer at `clauses.rs:170-192`
/// `collect_or_tokens_excluding_subqueries`. The outer `WHERE` walk
/// must NOT descend through `SDBL_SUBQUERY` to count an inner
/// nested KW_OR.
#[test]
fn test_slice11_where_kw_or_subquery_isolated() {
    use syntax::NodeOrToken;
    fn count_kw_or_excluding_subqueries(node: &syntax::SyntaxNode) -> usize {
        let mut total = 0usize;
        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::KW_OR => {
                    total += 1;
                }
                NodeOrToken::Node(n)
                    if !matches!(
                        n.kind(),
                        SyntaxKind::SDBL_SUBQUERY
                            | SyntaxKind::SDBL_SUBQUERY_EXPR
                            | SyntaxKind::SDBL_SELECT_QUERY,
                    ) =>
                {
                    total += count_kw_or_excluding_subqueries(&n);
                }
                _ => {}
            }
        }
        total
    }

    let root = assert_clean("ВЫБРАТЬ * ИЗ Т ГДЕ A В (ВЫБРАТЬ X ИЗ С ГДЕ X = 1 ИЛИ X = 2)");
    let where_clauses: Vec<_> =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_WHERE_CLAUSE).collect();
    assert_eq!(where_clauses.len(), 2);

    let mut sorted = where_clauses.clone();
    sorted.sort_by_key(|w| {
        w.ancestors()
            .filter(|a| {
                matches!(
                    a.kind(),
                    SyntaxKind::SDBL_SUBQUERY
                        | SyntaxKind::SDBL_SUBQUERY_EXPR
                        | SyntaxKind::SDBL_SELECT_QUERY,
                )
            })
            .count()
    });
    let outer = &sorted[0];
    let inner = &sorted[1];

    assert_eq!(
        count_kw_or_excluding_subqueries(outer),
        0,
        "Outer SdblWhereClause walk must skip the subquery → zero KW_OR",
    );
    assert_eq!(
        count_kw_or_excluding_subqueries(inner),
        1,
        "Inner subquery's SdblWhereClause walk must find one KW_OR",
    );
}

// ============================================================
// §GROUP BY — ITS pubqlang/34 §Группировка результата запроса;
// chapter 35 multi-field; SELECT mini-spec §GROUP BY.
// ============================================================

/// ITS chapter 34 canonical: `СГРУППИРОВАТЬ ПО <поле>` with
/// агрегатная функция. Source: `chapter_034.html:33, 44`.
#[test]
fn test_slice11_group_by_canonical_ru() {
    let root =
        assert_clean("ВЫБРАТЬ Товар, СУММА(Количество) ИЗ ПродажиТовары СГРУППИРОВАТЬ ПО Товар");
    find_kind(&root, SyntaxKind::SDBL_GROUP_CLAUSE);
}

/// English-bilingual multi-field canonical form. Source for the
/// language form: ITS chapter 35 — `chapter_035.html:29, 41`
/// (`СГРУППИРОВАТЬ ПО <a>, <b>` — "Часто требуется сгруппировать
/// записи исходной таблицы по значению нескольких полей сразу").
/// Bilingual GROUP/СГРУППИРОВАТЬ + BY/ПО (KwOnOrBy bundle)
/// attested in lexer Slice 2 attestation
/// `docs/legal/sdbl-clean-room-slice2.md` §clause starters;
/// SELECT mini-spec §GROUP BY (multiple-direct-children
/// contract).
#[test]
fn test_slice11_group_by_canonical_en() {
    let root = assert_clean(
        "SELECT Customer, Product, SUM(Quantity) FROM Sales GROUP BY Customer, Product",
    );
    find_kind(&root, SyntaxKind::SDBL_GROUP_CLAUSE);
}

/// §IDE-recovery allowance #3: missing-BY recovery for GROUP —
/// bare-keyword shape. Pinned by C0b test (c).
#[test]
fn test_slice11_group_missing_by_recovery() {
    let parse = parse_sdbl("ВЫБРАТЬ A ИЗ Т СГРУППИРОВАТЬ A");
    let root = parse.syntax_node();
    let group = find_kind(&root, SyntaxKind::SDBL_GROUP_CLAUSE);
    assert_eq!(
        group.children().count(),
        0,
        "Bare-keyword shape on missing-BY: zero direct child nodes",
    );
}

// ============================================================
// §HAVING — ITS pubqlang/35 §Условие на агрегаты;
// SELECT mini-spec §HAVING.
// ============================================================

/// ITS chapter 35 canonical: `СГРУППИРОВАТЬ ПО ... ИМЕЮЩИЕ ...`.
/// Source: `chapter_035.html:49` (`с помощью ключевого слова
/// ИМЕЮЩИЕ ... условие отбора аналогично условию в предложении
/// ГДЕ, но только оно накладывается ... на записи, получившиеся в
/// результате группировки`).
#[test]
fn test_slice11_having_canonical_ru() {
    let root = assert_clean(
        "ВЫБРАТЬ Товар, СУММА(Количество) ИЗ ПродажиТовары СГРУППИРОВАТЬ ПО Товар ИМЕЮЩИЕ СУММА(Количество) > 100",
    );
    find_kind(&root, SyntaxKind::SDBL_HAVING_CLAUSE);
}

/// English-bilingual canonical form. Bilingual HAVING/ИМЕЮЩИЕ
/// attested in lexer Slice 2 attestation §clause starters;
/// SELECT mini-spec §HAVING.
#[test]
fn test_slice11_having_canonical_en() {
    let root = assert_clean(
        "SELECT Customer, SUM(Amount) FROM Sales GROUP BY Customer HAVING SUM(Amount) > 1000",
    );
    find_kind(&root, SyntaxKind::SDBL_HAVING_CLAUSE);
}

/// §AST-shape invariant: HAVING calls `expression(p)` (NOT
/// `logical_expression(p)`), but Slice 10a wraps both entry points
/// in `SdblLogicalOrExpr` so the consumer-side filter at
/// `clauses.rs:28-41` matches identically.
#[test]
fn test_slice11_having_logical_expression_wrapping() {
    let root = assert_clean("ВЫБРАТЬ A ИЗ Т СГРУППИРОВАТЬ ПО A ИМЕЮЩИЕ A > 0");
    let having = find_kind(&root, SyntaxKind::SDBL_HAVING_CLAUSE);
    let direct_kinds: Vec<_> = having.children().map(|c| c.kind()).collect();
    assert!(
        direct_kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR),
        "HAVING calls expression(p) but the result wraps in \
         SdblLogicalOrExpr per Slice 10a contract. Got direct: {:?}",
        direct_kinds,
    );
}

// ============================================================
// §ORDER BY — ITS pubqlang/16 §Сортировка результата запроса;
// chapter 17 sort-by-ссылочное-поле; chapter 27
// §Иерархическая упорядоченная выборка (HIERARCHY); SELECT
// mini-spec §ORDER BY.
// ============================================================

/// ITS chapter 16 canonical: `УПОРЯДОЧИТЬ ПО <поле> ВОЗР, <поле>
/// УБЫВ`. Source: `chapter_016.html:48-49, 75-76`.
#[test]
fn test_slice11_order_by_canonical_ru() {
    let root =
        assert_clean("ВЫБРАТЬ Период, Цена ИЗ ЦеныТоваров УПОРЯДОЧИТЬ ПО Период ВОЗР, Цена УБЫВ");
    find_kind(&root, SyntaxKind::SDBL_ORDER_CLAUSE);
}

/// English-bilingual canonical form. Bilingual ORDER/УПОРЯДОЧИТЬ,
/// ASC/ВОЗР, DESC/УБЫВ attested in lexer Slice 2 attestation
/// §clause starters and §LEGACY block (KwAsc, KwDesc); SELECT
/// mini-spec §ORDER BY.
#[test]
fn test_slice11_order_by_canonical_en() {
    let root =
        assert_clean("SELECT Period, Price FROM ProductPrices ORDER BY Period ASC, Price DESC");
    find_kind(&root, SyntaxKind::SDBL_ORDER_CLAUSE);
}

/// §AST-shape invariant #6: `order_by_item` does NOT emit a
/// per-item wrapper. Direct children of `SdblOrderClause` are flat
/// expression nodes interleaved with ASC/DESC IDENT tokens.
#[test]
fn test_slice11_order_by_flat_children() {
    let root = assert_clean("ВЫБРАТЬ A, B ИЗ Т УПОРЯДОЧИТЬ ПО A ВОЗР, B УБЫВ");
    let order = find_kind(&root, SyntaxKind::SDBL_ORDER_CLAUSE);
    let direct_expr_count = order
        .children()
        .filter(|c| {
            matches!(
                c.kind(),
                SyntaxKind::SDBL_LOGICAL_OR_EXPR
                    | SyntaxKind::SDBL_COLUMN_REF
                    | SyntaxKind::SDBL_FUNCTION_CALL,
            )
        })
        .count();
    assert_eq!(direct_expr_count, 2, "Two flat expression children");
}

/// §IDE-recovery allowance #3: missing-BY recovery for ORDER —
/// bare-keyword shape.
#[test]
fn test_slice11_order_missing_by_recovery() {
    let parse = parse_sdbl("ВЫБРАТЬ A ИЗ Т УПОРЯДОЧИТЬ A");
    let root = parse.syntax_node();
    let order = find_kind(&root, SyntaxKind::SDBL_ORDER_CLAUSE);
    assert_eq!(order.children().count(), 0);
}

/// ITS chapter 27 canonical hierarchical-ordering: `УПОРЯДОЧИТЬ
/// ПО <поле> ИЕРАРХИЯ`. Source: `chapter_027.html:39, 51`. **Slice
/// 11 C2 MANDATORY FIX** — `order_by_item` consumes the optional
/// HIERARCHY/ИЕРАРХИЯ modifier as a flat sibling IDENT token of
/// `SdblOrderClause`. Parser-only acceptance — HIR semantic
/// interpretation deferred to Slice 13.
#[test]
fn test_slice11_order_by_hierarchy_canonical_ru() {
    let root = assert_clean(
        "ВЫБРАТЬ Наименование ИЗ Справочник.Товары УПОРЯДОЧИТЬ ПО Наименование ИЕРАРХИЯ",
    );
    let order = find_kind(&root, SyntaxKind::SDBL_ORDER_CLAUSE);

    let has_hierarchy_token = order.children_with_tokens().any(|c| {
        c.as_token().is_some_and(|t| {
            let s = t.text().to_uppercase();
            s == "HIERARCHY" || s == "ИЕРАРХИЯ"
        })
    });
    assert!(
        has_hierarchy_token,
        "ИЕРАРХИЯ must be consumed inside SdblOrderClause as a flat \
         sibling token (per ITS chapter 27 mandatory fix)",
    );
}

// ============================================================
// §AUTOORDER — ITS pubqlang/17 §АВТОУПОРЯДОЧИВАНИЕ;
// SELECT mini-spec §AUTOORDER.
// ============================================================

/// ITS chapter 17 canonical bare-keyword form. Source:
/// `chapter_017.html:17, 32, 52`.
#[test]
fn test_slice11_autoorder_canonical_ru() {
    let root = assert_clean("ВЫБРАТЬ A ИЗ Т АВТОУПОРЯДОЧИВАНИЕ");
    find_kind(&root, SyntaxKind::SDBL_AUTOORDER);
}

/// English-bilingual canonical form. Bilingual
/// AUTOORDER/АВТОУПОРЯДОЧИВАНИЕ attested in lexer Slice 2
/// attestation §LEGACY block (KwAutoOrder); SELECT mini-spec
/// §AUTOORDER.
#[test]
fn test_slice11_autoorder_canonical_en() {
    let root = assert_clean("SELECT A FROM T AUTOORDER");
    find_kind(&root, SyntaxKind::SDBL_AUTOORDER);
}

// ============================================================
// §TOTALS BY — ITS pubqlang/39 §Расчет общих итогов;
// SELECT mini-spec §TOTALS BY (narrowed flat-list).
// ============================================================

/// ITS chapter 39 with explicit aggregate list. Source:
/// `chapter_039.html:13, 25, 29` (`ИТОГИ <агрегат> ПО ОБЩИЕ`
/// / `ИТОГИ ... ПО ОБЩИЕ`).
#[test]
fn test_slice11_totals_canonical_ru() {
    let root =
        assert_clean("ВЫБРАТЬ Товар, Количество ИЗ ПродажиТовары ИТОГИ СУММА(Количество) ПО Товар");
    find_kind(&root, SyntaxKind::SDBL_TOTALS_BY);
}

/// English-bilingual canonical form. Bilingual TOTALS/ИТОГИ
/// attested in lexer Slice 2 attestation §clause starters;
/// SELECT mini-spec §TOTALS BY.
#[test]
fn test_slice11_totals_canonical_en() {
    let root = assert_clean("SELECT Product, Quantity FROM Sales TOTALS SUM(Quantity) BY Product");
    find_kind(&root, SyntaxKind::SDBL_TOTALS_BY);
}

/// §IDE-recovery allowance #1: OVERALL/ОБЩИЕ falls through
/// `is_expression_start` → consumed as bare `SdblColumnRef`. ITS
/// chapter 39 canonical `ИТОГИ ПО ОБЩИЕ` form — line 48-49.
#[test]
fn test_slice11_totals_overall_fallthrough_ru() {
    let root = assert_clean("ВЫБРАТЬ СУММА(A) ИЗ Т ИТОГИ ПО ОБЩИЕ");
    let totals = find_kind(&root, SyntaxKind::SDBL_TOTALS_BY);
    let has_expr_child = totals.children().any(|c| {
        matches!(
            c.kind(),
            SyntaxKind::SDBL_COLUMN_REF
                | SyntaxKind::SDBL_LOGICAL_OR_EXPR
                | SyntaxKind::SDBL_FUNCTION_CALL,
        )
    });
    assert!(
        has_expr_child,
        "OVERALL must fall through is_expression_start → \
         SdblColumnRef direct child",
    );
}

/// §IDE-recovery allowance #3 (TOTALS variant): missing-BY
/// recovery — pre-BY aggregate-expression loop runs FIRST, so
/// `ИТОГИ A` produces SdblTotalsBy with TOTALS+A expression child.
#[test]
fn test_slice11_totals_missing_by_recovery() {
    let parse = parse_sdbl("ВЫБРАТЬ A ИЗ Т ИТОГИ A");
    let root = parse.syntax_node();
    let totals = find_kind(&root, SyntaxKind::SDBL_TOTALS_BY);
    let direct_expr_count = totals
        .children()
        .filter(|c| {
            matches!(
                c.kind(),
                SyntaxKind::SDBL_LOGICAL_OR_EXPR
                    | SyntaxKind::SDBL_COLUMN_REF
                    | SyntaxKind::SDBL_FUNCTION_CALL,
            )
        })
        .count();
    assert!(
        direct_expr_count >= 1,
        "Pre-BY loop must consume `A` BEFORE the missing-BY check; \
         expected at least one expression direct child",
    );
}

// ============================================================
// §FOR UPDATE — Tier D (verified-no in dumped chapters 16–39);
// SELECT mini-spec §FOR UPDATE; bilingual via lexer Slice 2
// LEGACY (KwFor + KwUpdate).
// ============================================================

/// Russian canonical form with MDO chain. Tier D — FOR UPDATE
/// is not in dumped ITS chapters 16–39 (verified-no per §ITS
/// coverage verification table). Bilingual FOR/ДЛЯ +
/// UPDATE/ИЗМЕНЕНИЯ attested in lexer Slice 2 attestation
/// §LEGACY block (KwFor, KwUpdate); SELECT mini-spec §FOR UPDATE.
#[test]
fn test_slice11_for_update_canonical_ru() {
    let root =
        assert_clean("ВЫБРАТЬ A ИЗ Справочник.Контрагенты ДЛЯ ИЗМЕНЕНИЯ Справочник.Контрагенты");
    find_kind(&root, SyntaxKind::SDBL_FOR_UPDATE);
}

/// English-bilingual canonical form. Tier D — same Slice 2
/// LEGACY attestation as the RU test above; SELECT mini-spec
/// §FOR UPDATE.
#[test]
fn test_slice11_for_update_canonical_en() {
    let root = assert_clean("SELECT A FROM Catalog.X FOR UPDATE Catalog.X");
    find_kind(&root, SyntaxKind::SDBL_FOR_UPDATE);
}

/// §Preserved behaviour #4: greedy MDO chain — flat token-level
/// `Dot Ident` pairs at `SdblForUpdate`'s direct-child level.
#[test]
fn test_slice11_for_update_deep_mdo_chain() {
    let root = assert_clean("ВЫБРАТЬ A ИЗ Т ДЛЯ ИЗМЕНЕНИЯ Справочник.X.Y.Z");
    let for_update = find_kind(&root, SyntaxKind::SDBL_FOR_UPDATE);
    let dot_count = for_update
        .children_with_tokens()
        .filter(|c| c.as_token().is_some_and(|t| t.text() == "."))
        .count();
    assert_eq!(dot_count, 3, "Greedy MDO chain flattens `Справочник.X.Y.Z` → 3 Dot tokens",);
}

// ============================================================
// §INDEX BY — Tier D (verified-no in dumped chapters 16–39);
// SELECT mini-spec §INDEX BY; bilingual via lexer Slice 2
// LEGACY (KwIndex).
// ============================================================

/// Russian canonical form with multi-field index. Tier D —
/// INDEX BY is not in dumped ITS chapters 16–39 (verified-no
/// per §ITS coverage verification table). Bilingual
/// INDEX/ИНДЕКСИРОВАТЬ attested in lexer Slice 2 attestation
/// §LEGACY block (KwIndex); BY/ПО via Slice 2 KwOnOrBy bundle;
/// SELECT mini-spec §INDEX BY.
#[test]
fn test_slice11_index_by_canonical_ru() {
    let root = assert_clean("ВЫБРАТЬ Имя, Цена ИЗ Товары ИНДЕКСИРОВАТЬ ПО Имя, Цена");
    find_kind(&root, SyntaxKind::SDBL_INDEX_BY);
}

/// English-bilingual canonical form. Tier D — same Slice 2
/// LEGACY attestation as the RU test above; SELECT mini-spec
/// §INDEX BY.
#[test]
fn test_slice11_index_by_canonical_en() {
    let root = assert_clean("SELECT Name, Price FROM Products INDEX BY Name, Price");
    find_kind(&root, SyntaxKind::SDBL_INDEX_BY);
}

// ============================================================
// Tail-clause dispatcher (`select_tail_clauses`).
// ============================================================

/// §AST-shape invariant #2: any-order acceptance —
/// AUTOORDER appearing AFTER TOTALS in the tail-clause loop.
#[test]
fn test_slice11_tail_any_order_autoorder_after_totals() {
    let root = assert_clean("ВЫБРАТЬ A ИЗ Т ИТОГИ ПО A АВТОУПОРЯДОЧИВАНИЕ");
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_TOTALS_BY));
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_AUTOORDER));
}

/// §AST-shape invariant #1: ORDER BY can appear BOTH at the body-
/// tail position (consumed by `query_body_clauses`) AND at the
/// post-`query` tail-clause loop position (consumed by
/// `select_tail_clauses`). UNION is the natural way to exercise
/// the body-tail position via two queries within one
/// `SdblSelectQuery`.
#[test]
fn test_slice11_body_order_by_vs_tail_order_by() {
    // Single query body — ORDER BY consumed by select_tail_clauses
    // (single-query path).
    let root = assert_clean("ВЫБРАТЬ A ИЗ Т УПОРЯДОЧИТЬ ПО A");
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_ORDER_CLAUSE));

    // UNION body — ORDER BY at the post-UNION tail-clause loop.
    let root2 = assert_clean("ВЫБРАТЬ A ИЗ Т1 ОБЪЕДИНИТЬ ВСЕ ВЫБРАТЬ B ИЗ Т2 УПОРЯДОЧИТЬ ПО A");
    assert_eq!(
        root2.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_ORDER_CLAUSE).count(),
        1,
        "UNION-tail ORDER BY produces exactly one SdblOrderClause",
    );
}

/// §AST-shape invariant #10: `select_tail_clauses` skip_trivia
/// before each keyword check — trailing whitespace must not
/// confuse the lookahead.
#[test]
fn test_slice11_tail_clauses_skip_trivia() {
    let root = assert_clean("ВЫБРАТЬ A ИЗ Т   АВТОУПОРЯДОЧИВАНИЕ\n\nУПОРЯДОЧИТЬ ПО A");
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_AUTOORDER));
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_ORDER_CLAUSE));
}

// ============================================================
// `is_clause_keyword` predicate.
// ============================================================

/// §Child-attachment invariant #10: JOIN-family delegation.
/// Without it, alias scan would swallow ВНУТРЕННЕЕ as Т1's alias.
#[test]
fn test_slice11_is_clause_keyword_join_delegation() {
    let root = assert_clean("ВЫБРАТЬ A ИЗ Т1 ВНУТРЕННЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.X = Т2.Y");
    let join_count =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_JOIN_CLAUSE).count();
    assert_eq!(join_count, 1, "Exactly one JOIN clause must form");

    let first_data_source = find_kind(&root, SyntaxKind::SDBL_DATA_SOURCE);
    let has_alias = first_data_source.children().any(|c| c.kind() == SyntaxKind::SDBL_ALIAS);
    assert!(
        !has_alias,
        "Т1's data source must have NO alias child (ВНУТРЕННЕЕ \
         must terminate alias scan via is_join_keyword delegation)",
    );
}

/// `is_clause_keyword` terminates Slice 7 alias scan at WHERE.
/// Without recognition of WHERE, the alias for `КонтрагентАлиас`
/// would consume ГДЕ.
#[test]
fn test_slice11_is_clause_keyword_alias_termination() {
    let root = assert_clean(
        "ВЫБРАТЬ Контрагент КАК КонтрагентАлиас ИЗ Справочник.Контрагенты ГДЕ Активен = ИСТИНА",
    );
    assert!(
        root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_WHERE_CLAUSE),
        "ГДЕ must be recognised as a clause boundary by is_clause_keyword",
    );
}

/// §Preserved behaviour #4: `is_clause_keyword` guard on the
/// `for_update_clause` MDO chain. The chain must terminate when a
/// following clause keyword (e.g. ORDER BY at the
/// `select_tail_clauses` position) appears.
#[test]
fn test_slice11_is_clause_keyword_for_update_mdo_break() {
    let root = assert_clean("ВЫБРАТЬ A ИЗ Т ДЛЯ ИЗМЕНЕНИЯ Справочник.X УПОРЯДОЧИТЬ ПО A");
    let for_update = find_kind(&root, SyntaxKind::SDBL_FOR_UPDATE);
    let mdo_text = for_update.text().to_string();
    assert!(
        !mdo_text.contains("УПОРЯДОЧИТЬ"),
        "FOR UPDATE MDO chain must terminate at УПОРЯДОЧИТЬ via \
         is_clause_keyword guard. Got chain text: `{}`",
        mdo_text,
    );
    assert!(
        root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_ORDER_CLAUSE),
        "ORDER BY must form as a sibling of FOR UPDATE, not nested in it",
    );
}

// ============================================================
// Cross-slice integration.
// ============================================================

/// Slice 7 (`docs/legal/sdbl-clean-room-slice7.md`)
/// `selected_fields` / `selected_field` produce the SELECT field
/// list including aggregate function calls; Slice 11 §GROUP BY
/// and §HAVING attach as siblings inside `query_body_clauses`.
/// КОЛИЧЕСТВО is the Russian COUNT aggregate per ITS pubqlang/34
/// `chapter_034.html:46` ("в списке полей выборки запроса ...
/// могут присутствовать ... агрегатные функции ...
/// КОЛИЧЕСТВО()").
#[test]
fn test_slice11_x_slice7_select_field_having_predicate() {
    let root =
        assert_clean("ВЫБРАТЬ КОЛИЧЕСТВО(A) ИЗ Т СГРУППИРОВАТЬ ПО B ИМЕЮЩИЕ КОЛИЧЕСТВО(A) > 5");
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_GROUP_CLAUSE));
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_HAVING_CLAUSE));
}

/// Slice 9 (`docs/legal/sdbl-clean-room-slice9.md`) INNER JOIN +
/// Slice 11 §WHERE + §GROUP BY + §HAVING in one query —
/// exercises the full clauses-after-FROM pipeline. ВНУТРЕННЕЕ
/// СОЕДИНЕНИЕ canonical form per ITS chapter 44 (cited in Slice
/// 9 attestation §Sources). The `is_clause_keyword` predicate's
/// JOIN delegation (§Child-attachment invariant #10) ensures the
/// JOIN attaches as a SdblDataSource child while WHERE attaches
/// at the SdblQuery level.
#[test]
fn test_slice11_x_slice9_join_with_where_having() {
    let root = assert_clean(
        "ВЫБРАТЬ A ИЗ Т1 ВНУТРЕННЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.X = Т2.Y \
         ГДЕ Т1.X > 0 СГРУППИРОВАТЬ ПО Т1.X ИМЕЮЩИЕ Т1.X = 5",
    );
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_JOIN_CLAUSE));
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_WHERE_CLAUSE));
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_GROUP_CLAUSE));
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_HAVING_CLAUSE));
}

/// Slice 10b (`docs/legal/sdbl-clean-room-slice10b.md`) BETWEEN
/// predicate (`SdblBetweenExpr`) inside Slice 11 §WHERE clause.
/// МЕЖДУ canonical form per Slice 10b §predicate-list (BETWEEN
/// section); WHERE primary per ITS pubqlang/22 (cited in Slice
/// 11 attestation §Sources). Verifies that Slice 11 preserves
/// the §IDE-recovery allowance #2 routing — `where_clause`
/// delegates to `logical_expression(p)` which dispatches through
/// Slice 10b `predicate_expr` for BETWEEN.
#[test]
fn test_slice11_x_slice10b_predicate_in_where() {
    let root = assert_clean("ВЫБРАТЬ A ИЗ Т ГДЕ A МЕЖДУ 1 И 5");
    let where_clause = find_kind(&root, SyntaxKind::SDBL_WHERE_CLAUSE);
    assert!(
        where_clause.descendants().any(|n| n.kind() == SyntaxKind::SDBL_BETWEEN_EXPR),
        "BETWEEN predicate must lower as SdblBetweenExpr inside the \
         WHERE clause",
    );
}
