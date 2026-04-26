//! Clean-room acceptance tests for the Slice 2 structural keyword
//! vocabulary of `SdblTokenKind`.
//!
//! Sources:
//! - ITS query-language structure —
//!   <https://its.1c.ru/db/pubqlang/content/10/hdoc>
//!   (SELECT / FROM / WHERE / GROUP / ORDER / HAVING / TOTALS /
//!   UNION / ALL / DISTINCT / TOP / INTO clause starters, join
//!   clause, field aliasing, predicates, CASE expression).
//! - ITS lexical elements —
//!   <https://its.1c.ru/db/pubqlang/content/12/hdoc>
//!   (logical operators, boolean literals, NULL literal, identifier
//!   longest-match rule).
//!
//! Per-test docstring shorthand: `ITS pubqlang/N` refers to the
//! documentation sub-tree rooted at
//! `https://its.1c.ru/db/pubqlang/content/N/hdoc` for the given `N`.
//!
//! These tests were authored against the specifications above, not
//! against the existing `tokenize_sdbl` output. The one documented
//! pre-refactor behaviour preserved here is that `ON`, `BY`, and `ПО`
//! all lex to a single `KwOnOrBy` kind — see
//! `docs/legal/sdbl-clean-room-slice2.md` § Preserved pre-refactor
//! behaviours for rationale and the Slice 9 / Slice 11 split plan.

use lexer::sdbl::{tokenize_sdbl, SdblToken, SdblTokenKind};

fn significant(tokens: &[SdblToken]) -> Vec<(SdblTokenKind, String)> {
    tokens
        .iter()
        .filter(|t| t.kind != SdblTokenKind::Whitespace && t.kind != SdblTokenKind::Newline)
        .map(|t| (t.kind, t.text.to_string()))
        .collect()
}

fn single_kind(src: &str) -> SdblTokenKind {
    let toks: Vec<_> = tokenize_sdbl(src)
        .into_iter()
        .filter(|t| t.kind != SdblTokenKind::Whitespace && t.kind != SdblTokenKind::Newline)
        .collect();
    assert_eq!(toks.len(), 1, "expected exactly one token for {src:?}, got {toks:#?}");
    toks[0].kind
}

// ---------------------------------------------------------------------------
// Bilingual acceptance — clause starters
// ---------------------------------------------------------------------------

/// ITS pubqlang/10 — SELECT clause: the clause starters accept a
/// Russian and an English spelling, both case-insensitive.
#[test]
fn clause_starters_bilingual() {
    let pairs: &[(&str, &str, SdblTokenKind)] = &[
        ("ВЫБРАТЬ", "SELECT", SdblTokenKind::KwSelect),
        ("ИЗ", "FROM", SdblTokenKind::KwFrom),
        ("ПОМЕСТИТЬ", "INTO", SdblTokenKind::KwInto),
        ("ГДЕ", "WHERE", SdblTokenKind::KwWhere),
        ("СГРУППИРОВАТЬ", "GROUP", SdblTokenKind::KwGroup),
        ("УПОРЯДОЧИТЬ", "ORDER", SdblTokenKind::KwOrder),
        ("ИМЕЮЩИЕ", "HAVING", SdblTokenKind::KwHaving),
        ("ИТОГИ", "TOTALS", SdblTokenKind::KwTotals),
        ("ОБЪЕДИНИТЬ", "UNION", SdblTokenKind::KwUnion),
        ("ВСЕ", "ALL", SdblTokenKind::KwAll),
        ("РАЗЛИЧНЫЕ", "DISTINCT", SdblTokenKind::KwDistinct),
        ("ПЕРВЫЕ", "TOP", SdblTokenKind::KwTop),
    ];
    for (rus, eng, expected) in pairs {
        assert_eq!(single_kind(rus), *expected, "Russian {rus} should lex as {expected:?}");
        assert_eq!(single_kind(eng), *expected, "English {eng} should lex as {expected:?}");
    }
}

// ---------------------------------------------------------------------------
// Bilingual acceptance — join family
// ---------------------------------------------------------------------------

