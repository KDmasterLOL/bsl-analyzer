# SDBL Slice 3b — Clean-Room Attestation (metadata-object table vocabulary + `Error` fallback)

**Status:** in progress (C0a landed 2026-07-27).

This document attests the clean-room authorship of the Slice 3b material
of the SDBL lexer, per the staged migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

Slice 3b is the second of two sub-slices carved out of master-doc Slice 3
(metadata object & type vocabulary). Slice 3a
([`sdbl-clean-room-slice3a.md`](sdbl-clean-room-slice3a.md), complete
2026-05-07) claimed the seven primitive-type / undefined-literal /
narrow-period variants. Slice 3b claims the metadata-object table-root
vocabulary: the 14 `Mdo*` variants that existed before this slice, plus
four table roots that the audit below found missing from the lexer
entirely.

Slice 3b additionally claims the `Error` fallback variant, which the
Slice 3a attestation names as the closing step of the lexer work and
which is still the line introduced by the repository's first SDBL commit
`2ccf30bc`. It is not a vocabulary entry and gets its own section below.

## Status

The Slice 3b attestation flips from "in progress" to "complete" at phase
C3 with an absolute-date stamp at the top of this document. Until then,
the document is a working artefact. The C0b / C1 / C2 landings are
atomic but the §Commit trail placeholder for the C3 SHA only resolves on
the C3 commit itself; the absolute-last trailing commit on the branch
(the Anti-Hilbert disclosure) is necessarily not named in the
enumeration, mirroring the Slice 1, 2, 6, 7, 8, 9, 10a, 10b, 11,
Slice 7-addendum, Slice 8-addendum, Slice 2-addendum and Slice 3a
precedents.

## Scope

The paths claimed as clean-room Slice 3b authorship are:

- `crates/lexer/src/sdbl/mod.rs` — specifically:
  - the file-level docstring bullet added for Slice 3b;
  - the 18 metadata-object table-root variants of `SdblTokenKind`
    declared under the `CLEAN-ROOM Slice 3b — metadata-object table
    vocabulary` banner, with their `#[regex(...)]` annotations, their
    per-variant provenance docstrings, and the top-of-block thematic
    convenience index;
  - the `Error` fallback variant declared under its own
    `CLEAN-ROOM Slice 3b — lexer error fallback` banner;
  - the restored `LEGACY` banner over the still-unrewritten
    vocabularies (see § Marker restoration).
- `crates/parser/src/sdbl_token_converter.rs` — the four new variants
  appended to the existing shared `Mdo* => T::Ident` match arm. This is
  the one non-lexer file Slice 3b touches; see § Behaviour change for
  why the edit is parser-tree-invariant and why it cannot be deferred.
- `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt` — thematic
  Slice 3b corpus entries closing the 19 bilingual blind spots surfaced
  by the Pre-C0b corpus coverage audit below.
- `crates/lexer/tests/sdbl_golden_corpus.rs` — the snapshot regenerated
  against the extended corpus at C0b and again at C2 when the four new
  variants start emitting.
- `crates/lexer/tests/sdbl_slice3b_metadata_objects.rs` — the
  spec-driven acceptance test file born at C3.

### The 18 claimed table-root variants

Fourteen were already present in the lexer:

`MdoCatalog`, `MdoDocument`, `MdoInformationRegister`,
`MdoAccumulationRegister`, `MdoAccountingRegister`,
`MdoCalculationRegister`, `MdoChartOfAccounts`,
`MdoChartOfCalculationTypes`, `MdoChartOfCharacteristicTypes`,
`MdoEnum`, `MdoBusinessProcess`, `MdoTask`, `MdoConstant`,
`MdoSequence`.

Four are born in this slice as the C0a audit's Option A ADD outcome:

`MdoDocumentJournal`, `MdoExchangePlan`, `MdoFilterCriterion`,
`MdoConstants`.

This is a deliberate widening of the scope recorded for Slice 3b in the
master document, which named 14 variants. The master document counted
what the lexer had; the audit counted what the source attests. See
§ Coverage gap for the reasoning and § Behaviour change for the cost.

