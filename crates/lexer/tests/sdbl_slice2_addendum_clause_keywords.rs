//! Clean-room acceptance tests for the Slice 2-addendum clause
//! keyword leftovers vocabulary of `SdblTokenKind`.
//!
//! Status: file born at C2 with the 3 KwPeriods regression-gate
//! tests landing alongside the regex defect fix per
//! `docs/legal/sdbl-clean-room-slice2-addendum.md` § Behaviour
//! change Option A. C3 will expand this file into the full
//! spec-driven acceptance suite covering all 17 Slice 2-addendum
//! variants in both EN and RU spellings.
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
