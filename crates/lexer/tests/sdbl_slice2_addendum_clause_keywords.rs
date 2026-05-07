//! Clean-room acceptance tests for the Slice 2-addendum clause
//! keyword leftovers vocabulary of `SdblTokenKind`.
//!
//! Status: file born at C2 with 3 KwPeriods regression-gate tests
//! landing alongside the regex defect fix per
//! `docs/legal/sdbl-clean-room-slice2-addendum.md` § Behaviour
//! change Option A. C3 expanded the file to 29 spec-driven
//! acceptance tests: 3 KwPeriods regression gates, 16 bilingual
//! EN+RU variant pairs (KwPeriods covered by the regression
//! gates), 1 case-insensitivity sweep, 9 structural integration
//! tests, and 1 keyword-prefix Ident longest-match guard.
//!
//! Sources (per the per-variant tier source map in the
//! attestation):
//! - **Primary** SDBL grammar: v8.3.27 Developer's Reference Глава 8
//!   «Работа с запросами» —
//!   <https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>.
//! - **Secondary corroborating** ITS pubqlang dump at
//!   <https://its.1c.ru/db/pubqlang/content/N/hdoc> (chapters 16,
//!   17, 27, 31, 39, 40, 51, 73, 96).

use lexer::sdbl::{tokenize_sdbl, SdblTokenKind};

fn single_kind(src: &str) -> SdblTokenKind {
    let toks: Vec<_> = tokenize_sdbl(src)
        .into_iter()
        .filter(|t| t.kind != SdblTokenKind::Whitespace && t.kind != SdblTokenKind::Newline)
        .collect();
    assert_eq!(toks.len(), 1, "expected exactly one token for {src:?}, got {toks:#?}");
    toks[0].kind
}

// ---------------------------------------------------------------------------
// KwPeriods — canonical-spelling regression gates (C2 § Behaviour change)
// ---------------------------------------------------------------------------
//
// v8327doc Глава 8 specifies the Russian TOTALS BY period-spec keyword
// in **instrumental case** as `ПЕРИОДАМИ` (canonical EBNF
// `[ПЕРИОДАМИ(<period-types>, <begin>, <end>)]` + bilingual word-list
// pair ПЕРИОДАМИ ↔ PERIODS + canonical example
// `Период ПЕРИОДАМИ(МИНУТА, ДАТАВРЕМЯ(...), ДАТАВРЕМЯ(...))`).
//
// The pre-Slice-2-addendum lexer regex matched the wrong nominative-
// case form `ПЕРИОДЫ`. The Slice 2-addendum C2 commit flipped the
// Russian alternation to the canonical `ПЕРИОДАМИ` form per the
// codex-consult Option A verdict. These three tests pin the post-fix
// behaviour:
//   - ПЕРИОДАМИ tokenises as `KwPeriods` (canonical Russian);
//   - PERIODS tokenises as `KwPeriods` (English unchanged by the fix);
//   - ПЕРИОДЫ tokenises as `Ident` (legacy nominative-case misspelling
//     now falls through to identifier).
//
// See `docs/legal/sdbl-clean-room-slice2-addendum.md` § Behaviour
// change for the full rationale and impact analysis.

/// v8327doc Глава 8 — TOTALS BY period spec: canonical Russian
/// keyword `ПЕРИОДАМИ` (instrumental case) tokenises as `KwPeriods`.
#[test]
fn kw_periods_canonical_russian_periodami() {
    assert_eq!(single_kind("ПЕРИОДАМИ"), SdblTokenKind::KwPeriods);
}

/// v8327doc Глава 8 — TOTALS BY period spec: English keyword
/// `PERIODS` tokenises as `KwPeriods` (unchanged by the C2 regex
/// fix; the English alternation was already canonical).
#[test]
fn kw_periods_english_unchanged() {
    assert_eq!(single_kind("PERIODS"), SdblTokenKind::KwPeriods);
}

