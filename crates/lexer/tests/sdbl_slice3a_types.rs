//! Clean-room acceptance tests for the Slice 3a primitive types,
//! undefined literal, and narrow period-type vocabulary of
//! `SdblTokenKind`.
//!
//! Status: file born at C3 with the full spec-driven acceptance
//! suite per `docs/legal/sdbl-clean-room-slice3a.md` § Scope. The
//! C0a discrepancy audit found zero regex defects, so no C2
//! regression-gate tests were needed; the Slice 3a acceptance
//! surface is born here as one batch — 25 spec-driven tests:
//! 14 bilingual EN+RU canonical-form pins (7 variants × 2
//! spellings), 1 case-insensitivity sweep, 9 structural integration
//! tests exercising the variants in their canonical SDBL grammar
//! positions (CAST type slot, TYPE() expression, <Значение>
//! predicate slot, TOTALS BY PERIODS period-type slot), and 1
//! keyword-prefix Ident longest-match guard.
//!
//! Sources (per the per-variant tier source map in the
//! attestation):
//! - **Primary** SDBL grammar: v8.3.27 Developer's Reference Глава 8
//!   «Работа с запросами» —
//!   <https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>.
//! - **Secondary corroborating** ITS pubqlang dump at
//!   <https://its.1c.ru/db/pubqlang/content/N/hdoc> (chapters 12,
//!   39, 40).

use lexer::sdbl::{tokenize_sdbl, SdblTokenKind};

fn single_kind(src: &str) -> SdblTokenKind {
    let toks: Vec<_> = tokenize_sdbl(src)
        .into_iter()
        .filter(|t| t.kind != SdblTokenKind::Whitespace && t.kind != SdblTokenKind::Newline)
        .collect();
    assert_eq!(toks.len(), 1, "expected exactly one token for {src:?}, got {toks:#?}");
    toks[0].kind
}

fn significant_kinds(src: &str) -> Vec<SdblTokenKind> {
    tokenize_sdbl(src)
        .into_iter()
        .filter(|t| t.kind != SdblTokenKind::Whitespace && t.kind != SdblTokenKind::Newline)
        .map(|t| t.kind)
        .collect()
}

// ---------------------------------------------------------------------------
// Bilingual variant pairs (14 tests = 7 variants × 2 spellings)
// ---------------------------------------------------------------------------
//
// v8327doc Глава 8 attests each Slice 3a variant with a Russian /
// English word-list row. These 14 tests pin both canonical
// spellings of every variant; under `(?i)` case folding, additional
// case-mix coverage is provided by the case-insensitivity sweep
// below.

// --- TypeBoolean ---

/// v8327doc Глава 8 — primitive type literal: bilingual word-list
/// БУЛЕВО ↔ BOOLEAN. Canonical Russian spelling tokenises as
/// `TypeBoolean`.
#[test]
fn type_boolean_russian_canonical() {
    assert_eq!(single_kind("БУЛЕВО"), SdblTokenKind::TypeBoolean);
}

/// v8327doc Глава 8 — primitive type literal: bilingual word-list
/// БУЛЕВО ↔ BOOLEAN. Canonical English spelling tokenises as
/// `TypeBoolean`.
#[test]
fn type_boolean_english_canonical() {
    assert_eq!(single_kind("BOOLEAN"), SdblTokenKind::TypeBoolean);
}

// --- TypeNumber ---

/// v8327doc Глава 8 — primitive type literal: bilingual word-list
/// ЧИСЛО ↔ NUMBER. Canonical Russian spelling tokenises as
/// `TypeNumber`.
#[test]
fn type_number_russian_canonical() {
    assert_eq!(single_kind("ЧИСЛО"), SdblTokenKind::TypeNumber);
}

/// v8327doc Глава 8 — primitive type literal: bilingual word-list
/// ЧИСЛО ↔ NUMBER. Canonical English spelling tokenises as
/// `TypeNumber`.
#[test]
fn type_number_english_canonical() {
    assert_eq!(single_kind("NUMBER"), SdblTokenKind::TypeNumber);
}

