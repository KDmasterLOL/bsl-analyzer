# SDBL clean-room — Slice 2-addendum (clause keyword leftovers)

## Status

Complete (landed 2026-05-07).

The Slice 2-addendum is a deferred follow-up to the Slice 2 lexer
clean-room (which landed 2026-04-24 and explicitly excluded the
long-tail clause keywords from its `CLEAN-ROOM Slice 2 — structural
keyword vocabulary` banner). The addendum re-authors the 17
clause-level keyword variants under a new
`CLEAN-ROOM Slice 2-addendum — clause keyword leftovers` banner in
`crates/lexer/src/sdbl/mod.rs`, attaches per-variant ITS provenance
comments citing v8327doc Глава 8 plus pubqlang corroborating
chapters, and shrinks the residual LEGACY banner header from
`LEGACY (Slices 3–5 pending)` to `LEGACY (Slices 3, 4, 5 pending —
metadata / function / virtual-table vocabularies)`. The remaining
LEGACY surface after this addendum is exactly: 30 `Fn*` (Slice 4),
6 `Vt*` (Slice 5), 22 `Mdo*` / `Type*` / `LitUndefined` / `Period*`
(Slice 3), and the `Error` fallback — no more clause-shaped
keywords.

The Slice 2-addendum is **lexer-side only**. Parser-side downstream
files that consume these tokens
(`crates/parser/src/grammar/sdbl/**`, `crates/sdbl-hir/**`) remain
unchanged. The parser-side rustdoc comments at
`crates/parser/src/grammar/sdbl/select.rs:1292-1297` and `:1349-1352`
classify FOR UPDATE / INDEX BY as Tier D / Local IDE-recovery
allowance based on a pubqlang-only scan; those classifications are
**stale** — they predate the v8327doc Глава 8 source landing
(Slice 7-addendum, 2026-04-26) which attests both clauses with
canonical EBNF (`page.html:1324, 1328`). Tier-classification flip on
the parser-side rustdoc is documented in §Pre-existing parser-side
stale-classification follow-up below as a known follow-up; it is
out of Slice 2-addendum scope (parser-only edit).

## Scope

The 17 lexer token variants in
`crates/lexer/src/sdbl/mod.rs:470-528` (currently under the
`LEGACY (Slices 3–5 pending — not part of the clean-room claim)`
banner) claimed as Slice 2-addendum authorship:

| # | Variant | Line | Bilingual pair |
|---|---|---|---|
| 1 | `KwDrop` | 479-480 | УНИЧТОЖИТЬ ↔ DROP |
| 2 | `KwAutoOrder` | 482-483 | АВТОУПОРЯДОЧИВАНИЕ ↔ AUTOORDER |
| 3 | `KwAsc` | 485-486 | ВОЗР ↔ ASC |
| 4 | `KwDesc` | 488-489 | УБЫВ ↔ DESC |
| 5 | `KwHierarchy` | 491-492 | ИЕРАРХИЯ ↔ HIERARCHY |
| 6 | `KwAllowed` | 494-495 | РАЗРЕШЕННЫЕ ↔ ALLOWED |
| 7 | `KwFor` | 497-498 | ДЛЯ ↔ FOR |
| 8 | `KwUpdate` | 500-501 | ИЗМЕНЕНИЯ ↔ UPDATE |
| 9 | `KwIndex` | 503-504 | ИНДЕКСИРОВАТЬ ↔ INDEX |
| 10 | `KwOnly` | 506-507 | ТОЛЬКО ↔ ONLY |
| 11 | `KwOverall` | 509-510 | ОБЩИЕ ↔ OVERALL |
| 12 | `KwPeriods` | 512-513 | ПЕРИОДЫ¹ ↔ PERIODS |
| 13 | `KwEscape` | 515-516 | СПЕЦСИМВОЛ ↔ ESCAPE |
| 14 | `KwRefs` | 518-519 | ССЫЛКА ↔ REFS |
| 15 | `KwCast` | 521-522 | ВЫРАЗИТЬ ↔ CAST |
| 16 | `KwType` | 524-525 | ТИП ↔ TYPE |
| 17 | `KwValue` | 527-528 | ЗНАЧЕНИЕ ↔ VALUE |

¹ See §Behaviour change — KwPeriods regex defect.

The clean-room banner block at the new
`CLEAN-ROOM Slice 2-addendum — clause keyword leftovers` position
in `mod.rs`. The residual LEGACY banner header shrinks from
`LEGACY (Slices 3–5 pending)` to `LEGACY (Slices 3, 4, 5 pending —
metadata / function / virtual-table vocabularies)`, leaving only
the `Mdo*` / `Type*` / `Fn*` / `Vt*` / `Period*` / `LitUndefined`
variants and the `Error` fallback in the residual block.

The file-level `## Provenance` docstring at `mod.rs:1-43` is
extended with a new fourth bullet covering the Slice 2-addendum
material:

> - **Slice 2-addendum — clean-room.** Long-tail clause keywords:
>   DROP / AUTOORDER / ASC / DESC / HIERARCHY / ALLOWED / FOR /
>   UPDATE / INDEX / ONLY / OVERALL / PERIODS / ESCAPE / REFS /
>   CAST / TYPE / VALUE. Re-derived from v8327doc Глава 8 «Работа
>   с запросами» (the canonical SDBL grammar specification) with
>   pubqlang corroborating examples. Attested in
>   `docs/legal/sdbl-clean-room-slice2-addendum.md`.

