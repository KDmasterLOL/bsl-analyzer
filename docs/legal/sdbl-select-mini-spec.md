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

Supported limitation keywords:

- `DISTINCT` / `РАЗЛИЧНЫЕ`
- `TOP <decimal>` / `ПЕРВЫЕ <decimal>`
- `ALLOWED` / `РАЗРЕШЕННЫЕ`

Parser requirement:

- accept each of these before the field list;
- accept flexible ordering for robustness and recovery;
- do not require exact permutation reproduction from any existing ANTLR grammar.

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

The optional `[HIERARCHY|ИЕРАРХИЯ]` modifier in this BNF describes the
**post-Slice-11-C2 target grammar**, NOT the pre-Slice-11 parser shape. As of
C0a (this commit), `order_by_item` at
`crates/parser/src/grammar/sdbl/select.rs:1235-1246` only consumes the
ASC/DESC/ВОЗР/УБЫВ modifiers and leaves HIERARCHY in the token stream. The
HIERARCHY consumption ships in Slice 11 C2 as a structured modifier-token
extension to `order_by_item`, atomic with unignoring the C0b regression-gate
test `test_slice11_order_by_hierarchy_consumed` (test (g)). See the
§HIERARCHY modifier subsection below.

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
| WHERE / ГДЕ — primary | 22 §Условие отбора | TODO at C2 |
| WHERE — pattern matching integration | 23 §LIKE+WHERE | TODO at C2 |
| WHERE — additional examples | 24 §WHERE+subqueries | TODO at C2 |
| ORDER BY — primary | 16 §Сортировка результата запроса | TODO at C2 |
| ORDER BY — multi-level / variants | 17 §Многоуровневая сортировка | TODO at C2 |
| ORDER BY HIERARCHY / ИЕРАРХИЯ | 27 §Иерархическая упорядоченная выборка | verified yes (C0a — `chapter_027.html:39, 51` — `УПОРЯДОЧИТЬ ПО Наименование ИЕРАРХИЯ`); ITS Tier A2 / Slice 11 C2 MANDATORY FIX |
| AUTOORDER / АВТОУПОРЯДОЧИВАНИЕ | 17 §АВТОУПОРЯДОЧИВАНИЕ | verified yes (C0a — `chapter_017.html:17, 32, 52`) |
| GROUP BY — primary | 34 §Группировка результата запроса | TODO at C2 |
| GROUP BY — variants / HAVING examples | 35 §Расчет агрегатов | TODO at C2 |
| HAVING — primary | 34 §Группировка с фильтрацией по агрегатам | TODO at C2 |
| TOTALS BY — primary (incl. OVERALL / ОБЩИЕ) | 39 §Расчет общих итогов | verified yes (C0a — `chapter_039.html:13, 25, 29, 48, 49, 51` — canonical `ИТОГИ ПО ОБЩИЕ`); structured PERIODS form NOT verified |
| FOR UPDATE / ДЛЯ ИЗМЕНЕНИЯ | (not in ITS chapters 16–39) | verified-no — local IDE-recovery allowance (Tier D) |
| INDEX BY / ИНДЕКСИРОВАТЬ ПО | (not in ITS chapters 16–39) | verified-no — local IDE-recovery allowance (Tier D) |

C2 fills in the remaining "TODO at C2" rows after directly reading the dump
pages at `/home/itrous/src/tools_migration/its/dump/html/`, mirroring the
Slice 10b C0a → C2 verification handoff.

## Non-consultation statement (Slice 11 reaffirmation)

The §WHERE / §GROUP BY / §HAVING / §ORDER BY / §AUTOORDER / §TOTALS BY / §FOR
UPDATE / §INDEX BY full-body sections, the §IDE-recovery allowances block,
and the §ITS coverage verification table extension landed in commit C0a of
the Slice 11 clean-room slice were authored from:

- the previously-existing mini-spec sketches (which were authored under the
  same clean-room discipline);
- the three targeted ITS pubqlang chapter regions that were directly read
  at C0a time via the local dump path
  `/home/itrous/src/tools_migration/its/dump/html/`: chapter 17 lines
  17/32/52 (AUTOORDER provenance only — the multi-level / variants ORDER BY
  material elsewhere in chapter 17 was NOT read at C0a and remains a TODO
  row in the §ITS coverage verification table); chapter 27 lines 39/51
  (ORDER BY HIERARCHY provenance); chapter 39 lines 13/25/29/48/49/51
  (TOTALS BY canonical OVERALL form provenance). These three regions
  underpin the three "verified yes (C0a)" rows in the §ITS coverage
  verification table. The remaining table rows (chapters 16, 22, 23, 24,
  34, 35, plus the chapter 17 ORDER BY variants row) carry "TODO at C2"
  and were NOT directly read during C0a authoring — their verification is
  C2's responsibility per the §ITS coverage verification table footer;
- the lexer Slice 2 attestation (`docs/legal/sdbl-clean-room-slice2.md`) for
  bilingual keyword pairs;
- the Slice 1/2/6/7/8/9/10a/10b clean-room attestations for event-parser
  conventions and AST-shape contracts;
- the HIR consumer code at `crates/sdbl-hir/src/lower/clauses.rs` for
  read-only documentation of consumer-side AST-shape requirements.

The author did NOT consult `../bsl-parser/*` or any pre-C1 textual transcription
of the 12 Slice-11 parser function bodies as working text during C0a authoring.

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
