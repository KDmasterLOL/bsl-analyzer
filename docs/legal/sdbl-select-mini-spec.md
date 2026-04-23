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

## HAVING

```text
having-clause := (HAVING|ИМЕЮЩИЕ) logical-expression
```

## ORDER BY

```text
order-by-clause := (ORDER|УПОРЯДОЧИТЬ) (BY|ПО) order-item (',' order-item)*
order-item := expression [ASC|DESC|ВОЗР|УБЫВ]
```

Hierarchy or other extended ordering modifiers may be added if preserved by
current compatibility tests.

## FOR UPDATE

```text
for-update-clause := (FOR|ДЛЯ) (UPDATE|ИЗМЕНЕНИЯ) mdo-ref?
```

The trailing MDO reference is optional at parser level.

## INDEX BY

```text
index-by-clause := (INDEX|ИНДЕКСИРОВАТЬ) (BY|ПО) expression (',' expression)*
```

## AUTOORDER

```text
autoorder-clause := AUTOORDER|АВТОУПОРЯДОЧИВАНИЕ
```

## TOTALS BY

For the rewrite baseline, `TOTALS BY` should be implemented as a tolerant,
recoverable clause parser rather than as a verbatim transcription of a legacy
grammar.

Minimum requirement:

```text
totals-by-clause := (TOTALS|ИТОГИ) totals-prefix? (BY|ПО) totals-item (',' totals-item)*
totals-item := expression
```

Extended forms such as `OVERALL`, `ONLY`, `HIERARCHY`, `PERIODS(...)` may be
added incrementally from tests and official documentation.

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
