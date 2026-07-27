# SDBL Slice 12 — Clean-Room Attestation (recovery and IDE allowances)

**Status:** complete (2026-07-27).

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
document would be a false statement. Six such behaviours were found —
four by reading the code, and two more only after the first four were
fixed, because the fix is what made them legible.

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
- `crates/parser/tests/sdbl_parser_tests.rs` — the coverage pins added at
  C0b and turned over at C2;
- `crates/parser/tests/sdbl_recovery_and_allowances.rs` — the acceptance
  suite born at C3, 20 tests;
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
  for editor behaviour, with the reason stated. A15 and A16 were created
  by this slice rather than found by it: adding a rule adds the question
  of what to do when the input does not match it.
- **D** — neither. A behaviour that is not the language and that is not
  wanted. C2 changes these.

### R — implementing the language

| # | Site | Behaviour |
|---|---|---|
| R1 | `lexer` `SdblTokenKind::Comment` | `//` to end of line. Глава 8 §8.4.3 defines it and says comments are ignored when the query runs. Not a convenience; the master document's fifth scope bullet misreads it. |
| R2 | `select.rs` `query_extension` / `eat_query_extensions` | Brace regions consumed verbatim. Attested by Slice 1-addendum. |
| R3 | `expressions.rs` `at_property_name` | A handful of keywords are admitted after a dot as property names. Only a name can appear in that position, so the position resolves what the spelling cannot: the lexer has already decided that `В` is `IN`, and after a dot that decision is simply wrong. Not a tolerance — reading the position is what the grammar requires. |

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
| A15 | `select.rs` `totals_periods_modifier` | The rule allows at most two boundary arguments; the parser takes any number. A missing boundary after a comma is reported, and so is one that is neither a date literal nor a parameter, but a third *well-formed* boundary is not. Counting present arguments is a check with no recovery value, and refusing to parse them would only hide the rest of the clause. |
| A16 | `select.rs` `totals_periods_modifier` | At end of input, a `ПЕРИОДАМИ` whose paren is not open yet or not closed yet is taken and left quiet — the same case as the half-typed clause keywords of A1–A5, and quiet for the same reason. With a clause still to come, both are reported: there the paren is not being typed, it is missing. |
| A17 | `select.rs` `totals_group_modifier`, `totals_periods_modifier` | Where the rule is broken but the text is unambiguous — a period name outside the ten, `ИЕРАРХИЯ` and `ПЕРИОДАМИ` together — the parser reports and then consumes anyway. Refusing would cost the clause its shape to make a point the diagnostic already makes. |
| A19 | `select.rs` `at_period_boundary` | The boundary check reads the first token: a parameter, or `ДАТАВРЕМЯ` followed by `(`. Three shapes get through it. An expression *built from* a boundary (`&П + 1`) passes, because reading the whole boundary would mean inspecting the parsed node. A bare `&` passes, and this one is not a choice: `crates/parser/src/sdbl_token_converter.rs` maps both `Parameter` and `Ampersand` to `TokenKind::Ampersand`, so at parser level a named parameter and a lone sigil are the same token — telling them apart belongs to the converter, which is Slice 13's. And `ДАТАВРЕМЯ(Поле)` passes, because validating a date literal's arguments is the date-literal rule, not this one. |
| A18 | `select.rs` `totals_group_alias` | A *bare* alias is taken on the wide clause-keyword guard, an explicit one on the narrow `is_body_clause_keyword`. A bare alias cannot be told from a following clause by anything but its spelling, so the wide guard is the price of supporting it; after `КАК` there is no such ambiguity, and a name that spells a keyword is still a name — the same reading `source_alias` and the `ПОМЕСТИТЬ` name already take. |

### D — neither the language nor a decision

Stated as they were found, before the rewrite. All six are closed.

