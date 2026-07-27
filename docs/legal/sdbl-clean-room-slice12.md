# SDBL Slice 12 — Clean-Room Attestation (recovery and IDE allowances)

**Status:** in progress (C0a, 2026-07-27).

This document attests the clean-room authorship of the Slice 12 material
of the SDBL parser, per the staged migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

Slice 12 is the first slice in this programme whose subject is not a
vocabulary and not a grammar rule, but the parser's behaviour on input
the grammar does not describe. Its stated principle is that every
recovery rule must be documented as either

- required by the official syntax, or
- intentionally kept for editor behaviour.

The slice therefore has a third possible verdict the plan did not
anticipate, and which turns out to carry most of the work: **neither**.
A behaviour that is not required by the syntax and that no one would
choose on purpose is a defect, and calling it an allowance in a legal
document would be a false statement. Four such behaviours were found.

## Why this slice differs from its predecessors

The closed lexer slices re-derived a word list from official
documentation and compared it, spelling by spelling, against the code.
That method does not apply here: there is no vocabulary to compare. The
official grammar says what a well-formed query is and stops. It says
nothing whatever about what a parser should do with a query that is not
well-formed, because that is not a property of the language.

So the audit runs in the opposite direction. Instead of asking "is every
documented word present", it asks, of every behaviour the parser exhibits
on input the grammar does not cover: **is this behaviour chosen?** The
enumeration is of code sites, not of source articles, and the source is
consulted afterwards, to settle whether each site is implementing the
language or departing from it.

One consequence is worth stating plainly, because it inverts the usual
reading of a clean-room audit. In the vocabulary slices, agreement with
the source was the passing result. Here, several sites agree with no
source at all and are still correct — an IDE must do *something* with a
half-typed query, and the language has no opinion on what. The failing
result is not disagreement with the source; it is a behaviour nobody
decided.

## Scope

The paths claimed as clean-room Slice 12 authorship are:

- `crates/parser/src/grammar/sdbl.rs` — `query_package`, and the
  end-of-input drain it lacks;
- `crates/parser/src/grammar/sdbl/select.rs` — the recovery helpers, the
  clause-order driver `select_tail_clauses` / `query_body_clauses`, and
  the two control-point items `order_by_item` / `totals_group_item`;
- `crates/parser/src/grammar/sdbl/expressions.rs` — `recover_to_delimiter`,
  `is_recovery_point`, `parse_delimited_list`'s missing-item branch;
- `docs/legal/sdbl-select-mini-spec.md` — the §TOTALS BY grammar,
  narrowed by Slice 11 to the flat-list shape, and the per-slice
  §IDE-recovery allowance blocks, which this slice consolidates;
- `crates/parser/tests/sdbl_slice12_recovery.rs` — the acceptance suite
  born at C3;
- this document.

### Out of scope, and why

- **`crates/sdbl-hir/**`** — read-only for this slice, as it was for
  Slice 11. Any semantic reading of the modifiers this slice makes
  parseable belongs to Slice 13. The C2 work is therefore constrained to
  keep the existing node kinds: a control-point modifier is consumed as
  a token of `SdblTotalsBy`, not promoted into a node of its own. That
  is the same shape Slice 11 chose for the `HIERARCHY` modifier on
  `ORDER BY`, and it keeps this slice's diff out of the lowerer.
- **Multiline query-string artifacts.** The master document lists these
  as a Slice 12 concern. They are not a grammar concern: a BSL string
  literal is turned into SDBL source text in `crates/syntax/src/sdbl_query.rs`,
  which already returns the offset corrections needed to map a range
  back through doubled quotes and continuation pieces. By the layering
  rule the project works to, that boundary belongs to the syntax layer
  and not to the SDBL grammar, and there is nothing for this slice to
  do there. See § The five scope bullets, item 4.

## Sources

### Primary

The v8.3.27 Developer's Reference, Глава 8 «Работа с запросами» —
`https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453`. Used for:

- §8.4.3 «Комментарии в языке запросов» — the definition of `//`
  comments;
- §8.4.15 «Упорядочивание результатов запроса» — the canonical rule for
  an ordering field and its `<Порядок>` alternatives;
