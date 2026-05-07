# SDBL Slice 3a — Clean-Room Attestation (primitive types, undefined literal, narrow period vocabulary)

**Status:** complete (2026-05-07).

This document attests the clean-room authorship of the Slice 3a
material of the SDBL lexer, per the staged migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

Slice 3a is the first of two sub-slices carved out of master-doc
Slice 3 (metadata object & type vocabulary). It claims the seven
variants whose canonical SDBL grammar attestation in v8327doc
Глава 8 «Работа с запросами» is unambiguous and direct: the four
primitive type literals (`Булево / Число / Строка / Дата`), the
`Неопределено / UNDEFINED` literal, and the two narrow period-type
keywords that the lexer carries as dedicated tokens
(`Декада / TENDAYS`, `Полугодие / HALFYEAR`). The remaining 14 metadata
object variants (`Mdo*` minus `MdoExternalDataSource`) are claimed by
Slice 3b — a separate clean-room arc — once their per-variant
discrepancy audit has run. The single platform-late variant
`MdoExternalDataSource` is deferred to master-doc Slice 5 (which
already owns external-source handling) per the codex pair-mode
plan-review consensus.

## Status

The Slice 3a attestation flips from "in progress" to "complete" at
phase C3 with an absolute-date stamp at the top of this document.
Until then, the document is a working artefact. The C0b / C1 / C2
landings are atomic but the §Commit trail placeholder for the C3
SHA only resolves on the C3 commit itself; the absolute-last
trailing commit on the branch (the Anti-Hilbert disclosure) is
necessarily not named in the enumeration, mirroring the Slice 1, 2,
6, 7, 8, 9, 10a, 10b, 11, Slice 7-addendum, Slice 8-addendum, and
Slice 2-addendum precedents.

## Scope

The paths claimed as clean-room Slice 3a authorship are:

- `crates/lexer/src/sdbl/mod.rs` — specifically:
  - the file-level docstring bullet added for Slice 3a;
  - the seven variants of `SdblTokenKind` declared under the
    `CLEAN-ROOM Slice 3a — primitive types, undefined literal,
    narrow period vocabulary` banner, with their `#[regex(...)]`
    annotations, their per-variant v8327doc Глава 8 provenance
    docstrings, and the top-of-block thematic convenience index
    (Type / Lit / Period sub-sections). The full Slice 3a list is
    seven variants:
    - **Primitive type literals (4):** `TypeBoolean`, `TypeNumber`,
      `TypeString`, `TypeDate`. Bilingual canonical spellings per
      v8327doc bilingual word-list table:
      `БУЛЕВО / BOOLEAN`, `ЧИСЛО / NUMBER`, `СТРОКА / STRING`,
      `ДАТА / DATE` (`page.html:380, 384, 1230, 1234, 1120, 1124,
      500, 504`).
    - **Undefined literal (1):** `LitUndefined`. Bilingual canonical
      spelling per v8327doc bilingual word-list table:
      `НЕОПРЕДЕЛЕНО / UNDEFINED` (`page.html:890, 894`).
    - **Narrow period-type keywords (2):** `PeriodTenDays`,
      `PeriodHalfYear`. Bilingual canonical spellings per v8327doc
      bilingual word-list table: `ДЕКАДА / TENDAYS`,
      `ПОЛУГОДИЕ / HALFYEAR` (`page.html:520, 524, 980, 984`).
- `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt` — three
  thematic Slice 3a corpus entries (071–073) closing the six
  bilingual blind spots surfaced by the Pre-C0b corpus coverage
  audit. The C0a-reserved slots 074–076 collapsed at C0b — the
  three landed entries close all six audit-confirmed blind spots
  and codex pair-mode C0b review approved the batch unchanged.
- `crates/lexer/tests/sdbl_golden_corpus.rs` — the snapshot
  regenerated against the extended corpus at C0b.
- `crates/lexer/tests/sdbl_slice3a_types.rs` — new spec-driven
  acceptance test file born at C3 with 25 tests: 14 bilingual
  EN+RU canonical-form pins (7 variants × 2 spellings), 1 case-
  insensitivity sweep, 9 structural integration tests (4 CAST
  type-slot, 2 TYPE() expression, 1 LitUndefined / LitNull
  predicate-position asymmetry, 2 TOTALS BY PERIODS period-type
  slot), 1 keyword-prefix Ident longest-match guard. No C2
  regression-gate file was born — the C0a audit found zero
  defects.