/// ITS pubqlang/10 — join clause: each join modifier and the JOIN
/// keyword accept a Russian and an English spelling.
#[test]
fn join_family_bilingual() {
    let pairs: &[(&str, &str, SdblTokenKind)] = &[
        ("СОЕДИНЕНИЕ", "JOIN", SdblTokenKind::KwJoin),
        ("ВНУТРЕННЕЕ", "INNER", SdblTokenKind::KwInner),
        ("ЛЕВОЕ", "LEFT", SdblTokenKind::KwLeft),
        ("ПРАВОЕ", "RIGHT", SdblTokenKind::KwRight),
        ("ПОЛНОЕ", "FULL", SdblTokenKind::KwFull),
        ("ВНЕШНЕЕ", "OUTER", SdblTokenKind::KwOuter),
    ];
    for (rus, eng, expected) in pairs {
        assert_eq!(single_kind(rus), *expected);
        assert_eq!(single_kind(eng), *expected);
    }
}

// ---------------------------------------------------------------------------
// Bilingual acceptance — aliasing and predicates
// ---------------------------------------------------------------------------

/// ITS pubqlang/10 — field aliasing and predicates: each accepts a
/// Russian and an English spelling.
#[test]
fn aliasing_and_predicates_bilingual() {
    let pairs: &[(&str, &str, SdblTokenKind)] = &[
        ("КАК", "AS", SdblTokenKind::KwAs),
        ("В", "IN", SdblTokenKind::KwIn),
        ("МЕЖДУ", "BETWEEN", SdblTokenKind::KwBetween),
        ("ПОДОБНО", "LIKE", SdblTokenKind::KwLike),
        ("ЕСТЬ", "IS", SdblTokenKind::KwIs),
    ];
    for (rus, eng, expected) in pairs {
        assert_eq!(single_kind(rus), *expected);
        assert_eq!(single_kind(eng), *expected);
    }
}

// ---------------------------------------------------------------------------
// Bilingual acceptance — CASE family
// ---------------------------------------------------------------------------

/// ITS pubqlang/10 — conditional expression: CASE / WHEN / THEN /
/// ELSE / END each accept a Russian and an English spelling.
#[test]
fn case_family_bilingual() {
    let pairs: &[(&str, &str, SdblTokenKind)] = &[
        ("ВЫБОР", "CASE", SdblTokenKind::KwCase),
        ("КОГДА", "WHEN", SdblTokenKind::KwWhen),
        ("ТОГДА", "THEN", SdblTokenKind::KwThen),
        ("ИНАЧЕ", "ELSE", SdblTokenKind::KwElse),
        ("КОНЕЦ", "END", SdblTokenKind::KwEnd),
    ];
    for (rus, eng, expected) in pairs {
        assert_eq!(single_kind(rus), *expected);
        assert_eq!(single_kind(eng), *expected);
    }
}

// ---------------------------------------------------------------------------
// Bilingual acceptance — logical operators and literals
// ---------------------------------------------------------------------------

/// ITS pubqlang/12 — logical operators: AND / OR / NOT each accept a
/// Russian and an English spelling.
#[test]
fn logical_operators_bilingual() {
    assert_eq!(single_kind("И"), SdblTokenKind::OpAnd);
    assert_eq!(single_kind("AND"), SdblTokenKind::OpAnd);
    assert_eq!(single_kind("ИЛИ"), SdblTokenKind::OpOr);
    assert_eq!(single_kind("OR"), SdblTokenKind::OpOr);
    assert_eq!(single_kind("НЕ"), SdblTokenKind::OpNot);
    assert_eq!(single_kind("NOT"), SdblTokenKind::OpNot);
}