### Out of scope

- `MdoExternalDataSource` — the platform-late external-data-source root
  stays deferred to master-doc Slice 5, which already owns
  external-source handling. Slice 3b does not move, re-regex or
  re-document it; it stays in the `LEGACY` block.
- `Fn*` (42 variants) — master-doc Slice 4.
- `Vt*` (6 variants) — master-doc Slice 5.
- `LBrace` / `RBrace` — unattested and unowned; see § Unowned brace
  tokens. Slice 3b records them on the map but does not claim them.
- Parser-side grammar, `sdbl-hir`, and the parser-side rustdoc Tier
  classifications. The converter arm named under § Scope is the sole
  parser-crate edit and carries no grammar decision.

## Per-variant tier source map

All 18 variants are classified **Tier A1** with the 1C:Enterprise 8.3.27
syntax assistant section «Работа с запросами → Таблицы запросов» as the
primary canonical source. Every article in that section carries its
headline in the bilingual form `<Русское имя> (<English name>)`, which
attests both spellings of a table root in a single canonical line.

| Variant | RU canonical | EN canonical | Syntax-assistant article headline |
|---|---|---|---|
| `MdoCatalog` | `Справочник` | `Catalog` | `Справочник.<Имя справочника> (Catalog.<Имя справочника>)` |
| `MdoDocument` | `Документ` | `Document` | `Документ.<Имя документа> (Document.<Имя документа>)` |
| `MdoDocumentJournal` | `ЖурналДокументов` | `DocumentJournal` | `ЖурналДокументов.<Имя журнала документов> (DocumentJournal.<Имя журнала документов>)` |
| `MdoConstants` | `Константы` | `Constants` | `Константы (Constants)` |
| `MdoConstant` | `Константа` | `Constant` | `Константа.<Имя константы> (Constant.<Const name>)` |
| `MdoChartOfCharacteristicTypes` | `ПланВидовХарактеристик` | `ChartOfCharacteristicTypes` | `ПланВидовХарактеристик.<Имя плана видов характеристик> (ChartOfCharacteristicTypes.<Имя плана видов характеристик>)` |
| `MdoChartOfAccounts` | `ПланСчетов` | `ChartOfAccounts` | `ПланСчетов.<Имя плана счетов> (ChartOfAccounts.<Имя плана счетов>)` |
| `MdoChartOfCalculationTypes` | `ПланВидовРасчета` | `ChartOfCalculationTypes` | `ПланВидовРасчета.<Имя плана видов расчета> (ChartOfCalculationTypes.<Имя плана видов расчета>)` |
| `MdoFilterCriterion` | `КритерийОтбора` | `FilterCriterion` | `КритерийОтбора.<Имя критерия отбора> (FilterCriterion.<Имя критерия отбора>)` |
| `MdoExchangePlan` | `ПланОбмена` | `ExchangePlan` | `ПланОбмена.<Имя плана обмена> (ExchangePlan.<Имя плана обмена>)` |
| `MdoEnum` | `Перечисление` | `Enum` | `Перечисление.<Имя перечисления> (Enum.<Имя перечисления>)` |
| `MdoBusinessProcess` | `БизнесПроцесс` | `BusinessProcess` | `БизнесПроцесс.<Имя бизнес-процесса> (BusinessProcess.<Имя бизнес-процесса>)` |
| `MdoTask` | `Задача` | `Task` | `Задача.<Имя задачи> (Task.<Имя задачи>)` |
| `MdoSequence` | `Последовательность` | `Sequence` | `Последовательность.<Имя последовательности> (Sequence.<Имя последовательности>)` |
| `MdoInformationRegister` | `РегистрСведений` | `InformationRegister` | `РегистрСведений.<Имя регистра сведений> (InformationRegister.<Имя регистра сведений>)` |
| `MdoAccumulationRegister` | `РегистрНакопления` | `AccumulationRegister` | `РегистрНакопления.<Имя регистра накопления> (AccumulationRegister.<Имя регистра накопления>)` |
| `MdoAccountingRegister` | `РегистрБухгалтерии` | `AccountingRegister` | `РегистрБухгалтерии.<Имя регистра бухгалтерии> (AccountingRegister.<Имя регистра бухгалтерии>)` |
| `MdoCalculationRegister` | `РегистрРасчета` | `CalculationRegister` | `РегистрРасчета.<Имя регистра расчета> (CalculationRegister.<Имя регистра расчета>)` |