The deferred 14 `Mdo*` variants (`MdoCatalog`, `MdoDocument`,
`MdoInformationRegister`, `MdoAccumulationRegister`,
`MdoAccountingRegister`, `MdoCalculationRegister`,
`MdoChartOfAccounts`, `MdoChartOfCalculationTypes`,
`MdoChartOfCharacteristicTypes`, `MdoEnum`, `MdoBusinessProcess`,
`MdoTask`, `MdoConstant`, `MdoSequence`) and the platform-late
`MdoExternalDataSource` remain Tier B material for Slice 3b /
Slice 5 respectively. The `LEGACY` banner in
`crates/lexer/src/sdbl/mod.rs` therefore does not fully close at
Slice 3a landing; the `Fn*` (Slice 4), `Vt*` (Slice 5), `Mdo*`
(Slice 3b), `MdoExternalDataSource` (Slice 5), and `Error` fallback
families remain in LEGACY.

## Per-variant tier source map

All seven Slice 3a variants are classified **Tier A1** with v8327doc
Глава 8 «Работа с запросами» as the primary canonical SDBL grammar
source. The bilingual word-list tables (chapters 8.4.0–8.4.x of the
single document at v8327doc#bookmark:dev:TI000000453, locally
mirrored at `its/dump/its_db_v8327doc_bookmark_dev_TI000000453/page.html`)
attest each Russian / English token spelling at the line numbers
given below. Secondary contextual attestations are included for
robustness; the bilingual word-list rows alone are sufficient for the
Tier A1 claim.

| Variant         | RU canonical    | EN canonical | Word-list `page.html` lines     | Contextual lines              |
|-----------------|-----------------|--------------|---------------------------------|-------------------------------|
| `TypeBoolean`   | `БУЛЕВО`        | `BOOLEAN`    | `:380, :384`                    | `:4736, :4770`                |
| `TypeNumber`    | `ЧИСЛО`         | `NUMBER`     | `:1230, :1234`                  | `:4737, :4770, :4780`         |
| `TypeString`    | `СТРОКА`        | `STRING`     | `:1120, :1124`                  | `:4738, :4771, :4792-4796`    |
| `TypeDate`      | `ДАТА`          | `DATE`       | `:500, :504`                    | `:4739, :4771, :4798-4802`    |
| `LitUndefined`  | `НЕОПРЕДЕЛЕНО`  | `UNDEFINED`  | `:890, :894`                    | `:4785`                       |
| `PeriodTenDays` | `ДЕКАДА`        | `TENDAYS`    | `:520, :524`                    | `:3174, :3274`                |
| `PeriodHalfYear`| `ПОЛУГОДИЕ`     | `HALFYEAR`   | `:980, :984`                    | `:3174, :3275`                |

The contextual lines anchor each variant to a canonical EBNF
production or canonical prose enumeration:

- `Type*` contextual lines `:4730-4740` carry the canonical CAST type
  grammar `Булево | Число [(Длина[, Точность])] | Строка [(Длина)] |
  Дата | <Имя таблицы>` for the `<Тип значения>` non-terminal of
  `ВЫРАЗИТЬ ( <Выражение> КАК <Тип значения> )`. Lines `:4767-4806`
  carry the §«Константы и параметры в языке запросов» prose
  attesting that values of `Булево / Число / Строка / Дата` types
  may be used directly in query expressions, and the `<Значение>`
  EBNF production at `:4774-4787` lists `НЕОПРЕДЕЛЕНО` and `NULL`
  alongside the typed literals.
- `LitUndefined` contextual line `:4785` is the `НЕОПРЕДЕЛЕНО` slot
  in the `<Значение>` EBNF production cited above.
- `Period*` contextual line `:3174` is the canonical TOTALS BY period
  list inside `[ПЕРИОДАМИ(Секунда | Минута | Час | День | Неделя |
  Месяц | Квартал | Год | Декада | Полугодие [...])]`. Lines
  `:3270-3275` carry the matching prose enumeration listing the same
  ten period types verbatim. The `ПЕРИОДАМИ` keyword that introduces
  this period list is owned by `KwPeriods` (Slice 2-addendum scope,
  landed 2026-05-07); see
  [`sdbl-clean-room-slice2-addendum.md`](sdbl-clean-room-slice2-addendum.md)
  § Per-variant tier source map for that token's tier classification.