| # | Site | What happened |
|---|---|---|
| D1 | `sdbl.rs:12-38` `query_package` | **The tail of the input is dropped.** The package loop ends as soon as the current token is not `;`, and nothing consumes what remains. No error is emitted, and the syntax tree covers less text than it was given. |
| D2 | `select.rs:881` `totals_group_item` | `ПЕРИОДАМИ(…)` is not consumed. Since it is the last thing in the query, D1 then deletes it. |
| D3 | `select.rs:881` `totals_group_item` | A control-point alias, with or without `КАК`, is not consumed, and D1 deletes it. |
| D4 | `select.rs:759` `order_by_item` | The canonical `ИЕРАРХИЯ УБЫВ` loses its `УБЫВ` to D1. The parser accepts only the reverse order, which the source does not list. |
| D5 | `select.rs` `selected_fields` | The list parser takes its first item unconditionally, and an expression asked to start on a clause keyword takes it anyway. A selection list that is empty — because it has not been typed, or because the qualifier before it failed — becomes a field made of the *next clause's* keyword, and that clause is then never recognised. |
| D6 | `select.rs` `top_clause` | `Parser::expect` reports a missing count by bumping whatever is there, which for `ВЫБРАТЬ ПЕРВЫЕ ИЗ Т` is the `ИЗ`. Slice 7-addendum recorded this as allowance Q3 and deferred the fix here. |

D1 is the mechanism; D2–D4 are documented, valid query forms that fall
into it. That relationship is the finding of this slice.

D5 and D6 were not in the C0a inventory. They surfaced at C2, because
draining the leftover is what made them legible: with the remainder
discarded in silence there was nothing to see, and with it reported the
message named the wrong thing. D6 is the deferral Slice 7-addendum left
here in so many words; D5 is the reason D6's narrow fix would not have
been enough on its own.

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

Both are official; neither is a misprint. The parser accepts the comma
form — `ОБЩИЕ` falls through as a bare identifier expression, Slice 11
allowance #1 — and lost the prefix form to D1.

C0a planned to accept the union, by consuming `ОБЩИЕ` as a prefix keyword
whether or not a comma followed. C2 did not do that, on a measurement.
In the production configuration checked here, every `ПО ОБЩИЕ` is either
followed by a comma or ends the clause; the comma-less form with a
following list does not occur once. Promoting `ОБЩИЕ` to a prefix keyword
would therefore have changed the tree shape of the form that *is* written,
for the sake of one that is not — and that shape is Slice 11's stated
allowance and Slice 13's to interpret.

So the shape is left alone. The prefix form stops losing text as a side
effect of the control-point alias (`Н` in `ПО ОБЩИЕ Н` is taken as the
alias of `ОБЩИЕ`), which restores the invariant without deciding the
reading. What the reading should be stays open, and the disagreement
between the two sources is recorded here rather than resolved by a
parser that has no standing to resolve it.

## Behaviour change

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

### C2 outcome, measured

Against a production 1C configuration held locally as this project's
extension testbed and not part of this repository — 27 677 query literals
extracted from 21 114 modules:

| | |
|---|---|
| queries that produce the new leftover error | 851 (3.1%) |
| of those, queries that produced **no** diagnostic before | **68 (0.25%)** |
| of those, queries already carrying another parse error | 783 |
| queries whose tree still does not cover the input | 29 (0.1%) |

The 0.25% is the number that matters for the decision the owner made: a
query that was clean and now is not. The other 783 were already lit up.

Grouping the 851 by what the leftover starts with: 209 begin at a `КАК`,
149 at a `%1`/`%2`/`%3`, 116 at a `(Титульный,…`, and a further 24 at a
`#Имя` or `[Имя]`. Sampling the `КАК` class shows the same cause — the
queries read `ИЗ #ТаблицаПланаОбмена КАК ПланОбмена`, and it is the `#Имя`
before the alias that the grammar cannot place.

None of these markers are query syntax. The query language has no
substitution facility at all; what makes them possible is only that a
query lives in a string literal, and a string literal may contain
anything. Writing them is a practice the platform does not sanction and
one that is discouraged in all but rare cases, however common it has
become in practice.