- §8.4.16 «Расчет итогов запроса» — the canonical rule for `ИТОГИ`, its
  control points, the `[ТОЛЬКО] ИЕРАРХИЯ` and `ПЕРИОДАМИ(…)` modifiers,
  and the control-point alias, with worked examples in §8.4.16.3,
  §8.4.16.4, §8.4.16.5 and §8.4.16.7.

### Secondary

The «Язык запросов 1С:Предприятия» textbook, article «Расчет общих
итогов» — `https://its.1c.ru/db/content/pubqlang/src/35.html`. Cited by
Slices 9 and 11 as "chapter 39"; that number is a local reading index,
and the identifier above is the one the article declares for itself.
Used only for the `ОБЩИЕ` question in § Source questions, where it
disagrees with the primary source's own rule.

### Not a source

No behaviour in this slice is derived from a third-party grammar. That
statement is stronger here than in the vocabulary slices and worth
spelling out: recovery is precisely the part of a parser where an
inherited grammar's arbitrary choices would show through most visibly,
because there is no external standard to converge on. The enumeration
below therefore records, for each site, the reason the behaviour exists —
and where the honest answer is "no reason was recorded", it says so
instead of inventing one.

## The behaviour inventory

Every site is classified:

- **R** — required by the official syntax. The parser is implementing
  the language; nothing to justify.
- **A** — an intentional allowance. Not in the language; kept on purpose
  for editor behaviour, with the reason stated.
- **D** — neither. A behaviour that is not the language and that is not
  wanted. C2 changes these.

### R — implementing the language

| # | Site | Behaviour |
|---|---|---|
| R1 | `lexer` `SdblTokenKind::Comment` | `//` to end of line. Глава 8 §8.4.3 defines it and says comments are ignored when the query runs. Not a convenience; the master document's fifth scope bullet misreads it. |
| R2 | `select.rs` `query_extension` / `eat_query_extensions` | Brace regions consumed verbatim. Attested by Slice 1-addendum. |
| R3 | `expressions.rs` `at_property_name` | Keywords admitted after a dot as property names. The language allows a field named `И`; the lexer cannot know. |

### A — intentional allowances

| # | Site | Behaviour and reason |
|---|---|---|
| A1 | `select.rs:707` `group_by_clause` | `СГРУППИРОВАТЬ` with no `ПО` completes the node silently. The word is typed before the `ПО` that must follow it; erroring on every keystroke in between would be noise. Same for A2–A4. |
| A2 | `select.rs:731` `order_by_clause` | `УПОРЯДОЧИТЬ` with no `ПО`. |
| A3 | `select.rs:807` `index_by_clause` | `ИНДЕКСИРОВАТЬ` with no `ПО`. |
| A4 | `select.rs:863` `totals_by_clause` | `ИТОГИ` with no `ПО`. The canonical rule marks `ПО` mandatory in so many words («после обязательного ключевого слова ПО»). |
| A5 | `select.rs:782` `for_update_clause` | `ДЛЯ` with no `ИЗМЕНЕНИЯ`. |
| A6 | `select.rs:17` `recover_field_to_alias_or_delimiter` | A malformed field is skipped to the next alias, comma or clause boundary, tracking `CASE` and paren depth and refusing to stop inside a nested query. Emits «пропуск некорректного фрагмента». Keeps one bad field from destroying the rest of the list. |
| A7 | `select.rs:934` `recover_to_delimiter_vt` | The same idea inside virtual-table arguments, paren-depth aware. Attested by Slice 8-addendum allowance #5. |
| A8 | `expressions.rs:61` `recover_to_delimiter` | The same idea inside expressions, tracking parens and braces. |
| A9 | `expressions.rs:182` `parse_delimited_list` | A missing list element between two delimiters emits «пропущен элемент списка» and keeps going. |
| A10 | `select.rs:1028,1040` virtual-table empty arguments | `SdblMissingArg` for leading, trailing and consecutive empty arguments. Attested by Slice 8-addendum allowances #1–#4 — and the empty-argument form is itself canonical, so only the node shape is an allowance. |
| A11 | `select.rs:485,496` `join_clause` | Missing `СОЕДИНЕНИЕ` after a join type, and missing `ПО` after a source, each bump one token into an error node — or, at end of input, record a missing token instead. Slice 9 chose Option B PRESERVE and deferred hardening here; on the evidence below the recovery quality is unchanged by this slice, and the deferral is discharged as "no change needed", not as work skipped. |
| A12 | `select.rs:911` `limitations` | `РАЗЛИЧНЫЕ` / `ПЕРВЫЕ` / `РАЗРЕШЕННЫЕ` in any order and repeated. Attested by Slice 7-addendum allowances #1–#2. |
| A13 | `select.rs:753` `order_by_item` | `УБЫВ ИЕРАРХИЯ` accepted, though the canonical `<Порядок>` has only `ИЕРАРХИЯ УБЫВ`. Kept: it is a plausible slip, and rejecting a word order the language merely does not list buys nothing. See § Source questions. |
| A14 | `parser.rs:227` `check_iteration_limit` | A stuck-position guard on every unbounded loop. Not a language property at all; a liveness property of the parser. |

