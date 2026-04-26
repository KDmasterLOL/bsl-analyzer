# SDBL SELECT Mini-Spec

## Purpose

This document defines a clean-room mini-spec for the `SELECT` part of the SDBL
parser.

It is intended as the implementation basis for rewriting
`crates/parser/src/grammar/sdbl/select.rs` without relying on the textual grammar
from `bsl-parser`.

## Primary sources

Authoritative sources for this mini-spec:

- 1C query language documentation:
  - `Синтаксис текста запросов`
  - `https://its.1c.ru/db/pubqlang/content/12/hdoc`
- 1C query formatting standard:
  - `Оформление текстов запросов`
  - `https://its.1c.ru/db/v8std/content/437/hdoc`

Secondary source:

- current local parser behavior as captured by tests in:
  - `crates/parser/tests/sdbl_parser_tests.rs`
  - `crates/parser/src/grammar/sdbl/select.rs` tests
  - `crates/sdbl-hir/src/lower/tests.rs`

## Clean-room constraints

When implementing from this spec:

1. Do not consult `bsl-parser` grammar files during coding.
2. If a language rule is unclear, resolve it from official 1C docs or from an
   explicitly documented local behavior decision.
3. If current behavior is preserved for IDE recovery rather than strict syntax,
   say so in code comments and tests.

## Licensing note

As a practical working assumption, the SDBL language, its keywords, clauses, and
semantic constructs belong to the 1C platform as a language/specification, and
only 1C can realistically claim rights in the official language definition
itself.

That means:

- the existence of `SELECT`, `FROM`, `WHERE`, `JOIN`, `ОБЪЕДИНИТЬ`, `ПОМЕСТИТЬ`,
  etc. is not proprietary to `bsl-parser`;
- third-party projects may still hold rights in their own grammar text,
  examples, implementation code, and documentation;
- but a concrete grammar text or its close adaptation may still be copyrightable
  expression.

Generic `MIT`-licensed SQL grammars may be used as inspiration for parser shape,
rule factoring, or recovery techniques, but they must not become a substitute
for SDBL-specific specification. SDBL-specific rules should come from 1C docs and
independently authored tests.

## Scope

This mini-spec covers:

- query packages;
- `SELECT` queries;
- selected field lists;
- aliases;
- `INTO` / `ПОМЕСТИТЬ`;
- `FROM` sources;
- joins;
- `WHERE`;
- `GROUP BY`;
- `HAVING`;
- `ORDER BY`;
- `AUTOORDER`;
- `TOTALS BY`;
- `FOR UPDATE`;
- `INDEX BY`;
- `UNION` / `UNION ALL`.

It does not define the full expression grammar in detail. Expressions are treated
as sub-parsers referenced by this document.

## Lexical assumptions

The parser must assume:

- keywords are accepted in both Russian and English;
- keyword matching is case-insensitive;
- trivia (`whitespace`, `newlines`, `comments`) is preserved in the lossless tree
  but skipped for structural decisions;
- an identifier token may also carry keywords in some parser contexts because the
  SDBL token layer normalizes many keywords into identifier-like parser tokens.

## Structural model

### Query package

A query package is one or more query-package items separated by semicolons.

Model:

```text
query-package := query-item (';' query-item)* ';'?
```

For the current rewrite scope, `query-item` may be:

- `select-query`
- `drop-query` if supported by the parser version

## SELECT query model

### Top level

```text
select-query := subquery trailing-select-clauses*
```

Trailing clauses are:

- `AUTOORDER`
- `ORDER BY ...`
- `TOTALS ... BY ...`

Current parser behavior accepts them in flexible order. That behavior is allowed
to remain for recovery and compatibility, even if the internal implementation is
not a strict formal grammar transcription.

### Subquery and UNION

```text
subquery := query (union-clause)*
union-clause := UNION [ALL] query
```

Notes:

- `UNION` and `ОБЪЕДИНИТЬ` are equivalent.
- `ALL` and `ВСЕ` are equivalent.
- bare `UNION` is syntactically accepted by the parser, but semantic diagnostics
  may still report `UNION` without `ALL` where required by local rules.

### Query body

```text
query :=
  SELECT limitations?
  selected-fields
  into-clause?
  from-clause?
  where-clause?
  group-by-clause?
  having-clause?
  for-update-clause?
  index-by-clause?
  order-by-clause?
```

Notes:

- `SELECT` / `ВЫБРАТЬ` is mandatory.
- `FROM` is optional at parser level because some queries may be incomplete while
  the user is typing.
- later clauses are optional and should be parsed when present.

## Limitations

```text
limitations := limitation+
limitation  := DISTINCT
             | (TOP|ПЕРВЫЕ) <decimal>
             | ALLOWED
```

Supported limitation keywords:

- `DISTINCT` / `РАЗЛИЧНЫЕ`
- `TOP <decimal>` / `ПЕРВЫЕ <decimal>`
- `ALLOWED` / `РАЗРЕШЕННЫЕ`

Parser requirement:

- accept each of these before the field list;
- accept flexible ordering for robustness and recovery;
- do not require exact permutation reproduction from any existing ANTLR grammar.

### AST-shape contract (Slice 7-addendum extension)

`SdblLimitations` is a single direct child of `SdblQuery`, attached
between the `SELECT`/`ВЫБРАТЬ` token and the `selected-fields` list when
at least one limitation keyword is present. Its direct children are a
flat sequence of:

- bare keyword tokens for `DISTINCT`/`РАЗЛИЧНЫЕ` and
  `ALLOWED`/`РАЗРЕШЕННЫЕ` (the keywords arrive as `Ident` tokens via
  the lexer-converter bilingual-ident path; `at_sdbl_keyword` /
  `eat_sdbl_keyword` text-match against the EN/RU pair); and
- nested `SdblTopClause` wrapper nodes for each `TOP`/`ПЕРВЫЕ` form,
  whose direct children are the `TOP`/`ПЕРВЫЕ` keyword token and the
  count `Decimal` token.

There is no per-keyword wrapper for DISTINCT or ALLOWED. The flat
ordering of `SdblLimitations` direct children matches the source order
of the keywords as the parser reads them; the parser does not enforce
any canonical permutation.

### Deferred semantic constraint (codex Round-4 finding 4)

v8327doc Глава 8 at `page.html:1336` constrains РАЗРЕШЕННЫЕ to the
top-level `ВЫБРАТЬ` only and propagates the qualifier into nested
subqueries (paraphrased; see line 1336 for the original prose). The
current parser's
`query()` at `crates/parser/src/grammar/sdbl/select.rs:279-307` calls
`limitations()` for every query body it parses, INCLUDING nested
subqueries — it does NOT enforce the top-level-only constraint at
the parser level. The Slice 7-addendum **preserves** this — any
semantic restriction (HIR-level or IDE-diagnostic-level enforcement
of "ALLOWED only in top-level SELECT") is deferred to a future slice
(Slice 13 HIR reattachment, or a dedicated diagnostic).

### IDE-recovery allowances (Slice 7-addendum extension)

The following 3 quirks of the limitations parser are preserved by
Slice 7-addendum as IDE-recovery allowances. They are documented here
explicitly so future slices know which behaviours are intentional.

1. **Any-order qualifier acceptance.** DISTINCT, TOP, and ALLOWED are
   accepted in any order via the `while is_limitation_keyword(p)`
   loop. The parser does not enforce a canonical permutation.
   Cross-checked by `crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs:514-528`
   which labels both `ВЫБРАТЬ РАЗЛИЧНЫЕ ПЕРВЫЕ 10 …` and
   `SELECT TOP 50 DISTINCT …` as valid input.
2. **Duplicate-qualifier loop tolerance.** Input
   `ВЫБРАТЬ РАЗЛИЧНЫЕ РАЗЛИЧНЫЕ A` is accepted (the loop body
   re-enters on every `is_limitation_keyword` hit without
   deduplication). Semantic uniqueness is not enforced at parser
   level; the HIR consumer extracts DISTINCT and TOP without ordering
   or duplicate-qualifier legality checks.
