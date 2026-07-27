# SDBL Slice 4 — Clean-Room Attestation (query-function vocabulary)

**Status:** in progress (2026-07-27).

This document attests the clean-room authorship of the Slice 4 material
of the SDBL lexer, per the staged migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

Slice 4 claims the query-function vocabulary: the `Fn*` family of
`SdblTokenKind`. Forty-two `Fn*` variants existed before this slice; one
of them is unreachable and is removed here, and thirteen functions that
the canonical source defines have no lexer variant at all and are born
here. The slice therefore lands **54** variants.

## Status

The Slice 4 attestation flips from "in progress" to "complete" at phase
C3, with the absolute-date stamp at the top of this document. The
C0a / C0b / C1 / C2 / C3 landings are atomic; the absolute-last trailing
commit on the branch (the Anti-Hilbert disclosure) is necessarily not
named in the enumeration, mirroring the Slice 1, 2, 6, 7, 8, 9, 10a,
10b, 11, Slice 7-addendum, Slice 8-addendum, Slice 2-addendum, Slice 3a
and Slice 3b precedents.

## Scope

The paths claimed as clean-room Slice 4 authorship are:

- `crates/lexer/src/sdbl/mod.rs` — specifically:
  - the file-level docstring bullet added for Slice 4;
  - the 54 query-function variants of `SdblTokenKind` declared under the
    `CLEAN-ROOM Slice 4 — query-function vocabulary` banner, with their
    `#[regex(...)]` annotations, their per-variant provenance
    docstrings, and the top-of-block thematic convenience index;
  - the removal of the unreachable `FnDate` variant (see § The
    unreachable `FnDate` variant);
  - the retirement of the `LEGACY (Slice 4 pending)` banner.
- `crates/parser/src/sdbl_token_converter.rs` — the thirteen new
  variants appended to the existing shared `Fn* => T::Ident` arms and
  the removal of `FnDate` from its arm. This is the one non-lexer file
  Slice 4 touches; see § Behaviour change for why the edit is
  parser-tree-invariant and why it cannot be deferred.
- `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt` — thematic
  Slice 4 corpus entries closing the bilingual blind spots surfaced by
  the Pre-C0b corpus coverage audit below.
- `crates/lexer/tests/sdbl_golden_corpus.rs` — the snapshot regenerated
  against the extended corpus at C0b and again at C2 when the thirteen
  new variants start emitting.
- `crates/lexer/tests/sdbl_slice4_functions.rs` — the spec-driven
  acceptance test file born at C3.

### The 54 claimed variants

Forty-one are preserved from before this slice:

`FnSum`, `FnAvg`, `FnMin`, `FnMax`, `FnCount`, `FnYear`, `FnQuarter`,
`FnMonth`, `FnDayOfYear`, `FnDay`, `FnWeek`, `FnWeekDay`, `FnHour`,
`FnMinute`, `FnSecond`, `FnBeginOfPeriod`, `FnEndOfPeriod`,
`FnDateAdd`, `FnDateDiff`, `FnDateTime`, `FnSubstring`,
`FnStringLength`, `FnStrFind`, `FnUpper`, `FnLower`, `FnTrimAll`,
`FnTrimL`, `FnTrimR`, `FnRound`, `FnInt`, `FnLog10`, `FnLog`, `FnPow`,
`FnSqrt`, `FnValueType`, `FnPresentation`, `FnRefPresentation`,
`FnIsNull`, `FnEmptyTable`, `FnEmptyRef`, `FnUUID`.