/// Slice 2-addendum § Behaviour change — legacy Russian misspelling
/// `ПЕРИОДЫ` (nominative case) now falls through to `Ident` after the
/// C2 regex fix flipped the Russian alternation to the canonical
/// `ПЕРИОДАМИ` form. The token converter at
/// `crates/parser/src/sdbl_token_converter.rs` already maps
/// `KwPeriods → TokenKind::Ident`, so consumer code that read the
/// nominative form still gets an identifier-shaped token; only the
/// lexer-internal classification flips.
#[test]
fn kw_periods_legacy_misspelling_now_ident() {
    assert_eq!(single_kind("ПЕРИОДЫ"), SdblTokenKind::Ident);
}

// ---------------------------------------------------------------------------
// Bilingual acceptance — Slice 2-addendum vocabulary
// ---------------------------------------------------------------------------
//
// Each of the 17 Slice 2-addendum variants is bilingual: a Russian
// spelling and an English spelling lex to the same `SdblTokenKind`.
// These tests pin the bilingual contract per the v8327doc Глава 8
// word-list slots and corroborating pubqlang chapters cited in the
// per-variant docstrings of `crates/lexer/src/sdbl/mod.rs`.
//
// Note: KwPeriods is not in the bilingual sweep below — its Russian
// spelling is the instrumental-case `ПЕРИОДАМИ` (covered by
// `kw_periods_canonical_russian_periodami` and
// `kw_periods_english_unchanged` above), and the legacy nominative
// form `ПЕРИОДЫ` is intentionally rejected by the addendum regex
// (covered by `kw_periods_legacy_misspelling_now_ident`).

/// v8327doc Глава 8 — DROP query: bilingual УНИЧТОЖИТЬ ↔ DROP.
#[test]
fn kw_drop_bilingual() {
    assert_eq!(single_kind("УНИЧТОЖИТЬ"), SdblTokenKind::KwDrop);
    assert_eq!(single_kind("DROP"), SdblTokenKind::KwDrop);
}

/// pubqlang/17 — AUTOORDER: bilingual АВТОУПОРЯДОЧИВАНИЕ ↔ AUTOORDER.
#[test]
fn kw_autoorder_bilingual() {
    assert_eq!(single_kind("АВТОУПОРЯДОЧИВАНИЕ"), SdblTokenKind::KwAutoOrder);
    assert_eq!(single_kind("AUTOORDER"), SdblTokenKind::KwAutoOrder);
}

/// pubqlang/16 — ORDER BY direction: bilingual ВОЗР ↔ ASC.
#[test]
fn kw_asc_bilingual() {
    assert_eq!(single_kind("ВОЗР"), SdblTokenKind::KwAsc);
    assert_eq!(single_kind("ASC"), SdblTokenKind::KwAsc);
}

/// pubqlang/16 — ORDER BY direction: bilingual УБЫВ ↔ DESC.
#[test]
fn kw_desc_bilingual() {
    assert_eq!(single_kind("УБЫВ"), SdblTokenKind::KwDesc);
    assert_eq!(single_kind("DESC"), SdblTokenKind::KwDesc);
}

/// v8327doc Глава 8 + pubqlang/27 — HIERARCHY modifier: bilingual
/// ИЕРАРХИЯ ↔ HIERARCHY.
#[test]
fn kw_hierarchy_bilingual() {
    assert_eq!(single_kind("ИЕРАРХИЯ"), SdblTokenKind::KwHierarchy);
    assert_eq!(single_kind("HIERARCHY"), SdblTokenKind::KwHierarchy);
}

/// v8327doc Глава 8 — SELECT prefix qualifier: bilingual
/// РАЗРЕШЕННЫЕ ↔ ALLOWED.
#[test]
fn kw_allowed_bilingual() {
    assert_eq!(single_kind("РАЗРЕШЕННЫЕ"), SdblTokenKind::KwAllowed);
    assert_eq!(single_kind("ALLOWED"), SdblTokenKind::KwAllowed);
}