// --- TypeString ---

/// v8327doc Глава 8 — primitive type literal: bilingual word-list
/// СТРОКА ↔ STRING. Canonical Russian spelling tokenises as
/// `TypeString`.
#[test]
fn type_string_russian_canonical() {
    assert_eq!(single_kind("СТРОКА"), SdblTokenKind::TypeString);
}

/// v8327doc Глава 8 — primitive type literal: bilingual word-list
/// СТРОКА ↔ STRING. Canonical English spelling tokenises as
/// `TypeString`.
#[test]
fn type_string_english_canonical() {
    assert_eq!(single_kind("STRING"), SdblTokenKind::TypeString);
}

// --- TypeDate ---

/// v8327doc Глава 8 — primitive type literal: bilingual word-list
/// ДАТА ↔ DATE. Canonical Russian spelling tokenises as `TypeDate`.
#[test]
fn type_date_russian_canonical() {
    assert_eq!(single_kind("ДАТА"), SdblTokenKind::TypeDate);
}

/// v8327doc Глава 8 — primitive type literal: bilingual word-list
/// ДАТА ↔ DATE. Canonical English spelling tokenises as `TypeDate`.
#[test]
fn type_date_english_canonical() {
    assert_eq!(single_kind("DATE"), SdblTokenKind::TypeDate);
}

// --- LitUndefined ---

/// v8327doc Глава 8 — typed-undefined literal: bilingual word-list
/// НЕОПРЕДЕЛЕНО ↔ UNDEFINED. Canonical Russian spelling tokenises
/// as `LitUndefined`.
#[test]
fn lit_undefined_russian_canonical() {
    assert_eq!(single_kind("НЕОПРЕДЕЛЕНО"), SdblTokenKind::LitUndefined);
}

/// v8327doc Глава 8 — typed-undefined literal: bilingual word-list
/// НЕОПРЕДЕЛЕНО ↔ UNDEFINED. Canonical English spelling tokenises
/// as `LitUndefined`.
#[test]
fn lit_undefined_english_canonical() {
    assert_eq!(single_kind("UNDEFINED"), SdblTokenKind::LitUndefined);
}

// --- PeriodTenDays ---

/// v8327doc Глава 8 — TOTALS BY period-type literal: bilingual
/// word-list ДЕКАДА ↔ TENDAYS. Canonical Russian spelling
/// tokenises as `PeriodTenDays`.
#[test]
fn period_tendays_russian_canonical() {
    assert_eq!(single_kind("ДЕКАДА"), SdblTokenKind::PeriodTenDays);
}

/// v8327doc Глава 8 — TOTALS BY period-type literal: bilingual
/// word-list ДЕКАДА ↔ TENDAYS. Canonical English spelling
/// tokenises as `PeriodTenDays`.
#[test]
fn period_tendays_english_canonical() {
    assert_eq!(single_kind("TENDAYS"), SdblTokenKind::PeriodTenDays);
}

// --- PeriodHalfYear ---

/// v8327doc Глава 8 — TOTALS BY period-type literal: bilingual
/// word-list ПОЛУГОДИЕ ↔ HALFYEAR. Canonical Russian spelling
/// tokenises as `PeriodHalfYear`.
#[test]
fn period_halfyear_russian_canonical() {
    assert_eq!(single_kind("ПОЛУГОДИЕ"), SdblTokenKind::PeriodHalfYear);
}

/// v8327doc Глава 8 — TOTALS BY period-type literal: bilingual
/// word-list ПОЛУГОДИЕ ↔ HALFYEAR. Canonical English spelling
/// tokenises as `PeriodHalfYear`.
#[test]
fn period_halfyear_english_canonical() {
    assert_eq!(single_kind("HALFYEAR"), SdblTokenKind::PeriodHalfYear);
}