Five of the 18 carry an independent second attestation from the
Developer's Reference: v8.3.27 Глава 8 «Работа с запросами» §8.4.4
«Использование предопределенных данных конфигурации» enumerates the
types admissible in the `ЗНАЧЕНИЕ(...)` literal in the same bilingual
form — `Справочник (Catalog)`, `ПланВидовХарактеристик
(ChartOfCharacteristicTypes)`, `ПланСчетов (ChartOfAccounts)`,
`ПланВидовРасчета (ChartOfCalculationTypes)`, `Перечисление (Enum)`.

Russian spellings for the register family and for `Справочник` /
`Документ` / `Перечисление` are additionally attested by canonical
query examples throughout the ITS pubqlang textbook and Глава 8 (for
example `ИЗ Справочник.Номенклатура`, `Документ.РасходнаяНакладная`,
`РегистрСведений.Цены`).

The bilingualism of table names in general — the property that makes a
bilingual regex the right shape for these tokens — is stated in
Глава 8 §8.2: «Имя таблицы может быть задано на английском и русском
языках».

## C0a discrepancy audit

This section runs before any `#[regex]` body is touched, per the audit
discipline established in Slice 3a (which in turn was a response to the
Slice 2-addendum `KwPeriods` defect that surfaced only at C2).

### Audit method

For each of the 14 pre-existing `Mdo*` variants:

1. Read the current `#[regex(...)]` attribute body verbatim.
2. Locate the syntax-assistant «Таблицы запросов» article whose headline
   names that table root, and read both halves of its bilingual headline.
3. Compare the regex alternation byte-strings against the headline
   spellings under `(?i)` case folding.
4. Where a second independent attestation exists (Глава 8 §8.4.4, or a
   canonical query example), record it.

Then, in the opposite direction: enumerate every distinct table root in
the «Таблицы запросов» section and check that the lexer has a variant
for it. This second direction is what Slice 3a's audit did not need to
run (its seven variants came from a closed word-list) and it is where
this slice's only findings are.

### Audit results — spelling (14/14 MATCH)

No defects. Every regex alternation is the lower-case byte-form of its
canonical headline spelling; the `(?i)` flag is the equivalence bridge,
and is the natural expression of the documented rule «Регистр букв
(строчные или заглавные) при написании не имеет значения» (Глава 8,
note under the bilingual word list).

- `MdoCatalog` — regex `справочник|catalog`; headline `Справочник (Catalog)`;
  second attestation Глава 8 §8.4.4. **MATCH.**
- `MdoDocument` — regex `документ|document`; headline `Документ (Document)`.
  **MATCH.**
- `MdoInformationRegister` — regex `регистрсведений|informationregister`;
  headline `РегистрСведений (InformationRegister)`. **MATCH.**
- `MdoAccumulationRegister` — regex
  `регистрнакопления|accumulationregister`; headline `РегистрНакопления
  (AccumulationRegister)`. **MATCH.**
- `MdoAccountingRegister` — regex `регистрбухгалтерии|accountingregister`;
  headline `РегистрБухгалтерии (AccountingRegister)`. **MATCH.**
- `MdoCalculationRegister` — regex `регистррасчета|calculationregister`;
  headline `РегистрРасчета (CalculationRegister)`. **MATCH.**
- `MdoChartOfAccounts` — regex `плансчетов|chartofaccounts`; headline
  `ПланСчетов (ChartOfAccounts)`; second attestation Глава 8 §8.4.4.
  **MATCH.**