/// v8327doc Глава 8 — FOR UPDATE clause: bilingual ДЛЯ ↔ FOR.
#[test]
fn kw_for_bilingual() {
    assert_eq!(single_kind("ДЛЯ"), SdblTokenKind::KwFor);
    assert_eq!(single_kind("FOR"), SdblTokenKind::KwFor);
}

/// v8327doc Глава 8 — FOR UPDATE clause: bilingual ИЗМЕНЕНИЯ ↔ UPDATE.
#[test]
fn kw_update_bilingual() {
    assert_eq!(single_kind("ИЗМЕНЕНИЯ"), SdblTokenKind::KwUpdate);
    assert_eq!(single_kind("UPDATE"), SdblTokenKind::KwUpdate);
}

/// v8327doc Глава 8 — INDEX BY clause: bilingual ИНДЕКСИРОВАТЬ ↔ INDEX.
#[test]
fn kw_index_bilingual() {
    assert_eq!(single_kind("ИНДЕКСИРОВАТЬ"), SdblTokenKind::KwIndex);
    assert_eq!(single_kind("INDEX"), SdblTokenKind::KwIndex);
}

/// v8327doc Глава 8 — TOTALS BY group modifier: bilingual ТОЛЬКО ↔ ONLY.
#[test]
fn kw_only_bilingual() {
    assert_eq!(single_kind("ТОЛЬКО"), SdblTokenKind::KwOnly);
    assert_eq!(single_kind("ONLY"), SdblTokenKind::KwOnly);
}

/// pubqlang/39 — TOTALS BY OVERALL group: bilingual ОБЩИЕ ↔ OVERALL.
#[test]
fn kw_overall_bilingual() {
    assert_eq!(single_kind("ОБЩИЕ"), SdblTokenKind::KwOverall);
    assert_eq!(single_kind("OVERALL"), SdblTokenKind::KwOverall);
}

/// v8327doc Глава 8 — LIKE escape clause: bilingual СПЕЦСИМВОЛ ↔ ESCAPE.
#[test]
fn kw_escape_bilingual() {
    assert_eq!(single_kind("СПЕЦСИМВОЛ"), SdblTokenKind::KwEscape);
    assert_eq!(single_kind("ESCAPE"), SdblTokenKind::KwEscape);
}

/// v8327doc Глава 8 + pubqlang/40 — REFS predicate: bilingual
/// ССЫЛКА ↔ REFS.
#[test]
fn kw_refs_bilingual() {
    assert_eq!(single_kind("ССЫЛКА"), SdblTokenKind::KwRefs);
    assert_eq!(single_kind("REFS"), SdblTokenKind::KwRefs);
}

/// v8327doc Глава 8 + pubqlang/40 — CAST expression: bilingual
/// ВЫРАЗИТЬ ↔ CAST.
#[test]
fn kw_cast_bilingual() {
    assert_eq!(single_kind("ВЫРАЗИТЬ"), SdblTokenKind::KwCast);
    assert_eq!(single_kind("CAST"), SdblTokenKind::KwCast);
}

/// v8327doc Глава 8 — TYPE expression: bilingual ТИП ↔ TYPE.
#[test]
fn kw_type_bilingual() {
    assert_eq!(single_kind("ТИП"), SdblTokenKind::KwType);
    assert_eq!(single_kind("TYPE"), SdblTokenKind::KwType);
}

/// pubqlang/31 + /96 — VALUE expression: bilingual ЗНАЧЕНИЕ ↔ VALUE.
#[test]
fn kw_value_bilingual() {
    assert_eq!(single_kind("ЗНАЧЕНИЕ"), SdblTokenKind::KwValue);
    assert_eq!(single_kind("VALUE"), SdblTokenKind::KwValue);
}

// ---------------------------------------------------------------------------
// Case-insensitivity — Slice 2-addendum vocabulary
// ---------------------------------------------------------------------------

