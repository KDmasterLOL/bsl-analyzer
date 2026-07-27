# SDBL Slice 5 — Clean-Room Attestation (virtual tables and external data sources)

**Status:** complete (2026-07-27).

This document attests the clean-room authorship of the Slice 5 material
of the SDBL lexer, per the staged migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

Slice 5 claims the virtual-table vocabulary — the `Vt*` family of
`SdblTokenKind` — together with the `MdoExternalDataSource` table root
that Slice 3b deferred here. Seven variants existed before this slice;
eleven virtual-table suffixes that the canonical source defines have no
lexer variant at all and are born here. The slice therefore lands
**18** variants.

It is the last vocabulary slice on the lexer side. After it, the only
unattested lexer material is the brace pair recorded by Slice 3b
§ Unowned brace tokens.

## Status

The Slice 5 attestation flipped from "in progress" to "complete" at
phase C3, with the absolute-date stamp at the top of this document. The
C0a / C0b / C1 / C2 / C3 landings are atomic; the absolute-last trailing
commit on the branch (the Anti-Hilbert disclosure) is necessarily not
named in the enumeration, mirroring the Slice 1, 2, 6, 7, 8, 9, 10a,
10b, 11, Slice 7-addendum, Slice 8-addendum, Slice 2-addendum, Slice 3a,
Slice 3b and Slice 4 precedents.

## Scope

The paths claimed as clean-room Slice 5 authorship are:

- `crates/lexer/src/sdbl/mod.rs` — specifically:
  - the file-level docstring bullet added for Slice 5;
  - the 18 virtual-table and external-source variants of
    `SdblTokenKind` declared under the `CLEAN-ROOM Slice 5 — virtual
    tables and external data sources` banner, with their
    `#[regex(...)]` annotations, their per-variant provenance
    docstrings, and the top-of-block thematic convenience index;
  - the retirement of the last `LEGACY (Slice 5 pending)` banner.
- `crates/parser/src/sdbl_token_converter.rs` — the eleven new variants
  appended to the existing shared `Vt* => T::Ident` arm. This is the one
  non-lexer file Slice 5 touches; see § Behaviour change for why the
  edit is parser-tree-invariant and why it cannot be deferred.
- `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt` — thematic
  Slice 5 corpus entries closing the bilingual blind spots surfaced by
  the Pre-C0b corpus coverage audit below.
- `crates/lexer/tests/sdbl_golden_corpus.rs` — the snapshot regenerated
  against the extended corpus at C0b and again at C2 when the eleven new
  variants start emitting.
- `crates/lexer/tests/sdbl_slice5_virtual_tables.rs` — the spec-driven
  acceptance test file born at C3, 23 tests: 3 canonical-form sweeps
  over the 17 bilingual pairs and the English-only `Changes`,
  1 case-insensitivity sweep, 1 spelling-to-kind distinctness check,
  5 longest-match guards, 4 pins on the spellings and prefixes this
  slice deliberately did not take, 1 pin on the lexer's
  dot-insensitivity, and 8 structural integration tests.

### The 18 claimed variants

Seven are preserved from before this slice:

`MdoExternalDataSource`, `VtSliceFirst`, `VtSliceLast`, `VtBalance`,
`VtTurnovers`, `VtBalanceAndTurnovers`, `VtDrCrTurnovers`.