3. **Missing-TOP-count recovery.** `top_clause` calls
   `p.expect(TokenKind::Decimal)` at
   `crates/parser/src/grammar/sdbl/select.rs:1635`. When the
   current token is not a Decimal, `Parser::expect` invokes
   `Parser::error` (`crates/parser/src/parser.rs:160-166`),
   which bumps the next non-trivia token into an `ERROR`
   sub-node attached as a direct child of `SdblTopClause`.
   For input `ВЫБРАТЬ ПЕРВЫЕ A ИЗ Т`, the `A` Ident is
   absorbed into the ERROR sub-node; the limitations loop
   then exits because the following `ИЗ` is not a limitation
   keyword. The remaining tokens are consumed by the outer
   `selected_fields` parser without identifying `ИЗ` as the
   FROM keyword (current preserved IDE-recovery boundary;
   see test
   `crates/parser/tests/sdbl_parser_tests.rs::test_slice7adn_top_missing_decimal_recovery`).
   A tighter recovery (recognise FROM/clause-keyword
   boundary, emit empty error sub-node instead of consuming)
   is deferred to Slice 12.

### Tier classification (Slice 7-addendum extension)

The **primary** SDBL grammar specification is the v8.3.27 Developer's
Reference Глава 8 «Работа с запросами» —
`https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453`. Locally
saved snapshot at
`its/dump/its_db_v8327doc_bookmark_dev_TI000000453/page.html` for
line-numbered reviewer convenience; the canonical citation target
is the public URL above.
Line 1320 of `page.html` carries the canonical EBNF skeleton

```text
ВЫБРАТЬ [РАЗРЕШЕННЫЕ] [РАЗЛИЧНЫЕ] [ПЕРВЫЕ <Количество>]
    <Список полей выборки>
[ПОМЕСТИТЬ|ДОБАВИТЬ <Имя таблицы>]
[ИЗ <Список источников>]
[ИНДЕКСИРОВАТЬ ПО [НАБОРАМ] <Список полей>]
[ГДЕ <Условие отбора>]
[СГРУППИРОВАТЬ ПО <Поля группировки>]
[ИМЕЮЩИЕ <Условие отбора>]
[ДЛЯ ИЗМЕНЕНИЯ [<Список таблиц верхнего уровня>]]
```

with all three SELECT-prefix qualifiers (РАЗРЕШЕННЫЕ, РАЗЛИЧНЫЕ,
ПЕРВЫЕ) in their canonical first-qualifier slot. Lines 1331-1356
contain prose semantics for each qualifier.

The pubqlang dump (`its/dump/html/chapter_*.html`) is the
**secondary** textbook companion — its chapter-19/20/57 examples are
demonstrative, not specificational. The Slice 7-addendum cites both
sources, with v8327doc Глава 8 as the primary grammar source.

| Keyword | Tier | Source |
|---|---|---|
| `DISTINCT` / `РАЗЛИЧНЫЕ` | **A1** | v8327doc Глава 8 §<Описание запроса> at `page.html:1320, 1346-1348` (canonical EBNF + prose). Pubqlang chapter 20 at `chapter_020.html:18, 29, 42` provides the demonstrative `ВЫБРАТЬ РАЗЛИЧНЫЕ` examples; DISTINCT × ORDER BY interaction at `chapter_020.html:38`. Bilingual word-list at `page.html:1030-1034` (РАЗЛИЧНЫЕ ↔ DISTINCT). |
| `TOP <decimal>` / `ПЕРВЫЕ <decimal>` | **A1** | v8327doc Глава 8 at `page.html:1320, 1350-1356` (canonical EBNF `ПЕРВЫЕ <Количество>` + prose covering ordering interaction and nested-query support). Pubqlang chapter 19 at `chapter_019.html:19, 28` provides the demonstrative `ВЫБРАТЬ ПЕРВЫЕ 3` example. |
| `ALLOWED` / `РАЗРЕШЕННЫЕ` | **A1** | v8327doc Глава 8 at `page.html:1320, 1331-1344` — canonical EBNF places РАЗРЕШЕННЫЕ in the first SELECT-prefix slot; prose at lines 1331-1344 covers RLS scope (records visible to current user only), top-level-only constraint, propagation into subqueries, and interaction with ЧТЕНИЕ rights. Bilingual word-list at `page.html:1040-1044` (РАЗРЕШЕННЫЕ ↔ ALLOWED). The pubqlang dump's `chapter_057.html:50` UI-checkbox prose is a corroborating secondary reference only. |
| `is_identifier_token` predicate | **C/B local parser contract** | Body `p.at(TokenKind::Ident)` is trivially derivable. Load-bearing cross-slice semantics inherited from Slice 7 alias-scan (`selected_field_alias` at `crates/parser/src/grammar/sdbl/select.rs:357, 370`) and Slice 8 source-alias guard (`source_alias` at `crates/parser/src/grammar/sdbl/select.rs:582, 600`); see `docs/legal/sdbl-clean-room-slice8.md:264-269`. |

### ITS coverage

All three SELECT-prefix qualifiers (DISTINCT, TOP, ALLOWED) are Tier
A1 with canonical EBNF + prose in v8327doc Глава 8 at
`page.html:1320, 1331-1356`.
The pubqlang chapters 19 / 20 / 57 are demonstrative (textbook
companion) and provide additional canonical examples (chapters 19 / 20)
or UI-prose (chapter 57). The Slice 7-addendum C0 codex review pass
verified that v8327doc Глава 8 §<Описание запроса> is the primary
SDBL grammar specification and lists ALLOWED in the canonical
first-qualifier position alongside DISTINCT and TOP.

## Selected fields

### Field list

```text
selected-fields := selected-field (',' selected-field)*
```

A selected field is one of:

- asterisk field
- expression-like field
- implementation-defined special field forms supported by the parser/HIR

### Asterisk field

Supported forms:

- `*`
- `Table.*`
- multi-segment forms ending in `.*` where the current parser supports them

Asterisk fields do not require aliases.

### Alias

Alias syntax:

```text
alias := [AS|КАК] identifier
```

Parser requirements:

- alias is structurally optional;
- both explicit aliases (`AS Name`) and implicit aliases (`Expr Name`) are
  accepted in the syntax tree;
- whether implicit aliasing is allowed semantically is a diagnostics concern, not
  a parser rejection rule;
- if `AS` / `КАК` is present but alias name is missing, create an error node and
  continue.

## INTO clause

```text
into-clause := (INTO|ПОМЕСТИТЬ) identifier
```

Used for temporary tables.

Parser requirements:

- temporary table name is parsed as a dedicated node;
- missing name should produce a recoverable parse error.

## FROM clause

### Data source list

```text
from-clause := (FROM|ИЗ) data-source (',' data-source)*
```

### Data source

```text
data-source := primary-source alias? join-clause*
primary-source := subquery-source | table-ref | parameter-source
subquery-source := '(' subquery ')' alias?
parameter-source := '&' identifier
```

Notes:

- subqueries in `FROM` are valid;
- parameterized table sources such as `&ТЗ` are accepted where local behavior
  already supports them;
- joins attach to the immediately preceding data source chain.

## Table references

A table reference may represent:

- simple identifier source;
- metadata object path;
- metadata object path with nested object/table segments;
- virtual table source;
- temporary table source;
- parameter source.

Examples the parser should structurally handle:

- `Справочник.Номенклатура`
- `Catalog.Products`
- `#TempTable`
- `РегистрНакопления.ОстаткиТоваров.Обороты(...)`
- `&Таблица`

Parser requirements:

- parse dot-separated identifier chains;
- support virtual table calls with parenthesized arguments;
- preserve incomplete paths for IDE usage by emitting recoverable errors instead
  of aborting the whole source.

### Virtual table argument behavior

Current parser behavior allows empty VT arguments such as:

```text
Остатки(, , Авто, )
```

This behavior is part of the compatibility contract and may be preserved.

#### Grammar (Slice 8-addendum extension)

```text
virtual-table-args := '(' [vt-arg-list] ')'
vt-arg-list       := vt-arg (',' vt-arg)*
vt-arg            := expression | <empty>
```