That matters for how the 851 should be read. They are not the price of
this change and not noise it introduces: they are correct reports about
text that is not a query. What the change did was make them visible. The
text was never being parsed — it was being truncated in silence, and the
graph index has been blind past that point for as long as the entry point
has existed. The number is a measure of how widespread the practice is in
one configuration, not a measure of what the analyzer got wrong.

The commit message of C2 describes these same leftovers as "query
templates … not SDBL until they are substituted", which reads as though
substitution were a facility of the language. It is not, and history is
not rewritten for it — the corrected account is the one above.

### One defect this surfaced and did not fix

**An unterminated string literal still costs coverage.** 29 of the
27 677 remain short even with the drain in place, and in one case the
tree's text range ends *inside a character* rather than between two. All
of them end inside an unclosed `"`. The drain bumps every token the
parser is given, so a shortfall means the shortfall is upstream, in the
lexer's string mode or in the conversion of a BSL literal to SDBL text.
That is a different layer, and the lexer is closed and attested; the
finding is recorded here rather than acted on.

### What the review changed

The two-lens review of the whole slice returned thirteen findings. Every
one was reproduced first; every one held. Two of them were behaviours I
had written into this document as deliberate, which is the reason the
document is worth having: a claim of intent is checkable, and these did
not check out.

**The drain stopped at the end of input rather than at the separator.**
So a package whose first query was bad still lost the rest — not the
text, which the drain now covered, but the *structure*: the following
queries sat inside an error node, and a consumer that walks a package's
queries never saw them. Fixing the text loss and leaving the structural
loss is worse than either, because the diagnostic says the problem is
handled. The drain now runs inside the package loop and stops at the
next `;`, so one bad member of a package costs its own parse and nothing
else.

**The modifier rule was implemented as permissive sequential `if`s.**
The rule gives alternatives — `[[ТОЛЬКО] ИЕРАРХИЯ] | [ПЕРИОДАМИ(…)]` —
and a closed list of ten period names, and marks several parts
mandatory. The first implementation took each part with an independent
optional `if`, which accepted `ИЕРАРХИЯ ПЕРИОДАМИ(…)`, `ПЕРИОДАМИ(СМЕНА)`,
`ПЕРИОДАМИ()`, `ПЕРИОДАМИ` with no parens at all, a trailing comma with
no boundary, and `ТОЛЬКО` with no `ИЕРАРХИЯ`. Normalising a rule while
enforcing none of it turns a silent loss into a silent acceptance, which
is the same defect wearing the other coat. Each of those is now
reported, and reported without moving, so the clause after it still
parses.

**`ИЕРАРХИЯ ВОЗР` was accepted.** The four documented orderings put only
the descending direction after the hierarchy. Taking the direction with
the same shared helper as before quietly widened the language past the
one tolerance the slice had declared.

**`КАК` written out did not commit to a name**, and the name it did
accept was filtered by the wide clause-keyword guard rather than the
narrow one every other explicit alias in this grammar uses. So
`ПО Н КАК, П` passed in silence while `AS Inner` — a perfectly good alias
whose name happens to spell a JOIN keyword — produced a false error.

**A leading comma in the selection list cost the whole query.** The
empty-list guard treated any token that cannot start a field as an
absent list, and then the drain took everything after it. Only a clause
keyword, a separator or the end of input means the list is absent; a
stray comma means a list with something wrong inside it, and the list
parser's own recovery handles that far better.

The remaining findings were arithmetic in this document and the master
document: counts not updated after the inventory grew, and a corpus
figure that had been refreshed in one document and not the other.

### What the second review round changed

The first round's fixes were themselves reviewed, because two of that
round's findings had been defects inside earlier fixes of this same
slice. The second round returned ten findings; nine held and were fixed,
one was declined.