/// ITS pubqlang/12 — boolean literals: TRUE and FALSE each accept a
/// Russian and an English spelling. NULL is English-only by the
/// grammar.
#[test]
fn boolean_and_null_literals() {
    assert_eq!(single_kind("ИСТИНА"), SdblTokenKind::LitTrue);
    assert_eq!(single_kind("TRUE"), SdblTokenKind::LitTrue);
    assert_eq!(single_kind("ЛОЖЬ"), SdblTokenKind::LitFalse);
    assert_eq!(single_kind("FALSE"), SdblTokenKind::LitFalse);
    assert_eq!(single_kind("NULL"), SdblTokenKind::LitNull);
}

// ---------------------------------------------------------------------------
// Case-insensitivity
// ---------------------------------------------------------------------------

/// ITS pubqlang/12 — lexical elements: keyword matching is
/// case-insensitive. Every case-permutation of a keyword lexes to the
/// same kind with its original text preserved.
#[test]
fn case_insensitivity_english() {
    for s in ["SELECT", "Select", "select", "sElEcT"] {
        let toks = tokenize_sdbl(s);
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, SdblTokenKind::KwSelect);
        assert_eq!(toks[0].text.as_str(), s);
    }
}

/// ITS pubqlang/12 — lexical elements: case-insensitivity applies to
/// Russian keyword spellings too.
#[test]
fn case_insensitivity_russian() {
    for s in ["ВЫБРАТЬ", "Выбрать", "выбрать", "вЫбРаТь"] {
        let toks = tokenize_sdbl(s);
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, SdblTokenKind::KwSelect);
        assert_eq!(toks[0].text.as_str(), s);
    }
}

/// ITS pubqlang/10 — join clause: case-insensitivity across the join
/// family in a realistic clause.
#[test]
fn case_insensitivity_join_family() {
    assert_eq!(single_kind("Left"), SdblTokenKind::KwLeft);
    assert_eq!(single_kind("lEfT"), SdblTokenKind::KwLeft);
    assert_eq!(single_kind("inner"), SdblTokenKind::KwInner);
    assert_eq!(single_kind("OUTER"), SdblTokenKind::KwOuter);
}

/// ITS pubqlang/10 — conditional expression: case-insensitivity of
/// the CASE family.
#[test]
fn case_insensitivity_case_family() {
    assert_eq!(single_kind("Case"), SdblTokenKind::KwCase);
    assert_eq!(single_kind("WHEN"), SdblTokenKind::KwWhen);
    assert_eq!(single_kind("tHeN"), SdblTokenKind::KwThen);
    assert_eq!(single_kind("else"), SdblTokenKind::KwElse);
    assert_eq!(single_kind("End"), SdblTokenKind::KwEnd);
}

/// ITS pubqlang/10 — predicates: case-insensitivity across the
/// predicate family.
#[test]
fn case_insensitivity_predicates() {
    assert_eq!(single_kind("Between"), SdblTokenKind::KwBetween);
    assert_eq!(single_kind("lIkE"), SdblTokenKind::KwLike);
    assert_eq!(single_kind("is"), SdblTokenKind::KwIs);
}

// ---------------------------------------------------------------------------
// Non-overlap with Ident (longest-match)
// ---------------------------------------------------------------------------

/// ITS pubqlang/12 — identifier rule: a Unicode letter followed by
/// letters, digits, or underscores is a single identifier. Logos
/// longest-match therefore consumes a keyword-prefixed identifier as
/// one `Ident`, not as keyword plus suffix.
#[test]
fn keyword_prefix_identifiers_lex_as_ident() {
    for s in ["SELECTED", "FROMAGE", "WHEREVER", "ANDROID", "UNIONIZED", "BETWEEN_INDEX"] {
        let toks = tokenize_sdbl(s);
        assert_eq!(toks.len(), 1, "expected one token for {s:?}");
        assert_eq!(toks[0].kind, SdblTokenKind::Ident, "{s:?} should lex as Ident");
        assert_eq!(toks[0].text.as_str(), s);
    }
}