The `KwType` token (the `ТИП(<Имя типа>)` keyword that introduces
the type-name function call producing values of type `Тип`) is
owned by Slice 2-addendum and gates the user-facing call sites
where `Type*` literals appear as `<Имя типа>` arguments. The
canonical EBNF for the `ТИП(<Имя типа>)` production lives at
`page.html:4831` and the cross-reference is preserved verbatim in
the Slice 3a `Type*` per-variant docstrings authored at C2.

The `LitNull` token (the `NULL` literal) is **not** in Slice 3a
scope — it was claimed clean-room by Slice 2 (landed 2026-04-24)
under the `CLEAN-ROOM Slice 2 — structural keyword vocabulary`
banner. Slice 2's `LitNull` docstring records the converter
mapping `LitNull → TokenKind::Ident` preserved by
`crates/parser/src/sdbl_token_converter.rs`. Slice 3a does not
modify that mapping or that token.

## Behaviour change

**None.** Slice 3a is a pure preserve-shape clean-room re-derivation.
The seven `#[regex(...)]` attribute bodies as written in the
pre-Slice-3a LEGACY block are canonical-spelling equivalent to the
v8327doc bilingual word-list cells under `(?i)` case folding (the
regex literals are lowercase byte-forms; the v8327doc cells render
uppercase for display; the `(?i)` flag is the equivalence bridge):

```
#[regex(r"(?i)булево|(?i)boolean")]      TypeBoolean
#[regex(r"(?i)число|(?i)number")]        TypeNumber
#[regex(r"(?i)строка|(?i)string")]       TypeString
#[regex(r"(?i)дата|(?i)date")]           TypeDate
#[regex(r"(?i)неопределено|(?i)undefined")] LitUndefined
#[regex(r"(?i)декада|(?i)tendays")]      PeriodTenDays
#[regex(r"(?i)полугодие|(?i)halfyear")]  PeriodHalfYear
```

Each Russian alternation literal is the lower-case byte-form of the
v8327doc word-list `Term` cell (which is rendered upper-case for
display); the `(?i)` flag is the natural expression of the v8327doc
prose convention "идентификаторы языка запросов нечувствительны к
регистру". Each English alternation literal is the lower-case
byte-form of the v8327doc word-list right-hand cell. There are no
regex defects, no canonical-spelling mismatches, no
nominative-vs-instrumental case errors of the kind found in
Slice 2-addendum's KwPeriods (which the audit caught and
Option-A-fixed at C2 of that addendum). The full audit trail is
documented in § C0a discrepancy audit below.

The token converter at `crates/parser/src/sdbl_token_converter.rs`
maps `TypeBoolean / TypeNumber / TypeString / TypeDate` and
`LitUndefined` and `PeriodTenDays / PeriodHalfYear` onto
`TokenKind::Ident` so the parser grammar disambiguates them by text
in the relevant slots (CAST `<Тип значения>`, `<Значение>`,
TOTALS BY ПЕРИОДАМИ list). This converter mapping is preserved
unchanged by Slice 3a.

The lexer-level disambiguation between `Period*` tokens and `Fn*`
tokens that "double as period types in TOTALS BY ... PERIODS(...)"
(`FnYear`, `FnQuarter`, `FnMonth`, `FnDay`, `FnWeek`, `FnHour`,
`FnMinute`, `FnSecond`) is **not** in Slice 3a scope. Those
ambiguous-shaped tokens are explicitly Fn* (function-side) and stay
in LEGACY pending Slice 4. The `priority = 2` attribute on those
Fn* declarations is preserved verbatim through Slice 3a.

## C0a discrepancy audit

This section is named explicitly per the codex pair-mode plan-review
consensus: the Slice 2-addendum KwPeriods regex defect (`ПЕРИОДЫ`
vs canonical `ПЕРИОДАМИ`) was found only at C2 because no audit
step was run before regex rewrite. For Slice 3a, the audit runs at
C0a — before any `#[regex]` body is touched — so any defect would
be surfaced before C2 commits to a rewrite path.

### Audit method

For each of the seven Slice 3a variants:

1. Read the current `#[regex(...)]` attribute body in
   `crates/lexer/src/sdbl/mod.rs` lines 881–902 verbatim.