The one that mattered was a class, not an instance. Making the drain stop
at the separator assumed that every inner recovery path leaves the
separator where it found it — and several do not, because they report by
bumping a token, and the token they bump can be the `;`. So `УНИЧТОЖИТЬ ;
ВЫБРАТЬ A ИЗ T` lost the good query anyway, and so did three other shapes.
The fix is not another guard per site: the package loop no longer depends
on the separator surviving at all. It runs while there is input left,
skips separators wherever it finds them, and the drain stops at a
separator *or* at the start of the next query. That is the same shape the
BSL entry point has, and the invariant now holds by construction rather
than by everyone downstream being careful.

Four smaller ones were the rule again being normalised without being
enforced: a period boundary could be any expression rather than a date
literal or a parameter; `ВОЗР ИЕРАРХИЯ` was accepted although the single
declared tolerance covers only `УБЫВ ИЕРАРХИЯ`; a closing paren did not
count as the end of an empty selection list, so an empty subquery
swallowed the separator after it; and a separator did not count as a
missing `ПЕРВЫЕ` count. One was a false message: `ТОЛЬКО` with no
`ИЕРАРХИЯ` also claimed a conflict with `ПЕРИОДАМИ`, about a word that
was not in the input.

**The declined finding.** `ПЕРИОДАМИ` with no parenthesis at all at end of
input is accepted in silence, and the review asked for it to be reported
on the grounds that the opening paren is mandatory. It is — but so is the
`ПО` after `СГРУППИРОВАТЬ`, and A1–A5 stay quiet about that for the same
reason: at end of input the user is mid-word, not mistaken. Reporting here
and not there would be the inconsistency. A16's wording was too narrow to
cover this and has been widened; the behaviour stands.

### Where the separator problem actually was

Three rounds circled the same thing and each fixed it one level too high.
The drain was made to stop at the separator; then the package loop was
made not to depend on the separator being there; then the loop was made to
report when it was genuinely absent. The fourth round showed why none of
that was enough: `Parser::emit_error` reports by taking the offending
token, and when the parser is standing on a `;` the token it takes is the
separator itself.

Everything downstream was compensation. A recovery that consumed the
separator and returned immediately could be papered over by recognising
the next query's first word — but a recovery that consumed it and *kept
going*, as the missing-`ON` path in a `JOIN` does, absorbed the next
query's `ВЫБРАТЬ` into the broken clause before the loop ever got control
back. No amount of care in the caller can fix that.

So the fix is in `emit_error`: a statement separator is never the
offending token. It is the boundary that lets what follows still be
parsed, and taking it converts one bad statement into the loss of the
next. This is shared with the BSL grammar, where the same reasoning
applies and where the whole workspace's tests confirm it changes nothing
else.

The drain also learned paren depth in the same pass. Stopping at a query
start is right at the top level and wrong inside a paren group, where the
query start belongs to a subquery — without the depth count the drain
handed the lowerer a subquery dressed as a package member.

### The root fix needed a second half

Not bumping the separator was only half of what "report at a separator"
means. The error still carried `RecoveryKind::BumpToken`, and the sink
computes that kind's range from the *previous* token — so every complaint
made next to a `;` landed on the word before the gap instead of in it.
`УНИЧТОЖИТЬ;` underlined `УНИЧТОЖИТЬ` rather than pointing at where the
name should have been. The kind is now chosen where the other kinds are
chosen: standing on a separator is treated exactly like standing at end of
input, which is what it is from the rule's point of view. This affects the
BSL grammar identically.

Two more findings were the drain's new depth counter applied too widely
and not widely enough at once. A separator cannot belong to a paren group
in this language, so guarding the separator check by depth let an unclosed
`(` in the leftover swallow the next member. A query keyword *can* belong
to a group — a subquery inside parens, extension text inside braces — so
the depth count had to cover braces too.

The fourth finding was subtler and needed something the parser did not
have. A rule that gives up inside an unclosed group hands back a position
that is not top level, and the package loop had no way to know: the group
was opened by the grammar, not by the drain, so the drain's local counter
could not see it. `ВЫБРАТЬ Ф(А X ВЫБРАТЬ Б ИЗ У)` therefore produced two
package members, the second of which `sdbl-hir` would have lowered as a
query in its own right. The parser now offers the net nesting a rule
introduced, and the loop asks before treating a query keyword as a new
member.