- `MdoChartOfCalculationTypes` — regex
  `планвидоврасчета|chartofcalculationtypes`; headline `ПланВидовРасчета
  (ChartOfCalculationTypes)`; second attestation Глава 8 §8.4.4.
  **MATCH.** Note the canonical Russian spelling is singular-genitive
  `Расчета`, not `Расчетов`; the regex is correct.
- `MdoChartOfCharacteristicTypes` — regex
  `планвидовхарактеристик|chartofcharacteristictypes`; headline
  `ПланВидовХарактеристик (ChartOfCharacteristicTypes)`; second
  attestation Глава 8 §8.4.4. **MATCH.**
- `MdoEnum` — regex `перечисление|enum`; headline `Перечисление (Enum)`;
  second attestation Глава 8 §8.4.4. **MATCH.**
- `MdoBusinessProcess` — regex `бизнеспроцесс|businessprocess`; headline
  `БизнесПроцесс (BusinessProcess)`; second attestation Глава 8 §8.4.4
  route-point example `БизнесПроцесс.<Имя>.ТочкаМаршрута.<Имя>`.
  **MATCH.**
- `MdoTask` — regex `задача|task`; headline `Задача (Task)`. **MATCH.**
- `MdoConstant` — regex `константа|constant`; headline `Константа.<Имя
  константы> (Constant.<Const name>)`. **MATCH** for the per-constant
  table root. See the coverage gap below for the sibling aggregate table.
- `MdoSequence` — regex `последовательность|sequence`; headline
  `Последовательность (Sequence)`. **MATCH.**

### Audit results — coverage (4 gaps)

The «Таблицы запросов» section defines 19 distinct table roots. The
lexer carries 15 of them (the 14 above plus `MdoExternalDataSource`,
which belongs to Slice 5). Four roots have no lexer variant at all and
therefore lex as bare `Ident`:

| Missing root | RU | EN | Article headline |
|---|---|---|---|
| document journal | `ЖурналДокументов` | `DocumentJournal` | `ЖурналДокументов.<Имя журнала документов> (DocumentJournal.<Имя журнала документов>)` |
| exchange plan | `ПланОбмена` | `ExchangePlan` | `ПланОбмена.<Имя плана обмена> (ExchangePlan.<Имя плана обмена>)` |
| filter criterion | `КритерийОтбора` | `FilterCriterion` | `КритерийОтбора.<Имя критерия отбора> (FilterCriterion.<Имя критерия отбора>)` |
| constants (aggregate) | `Константы` | `Constants` | `Константы (Constants)` |

The fourth is the one most easily missed by inspection: the platform has
**two** constant-related query tables. `Константа.<Имя константы>`
(present as `MdoConstant`) exposes a single named constant, while
`Константы` — no dot, no name part — is the aggregate table holding all
constants. The two spellings differ by one letter and the lexer only had
the first.

These four are true false-negatives of the vocabulary, not stylistic
gaps: each is a legal query source root today, and each currently
tokenises as an ordinary identifier.

### Audit conclusion