Eleven are born in this slice as the C0a audit's Option A ADD outcome
(repository owner's decision, 2026-07-27):

`VtExtDimensions`, `VtRecordsWithExtDimensions`, `VtScheduleData`,
`VtAdjustedEffectivePeriod`, `VtBoundaries`, `VtPoints`,
`VtTasksByPerformer`, `VtTable`, `VtCube`, `VtDimensionTable`,
`VtChanges`.

### Out of scope

- `База<Имя базового регистра расчета>` / `Base<...>` — a documented
  virtual table whose name is a **prefix glued to a register name**, not
  a standalone word. See § The `База` prefix is not a token.
- `ТочкаМаршрута (RoutePoint)` — a field name and an element of the
  predefined-data path `БизнесПроцесс.<Имя>.ТочкаМаршрута.<Имя точки>`,
  not a virtual table. Field names are identifiers throughout this
  lexer — `Код (Code)`, `Наименование (Description)` and
  `ПометкаУдаления (DeletionMark)` all lex as `Ident` — and Slice 5
  keeps that treatment.
- Virtual-table **parameters** — `Период`, `Условие`, `Периодичность`,
  `Метод дополнения периодов`, `Разрезы`, `Субконто` as a parameter
  name. The «Таблицы запросов» section documents these as prose
  headings without a bilingual pair, and they are argument positions
  rather than name components. The parser-side handling of the argument
  list is attested by Slice 8-addendum.
- Standard fields with a bilingual pair — `Ссылка (Ref)`, `Код (Code)`,
  `Период (Period)`, `Регистратор (Recorder)` and the rest. They are
  fields of tables, not table names, and the lexer emits `Ident` for
  them by design. Where such a spelling is already a token it is
  because another construct claimed it: `KwRefs` exists for the
  `ССЫЛКА (REFS)` type-test operator, not for the `Ссылка` field.
- Resource-derived field suffixes — `<Имя ресурса>Остаток (Balance)`,
  `<Имя ресурса>Оборот (Turnover)`, `<Имя ресурса>КонечныйОстаток
  (ClosingBalance)` and their forty-odd siblings. Like `База`, each is
  glued to a resource name and is never a token on its own.
- `Mdo*` other than `MdoExternalDataSource` — Slice 3b.
- `Fn*` — Slice 4.
- `LBrace` / `RBrace` — unattested and unowned; recorded by Slice 3b
  § Unowned brace tokens and recommended as Slice 1-addendum.
- Parser-side grammar and `sdbl-hir`. The converter arm named under
  § Scope is the sole parser-crate edit and carries no grammar decision.

### Two scope bullets that turned out to be empty

The master document's Slice 5 scope names four things. Two of them do
not exist at the lexer layer, and saying so is part of closing the
slice honestly rather than quietly delivering three quarters of it:

- **"`DOT`-sensitive table resolution."** The lexer is not
  dot-sensitive anywhere. `Dot` is an ordinary punctuation token, and
  `tokenize_sdbl` keeps no record of what preceded the current
  position. Deciding that `Остатки` after two dots names a virtual
  table, and after none is an ordinary identifier, needs the token's
  position in a source expression — which is the parser's knowledge.
  The same conclusion drove the Slice 4 treatment of `Лев` / `Left`.
- **"Any special field names that currently require dedicated lexer
  states."** There is exactly one mode in the crate, `strings_mode`,
  and it exists for quoted strings; it is Slice 1 material and Slice 5
  does not touch it. No field name requires a state, because no field
  name is a token.

Both bullets were written before the lexer was audited. They describe a
design the lexer does not have, and the correct outcome is to record
that, not to invent state to satisfy the checklist.

## Per-variant tier source map

All 18 variants are classified **Tier A1** with the 1C:Enterprise 8.3.27
syntax assistant section «Работа с запросами → Таблицы запросов» as the
primary canonical source — the same section Slice 3b used for table
roots. Each virtual table has its own article, and each article headline
carries the whole dotted path in the bilingual form
`<Русский путь> (<English path>)`, which attests both spellings of a
suffix in one canonical line.

| Variant | RU canonical | EN canonical | Article headline |
|---|---|---|---|
| `MdoExternalDataSource` | `ВнешнийИсточникДанных` | `ExternalDataSource` | `ВнешнийИсточникДанных.<Имя внешнего источника данных>.Таблица.<Имя таблицы> (ExternalDataSource.<Имя внешнего источника данных>.Table.<Имя таблицы>)` |
| `VtSliceFirst` | `СрезПервых` | `SliceFirst` | `РегистрСведений.<Имя регистра сведений>.СрезПервых (InformationRegister.<Имя регистра сведений>.SliceFirst)` |
| `VtSliceLast` | `СрезПоследних` | `SliceLast` | `РегистрСведений.<Имя регистра сведений>.СрезПоследних (InformationRegister.<Имя регистра сведений>.SliceLast)` |
| `VtBalance` | `Остатки` | `Balance` | `РегистрНакопления.<Имя регистра накопления>.Остатки (AccumulationRegister.<Имя регистра накопления>.Balance)` |
| `VtTurnovers` | `Обороты` | `Turnovers` | `РегистрНакопления.<Имя регистра накопления>.Обороты (AccumulationRegister.<Имя регистра накопления>.Turnovers)` |
| `VtBalanceAndTurnovers` | `ОстаткиИОбороты` | `BalanceAndTurnovers` | `РегистрНакопления.<Имя регистра накопления>.ОстаткиИОбороты (AccumulationRegister.<Имя регистра накопления>.BalanceAndTurnovers)` |
| `VtDrCrTurnovers` | `ОборотыДтКт` | `DrCrTurnovers` | `РегистрБухгалтерии.<Имя регистра бухгалтерии>.ОборотыДтКт (AccountingRegister.<Имя регистра бухгалтерии>.DrCrTurnovers)` |
| `VtExtDimensions` | `Субконто` | `ExtDimensions` | `РегистрБухгалтерии.<Имя регистра бухгалтерии>.Субконто (AccountingRegister.<Имя регистра бухгалтерии>.ExtDimensions)` |
| `VtRecordsWithExtDimensions` | `ДвиженияССубконто` | `RecordsWithExtDimensions` | `РегистрБухгалтерии.<Имя регистра бухгалтерии>.ДвиженияССубконто (AccountingRegister.<Имя регистра бухгалтерии>.RecordsWithExtDimensions)` |
| `VtScheduleData` | `ДанныеГрафика` | `ScheduleData` | `РегистрРасчета.<Имя регистра расчета>.ДанныеГрафика (CalculationRegister.<Имя регистра расчета>.ScheduleData)` |
| `VtAdjustedEffectivePeriod` | `ФактическийПериодДействия` | `AdjustedEffectivePeriod` | `РегистрРасчета.<Имя регистра расчета>.ФактическийПериодДействия (CalculationRegister.<Имя регистра расчета>.AdjustedEffectivePeriod)` |
| `VtBoundaries` | `Границы` | `Boundaries` | `Последовательность.<Имя последовательности>.Границы (Sequence.<Имя последовательности>.Boundaries)` |
| `VtPoints` | `Точки` | `Points` | `БизнесПроцесс.<Имя бизнес-процесса>.Точки (BusinessProcess.<Имя бизнес-процесса>.Points)` |
| `VtTasksByPerformer` | `ЗадачиПоИсполнителю` | `TasksByPerformer` | `Задача.<Имя задачи>.ЗадачиПоИсполнителю (Task.<Имя задачи>.TasksByPerformer)` |
| `VtTable` | `Таблица` | `Table` | `ВнешнийИсточникДанных.<Имя внешнего источника данных>.Таблица.<Имя таблицы> (ExternalDataSource.<Имя внешнего источника данных>.Table.<Имя таблицы>)` |
| `VtCube` | `Куб` | `Cube` | `ВнешнийИсточникДанных.<Имя внешнего источника данных>.Куб.<Имя куба> (ExternalDataSource.<Имя внешнего источника данных>.Cube.<Имя куба>)` |
| `VtDimensionTable` | `ТаблицаИзмерения` | `DimensionTable` | `ВнешнийИсточникДанных.<Имя внешнего источника данных>.Куб.<Имя куба>.ТаблицаИзмерения.<Имя таблицы> (ExternalDataSource.<Имя внешнего источника данных>.Cube.<Имя куба>.DimensionTable.<Имя таблицы>)` |
| `VtChanges` | `Изменения` (not lexed; see below) | `Changes` | `Справочник.<Имя справочника>.Изменения (Catalog.<Имя справочника>.Changes)` |

The concept itself — that a dotted suffix names a table computed at
query time rather than a stored one — is attested independently by the
ITS query-language textbook chapter «Виртуальные таблицы»
(<https://its.1c.ru/db/pubqlang/content/9/hdoc>), which uses
`РегистрСведений.Цены.СрезПоследних` as its worked example. The
textbook also points at the syntax assistant as the inventory of query
tables (<https://its.1c.ru/db/pubqlang/content/7/hdoc>), which is why
the syntax assistant and not the textbook is the primary source here.

`Субконто` carries a second attestation from the textbook chapter
«Параметр "Субконто"»
(<https://its.1c.ru/db/pubqlang/content/114/hdoc>), and the accounting
family from «Регистры бухгалтерии»
(<https://its.1c.ru/db/pubqlang/content/111/hdoc>).

Bilingualism and case-insensitivity rest where Slice 3b put them:
Developer's Reference Глава 8 §8.2 for «Имя таблицы может быть задано
на английском и русском языках», and §8.4.5 for «Регистр букв (строчные
или заглавные) при написании не имеет значения», which is what the
`(?i)` flag expresses
(<https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>).

## C0a discrepancy audit

The audit runs in three directions, as established by Slice 3a, extended
by Slice 3b and extended again by Slice 4.

### Audit method

For each of the 7 pre-existing variants:

1. Read the current `#[regex(...)]` attribute body verbatim.
2. Locate the «Таблицы запросов» article whose headline contains that
   suffix, and read both halves of its bilingual headline.
3. Compare the regex alternation byte-strings against the headline
   spellings under `(?i)` case folding.

Then, in the opposite direction: enumerate every distinct dotted suffix
in the «Таблицы запросов» section and check that the lexer has a variant
for it.

Then, third: feed every claimed spelling to `tokenize_sdbl` and record
what it actually emits, so that a byte-correct but unreachable
declaration cannot pass as working. This pass is what caught `FnDate` in
Slice 4.

### Audit results — spelling (7/7 MATCH)

No defects. Every regex alternation is the lower-case byte-form of its
canonical headline spelling.

- `MdoExternalDataSource` — regex
  `внешнийисточникданных|externaldatasource`; headline
  `ВнешнийИсточникДанных… (ExternalDataSource…)`. **MATCH.**
- `VtSliceFirst` — regex `срезпервых|slicefirst`; headline
  `…СрезПервых (…SliceFirst)`. **MATCH.**
- `VtSliceLast` — regex `срезпоследних|slicelast`; headline
  `…СрезПоследних (…SliceLast)`. **MATCH.** Second attestation: the
  textbook's worked example `РегистрСведений.Цены.СрезПоследних`.
- `VtBalance` — regex `остатки|balance`; headline `…Остатки
  (…Balance)`. **MATCH.** Note the canonical English is the singular
  `Balance` against the plural Russian `Остатки`; the regex is correct.
- `VtTurnovers` — regex `обороты|turnovers`; headline `…Обороты
  (…Turnovers)`. **MATCH.**
- `VtBalanceAndTurnovers` — regex `остаткииобороты|balanceandturnovers`;
  headline `…ОстаткиИОбороты (…BalanceAndTurnovers)`. **MATCH.** The
  Russian form has the conjunction `И` inside the word, giving the
  doubled `ии`; the regex reproduces it.
- `VtDrCrTurnovers` — regex `оборотыдткт|drcrturnovers`; headline
  `…ОборотыДтКт (…DrCrTurnovers)`. **MATCH.**

### Audit results — coverage (12 gaps, 11 addable)

The «Таблицы запросов» section defines 18 distinct dotted suffixes. The
lexer carries 6. Twelve have no lexer variant and therefore lex as bare
`Ident`:

| Missing suffix | RU | EN | Table that carries it |
|---|---|---|---|
| ext dimensions | `Субконто` | `ExtDimensions` | accounting register |
| records with ext dimensions | `ДвиженияССубконто` | `RecordsWithExtDimensions` | accounting register |
| schedule data | `ДанныеГрафика` | `ScheduleData` | calculation register |
| adjusted effective period | `ФактическийПериодДействия` | `AdjustedEffectivePeriod` | calculation register |
| boundaries | `Границы` | `Boundaries` | sequence |
| route points | `Точки` | `Points` | business process |
| tasks by performer | `ЗадачиПоИсполнителю` | `TasksByPerformer` | task |
| external table | `Таблица` | `Table` | external data source |
| cube | `Куб` | `Cube` | external data source |
| cube dimension table | `ТаблицаИзмерения` | `DimensionTable` | cube |
| change registration | `Изменения` | `Changes` | fourteen roots |
| base register | `База<Имя>` | `Base<Имя>` | calculation register |

Eleven of the twelve are added. The twelfth, `База<Имя>`, is rejected
with a measured reason; see § The `База` prefix is not a token.

`Изменения (Changes)` is the widest of them: the section gives it its
own article under fourteen roots — catalog, document, chart of
characteristic types, chart of accounts, chart of calculation types,
constant, sequence, information register, accumulation register,
accounting register, calculation register, calculation-register
recalculation, business process and task. It is also the only one with a
spelling collision; see § Spellings shared with other vocabularies.

The external-data-source family is the other notable cluster. Slice 3b
deferred `MdoExternalDataSource` to this slice on the grounds that
external sources are its subject, and the deferral turns out to have
been the right call: the root is useless on its own, because every
external-source table path continues through `Таблица`, `Куб` or
`Куб.<Имя>.ТаблицаИзмерения`, and none of those three had a variant.

### The `База` prefix is not a token

`РегистрРасчета.<Имя>.База<Имя базового регистра расчета>` is a
documented virtual table, but its name has no standalone spelling: the
word `База` is glued to the base register's name, giving identifiers
like `БазаОсновныеНачисления`.

Measured before any edit:

```
РегистрРасчета.Начисления.БазаОсновныеНачисления
  -> [MdoCalculationRegister, Dot, Ident, Dot, Ident]
```

The trailing component is **one** `Ident`. A `база|base` alternation
could not change that: logos takes the longest match, and `Ident`
matches all 22 characters where `база` matches 4. The variant would fire
only on a bare `База`, which the grammar never produces in that
position — it would be born unreachable, exactly like the `FnDate` that
Slice 4 removed.

The same reasoning excludes the resource-derived field suffixes
`<Имя ресурса>Остаток`, `<Имя ресурса>Оборот`,
`<Имя ресурса>КонечныйОстаток` and their siblings, which the section
also documents bilingually. Recognising `База<Имя>` as a virtual table
is name resolution over a metadata catalogue, not tokenisation; it
belongs above the lexer, and this attestation records it as such rather
than leaving a dead variant behind to look like coverage.

## Spellings shared with other vocabularies

`Изменения (Changes)` collides byte for byte with `KwUpdate`, the
`ДЛЯ ИЗМЕНЕНИЯ (FOR UPDATE)` clause keyword owned by Slice 2 — but only
on the Russian side. The English spellings differ: `Changes` against
`UPDATE`.

Measured before any edit:

```
Справочник.Товары.Изменения -> [MdoCatalog, Dot, Ident, Dot, KwUpdate]
ДЛЯ ИЗМЕНЕНИЯ               -> [KwFor, Whitespace, KwUpdate]
Changes                     -> [Ident]
```

This is the mirror image of the Slice 4 `Лев (Left)` / `Прав (Right)`
case, where the collision was on the English side and the Russian
spelling was free. The resolution follows the same rule (repository
owner's decision, 2026-07-27): **the colliding spelling stays with the
incumbent owner, and the free spelling gets the new variant.**
`VtChanges` therefore carries an English-only alternation, `Изменения`
keeps emitting `KwUpdate`, and the parser separates the two readings by
position — a clause keyword follows `ДЛЯ`, a table suffix follows a dot.

The asymmetry is real and is not hidden: `Справочник.Товары.Изменения`
and `Catalog.Goods.Changes` reach the parser with different token kinds
for the same construct. A future parser-side slice implementing the
change-registration table must accept both.

## Behaviour change

**Additive, parser-tree-invariant.**

Eleven new `SdblTokenKind` variants change what the lexer emits for
twenty-one spellings that previously emitted `Ident`. Nothing downstream
observes the difference, for two reasons:

1. `SdblTokenKind` has exactly two consumers in the workspace — the
   `lexer` crate itself and `crates/parser/src/sdbl_token_converter.rs`.
   No other crate names the type.
2. The converter maps every `Vt*` variant to `TokenKind::Ident` through
   a single shared match arm. Appending the eleven new variants to that
   arm reproduces, token for token, what the parser saw when these words
   were lexed as `Ident`.

The converter edit is mandatory and cannot be deferred: the match is
exhaustive over `SdblTokenKind`, so adding a variant without extending
the arm does not compile. It is a mechanical edit with no grammar
decision in it.

Longest-match interaction was checked for each new alternation:

- `таблица` vs `таблицаизмерения`, and `table` vs `dimensiontable` —
  the shorter is a proper prefix of the longer, so longest match takes
  the dimension table on its own input and the plain table on its own.
- `таблица` vs `пустаятаблица` (`FnEmptyTable`), and `table` vs
  `emptytable` — matching starts at the token boundary, so the longer
  spellings cannot be entered part way; the pairs never compete.
- `субконто` vs `движенияссубконто` — likewise: `субконто` occurs
  inside the longer word but not at its start.
- `границы`, `точки`, `куб`, `данныеграфика`,
  `фактическийпериоддействия`, `задачипоисполнителю`, `changes`,
  `boundaries`, `points`, `cube`, `scheduledata`,
  `adjustedeffectiveperiod`, `tasksbyperformer`,
  `extdimensions`, `recordswithextdimensions` — no other alternation in
  the enum shares a prefix with any of these.
- Against `Ident`, which matches the same bytes in every case, the
  explicit `priority = 1` on `Ident` yields to the keyword rules, and an
  identifier that merely *starts* with one of these spellings stays an
  identifier because logos prefers the longest overall match. `Куб` and
  `Таблица` are the two entries where that matters most in practice, and
  the acceptance suite pins `КубическийМетр` and `ТаблицаТоваров`.

The golden corpus is the gate: C0b lands corpus entries for all eleven
new suffixes while they still tokenise as `Ident`, so the snapshot
records the pre-change behaviour; C2 flips those snapshot lines to the
new kinds.

### C2 outcome

The measured blast radius matches the C0b bound exactly. Regenerating
the snapshot over the whole 110-entry corpus changed **22 lines and no
others**:

```
-  Ident @170 "Субконто"                  +  VtExtDimensions            @170
-  Ident @250 "ДвиженияССубконто"         +  VtRecordsWithExtDimensions @250
-  Ident @38  "ExtDimensions"             +  VtExtDimensions            @38
-  Ident @77  "RecordsWithExtDimensions"  +  VtRecordsWithExtDimensions @77
-  Ident @72  "ДанныеГрафика"             +  VtScheduleData             @72
-  Ident @150 "ФактическийПериодДействия" +  VtAdjustedEffectivePeriod  @150
-  Ident @43  "ScheduleData"              +  VtScheduleData             @43
-  Ident @86  "AdjustedEffectivePeriod"   +  VtAdjustedEffectivePeriod  @86
-  Ident @58  "Границы"                   +  VtBoundaries               @58
-  Ident @90  "Boundaries"                +  VtBoundaries               @90
-  Ident @153 "Точки"                     +  VtPoints                   @153
-  Ident @189 "Points"                    +  VtPoints                   @189
-  Ident @32  "ЗадачиПоИсполнителю"       +  VtTasksByPerformer         @32
-  Ident @87  "TasksByPerformer"          +  VtTasksByPerformer         @87
-  Ident @76  "Таблица"                   +  VtTable                    @76
-  Ident @39  "Table"                     +  VtTable                    @39
-  Ident @54  "Куб"                       +  VtCube                     @54
-  Ident @76  "ТаблицаИзмерения"          +  VtDimensionTable           @76
-  Ident @153 "Cube"                      +  VtCube                     @153
-  Ident @164 "DimensionTable"            +  VtDimensionTable           @164
-  Ident @28  "Changes"                   +  VtChanges                  @28
-  Ident @64  "Changes"                   +  VtChanges                  @64
```

Every claim the audit makes is visible in what did **not** change:

- No pre-existing token kind moved and no offset shifted.
- Entry 102 still renders `БазаОсновныеНачисления` and
  `BaseMainAccruals` as single `Ident` tokens, so refusing the `База`
  variant is measured rather than asserted.
- Entry 109 still renders `KwUpdate` for both `Изменения` positions, so
  giving `VtChanges` an English-only alternation left `ДЛЯ ИЗМЕНЕНИЯ`
  and `Справочник.Товары.Изменения` exactly where they were.
- Entry 110 still renders `Ident` for `КубическийМетр`,
  `ТаблицаТоваров`, `ГраницыДиапазона`, `ТочкаМаршрута`, `Changeset`
  and `Tableau` — the six identifiers that share a prefix with a new
  suffix.
- Entries 096–098 keep the six pre-existing suffixes on their old
  kinds, so the preserved regexes accept exactly the text they accepted
  before.
- In entry 107 the six-component external-source path resolves
  `ТаблицаИзмерения` to `VtDimensionTable` while entry 105 resolves the
  bare `Таблица` to `VtTable`, confirming the longest-match reading of
  that pair.

`SdblTokenKind` goes from 170 variants to 181, of which 17 are `Vt*`.

Test counts after C2: `lexer` 277 passing (unchanged — the corpus
snapshot is a single test whose content changed), `parser` 596 passing
(unchanged), `clippy -p lexer -p parser --all-targets --all-features
-- -D warnings` clean. The unchanged parser count is the empirical form
of the parser-tree-invariance claim.

## Marker restoration

Slices 3b and 4 restored the provenance map that `3aa29b99` erased from
`crates/lexer/src/sdbl/mod.rs`. Slice 5 retires the last `LEGACY`
banner in the file and replaces it with a `CLEAN-ROOM Slice 5` banner
plus per-variant provenance docstrings.

After this slice the only marker left describing unattested material is
the `LEGACY (unowned)` note over `LBrace` / `RBrace`, which Slice 3b
placed deliberately and which Slice 5 does not touch.

Banners and per-variant docstrings for the closed Slices 1, 2,
2-addendum and 3a remain **not** restored, and the module docstring
continues to say so, so that a missing banner is not read as a missing
attestation.

## Sources consulted

The Slice 5 material was re-derived from:

1. **1C:Enterprise 8.3.27 syntax assistant**, section «Работа
   с запросами → Таблицы запросов» (ships with the platform as
   `shcntx_ru.hbk`; the same artefact the repository already uses as the
   provenance base for `crates/bsl-platform/data/platform_data.json`,
   see `crates/bsl-platform/data/PROVENANCE.md`). Each virtual-table
   article's bilingual headline is the canonical attestation for one
   suffix. This is the primary source: it is the only one that attests
   all 18 Russian/English pairs.
2. **1C:Enterprise 8.3.27 syntax assistant**, book «Синтаксис текста
   запросов» (ships as `shquery_ru.hbk`, with the English build as
   `shquery_root.hbk`) — consulted for its article «Двуязычное
   представление ключевых слов», which establishes two negatives that
   matter here. None of the eighteen virtual-table suffixes appears in
   that table at all, confirming that suffixes are table-name
   components rather than keywords. And its only change-related row is
   the phrase `ДЛЯ ИЗМЕНЕНИЯ | FOR UPDATE [OF]`, not a bare
   `ИЗМЕНЕНИЯ` — so the word standing alone as a keyword comes from the
   Developer's Reference §8.4.5 rendering `ИЗМЕНЕНИЯ | UPDATE`, which
   is what `KwUpdate` was derived from.
3. **v8.3.27 Developer's Reference, Глава 8 «Работа с запросами»** —
   <https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>:
   - §8.2 «Источники данных (таблицы) запросов» — the statement that
     table names exist in both languages;
   - §8.4.5 bilingual keyword table and its case-insensitivity note —
     the basis for the `(?i)` flag.
4. **1C ITS pubqlang textbook** (secondary, corroborating) —
   <https://its.1c.ru/db/pubqlang/content/9/hdoc> («Виртуальные
   таблицы», the concept and the `СрезПоследних` worked example),
   <https://its.1c.ru/db/pubqlang/content/7/hdoc> (source tables for
   queries, which names the syntax assistant as the inventory),
   <https://its.1c.ru/db/pubqlang/content/111/hdoc> (accounting
   registers) and <https://its.1c.ru/db/pubqlang/content/114/hdoc>
   (the `Субконто` parameter).
5. The Slice 1, 2, 2-addendum, 3a, 3b and 4 clean-room material already
   present in `crates/lexer/src/sdbl/mod.rs` — consulted only for the
   existing `Ident` regex priority shape, the per-variant docstring
   format, and the convenience-index style.

Per the citation policy adopted in Slice 8-addendum and reaffirmed in
every slice since, committed artefacts cite only public ITS URLs and
named document sections; local mirror paths are working convenience only
and appear nowhere in this document, in source provenance comments, or
in commit messages. The syntax assistant has no public per-article URL,
so it is cited by product, version and section path.

## Non-consultation statement

During the authorship of the Slice 5 material the following sources
were not used as working text:

- the sibling `bsl-parser` project — neither its grammar files nor its
  token inventory were consulted;
- `bsl-language-server` — neither its source tree nor its diagnostics;
- the "Comparison against the upstream grammars" section of
  `sdbl-provenance-2026-07-audit.md`, which is under read-quarantine for
  clean-room authors by that document's own read-order warning;
- the pre-clean-room regex text of the Slice 5 variants themselves
  beyond reading them once at C0a for the discrepancy audit;
- any other third-party SDBL grammar, token inventory, or parser.

The byte-identity golden corpus
(`crates/lexer/tests/sdbl_golden_corpus.rs`) is the verification gate
that the preserved regex patterns accept exactly the same text set as
before, and that the added patterns change exactly the token kinds the
audit intends and nothing else.

## Verification recipe

All of the following must be green before this attestation is considered
live. Pre-Slice-5 baseline test counts (post-Slice-4, `develop` as of
2026-07-27) are pinned to detect silent test regressions:

1. `cargo test -p lexer --lib` — **70 passed** pre-Slice-5.
2. `cargo test -p lexer --test sdbl_slice1_core` — **34 passed**
   pre-Slice-5; expected unchanged (Slice 1 is closed).
3. `cargo test -p lexer --test sdbl_slice2_keywords` — **43 passed**
   pre-Slice-5; expected unchanged. This suite is the guard on the
   `Изменения` decision: `KwUpdate` is Slice 2 material and must not
   move.
4. `cargo test -p lexer --test sdbl_slice2_addendum_clause_keywords` —
   **30 passed** pre-Slice-5; expected unchanged.
5. `cargo test -p lexer --test sdbl_slice3a_types` — **25 passed**
   pre-Slice-5; expected unchanged.
6. `cargo test -p lexer --test sdbl_slice3b_metadata_objects` —
   **49 passed** pre-Slice-5; expected unchanged.
7. `cargo test -p lexer --test sdbl_slice4_functions` — **25 passed**
   pre-Slice-5; expected unchanged. `FnEmptyTable` lives here and shares
   a substring with the new `VtTable`.
8. `cargo test -p lexer --test sdbl_golden_corpus` — **1 passed**
   (single snapshot test) throughout; the snapshot content changes at
   C0b and again at C2.
9. `cargo test -p lexer --test sdbl_slice5_virtual_tables` — file does
   **not exist** pre-Slice-5; **23 passed** post-Slice-5.
10. `cargo test -p lexer --tests` — **277 passed** pre-Slice-5,
    **300 passed** post-Slice-5 (277 + 23, matching item 9).
11. `cargo test -p parser` — **596 passed** both before and after; the
    converter edit is additive and every `Vt*` variant maps to the same
    `TokenKind::Ident` before and after.
12. `cargo build --workspace --all-targets`.
13. `cargo clippy -p lexer -p parser --all-targets --all-features
    -- -D warnings`.

## Pre-C0b corpus coverage audit

Coverage of the claimed spellings in the pre-Slice-5 corpus
(`crates/lexer/tests/fixtures/sdbl_golden_corpus.txt`). Entry 048 sweeps
the English spellings of the six existing suffixes and entry 047 the
English external-source root; the Russian side is entirely absent, and
the eleven new suffixes are absent in every language.

| Variant | RU in corpus? | EN in corpus? |
|---|---|---|
| `MdoExternalDataSource` | ❌ MISSING | ✓ 047 |
| `VtSliceFirst` | ❌ MISSING | ✓ 048 |
| `VtSliceLast` | ❌ MISSING | ✓ 048 |
| `VtBalance` | ❌ MISSING | ✓ 048 |
| `VtTurnovers` | ❌ MISSING | ✓ 048 |
| `VtBalanceAndTurnovers` | ❌ MISSING | ✓ 048 |
| `VtDrCrTurnovers` | ❌ MISSING | ✓ 048 |
| `VtExtDimensions` | ❌ MISSING | ❌ MISSING |
| `VtRecordsWithExtDimensions` | ❌ MISSING | ❌ MISSING |
| `VtScheduleData` | ❌ MISSING | ❌ MISSING |
| `VtAdjustedEffectivePeriod` | ❌ MISSING | ❌ MISSING |
| `VtBoundaries` | ❌ MISSING | ❌ MISSING |
| `VtPoints` | ❌ MISSING | ❌ MISSING |
| `VtTasksByPerformer` | ❌ MISSING | ❌ MISSING |
| `VtTable` | ❌ MISSING | ❌ MISSING |
| `VtCube` | ❌ MISSING | ❌ MISSING |
| `VtDimensionTable` | ❌ MISSING | ❌ MISSING |
| `VtChanges` | n/a (Russian spelling stays `KwUpdate`) | ❌ MISSING |

Blind spots requiring corpus gap-fill at C0b — 28 in total: 7 Russian
spellings of the existing variants, both spellings of each of the ten
new bilingual suffixes, and the English `Changes`. The new spellings
enter the corpus at C0b **while they still lex as `Ident`**, so that the
C2 snapshot diff is the record of the behaviour change.

`База<Имя>` gets a corpus entry too, even though no variant is added:
the entry pins that the glued form stays a single `Ident`, which is the
claim § The `База` prefix is not a token rests on.

### C0b outcome

Fifteen thematic entries landed, numbered 096–110, closing all 28 blind
spots:

- **096** information register slices russian — `СрезПервых`,
  `СрезПоследних`.
- **097** accumulation register virtual tables russian — `Остатки`,
  `Обороты`, `ОстаткиИОбороты`, the last also guarding longest match
  over the first.
- **098** accounting register virtual tables russian — `ОборотыДтКт`
  next to the two new accounting suffixes `Субконто` and
  `ДвиженияССубконто`, so the entry contrasts them directly.
- **099** accounting register virtual tables english — `ExtDimensions`,
  `RecordsWithExtDimensions`.
- **100** / **101** calculation register virtual tables, russian then
  english — `ДанныеГрафика`, `ФактическийПериодДействия`,
  `ScheduleData`, `AdjustedEffectivePeriod`.
- **102** base register prefix stays one identifier — the entry that
  pins § The `База` prefix is not a token, in both languages.
- **103** sequence boundaries and route points bilingual — `Границы`,
  `Boundaries`, `Точки`, `Points`.
- **104** tasks by performer bilingual — `ЗадачиПоИсполнителю`,
  `TasksByPerformer`.
- **105** / **106** external data source table, russian then english —
  `Таблица`, `Table`, each under the `ВнешнийИсточникДанных` root, whose
  Russian spelling had no corpus entry until now.
- **107** external data source cube dimension table bilingual — the full
  six-component path `…Куб.<Имя>.ТаблицаИзмерения.<Имя>` and its English
  form, which also guards longest match for `ТаблицаИзмерения` over
  `Таблица`.
- **108** change registration table english — `Changes` under two
  different roots.
- **109** change registration russian keeps the update keyword —
  `Справочник.Товары.Изменения` next to `… ДЛЯ ИЗМЕНЕНИЯ`, both
  rendering `KwUpdate`. This entry exists to be *unchanged* by C2.
- **110** identifiers starting with a virtual table suffix —
  `КубическийМетр`, `ТаблицаТоваров`, `ГраницыДиапазона`,
  `ТочкаМаршрута`, `Changeset`, `Tableau`.

The regenerated snapshot confirms all three directions of the C0a audit:

- All 7 previously-unpinned Russian spellings tokenise to their `Vt*` and
  `MdoExternalDataSource` variants, so the preserved regexes accept the
  Russian side of the vocabulary at the byte level.
- All 21 spellings that the slice adds tokenise to `Ident`. Counting
  occurrences rather than spellings, they occupy exactly 22 snapshot
  lines — `Changes` appears twice in entry 108 — so the expected C2 diff
  is 22 lines and no others, bounded by construction rather than by
  assertion.
- Entry 102 renders `БазаОсновныеНачисления` and `BaseMainAccruals` as
  single `Ident` tokens, entry 109 renders `KwUpdate` for both
  `Изменения` positions, and entry 110 renders `Ident` for all six
  prefix-sharing identifiers.

## Commit trail

- `39bee739` (2026-07-27) — C0a: this attestation document authored.
  Sole change: addition of `docs/legal/sdbl-clean-room-slice5.md`.
- `63cf6f76` (2026-07-27) — C0b: corpus entries 096–110 added to
  `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt`; snapshot
  regenerated via `UPDATE_EXPECT=1`. See § C0b outcome.
- `c494f586` (2026-07-27) — C1: module docstring and the `CLEAN-ROOM
  Slice 5` banner replacing the last `LEGACY (Slice 5 pending)` one.
  Comments only: no declaration moved and no regex changed, so the
  golden corpus snapshot needed no regeneration.
- `25b64892` (2026-07-27) — C2: per-variant provenance docstrings and
  the thematic index, the eleven added variants, the converter arm
  extension, and the corpus snapshot flip. See § C2 outcome.
- `56c7f72f` (2026-07-27) — C3: the `sdbl_slice5_virtual_tables.rs`
  acceptance suite (23 tests), this attestation's flip to complete, and
  the master-document Slice 5 section.
- Close-out: a single trailing commit strikes item 3 from the exit
  criteria in `sdbl-provenance-2026-07-audit.md` and pins the C3 SHA
  explicitly in the trail above. The exit-criteria edit is deliberately
  last: that document's § Finding 2 is under read-quarantine for
  clean-room authors, and touching the file surfaces its contents, so
  the edit waits until the authorship it could taint is finished. The
  Anti-Hilbert disclosure: this trailing commit's own SHA is NOT named
  in this enumeration — it cannot be, by construction.

## Licensing note

Slice 5 is the last vocabulary slice on the lexer side, but it does not
by itself promote `crates/lexer` from `LGPL-3.0-or-later` to Tier A
(`MIT OR Apache-2.0`). The brace pair recorded by Slice 3b § Unowned
brace tokens still carries no attestation, and crate-level promotion
additionally requires the parser-side and test-corpus criteria in
[`sdbl-provenance-2026-07-audit.md`](sdbl-provenance-2026-07-audit.md)
§ Exit criteria, including the BSL grammar layer, which shares these
crates and has no slice plan at all.

## Author attestation

The Slice 5 material listed above under **Scope** was authored as a
clean-room re-derivation from the sources listed under **Sources
consulted**, without using the `bsl-parser` project, the
`bsl-language-server` project, the pre-clean-room regex text of the
Slice 5 variants beyond a single read-once for the C0a discrepancy
audit, or any other third-party SDBL grammar as working text. This
attestation applies at the date recorded at the top of the document.