/// ITS pubqlang/12 — identifier rule applies to Cyrillic leading
/// letters too: `ИСТИНАLIKE` is a single identifier spanning Russian
/// and Latin letters, not `LitTrue` + suffix.
#[test]
fn keyword_prefix_identifiers_lex_as_ident_cyrillic() {
    for s in ["ИСТИНАLIKE", "ВЫБРАТЬ_КОЛОНКА", "СОЕДИНЕНИЕ1С"] {
        let toks = tokenize_sdbl(s);
        assert_eq!(toks.len(), 1, "expected one token for {s:?}");
        assert_eq!(toks[0].kind, SdblTokenKind::Ident);
        assert_eq!(toks[0].text.as_str(), s);
    }
}

// ---------------------------------------------------------------------------
// Longest-match vs Ident priority
// ---------------------------------------------------------------------------

/// ITS pubqlang/12 — identifier rule: a keyword followed immediately
/// by a digit becomes an identifier because the digit extends the
/// identifier run.
#[test]
fn keyword_plus_digit_is_ident() {
    for s in ["SELECT1", "FROM2", "WHERE3", "CASE4"] {
        let toks = tokenize_sdbl(s);
        assert_eq!(toks.len(), 1, "expected one token for {s:?}");
        assert_eq!(toks[0].kind, SdblTokenKind::Ident);
        assert_eq!(toks[0].text.as_str(), s);
    }
}

/// ITS pubqlang/12 — identifier rule: two keywords fused without a
/// separator are a single identifier, not two keywords.
#[test]
fn fused_keywords_lex_as_ident() {
    for s in ["CASEWHEN", "SELECTFROM", "GROUPBY"] {
        let toks = tokenize_sdbl(s);
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, SdblTokenKind::Ident, "{s:?} should lex as Ident");
    }
}

// ---------------------------------------------------------------------------
// Adjacency — keyword followed immediately by a non-identifier char
// ---------------------------------------------------------------------------

/// ITS pubqlang/10 — SELECT clause: `(` after SELECT separates from
/// the keyword without whitespace.
#[test]
fn select_adjacent_lparen() {
    let toks = significant(&tokenize_sdbl("SELECT("));
    assert_eq!(
        toks,
        vec![
            (SdblTokenKind::KwSelect, "SELECT".to_string()),
            (SdblTokenKind::LParen, "(".to_string()),
        ]
    );
}

/// ITS pubqlang/10 — parameters and SELECT clause: `&Parameter` after
/// WHERE separates into keyword and parameter reference.
#[test]
fn where_adjacent_parameter() {
    let toks = significant(&tokenize_sdbl("WHERE&P"));
    assert_eq!(
        toks,
        vec![
            (SdblTokenKind::KwWhere, "WHERE".to_string()),
            (SdblTokenKind::Parameter, "&P".to_string()),
        ]
    );
}

/// ITS pubqlang/12 — separators: a comma immediately after a boolean
/// literal separates it from the next token without whitespace.
#[test]
fn true_adjacent_comma() {
    let toks = significant(&tokenize_sdbl("TRUE,FALSE"));
    assert_eq!(
        toks,
        vec![
            (SdblTokenKind::LitTrue, "TRUE".to_string()),
            (SdblTokenKind::Comma, ",".to_string()),
            (SdblTokenKind::LitFalse, "FALSE".to_string()),
        ]
    );
}

/// ITS pubqlang/10 — predicates: `IS NULL` in direct adjacency to
/// parentheses splits into KwIs / Whitespace / LitNull.
#[test]
fn is_null_in_where_adjacency() {
    let toks = significant(&tokenize_sdbl("(X IS NULL)"));
    assert_eq!(
        toks,
        vec![
            (SdblTokenKind::LParen, "(".to_string()),
            (SdblTokenKind::Ident, "X".to_string()),
            (SdblTokenKind::KwIs, "IS".to_string()),
            (SdblTokenKind::LitNull, "NULL".to_string()),
            (SdblTokenKind::RParen, ")".to_string()),
        ]
    );
}

// ---------------------------------------------------------------------------
// KwOnOrBy — preserved pre-refactor bundling
// ---------------------------------------------------------------------------