The third bullet (Slices 3–5 pending) is updated to drop the
"long-tail keywords" mention.

**No NodeKinds, no SyntaxKind variants, no public-API surface
changes** — Slice 2-addendum is purely lexer-internal. The token
type names (`KwDrop`, …, `KwValue`) are preserved bit-for-bit; only
`#[regex(...)]` bodies (one regex per variant) and per-variant
docstrings change. The token converter at
`crates/parser/src/sdbl_token_converter.rs:82, 91` and downstream
parser/HIR consumers are unaffected.

## Per-variant tier source map

Tier scheme per Slice 9 / Slice 7-addendum / Slice 8-addendum
precedent:
- **A1** = primary canonical SDBL grammar source attests the
  keyword in a bilingual word-list and/or canonical EBNF/example.
- **A2** = primary source attests prose mention only (no canonical
  syntax form spelled).
- **B** = lexer Slice 2 attested keyword pair (rare for new
  tokens; relevant where a Slice 2 token is reused).
- **C** = local mini-spec / behaviour contract.
- **D** = local IDE-recovery allowance.

All 17 Slice 2-addendum variants are **Tier A1** with v8327doc
Глава 8 «Работа с запросами»
(<https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>) as the
primary canonical SDBL grammar source. Pubqlang chapters cited
where they corroborate with canonical examples beyond the bilingual
word-list.

| # | Variant | Tier | Primary source (v8327doc Глава 8, public URL above) | Pubqlang corroborating |
|---|---|---|---|---|
| 1 | `KwDrop` | A1 | `:1200` Term word-list УНИЧТОЖИТЬ; `:2512` prose; `:2516` canonical syntax `УНИЧТОЖИТЬ ВременнаяТаблица` | pubqlang/51 (:111) + pubqlang/73 (:108) — temp-table lifecycle prose |
| 2 | `KwAutoOrder` | A1 | bilingual word-list slot (АВТОУПОРЯДОЧИВАНИЕ ↔ AUTOORDER) | pubqlang/17 (:17, 32, 52) — canonical bare-keyword form |
| 3 | `KwAsc` | A1 | bilingual word-list slot (ВОЗР ↔ ASC) | pubqlang/16 — ORDER BY direction marker |
| 4 | `KwDesc` | A1 | bilingual word-list slot (УБЫВ ↔ DESC) | pubqlang/16 — ORDER BY direction marker |
| 5 | `KwHierarchy` | A1 | `:3174` canonical EBNF `[[ТОЛЬКО] ИЕРАРХИЯ]` in TOTALS slot; co-located with KwOnly | pubqlang/27 (:39, 51, 71) — `УПОРЯДОЧИТЬ ПО Наименование ИЕРАРХИЯ` (ORDER BY HIERARCHY canonical) |
| 6 | `KwAllowed` | A1 | `:1320` canonical EBNF first-prefix slot; `:1040-1044` bilingual word-list (РАЗРЕШЕННЫЕ ↔ ALLOWED); `:1331-1344` prose semantics. Already Tier-A1-attested by Slice 7-addendum at `docs/legal/sdbl-clean-room-slice7-addendum.md:107-132` | pubqlang/57 (:50) — UI-checkbox prose (secondary) |
| 7 | `KwFor` | A1 | `:1328` canonical EBNF `[ДЛЯ ИЗМЕНЕНИЯ [<Список таблиц верхнего уровня>]]`; `:2230` canonical example | — (verified absent in pubqlang dump 16-39) |
| 8 | `KwUpdate` | A1 | `:650` Term word-list ИЗМЕНЕНИЯ; `:1391` Term ДЛЯ ИЗМЕНЕНИЯ; `:2212` prose; co-located with KwFor canonical EBNF at `:1328` | — |
| 9 | `KwIndex` | A1 | `:1324` canonical EBNF `[ИНДЕКСИРОВАТЬ ПО [НАБОРАМ] <Список полей>]`; `:690` Term word-list ИНДЕКСИРОВАТЬ; `:2354, 2403` canonical examples | — (verified absent in pubqlang dump 16-39) |
| 10 | `KwOnly` | A1 | `:1170` Term word-list ТОЛЬКО; `:3174` canonical EBNF `[[ТОЛЬКО] ИЕРАРХИЯ]`; `:3257, 3387` canonical examples `Номенклатура ТОЛЬКО ИЕРАРХИЯ` | — |
| 11 | `KwOverall` | A1 | bilingual word-list slot (ОБЩИЕ ↔ OVERALL) | pubqlang/39 (:13, 25, 29, 48, 49, 51) — canonical `ИТОГИ ... ПО ОБЩИЕ` |
| 12 | `KwPeriods` | A1 (with regex defect — see §Behaviour change) | `:930, 934` bilingual word-list (**ПЕРИОДАМИ** ↔ **PERIODS**, instrumental case for Russian); `:3174` canonical EBNF `... \| [ПЕРИОДАМИ(<period-type-list>)]`; `:3269` prose; `:3296` canonical example `Период ПЕРИОДАМИ(МИНУТА, ДАТАВРЕМЯ(...), ДАТАВРЕМЯ(...))` | — |
| 13 | `KwEscape` | A1 | `:1090, 1094` bilingual word-list (СПЕЦСИМВОЛ ↔ ESCAPE); `:4971` canonical EBNF `[СПЕЦСИМВОЛ <Литерал типа СТРОКА>]` in LIKE slot; `:5381, 5385` prose | — (verified absent in pubqlang/23, /60) |
| 14 | `KwRefs` | A1 | `:1110` Term word-list ССЫЛКА; `:4969` canonical EBNF `<Выражение> ССЫЛКА <Имя таблицы>`; `:5329, 5343` prose + canonical example | pubqlang/40 (:83, 98) — `(ОстаткиТоваров.Регистратор ССЫЛКА Документ.ПриходнаяНакладная)` |
| 15 | `KwCast` | A1 | `:460` Term word-list ВЫРАЗИТЬ; `:4730` canonical EBNF `ВЫРАЗИТЬ ( <Выражение> КАК <Тип значения> )` | pubqlang/40 (:24, 66, 84-86, 99, 102) — canonical `ВЫРАЗИТЬ(...)` examples + prose |
| 16 | `KwType` | A1 | `:4831` canonical EBNF `ТИП(<Имя типа>)` | — (no pubqlang canonical example located in dumped chapters) |
| 17 | `KwValue` | A1 | bilingual word-list slot (ЗНАЧЕНИЕ ↔ VALUE) | pubqlang/31 (:25, 28) canonical example `Товары.Родитель = ЗНАЧЕНИЕ(Справочник.Товары.ПустаяСсылка)` + prose «литерал функционального типа ЗНАЧЕНИЕ()»; pubqlang/96 (:26) corroborating |