// ---------------------------------------------------------------------------
// Case-insensitivity sweep (1 test, 7 variants × 15 case-mix samples)
// ---------------------------------------------------------------------------
//
// The `(?i)` flag in each Slice 3a regex makes the token recognition
// case-insensitive across both alternation halves. This sweep
// verifies a representative spread of mixed-case spellings across
// all 7 variants in one test.

#[test]
fn case_insensitivity_sweep() {
    use SdblTokenKind::*;
    let cases: &[(&str, SdblTokenKind)] = &[
        ("Булево", TypeBoolean),
        ("boolean", TypeBoolean),
        ("BoOLeAn", TypeBoolean),
        ("Число", TypeNumber),
        ("number", TypeNumber),
        ("Строка", TypeString),
        ("string", TypeString),
        ("Дата", TypeDate),
        ("date", TypeDate),
        ("Неопределено", LitUndefined),
        ("undefined", LitUndefined),
        ("Декада", PeriodTenDays),
        ("tendays", PeriodTenDays),
        ("Полугодие", PeriodHalfYear),
        ("halfyear", PeriodHalfYear),
    ];
    for (src, expected) in cases {
        assert_eq!(single_kind(src), *expected, "case-insensitive mismatch on {src:?}");
    }
}

// ---------------------------------------------------------------------------
// Structural integration tests (9 tests)
// ---------------------------------------------------------------------------
//
// v8327doc Глава 8 grammar context for each Slice 3a variant. These
// tests verify the variant tokenises correctly inside its canonical
// SDBL grammar position alongside Slice 1 / Slice 2 / Slice 2-addendum
// neighbours.

/// CAST expression with Boolean type-slot:
/// `ВЫРАЗИТЬ ( <Выражение> КАК <Тип значения> )` per v8327doc
/// Глава 8 § Приведение типа.
#[test]
fn cast_to_boolean_russian() {
    use SdblTokenKind::*;
    let kinds = significant_kinds("ВЫРАЗИТЬ(X КАК БУЛЕВО)");
    assert_eq!(kinds, vec![KwCast, LParen, Ident, KwAs, TypeBoolean, RParen]);
}

/// CAST expression with Number(length, precision) type-slot.
/// v8327doc Глава 8 CAST grammar slot 2 with optional length and
/// precision modifiers.
#[test]
fn cast_to_number_with_precision_russian() {
    use SdblTokenKind::*;
    let kinds = significant_kinds("ВЫРАЗИТЬ(X КАК ЧИСЛО(15, 2))");
    assert_eq!(
        kinds,
        vec![
            KwCast, LParen, Ident, KwAs, TypeNumber, LParen, Decimal, Comma, Decimal, RParen,
            RParen,
        ]
    );
}

/// CAST expression with String(length) type-slot. v8327doc Глава 8
/// CAST grammar slot 3 with optional length modifier.
#[test]
fn cast_to_string_with_length_russian() {
    use SdblTokenKind::*;
    let kinds = significant_kinds("ВЫРАЗИТЬ(X КАК СТРОКА(50))");
    assert_eq!(
        kinds,
        vec![KwCast, LParen, Ident, KwAs, TypeString, LParen, Decimal, RParen, RParen]
    );
}

/// CAST expression with Date type-slot. v8327doc Глава 8 CAST
/// grammar slot 4 (no length / precision modifiers).
#[test]
fn cast_to_date_english() {
    use SdblTokenKind::*;
    let kinds = significant_kinds("CAST(X AS DATE)");
    assert_eq!(kinds, vec![KwCast, LParen, Ident, KwAs, TypeDate, RParen]);
}

/// TYPE() expression with primitive type-name argument: `ТИП(<Имя
/// типа>)` per v8327doc Глава 8 § Литерал типа Тип. The `Type*`
/// variants gate the `<Имя типа>` argument position.
#[test]
fn type_function_with_primitive_argument_russian() {
    use SdblTokenKind::*;
    let kinds = significant_kinds("ТИП(Число)");
    assert_eq!(kinds, vec![KwType, LParen, TypeNumber, RParen]);
}