Thirteen are born in this slice as the C0a audit's Option A ADD outcome
(repository owner's decision, 2026-07-27):

`FnLeft`, `FnRight`, `FnStrReplace`, `FnRecordAutoNumber`,
`FnGroupedBy`, `FnStoredDataSize`, `FnExp`, `FnACos`, `FnASin`,
`FnATan`, `FnCos`, `FnSin`, `FnTan`.

One pre-existing variant is removed: `FnDate`. It is unreachable —
`TypeDate`, declared earlier with the identical pattern, wins every
input. See § The unreachable `FnDate` variant.

### Out of scope

- `СТРОКА (String)` — a documented query function whose spelling is
  already owned by Slice 3a's `TypeString`, because `СТРОКА` is
  simultaneously the name of the primitive type. No variant is added;
  see § Spellings shared with other vocabularies.
- `ЛЕВОЕ (LEFT)` / `ПРАВОЕ (RIGHT)` — the join keywords, owned by
  Slice 2. Slice 4 does not touch `KwLeft` / `KwRight`, and the
  consequence for the English spellings of `Лев` / `Прав` is recorded
  under § Spellings shared with other vocabularies.
- `Mdo*` — Slices 3b and 5.
- `Vt*` and `MdoExternalDataSource` — master-doc Slice 5.
- `LBrace` / `RBrace` — unattested and unowned; recorded by Slice 3b
  § Unowned brace tokens and recommended as Slice 1-addendum. Slice 4
  keeps them on the map but does not claim them.
- Parser-side grammar and `sdbl-hir`. The converter arms named under
  § Scope are the sole parser-crate edit and carry no grammar decision.

## Per-variant tier source map

All 54 variants are classified **Tier A1** with the 1C:Enterprise 8.3.27
syntax assistant book «Синтаксис текста запросов» as the primary
canonical source. Two of its articles carry the whole vocabulary:

- «Функции языка запросов» — the complete grouped index of query
  functions, one syntax line per function;
- «Двуязычное представление ключевых слов» — the Russian/English
  correspondence table for the query language.

Individual function articles supply a bilingual headline of the form
`Функция <Русское имя>(<English name>)` wherever the correspondence
table does not carry the pair.

### String functions

| Variant | RU canonical | EN canonical | Canonical attestation |
|---|---|---|---|
| `FnSubstring` | `ПОДСТРОКА` | `SUBSTRING` | bilingual keyword table |
| `FnStringLength` | `ДлинаСтроки` | `StringLength` | article headline `Функция ДлинаСтроки (StringLength)` |
| `FnTrimL` | `СокрЛ` | `TrimL` | article headline `Функция СокрЛ (TrimL)` |
| `FnTrimR` | `СокрП` | `TrimR` | article headline `Функция СокрП (TrimR)` |
| `FnTrimAll` | `СокрЛП` | `TrimAll` | article headline `Функция СокрЛП (TrimAll)` |
| `FnLeft` | `Лев` | `Left` | article headline `Функция Лев(Left)` |
| `FnRight` | `Прав` | `Right` | article headline `Функция Прав(Right)` |
| `FnStrFind` | `СтрНайти` | `StrFind` | article headline `Функция СтрНайти(StrFind)` |
| `FnStrReplace` | `СтрЗаменить` | `StrReplace` | article headline `Функция СтрЗаменить(StrReplace)` |
| `FnUpper` | `ВРег` | `Upper` | article headline `Функция ВРег(Upper)` |
| `FnLower` | `НРег` | `Lower` | article headline `Функция НРег(Lower)` |

### Date functions

| Variant | RU canonical | EN canonical | Canonical attestation |
|---|---|---|---|
| `FnYear` | `ГОД` | `YEAR` | bilingual keyword table |
| `FnQuarter` | `КВАРТАЛ` | `QUARTER` | bilingual keyword table |
| `FnMonth` | `МЕСЯЦ` | `MONTH` | bilingual keyword table |
| `FnDayOfYear` | `ДЕНЬГОДА` | `DAYOFYEAR` | bilingual keyword table |
| `FnDay` | `ДЕНЬ` | `DAY` | bilingual keyword table |
| `FnWeek` | `НЕДЕЛЯ` | `WEEK` | bilingual keyword table |
| `FnWeekDay` | `ДЕНЬНЕДЕЛИ` | `WEEKDAY` | bilingual keyword table |
| `FnHour` | `ЧАС` | `HOUR` | bilingual keyword table |
| `FnMinute` | `МИНУТА` | `MINUTE` | bilingual keyword table |
| `FnSecond` | `СЕКУНДА` | `SECOND` | bilingual keyword table |
| `FnBeginOfPeriod` | `НАЧАЛОПЕРИОДА` | `BEGINOFPERIOD` | bilingual keyword table |
| `FnEndOfPeriod` | `КОНЕЦПЕРИОДА` | `ENDOFPERIOD` | bilingual keyword table |
| `FnDateAdd` | `ДОБАВИТЬКДАТЕ` | `DATEADD` | bilingual keyword table |
| `FnDateDiff` | `РАЗНОСТЬДАТ` | `DATEDIFF`, `DATEDIFFERENCE` | bilingual keyword table and function index give `DATEDIFF`; the English article headline and its examples give `DATEDIFFERENCE`. See § Source conflict on the English name of `РАЗНОСТЬДАТ`. |
| `FnDateTime` | `ДАТАВРЕМЯ` | `DATETIME` | bilingual keyword table; article «Литерал типа ДАТА». Not a function — see § Variants that are not functions. |

### Mathematical functions

These have no Russian spelling: the source writes them in Latin letters
in both language builds.

| Variant | Canonical name | Canonical attestation |
|---|---|---|
| `FnACos` | `ACos` | article headline `Функция ACos`; function index `ACos(<Number>)` |
| `FnASin` | `ASin` | article headline `Функция ASin` |
| `FnATan` | `ATan` | article headline `Функция ATan` |
| `FnCos` | `Cos` | article headline `Функция Cos` |
| `FnSin` | `Sin` | article headline `Функция Sin` |
| `FnTan` | `Tan` | article headline `Функция Tan` |
| `FnExp` | `Exp` | article headline `Функция Exp` |
| `FnLog` | `Log` | article headline `Функция Log` |
| `FnLog10` | `Log10` | article headline `Функция Log10` |
| `FnPow` | `Pow` | article headline `Функция Pow` |
| `FnSqrt` | `Sqrt` | article headline `Функция Sqrt` |

| Variant | RU canonical | EN canonical | Canonical attestation |
|---|---|---|---|
| `FnRound` | `Окр` | `Round` | article headline `Функция Окр (Round)` |
| `FnInt` | `Цел` | `Int` | article headline `Функция Цел (Int)` |

### Aggregate functions

| Variant | RU canonical | EN canonical | Canonical attestation |
|---|---|---|---|
| `FnSum` | `СУММА` | `SUM` | bilingual keyword table; article `Агрегатная функция СУММА` |
| `FnAvg` | `СРЕДНЕЕ` | `AVG` | bilingual keyword table |
| `FnMin` | `МИНИМУМ` | `MIN` | bilingual keyword table |
| `FnMax` | `МАКСИМУМ` | `MAX` | bilingual keyword table |
| `FnCount` | `КОЛИЧЕСТВО` | `COUNT` | bilingual keyword table |

### Other functions

| Variant | RU canonical | EN canonical | Canonical attestation |
|---|---|---|---|
| `FnRecordAutoNumber` | `АВТОНОМЕРЗАПИСИ` | `RECORDAUTONUMBER` | bilingual keyword table |
| `FnPresentation` | `ПРЕДСТАВЛЕНИЕ` | `PRESENTATION` | bilingual keyword table |
| `FnRefPresentation` | `ПРЕДСТАВЛЕНИЕССЫЛКИ` | `REFPRESENTATION` | bilingual keyword table |
| `FnIsNull` | `ЕСТЬNULL` | `ISNULL` | bilingual keyword table |
| `FnValueType` | `ТИПЗНАЧЕНИЯ` | `VALUETYPE` | bilingual keyword table |
| `FnGroupedBy` | `СГРУППИРОВАНОПО` | `GROUPEDBY` | bilingual keyword table |
| `FnStoredDataSize` | `РАЗМЕРХРАНИМЫХДАННЫХ` | `StoredDataSize` | article headline `StoredDataSize function`; Developer's Reference §8.4.17.4.24 for the Russian spelling |
| `FnUUID` | `УНИКАЛЬНЫЙИДЕНТИФИКАТОР` | `UUID` | bilingual keyword table |
| `FnEmptyTable` | `ПУСТАЯТАБЛИЦА` | `EMPTYTABLE` | bilingual keyword table. Not a function — see § Variants that are not functions. |
| `FnEmptyRef` | `ПустаяСсылка` | `EmptyRef` | article «Использование предопределенных данных конфигурации». Not a function — see § Variants that are not functions. |

The Russian half of the date-function and aggregate-function rows is
independently corroborated by the Developer's Reference bilingual
keyword table (Глава 8 §8.4.5) and by the per-function sections
§8.4.17.3 and §8.4.17.4, which enumerate the same Russian names.

The case-insensitivity that the `(?i)` flag expresses is stated in both
sources: «Регистр букв (строчные или заглавные) при написании
не имеет значения» (Глава 8 §8.4.5).

## C0a discrepancy audit

This section runs before any `#[regex]` body is touched, per the audit
discipline established in Slice 3a and reaffirmed in Slice 3b, whose
findings all came from the reverse direction.

### Audit method

For each of the 42 pre-existing `Fn*` variants:

1. Read the current `#[regex(...)]` attribute body verbatim.
2. Locate the syntax-assistant article for that function and read its
   headline; where the bilingual keyword table carries the pair, read
   that row too.
3. Compare the regex alternation byte-strings against the canonical
   spellings under `(?i)` case folding.
4. Record any second independent attestation from the Developer's
   Reference.

Then, in the opposite direction: enumerate every function in the
«Функции языка запросов» index and check that the lexer has a variant
for it.

Then, as a third pass introduced by this slice: feed every claimed
spelling to `tokenize_sdbl` and record what it actually emits. A regex
can be byte-correct and still be dead if an earlier declaration shadows
it, and no paper audit can see that.

### Audit results — spelling (41/42 MATCH, 1 defect)

Every regex alternation except one is the lower-case byte-form of its
canonical spelling.

- `FnSum` — `сумма|sum`; `СУММА (SUM)`. **MATCH.**
- `FnAvg` — `среднее|avg`; `СРЕДНЕЕ (AVG)`. **MATCH.**
- `FnMin` — `минимум|min`; `МИНИМУМ (MIN)`. **MATCH.**
- `FnMax` — `максимум|max`; `МАКСИМУМ (MAX)`. **MATCH.**
- `FnCount` — `количество|count`; `КОЛИЧЕСТВО (COUNT)`. **MATCH.**
- `FnYear` — `год|year`. **MATCH.**
- `FnQuarter` — `квартал|quarter`. **MATCH.**
- `FnMonth` — `месяц|month`. **MATCH.**
- `FnDayOfYear` — `деньгода|dayofyear`. **MATCH.**
- `FnDay` — `день|day`. **MATCH.**
- `FnWeek` — `неделя|week`. **MATCH.**
- `FnWeekDay` — `деньнедели|weekday`. **MATCH.**
- `FnHour` — `час|hour`. **MATCH.**
- `FnMinute` — `минута|minute`. **MATCH.**
- `FnSecond` — `секунда|second`. **MATCH.**
- `FnBeginOfPeriod` — `началопериода|beginofperiod`. **MATCH.**
- `FnEndOfPeriod` — `конецпериода|endofperiod`. **MATCH.**
- `FnDateAdd` — `добавитькдате|dateadd`. **MATCH.**
- `FnDateDiff` — `разностьдат|datediff`. **MATCH** against the
  bilingual keyword table and the function index; **incomplete**
  against the English article, which spells the function
  `DATEDIFFERENCE`. See § Source conflict on the English name of
  `РАЗНОСТЬДАТ`.
- `FnDateTime` — `датавремя|datetime`. **MATCH.**
- `FnDate` — `дата|date`. Spelling matches the `ДАТА (DATE)` row of the
  bilingual keyword table, but the variant is **unreachable**. See
  § The unreachable `FnDate` variant.
- `FnSubstring` — `подстрока|substring`. **MATCH.**
- `FnStringLength` — `длинастроки|stringlength`. **MATCH.**
- `FnStrFind` — `стрнайти|strfind`. **MATCH.**
- `FnUpper` — `врег|upper`. **MATCH.**
- `FnLower` — `нрег|lower`. **MATCH.**
- `FnTrimAll` — `сокрлп|trimall`. **MATCH.**
- `FnTrimL` — `сокрл|triml`. **MATCH.**
- `FnTrimR` — `сокрп|trimr`. **MATCH.**
- `FnRound` — `окр|round`. **MATCH.**
- `FnInt` — `цел|int`. **MATCH.**
- `FnLog10` — `log10`. **MATCH** (no Russian spelling exists).
- `FnLog` — `log`. **MATCH.**
- `FnPow` — `pow`. **MATCH.**
- `FnSqrt` — `sqrt`. **MATCH.**
- `FnValueType` — `типзначения|valuetype`. **MATCH.**
- `FnPresentation` — `представление|presentation`. **MATCH.**
- `FnRefPresentation` — `представлениессылки|refpresentation`.
  **MATCH.** The Developer's Reference does not mention this function
  at all; the syntax assistant carries both the article and the
  bilingual keyword-table row, which is why it is the primary source
  for this slice.
- `FnIsNull` — `естьnull|isnull`. **MATCH.**
- `FnEmptyTable` — `пустаятаблица|emptytable`. **MATCH.**
- `FnEmptyRef` — `пустаяссылка|emptyref`. **MATCH.**
- `FnUUID` — `уникальныйидентификатор|uuid`. **MATCH.**

### Audit results — coverage (13 gaps)

The «Функции языка запросов» index defines 52 functions. Two of them
(`СТРОКА (String)` and the pair `Лев (Left)` / `Прав (Right)` on their
English side) share a spelling with a vocabulary another slice owns.
Setting `СТРОКА` aside, thirteen functions have no lexer variant at all
and therefore lex as bare `Ident`:

| Missing function | RU | EN | Group |
|---|---|---|---|
| left substring | `Лев` | `Left` | string |
| right substring | `Прав` | `Right` | string |
| substring replacement | `СтрЗаменить` | `StrReplace` | string |
| temporary-table row counter | `АВТОНОМЕРЗАПИСИ` | `RECORDAUTONUMBER` | other |
| grouping-set probe | `СГРУППИРОВАНОПО` | `GROUPEDBY` | other |
| stored data size | `РАЗМЕРХРАНИМЫХДАННЫХ` | `StoredDataSize` | other |
| exponent | — | `Exp` | mathematical |
| arc cosine | — | `ACos` | mathematical |
| arc sine | — | `ASin` | mathematical |
| arc tangent | — | `ATan` | mathematical |
| cosine | — | `Cos` | mathematical |
| sine | — | `Sin` | mathematical |
| tangent | — | `Tan` | mathematical |

The mathematical group is the most striking: the lexer carried four of
the eleven mathematical functions (`Log`, `Log10`, `Pow`, `Sqrt`) and
missed the seven remaining ones, which is what a partial transcription
looks like rather than a considered subset. The Developer's Reference
lists all eleven, in two tables — §8.4.17.4.3 «Алгебраические функции»
and §8.4.17.4.35 «Тригонометрические функции» — and the syntax
assistant lists them in one.

These thirteen are true false-negatives of the vocabulary: each is a
legal query function today, and each currently tokenises as an ordinary
identifier.

### The unreachable `FnDate` variant

`FnDate` carries `#[regex(r"(?i)дата|(?i)date", priority = 2)]`. The
identical pattern is already declared as `TypeDate`, 78 lines earlier,
with no explicit priority. logos derives a default priority from the
pattern — for this pattern, higher than the literal `2` written on
`FnDate` — so `TypeDate` wins every input and `FnDate` is emitted for
none.

Measured before any edit:

```
Дата  -> [TypeDate]
ДАТА  -> [TypeDate]
Date  -> [TypeDate]
```

The word belongs to `TypeDate`. The source calls `ДАТА (DATE)` a
primitive type — the article «Литерал типа ДАТА» and the `ВЫРАЗИТЬ`
type list both use it that way, and the function index does not list a
`ДАТА` function. Slice 3a already claims `TypeDate` on exactly that
reading. `FnDate` is therefore removed rather than re-derived
(repository owner's decision, 2026-07-27).

Removal is behaviour-preserving by construction: a variant that is
emitted for no input cannot change what the lexer emits. It is not
edit-free, though — the converter's match is exhaustive, so the arm
naming `FnDate` must lose it in the same commit.

### Source conflict on the English name of `РАЗНОСТЬДАТ`

The syntax assistant gives two English spellings for one function:

- the bilingual keyword table row reads `РАЗНОСТЬДАТ | DATEDIFF`;
- the function index reads
  `DATEDIFF(<Expression>, <Expression>, SECOND | MINUTE | …)`;
- the English article is headlined `DATEDIFFERENCE function` and its
  worked examples read `SELECT DATEDIFFERENCE(DATETIME(2002, 10, 12,
  10, 15, 34), …)`.

The Developer's Reference does not settle it: §8.4.5 omits
`РАЗНОСТЬДАТ` from the bilingual table entirely, and §8.4.17.4.25 gives
only the Russian name.

Both spellings are attested by the primary source, so the vocabulary
accepts both (repository owner's decision, 2026-07-27). A token
vocabulary is an accept-set; admitting a spelling the source documents
costs nothing downstream, whereas rejecting it makes a documented query
lex as an identifier. `datediff` is a proper prefix of
`datedifference`, so longest-match separates them without ambiguity.

### Audit conclusion

Forty-one of the 42 pre-existing regex bodies are correct and are
preserved byte-for-byte through C1 and C2; `FnDateDiff` gains one
alternation and keeps the two it had. The slice's behaviour changes are
the thirteen added functions and the `datedifference` alternation. The
`FnDate` removal changes no behaviour at all.

## Variants that are not functions

Three claimed variants are named `Fn*` but are not functions in the
source. Slice 4 owns them because it owns the `Fn*` family, and
documents what they actually are rather than renaming them — a rename
would be a downstream-visible change with no provenance value.

- `FnDateTime` (`ДАТАВРЕМЯ (DATETIME)`) is the **date literal
  keyword**. The article «Литерал типа ДАТА» writes the literal as
  `ДАТАВРЕМЯ(<год>, <месяц>, <день>[, <час>, <минута>, <секунда>])`;
  the function index does not list it.
- `FnEmptyTable` (`ПУСТАЯТАБЛИЦА (EMPTYTABLE)`) is a **keyword of the
  selection field list**, used to supply an empty nested table on one
  side of a `ОБЪЕДИНИТЬ`. The article «Пустые вложенные таблицы
  в списке выборки» calls it «ключевое слово».
- `FnEmptyRef` (`ПустаяСсылка (EmptyRef)`) is a **predefined-data
  selector**, the value part of `ЗНАЧЕНИЕ(<тип>.<объект>.ПустаяСсылка)`.
  The article «Использование предопределенных данных конфигурации»
  gives the bilingual pair and the canonical example
  `ГДЕ Город = ЗНАЧЕНИЕ(Справочник.Города.ПустаяСсылка)`.

All three spellings are canonical; only their family label is wrong.

## Spellings shared with other vocabularies

Three canonical function spellings collide, byte for byte, with
spellings that other slices already own. A lexer cannot resolve them:
the distinction is which grammatical position the word stands in, which
only the parser knows.

- **`СТРОКА (String)`** — the function `STRING(<Value>)` and the
  primitive type `Строка` are the same word in both languages. The
  spelling is owned by Slice 3a's `TypeString`, which already emits for
  both. Slice 4 adds no variant and records the double duty here.
- **`Лев (Left)` and `Прав (Right)`** — the English spellings are
  identical to the join keywords `ЛЕВОЕ (LEFT)` and `ПРАВОЕ (RIGHT)`,
  owned by Slice 2 as `KwLeft` / `KwRight`. The Russian spellings do
  not collide: `лев` and `прав` are proper prefixes of `левое` and
  `правое`, which longest-match separates.

  `FnLeft` and `FnRight` are therefore declared with **Russian-only**
  alternations (repository owner's decision, 2026-07-27). Declaring
  `left` on both `FnLeft` and `KwLeft` would make two rules match the
  same five bytes, which logos resolves by priority rather than by
  meaning — and whichever won, one of `LEFT JOIN` or `Left(<String>,
  <N>)` would be mis-tokenised. Leaving the English spellings on the
  join keywords keeps `LEFT JOIN` — by far the more common
  construct — correct at the lexer, and leaves the function reading to
  the parser, which sees the following `(`.

  The asymmetry is real and is not hidden: `ЛЕВ(...)` lexes as
  `FnLeft` while `Left(...)` lexes as `KwLeft`. Both reach the parser
  as different token kinds, and a future parser-side slice that
  implements the `Лев` function must accept both.

## Behaviour change

**Additive, parser-tree-invariant.**

Thirteen new `SdblTokenKind` variants change what the lexer emits for
twenty spellings that previously emitted `Ident`, and one existing
variant gains one alternation. Nothing downstream observes the
difference, for two reasons:

1. `SdblTokenKind` has exactly two consumers in the workspace — the
   `lexer` crate itself and `crates/parser/src/sdbl_token_converter.rs`.
   No other crate names the type.
2. The converter maps **every** `Fn*` variant, without exception, to
   `TokenKind::Ident`. Appending the new variants to those arms
   reproduces, token for token, what the parser saw when these words
   were lexed as `Ident`.

The same two facts make the `FnDate` removal invisible: it was emitted
for no input, and its converter arm produced `T::Ident` anyway.

The converter edit is mandatory and cannot be deferred to a later
slice: the match is exhaustive over `SdblTokenKind`, so adding a variant
without extending an arm — or removing one still named in an arm — does
not compile. It is a mechanical edit with no grammar decision in it,
which is why Slice 4, like Slice 3b, is not a strictly lexer-only slice.

Longest-match interaction was checked for each new alternation:

- `лев` vs `левое` (`KwLeft`) and `прав` vs `правое` (`KwRight`) — the
  function spellings are proper prefixes of the keyword spellings, so
  the keyword wins its own input by length and the function wins the
  shorter input outright.
- `acos` / `asin` / `atan` vs `cos` / `sin` / `tan` — matching starts
  at the token boundary, so `cos` cannot match inside `acos`; the pairs
  never compete.
- `сгруппированопо` vs `сгруппировать` (`KwGroup`) — the two diverge at
  the tenth character. `groupedby` vs `group` — longest match takes the
  nine-byte alternation.
- `datedifference` vs `datediff` — same alternation, longest match.
- `exp`, `стрзаменить|strreplace`, `автономерзаписи|recordautonumber`,
  `размерхранимыхданных|storeddatasize` — no other alternation in the
  enum shares a prefix with any of these.
- Against `Ident`, which matches the same bytes in every case, the
  explicit `priority = 1` on `Ident` yields to the keyword rules. This
  is the tie-break Slice 1 established for the whole keyword
  vocabulary.

The golden corpus is the gate: C0b lands corpus entries for all thirteen
new functions while they still tokenise as `Ident`, so the snapshot
records the pre-change behaviour; C2 flips those snapshot lines to the
new kinds. The flip is the audit trail of the behaviour change, in the
same shape as the Slice 3b `Mdo*` corpus flip.

## Marker restoration

Slice 3b restored the provenance map that `3aa29b99` erased from
`crates/lexer/src/sdbl/mod.rs`, and left a `LEGACY (Slice 4 pending —
query-function vocabulary)` banner over the `Fn*` block. Slice 4
retires that banner and replaces it with a `CLEAN-ROOM Slice 4` banner
plus per-variant provenance docstrings, on the model Slice 3b
established.

Banners and per-variant docstrings for the closed Slices 1, 2,
2-addendum and 3a remain **not** restored, and the module docstring
continues to say so, so that a missing banner is not read as a missing
attestation.

## Sources consulted

The Slice 4 material was re-derived from:

1. **1C:Enterprise 8.3.27 syntax assistant, book «Синтаксис текста
   запросов»** (ships with the platform as `shquery_ru.hbk`, with the
   English build as `shquery_root.hbk`; the same class of artefact the
   repository already uses as the provenance base for
   `crates/bsl-platform/data/platform_data.json`, see
   `crates/bsl-platform/data/PROVENANCE.md`). This is the primary
   source. Within it:
   - the article «Функции языка запросов» — the complete grouped index
     of query functions and their syntax lines;
   - the article «Двуязычное представление ключевых слов» — the
     Russian/English correspondence table, which is strictly more
     complete for functions than the Developer's Reference table: it
     alone carries `АВТОНОМЕРЗАПИСИ`, `ПРЕДСТАВЛЕНИЕССЫЛКИ`,
     `РАЗНОСТЬДАТ`, `СГРУППИРОВАНОПО` and
     `УНИКАЛЬНЫЙИДЕНТИФИКАТОР`;
   - the per-function articles, whose headlines carry the bilingual
     pair for every function the correspondence table omits;
   - «Литерал типа ДАТА», «Пустые вложенные таблицы в списке выборки»
     and «Использование предопределенных данных конфигурации» — the
     three articles that reclassify the non-function variants.
2. **v8.3.27 Developer's Reference, Глава 8 «Работа с запросами»** —
   <https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>:
   - §8.4.5 bilingual keyword table and its case-insensitivity note —
     the basis for the `(?i)` flag and a second attestation for the
     date and aggregate families;
   - §8.4.17.3 «Агрегатные функции языка запросов» — the five
     aggregate functions;
   - §8.4.17.4 «Встроенные функции языка запросов» — the per-function
     Russian enumeration, including §8.4.17.4.3 «Алгебраические
     функции» and §8.4.17.4.35 «Тригонометрические функции» for the
     eleven mathematical functions, and §8.4.17.4.24 for the Russian
     spelling of `StoredDataSize`;
   - §8.4.17.7 «Константы и параметры в языке запросов» — the
     `ДАТАВРЕМЯ` literal.
3. **1C ITS pubqlang textbook** (secondary, corroborating) —
   <https://its.1c.ru/db/pubqlang/content/12/hdoc> and
   <https://its.1c.ru/db/pubqlang/content/10/hdoc>, for canonical query
   examples using the Russian function spellings.
4. The Slice 1, 2, 2-addendum, 3a and 3b clean-room material already
   present in `crates/lexer/src/sdbl/mod.rs` — consulted only for the
   existing `Ident` regex priority shape, the per-variant docstring
   format, and the convenience-index style.

Per the citation policy adopted in Slice 8-addendum and reaffirmed in
Slice 2-addendum, Slice 3a and Slice 3b, committed artefacts cite only
public ITS URLs and named document sections; local mirror paths are
working convenience only and appear nowhere in this document, in source
provenance comments, or in commit messages. The syntax assistant has no
public per-article URL, so it is cited by product, version, book and
article title, which identifies the article unambiguously for anyone
with the platform installed.

## Non-consultation statement

During the authorship of the Slice 4 material the following sources
were not used as working text:

- the sibling `bsl-parser` project — neither its grammar files nor its
  token inventory were consulted;
- `bsl-language-server` — neither its source tree nor its diagnostics;
- the "Comparison against the upstream grammars" section of
  `sdbl-provenance-2026-07-audit.md`, which is under read-quarantine for
  clean-room authors by that document's own read-order warning;
- the pre-clean-room regex text of the Slice 4 variants themselves
  beyond reading them once at C0a for the discrepancy audit;
- any other third-party SDBL grammar, token inventory, or parser.

The byte-identity golden corpus
(`crates/lexer/tests/sdbl_golden_corpus.rs`) is the verification gate
that the preserved regex patterns accept exactly the same text set as
before, and that the added patterns change exactly the token kinds the
audit intends and nothing else.

## Verification recipe

All of the following must be green before this attestation is considered
live. Pre-Slice-4 baseline test counts (post-Slice-3b, `develop` as of
2026-07-27) are pinned to detect silent test regressions:

1. `cargo test -p lexer --lib` — **70 passed** pre-Slice-4.
2. `cargo test -p lexer --test sdbl_slice1_core` — **34 passed**
   pre-Slice-4; expected unchanged (Slice 1 is closed).
3. `cargo test -p lexer --test sdbl_slice2_keywords` — **43 passed**
   pre-Slice-4; expected unchanged.
4. `cargo test -p lexer --test sdbl_slice2_addendum_clause_keywords` —
   **30 passed** pre-Slice-4; expected unchanged.
5. `cargo test -p lexer --test sdbl_slice3a_types` — **25 passed**
   pre-Slice-4; expected unchanged. This suite is the guard on the
   `FnDate` removal: `TypeDate` is its variant, and its behaviour must
   not move.
6. `cargo test -p lexer --test sdbl_slice3b_metadata_objects` —
   **49 passed** pre-Slice-4; expected unchanged.
7. `cargo test -p lexer --test sdbl_golden_corpus` — **1 passed**
   (single snapshot test) throughout; the snapshot content changes at
   C0b and again at C2.
8. `cargo test -p lexer --test sdbl_slice4_functions` — file does
   **not exist** pre-Slice-4.
9. `cargo test -p lexer --tests` — **252 passed** pre-Slice-4.
10. `cargo test -p parser` — **596 passed** both before and after; the
    converter edit is additive and every `Fn*` variant maps to the same
    `TokenKind::Ident` before and after.
11. `cargo build --workspace --all-targets`.
12. `cargo clippy -p lexer -p parser --all-targets --all-features
    -- -D warnings`.

## Pre-C0b corpus coverage audit

Coverage of the claimed spellings in the pre-Slice-4 corpus
(`crates/lexer/tests/fixtures/sdbl_golden_corpus.txt`). Entries 041–046
sweep the English spellings of the pre-existing functions; the Russian
spellings are almost entirely absent, and the thirteen new functions are
absent in every language they have.

| Variant | RU in corpus? | EN in corpus? |
|---|---|---|
| `FnSum` | ✓ 040, 053, 062 | ✓ 041, 055, 070, 073 |
| `FnAvg` | ❌ MISSING | ✓ 041 |
| `FnMin` | ❌ MISSING | ✓ 041 |
| `FnMax` | ❌ MISSING | ✓ 041 |
| `FnCount` | ❌ MISSING | ✓ 027, 041 |
| `FnYear` | ❌ MISSING | ✓ 042, 043 |
| `FnQuarter` | ❌ MISSING | ✓ 042, 043 |
| `FnMonth` | ❌ MISSING | ✓ 042, 043 |
| `FnDayOfYear` | ❌ MISSING | ✓ 042 |
| `FnDay` | ✓ 040 | ✓ 042, 043, 070 |
| `FnWeek` | ❌ MISSING | ✓ 042 |
| `FnWeekDay` | ❌ MISSING | ✓ 042 |
| `FnHour` | ❌ MISSING | ✓ 042 |
| `FnMinute` | ❌ MISSING | ✓ 042 |
| `FnSecond` | ❌ MISSING | ✓ 042 |
| `FnBeginOfPeriod` | ❌ MISSING | ✓ 043 |
| `FnEndOfPeriod` | ❌ MISSING | ✓ 043 |
| `FnDateAdd` | ❌ MISSING | ✓ 043 |
| `FnDateDiff` | ❌ MISSING | ✓ 043 (`DATEDIFF`); `DATEDIFFERENCE` ❌ MISSING |
| `FnDateTime` | ✓ 040 | ✓ 070 |
| `FnSubstring` | ❌ MISSING | ✓ 044 |
| `FnStringLength` | ❌ MISSING | ✓ 044 |
| `FnStrFind` | ❌ MISSING | ✓ 044 |
| `FnUpper` | ❌ MISSING | ✓ 044 |
| `FnLower` | ❌ MISSING | ✓ 044 |
| `FnTrimAll` | ❌ MISSING | ✓ 044 |
| `FnTrimL` | ❌ MISSING | ✓ 044 |
| `FnTrimR` | ❌ MISSING | ✓ 044 |
| `FnRound` | ❌ MISSING | ✓ 045 |
| `FnInt` | ❌ MISSING | ✓ 045 |
| `FnLog10` | — | ✓ 045 |
| `FnLog` | — | ✓ 045 |
| `FnPow` | — | ✓ 045 |
| `FnSqrt` | — | ✓ 045 |
| `FnValueType` | ❌ MISSING | ✓ 046 |
| `FnPresentation` | ❌ MISSING | ✓ 046 |
| `FnRefPresentation` | ❌ MISSING | ✓ 046 |
| `FnIsNull` | ❌ MISSING | ✓ 046 |
| `FnEmptyTable` | ❌ MISSING | ✓ 046 |
| `FnEmptyRef` | ❌ MISSING | ✓ 046 |
| `FnUUID` | ❌ MISSING | ✓ 046 |
| `FnLeft` | ❌ MISSING | n/a (English spelling stays `KwLeft`) |
| `FnRight` | ❌ MISSING | n/a (English spelling stays `KwRight`) |
| `FnStrReplace` | ❌ MISSING | ❌ MISSING |
| `FnRecordAutoNumber` | ❌ MISSING | ❌ MISSING |
| `FnGroupedBy` | ❌ MISSING | ❌ MISSING |
| `FnStoredDataSize` | ❌ MISSING | ❌ MISSING |
| `FnExp` | — | ❌ MISSING |
| `FnACos` | — | ❌ MISSING |
| `FnASin` | — | ❌ MISSING |
| `FnATan` | — | ❌ MISSING |
| `FnCos` | — | ❌ MISSING |
| `FnSin` | — | ❌ MISSING |
| `FnTan` | — | ❌ MISSING |

Blind spots requiring corpus gap-fill at C0b — 52 in total: 34 Russian
spellings of pre-existing functions, both spellings of `СтрЗаменить`,
`АВТОНОМЕРЗАПИСИ`, `СГРУППИРОВАНОПО` and `РАЗМЕРХРАНИМЫХДАННЫХ`, the
Russian spellings of `Лев` and `Прав`, the seven mathematical
functions, and the `DATEDIFFERENCE` alternation. The new spellings enter
the corpus at C0b **while they still lex as `Ident`**, so that the C2
snapshot diff is the record of the behaviour change.

`FnDate` needs no gap-fill row and gets none: entries 049 and 071
already contain `Date` and `ДАТА`, and the snapshot already records them
as `TypeDate`. Those two lines are the pin that the removal changes
nothing.

### C0b outcome

Fourteen thematic entries landed, numbered 082–095, closing all 52 blind
spots:

- **082** aggregate functions russian — `СУММА`, `СРЕДНЕЕ`, `МИНИМУМ`,
  `МАКСИМУМ`, `КОЛИЧЕСТВО`.
- **083** date part functions russian — the ten date-component
  functions from `ГОД` to `СЕКУНДА`.
- **084** date arithmetic functions russian — `НАЧАЛОПЕРИОДА`,
  `КОНЕЦПЕРИОДА`, `ДОБАВИТЬКДАТЕ`, `РАЗНОСТЬДАТ`, with `ДАТАВРЕМЯ`
  nested as the literal it is.
- **085** string functions russian — `ПОДСТРОКА`, `ДЛИНАСТРОКИ`,
  `СТРНАЙТИ`, `ВРЕГ`, `НРЕГ`, and the three `СОКР*` spellings in one
  line, so the entry doubles as the longest-match guard for
  `СОКРЛП` over `СОКРЛ`.
- **086** numeric, type and null functions russian — `ОКР`, `ЦЕЛ`,
  `ТИПЗНАЧЕНИЯ`, `ЕСТЬNULL`, `УНИКАЛЬНЫЙИДЕНТИФИКАТОР`, and
  `ПРЕДСТАВЛЕНИЕ` next to `ПРЕДСТАВЛЕНИЕССЫЛКИ`, the second
  longest-match pair.
- **087** empty-table keyword and empty-ref selector russian — both
  non-function variants in the grammatical position the source gives
  them: `ПУСТАЯТАБЛИЦА.(…)` in the selection list and
  `ЗНАЧЕНИЕ(Справочник.Товары.ПустаяСсылка)` in a filter.
- **088** left and right substring russian — `ЛЕВ`, `ПРАВ`.
- **089** join keywords keep the english left and right — `ЛЕВОЕ`,
  `ПРАВОЕ`, `LEFT`, `RIGHT` around `СОЕДИНЕНИЕ` / `JOIN`. This entry
  exists to be *unchanged* by C2: it is the pin that adding `FnLeft`
  and `FnRight` does not disturb the join keywords.
- **090** string replacement bilingual — `СтрЗаменить`, `StrReplace`.
- **091** record autonumber bilingual — `АВТОНОМЕРЗАПИСИ`,
  `RECORDAUTONUMBER`.
- **092** grouped by probe bilingual — `СГРУППИРОВАНОПО`, `GROUPEDBY`,
  the latter also guarding longest match over `GROUP`.
- **093** stored data size bilingual — `РАЗМЕРХРАНИМЫХДАННЫХ`,
  `STOREDDATASIZE`.
- **094** trigonometric and exponential functions — the seven missing
  mathematical functions next to the four the lexer already had, so the
  entry contrasts them directly.
- **095** date difference alternate english spelling —
  `DATEDIFFERENCE`.

The regenerated snapshot confirms all three halves of the C0a audit:

- All 34 previously-unpinned Russian spellings tokenise to their `Fn*`
  variants, so the preserved regexes accept the Russian side of the
  vocabulary at the byte level and not only on paper.
- All 18 spellings that the slice adds tokenise to `Ident`: `ЛЕВ`,
  `ПРАВ`, `СтрЗаменить`, `StrReplace`, `АВТОНОМЕРЗАПИСИ`,
  `RECORDAUTONUMBER`, `СГРУППИРОВАНОПО`, `GROUPEDBY`,
  `РАЗМЕРХРАНИМЫХДАННЫХ`, `STOREDDATASIZE`, `ACos`, `ASin`, `ATan`,
  `Cos`, `Sin`, `Tan`, `Exp`, `DATEDIFFERENCE`. This is the pre-change
  behaviour the audit predicted, and it is now pinned; the C2 diff on
  these 18 lines is the record of the behaviour change. Each of the 18
  spellings occurs in exactly one corpus entry, so the expected diff is
  18 lines and no others.
- Entry 089 renders `KwLeft`/`KwRight` for all four join spellings, and
  entry 087 renders `FnEmptyTable` and `FnEmptyRef` in their canonical
  positions.

## Commit trail

- `eb49bba3` (2026-07-27) — C0a: this attestation document authored.
  Sole change: addition of `docs/legal/sdbl-clean-room-slice4.md`.
- C0b — corpus entries 082–095 added to
  `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt`; snapshot
  regenerated via `UPDATE_EXPECT=1`. See § C0b outcome.

## Licensing note

The `crates/lexer` crate retains its `LGPL-3.0-or-later` license until
the full Slice 1 → Slice 5 migration is complete. Slice 4 does not
promote the crate to Tier A (`MIT OR Apache-2.0`). After Slice 4 lands,
the remaining lexer-side work is Slice 5 (`Vt*`, 6 variants, plus
`MdoExternalDataSource`) and the two unowned brace tokens recorded by
Slice 3b § Unowned brace tokens.

Crate-level promotion additionally requires the parser-side and
test-corpus criteria in
[`sdbl-provenance-2026-07-audit.md`](sdbl-provenance-2026-07-audit.md)
§ Exit criteria, including the BSL grammar layer, which shares these
crates and has no slice plan at all.

## Author attestation

The Slice 4 material listed above under **Scope** was authored as a
clean-room re-derivation from the sources listed under **Sources
consulted**, without using the `bsl-parser` project, the
`bsl-language-server` project, the pre-clean-room regex text of the
Slice 4 variants beyond a single read-once for the C0a discrepancy
audit, or any other third-party SDBL grammar as working text. This
attestation applies at the date recorded at the top of the document.