### D — neither the language nor a decision

| # | Site | What actually happens |
|---|---|---|
| D1 | `sdbl.rs:12-38` `query_package` | **The tail of the input is dropped.** The package loop ends as soon as the current token is not `;`, and nothing consumes what remains. No error is emitted, and the syntax tree covers less text than it was given. |
| D2 | `select.rs:881` `totals_group_item` | `ПЕРИОДАМИ(…)` is not consumed. Since it is the last thing in the query, D1 then deletes it. |
| D3 | `select.rs:881` `totals_group_item` | A control-point alias, with or without `КАК`, is not consumed, and D1 deletes it. |
| D4 | `select.rs:759` `order_by_item` | The canonical `ИЕРАРХИЯ УБЫВ` loses its `УБЫВ` to D1. The parser accepts only the reverse order, which the source does not list. |

D1 is the mechanism; D2–D4 are documented, valid query forms that fall
into it. That relationship is the finding of this slice.

## The silent tail loss

Measured on the tree, before any change. `lost` is the number of source
bytes the syntax tree does not cover; `errors` is what the parse
reported.

```text
ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ)   lost=28  errors=0
ВЫБРАТЬ А ИЗ Т ИТОГИ СУММА(А) ПО Н КАК Группа        lost=19  errors=0
ВЫБРАТЬ А ИЗ Т УПОРЯДОЧИТЬ ПО Н ИЕРАРХИЯ УБЫВ        lost=8   errors=0
ВЫБРАТЬ А ГДЕ А=1 ИЗ Т                               lost=7   errors=0
ВЫБРАТЬ А ИЗ Т ГДЕ А = 1 ФУНК(Х); ВЫБРАТЬ 2 ИЗ У     lost=38  errors=0
```

Four properties make this a defect rather than an allowance.

**It is silent.** Every one of those parses reports success. A consumer
cannot distinguish "the query was fine" from "I threw away the second
half".

**It breaks the tree invariant the architecture rests on.** A Rowan tree
is full-fidelity: its text is the source text. Every other entry point
maintains this. The BSL entry point `grammar::source_file` loops
`while !p.at_end()` and cannot leave input unconsumed; the SDBL entry
point has no such loop. The asymmetry runs one way only, which is what
an oversight looks like and not what a decision looks like.

**It reaches beyond the editor.** `parse_sdbl` feeds the query-to-table
graph in `crates/hir-def/src/graph_index.rs` and the lowering of every
embedded query string in `crates/hir-def/src/body/lower/expr.rs`. The
last line above shows a whole following query, `ИЗ` clause and all,
disappearing from a package. Whatever the graph does with that, it does
not know it happened.

**It fires on valid input.** Not only on the nonsense in line four. The
first three lines are forms Глава 8 defines and gives worked examples
for.

## The five scope bullets

The master document lists five concerns for this slice. Examined against
the code, they do not survive as five.

1. **Incomplete queries while typing.** Real and working — A1–A5.
   Nothing to build; the slice records why they are silent.