/// ITS pubqlang/10 — join clause (ON) and grouping clause (BY / ПО):
/// preserved pre-refactor behaviour is that ON, BY, and ПО all lex to
/// the same `KwOnOrBy` kind; the parser disambiguates by context.
/// See `docs/legal/sdbl-clean-room-slice2.md` § Preserved pre-refactor
/// behaviours for the rationale and the Slice 9 / Slice 11 split plan.
#[test]
fn on_by_po_share_kwonorby() {
    assert_eq!(single_kind("ON"), SdblTokenKind::KwOnOrBy);
    assert_eq!(single_kind("on"), SdblTokenKind::KwOnOrBy);
    assert_eq!(single_kind("BY"), SdblTokenKind::KwOnOrBy);
    assert_eq!(single_kind("by"), SdblTokenKind::KwOnOrBy);
    assert_eq!(single_kind("ПО"), SdblTokenKind::KwOnOrBy);
    assert_eq!(single_kind("по"), SdblTokenKind::KwOnOrBy);
}

/// ITS pubqlang/10 — text preservation: even though ON / BY / ПО all
/// resolve to one kind, the original text is preserved unchanged so
/// the parser can distinguish them when needed.
#[test]
fn kwonorby_preserves_original_text() {
    for s in ["ON", "BY", "ПО", "on", "bY", "По"] {
        let toks = tokenize_sdbl(s);
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, SdblTokenKind::KwOnOrBy);
        assert_eq!(toks[0].text.as_str(), s);
    }
}

// ---------------------------------------------------------------------------
// Text preservation (converter-facing invariant)
// ---------------------------------------------------------------------------

/// ITS pubqlang/12 — lexical elements: every keyword token preserves
/// its original input text. The downstream converter maps Slice 2
/// `Kw*` tokens to `TokenKind::Ident` and re-checks the text, so text
/// fidelity is a load-bearing invariant beyond mere display.
#[test]
fn keyword_text_preserved_verbatim() {
    let src = "Выбрать ПЕРВЫЕ 10 a ГДЕ b МЕЖДУ 1 И 10";
    let toks = significant(&tokenize_sdbl(src));
    let expected: Vec<(SdblTokenKind, &str)> = vec![
        (SdblTokenKind::KwSelect, "Выбрать"),
        (SdblTokenKind::KwTop, "ПЕРВЫЕ"),
        (SdblTokenKind::Decimal, "10"),
        (SdblTokenKind::Ident, "a"),
        (SdblTokenKind::KwWhere, "ГДЕ"),
        (SdblTokenKind::Ident, "b"),
        (SdblTokenKind::KwBetween, "МЕЖДУ"),
        (SdblTokenKind::Decimal, "1"),
        (SdblTokenKind::OpAnd, "И"),
        (SdblTokenKind::Decimal, "10"),
    ];
    assert_eq!(toks, expected.into_iter().map(|(k, t)| (k, t.to_string())).collect::<Vec<_>>());
}

// ---------------------------------------------------------------------------
// Representative clause fragments — integration-shaped sanity
// ---------------------------------------------------------------------------

/// ITS pubqlang/10 — SELECT clause: DISTINCT and TOP modifiers appear
/// between SELECT and the field list.
#[test]
fn select_distinct_top_sequence() {
    let toks: Vec<_> = significant(&tokenize_sdbl("SELECT DISTINCT TOP 5 A"))
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(
        toks,
        vec![
            SdblTokenKind::KwSelect,
            SdblTokenKind::KwDistinct,
            SdblTokenKind::KwTop,
            SdblTokenKind::Decimal,
            SdblTokenKind::Ident,
        ]
    );
}

/// ITS pubqlang/10 — join clause: LEFT OUTER JOIN ... ON ... emits
/// the expected token sequence with KwOnOrBy on the ON keyword.
#[test]
fn left_outer_join_on_sequence() {
    let toks: Vec<_> = significant(&tokenize_sdbl("LEFT OUTER JOIN T ON A = B"))
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(
        toks,
        vec![
            SdblTokenKind::KwLeft,
            SdblTokenKind::KwOuter,
            SdblTokenKind::KwJoin,
            SdblTokenKind::Ident,
            SdblTokenKind::KwOnOrBy,
            SdblTokenKind::Ident,
            SdblTokenKind::Eq,
            SdblTokenKind::Ident,
        ]
    );
}