Empty `vt-arg` produces an `SdblMissingArg` marker as a direct child of the
enclosing `SdblTableRef`. The outer `[vt-arg-list]` makes the entire
argument-list optional, so `()` is a well-formed empty-arg-list form (see
§IDE-recovery allowance #4 below).

#### AST-shape contract (Slice 8-addendum extension)

`virtual_table_args` does NOT emit a wrapper NodeKind. Its tokens (`LParen`,
`Comma`, `RParen`), expression-node children (one of 9 expression NodeKinds
per the Slice 10a / 10b expression-backbone attestations:
`SDBL_LOGICAL_OR_EXPR`, `SDBL_LOGICAL_AND_EXPR`, `SDBL_NOT_EXPR`,
`SDBL_COMPARISON_EXPR`, `SDBL_COLUMN_REF`, `SDBL_LITERAL`,
`SDBL_MULTI_STRING`, `SDBL_FUNCTION_CALL`, `SDBL_PAREN_EXPR`),
`SdblMissingArg` empty-arg markers, and `Error` sub-nodes from
`recover_to_delimiter_vt` all attach as **flat direct children** of the
enclosing `SdblTableRef` (which itself opens at `table_ref` in Slice 8).

The HIR consumer at `crates/sdbl-hir/src/lower/from_clause.rs:246-371` walks
`SdblTableRef.syntax().children()` directly and lowers each
expression-NodeKind into `ExprHir` for the
`virtual_table_params: Vec<ExprHir>` field declared at
`crates/sdbl-hir/src/hir.rs:172`. The clean-room rewrite must preserve this
flat direct-child layout so the existing HIR walker continues working
unchanged. Acceptance tests walk `SdblTableRef.syntax().children()` and
assert the expected sequence; no per-arg wrapper is introduced.

The outer `if p.at(LParen)` guard at the top of `virtual_table_args` makes
the call site in `table_ref` unconditional — `virtual_table_args(p)` is a
no-op when its caller's next token is not `(`. This lets `table_ref` invoke
the function unconditionally without checking for `(` itself.

#### IDE-recovery allowances (Slice 8-addendum extension)

1. **Empty-leading-arg `(, ...)`** — `SdblMissingArg` is emitted before the
   first non-empty arg. Used by 1C idiom `.Остатки(, &Период, ...)` (see
   ITS coverage row for pubqlang chapter 152, which exhibits the
   leading-empty form).
2. **Empty-trailing-arg `(..., )`** — `SdblMissingArg` is emitted after the
   last comma when the parser sees `)`. Used by 1C idiom
   `.Обороты(&Начало, &Конец, , )` (one or more trailing empty args).
3. **Consecutive empty args `(, ,)`** — multiple `SdblMissingArg` siblings
   produced by the comma loop's inner branch when it sees `Comma` /
   `RParen` / non-expression-start in succession. Used by the 1C idiom
   `.ОстаткиИОбороты(, , Авто, , )` (canonical v8327doc Глава 8.3 example).
4. **Empty `()` (no args at all)** — the outer `if !p.at(RParen)` skip
   means an empty paren pair just emits `LParen` + `RParen` as flat
   children of `SdblTableRef`, with no `SdblMissingArg`. The grammar
   `[vt-arg-list]` makes the argument list optional. Mirrors the canonical
   shape `.Остатки()` for VT methods that take no parameters (see ITS
   coverage row for pubqlang chapter 152, which exhibits the no-args
   form).
5. **Mid-arg paren-balanced recovery via `recover_to_delimiter_vt`** —
   when an expression is followed by an unexpected token that's neither
   `,` nor `)`, `recover_to_delimiter_vt` opens an `Error` marker and
   consumes tokens with **paren-depth tracking**: it descends into nested
   `(...)` and only stops on top-level `,` / `;` / clause-keyword / outer
   `)` / EOF. This handles malformed input like `Остатки(СУММА(A) Q, B)`
   where the spurious `Q` between expression and comma is absorbed into
   the `Error` node without breaking the outer comma loop. The helper is
   a **safety net for malformed input only**; clean nested forms like
   `Остатки(СУММА(A))` and `Остатки(Поле В (ВЫБРАТЬ ...))` are fully
   consumed by `expression(p)` / `predicate_expr` (Slice 10b territory)
   without invoking this helper. The `Error` marker is unconditionally
   completed after the recovery loop; whether it contains token children
   depends on what the loop consumed before terminating.
6. **Empty-arg-after-comma fallback to `is_expression_start` guard** —
   when `is_expression_start(p)` returns false for a non-comma,
   non-`RParen` token (e.g. the token is a clause keyword like `ИЗ` /
   `ГДЕ`), the inner branch emits `SdblMissingArg` and breaks the
   comma loop. This lets `expression(p)`'s clause-keyword fix (Slice
   10b territory) propagate into VT-args context: e.g. `Остатки(ИЗ T)`
   does not greedily consume `ИЗ` as an arg.

#### Tier classification (Slice 8-addendum extension)