2. Locate the v8327doc bilingual word-list cell for the Russian
   token spelling (the left-hand `Term` cell of the relevant
   word-list row).
3. Locate the v8327doc bilingual word-list cell for the English
   token spelling (the right-hand `Term` cell of the same row).
4. Locate at least one contextual canonical EBNF or canonical prose
   line confirming the spelling matches its grammar role.
5. Compare regex byte-strings against word-list cells under
   `(?i)` case folding.

### Audit results

All seven variants pass the audit with zero defects. Each row of
the per-variant tier source map table above is the audit record:
the regex byte-string matches the v8327doc word-list canonical
spelling exactly under `(?i)` case folding, and the contextual EBNF
/ prose lines confirm the variant role. The audit log:

- `TypeBoolean` — regex `булево|boolean`; word-list `БУЛЕВО / BOOLEAN`
  (`:380, :384`); contextual CAST type slot `Булево` (`:4736`) and
  constants prose `типа Булево` (`:4770`). **MATCH.**
- `TypeNumber` — regex `число|number`; word-list `ЧИСЛО / NUMBER`
  (`:1230, :1234`); contextual CAST type slot `Число [(Длина[,
  Точность])]` (`:4737`), constants prose `типа Число` (`:4770`),
  `<Литерал типа Число>` EBNF anchor (`:4780`). **MATCH.**
- `TypeString` — regex `строка|string`; word-list `СТРОКА / STRING`
  (`:1120, :1124`); contextual CAST type slot `Строка [(Длина)]`
  (`:4738`), constants prose `типа Строка` (`:4771`),
  `<Литерал типа Строка>` EBNF (`:4792-4796`). **MATCH.**
- `TypeDate` — regex `дата|date`; word-list `ДАТА / DATE`
  (`:500, :504`); contextual CAST type slot `Дата` (`:4739`),
  constants prose `типа Дата` (`:4771`), `<Литерал типа Дата>` EBNF
  using `ДАТАВРЕМЯ(...)` constructor (`:4798-4802`). **MATCH.**
- `LitUndefined` — regex `неопределено|undefined`; word-list
  `НЕОПРЕДЕЛЕНО / UNDEFINED` (`:890, :894`); contextual `<Значение>`
  EBNF slot (`:4785`). **MATCH.**
- `PeriodTenDays` — regex `декада|tendays`; word-list
  `ДЕКАДА / TENDAYS` (`:520, :524`); contextual TOTALS BY period
  list `Декада` (`:3174`), prose enumeration `Декада` (`:3274`).
  **MATCH.**
- `PeriodHalfYear` — regex `полугодие|halfyear`; word-list
  `ПОЛУГОДИЕ / HALFYEAR` (`:980, :984`); contextual TOTALS BY
  period list `Полугодие` (`:3174`), prose enumeration `Полугодие`
  (`:3275`). **MATCH.**

### Audit conclusion

Slice 3a carries **no behaviour change**. C2 is therefore a pure
provenance-rewrite phase (per-variant docstring authorship plus the
thematic convenience index); no regex bodies are flipped at C2, no
golden corpus byte-strings are flipped at C2, no parser-tree-shape
invariants need re-asserting. This is the intentional design
trade-off of the C0a-first audit: the audit cost is paid up front
in exchange for a cleaner C2 commit and a smaller blast radius.

## Pre-existing parser-side stale-classification follow-up

The Slice 2-addendum attestation §Pre-existing parser-side
stale-classification follow-up
([`sdbl-clean-room-slice2-addendum.md`](sdbl-clean-room-slice2-addendum.md)
§ Pre-existing parser-side stale-classification follow-up) records a
known out-of-scope follow-up: parser-side rustdoc Tier-D
classifications for FOR UPDATE / INDEX BY at
`crates/parser/src/grammar/sdbl/select.rs:1292-1297, 1349-1352` and
`docs/legal/sdbl-select-mini-spec.md:759-789` are stale (they
predate v8327doc landing in Slice 7-addendum 2026-04-26, which now
Tier-A1-attests both clauses). That follow-up belongs to a separate
parser-only commit and is **not** picked up by Slice 3a. Slice 3a is
a lexer-only clean-room, identically-shaped to Slice 2-addendum, and
has the same out-of-scope boundary.

## Sources consulted