**Tier-classification overrides vs codex C0-plan-review** (recorded
for audit trail):

The codex C0-plan-review classified KwFor / KwUpdate / KwIndex /
KwEscape / KwPeriods / KwDrop / KwType / KwValue as Tier D or B
based on the assumption that pubqlang was the canonical primary
source. This addendum uses **v8327doc Глава 8 as the primary source
per Slice 7-addendum precedent** (the developer's reference is the
authoritative SDBL grammar specification; pubqlang is a textbook
companion). All eight candidates promoted to A1 once v8327doc
verification was performed at C0a:

- KwFor / KwUpdate: v8327doc `:1328` canonical EBNF.
- KwIndex: v8327doc `:1324` canonical EBNF.
- KwEscape: v8327doc `:4971` canonical EBNF.
- KwPeriods: v8327doc `:3174` canonical EBNF (with §Behaviour
  change for the regex defect).
- KwDrop: v8327doc `:2516` canonical syntax + Slice 6 attestation
  `:168-178` already noted that "a tightened rewrite with a
  specific ITS sub-page citation is expected when Slice 3 promotes
  the KwDrop lexer variant out of the LEGACY banner" — this is
  exactly that promotion.
- KwType: v8327doc `:4831` canonical EBNF.
- KwValue: pubqlang/31 canonical example + prose.

The codex C0-plan-review verdict `STRONG: KwRefs is documented
against pubqlang/40, not /22` is incorporated — the original plan's
KwRefs → /22 mapping was a typo; the addendum cites pubqlang/40 as
the corroborating chapter (v8327doc remains primary).

The codex `STRONG: split CAST/TYPE/VALUE grouping` directive is
absorbed — the §Per-variant tier source map treats each of KwCast,
KwType, KwValue as an independent A1 entry (not as a CAST-by-
association group). Each has its own per-variant primary citation.

## Behaviour change

**KwPeriods canonical-spelling regex correction.**

The pre-addendum regex at `mod.rs:512` is
`#[regex(r"(?i)периоды|(?i)periods")]`. The Russian alternation
`(?i)периоды` matches `ПЕРИОДЫ` (nominative case). The canonical
SDBL grammar in v8327doc Глава 8 specifies the keyword in
**instrumental case** — `ПЕРИОДАМИ`:

- `page.html:930` `<span class="Term">ПЕРИОДАМИ</span>` (Russian
  bilingual word-list slot).
- `page.html:934` `<span class="Term">PERIODS</span>` (English
  bilingual word-list slot — matches current regex).
- `page.html:3174` canonical EBNF in TOTALS clause:
  `... | [ПЕРИОДАМИ(Секунда | Минута | ... | Декада | Полугодие
  ...)]`.
- `page.html:3269` prose: «при помощи ключевого слова ПЕРИОДАМИ».
- `page.html:3296` canonical example:
  `Период ПЕРИОДАМИ(МИНУТА, ДАТАВРЕМЯ(2006,6,28),
  ДАТАВРЕМЯ(2006,6,28))`.

C2 changes the regex to:
```rust
#[regex(r"(?i)периодами|(?i)periods")]
KwPeriods,
```

This is **strictly a behaviour change** at the lexer level (the
input `ПЕРИОДЫ` previously tokenized as `KwPeriods`; after the fix
it tokenizes as `Ident`). However the **observable behaviour at the
parser/HIR level is unchanged** because:

- Slice 11 narrowed `TOTALS BY` to a flat-list parser shape and
  explicitly defers structured PERIODS-modifier handling to Slice
  12 (see `docs/legal/sdbl-select-mini-spec.md:861-873` and
  Slice 11 attestation §What is NOT supported in Slice 11).
- The token converter at
  `crates/parser/src/sdbl_token_converter.rs` maps `KwPeriods →
  TokenKind::Ident`, so the parser sees an `Ident` regardless of
  the lexer-side classification.
- No HIR consumer references the `KwPeriods` token kind directly.
- Both `ПЕРИОДЫ` (pre-fix) and `ПЕРИОДАМИ` (post-fix) end up as
  bare `Ident` expressions in the parse tree under the
  `select_tail_clauses` body.