- **A1** (ITS canonical example):
  - v8327doc Глава 8.2 «Виртуальные таблицы» + Глава 8.3 «Виртуальные и
    обычные поля» canonical example
    `РегистрНакопления.УчетНоменклатуры.ОстаткиИОбороты(, , Авто, , )` at
    `https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453`. Глава 8.3
    lists 4–5 sibling examples in the same section.
  - pubqlang chapter 104 — `Обороты` VT call with date-helper function
    call inside an arg + named-condition arg in trailing slot (primary
    structural attestation for "expression-as-arg with nested function
    call" form).
  - pubqlang chapter 116 — `Обороты()` parameter-order prose
    (`НачалоПериода`, `КонецПериода`, `Субконто`, `КорСубконто`):
    confirms positional args are the canonical idiom.
  - pubqlang chapter 152 — empty `Остатки()` and leading-empty
    `Остатки( , Номенклатура = &Номенклатура)` (primary attestation for
    IDE-recovery allowance #4 (no-args) and #1 (leading-empty)).
  - pubqlang chapter 156 — multi-line VT call with `IN (ВЫБРАТЬ ...)`
    subquery as a VT param: structural attestation for the
    IN-subquery-as-VT-param form. **Not** an attestation of recovery-helper
    behaviour: clean `IN (subquery)` is consumed inside `expression(p)` /
    `predicate_expr` → `super::select::subquery(p)`, NOT by
    `recover_to_delimiter_vt` (the helper is a safety net for malformed
    input only — see §IDE-recovery allowance #5).
  - pubqlang chapter 9 — `СрезПоследних` virtual-table intro prose
    (peripheral, prose only, no VT-args structure; corroborating only).
- **C** (mini-spec): §Table references / §Virtual table argument behavior
  (this section) — grammar EBNF, AST-shape contract, IDE-recovery
  allowances, Tier classification, and §ITS coverage verification rows.
- **D** (local IDE-recovery): empty-arg patterns (`SdblMissingArg` AST
  shape) and paren-balanced recovery via `recover_to_delimiter_vt`. The
  empty-arg form IS canonical per Tier A1, but the per-arg
  `SdblMissingArg` AST shape is a parser-internal recovery node with no
  ITS source; `recover_to_delimiter_vt` is a parser-internal recovery
  utility for malformed input.

The Slice 8-addendum extension to §ITS coverage verification (the table at
the end of this document) adds rows for v8327doc Глава 8.2 / 8.3 and
pubqlang chapters 9 / 104 / 116 / 152 / 156. C2 of Slice 8-addendum fills
in the verification status by directly reading the cited material.

## JOIN clauses

### Shape

```text
join-clause :=
  [join-type] [OUTER|ВНЕШНЕЕ] (JOIN|СОЕДИНЕНИЕ)
  data-source
  (ON|ПО)
  logical-expression
```

Where `join-type` may be:

- `LEFT` / `ЛЕВОЕ`
- `RIGHT` / `ПРАВОЕ`
- `FULL` / `ПОЛНОЕ`
- `INNER` / `ВНУТРЕННЕЕ`

Behavioral note:

- bare `JOIN` without explicit type is accepted and treated structurally as a
  valid join form.

## WHERE clause

```text
where-clause := (WHERE|ГДЕ) logical-expression
```

The full expression grammar is defined elsewhere, but this clause must hand off
to the logical-expression parser and remain recoverable when the expression is
incomplete.

### AST-shape contract (Slice 11 extension)

`SdblWhereClause` has exactly one direct expression-node child — the result of
`logical_expression(p)`. The HIR consumer at
`crates/sdbl-hir/src/lower/clauses.rs:28-41` filters direct children for one of
9 expression NodeKinds: `SDBL_LOGICAL_OR_EXPR`, `SDBL_LOGICAL_AND_EXPR`,
`SDBL_NOT_EXPR`, `SDBL_COMPARISON_EXPR`, `SDBL_COLUMN_REF`, `SDBL_LITERAL`,
`SDBL_MULTI_STRING`, `SDBL_FUNCTION_CALL`, `SDBL_PAREN_EXPR`. Because
`logical_expression(p)` wraps its result in `SdblLogicalOrExpr` per the Slice
10a expression-backbone attestation, the consumer's first match is the
`SdblLogicalOrExpr` wrapper.

### KW_OR token recursive-walk reachability invariant

The IDE diagnostic `LogicalOrInWhere` (handler at
`crates/ide-diagnostics/src/handlers/logical_or_in_the_where_section_of_query.rs`)
is sourced from a recursive walk in
`crates/sdbl-hir/src/lower/clauses.rs:170-192`
(`collect_or_tokens_excluding_subqueries`). That walk visits
`children_with_tokens()` of the `SdblWhereClause` node and **recurses** into
every non-subquery child node, skipping the three subquery kinds
`SDBL_SUBQUERY`, `SDBL_SUBQUERY_EXPR`, `SDBL_SELECT_QUERY` (lines 181–186) so
nested SELECTs collect their own KW_OR tokens via their own WHERE lowering.

KW_OR tokens are therefore **NOT** direct token children of `SdblWhereClause` —
they sit inside the direct `SdblLogicalOrExpr` child wrapper, and the consumer
reaches them via the recursive walk through that wrapper. The Slice 11
`where_clause` rewrite preserves recursive-walk reachability matching
`collect_or_tokens_excluding_subqueries`: every KW_OR token must be reachable
from `SdblWhereClause` via `children_with_tokens()` recursion through any
non-subquery descendant node, regardless of where in the expression tree it
ends up.

### Recovery policy

Incomplete `logical_expression` after `WHERE`/`ГДЕ` stays recoverable per
§Recovery requirements item #5 (incomplete VT parameter list shape — extended
here to incomplete WHERE expression).

## GROUP BY

```text
group-by-clause := (GROUP|СГРУППИРОВАТЬ) (BY|ПО) group-item (',' group-item)*
group-item := expression
```

For clean-room rewrite purposes, the minimum stable requirement is:

- parse one or more comma-separated grouping expressions;
- preserve incomplete items via recoverable parser errors.

More advanced grouping forms can be added later if documented by tests and
official sources.

### AST-shape contract (Slice 11 extension)

`SdblGroupClause` has multiple direct expression-node children — one per
grouping expression in the comma-separated list. There is **no per-group-item
wrapper node** (no `SdblGroupItem` NodeKind exists). The HIR consumer at
`crates/sdbl-hir/src/lower/clauses.rs:74-82` filters direct children for one
of 3 expression NodeKinds: `SDBL_LOGICAL_OR_EXPR`, `SDBL_COLUMN_REF`,
`SDBL_FUNCTION_CALL`. The list-shape (commas, GROUP/BY tokens) is preserved
as flat sibling tokens at the same depth.

## HAVING

```text
having-clause := (HAVING|ИМЕЮЩИЕ) logical-expression
```

### AST-shape contract (Slice 11 extension)

`SdblHavingClause` shape parallels `SdblWhereClause` — exactly one direct
expression-node child. Note that the current parser for `having_clause` calls
`expression(p)` (NOT `logical_expression(p)`); this asymmetry is preserved as a
behavioural quirk because Slice 10a's expression-backbone attestation wraps both
entry points in `SdblLogicalOrExpr`, so the consumer-side filter receives the
same NodeKind shape regardless of entry point.

HAVING is NOT yet lowered to HIR (`hir.having = None` at
`crates/sdbl-hir/src/lower/mod.rs:349`); the consumer-side reader is Slice
13's responsibility. Slice 11 must emit a shape that Slice 13 can read without
parser changes — i.e. the same single-expression-direct-child shape as
`SdblWhereClause`.

## ORDER BY

```text
order-by-clause := (ORDER|УПОРЯДОЧИТЬ) (BY|ПО) order-item (',' order-item)*
order-item := expression [ASC|DESC|ВОЗР|УБЫВ] [HIERARCHY|ИЕРАРХИЯ]
```

The optional `[HIERARCHY|ИЕРАРХИЯ]` modifier is the **active Slice 11 C2
parser grammar**: `order_by_item` consumes the optional ASC/ВОЗР/DESC/УБЫВ
modifier followed by the optional HIERARCHY/ИЕРАРХИЯ modifier and emits
both as flat IDENT siblings of `SdblOrderClause` (no per-item wrapper).
The C0b regression-gate test `test_slice11_order_by_hierarchy_consumed`
(test (g)) is now an active gate and PASSES. Only the **HIR semantic
interpretation** of the HIERARCHY modifier remains deferred — see the
§HIR semantic-interpretation scope subsection below.

### AST-shape contract (Slice 11 extension)

`SdblOrderClause` direct children are arranged as a **flat interleaved
sequence**: ORDER token, BY token, then alternating expression-node children
and IDENT tokens for `ASC`/`ВОЗР`/`DESC`/`УБЫВ` and (post-Slice-11-C2)
`HIERARCHY`/`ИЕРАРХИЯ`. There is **no per-item wrapper node** — the function
named `order_by_item` parses one item but does NOT call
`m.start()`/`m.complete()`. The expression node and the modifier IDENT tokens
end up as flat siblings of the parent `SdblOrderClause`.

The HIR consumer at `crates/sdbl-hir/src/lower/clauses.rs:114-156` walks
`children_with_tokens()` alternately picking expression-node children (3
NodeKinds: `SDBL_LOGICAL_OR_EXPR`, `SDBL_COLUMN_REF`, `SDBL_FUNCTION_CALL`)
and IDENT tokens whose text matches `ASC`/`ВОЗР`/`DESC`/`УБЫВ`. The
`current_expr` state pattern flushes the previous expression when a new
expression or modifier token is found.

### HIERARCHY modifier (ITS Tier A2 — Slice 11 C2 MANDATORY FIX)

ITS chapter 27 (`https://its.1c.ru/db/pubqlang/content/27/hdoc`) explicitly
documents `УПОРЯДОЧИТЬ ПО Наименование ИЕРАРХИЯ` as canonical hierarchical-
ordering syntax (verified at `chapter_027.html:39,51`). The Slice 11 C2
clean-room rewrite extends `order_by_item` to consume the optional
HIERARCHY/ИЕРАРХИЯ modifier as a third position after the optional ASC/DESC
modifier, preserving the flat-sibling layout (the IDENT token sits next to
the expression node and the ASC/DESC token, all flat at the
`SdblOrderClause` level).

Bilingual ASC/ВОЗР, DESC/УБЫВ, HIERARCHY/ИЕРАРХИЯ pairs are attested in the
lexer Slice 2 LEGACY block at `crates/lexer/src/sdbl/mod.rs:485-489, 491-492`
(KwAsc, KwDesc, KwHierarchy variants).

**HIR semantic-interpretation scope.** The Slice 11 C2 HIERARCHY consumption
fix is **parser-only acceptance**: HIERARCHY/ИЕРАРХИЯ is reachable from
`SdblOrderClause` as a flat sibling IDENT token, but the HIR consumer at
`crates/sdbl-hir/src/lower/clauses.rs:114-156` and the HIR `OrderByItem`
struct at `crates/sdbl-hir/src/hir.rs` do NOT yet recognise it (`OrderByItem`
has no hierarchy field; the `SortDirection` lowering only reads
ASC/ВОЗР/DESC/УБЫВ). Therefore `ORDER BY A ИЕРАРХИЯ` lowers identically to
`ORDER BY A` from the IDE/semantic layer's perspective. Extending HIR is
**out of Slice 11 scope** (per plan §Constraints — `crates/sdbl-hir/**` is
read-only) and is owned by Slice 13 (sdbl-hir reattachment), which will add
a hierarchy field to `OrderByItem` and a HIR regression test. Slice 11's
contribution is the syntax-tree contract that Slice 13's reader will
consume.

### Recovery policy

Missing-BY recovery: if `BY`/`ПО` is missing after `ORDER`/`УПОРЯДОЧИТЬ`, the
clause emits `SdblOrderClause` containing only the `ORDER` token and returns
without consuming further tokens (cross-references §IDE-recovery allowance #3).

## FOR UPDATE

```text
for-update-clause := (FOR|ДЛЯ) (UPDATE|ИЗМЕНЕНИЯ) mdo-ref?
```

The trailing MDO reference is optional at parser level.

### AST-shape contract (Slice 11 extension)

`SdblForUpdate` direct children are flat tokens (no wrapper for the MDO
chain): the `FOR`/`ДЛЯ` token, the `UPDATE`/`ИЗМЕНЕНИЯ` token, then optionally
a bare `Ident` token followed by zero or more `Dot Ident` token pairs.

The MDO-chain loop is greedy until the post-`Dot` lookahead fails to be an
`Ident`. The outer guard `is_clause_keyword(p)` prevents the chain from
consuming a clause keyword that follows the FOR UPDATE clause; this
clause-keyword guard is enumerated under §IDE-recovery allowance #4.

If `UPDATE`/`ИЗМЕНЕНИЯ` is missing after `FOR`/`ДЛЯ`, the clause still emits
`SdblForUpdate` (just the FOR token plus whatever MDO-like Idents follow) for
mid-typing IDE recovery.

### ITS coverage

The FOR UPDATE / ДЛЯ ИЗМЕНЕНИЯ form does not appear in dumped ITS chapters
16–39; documented as a Tier D local IDE-recovery allowance with bilingual
support from the lexer Slice 2 LEGACY block (KwFor, KwUpdate at
`crates/lexer/src/sdbl/mod.rs:497-498, 500-501`). The Slice 9 ESCAPE-pattern
precedent applies — Tier D entries cite lexer-attested bilingual keyword pairs
without claiming an ITS source.

## INDEX BY

```text
index-by-clause := (INDEX|ИНДЕКСИРОВАТЬ) (BY|ПО) expression (',' expression)*
```

### AST-shape contract (Slice 11 extension)

`SdblIndexBy` direct children are multiple expression-node entries, parallel
to `SdblGroupClause`. There is no per-item wrapper. The clause is purely
syntactic — the parser does NOT enforce that indexed expressions correspond to
selected fields (semantic checking is Slice 13's territory).

Missing-BY recovery: if `BY`/`ПО` is missing after `INDEX`/`ИНДЕКСИРОВАТЬ`,
the clause emits `SdblIndexBy` containing only the `INDEX` token and returns
without consuming further tokens (§IDE-recovery allowance #3).

### ITS coverage

INDEX BY does not appear in dumped ITS chapters 16–39; documented as a Tier D
local IDE-recovery allowance with bilingual support from the lexer Slice 2
LEGACY block (KwIndex at `crates/lexer/src/sdbl/mod.rs:503-504`).

## AUTOORDER

```text
autoorder-clause := AUTOORDER|АВТОУПОРЯДОЧИВАНИЕ
```

### AST-shape contract (Slice 11 extension)

`SdblAutoorder` is a bare-keyword wrapper with no expression children — the
node contains only the `AUTOORDER`/`АВТОУПОРЯДОЧИВАНИЕ` token (consumed via
`eat_sdbl_keyword`). The clause is one statement long.

AUTOORDER and ORDER BY can coexist in the same query. Within a single
`select_tail_clauses` loop, three independent flags (AUTOORDER / ORDER /
TOTALS) gate the accepted starters — each tail clause is consumed by that
loop only if its flag is unset, so within one tail-clause loop each clause
appears at most once.

This `select_tail_clauses` flag scope is loop-local, NOT whole-query: the
parser also accepts ORDER BY at the `query_body_clauses` body-tail position
inside `query()`, before `select_query()` later calls `select_tail_clauses`.
Both accept points are reachable in the same query — see §AST-shape
invariant #1 in the Slice 11 plan; an input with ORDER BY in the body AND
ORDER BY in the tail will emit two `SdblOrderClause` nodes (preserved
pre-refactor behaviour, not a duplicate-detection contract). The any-order
acceptance is preserved per §Behavioral contract from current parser.

### ITS coverage

ITS chapter 17 (`https://its.1c.ru/db/pubqlang/content/17/hdoc`) attests
`АВТОУПОРЯДОЧИВАНИЕ` as canonical syntax — verified at `chapter_017.html:17,
32, 52`. Bilingual AUTOORDER/АВТОУПОРЯДОЧИВАНИЕ via the lexer Slice 2 LEGACY
block (KwAutoOrder).

## TOTALS BY

For the rewrite baseline, `TOTALS BY` is implemented as a tolerant,
recoverable, **flat-list** clause parser. The actually-supported parser-side
grammar (Slice 11 narrowed scope per the §IDE-recovery allowances split):

```text
totals-by-clause :=
  (TOTALS|ИТОГИ) totals-aggregate-list?
  (BY|ПО) totals-group-list
totals-aggregate-list := expression (',' expression)*
totals-group-list     := totals-group (',' totals-group)*
totals-group          := expression
```

### AST-shape contract (Slice 11 extension)

`SdblTotalsBy` direct children are arranged as a **flat layout**: optional
aggregate-expression list before the BY token, then the BY token, then a
flat comma-separated post-BY expression list. There is no per-group wrapper
node.

`OVERALL`/`ОБЩИЕ` falls through `is_expression_start` and is consumed as a
bare `Ident` expression (`SdblColumnRef`-shape). See §IDE-recovery allowance
#1 below — the lexer's KwOverall variant converts to `TokenKind::Ident` via
the bilingual-ident path, and the post-BY `expression(p)` call dispatches
through `column_or_function`, treating the Ident as a bare `SdblColumnRef`.
Slice 13 will own the semantic interpretation of OVERALL as a TOTALS-group
marker.

### Pre-BY clause-keyword guard

The pre-BY aggregate-expression loop checks `is_clause_keyword(p)` to break
out before consuming another clause's starter as a pre-BY aggregate. Without
this guard, `ИТОГИ ИЗ T` would consume `ИЗ` as a pre-BY expression. This
guard is preserved by the Slice 11 rewrite.

### What is NOT supported in Slice 11

The structured-modifier forms `ONLY/ТОЛЬКО`, `HIERARCHY/ИЕРАРХИЯ` (in TOTALS
context), and `PERIODS/ПЕРИОДЫ(...)` are **NOT** supported as structured
TOTALS modifiers in Slice 11. The pre-Slice-11 parser at
`crates/parser/src/grammar/sdbl/select.rs:1397-1405` carries an explicit
`// TODO: Add proper support for OVERALL, HIERARCHY, PERIODS` comment
confirming the flat-list shape is the actually-supported form. Their
structured-modifier promotion is deferred to Slice 12 (the IDE-recovery /
grammar-extension owner). Lexer keyword pairs (KwOnly, KwHierarchy,
KwPeriods) are attested in the Slice 2 LEGACY block but are not consumed as
structured modifiers under Slice 11.

### ITS coverage

ITS chapter 39 (`https://its.1c.ru/db/pubqlang/content/39/hdoc`) attests the
canonical `ИТОГИ ... ПО ОБЩИЕ, ...` example (verified at `chapter_039.html:13,
25, 29, 48, 49, 51`). Bilingual TOTALS/ИТОГИ via Slice 2 attestation;
OVERALL/ОБЩИЕ via Slice 2 LEGACY block (KwOverall).

PERIODS structured-modifier coverage was **NOT** verified in chapter 39 by
direct dump read at C0a time; Slice 11 cites only the canonical OVERALL
form. Any future PERIODS support is owned by Slice 12.

## IDE-recovery allowances (Slice 11 extension)

This block enumerates **parser-shape allowances** — implementation-detail
behaviours of the Slice 11 parser that are NOT themselves ITS-attested
language forms. Some entries cover an ITS-attested language form whose parser
shape is a local stylistic choice rather than a structured AST representation
(entry #1); others cover stylistic helper-function inconsistencies (entry #2),
recovery shapes (entry #3), and clause-keyword guard wiring (entry #4). Each
entry cross-references the corresponding §clause-body section and is
regression-pinned by a Bucket-A test added in Slice 11 C0b.

The HIERARCHY-modifier-on-ORDER-BY language form is **NOT** in this list — it
is ITS Tier A2 (chapter 27 attests `УПОРЯДОЧИТЬ ПО Наименование ИЕРАРХИЯ`)
AND its parser shape is promoted to a structured modifier-token consumption
in Slice 11 C2's `order_by_item` mandatory fix, so neither dimension is a
local allowance. Likewise, the structured PERIODS / ONLY / HIERARCHY-in-TOTALS
modifier language forms are **NOT** in this list — Slice 11 narrows the
TOTALS BY mini-spec to the actually-supported flat-list shape; structured-
modifier promotion (both the language-form scope and the parser-shape work)
is deferred to Slice 12.

1. **OVERALL / ОБЩИЕ as a TOTALS BY group — flat-Ident parser shape.** The
   `OVERALL`/`ОБЩИЕ` language form is ITS-attested (chapter 39 — see §TOTALS
   BY and the §ITS coverage verification table — canonical `ИТОГИ ПО ОБЩИЕ`).
   The local allowance documented here is the **parser shape**, NOT the
   language form: `totals_by_clause` does NOT recognise OVERALL as a
   structured TOTALS-marker NodeKind. Instead, the keyword falls through
   `is_expression_start` (the lexer's KwOverall variant at
   `crates/lexer/src/sdbl/mod.rs:509-510` converts to `TokenKind::Ident` via
   the lexer-converter bilingual-ident path), the post-BY `expression(p)`
   call dispatches through `column_or_function`, and OVERALL is consumed as
   a bare `SdblColumnRef` expression. Slice 13 will own the semantic
   interpretation of this `SdblColumnRef` shape as a TOTALS-group marker.
   The flat-Ident parser shape (rather than a structured TOTALS-marker
   NodeKind) is the Slice 11 implementation choice; promotion to a structured
   marker, if desired, is Slice 12 territory.

2. **ASC/DESC bilingual identifiers consumed via `p.at_keyword`.** The
   `order_by_item` helper calls `p.at_keyword("ASC") || p.at_keyword("ВОЗР") ||
   p.at_keyword("DESC") || p.at_keyword("УБЫВ")` directly rather than routing
   through `at_sdbl_keyword` (which goes through the Slice 2 keyword
   vocabulary). The two paths are behaviourally equivalent because
   `p.at_keyword` is case-insensitive Ident matching, but the inconsistency is
   documented; routing through `at_sdbl_keyword` is a stylistic fix candidate
   deferred to Slice 12. See §ORDER BY.

3. **Missing-keyword recoveries (early-return shape).** `where_clause`
   delegates fully to `logical_expression(p)` (which has its own internal
   recovery). `group_by_clause`, `order_by_clause`, and `index_by_clause`
   each return early on missing `BY`/`ПО` immediately after the leading
   clause keyword (GROUP / ORDER / INDEX) WITHOUT consuming the leading
   keyword as ERROR — the leading clause keyword is already consumed via
   `eat_sdbl_keyword` BEFORE the BY check, so the early-return emits a bare
   `SdblGroupClause` / `SdblOrderClause` / `SdblIndexBy` containing only the
   leading keyword.

   `totals_by_clause` has a different early-return shape: it FIRST runs a
   pre-BY aggregate-expression loop (verified at
   `crates/parser/src/grammar/sdbl/select.rs:1359-1386`) that consumes any
   expressions before the BY token, breaking on `is_clause_keyword(p)` or
   non-expression-start. Only AFTER that loop does it check for `BY`/`ПО` at
   line 1389 — if missing, it emits `SdblTotalsBy` containing the leading
   `TOTALS`/`ИТОГИ` token PLUS any pre-BY expressions that were already
   consumed. So `ИТОГИ Foo` (no BY) produces `SdblTotalsBy` with the
   `ИТОГИ` token and a `Foo` expression child, NOT a bare-keyword node.

   Documented as known IDE-recovery boundaries; see §GROUP BY, §ORDER BY,
   §INDEX BY, §TOTALS BY.

4. **`is_clause_keyword` participation in the `for_update_clause` MDO chain.**
   The optional MDO loop in `for_update_clause` uses
   `p.at(TokenKind::Ident) && !is_clause_keyword(p)` to break on clause
   keywords. This is a deliberate clause-keyword guard, mirroring Slice 10b's
   `column_or_function` clause-keyword fix at the function-call argument-list
   probes. Without the guard, the MDO chain would greedily consume a
   following clause keyword as a chain segment. Preserved by Slice 11; see
   §FOR UPDATE.

## ITS coverage verification

| Clause / form | ITS chapter | Verification status |
|---|---|---|
| WHERE / ГДЕ — primary | 22 §Условие отбора | verified yes (C2 — `chapter_022.html:15, 26, 35` — `Условие отбора данных из таблицы задается после ключевого слова ГДЕ`) |
| WHERE — pattern matching integration | 23 §LIKE+WHERE | verified yes (C2 — `chapter_023.html:13, 15, 25-27, 45-46` — `ГДЕ Наименование ПОДОБНО "%Иван%"` + `НЕ … ПОДОБНО` form) |
| WHERE — additional examples | 24 §WHERE+parameters | verified yes (C2 — `chapter_024.html:15, 16` — параметры запроса `&Клиент` в условии отбора `ГДЕ`) |
| ORDER BY — primary | 16 §Сортировка результата запроса | verified yes (C2 — `chapter_016.html:19, 31, 33, 37, 49` — `УПОРЯДОЧИТЬ ПО ... ВОЗР` canonical sort form) |
| ORDER BY — multi-level / variants | 16 §Многоуровневая сортировка + 17 §Сортировка по реквизитам | verified yes (C2 — `chapter_016.html:63, 64, 75-76` — `УПОРЯДОЧИТЬ ПО Период УБЫВ, ...` multi-field; `chapter_017.html:29, 49` — sort by ссылочное поле; chapter 16 also references HIERARCHY at `chapter_016.html:63` "Можно также упорядочивать иерархические данные по иерархии") |
| ORDER BY HIERARCHY / ИЕРАРХИЯ | 27 §Иерархическая упорядоченная выборка | verified yes (C0a — `chapter_027.html:39, 51` — `УПОРЯДОЧИТЬ ПО Наименование ИЕРАРХИЯ`); ITS Tier A2 / Slice 11 C2 MANDATORY FIX |
| AUTOORDER / АВТОУПОРЯДОЧИВАНИЕ | 17 §АВТОУПОРЯДОЧИВАНИЕ | verified yes (C0a — `chapter_017.html:17, 32, 52`) |
| GROUP BY — primary | 34 §Группировка результата запроса | verified yes (C2 — `chapter_034.html:14, 33, 44, 46, 51, 52` — `СГРУППИРОВАТЬ ПО` canonical with агрегатные функции СУММА/МИНИМУМ/МАКСИМУМ/СРЕДНЕЕ/КОЛИЧЕСТВО) |
| GROUP BY — variants / multi-field | 35 §Расчет агрегатов | verified yes (C2 — `chapter_035.html:23, 29, 41, 44, 45` — multi-field `СГРУППИРОВАТЬ ПО` example) |
| HAVING — primary | 35 §Условие на агрегаты | verified yes (C2 — `chapter_035.html:49` — `с помощью ключевого слова ИМЕЮЩИЕ ... условие отбора аналогично условию в предложении ГДЕ, но только оно накладывается ... на записи, получившиеся в результате группировки`) |
| TOTALS BY — primary (incl. OVERALL / ОБЩИЕ) | 39 §Расчет общих итогов | verified yes (C0a — `chapter_039.html:13, 25, 29, 48, 49, 51` — canonical `ИТОГИ ПО ОБЩИЕ`); structured PERIODS form NOT verified |
| FOR UPDATE / ДЛЯ ИЗМЕНЕНИЯ | (not in ITS chapters 16–39) | verified-no (C2 — direct `rg` of dumped chapters 16–39 found no `ДЛЯ ИЗМЕНЕНИЯ` / `FOR UPDATE` form) — local IDE-recovery allowance (Tier D) |
| INDEX BY / ИНДЕКСИРОВАТЬ ПО | (not in ITS chapters 16–39) | verified-no (C2 — direct `rg` of dumped chapters 16–39 found no `ИНДЕКСИРОВАТЬ` form) — local IDE-recovery allowance (Tier D) |
| DISTINCT / РАЗЛИЧНЫЕ — primary | v8327doc Глава 8 §<Описание запроса> + pubqlang 20 §ВЫБРАТЬ РАЗЛИЧНЫЕ | verified yes (C0 of Slice 7-addendum — `page.html:1320` canonical EBNF skeleton, `:1346-1348` prose explanation; `its/dump/html/chapter_020.html:18, 29, 42` — demonstrative `ВЫБРАТЬ РАЗЛИЧНЫЕ`; `chapter_020.html:38` — DISTINCT × ORDER BY validity rule); ITS Tier A1 |
| TOP / ПЕРВЫЕ — primary | v8327doc Глава 8 §<Описание запроса> + pubqlang 19 §ВЫБРАТЬ ПЕРВЫЕ | verified yes (C0 of Slice 7-addendum — `page.html:1320` canonical EBNF skeleton with `[ПЕРВЫЕ <Количество>]`, `:1350-1356` prose covering ordering interaction and nested-query support; `chapter_019.html:19, 28` — demonstrative `ВЫБРАТЬ ПЕРВЫЕ 3`); ITS Tier A1 |
| ALLOWED / РАЗРЕШЕННЫЕ — primary | v8327doc Глава 8 §<Описание запроса> | verified yes (C0 of Slice 7-addendum — `page.html:1320` canonical EBNF places `[РАЗРЕШЕННЫЕ]` in first SELECT-prefix slot; `:1331-1344` prose paraphrased: РАЗРЕШЕННЫЕ scopes the result to records the current user has rights to; constrained to the top-level ВЫБРАТЬ; propagates into subqueries; interaction with ЧТЕНИЕ-table rights documented; bilingual word-list at `:1038-1046` РАЗРЕШЕННЫЕ ↔ ALLOWED). The pubqlang dump's `chapter_057.html:50` UI-checkbox prose is a secondary corroborating reference. ITS Tier A1. |
| Virtual table arguments — primary | v8327doc Глава 8.2 «Виртуальные таблицы» — `https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453` | TODO at C2 (Slice 8-addendum) — verify VT introduction prose; ITS Tier A1 |
| Virtual table arguments — canonical example with empty-arg + named-pos slots | v8327doc Глава 8.3 «Виртуальные и обычные поля» — `https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453` | TODO at C2 (Slice 8-addendum) — verify canonical `РегистрНакопления.УчетНоменклатуры.ОстаткиИОбороты(, , Авто, , )` example and 4–5 sibling examples in the same section; ITS Tier A1 |
| Virtual table arguments — `СрезПоследних` intro (peripheral) | pubqlang chapter 9 lines 13, 20 | TODO at C2 (Slice 8-addendum) — verify intro prose; peripheral / corroborating only (Tier A1 corroborator) |
| Virtual table arguments — nested function call + named-condition trailing arg | pubqlang chapter 104 line 23 | TODO at C2 (Slice 8-addendum) — verify `Обороты(&НачалоПериода, КОНЕЦПЕРИОДА(&КонецПериода, ДЕНЬ), , Номенклатура = &Товар) КАК ПродажиОбороты` form (date-helper function call as arg + named-condition trailing arg); ITS Tier A1 (primary structural for "expression-as-arg with nested function call") |
| Virtual table arguments — `Обороты()` parameter-order prose | pubqlang chapter 116 lines 13–42 | TODO at C2 (Slice 8-addendum) — verify parameter-order doc (`НачалоПериода`, `КонецПериода`, `Субконто`, `КорСубконто`); ITS Tier A1 (primary positional-args attestation) |
| Virtual table arguments — empty `Остатки()` and leading-empty `Остатки( , cond)` | pubqlang chapter 152 lines 23, 35 | TODO at C2 (Slice 8-addendum) — verify no-args form (line 23) and leading-empty form (line 35: `Остатки( , Номенклатура = &Номенклатура)`); ITS Tier A1 (primary attestation for IDE-recovery allowances #4 and #1) |
| Virtual table arguments — IN-subquery as VT param (structural) | pubqlang chapter 156 lines 50–56 | TODO at C2 (Slice 8-addendum) — verify multi-line VT call with `Остатки( , ... В (ВЫБРАТЬ ...))`; ITS Tier A1 (structural — confirms `IN (subquery)` is a canonical VT-arg form; NOT an attestation of `recover_to_delimiter_vt` behaviour — the subquery's `)` is consumed inside `predicate_expr` → `super::select::subquery(p)` per Slice 10b) |

C2 fills in the remaining "TODO at C2" rows after directly reading the dump
pages at `/home/itrous/src/tools_migration/its/dump/html/`, mirroring the
Slice 10b C0a → C2 verification handoff.

**Line-number stability check (per D8a, applies to Slice 8-addendum
pubqlang rows above).** Each cited "pubqlang chapter NNN line MM" row in
the table above is a line-numbered reference into the pubqlang HTML
chapters. The C2 author verifies line-number stability across the public
ITS canonical version and either (a) keeps the line numbers if stable, or
(b) falls back to "pubqlang chapter NNN §<section name>" form if line
numbers cannot be confirmed stable. Section-name references are stable by
definition since they follow ITS heading anchors.

## Non-consultation statement (Slice 11 reaffirmation)

The §WHERE / §GROUP BY / §HAVING / §ORDER BY / §AUTOORDER / §TOTALS BY / §FOR
UPDATE / §INDEX BY full-body sections, the §IDE-recovery allowances block,
and the §ITS coverage verification table extension landed in commit C0a of
the Slice 11 clean-room slice were authored from:

- the previously-existing mini-spec sketches (which were authored under the
  same clean-room discipline);
- ITS pubqlang chapter regions read directly via the local dump path
  `/home/itrous/src/tools_migration/its/dump/html/`. C0a authoring read
  three targeted regions only — chapter 17 lines 17/32/52 (AUTOORDER
  provenance), chapter 27 lines 39/51 (ORDER BY HIERARCHY provenance),
  chapter 39 lines 13/25/29/48/49/51 (TOTALS BY canonical OVERALL form
  provenance). The C2 verification pass extended this with direct
  reads of chapter 16 (ORDER BY primary + multi-level variants),
  chapter 17 sort-by-ссылочное-поле lines, chapter 22 (WHERE primary),
  chapter 23 (LIKE+WHERE integration), chapter 24 (WHERE+parameters),
  chapter 34 (GROUP BY primary), chapter 35 (GROUP BY variants +
  HAVING) — see the §ITS coverage verification table for the full
  verified-yes citations with line numbers. The FOR UPDATE / INDEX BY
  "verified-no" rows reflect a direct `rg` of the dumped chapters
  16–39 confirming absence of those forms;
- the lexer Slice 2 attestation (`docs/legal/sdbl-clean-room-slice2.md`) for
  bilingual keyword pairs;
- the Slice 1/2/6/7/8/9/10a/10b clean-room attestations for event-parser
  conventions and AST-shape contracts;
- the HIR consumer code at `crates/sdbl-hir/src/lower/clauses.rs` for
  read-only documentation of consumer-side AST-shape requirements.

The author did NOT consult `../bsl-parser/*` or any pre-C1 textual transcription
of the 12 Slice-11 parser function bodies as working text during C0a authoring.

## Non-consultation statement (Slice 7-addendum reaffirmation)

The §Limitations §AST-shape contract / §IDE-recovery allowances /
§Tier classification / §ITS coverage subsections, plus the three new
rows in the §ITS coverage verification table (DISTINCT, TOP, ALLOWED),
landed in commit C0 of the Slice 7-addendum and were authored from:

- the previously-existing brief §Limitations sketch in this mini-spec
  (which was authored under the same clean-room discipline);
- the **primary** SDBL grammar specification: v8.3.27 Developer's
  Reference Глава 8 «Работа с запросами» —
  `https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453`. Locally
  saved snapshot at
  `its/dump/its_db_v8327doc_bookmark_dev_TI000000453/page.html` for
  line-numbered reviewer convenience; the canonical citation target
  is the public URL above. Per codex Round-4 finding 5 (LOW),
  citations are line-based and excerpts kept minimal:
  - `page.html:1320` — canonical EBNF skeleton for `<Описание
    запроса>` placing РАЗРЕШЕННЫЕ, РАЗЛИЧНЫЕ, and ПЕРВЫЕ
    `<Количество>` in the first three optional SELECT-prefix
    slots (see line for the full skeleton including ПОМЕСТИТЬ |
    ДОБАВИТЬ, ИЗ, ИНДЕКСИРОВАТЬ ПО НАБОРАМ, ГДЕ, СГРУППИРОВАТЬ ПО,
    ИМЕЮЩИЕ, ДЛЯ ИЗМЕНЕНИЯ);
  - `page.html:1331-1344` — RLS-scope prose for РАЗРЕШЕННЫЕ
    (top-level-only constraint; propagation into subqueries;
    interaction with ЧТЕНИЕ rights);
  - `page.html:1346-1348` — duplicate-elimination prose for
    РАЗЛИЧНЫЕ;
  - `page.html:1350-1356` — limit / ordering / nested-query prose
    for ПЕРВЫЕ;
  - `page.html:1030-1034` — bilingual pair РАЗЛИЧНЫЕ ↔ DISTINCT;
    `page.html:1040-1044` — bilingual pair РАЗРЕШЕННЫЕ ↔ ALLOWED;
    `page.html:920-924` — bilingual pair ПЕРВЫЕ ↔ TOP;
- the **secondary** ITS pubqlang dump (textbook companion) at
  `/home/itrous/src/tools_migration/its/dump/html/` — chapter 19
  lines 19/28 (TOP / ПЕРВЫЕ canonical demonstrative example),
  chapter 20 lines 18/29/38/42 (DISTINCT / РАЗЛИЧНЫЕ canonical
  demonstrative example + DISTINCT × ORDER BY interaction), chapter
  57 line 50 (UI-checkbox prose listing "Разрешенные" as a query
  designer GUI flag — corroborating only);
- the lexer Slice 2 attestation (`docs/legal/sdbl-clean-room-slice2.md`)
  for bilingual keyword pairs (Slice-2-LEGACY KwAllowed at
  `crates/lexer/src/sdbl/mod.rs:470, 494`);
- the Slice 1/2/6/7/8/9/10a/10b/11 clean-room attestations for
  event-parser conventions and AST-shape contracts (in particular the
  Slice 8 attestation at
  `docs/legal/sdbl-clean-room-slice8.md:264-269` for the
  `is_identifier_token` cross-slice contract);
- the HIR consumer code at
  `crates/sdbl-hir/src/lower/select_fields.rs:45-90` for read-only
  documentation of consumer-side DISTINCT / TOP extraction (no ALLOWED
  consumer exists at HIR level);
- the IDE-diagnostics test gates at
  `crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs:442-491,
  514-528` for cross-checking any-order acceptance.

The author did NOT consult `../bsl-parser/*` or any pre-C1 textual
transcription of the 4 Slice-7-addendum parser function bodies
(`is_limitation_keyword`, `limitations`, `top_clause`,
`is_identifier_token`) as working text during C0 authoring.

The first-pass codex adversarial review (Rounds 1-3) classified
ALLOWED as Tier D / B-contested on the strength of the pubqlang dump
alone (chapter 57:50 UI prose). After the v8327doc Глава 8 download
landed at `v8327doc snapshot directory`,
ALLOWED was reclassified to **Tier A1** because the developer's
reference is the primary SDBL grammar specification and lists
РАЗРЕШЕННЫЕ in the canonical first-SELECT-prefix-slot at
`page.html:1320` with full prose at lines 1331-1344. The Round-3
"Plan is IMPLEMENTATION-READY" verdict was issued before this
discovery; a Round-4 codex pass verifies the reclassification before
C0 is committed.

## Non-consultation statement (Slice 8-addendum reaffirmation)

The §Virtual table argument behavior subsection extension (Grammar EBNF,
AST-shape contract, IDE-recovery allowances 1–6, Tier classification),
plus the seven new rows in the §ITS coverage verification table for
v8327doc Глава 8.2 / 8.3 and pubqlang chapters 9 / 104 / 116 / 152 / 156,
landed in commit C0a of the Slice 8-addendum and were authored from:

- the previously-existing 3-line §Virtual table argument behavior sketch
  in this mini-spec (which was authored under the same clean-room
  discipline);
- the **primary** SDBL grammar specification: v8.3.27 Developer's
  Reference Глава 8 «Работа с запросами» —
  `https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453`. C0a authoring
  cited the canonical example
  `РегистрНакопления.УчетНоменклатуры.ОстаткиИОбороты(, , Авто, , )` from
  Глава 8.3 «Виртуальные и обычные поля» plus the VT introduction prose
  in Глава 8.2 «Виртуальные таблицы»; full per-line verification of these
  citations is performed at C2;
- the **secondary** ITS pubqlang dump (textbook companion) — chapter 9
  lines 13/20 (`СрезПоследних` intro prose, peripheral), chapter 104
  line 23 (`Обороты` with date-helper nested function call + named-
  condition trailing arg), chapter 116 lines 13–42 (`Обороты()`
  parameter-order prose: `НачалоПериода` / `КонецПериода` / `Субконто` /
  `КорСубконто`), chapter 152 lines 23/35 (no-args `Остатки()` and
  leading-empty `Остатки( , Номенклатура = &Номенклатура)`), chapter 156
  lines 50–56 (multi-line VT call with `IN (ВЫБРАТЬ ...)` subquery as a
  VT param — structural attestation only). C0a authoring identified
  these chapter regions; full per-line verification is performed at C2;
- the lexer Slice 2 attestation (`docs/legal/sdbl-clean-room-slice2.md`)
  for cross-checking that VT-args use `Ident` pass-through (`Авто`,
  `Остатки`, `Обороты`, `СрезПоследних`, `ОстаткиИОбороты` are regular
  identifiers, NOT lexer keywords);
- the Slice 1/2/6/7/8/9/10a/10b/11/7-addendum clean-room attestations for
  event-parser conventions and AST-shape contracts (in particular the
  Slice 8 attestation for the `table_ref` MDO chain that hosts the
  `virtual_table_args` call, and the Slice 10a / 10b attestations for the
  9-NodeKind expression backbone that produces the expression-arg
  children);
- the HIR consumer code at `crates/sdbl-hir/src/lower/from_clause.rs:246-371`
  for read-only documentation of consumer-side AST-shape requirements
  (the existing `virtual_table_params: Vec<ExprHir>` lowering walks
  `SdblTableRef.syntax().children()` directly; the parser's flat
  direct-child layout is what the HIR walker reads).

The author did NOT consult `../bsl-parser/*` or any pre-C1 textual
transcription of the 2 Slice-8-addendum parser function bodies
(`virtual_table_args_legacy`, `recover_to_delimiter_vt`) as working text
during C0a authoring. Per the user's citation-policy directive, this
§Non-consultation statement and the new §Virtual table argument behavior
sub-sections (Grammar / AST-shape contract / IDE-recovery allowances /
Tier classification) cite only the public ITS URL
(`https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453`) and pubqlang
chapter identifiers (e.g. "pubqlang chapter 104 line 23") with optional
stable line numbers per §D8a; no local mirror paths appear in the new
content. This is the first SDBL clean-room slice authored under that
prospective policy; prior slices (Slice 7-addendum and earlier) retain
their pre-policy citation form by deliberate non-revision (per memory
`feedback_citation_policy.md` item #7).

## Recovery requirements

This rewrite is for an IDE parser, not just batch validation. The parser must
keep recovering after local damage.

Critical recovery scenarios:

1. incomplete selected field before `FROM`
2. incomplete field in the middle of a field list
3. incomplete alias after `AS` / `КАК`
4. incomplete table reference after dot
5. incomplete VT parameter list
6. incomplete join condition after `ON` / `ПО`
7. incomplete `GROUP BY` / `ORDER BY` lists

Recovery goal:

- emit a local error node;
- continue parsing the rest of the query package;
- preserve later structural clauses whenever possible.

## Behavioral contract from current parser

The clean-room rewrite should preserve these high-value behaviors where possible:

- bilingual keywords (`RU`/`EN`);
- case-insensitive keyword acceptance;
- flexible limitation ordering;
- accepted implicit aliases in syntax tree;
- recoverable field-list parsing;
- subqueries in `FROM`;
- `UNION` and `UNION ALL`;
- VT arguments with empty placeholders;
- lossless tree with trivia retained.

## Out of scope for this mini-spec

The following should be specified separately if needed:

- full SDBL expression precedence and predicate grammar;
- `VALUE()` detailed syntax;
- metadata object type grammar;
- semantic interpretation of joins, aliases, and virtual tables;
- diagnostics rules layered on top of the syntax tree.

## Recommended implementation sequence

1. Rebuild clause skeleton:
   - `select-query`
   - `subquery`
   - `query`
2. Rebuild field list and alias handling.
3. Rebuild `FROM` + `data-source` + `JOIN`.
4. Rebuild `WHERE`, `GROUP BY`, `HAVING`, `ORDER BY`.
5. Rebuild `INTO`, `FOR UPDATE`, `INDEX BY`, `AUTOORDER`, `TOTALS BY`.
6. Reintroduce recovery scenarios from tests one group at a time.

## Success criterion

The rewrite is successful when:

- it is implementable from this document plus official 1C docs and local tests;
- it no longer needs textual consultation of `bsl-parser` grammar files;
- behavior-critical parser tests can be ported or rewritten against the new
  structure without relying on upstream grammar text.