### Nesting kept in two places agreed with itself only by luck

The fifth round's fix gave the package loop a way to ask whether a rule
had left a group open. The sixth round found four ways that answer could
be wrong, and they share one cause: the bookkeeping existed twice — an
accumulator in the loop and a local counter in the drain — and neither
knew what the other had seen.

A group closed *by the drain* never reached the loop's accumulator, so the
state stuck and every later top-level query in that package was swallowed.
And both counters treated the two bracket kinds as one arithmetic balance,
so `( }` cancelled to zero and a query still inside an unclosed paren was
promoted to a package member.

There is now one answer to the question, computed rather than
accumulated: everything since the last separator is one member's worth of
tokens, and nesting is read from that whole span, with parens and braces
counted apart and a closer that has nothing to close ignored rather than
driving a count negative. Two counters cannot disagree if there is one.

The fourth finding was that the separator rule had been applied to two of
the three paths that report errors. `Parser::expect` is the third, and it
was still marking a `;` as the offending token — so `SELECT A FROM ;
SELECT B FROM U`, which is missing only a source, also got told its
separator was missing.

### Correct and quadratic is not correct

Replacing two disagreeing counters with one computed answer fixed the
disagreement and introduced a worse problem. The answer was computed by
scanning the member's tokens from its start, and a member only ends at a
separator — so on a package written without separators, every query
keyword rescanned everything before it. A hundred thousand of them is
five billion token checks. The iteration limiter does not see this,
because the scanning happens inside a single iteration; the parser simply
stops responding. For an editor, that input is not exotic: a run-on
package is what a half-written query looks like.

The count is now kept where the tokens go by. `bump` is the only way a
token is consumed and the position never rewinds, so it is the one place
that sees each token exactly once — counting anywhere else means either
counting twice or rescanning. A caller records the count when a member
begins and compares; the answer is constant-time, and the same input now
parses in tens of milliseconds.

The second finding fell out of the same move. Brackets inside a `{…}`
region belong to that region's verbatim text and say nothing about the
structure around it, so they must not be counted at all — `SELECT A { ) (
} SELECT B` has a complete extension and two queries, not an unclosed
paren swallowing the second. Counting is skipped while a brace region is
open.

### A counter that outlived what it was counting

Moving the count into `bump` gave it the parser's lifetime, and a package
member's lifetime is shorter than that: a group cannot span a separator.
The state leaked across one in two ways. An unclosed `{` left by a bad
member kept the opaque-region rule switched on, so every paren in every
member after it went uncounted. And comparing totals could not tell
"closed the inherited group and opened my own" from "changed nothing",
so a query inside a fresh paren was promoted to a member of the package.

A separator now resets the count. That is not a patch on the comparison —
it is the rule itself, stated once: what a member left open is that
member's business, and the next one starts level. The baseline the loop
used to carry disappears with it, because after a reset the baseline is
always nothing.

One finding of that round was refuted rather than fixed. It read the
change of wording about substitution markers as an unrelated legal
revision smuggled into a performance fix. The wording change is its own
commit, `edf05a6e`; the review saw them together because the base it was
given reached back two commits. Nothing was mixed.

### The separator rule reached the last three places that ignored it

The fourth round stopped errors from consuming a separator. The ninth
found three rules that consume one without reporting anything at all: a
brace region reads to its closing brace and takes whatever is in between,
and both skip-ahead recoveries stopped at a separator only while their own
local depth was zero. So an unclosed `{` swallowed the rest of the
package in silence, and a recovery inside a paren swallowed the boundary
and left the outer group standing.

The rule is now stated in each of them and, more importantly, in `bump`
itself: consuming a separator closes whatever grouping was open, wherever
it is consumed. Tying that to the one caller that happens to notice the
separator was what let every other path keep stale depth.