The Slice 3a material was re-derived from:

1. v8.3.27 Developer's Reference Глава 8 «Работа с запросами»:
   - <https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453> —
     the single-document URL covering the full chapter; locally
     mirrored at
     `its/dump/its_db_v8327doc_bookmark_dev_TI000000453/page.html`
     for line-numbered reviewer convenience.
   - Specific cited regions:
     - Bilingual word-list table rows for the seven variants
       (`page.html:380, 384, 500, 504, 520, 524, 890, 894, 980, 984,
       1120, 1124, 1230, 1234`).
     - §«Приведение типа» (CAST type grammar, `:4720-4765`) — the
       `<Тип значения>` non-terminal listing the four primitive
       types and `<Имя таблицы>`.
     - §«Константы и параметры в языке запросов» (`:4767-4825`) —
       the `<Значение>` EBNF production listing all literal forms
       including `НЕОПРЕДЕЛЕНО` and the `<Литерал типа Тип>` cross-
       reference.
     - §«Расчет общих итогов» / §«Дополнение дат»
       (`:3170-3290`) — the `[ПЕРИОДАМИ(Секунда | Минута | Час |
       День | Неделя | Месяц | Квартал | Год | Декада | Полугодие
       [...])]` canonical period-list grammar plus matching prose
       enumeration.
2. 1C ITS pubqlang documentation (secondary, corroborating):
   - <https://its.1c.ru/db/pubqlang/content/12/hdoc> — lexical
     elements, identifier longest-match rule, and literal vocabulary
     (already cited by Slice 2 for `LitTrue / LitFalse / LitNull`;
     re-cited here for `LitUndefined` corroboration).
   - <https://its.1c.ru/db/pubqlang/content/40/hdoc> — type
     literals and CAST examples (corroborating Type* citations).
   - <https://its.1c.ru/db/pubqlang/content/39/hdoc> — TOTALS BY
     period grammar (corroborating Period* citations).
3. The Slice 1, Slice 2, and Slice 2-addendum clean-room material
   already present in `crates/lexer/src/sdbl/mod.rs` — consulted
   only for the existing `Ident` regex priority shape, the
   per-variant docstring format, and the convenience-index style.

Per the citation policy adopted in Slice 8-addendum and reaffirmed
in Slice 2-addendum (cite the public ITS URL plus pubqlang chapter
identifier in committed artefacts; local mirror paths are working
convenience only), Slice 3a source-code provenance comments at C2
and Slice 3a commit messages cite ONLY the public ITS URL plus
pubqlang chapter identifiers — no local mirror paths. The
local-mirror line-number references inside **this attestation
document** (`page.html:NNN`) are an explicit exemption: legal
attestation documents are reviewer-facing working artefacts, not
source-code rustdoc text, and the line numbers here let a reviewer
verify the audit trail without re-deriving citations themselves.
The exemption applies to attestation documents only; source-code
rustdoc text authored at C2 propagates the public-URL form of each
citation, never the `page.html:NNN` form.

## Non-consultation statement

During the authorship of the Slice 3a material the following sources
were not used as working text:

- the sibling `../bsl-parser` project — neither its grammar files
  nor its token inventory were consulted;
- the pre-clean-room regex text of the Slice 3a variants themselves
  beyond reading them once at C0a for the discrepancy audit;
- any other third-party SDBL grammar, token inventory, or parser.