The pre-existing 14 regex bodies are correct and are preserved
byte-for-byte through C1 and C2. The slice's only behaviour change is
the addition of the four missing roots (Option A ADD, per the repository
owner's decision recorded 2026-07-27), which makes the metadata-object
vocabulary complete with respect to its canonical source.

## Behaviour change

**Additive, parser-tree-invariant.**

Four new `SdblTokenKind` variants change what the lexer emits for four
words that previously emitted `Ident`. Nothing downstream observes the
difference, for two reasons:

1. `SdblTokenKind` has exactly two consumers in the workspace — the
   `lexer` crate itself and `crates/parser/src/sdbl_token_converter.rs`.
   No other crate names the type.
2. The converter maps every `Mdo*` variant to `TokenKind::Ident` through
   a single shared match arm. Appending the four new variants to that arm
   reproduces, token for token, what the parser saw when these words were
   lexed as `Ident`.

The converter edit is mandatory and cannot be deferred to a later slice:
the match is exhaustive over `SdblTokenKind`, so adding a variant without
extending the arm does not compile. It is a mechanical edit with no
grammar decision in it, which is why Slice 3b is not a strictly
lexer-only slice the way Slices 2-addendum and 3a were.

Longest-match interaction was checked for each new alternation:

- `documentjournal` (15 bytes) vs `document` (8 bytes) — logos prefers
  the longer match, so `DocumentJournal` cannot be shadowed by
  `MdoDocument`; and against `Ident`, which matches the same 15 bytes,
  the explicit `priority = 1` on `Ident` yields to the keyword rule.
  This is the same tie-break Slice 1 established for the whole keyword
  vocabulary.
- `константы` vs `константа` — the two alternations diverge on their
  final character, so neither can match the other's input.
- `планобмена` vs `плансчетов` / `планвидоврасчета` /
  `планвидовхарактеристик` — all four diverge at the fifth character.
- `критерийотбора` — no other alternation in the enum shares its prefix.

The golden corpus is the gate: C0b lands corpus entries for all four
words while they still tokenise as `Ident`, so the snapshot records the
pre-change behaviour; C2 flips those snapshot lines to the new kinds.
The flip is the audit trail of the behaviour change, in the same shape
as the Slice 2-addendum `KwPeriods` corpus flip.

## The `Error` fallback variant

`SdblTokenKind::Error` is claimed by this slice as the closing step of
the lexer clean-room, per the Slice 3a attestation's Licensing note.

It is not vocabulary. It carries no `#[token]` or `#[regex]` attribute
and matches no input by itself. It exists because `tokenize_sdbl`
substitutes it for logos' `Err` result at
`crates/lexer/src/sdbl/mod.rs` — when no rule matches at the current
position, the offending byte range is still emitted as a token so that
the token stream covers the whole input.

**Tier D — local IDE-recovery contract.** No 1C source defines an error
token, and none could: the documented query language describes what is
accepted, not how a tool should represent what is not. The requirement
is the project's own, and it is the same requirement that produced the
parser-side recovery allowances attested in Slices 9, 10a and 11: an
editor must be able to point at the offending span, which means the span
must survive tokenisation rather than be dropped.

Its downstream mapping is `Error => TokenKind::Error` in the converter,
alongside `Quote => TokenKind::Error` for the unreachable quote case.
Slice 3b does not modify either mapping.

C2 gives the variant a provenance docstring stating the above; the
declaration itself does not change, because there is nothing in it to
re-derive. The C3 acceptance suite pins the contract that unmatched
input produces `Error` rather than being silently dropped or absorbed
into a neighbouring token.

## Marker restoration

`3aa29b99` (2026-05-29, "refactor: prune project comments") removed every
`CLEAN-ROOM` and `LEGACY` banner from `crates/lexer/src/sdbl/mod.rs`,
taking the map of the remaining unrewritten surface with them. The
July 2026 provenance audit records this as Finding 4 and notes that the
acceptance criterion "no `LEGACY` marker remains" now passes vacuously.

Slice 3b restores the minimum that makes the map readable again:

- a `CLEAN-ROOM Slice 3b` banner over the 18 table-root variants and a
  second one over the `Error` variant, with per-variant provenance
  docstrings — the material this slice authors;
- a `LEGACY` banner over what is still unrewritten, naming its owner
  slice: `Fn*` (Slice 4), `Vt*` and `MdoExternalDataSource` (Slice 5),
  and `LBrace` / `RBrace` as unowned (§ Unowned brace tokens).

Banners and per-variant docstrings for the already-closed Slices 1, 2,
2-addendum and 3a are **not** restored here. Their attestations remain
accurate about what was authored; only their by-line citations into the
source are stale. Restoring them is a separate decision with its own
commit, and is recorded as such in the master document rather than
folded into this slice's diff.

The restored banners are provenance artefacts, not process notes: each
states which source a declaration was derived from. The repository's
comment rule against citing plans, phases and reviews in source still
holds, and the banners carry no such reference beyond the slice
identifier that names the attestation document.

## Unowned brace tokens

Restoring the map surfaced a gap that no slice document had recorded:
`SdblTokenKind::LBrace` and `SdblTokenKind::RBrace` are covered by no
attestation and owned by no pending slice.

They were introduced by `537527eb` (2026-06-16, "feat(parser): tolerate
{…} query-language extension blocks in SDBL") — after every closed
lexer slice had landed (Slice 1 and 2 on 2026-04-24, Slice 2-addendum
and 3a on 2026-05-07), so no existing attestation could have covered
them, and none names them.

The gap was found by enumerating the class rather than the instance.
All 154 `SdblTokenKind` variants were partitioned against the union of
the four closed attestations' scopes and the three pending slice
families; the partition leaves exactly two variants unaccounted for, so
the class is closed at these two. (`OpAnd` / `OpOr` / `OpNot` look
unowned by name — the Slice 2 scope calls them "logical operators" —
but the Slice 2 attestation names all three explicitly under
§ Logical operators & literals, so they are attested.)

Two further observations about their present state:

- They have no golden-corpus entry and no lexer test. `{` and `}` do
  not appear anywhere in
  `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt`; the only
  reference to either variant outside the enum is the converter's
  `LBrace => T::LBrace` / `RBrace => T::RBrace` pair.
- Their subject matter is punctuation, not vocabulary. In the enum they
  sit inside the Slice 1 punctuation run, between `RParen` and `Dot`,
  which is where they would have been authored had they existed when
  Slice 1 ran.

**Slice 3b does not claim them.** They are not metadata objects, and
folding an unrelated construct into a vocabulary slice to make a number
come out even is how the map got lost in the first place. The
recommendation carried into the master document at C3 is a small
**Slice 1-addendum** on the model of Slice 2-addendum and
Slice 7-addendum: two tokens, derived from the official documentation
of the data-composition query-language extension that gives `{…}` its
meaning, with corpus and acceptance coverage that neither token has
today.

What Slice 3b does do is put them back on the map: the restored
`LEGACY` banner names them as unowned, so the next reader of the file
sees the gap instead of inferring from silence that the lexer is done.

## Sources consulted

The Slice 3b material was re-derived from:

1. **1C:Enterprise 8.3.27 syntax assistant**, section «Работа
   с запросами → Таблицы запросов» (ships with the platform as
   `shcntx_ru.hbk`; the same artefact the repository already uses as the
   provenance base for `crates/bsl-platform/data/platform_data.json`,
   see `crates/bsl-platform/data/PROVENANCE.md`). Each table article's
   bilingual headline is the canonical attestation for one table root.
   This is the primary source: it is the only one that attests all 18
   Russian/English pairs, and the ITS query-language textbook itself
   points at it — «Состав таблиц, доступных для запроса, и их описание
   мы можем увидеть в синтакс-помощнике в разделе Работа с запросами >
   Таблицы запросов»
   (<https://its.1c.ru/db/pubqlang/content/7/hdoc>).
2. **v8.3.27 Developer's Reference, Глава 8 «Работа с запросами»** —
   <https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>:
   - §8.2 «Источники данных (таблицы) запросов» — the statement that
     table names exist in both languages;
   - §8.4.4 «Использование предопределенных данных конфигурации» — the
     bilingual list of predefined-value types, an independent second
     attestation for five of the 18 variants;
   - §8.4.5 bilingual keyword table and its case-insensitivity note —
     the basis for the `(?i)` flag.
3. **1C ITS pubqlang textbook** (secondary, corroborating) —
   <https://its.1c.ru/db/pubqlang/content/7/hdoc> (source tables for
   queries), <https://its.1c.ru/db/pubqlang/content/8/hdoc> (real
   tables), plus the canonical query examples in the register chapters
   for the Russian spellings of the register family.
4. The Slice 1, 2, 2-addendum and 3a clean-room material already present
   in `crates/lexer/src/sdbl/mod.rs` — consulted only for the existing
   `Ident` regex priority shape, the per-variant docstring format, and
   the convenience-index style.

Per the citation policy adopted in Slice 8-addendum and reaffirmed in
Slice 2-addendum and Slice 3a, committed artefacts cite only public ITS
URLs and named document sections; local mirror paths are working
convenience only and appear nowhere in this document, in source
provenance comments, or in commit messages. The syntax assistant has no
public per-article URL, so it is cited by product, version and section
path, which identifies the article unambiguously for anyone with the
platform installed.

## Non-consultation statement

During the authorship of the Slice 3b material the following sources
were not used as working text:

- the sibling `bsl-parser` project — neither its grammar files nor its
  token inventory were consulted;
- `bsl-language-server` — neither its source tree nor its diagnostics;
- the "Comparison against the upstream grammars" section of
  `sdbl-provenance-2026-07-audit.md`, which is under read-quarantine for
  clean-room authors by that document's own read-order warning;
- the pre-clean-room regex text of the Slice 3b variants themselves
  beyond reading them once at C0a for the discrepancy audit;
- any other third-party SDBL grammar, token inventory, or parser.

The byte-identity golden corpus
(`crates/lexer/tests/sdbl_golden_corpus.rs`) is the verification gate
that the preserved regex patterns accept exactly the same text set as
before, and that the four added patterns change exactly the four token
kinds the audit intends and nothing else.

## Verification recipe

All of the following must be green before this attestation is considered
live. Pre-Slice-3b baseline test counts (post-Slice-3a, `develop` as of
2026-07-27) are pinned to detect silent test regressions:

1. `cargo test -p lexer --lib` — **70 passed** pre-Slice-3b.
2. `cargo test -p lexer --test sdbl_slice1_core` — **34 passed**
   pre-Slice-3b; expected unchanged (Slice 1 is closed).
3. `cargo test -p lexer --test sdbl_slice2_keywords` — **43 passed**
   pre-Slice-3b; expected unchanged.
4. `cargo test -p lexer --test sdbl_slice2_addendum_clause_keywords` —
   **30 passed** pre-Slice-3b; expected unchanged.
5. `cargo test -p lexer --test sdbl_slice3a_types` — **25 passed**
   pre-Slice-3b; expected unchanged.
6. `cargo test -p lexer --test sdbl_golden_corpus` — **1 passed**
   (single snapshot test) throughout; the snapshot content changes at
   C0b and again at C2.
7. `cargo test -p lexer --test sdbl_slice3b_metadata_objects` — file
   does **not exist** pre-Slice-3b; the C3 acceptance suite count is
   recorded here at C3.
8. `cargo test -p lexer --tests` — **203 passed** pre-Slice-3b.
9. `cargo test -p parser` — parser-side counts unchanged; the converter
   edit is additive and the four affected words map to the same
   `TokenKind::Ident` before and after.
10. `cargo build --workspace --all-targets`.
11. `cargo clippy -p lexer -p parser --all-targets --all-features
    -- -D warnings`.

## Pre-C0b corpus coverage audit

Coverage of the 18 claimed variants in the pre-Slice-3b corpus
(`crates/lexer/tests/fixtures/sdbl_golden_corpus.txt`). Entry 047
sweeps the English spellings of the pre-existing roots; the Russian
spellings are almost entirely absent, and the four new roots are absent
in both languages.

| Variant | RU in corpus? | EN in corpus? |
|---|---|---|
| `MdoCatalog` | ✓ 006, 051, 059 | ✓ 005, 036, 047, 052 |
| `MdoDocument` | ✓ 064 | ✓ 007, 047 |
| `MdoInformationRegister` | ❌ MISSING | ✓ 047, 048 |
| `MdoAccumulationRegister` | ❌ MISSING | ✓ 047, 048 |
| `MdoAccountingRegister` | ❌ MISSING | ✓ 047, 048 |
| `MdoCalculationRegister` | ❌ MISSING | ✓ 047 |
| `MdoChartOfAccounts` | ❌ MISSING | ✓ 047 |
| `MdoChartOfCalculationTypes` | ❌ MISSING | ✓ 047 |
| `MdoChartOfCharacteristicTypes` | ❌ MISSING | ✓ 047 |
| `MdoEnum` | ✓ 067 | ✓ 047, 049 |
| `MdoBusinessProcess` | ❌ MISSING | ✓ 047 |
| `MdoTask` | ❌ MISSING | ✓ 047 |
| `MdoConstant` | ❌ MISSING | ✓ 047 |
| `MdoSequence` | ❌ MISSING | ✓ 047 |
| `MdoDocumentJournal` | ❌ MISSING | ❌ MISSING |
| `MdoExchangePlan` | ❌ MISSING | ❌ MISSING |
| `MdoFilterCriterion` | ❌ MISSING | ❌ MISSING |
| `MdoConstants` | ❌ MISSING | ❌ MISSING |

Nineteen blind spots require corpus gap-fill at C0b: eleven Russian
spellings of pre-existing roots, and both spellings of each of the four
new roots. The four new roots enter the corpus at C0b **while they still
lex as `Ident`**, so that the C2 snapshot diff is the record of the
behaviour change.

The `Error` fallback has no corpus coverage audit row: it is not a
spelling. Its C0b corpus entry feeds the lexer a byte that no rule
matches, which pins the current fallback behaviour before C2 documents
it.

## Commit trail

- `<C0a>` (2026-07-27) — C0a: this attestation document authored. Sole
  change: addition of `docs/legal/sdbl-clean-room-slice3b.md`.
- `<C0b>` — C0b: corpus gap-fill and snapshot regeneration.
- `<C1>` — C1: banner restoration and relocation; pure refactor.
- `<C2>` — C2: provenance docstrings, the four added variants, the
  converter arm extension, and the corpus snapshot flip.
- `<C3>` — C3: acceptance suite, attestation flip to complete,
  master-document update.
- Anti-Hilbert close-out: a single trailing fixup commit pins the C3 SHA
  explicitly in the trail above. The Anti-Hilbert disclosure: this
  trailing commit's own SHA is NOT named in this enumeration — it cannot
  be, by construction. The Slice 1, 2, 6, 7, 8, 9, 10a, 10b, 11,
  Slice 7-addendum, Slice 8-addendum, Slice 2-addendum and Slice 3a
  attestations all share this pattern.

## Licensing note

The `crates/lexer` crate retains its `LGPL-3.0-or-later` license until
the full Slice 1 → Slice 5 migration is complete. Slice 3b does not
promote the crate to Tier A (`MIT OR Apache-2.0`). After Slice 3b lands,
the remaining lexer-side work is Slice 4 (`Fn*`, 42 variants), Slice 5
(`Vt*`, 6 variants, plus `MdoExternalDataSource`), and the two unowned
brace tokens of § Unowned brace tokens; the `Error` fallback, previously
named as the closing step, closes here.

The exit-criteria list in
[`sdbl-provenance-2026-07-audit.md`](sdbl-provenance-2026-07-audit.md)
§ Exit criteria names Slices 3b, 4, 5 and the `Error` fallback as the
lexer-side items. That list was written before the brace tokens were
noticed and is therefore incomplete by two variants; treating it as
exhaustive would let the lexer be declared clean while two variants
carry no attestation at all. C3 corrects both that list and the master
document.
Crate-level promotion additionally requires the parser-side and
test-corpus criteria in
[`sdbl-provenance-2026-07-audit.md`](sdbl-provenance-2026-07-audit.md)
§ Exit criteria, including the BSL grammar layer, which shares these
crates and has no slice plan at all.

## Author attestation

The Slice 3b material listed above under **Scope** was authored as a
clean-room re-derivation from the sources listed under **Sources
consulted**, without using the `bsl-parser` project, the
`bsl-language-server` project, the pre-clean-room regex text of the
Slice 3b variants beyond a single read-once for the C0a discrepancy
audit, or any other third-party SDBL grammar as working text. This
attestation applies at the date recorded at the top of the document.