Fixing the swallow exposed a behaviour that had been hiding behind it.
After a separator a query is due, and the loop used to force a query rule
to run whatever came next — which on a `)` mints an empty member node, and
`sdbl-hir` walks those. It now runs one only where something can begin a
member. A clause keyword still counts: `ИЗ Т` with no `ВЫБРАТЬ` yet is a
query being written, and Slice 7 guarantees it a node to hang on to, which
is a contract this slice must not quietly break. A `)` is not.

### Dropping a token is not the same as filling the slot

The ninth round's fix stopped the loop from minting a member for a token
that begins nothing. It also cleared the "a member is owed" flag when it
dropped that token — as though the junk had been the member. So one
stray `)` after a separator cancelled the incomplete query behind it, and
`SELECT A FROM T; ) FROM Products` lost the node that Slice 7 promises
`FROM Products`. The contract this slice had just gone out of its way to
protect was broken by the protection.

The flag now survives the drop, and the drain stops at a clause keyword
while a member is still owed, so the query after the junk is reached. What
is one-per-member is the complaint, not the debt.

### Two voices reporting the same silence

Three of the eleventh round's four findings were the same mistake seen
from different inputs: the loop and the query rule both reported a member
with no query in it. The rule says so when it runs; the loop had been
saying so before letting it run, so an incomplete query got its diagnosis
twice. And the flag meant to keep the loop to one complaint per member was
not cleared at the separator, so a second short member inherited the
first's silence and got none.

Patching three sites would have been patching one rule stated three ways.
The rule is: the loop speaks only for members no query rule ever saw, and
it speaks at the end of the member, where it knows whether anything was in
it at all. Everything else — including a missing `ВЫБРАТЬ` in a member the
rule did see — belongs to the rule.

Stating it that way immediately caught a fourth case the review had not
raised: a trailing separator owes nothing, and the end-of-input complaint
had started firing on it. An owed member with no tokens at the end of the
input is not a missing member; it is a `;` and then nothing.

The remaining finding was the drain stopping at a clause keyword that
followed a dot. `T.ИЗ` is one fragment being skipped, not a fragment and
then a query, and treating the word after the dot as a beginning minted a
query node where no query exists.

### A test that was asserting something false

`test_batch_with_drop` fed `ВЫБРАТЬ Поле ИЗ Таблица ПОМЕСТИТЬ ВТ;
УНИЧТОЖИТЬ ВТ` to `check_no_errors` and passed. `ПОМЕСТИТЬ` precedes `ИЗ`
in the canonical rule, and in the testbed configuration every one of the
several hundred `ПОМЕСТИТЬ` clauses is written that way. The test's input
had the two the wrong way round, so half of it was being discarded, and
the assertion "no errors" was true only because the discarding was
silent. The input is corrected to the documented order and the misordered
form joins the leftover cases.

This is the clearest single argument for the slice. A test whose whole
purpose is to check that a query package parses had been quietly checking
half a query package instead, for as long as it has existed, and no
amount of reading the test would have revealed it.

## What this slice does not do

- **It does not make clause order free.** See § The five scope bullets,
  item 2.
- **It does not settle where `ОБЩИЕ` goes.** Two official sources
  disagree; the parser keeps the shape it had and merely stops losing
  text on the form it does not model. See § Source questions.
- **It does not promote modifiers into their own node kinds.** The AST
  shape is held constant so that `sdbl-hir` stays Slice 13's.
- **It does not interpret the modifiers.** That a `ПЕРИОДАМИ` control
  point means date completion, and what an omitted date range defaults
  to, is semantics.
- **It does not touch the two `join_clause` recoveries** beyond
  attesting them (A11). Slice 9 deferred hardening here; on inspection
  there is nothing this slice would improve, and the deferral is
  discharged rather than carried forward again.

## Blind spots pinned at C0b

The regression pins added before any behaviour change, each recording the
pre-rewrite answer so that C2 had to move it deliberately:

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