/// ITS pubqlang/10 — SELECT clause: UNION ALL combines two SELECTs.
#[test]
fn union_all_sequence() {
    let toks: Vec<_> = significant(&tokenize_sdbl("SELECT 1 UNION ALL SELECT 2"))
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(
        toks,
        vec![
            SdblTokenKind::KwSelect,
            SdblTokenKind::Decimal,
            SdblTokenKind::KwUnion,
            SdblTokenKind::KwAll,
            SdblTokenKind::KwSelect,
            SdblTokenKind::Decimal,
        ]
    );
}

/// ITS pubqlang/10 — conditional expression: a full CASE / WHEN /
/// THEN / ELSE / END expression emits the expected keyword sequence.
#[test]
fn case_when_then_else_end_sequence() {
    let toks: Vec<_> = significant(&tokenize_sdbl("CASE WHEN X > 0 THEN 1 ELSE 0 END"))
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(
        toks,
        vec![
            SdblTokenKind::KwCase,
            SdblTokenKind::KwWhen,
            SdblTokenKind::Ident,
            SdblTokenKind::Gt,
            SdblTokenKind::Decimal,
            SdblTokenKind::KwThen,
            SdblTokenKind::Decimal,
            SdblTokenKind::KwElse,
            SdblTokenKind::Decimal,
            SdblTokenKind::KwEnd,
        ]
    );
}

/// ITS pubqlang/10 — field aliasing: `AS alias` in select-list and
/// FROM-source position emits KwAs in both slots. Disambiguation
/// (alias vs CAST target) is a parser-level concern.
#[test]
fn as_in_select_and_from_positions() {
    let src = "SELECT A AS B FROM T AS X";
    let toks: Vec<_> = significant(&tokenize_sdbl(src)).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        toks,
        vec![
            SdblTokenKind::KwSelect,
            SdblTokenKind::Ident,
            SdblTokenKind::KwAs,
            SdblTokenKind::Ident,
            SdblTokenKind::KwFrom,
            SdblTokenKind::Ident,
            SdblTokenKind::KwAs,
            SdblTokenKind::Ident,
        ]
    );
}

/// ITS pubqlang/10 — predicates: IN with a parenthesised parameter
/// list emits KwIn plus the parameter.
#[test]
fn in_predicate_with_parameter_list() {
    let toks: Vec<_> =
        significant(&tokenize_sdbl("WHERE X IN (&List)")).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        toks,
        vec![
            SdblTokenKind::KwWhere,
            SdblTokenKind::Ident,
            SdblTokenKind::KwIn,
            SdblTokenKind::LParen,
            SdblTokenKind::Parameter,
            SdblTokenKind::RParen,
        ]
    );
}

/// ITS pubqlang/10 — predicates: BETWEEN A AND B emits KwBetween plus
/// OpAnd as the range delimiter.
#[test]
fn between_and_predicate() {
    let toks: Vec<_> =
        significant(&tokenize_sdbl("X BETWEEN 1 AND 10")).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        toks,
        vec![
            SdblTokenKind::Ident,
            SdblTokenKind::KwBetween,
            SdblTokenKind::Decimal,
            SdblTokenKind::OpAnd,
            SdblTokenKind::Decimal,
        ]
    );
}

/// ITS pubqlang/10 — predicates: LIKE with a string pattern emits
/// KwLike between the column and the pattern literal.
#[test]
fn like_string_pattern() {
    let toks: Vec<_> = tokenize_sdbl("N LIKE \"%x%\"")
        .into_iter()
        .filter(|t| t.kind != SdblTokenKind::Whitespace)
        .map(|t| t.kind)
        .collect();
    assert!(toks.contains(&SdblTokenKind::KwLike));
    assert!(matches!(toks[0], SdblTokenKind::Ident));
}