/// v8327doc Глава 8 — case-insensitivity applies uniformly across the
/// addendum vocabulary. Both Russian and English forms accept any
/// case-permutation per the project's case-insensitive regex pattern
/// `(?i)<russian>|(?i)<english>`.
#[test]
fn case_insensitivity_addendum() {
    for s in ["УНИЧТОЖИТЬ", "Уничтожить", "уничтожить"] {
        assert_eq!(single_kind(s), SdblTokenKind::KwDrop);
    }
    for s in ["DROP", "Drop", "drop"] {
        assert_eq!(single_kind(s), SdblTokenKind::KwDrop);
    }
    for s in ["ВЫРАЗИТЬ", "Выразить", "выразить"] {
        assert_eq!(single_kind(s), SdblTokenKind::KwCast);
    }
    for s in ["ИНДЕКСИРОВАТЬ", "Индексировать", "индексировать"]
    {
        assert_eq!(single_kind(s), SdblTokenKind::KwIndex);
    }
    for s in ["ПЕРИОДАМИ", "Периодами", "периодами"] {
        assert_eq!(single_kind(s), SdblTokenKind::KwPeriods);
    }
}

// ---------------------------------------------------------------------------
// Structural — addendum keywords in realistic SDBL clause fragments
// ---------------------------------------------------------------------------

fn significant_kinds(src: &str) -> Vec<SdblTokenKind> {
    tokenize_sdbl(src)
        .into_iter()
        .filter(|t| t.kind != SdblTokenKind::Whitespace && t.kind != SdblTokenKind::Newline)
        .map(|t| t.kind)
        .collect()
}

/// v8327doc Глава 8 — DROP query in a batch: `... ; УНИЧТОЖИТЬ <ident>`
/// emits Semicolon between the prior query and the DROP statement.
#[test]
fn structural_drop_in_batch() {
    let kinds = significant_kinds("ВЫБРАТЬ 1 ПОМЕСТИТЬ #T; УНИЧТОЖИТЬ #T");
    assert!(kinds.contains(&SdblTokenKind::KwDrop));
    assert!(kinds.contains(&SdblTokenKind::Semicolon));
}

/// pubqlang/16, /27 — ORDER BY with direction modifiers and HIERARCHY:
/// the addendum vocabulary mixes naturally with Slice 2 keywords.
#[test]
fn structural_order_by_with_modifiers() {
    let kinds = significant_kinds("УПОРЯДОЧИТЬ ПО Имя ВОЗР, Цена УБЫВ ИЕРАРХИЯ");
    assert!(kinds.contains(&SdblTokenKind::KwOrder));
    assert!(kinds.contains(&SdblTokenKind::KwAsc));
    assert!(kinds.contains(&SdblTokenKind::KwDesc));
    assert!(kinds.contains(&SdblTokenKind::KwHierarchy));
}

/// v8327doc Глава 8 — TOTALS BY ... PERIODS canonical example
/// `Период ПЕРИОДАМИ(...)` emits KwPeriods at the head of the
/// period-spec sub-clause.
#[test]
fn structural_totals_periods_canonical() {
    let kinds = significant_kinds(
        "ИТОГИ СУММА(X) ПО ОБЩИЕ, Период ПЕРИОДАМИ(МИНУТА, ДАТАВРЕМЯ(2024, 1, 1), &End)",
    );
    assert!(kinds.contains(&SdblTokenKind::KwTotals));
    assert!(kinds.contains(&SdblTokenKind::KwOverall));
    assert!(kinds.contains(&SdblTokenKind::KwPeriods));
}

/// v8327doc Глава 8 — FOR UPDATE clause: `ДЛЯ ИЗМЕНЕНИЯ` two-word form
/// emits KwFor + KwUpdate as separate tokens (the parser combines them
/// at the grammar level).
#[test]
fn structural_for_update_two_word_form() {
    let kinds = significant_kinds("ВЫБРАТЬ 1 ИЗ T ДЛЯ ИЗМЕНЕНИЯ");
    assert_eq!(
        kinds
            .iter()
            .filter(|k| matches!(k, SdblTokenKind::KwFor | SdblTokenKind::KwUpdate))
            .count(),
        2
    );
}