The byte-identity golden corpus
(`crates/lexer/tests/sdbl_golden_corpus.rs`) is the verification gate
that the re-derived (in this slice's case, byte-identical) regex
patterns accept exactly the same text set as the pre-refactor
implementation; the corpus is extended at C0b to close the six
bilingual blind spots surfaced by the Pre-C0b corpus coverage audit
below before any banner relocation.

## Verification recipe

All of the following must be green before this attestation is
considered live. Pre-Slice-3a baseline test counts (post-Slice
2-addendum, develop branch as of 2026-05-07) are pinned to detect
silent test regressions:

1. `cargo test -p lexer --test sdbl_slice1_core` — **34 passed**
   pre-Slice-3a; expected 34 passed post-Slice-3a (unchanged; Slice
   1 is closed).
2. `cargo test -p lexer --test sdbl_slice2_keywords` — **43 passed**
   pre-Slice-3a; expected 43 passed post-Slice-3a (unchanged; Slice
   2 / Slice 2-addendum are closed).
3. `cargo test -p lexer --test sdbl_slice2_addendum_clause_keywords` —
   **30 passed** pre-Slice-3a; expected 30 passed post-Slice-3a
   (unchanged; Slice 2-addendum is closed).
4. `cargo test -p lexer --test sdbl_golden_corpus` — **1 passed**
   pre-Slice-3a (single snapshot test); expected 1 passed
   post-Slice-3a after the C0b snapshot regeneration captures the six
   new corpus entries.
5. `cargo test -p lexer --test sdbl_slice3a_types` — file does
   **not exist** pre-Slice-3a; **25 passed** post-Slice-3a (the
   spec-driven acceptance suite born at C3 — 14 bilingual EN+RU
   canonical-form pins, 1 case-insensitivity sweep, 9 structural
   integration tests, 1 keyword-prefix Ident longest-match guard).
6. `cargo test -p lexer --tests` — **173 passed** pre-Slice-3a;
   **198 passed** post-Slice-3a (173 + 25, matching item 5).
7. `cargo test -p parser --test sdbl_parser_tests` — pre-Slice-3a
   baseline unchanged because Slice 3a does not edit the converter
   or any parser-side files; the parser-side test count is
   parser-owned and not pinned in this lexer-only attestation.
8. `cargo build --workspace --all-targets` — workspace build.
9. `cargo clippy -p lexer --all-targets --all-features -- -D warnings`
   — lexer clippy with warnings denied.

## Commit trail

- `51f17fff` (2026-05-07) — C0a: this attestation document
  authored. Sole change: addition of `docs/legal/sdbl-clean-room-slice3a.md`.
- `f6fcdc2e` (2026-05-07) — C0b: extend
  `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt` with three
  thematic corpus entries (071–073) closing all six bilingual
  blind spots surfaced by the Pre-C0b corpus coverage audit;
  regenerate `crates/lexer/tests/sdbl_golden_corpus.rs` snapshot
  via `UPDATE_EXPECT=1`. The C0a-reserved slots 074–076 collapsed
  unused after codex C0b review approval. The byte-identity
  golden corpus gate confirms the audit's PRESERVE-shape
  conclusion: each newly-covered form tokenises to its expected
  `SdblTokenKind` without `Error` fallback or `Ident` shadow.
- `297b529f` (2026-05-07) — C1: relocate the seven `#[regex]`
  declarations out of the LEGACY block in
  `crates/lexer/src/sdbl/mod.rs` into the new
  `CLEAN-ROOM Slice 3a — primitive types, undefined literal,
  narrow period vocabulary` banner. Pure refactor — regex bodies
  move byte-for-byte, only banner header / placeholder provenance
  markers / file-level docstring change. LEGACY banner header
  tightened to `LEGACY (Slices 3b, 4, 5 pending —
  Mdo*/function/virtual-table vocabularies + ExternalDataSource)`.
- `077ae770` (2026-05-07) — C2: replace the C1 placeholder
  markers with full per-variant v8327doc Глава 8 provenance
  docstrings (one rustdoc block per variant citing word-list pair
  and contextual EBNF / prose anchor); add the thematic
  convenience index (Type / Lit / Period sub-sections) at the top
  of the banner; cross-reference `KwType` for `Type*` variants
  per the codex STRONG #5 finding on the C0a plan critique. No
  regex body changes — the C0a audit found zero defects. Codex
  pair-mode C2 review caught one BLOCKER (LitUndefined converter
  mapping claim) addressed inline before commit: the
  `LitNull → Ident` / `LitUndefined → KwUndefined` asymmetry per
  `crates/parser/src/sdbl_token_converter.rs:57,196` is now
  recorded explicitly in the LitUndefined docstring.
- `cd31ce71` (2026-05-07) — C3: this attestation flips from
  "in progress" to "complete (2026-05-07)"; the new
  `crates/lexer/tests/sdbl_slice3a_types.rs` acceptance suite
  (25 tests) is born; the master-doc `sdbl-clean-room-slices.md`
  carries a new `## Slice 3a` section recording the Slice 3a
  sub-slice landing; `crates/lexer/src/sdbl/mod.rs` file-level
  docstring 4th bullet flips from "(in progress)" to "(complete,
  2026-05-07)" with attestation citation; the in-enum banner
  header `CLEAN-ROOM Slice 3a — ... (in progress)` flips to
  `(complete, 2026-05-07)`.