The parser-tree output for any input containing either `ПЕРИОДЫ`
or `ПЕРИОДАМИ` is identical before and after the fix — only the
lexer-internal token classification flips.

C2 also updates the existing golden-corpus expectation at:
- `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt:80` —
  current entry uses `ПЕРИОДЫ(ДЕНЬ, ДАТАВРЕМЯ(...), &End)`. C2
  changes the corpus entry to `ПЕРИОДАМИ(ДЕНЬ, ДАТАВРЕМЯ(...),
  &End)` to use the canonical Russian spelling.
- `crates/lexer/tests/sdbl_golden_corpus.rs:484` — current
  expectation `KwPeriods @78 "ПЕРИОДЫ"`. C2 updates the
  expectation alongside the corpus byte-string change.

C2 also creates
`crates/lexer/tests/sdbl_slice2_addendum_clause_keywords.rs`
(the file is born at C2 with the regex fix; per Slice 7-addendum
/ Slice 8-addendum precedent the new acceptance file is born no
earlier than the slice's clean-room rewrite phase) and adds
three regression-gate tests:

1. `ПЕРИОДАМИ` tokenizes as `KwPeriods` (canonical Russian).
2. `PERIODS` tokenizes as `KwPeriods` (English unchanged).
3. `ПЕРИОДЫ` tokenizes as `Ident` (legacy misspelling now falls
   through to identifier).

Decision rationale (C0a, with codex consult on 2026-05-07): Option
A (fix to canonical) selected over Option B (accept both) and
Option C (preserve and defer) because:

- Slice 2-addendum's purpose is clean-room re-derivation from
  canonical sources; preserving a documented misspelling against
  v8327doc canonical EBNF undermines the clean-room claim itself.
- Observable parse-tree impact is zero (see above).
- C2's clean-room latitude (per Slice 7-addendum precedent at
  `docs/legal/sdbl-clean-room-slice7-addendum.md:294-301`)
  accommodates regex byte changes when grounded in primary source.
- Option B (accept both) creates ambiguity about which form is
  canonical and adds an IDE-recovery allowance for a typo nobody
  actually makes.
- Option C (preserve and defer) leaves a primary-source-attested
  defect in clean-room code with deferred-fix language, which
  erodes attestation authority.

The codex consult confirmed Option A and verified by independent
grep that no structural parser/HIR consumer reads `KwPeriods` —
only the lexer variant, the converter mapping, the golden corpus,
and docs. The fix is fully contained at the lexer + corpus + new
acceptance tests; no parser, HIR, or downstream-test edits.

**Behaviour change classification:** lexer-internal,
parser-tree-invariant. Slice 12 (PERIODS structured-handling owner)
inherits a corrected lexer surface.

## C2 priority assertion

For the 17 Slice 2-addendum variants, C2 asserts that **no logos
priority changes are required**. The relevant priority landscape
in `mod.rs`:

- `Ident` regex at `mod.rs:228` carries explicit `priority = 1`
  (lower than the default keyword priority).
- `Fn*` date tokens in the residual LEGACY block (`FnYear`,
  `FnQuarter`, `FnMonth`, `FnDayOfYear`, `FnDay`, `FnWeek`,
  `FnWeekDay`, `FnHour`, `FnMinute`, `FnSecond`, `FnDate`) at
  `mod.rs:545-593` carry explicit `priority = 2`. The priority-2
  bump exists because these date function names overlap with
  period-type tokens via Slice 4's eventual disambiguation (see
  the comment block at `:545-547`).
- The 17 Slice 2-addendum keyword regexes use the **default
  longest-match priority** — same as all 35 Slice 2 keyword
  regexes — and there is no overlap between the addendum
  vocabulary and any priority-2 date function name. Verified by
  pairwise lexicographic comparison: none of the 34 bilingual
  alternations (17 RU + 17 EN) overlap with `FnYear`/`FnQuarter`/
  `FnMonth`/`FnDayOfYear`/`FnDay`/`FnWeek`/`FnWeekDay`/`FnHour`/
  `FnMinute`/`FnSecond`/`FnDate` regex bodies.

C2 preserves default priority on all 17 variants. No
`priority = N` annotations added.

## Pre-existing parser-side stale-classification follow-up

The parser-side rustdoc comments at:

- `crates/parser/src/grammar/sdbl/select.rs:1292-1297`
  (`for_update_clause` Provenance bullet — Tier D / "verified
  absent in dumped ITS chapters 16–39")
- `crates/parser/src/grammar/sdbl/select.rs:1349-1352`
  (`index_by_clause` Provenance bullet — Tier D / "verified absent
  in dumped ITS chapters 16–39")

and the SELECT mini-spec sections at
`docs/legal/sdbl-select-mini-spec.md:759-766` (FOR UPDATE §ITS
coverage) and `:785-789` (INDEX BY §ITS coverage) classify FOR
UPDATE / INDEX BY as Tier D based on a pubqlang-only scan. With
the v8327doc Глава 8 source landed (Slice 7-addendum, 2026-04-26),
both clauses are now Tier A1 — v8327doc `:1324` and `:1328` carry
canonical EBNF for both.

This follow-up is **out of Slice 2-addendum scope** (lexer-only
edit). The parser-side rustdoc tier-classification flip should
land in a separate parser-only follow-up commit referencing this
addendum — likely candidate timing: alongside Slice 12 work or as
a documentation-only commit.

The same is true for the §IDE-recovery allowance language around
`KwPeriods` — once the regex defect is fixed at C2 the
"Tier D / NOT verified" prose at
`docs/legal/sdbl-select-mini-spec.md:882-884` may merit revisiting
when Slice 12 promotes structured PERIODS handling.

## Sources consulted

The Slice 2-addendum material was authored from:

1. **Primary** SDBL grammar specification: v8.3.27 Developer's
   Reference Глава 8 «Работа с запросами» —
   <https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>.
   Per the project's citation policy
   (`docs/legal/sdbl-clean-room-slice8-addendum.md` precedent)
   only the public URL above is cited as the canonical source;
   line numbers are reviewer-convenience references against a
   local snapshot. Specifically:
   - `:460` — Term ВЫРАЗИТЬ.
   - `:650` — Term ИЗМЕНЕНИЯ.
   - `:690` — Term ИНДЕКСИРОВАТЬ.
   - `:920-924` — bilingual ПЕРВЫЕ ↔ TOP (cross-reference, Slice
     7-addendum already attested).
   - `:930-934` — bilingual ПЕРИОДАМИ ↔ PERIODS.
   - `:1030-1034` — bilingual РАЗЛИЧНЫЕ ↔ DISTINCT (cross-
     reference, Slice 2 already attested).
   - `:1040-1044` — bilingual РАЗРЕШЕННЫЕ ↔ ALLOWED
     (cross-reference, Slice 7-addendum already attested).
   - `:1090-1094` — bilingual СПЕЦСИМВОЛ ↔ ESCAPE.
   - `:1110` — Term ССЫЛКА.
   - `:1170` — Term ТОЛЬКО.
   - `:1200` — Term УНИЧТОЖИТЬ.
   - `:1320` — canonical EBNF skeleton for `<Описание запроса>`
     placing РАЗРЕШЕННЫЕ / РАЗЛИЧНЫЕ / ПЕРВЫЕ in the first three
     SELECT-prefix slots.
   - `:1324` — canonical EBNF
     `[ИНДЕКСИРОВАТЬ ПО [НАБОРАМ] <Список полей>]`.
   - `:1328` — canonical EBNF
     `[ДЛЯ ИЗМЕНЕНИЯ [<Список таблиц верхнего уровня>]]`.
   - `:1391` — Term ДЛЯ ИЗМЕНЕНИЯ (combined-term entry).
   - `:2212-2230` — FOR UPDATE prose + canonical example block.
   - `:2341-2428` — INDEX BY prose + canonical examples
     `ИНДЕКСИРОВАТЬ ПО Поле` + `ИНДЕКСИРОВАТЬ ПО НАБОРАМ ( ... )`.
   - `:2512-2516` — DROP prose + canonical syntax
     `УНИЧТОЖИТЬ ВременнаяТаблица`.
   - `:3174` — canonical EBNF for TOTALS BY group spec
     `<Выражение> [[ТОЛЬКО] ИЕРАРХИЯ] | [ПЕРИОДАМИ(...)]`.
   - `:3240` — ТОЛЬКО prose.
   - `:3257` — canonical example `Номенклатура ТОЛЬКО ИЕРАРХИЯ`.
   - `:3269` — ПЕРИОДАМИ prose.
   - `:3296` — canonical PERIODS example.
   - `:3387` — canonical TOTALS-with-alias example.
   - `:4730` — canonical EBNF
     `ВЫРАЗИТЬ ( <Выражение> КАК <Тип значения> )`.
   - `:4831` — canonical EBNF `ТИП(<Имя типа>)`.
   - `:4969` — canonical EBNF `<Выражение> ССЫЛКА <Имя таблицы>`.
   - `:4971` — canonical EBNF `[СПЕЦСИМВОЛ <Литерал типа СТРОКА>]`
     in LIKE slot.
   - `:5329-5343` — ССЫЛКА prose + canonical example.
   - `:5381-5385` — СПЕЦСИМВОЛ prose.

2. **Secondary corroborating** ITS pubqlang dump (textbook
   companion):
   - pubqlang/16 — ORDER BY direction modifiers (ASC/DESC).
   - pubqlang/17 (`chapter_017.html:17, 32, 52`) — canonical bare
     `АВТОУПОРЯДОЧИВАНИЕ`.
   - pubqlang/27 (`chapter_027.html:39, 51, 71`) — canonical
     `УПОРЯДОЧИТЬ ПО Наименование ИЕРАРХИЯ` (ORDER BY HIERARCHY).
   - pubqlang/31 (`chapter_031.html:25, 28`) — canonical
     `ЗНАЧЕНИЕ(Справочник.Товары.ПустаяСсылка)` + prose
     «литерал функционального типа ЗНАЧЕНИЕ()».
   - pubqlang/39 (`chapter_039.html:13, 25, 29, 48, 49, 51`) —
     canonical `ИТОГИ ... ПО ОБЩИЕ`.
   - pubqlang/40 (`chapter_040.html:24, 66, 83-86, 98, 99, 102`)
     — canonical ВЫРАЗИТЬ() + ССЫЛКА examples and prose.
   - pubqlang/51 (`chapter_051.html:111`) — DROP / УНИЧТОЖИТЬ
     temp-table lifecycle prose (cross-reference, Slice 6
     already noted).
   - pubqlang/57 (`chapter_057.html:50`) — UI-checkbox prose for
     "Разрешенные" (cross-reference, Slice 7-addendum).
   - pubqlang/73 (`chapter_073.html:108`) — DROP /
     УНИЧТОЖИТЬ via temp-table-manager prose.
   - pubqlang/96 (`chapter_096.html:26`) — corroborating
     `ЗНАЧЕНИЕ(Перечисление.ВидыОпераций.ПоступлениеОтПроизводителей)`.

3. The Slice 1, 2, 6, 7, 8, 9, 10a, 10b, 11, 7-addendum, and
   8-addendum clean-room attestations for the established
   tier-classification scheme, the citation-policy rule
   (`feedback_citation_policy.md` — public URL only in
   committed artefacts), per-variant provenance comment shape,
   bilingual-alternation regex shape, and Anti-Hilbert close-out
   convention.

4. The lexer Slice 2 attestation
   (`docs/legal/sdbl-clean-room-slice2.md`) for the file-level
   docstring shape, the `CLEAN-ROOM Slice N` banner format, and
   the bilingual alternation regex pattern
   `(?i)<russian>|(?i)<english>`.

The author did NOT consult `../bsl-parser/*` or any pre-C1 textual
transcription of the 17 Slice 2-addendum lexer regex bodies as
working text during C0 / C2 / C3 authoring. The C1 commit
physically relocates the 17 `#[regex(...)]` declarations out of
the LEGACY block into the new `CLEAN-ROOM Slice 2-addendum —
clause keyword leftovers` banner with `// C1 placeholder —
clean-room rewrite in C2` markers attached; C2 re-derives each
regex pattern from the v8327doc bilingual word-list slots cited
above without using the C1 placeholder bytes as working text.

## Non-consultation statement

The author did not consult any `../bsl-parser/*` file, any prior
textual transcription of the 17 lexer regex bodies, or any
third-party SDBL lexer/parser as working text during the
Slice 2-addendum C0 / C1 / C2 / C3 authoring. The bilingual word
pairs and the `(?i)<russian>|(?i)<english>` regex shape are direct
mechanical applications of v8327doc bilingual word-list content
through the lexer `logos` regex syntax — the same mechanical
shape established by Slice 1 and Slice 2.

## Verification recipe

All of the following must be green before this attestation is
considered live. Test counts pinned against the pre-addendum
baseline measured at C0a (2026-05-07) on `develop` branch tip
`31abfe22`:

1. `cargo test -p lexer --test sdbl_slice1_core` — 34 tests.
2. `cargo test -p lexer --test sdbl_slice2_keywords` — 30 tests.
3. `cargo test -p lexer --test sdbl_golden_corpus` — 1 test
   (single expect-test snapshot over the full fixture file). The
   line 80 entry update (ПЕРИОДЫ → ПЕРИОДАМИ) at C2 keeps the
   test count at 1 but flips the snapshot bytes — the
   `sdbl_golden_corpus.rs:484` expectation must hold against the
   updated corpus.
4. `cargo test -p lexer --test sdbl_slice2_addendum_clause_keywords`
   — new test file. Per Slice 7-addendum / Slice 8-addendum
   precedent (gap-tests in EXISTING test file at C0b, new
   acceptance file at C3) the file was **born at C2** with the 3
   KwPeriods regression-gate tests landing alongside the regex
   fix (canonical / English / legacy-misspelling-now-Ident). C3
   expanded the file into the spec-driven acceptance suite
   covering all 17 variants. Per-phase counts:
   - C0a: file does not exist (count 0).
   - C0b: file does not exist (count 0).
   - C1: file does not exist (count 0).
   - C2: 3 tests (KwPeriods regression gates).
   - C3: 30 tests — 3 KwPeriods regression gates + 16
     bilingual EN+RU variant pairs (KwPeriods covered by the
     regression gates) + 1 case-insensitivity sweep + 9
     structural integration tests (DROP-in-batch,
     ORDER-BY-with-modifiers, TOTALS-PERIODS-canonical,
     FOR-UPDATE, INDEX-BY, LIKE-ESCAPE, REFS, CAST, VALUE) + 1
     keyword-prefix Ident longest-match guard.
5. `cargo test -p lexer` — full lexer suite, pre-addendum
   baseline 132 tests (65 lib unit tests in `src/` + 1
   `sdbl_golden_corpus` + 34 `sdbl_slice1_core` + 30
   `sdbl_slice2_keywords` + 2 doc-tests). Per-phase deltas:
   - C0b: Bucket-A RU gap tests added to
     `sdbl_slice2_keywords.rs` (`sdbl_slice2_keywords` count
     30 → 43). Lexer suite: 132 → 145.
   - C2: `sdbl_slice2_addendum_clause_keywords.rs` created with
     3 regression gates; line 80 corpus entry + line 484
     expectation updated in-place (golden_corpus test count
     stays at 1). Lexer suite: 145 → 148.
   - C3: `sdbl_slice2_addendum_clause_keywords.rs` expanded
     from 3 → 30 tests. Lexer suite: 148 → 175 (65 lib + 1
     golden_corpus + 34 slice1 + 30 slice2_addendum + 43
     slice2_keywords + 2 doc-tests).
6. `cargo test -p parser` — full parser suite (regression gate;
   no parser changes expected). Per-file pre-addendum baseline:
   `sdbl_parser_tests` 204, `sdbl_slice6_package` 26,
   `sdbl_slice7_fields` 33, `sdbl_slice8_sources` 28,
   `sdbl_slice9_joins` 17, `sdbl_slice10a_backbone` 33,
   `sdbl_slice10b_predicates` 43, `sdbl_slice11_clauses` 35,
   `sdbl_slice7_addendum_limitations` 13,
   `sdbl_slice8_addendum_virtual_table_args` 19. None of these
   counts change at any phase of the addendum (no parser file is
   edited; the converter at `sdbl_token_converter.rs` already
   maps `KwPeriods → Ident` so the parse tree is invariant under
   the regex flip).
7. `cargo test -p sdbl-hir` — HIR regression gate; pre-addendum
   baseline 207 tests. No HIR consumer references KwPeriods
   directly; count unchanged at every phase.
8. `cargo test -p ide-diagnostics` — IDE-diagnostics regression
   gate; pre-addendum baseline 1609 tests. Count unchanged.
9. `cargo test -p ide` — full IDE test suite; pre-addendum
   baseline 6 tests in the main suite. Count unchanged.
10. `cargo test --workspace` — full workspace regression gate.
11. `cargo build --workspace --all-targets` — workspace build.
12. `cargo clippy -p lexer --all-targets --all-features -- -D
    warnings` — lexer clippy clean.
13. `git log --follow crates/lexer/src/sdbl/mod.rs` — shows C1
    + C2 as separate commits; C0a authors only this attestation
    document (no source / test changes); C0b authors only
    test-side files (gap tests in
    `crates/lexer/tests/sdbl_slice2_keywords.rs` and corpus
    fixture additions); C1 banner-relocates regex declarations;
    C2 lands the regex rewrite + KwPeriods canonical fix +
    creates the new acceptance test file with 3 regression
    gates; C3 finalises the attestation + expands the
    acceptance suite + master-doc flip.
14. `git diff develop..HEAD --stat` — exactly 8 files at addendum
    landing:
    - `crates/lexer/src/sdbl/mod.rs` (C1 banner relocate + C2
      regex rewrite + per-variant provenance comments + file-
      level docstring extension).
    - `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt` (C0b
      13 thematic entries (058–070, covering 13 RU blind-spot
      variants + 3 EN gap-fillers) + C2 line 80 ПЕРИОДЫ →
      ПЕРИОДАМИ canonical-spelling update).
    - `crates/lexer/tests/sdbl_golden_corpus.rs` (C2 expectation
      update for the corpus byte-string change at line 484).
    - `crates/lexer/tests/sdbl_slice2_keywords.rs` (C0b 13 RU
      Bucket-A gap-test functions added).
    - `crates/lexer/tests/sdbl_slice2_addendum_clause_keywords.rs`
      (C2 file creation with 3 KwPeriods regression gates + C3
      expansion to spec-driven acceptance suite).
    - `docs/legal/sdbl-clean-room-slice2-addendum.md` (this
      document — C0a creation, C2 / C3 finalisation).
    - `docs/legal/sdbl-clean-room-slices.md` (C3 master-doc
      addition of Slice 2-addendum section).
    - `docs/legal/sdbl-clean-room-slice2.md` (C3 §Scope flip
      acknowledging the addendum).

    No other paths.

## Commit trail

- `7a6baf09` (2026-05-07) — C0a: this attestation document
  created with full §Scope, §Per-variant tier source map,
  §Behaviour change (KwPeriods regex defect — Option A
  decision), §C2 priority assertion, §Pre-existing parser-side
  stale-classification follow-up, §Sources consulted (v8327doc
  Глава 8 + 10 corroborating pubqlang chapters), §Non-
  consultation statement, §Verification recipe; codex-consult
  Option-A verdict for KwPeriods regex fix recorded in
  §Behaviour change. No production-code changes; no test
  changes (those land in C0b).
- `768704a6` (2026-05-07) — C0b: 13 RU Bucket-A gap-test functions
  added to `crates/lexer/tests/sdbl_slice2_keywords.rs` covering
  the RU spelling blind spots (`sdbl_slice2_keywords` count 30 →
  43). 13 thematic corpus entries added to
  `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt` (entries
  058–070, covering all 13 RU spelling blind spots + 3 EN
  gap-fillers for KwDrop / KwAutoOrder / KwPeriods — actual
  entry count 13 with thematic merging per the §Pre-C0b corpus
  coverage audit allowance). The KwPeriods corpus entry preserves the legacy
  `ПЕРИОДЫ` spelling at C0b as the Bucket-A baseline pin; the
  canonical-spelling flip to `ПЕРИОДАМИ` lands at C2 alongside
  the regex fix. No production-code changes; no new test files
  (per Slice 7/8-addendum precedent).
- `9f535e0d` (2026-05-07) — C1: pure-refactor relocation of the 17
  `#[regex(...)]` declarations out of the
  `LEGACY (Slices 3–5 pending)` block into a new
  `CLEAN-ROOM Slice 2-addendum — clause keyword leftovers`
  banner; LEGACY banner header shrunk to `LEGACY (Slices 3, 4,
  5 pending — metadata / function / virtual-table
  vocabularies)`; per-variant `// C1 placeholder — clean-room
  rewrite in C2` markers attached; module-level
  `## Provenance` docstring extended with the Slice 2-addendum
  bullet (no attestation citation per forward-reference
  prohibition; flipped to "complete" in C3). This commit is the
  safe revert boundary for the clean-room rewrite.
- `4e615e95` (2026-05-07) — C2: re-author the 17 Slice 2-addendum
  variant regex bodies and per-variant docstrings from
  v8327doc Глава 8 + corroborating pubqlang chapters + the
  C0a-extended attestation; attach one per-variant provenance
  comment each; apply the KwPeriods regex defect fix per
  §Behaviour change Option A
  (`(?i)периоды|(?i)periods` → `(?i)периодами|(?i)periods`);
  update `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt:80`
  to use ПЕРИОДАМИ; update `crates/lexer/tests/sdbl_golden_corpus.rs:484`
  expectation accordingly; **create**
  `crates/lexer/tests/sdbl_slice2_addendum_clause_keywords.rs`
  with the 3 KwPeriods regression-gate tests (canonical /
  English / legacy-misspelling-now-Ident). Codex C2 review pass
  per pair-mode protocol.
- `3a95a3b3` (2026-05-07) — C3: this attestation finalised
  (C0a / C0b / C1 / C2 placeholders replaced with actual SHAs),
  the `sdbl_slice2_addendum_clause_keywords.rs` test file
  expanded from the 3 C2-born regression gates to 30
  spec-driven acceptance tests (3 KwPeriods regression gates +
  16 bilingual EN+RU variant pairs + 1 case-insensitivity
  sweep + 9 structural integration tests + 1 keyword-prefix
  Ident longest-match guard), the Slice 2-addendum status
  block added to
  [`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md)
  (placement: after Slice 8-addendum in chronological addendum
  order per codex C0-plan-review NIT), the file-level
  `## Provenance` docstring fourth-bullet flipped from
  "(in progress)" to "(complete)" with attestation citation,
  the Slice 2 attestation § Scope flipped at
  `docs/legal/sdbl-clean-room-slice2.md:41-48` to acknowledge
  the addendum claim of the 17 clause-keyword variants. Codex
  C3 review pass per pair-mode protocol.
- `<HILBERT_COMMIT>` (2026-05-07) — Anti-Hilbert close-out:
  replaced the `<C3_COMMIT>` placeholder in the C3 §Commit
  trail entry above with the actual C3 SHA `3a95a3b3`; fixed
  test-file header drift (29 → 30 acceptance test count). This
  amendment commit is itself self-referential by design — it
  is the only commit whose own SHA does not appear in the
  attestation §Commit trail — mirroring the Slice 7-addendum
  (`cb383521`) and Slice 8-addendum Anti-Hilbert close-outs.

## Pre-C0b corpus coverage audit

`crates/lexer/tests/fixtures/sdbl_golden_corpus.txt` already
covers the EN spelling for 13 of the 17 variants:

- Line 56 (entry 028): `ASC DESC HIERARCHY`.
- Line 64 (entry 032): `LIKE ... ESCAPE`.
- Line 70 (entry 035): `CAST(...)`.
- Line 72 (entry 036): `REFS`.
- Line 78 (entry 039):
  `FOR UPDATE INDEX BY ... ONLY ALLOWED OVERALL`.
- Line 98 (entry 048): `TYPE(...)` + `VALUE(...)`.

RU coverage is partial:

- Line 76 (entry 038): `УНИЧТОЖИТЬ #T` ✓ (KwDrop).
- Line 80 (entry 040): `АВТОУПОРЯДОЧИВАНИЕ ОБЩИЕ ПЕРИОДЫ(...)`
  ✓ (KwAutoOrder, KwOverall, KwPeriods — but ПЕРИОДЫ becomes
  ПЕРИОДАМИ at C2).

C0b adds RU corpus entries for the 13 RU blind spots:
KwAsc (ВОЗР), KwDesc (УБЫВ), KwHierarchy (ИЕРАРХИЯ),
KwAllowed (РАЗРЕШЕННЫЕ), KwFor (ДЛЯ), KwUpdate (ИЗМЕНЕНИЯ),
KwIndex (ИНДЕКСИРОВАТЬ), KwOnly (ТОЛЬКО),
KwEscape (СПЕЦСИМВОЛ), KwCast (ВЫРАЗИТЬ), KwRefs (ССЫЛКА),
KwType (ТИП), KwValue (ЗНАЧЕНИЕ). Plus EN gap-fill for
KwDrop (DROP), KwAutoOrder (AUTOORDER), KwPeriods (PERIODS).

Total C0b corpus addition: ~16 new entries (or merged into
existing thematic entries if structurally close — final count
to be determined at C0b authoring).

## Licensing note

The `crates/lexer` crate retains its current license disposition
until the full Slice 1 → 2 → 2-addendum → 3 → 4 → 5 lexer
clean-room migration is complete and the `LEGACY (Slices 3, 4, 5
pending)` banner is fully retired. Promoting the crate to
Tier A (`MIT OR Apache-2.0`) is explicitly out of scope for the
Slice 2-addendum and will happen once the last LEGACY-banner
content (the `Mdo*`, `Type*`, `Period*`, `LitUndefined`, `Fn*`,
`Vt*`, and `Error` variants) has been re-derived by Slices 3–5.

The 17 Slice 2-addendum tokens enter clean-room status with this
addendum; the residual LEGACY tokens remain Tier B / pre-clean-
room and continue to carry the inherited license disposition
until their respective slices land.

## Author attestation

The Slice 2-addendum material listed above under **Scope** was
authored as a clean-room re-derivation from the sources listed
under **Sources consulted**, without using the `../bsl-parser`
project, the pre-C1 regex bodies of the 17 Slice 2-addendum
lexer variants, or any other third-party SDBL lexer as working
text. This attestation applies at the date recorded at the top
of the document and will be amended with actual commit SHAs at
the C3 / Anti-Hilbert close-out commits.