Twelve tests. The helper returns the *missing suffix* rather than a byte
count, so a failure reads as the text that went missing. Seven moved at
C2 and are now written the other way round; five were there to hold still
and did.

## Commit trail

Slice 12 was not empty before this session. Two fixes landed under its
name in April 2026 and were never attested — `9d418084`, aligning
`recover_to_delimiter` with the any-depth clause-keyword stop, and
`88439afa`, splitting that stop into hard clauses and query
starters/combiners across all three recovery helpers. They are the
"fixes landed, no attestation" the master document recorded, and they
are covered by entries A6–A8 above.

- C0a `18b5bb70` — this document, as the sole change;
- C0b `27eb6e92` — the twelve coverage pins, parser tests 596 → 608;
- C1 `80350945` — the clean-room banners and the module docstring the
  parser had been without since the comment prune; comments only,
  97 insertions, 0 deletions, test count unmoved;
- C2 `da3eeaa0` — the modifiers, then the drain; the seven pins turned
  over; one snapshot in `crates/ide-diagnostics` gains a line;
- C2b — `selected_fields` and `top_clause` (D5, D6), found because the
  drain made them legible; Slice 7-addendum's two audit-gate tests turned
  over; the same snapshot re-taken, and it now names the missing
  selection list where before it named an unparsed remainder;
- C3 — this document's status flip, the mini-spec and master-document
  updates, and `crates/parser/tests/sdbl_recovery_and_allowances.rs`,
  20 tests: 6 on the worked examples the Developer's Reference gives,
  5 on the closed lists its rules enumerate, 5 on the tolerances kept
  on purpose, and 4 on nothing leaving the parser without a word.
  Parser tests 608 → 628.
- C4 `3aebeb9b` — the first review round. Seven substantive findings and
  six small ones, all reproduced before being acted on and all confirmed.
  Parser tests 628 → 632.
- C5 `7a7fb54d` — the second review round, run because two of the first
  round's findings sat inside the first round's own fixes. Ten more
  findings, nine acted on and one declined with a reason. Parser tests
  632 → 636; the acceptance suite grows from 23 tests to 27.
- C6 — the third round. Five findings, two of them regressions introduced
  by C5's own fix: a package with no separator between its members, and an
  empty member between two separators, had both become silently acceptable.
  Parser tests 636 → 638. One lens of this round did not run at all — the
  task file had grown past the argument-size limit — so this round covers
  one perspective, not two.
- C7 `efea2e7e` — the fourth round, with both lenses. Three findings, all
  one class and all confirmed: an inner recovery consuming the package
  separator. Closed at the root rather than per site. Parser tests
  638 → 640.
- C8 `76f4e5ad` — the fifth round. Four findings, three of them in the
  fourth round's own fix. Parser tests 640 → 643.
- C9 `77dee744` — the sixth round, narrowed to the two commits that touch
  the shared parser, because the whole slice's diff had grown past what the
  review tool can pass to a lens. Four findings, all in the fifth round's
  own fix. Parser tests 643 → 645.
- C10 `b23695ff` — the seventh round. Two findings, both in the sixth
  round's own fix, one of them a freeze rather than a wrong answer. Parser
  tests 645 → 647.
- C11 `0bcbdf4a` — the eighth round. Two findings held and one was
  refuted. Parser tests 647 → 648.
- C12 `09ceb39e` — the ninth round. Three findings, one class, and the
  class is the fourth round's again at a deeper level. Parser tests
  648 → 650.
- C13 `9cf16871` — the tenth round. One finding, in the ninth round's own
  fix. One lens found nothing. Parser tests 650 → 651.
- C14 `2e358746` — the eleventh round. Four findings, all in the tenth
  round's own bookkeeping. Parser tests 651 → 653.
- C15 — the twelfth round. One finding, and one this document had already
  named as a risk without testing it. Parser tests 653 → 654.

The absolute-last commit on the branch is the one that edits this trail,
and is therefore necessarily not named in it — the anti-Hilbert
disclosure shared with every closed slice in this programme.

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