2. **Flexible clause ordering retained for IDE usefulness.** *This does
   not exist.* `query_body_clauses` tests for `ИЗ`, `ГДЕ`,
   `СГРУППИРОВАТЬ`, `ИМЕЮЩИЕ`, `ДЛЯ`, `ИНДЕКСИРОВАТЬ` in that fixed
   order, once each. A clause out of order is not accepted flexibly — it
   is never tested for, and D1 then deletes it:

   ```text
   ВЫБРАТЬ А ГДЕ А=1 ИЗ Т                             lost="ИЗ Т"
   ВЫБРАТЬ А ИЗ Т СГРУППИРОВАТЬ ПО Н ГДЕ А=1          lost="ГДЕ А=1"
   ВЫБРАТЬ А ИЗ Т ИМЕЮЩИЕ А>0 СГРУППИРОВАТЬ ПО Н      lost="СГРУППИРОВАТЬ ПО Н"
   ```

   The three trailing clauses `АВТОУПОРЯДОЧИВАНИЕ`, `УПОРЯДОЧИТЬ ПО` and
   `ИТОГИ` *are* order-free, via the flag loop in `select_tail_clauses` —
   so the bullet is true of three clauses out of nine and false of the
   rest. The claim is corrected rather than implemented: making clause
   order free everywhere is a grammar change with no source behind it,
   and it is not what an IDE needs. What an IDE needs is for the
   misplaced clause to be visible, which is what fixing D1 gives it.

3. **Conservative error nodes.** The leftover input gets no node at all,
   conservative or otherwise. This is D1, and it is the slice's work.

4. **Multiline query string artifacts.** Handled a layer down, in
   `crates/syntax/src/sdbl_query.rs`, which converts a BSL string
   literal into SDBL text and returns the corrections needed to map
   ranges back across doubled quotes. Out of the grammar's scope by the
   project's layering rule. No work.

5. **Line comments "if the project still wants them".** The premise is
   wrong: comments are canonical (R1). There is no want to exercise.

Two bullets are therefore empty, one is misstated, and the remaining two
are the slice.

## Source questions

### `ПЕРИОДАМИ` — settled by the primary source

Глава 8 §8.4.16 gives the control-point rule in full:

```text
<Контрольная точка>
<Выражение> [[ТОЛЬКО] ИЕРАРХИЯ] | [ПЕРИОДАМИ(Секунда | Минута | Час |
День | Неделя | Месяц | Квартал | Год | Декада | Полугодие
[,<Литерал типа DATE> | <Идентификатор параметра>]
[,<Литерал типа DATE> | <Идентификатор параметра>])] [[КАК] Псевдоним поля]
```

with the worked example in §8.4.16.4 —
`ПО Период ПЕРИОДАМИ(МИНУТА, ДАТАВРЕМЯ(2006,6,28), ДАТАВРЕМЯ(2006,6,28))`.
Ten period names, and both date arguments optional. Tier A1. The prose
adds that when the dates are omitted, the first and last dates present in
the result are used — a semantic point, and so Slice 13's, not this
slice's.

Slice 11 recorded that `ПЕРИОДАМИ` coverage "was NOT verified in chapter
39 by direct dump read". That is accurate about the textbook article and
was the right thing to write at the time. It is the primary source, not
the textbook, that carries the rule.

### The order of `ИЕРАРХИЯ` and `УБЫВ` — the parser has it backwards

Глава 8 §8.4.15 enumerates the ordering-field rule:

```text
<Поле упорядочивания>
<Выражение> <Порядок> ВОЗР | УБЫВ | ИЕРАРХИЯ | ИЕРАРХИЯ УБЫВ
```

Four alternatives, and `ИЕРАРХИЯ УБЫВ` is one of them. `order_by_item`
consumes an optional `ВОЗР|УБЫВ` and *then* an optional `ИЕРАРХИЯ`,
which accepts `УБЫВ ИЕРАРХИЯ` — not listed — and drops the `УБЫВ` of
`ИЕРАРХИЯ УБЫВ`, which is. C2 adds the canonical order (D4) and keeps
the reverse one as a stated allowance (A13) rather than starting to
reject input that has been accepted for the life of the parser.

### Where `ОБЩИЕ` goes — the two official sources disagree

The primary rule places it as a prefix keyword, before the list and
without a separator:

```text
ИТОГИ [<Список итоговых полей>] ПО [ОБЩИЕ] <Список контрольных точек>
```

The textbook article's worked listing puts it in the list, with a comma:

```text
ИТОГИ
  СУММА(Количество),
  СУММА(Сумма)
ПО
  ОБЩИЕ,
  Поставщик
```

Both are official; neither is a misprint. The parser today accepts the
comma form (`ОБЩИЕ` falls through as a bare identifier expression — Slice
11 allowance #1) and loses the prefix form to D1. Rather than choose
between two sources that are each canonical in their own document, C2
accepts the union: `ОБЩИЕ` immediately after `ПО` is consumed whether or
not a comma follows it. This is the same resolution Slice 4 reached for
`РАЗНОСТЬДАТ`, where two spellings each had a source and both were
accepted.

## Behaviour change planned for C2

In this order, which is not free:

1. **The modifiers first.** `ПЕРИОДАМИ(…)` with its period name and up to
   two date arguments; the control-point alias; the canonical
   `ИЕРАРХИЯ УБЫВ`; `ОБЩИЕ` in prefix position. All consumed as tokens
   and expressions of the existing nodes, no new node kinds, so
   `crates/sdbl-hir` sees no shape it did not see before.

2. **The drain second.** `query_package` stops leaving input unconsumed:
   whatever remains when no rule will take it is bumped into an `ERROR`
   node and reported as a `ParseError`.

The order matters because SDBL parse errors are not internal. They are
mapped back into BSL ranges in `crates/hir-def/src/body/lower/expr.rs`
and surface to the user. Draining before the modifiers exist would put a
fresh diagnostic on every query that uses a documented form the parser
has simply never handled — in one production configuration available to
this project, `ПЕРИОДАМИ(` occurs 16 times and `ТОЛЬКО ИЕРАРХИЯ` 13
times. Fixing the parser's blindness and then reporting what is still
blind is a diagnostic; reporting first is a regression.

The owner's decision, recorded 2026-07-27, was the full scope with a
visible diagnostic, over the two narrower options offered (drain only,
or attestation with no behaviour change at all).

## What this slice does not do

- **It does not make clause order free.** See § The five scope bullets,
  item 2.
- **It does not promote modifiers into their own node kinds.** The AST
  shape is held constant so that `sdbl-hir` stays Slice 13's.
- **It does not interpret the modifiers.** That a `ПЕРИОДАМИ` control
  point means date completion, and what an omitted date range defaults
  to, is semantics.
- **It does not touch the two `join_clause` recoveries** beyond
  attesting them (A11). Slice 9 deferred hardening here; on inspection
  there is nothing this slice would improve, and the deferral is
  discharged rather than carried forward again.

## Blind spots to pin at C0b

The regression pins added before any behaviour change, each recording
today's answer so that C2 has to move it deliberately:

1. the tail-loss class itself — trailing token, trailing call, a whole
   following query in a package;
2. `ПЕРИОДАМИ` in one- and three-argument form, and after a comma-separated
   second control point;
3. the control-point alias, with and without `КАК`;
4. `ИЕРАРХИЯ УБЫВ` versus `УБЫВ ИЕРАРХИЯ`;
5. `ОБЩИЕ` with a following comma, and without one;
6. the silent optionals A1–A5, which must *not* move;
7. clause reordering, which must become visible rather than silent;
8. the coverage invariant itself, as a property: for a corpus of inputs,
   tree text length equals source length.

## Commit trail

- C0a — this document, as the sole change.

The remaining entries are filled in as they land. The absolute-last
commit on the branch is the one that edits this trail, and is therefore
necessarily not named in it — the anti-Hilbert disclosure shared with
every closed slice in this programme.

## Licensing note

Nothing in this slice is derived from a third-party grammar or parser.
The behaviours it changes are derived from the primary source cited
above; the behaviours it preserves are justified in this document on
their own terms, and where no justification existed the document says
so rather than supplying one after the fact.

## Author attestation

The author of this slice did not read, and did not have open, any
third-party SDBL or BSL grammar file while producing it. The
`sdbl-provenance-2026-07-audit.md` section comparing this tree against
the upstream grammars remained under read-quarantine throughout.