- Anti-Hilbert close-out: a single trailing fixup commit pins the
  C3 SHA `cd31ce71` explicitly in the trail above and fixes any
  drift in subsidiary doc/test header counts surfaced after C3
  lands; mirrors the Slice 2-addendum `c54aa4a7` close-out shape.
  The Anti-Hilbert disclosure: this trailing commit's own SHA is
  NOT named in this enumeration — it cannot be, by construction.
  The Slice 1, 2, 6, 7, 8, 9, 10a, 10b, 11, Slice 7-addendum,
  Slice 8-addendum, and Slice 2-addendum attestations all share
  this pattern.

## Pre-C0b corpus coverage audit

Before C0b extends the corpus, the audit identifies the seven
Slice 3a variants' coverage in the pre-Slice-3a corpus
(`crates/lexer/tests/fixtures/sdbl_golden_corpus.txt`):

| Variant         | RU canonical    | RU in corpus?                                   | EN canonical | EN in corpus?                                |
|-----------------|-----------------|-------------------------------------------------|--------------|----------------------------------------------|
| `TypeBoolean`   | `БУЛЕВО`        | ❌ MISSING                                      | `BOOLEAN`    | ✓ entry 049 (`TYPE(Boolean)`)                |
| `TypeNumber`    | `ЧИСЛО`         | ✓ entries 035 (`NUMBER` lex-equivalent), 065 (`ЧИСЛО`), 066 (`Число`) | `NUMBER` | ✓ entry 035 (`AS NUMBER`), 049 (`TYPE(Number)`) |
| `TypeString`    | `СТРОКА`        | ❌ MISSING                                      | `STRING`     | ✓ entry 049 (`TYPE(String)`)                 |
| `TypeDate`      | `ДАТА`          | ❌ MISSING                                      | `DATE`       | ✓ entry 049 (`TYPE(Date)`)                   |
| `LitUndefined`  | `НЕОПРЕДЕЛЕНО`  | ✓ entry 049 (`Неопределено`)                    | `UNDEFINED`  | ❌ MISSING                                   |
| `PeriodTenDays` | `ДЕКАДА`        | ✓ entry 050 (`ДЕКАДА`)                          | `TENDAYS`    | ❌ MISSING                                   |
| `PeriodHalfYear`| `ПОЛУГОДИЕ`     | ✓ entry 050 (`ПОЛУГОДИЕ`)                       | `HALFYEAR`   | ❌ MISSING                                   |

Six bilingual blind spots (one per row marked ❌) required corpus
gap-fill at C0b. The C0b commit (`f6fcdc2e`) added three thematic
entries (071–073) closing all six blind spots:

- **071** Slice-3a primitive types russian — covers `БУЛЕВО`,
  `СТРОКА`, `ДАТА` in CAST type-slot context (`ВЫРАЗИТЬ(... КАК
  ...)`).
- **072** Slice-3a undefined literal english — covers `UNDEFINED`
  in `<Значение>` predicate position.
- **073** Slice-3a period types english — covers `TENDAYS`,
  `HALFYEAR` in TOTALS BY period-list context.

The C0a-reserved slots 074–076 collapsed unused — the codex C0b
review approved the 071–073 batch unchanged and the three landed
entries close all six audit-confirmed blind spots. The final
landed range is **071–073**.

## Licensing note

The `crates/lexer` crate retains its `LGPL-3.0-or-later` license
until the full Slice 1 → Slice 5 migration is complete. Slice 3a
does not promote the crate to Tier A (`MIT OR Apache-2.0`); that
promotion happens once the last legacy variant has been re-derived
(`Mdo*` minus `MdoExternalDataSource` for Slice 3b, `Fn*` for
Slice 4, `Vt*` plus `MdoExternalDataSource` for Slice 5, and the
`Error` fallback as the closing step).

## Author attestation

The Slice 3a material listed above under **Scope** was authored as a
clean-room re-derivation from the sources listed under **Sources
consulted**, without using the `../bsl-parser` project, the
pre-clean-room regex text of the Slice 3a variants beyond a single
read-once for the C0a discrepancy audit, or any other third-party
SDBL grammar as working text. This attestation applies at the date
recorded at the top of the document.