/// TYPE() expression with English primitive argument.
#[test]
fn type_function_with_primitive_argument_english() {
    use SdblTokenKind::*;
    let kinds = significant_kinds("TYPE(Boolean)");
    assert_eq!(kinds, vec![KwType, LParen, TypeBoolean, RParen]);
}

/// LitUndefined and LitNull side-by-side in `<Значение>` predicate
/// positions. Both are typed-literal slots in the v8327doc Глава 8
/// `<Значение>` EBNF; the converter at
/// `crates/parser/src/sdbl_token_converter.rs` treats them
/// asymmetrically (LitNull → Ident with text probe; LitUndefined →
/// dedicated KwUndefined kind).
#[test]
fn undefined_predicate_position_english() {
    use SdblTokenKind::*;
    let kinds = significant_kinds("WHERE X = UNDEFINED OR Y <> NULL");
    assert_eq!(kinds, vec![KwWhere, Ident, Eq, LitUndefined, OpOr, Ident, Neq, LitNull]);
}

/// TOTALS BY PERIODS(TENDAYS, ...) — PeriodTenDays in slot 9 of the
/// 10-element canonical period-type list inside
/// `ПЕРИОДАМИ(<period-types>, <begin>, <end>)`. KwPeriods (Slice
/// 2-addendum) introduces the keyword.
#[test]
fn totals_by_periods_tendays_english() {
    use SdblTokenKind::*;
    let kinds = significant_kinds("TOTALS SUM(X) BY PERIODS(TENDAYS, &S, &E)");
    assert_eq!(
        kinds,
        vec![
            KwTotals,
            FnSum,
            LParen,
            Ident,
            RParen,
            KwOnOrBy,
            KwPeriods,
            LParen,
            PeriodTenDays,
            Comma,
            Parameter,
            Comma,
            Parameter,
            RParen,
        ]
    );
}

/// TOTALS BY PERIODS(HALFYEAR, ...) — PeriodHalfYear in slot 10 of
/// the 10-element canonical period-type list.
#[test]
fn totals_by_periods_halfyear_english() {
    use SdblTokenKind::*;
    let kinds = significant_kinds("TOTALS SUM(X) BY PERIODS(HALFYEAR, &S, &E)");
    assert_eq!(
        kinds,
        vec![
            KwTotals,
            FnSum,
            LParen,
            Ident,
            RParen,
            KwOnOrBy,
            KwPeriods,
            LParen,
            PeriodHalfYear,
            Comma,
            Parameter,
            Comma,
            Parameter,
            RParen,
        ]
    );
}

// ---------------------------------------------------------------------------
// Keyword-prefix Ident longest-match guard (1 test)
// ---------------------------------------------------------------------------
//
// Logos longest-match rule: identifier matches with priority = 1
// (Slice 1 attestation) win against keyword matches when the
// identifier is strictly longer than the keyword. This guard
// verifies that user-defined identifiers whose prefix coincides
// with a Slice 3a keyword do not get split into keyword-plus-suffix.

#[test]
fn keyword_prefix_idents_lex_as_ident() {
    use SdblTokenKind::*;
    let cases: &[&str] = &[
        "БУЛЕВОТЕСТ",     // Russian Ident with TypeBoolean prefix
        "numberOfItems",  // English Ident with TypeNumber prefix
        "СТРОКАТАБЛИЦЫ",  // Russian Ident with TypeString prefix
        "DATEStamp",      // English Ident with TypeDate prefix
        "UNDEFINEDValue", // English Ident with LitUndefined prefix
        "ДЕКАДАРАСЧЕТ",   // Russian Ident with PeriodTenDays prefix
        "HALFYEARS",      // English Ident with PeriodHalfYear prefix
    ];
    for src in cases {
        assert_eq!(single_kind(src), Ident, "expected Ident for {src:?}");
    }
}