/// v8327doc Глава 8 — INDEX BY clause: `ИНДЕКСИРОВАТЬ ПО <field>` emits
/// KwIndex + KwOnOrBy + Ident.
#[test]
fn structural_index_by_field() {
    let kinds = significant_kinds("ИНДЕКСИРОВАТЬ ПО T.F");
    assert!(kinds.contains(&SdblTokenKind::KwIndex));
    assert!(kinds.contains(&SdblTokenKind::KwOnOrBy));
}

/// v8327doc Глава 8 — LIKE ESCAPE: `ПОДОБНО "%X%" СПЕЦСИМВОЛ "!"` emits
/// KwLike + literal + KwEscape + literal.
#[test]
fn structural_like_escape_canonical() {
    let kinds = significant_kinds("Н ПОДОБНО \"%X%\" СПЕЦСИМВОЛ \"!\"");
    assert!(kinds.contains(&SdblTokenKind::KwLike));
    assert!(kinds.contains(&SdblTokenKind::KwEscape));
}

/// pubqlang/40 — REFS predicate canonical example
/// `(Регистратор ССЫЛКА Документ.X)` emits KwRefs between the
/// composite-typed expression and the metadata reference path.
#[test]
fn structural_refs_canonical() {
    let kinds = significant_kinds("ГДЕ Р ССЫЛКА Документ.ПриходнаяНакладная");
    assert!(kinds.contains(&SdblTokenKind::KwWhere));
    assert!(kinds.contains(&SdblTokenKind::KwRefs));
}

/// v8327doc Глава 8 — CAST expression: `ВЫРАЗИТЬ(<expr> КАК <type>)`
/// emits KwCast + LParen + ... + KwAs + ... + RParen.
#[test]
fn structural_cast_with_as_target() {
    let kinds = significant_kinds("ВЫРАЗИТЬ(П КАК ЧИСЛО(15, 2))");
    assert!(kinds.contains(&SdblTokenKind::KwCast));
    assert!(kinds.contains(&SdblTokenKind::KwAs));
}

/// pubqlang/31 — VALUE expression canonical example
/// `ЗНАЧЕНИЕ(Справочник.Товары.ПустаяСсылка)` emits KwValue at the
/// head of the parenthesised path.
#[test]
fn structural_value_canonical() {
    let kinds = significant_kinds("ЗНАЧЕНИЕ(Справочник.Товары.ПустаяСсылка)");
    assert!(kinds.contains(&SdblTokenKind::KwValue));
    assert!(kinds.contains(&SdblTokenKind::LParen));
    assert!(kinds.contains(&SdblTokenKind::RParen));
}

// ---------------------------------------------------------------------------
// Longest-match — addendum keywords vs Ident
// ---------------------------------------------------------------------------

/// ITS pubqlang/12 — identifier rule: a Unicode letter followed by
/// letters, digits, or underscores is a single identifier. Logos
/// longest-match therefore consumes an addendum-keyword-prefixed
/// identifier as one `Ident`, not as keyword plus suffix. This guard
/// mirrors the Slice 2 `keyword_prefix_identifiers_lex_as_ident`
/// pattern for the addendum vocabulary.
#[test]
fn addendum_keyword_prefix_identifiers_lex_as_ident() {
    for s in [
        "DROPPED",
        "ALLOWED_FLAG",
        "INDEXING",
        "CASTABLE",
        "ВЫРАЗИТЬNESS",
        "УНИЧТОЖИТЬ_ALL",
        "ИНДЕКСИРОВАТЬ_ВСЁ",
    ] {
        let toks = tokenize_sdbl(s);
        assert_eq!(toks.len(), 1, "expected one token for {s:?}");
        assert_eq!(toks[0].kind, SdblTokenKind::Ident, "{s:?} should lex as Ident");
        assert_eq!(toks[0].text.as_str(), s);
    }
}
