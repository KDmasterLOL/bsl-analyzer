# SDBL Expressions Mini-Spec

## Purpose

This document defines a clean-room mini-spec for the **expression**
sub-grammar of the SDBL parser. It is the implementation basis for
rewriting `crates/parser/src/grammar/sdbl/expressions.rs` without
relying on the ANTLR-shaped grammar text from the `bsl-parser`
project.

It is the companion document to
[`sdbl-select-mini-spec.md`](sdbl-select-mini-spec.md), which
explicitly defers expressions (§Out of scope: "full SDBL expression
precedence and predicate grammar"). This document fills that gap for
the Slice 10a + 10b clean-room rewrites.

## Primary sources

Authoritative sources for this mini-spec — 1C ITS pubqlang chapters
(in canonical English / Russian title order). The publicly reachable
URLs at `its.1c.ru/db/pubqlang/...` are paywalled and serve
JS-rendered navigation only; the working copy used for verification
is the local dump at `<ITS pubqlang dump>/`
(file `index.json` maps each URL to a `chapter_NNN.html` snapshot).

- `Язык запросов «1С:Предприятия»` — short overview chapter:
  - `https://its.1c.ru/db/pubqlang/content/10/hdoc`
- `Синтаксис текста запросов` — bilingual-keywords mention,
  list of query sections (описание / объединение / упорядочивание /
  автоупорядочивание / итоги):
  - `https://its.1c.ru/db/pubqlang/content/12/hdoc`
- `Как получить записи из таблицы, отобранные по некоторому
  условию` — WHERE clause; **logical operator precedence ladder
  (NOT > AND > OR) verbatim**; `И` / `ИЛИ` / `НЕ` operator
  inventory; `МЕЖДУ` (BETWEEN); parens-override-precedence rule:
  - `https://its.1c.ru/db/pubqlang/content/22/hdoc`
- `Примеры использования выражений в списке полей выборки запроса`
  — literal forms (число, строка, Истина/Ложь, Null, Неопределено);
  arithmetic operators (+, −, /, *) with explicit exclusion of `%`;
  string concatenation `+`; ВЫБОР (CASE), ВЫРАЗИТЬ (CAST),
  ССЫЛКА (REFS), built-in functions (ДЕНЬ / МЕСЯЦ / ГОД / etc.),
  aggregates (СУММА / МИНИМУМ / МАКСИМУМ / СРЕДНЕЕ / КОЛИЧЕСТВО):
  - `https://its.1c.ru/db/pubqlang/content/40/hdoc`
- `Передача параметров в запрос` — `&Identifier` parameter prefix
  syntax; ПОДОБНО (LIKE):
  - `https://its.1c.ru/db/pubqlang/content/60/hdoc`
- `Как получить записи таблицы, содержащие строки, соответствующие
  заданному шаблону` — ПОДОБНО (LIKE) pattern-matching primitive
  (Slice 10b primary source):
  - `https://its.1c.ru/db/pubqlang/content/23/hdoc`
- `Как получить записи иерархической таблицы и расположить их в
  порядке иерархии` — ЕСТЬ NULL canonical example
  (Slice 10b primary source):
  - `https://its.1c.ru/db/pubqlang/content/27/hdoc`
- `Как получить записи иерархической таблицы, находящиеся в
  иерархии выбранной группы` — В ИЕРАРХИИ canonical example
  (Slice 10b primary source):
  - `https://its.1c.ru/db/pubqlang/content/32/hdoc`

Secondary sources:

- `crates/lexer/src/sdbl/mod.rs` (Slice 1 + 2 clean-room) — for the
  canonical `TokenKind` mapping of operator and keyword lexemes;
- `crates/parser/src/parser.rs` — for the project's own event-parser
  conventions established in Slices 6 / 7 / 8;
- the project's own attestations under `docs/legal/sdbl-clean-room-slice{1,2,6,7,8,10a}.md`
  — for the per-slice clean-room discipline mirrored here.

## Clean-room constraints

When implementing from this spec:

1. Do not consult `../bsl-parser/*` grammar files during coding.
2. If a language rule is unclear, resolve it from official 1C docs or
   from an explicitly documented local behavior decision recorded in
   §IDE-recovery allowances.
3. If current parser behavior is preserved for IDE recovery rather
   than for strict syntax conformance, say so in code comments and
   in the Slice 10a / 10b attestation §Preserved pre-refactor
   behaviours.

## Licensing note

As a practical working assumption, the SDBL language — including its
expression operators, predicates, function vocabulary, and
expression-level keywords (`AND`, `OR`, `NOT`, `IS`, `IN`,
`BETWEEN`, `LIKE`, `REFS`, `CASE`, `WHEN`, `THEN`, `ELSE`, `END`,
`TRUE`, `FALSE`, `NULL`, `UNDEFINED`, etc.) — belongs to the 1C
platform as a language specification. Only 1C can realistically claim
rights in the official language definition itself.

That means:

- the existence of these operators / predicates / keywords is not
  proprietary to `bsl-parser`;
- third-party projects may still hold rights in their own grammar
  text, examples, implementation code, and documentation;
- but a concrete grammar text or its close adaptation may still be
  copyrightable expression.

Generic `MIT`-licensed SQL grammars may be used as inspiration for
expression-parser shape, precedence-climbing techniques, or recovery
patterns, but they must not become a substitute for SDBL-specific
specification. SDBL-specific rules should come from 1C docs and
independently authored tests.

## Scope

This mini-spec covers:

- expression entry points (`expression`, `logical_expression`);
- operator precedence ladder (logical, NOT, comparison/predicate,
  arithmetic, unary, primary);
- atoms (literals, parameters, identifiers, parens, tuples,
  subqueries in expression position, the bare `*` for `COUNT(*)`);
- trivia handling convention for operator probes;
- recovery contract for the `is_expression_start` /
  `is_recovery_point` / `recover_to_delimiter` /
  `parse_delimited_list` helper family;
- AST-shape contracts that downstream consumers
  (`crates/sdbl-hir/src/lower/expr/**`, `crates/syntax/src/ast.rs`,
  `crates/ide-diagnostics/src/handlers/multiline_string_in_query.rs`,
  `crates/ide-diagnostics/src/handlers/query_parse_error.rs`) read
  beyond NodeKind identity.

It now also covers (added in the Slice 10b extension):

- predicate bodies (IN, IN HIERARCHY, IS NULL, BETWEEN, LIKE, REFS);
- column references, function call argument shape, and inline
  tabular field syntax;
- CAST type specification (`ВЫРАЗИТЬ(... КАК ...)`);
- CASE expression body (WHEN / THEN / ELSE / END).

It does **not** cover:

- specialised function vocabulary at the lexer / type-checker layer
  — Slice 4 owns the lexer-level vocabulary for aggregate / date /
  string / type-helper functions; Slice 13 owns type-level CAST
  checking (matching `parseCastType` output against the
  `bsl-metadata` MDO catalog).

## Lexical assumptions

The expression parser must assume the post-conversion token stream
emitted by `crates/parser/src/sdbl_token_converter.rs` (the SDBL
`SdblTokenKind` lexer variants → BSL `TokenKind` mapping). The
canonical conversion table for keywords the expression parser
inspects is:

| Source SDBL token (RU/EN) | `SdblTokenKind` | Parser-visible `TokenKind` | How the parser probes it |
|---|---|---|---|
| `И` / `AND` | `OpAnd` | `KwAnd` | `p.at(TokenKind::KwAnd)` |
| `ИЛИ` / `OR` | `OpOr` | `KwOr` | `p.at(TokenKind::KwOr)` |
| `НЕ` / `NOT` | `OpNot` | `KwNot` | `p.at(TokenKind::KwNot)` |
| `В` / `IN` | `KwIn` | `KwIn` | `p.at(TokenKind::KwIn)` |
| `ИСТИНА` / `TRUE` | `LitTrue` | `KwTrue` | `p.at(TokenKind::KwTrue)` |
| `ЛОЖЬ` / `FALSE` | `LitFalse` | `KwFalse` | `p.at(TokenKind::KwFalse)` |
| `УНДЕФИНЕД` / `UNDEFINED` | `LitUndefined` | `KwUndefined` | `p.at(TokenKind::KwUndefined)` |
| `NULL` (single spelling) | `LitNull` | `Ident` | `p.at_keyword("NULL")` |
| `CASE` / `ВЫБОР` | (`Ident` family) | `Ident` | `p.at_keyword("CASE")` / `p.at_keyword("ВЫБОР")` |
| `WHEN` / `КОГДА`, `THEN` / `ТОГДА`, `ELSE` / `ИНАЧЕ`, `END` / `КОНЕЦ` | (`Ident` family) | `Ident` | `p.at_keyword("…")` |
| `BETWEEN` / `МЕЖДУ`, `LIKE` / `ПОДОБНО`, `IS` / `ЕСТЬ`, `HIERARCHY` / `ИЕРАРХИИ`, `REFS` / `ССЫЛКА`, `ESCAPE` / `СПЕЦСИМВОЛ` | (`Ident` family) | `Ident` | `p.at_keyword("…")` |

Two boundary notes:

- **`NULL` is NOT a dedicated parser TokenKind.** The converter at
  `crates/parser/src/sdbl_token_converter.rs` maps
  `LitNull → TokenKind::Ident` (the converter source carries the
  comment "FIXED (treated as keyword in SDBL)" on that mapping). A
  pre-Slice-10a-C2 `is_expression_start` listed
  `Some(TokenKind::KwNull) => true`, but that arm was unreachable
  dead code — the converter never produces `KwNull`. Slice 10a C2
  dropped the dead arm. After C2 the recognition paths are:
  `is_expression_start` accepts `NULL` through the generic
  non-clause-keyword `Ident` arm (the `_` fallback's
  `at_keyword("NULL")` probe is unreachable under the current
  `Parser::at_keyword` API — see §Recovery contract
  `is_expression_start` for details); `primary_expr` performs the
  **decisive**
  dispatch with an `at_keyword("NULL")` probe **before** the
  generic `Ident → column_or_function` match arm so a bare `NULL`
  literal emits `SdblLiteral` rather than `SdblColumnRef`. The
  landed regression gates are
  `test_slice10a_bare_null_emits_literal_not_column_ref` and
  `test_slice10a_select_field_null_emits_literal` in
  `crates/parser/tests/sdbl_parser_tests.rs`.
- **`IN` IS a dedicated parser TokenKind.** The converter maps
  `KwIn → TokenKind::KwIn`. The Slice 10b legacy
  `predicate_expr_legacy` (under the LEGACY banner in
  `expressions.rs`) probes via `p.at(TokenKind::KwIn)`, which is
  the correct dedicated-TokenKind contract.

Other `TokenKind` variants the expression parser inspects:

- numerical / lexical: `Decimal`, `Float`, `String`, `Ident`,
  `Whitespace`, `Newline`, `Comment`;
- punctuation: `LParen`, `RParen`, `Comma`, `Dot`, `Semicolon`;
- arithmetic / comparison: `Plus`, `Minus`, `Star`, `Slash`,
  `Percent`, `Eq`, `Neq`, `Lt`, `Le`, `Gt`, `Ge`;
- parameter prefix: `Ampersand`.

Trivia (`Whitespace`, `Newline`, `Comment`) is preserved in the
lossless tree but skipped for structural decisions.

## Expression entry points

The parser exposes two `pub fn` entries to the surrounding `select`
grammar:

```text
expression          :=  logicalExpression
logicalExpression   :=  logicalOrExpression
```

Today they are functionally equivalent — `expression` exists as a
distinct entry to leave room for a future Slice 12 split between
"general expression" (used in SELECT field positions) and "logical
expression" (used in WHERE / HAVING / ON contexts) — but both bodies
delegate to the same operator chain. The split is preserved in
Slice 10a for scope-discipline reasons (changing the call sites in
`select.rs` would touch Slice 7 / 8 / 11 territory).

## Operator precedence ladder

The expression chain proceeds, lowest binding to highest:

```text
logicalOrExpression       OR   (lowest binding)
logicalAndExpression      AND
notExpression             NOT (prefix, right-recursive)
comparisonExpression      = <> < <= > >= and predicate tail
                          (delegated to predicate body)
additiveExpression        + −
multiplicativeExpression  * / %
unaryExpression           + − NOT (prefix, right-recursive)
primaryExpression         atoms                       (highest binding)
```

**Source attribution.** The logical-operator precedence
`NOT > AND > OR` is **ITS-derived from pubqlang/22 §Условие отбора**,
which states verbatim: «В условиях сначала вычисляются простые
логические выражения, затем операции НЕ, затем операции И, в
последнюю очередь – операции ИЛИ. Для того чтобы обеспечить другой
порядок вычислений, можно использовать круглые скобки.» This
sentence specifies both the binding order (NOT tightest, OR loosest)
and the parens-override rule. The arithmetic operator inventory
`+ − × ÷` and the `+`-as-string-concatenation operator are
**ITS-derived from pubqlang/40 §Арифметические операции**. The
relative binding strength between the comparison/predicate slot and
the arithmetic chain (multiplicative tighter than additive tighter
than comparison) is the standard SQL convention universally used in
SDBL-flavored query implementations and is not in itself ITS-quoted;
this mini-spec adopts the standard convention. The clean-room rule
holds: the source record above does not consult any third-party SQL
grammar text or `../bsl-parser/*` — only ITS chapters and the
project's own event-parser conventions.

The chosen binding strengths produce the following intuitions:

- `a OR b AND c` parses as `a OR (b AND c)`;
- `a AND b = c` parses as `a AND (b = c)`;
- `a + b * c` parses as `a + (b * c)`;
- `NOT a AND b` parses as `(NOT a) AND b` — `NOT` binds tighter than
  `AND` because `not_expr` is the `logicalAndExpression` operand.

The Slice 10b body inherits the `comparison/predicate` slot — that is
where `IN`, `BETWEEN`, `LIKE`, `IS NULL`, `REFS`, and the comparison
operator tail land.

## Atoms

`primaryExpression` dispatches by leading token. Order is significant
— literal-keyword probes (`CASE/ВЫБОР`, bare `NULL`) **must** run
before generic `Ident` dispatch into `columnOrFunctionCall`, because
those tokens arrive as `Ident` from the converter and would otherwise
be consumed as column references:

```text
primaryExpression
  := caseExpression                         (at_keyword "CASE" / "ВЫБОР")        ← keyword probe FIRST
   | nullLiteral                            (at_keyword "NULL")                  ← keyword probe FIRST
   | parenOrSubqueryExpression              (LPAREN)
   | parameterExpression                    (Ampersand + Ident)
   | literalExpression                      (Decimal | Float | String | KwTrue | KwFalse | KwUndefined)
   | starLiteral                            (Star — emits SdblLiteral)
   | columnOrFunctionCall                   (generic Ident — must NOT match the keyword probes above)
   | error-fallback (SdblError)             (anything else)
```

**Historical note (pre-Slice-10a-C2 bug, fixed in C2).** Before
Slice 10a C2 the implementation only probed for `CASE` / `ВЫБОР`
before the generic `Ident` arm and then routed the `Ident` match to
`column_or_function`. Because the converter at
`sdbl_token_converter.rs:57` maps `LitNull → TokenKind::Ident`, the
historical `Some(TokenKind::KwNull)` arm in `is_expression_start`
and `primary_expr` was unreachable dead code, and bare `NULL` at
expression-head positions was silently consumed as an
`SdblColumnRef` rather than an `SdblLiteral`. Slice 10a C2 fixed
this by adding the `at_keyword("NULL")` probe to `primary_expr`
before the `Ident → column_or_function` arm and dropping the dead
`KwNull` arm from `is_expression_start`. Regression gates live in
`crates/parser/tests/sdbl_parser_tests.rs`:
`test_slice10a_bare_null_emits_literal_not_column_ref` (WHERE side,
`ВЫБРАТЬ * ИЗ Т ГДЕ Поле = NULL`) and
`test_slice10a_select_field_null_emits_literal` (SELECT side,
`ВЫБРАТЬ NULL ИЗ Т`); both assert the NULL token's direct parent
kind is `SDBL_LITERAL` and that no `SdblColumnRef` in the tree
contains the NULL token. A function-call-args case
(`Аргумент(NULL)`) is Slice 10b territory and is deferred to the
Slice 10b acceptance suite.

### Literals

```text
literalExpression
  := numericLiteral                         (Decimal | Float)
   | stringLiteralOrMulti
   | booleanLiteral                         (KwTrue | KwFalse — bilingual)
   | nullLiteral                            (KwUndefined | Ident-at-keyword "NULL")
```

`numericLiteral`, `booleanLiteral`, `nullLiteral` each emit
`SdblLiteral` wrapping the single token.

### String literal — single vs multi

A consecutive run of `String` tokens is collected into a single
expression-level node:

```text
stringLiteralOrMulti
  := String+
     ↦  if count == 1 → SdblLiteral
        if count >= 2 → SdblMultiString
```

The 2+ consecutive `String` tokens form is an IDE-recovery allowance
for multi-line query strings observed in production BSL code; the
ITS spec does not mandate the form. The single-token case must keep
`String` as a *direct token child* of `SdblLiteral` because
`crates/sdbl-hir/src/lower/expr/mod.rs:173-177` scans
`children_with_tokens()` for the `String` token to detect embedded
newlines.

### Parameters

```text
parameterExpression
  := Ampersand Ident
     ↦ SdblParameter{ Ampersand, Ident }
```

**No trivia between `&` and `Ident`.** The parser does not call
`p.skip_trivia()` between the `Ampersand` bump and the `Ident`
bump. Whether the lexer fuses `&\nT` into one or two tokens is a
lexer-level decision; the parser-side constraint is "no
`p.skip_trivia()` call between the two bumps". Both this expression-
context production site (Slice 10a) and the FROM-context
production site in `select::table_ref` (Slice 8) emit
`SdblParameter` with the same shape — the node is shared between
the two parser surfaces.

### Star

The bare `Star` token (`*`) is accepted as an expression atom only
because of the special `COUNT(*)` syntax. The `Star` is consumed by
`primary_expr` and emitted as an `SdblLiteral`. Inside a SELECT-
field context the asterisk is handled separately by
`select::asterisk_field` (Slice 7 territory) — `is_expression_start`
returns true for `Star` so that `COUNT(*)` parses as a function
call with a literal-`*` argument rather than rejecting at the
`is_expression_start` predicate.

### Parens, tuples, subqueries

```text
parenOrSubqueryExpression
  := '(' (subqueryHead | expressionTail) ')'
subqueryHead    :=  ( SELECT | ВЫБРАТЬ ) ...     (delegate to select::subquery, emit SdblSubqueryExpr)
expressionTail
  :=  expression                                   (single → SdblParenExpr)
   |  expression (',' expression)+                 (multi  → SdblTupleExpr)
```

**SELECT-only lookahead.** The subquery branch is entered if and
only if the post-`(` token is `SELECT` or `ВЫБРАТЬ`. Every other
input — including `(&T)`, `(1)`, `(1, 2)`, `(T + 1)`, `(* 2)` —
routes to the expression branch.

**Note:** this is the *opposite* routing decision from the
FROM-context `data_source` (Slice 8), where any `(` routes to
subquery-source. The two parsers make opposite decisions for
legitimate reasons — in FROM context `(...)` cannot be a tuple or
parenthesised expression because tuples / parenthesised expressions
do not appear at FROM-source position; in expression context
subqueries are only valid inside `IN (...)` and similar predicate
positions, so the SELECT keyword must appear explicitly.

The empty tuple `()` and the trailing-comma case `(1, 2,)` are
handled by the empty-element recovery loop — see §Recovery
contract.

## Predicates

The Slice 10b predicate body slot sits inside the
`comparisonExpression` precedence level (between NOT and the
arithmetic chain). The slot is shared between **comparison
operators** and **predicate forms**: a single `predicateExpression`
function reads one `additiveExpression` operand, then dispatches on
the next keyword/operator to one of seven branches:

```text
predicateExpression
  := additiveExpression
       ( (KwNot)? KwIn (HIERARCHY|ИЕРАРХИИ)? '(' inListBody ')'   ← IN / NOT IN / IN HIERARCHY
       | (IS|ЕСТЬ) (KwNot)? "NULL"                                ← IS [NOT] NULL
       | (KwNot)? (BETWEEN|МЕЖДУ) additiveExpression
                              KwAnd additiveExpression?           ← BETWEEN ... AND ...
       | (KwNot)? (LIKE|ПОДОБНО) additiveExpression
                              ((ESCAPE|СПЕЦСИМВОЛ) additiveExpression)?  ← LIKE [ESCAPE]
       | (REFS|ССЫЛКА) Ident ('.' Ident)*                         ← REFS Mdo.Path
       | ('=' | '<>' | '<' | '<=' | '>' | '>=') additiveExpression  ← comparison
       | ε                                                        ← fall-through (no wrapper)
       )
```

The leading `(KwNot)?` is consumed **before** probing for the
predicate keyword so that `NOT IN (...)`, `NOT BETWEEN ... AND ...`,
and `NOT LIKE ...` all route into the matching predicate branch with
the `KwNot` token captured as a direct child of the eventual
predicate node. If the post-`NOT` lookahead does not match any
predicate / comparison keyword, the marker is `m.abandon`-ed and the
consumed `NOT` token remains as a stray token in the syntax tree —
this is a known IDE-recovery boundary documented in
§IDE-recovery allowances.

### `SdblInExpr` (IN, NOT IN)

Accepted shapes: `expr IN ( v1, v2, ... )` and
`expr IN ( SELECT ... )`. Direct children of `SdblInExpr`:

- the `additiveExpression` operand (the value being checked);
- optional `KwNot` token (when the predicate is `NOT IN`);
- `KwIn` token;
- `LParen` token;
- either a `SdblSubquery` child (when the post-`(` lookahead
  detects `SELECT/ВЫБРАТЬ`) or a sequence of expression children
  separated by `Comma` tokens (the value list, with `Error`
  placeholders for empty / missing / trailing-comma elements);
- `RParen` token.

The empty list form `IN ()` is accepted as a recoverable parse —
see §IDE-recovery allowances.

### `SdblInHierarchyExpr` (IN HIERARCHY, В ИЕРАРХИИ)

Accepted shape: `expr IN HIERARCHY ( root )`. Direct children:

- the `additiveExpression` operand;
- optional `KwNot` token;
- `KwIn` token;
- `Ident` token (`HIERARCHY` or `ИЕРАРХИИ`);
- `LParen` token;
- single expression child (the hierarchy root);
- `RParen` token.

The `HIERARCHY` / `ИЕРАРХИИ` keyword is parsed as an `Ident`-shaped
suffix to `IN`, not as a single `IN HIERARCHY` keyword pair (the
parser bumps `KwIn`, then probes `at_keyword("HIERARCHY")` /
`at_keyword("ИЕРАРХИИ")`). This is load-bearing for IDE recovery on
mid-typed `IN HIE` — the HIERARCHY arm fails, falls through to the
regular IN-list arm, which expects `LParen` and recovers.

### `SdblIsNullExpr` (IS NULL, ЕСТЬ NULL)

Accepted shape: `expr IS [NOT] NULL`. Direct children:

- the `additiveExpression` operand;
- `Ident` token (`IS` or `ЕСТЬ`);
- optional `KwNot` token (when the predicate is `IS NOT NULL`);
- `Ident` token (`NULL`).

The literal `NULL` Ident is required; if it is absent, the marker
is abandoned and the consumed `IS` (and optional `NOT`) tokens
remain as stray tokens in the syntax tree. This is a known
IDE-recovery boundary — see §IDE-recovery allowances.

### `SdblBetweenExpr` (BETWEEN, МЕЖДУ)

Accepted shape: `expr BETWEEN low AND high`. Direct children:

- the `additiveExpression` operand (the test value);
- optional `KwNot` token;
- `Ident` token (`BETWEEN` or `МЕЖДУ`);
- `additiveExpression` (low bound);
- `KwAnd` token;
- `additiveExpression` (high bound).

The `KwAnd` is required by ITS pubqlang/22 §МЕЖДУ, but the parser
emits `SdblBetweenExpr` even if the `AND` is missing — the
high-bound `additiveExpression` is omitted in that case. This is
IDE-recovery for mid-typing `BETWEEN 1` and is documented in
§IDE-recovery allowances.

### `SdblLikeExpr` (LIKE, ПОДОБНО)

Accepted shape: `expr LIKE pattern [ESCAPE char]`. Direct children:

- the `additiveExpression` operand (subject);
- optional `KwNot` token;
- `Ident` token (`LIKE` or `ПОДОБНО`);
- `additiveExpression` (pattern);
- optional `Ident` token (`ESCAPE` or `СПЕЦСИМВОЛ`);
- optional `additiveExpression` (escape character).

`ESCAPE` / `СПЕЦСИМВОЛ` is **not documented in the dumped ITS
chapters**: the LIKE clause itself is ITS-spec'd at pubqlang/23
(pattern-matching primitive) and pubqlang/60 (concrete usage), but
neither chapter mentions an ESCAPE clause. The clause is preserved
as a local IDE-recovery allowance — see §IDE-recovery allowances.
The single-shot shape (at most one ESCAPE per LIKE) is the
parser-side contract; it is not contradicted by ITS, but neither
is it confirmed.

### `SdblRefsExpr` (REFS, ССЫЛКА)

Accepted shape: `expr REFS Mdo.Type [.Subtype]*`. Direct children:

- the `additiveExpression` operand (the value being checked);
- `Ident` token (`REFS` or `ССЫЛКА`);
- a chain of `Ident` and `Dot` tokens representing the MDO
  reference (greedy — eats all subsequent `Dot Ident` pairs).

The MDO chain is greedy; the parser does not enforce a fixed
two-segment shape. Trailing dot without an Ident is detected by
`crates/ide-diagnostics/src/handlers/query_parse_error.rs:78` as
a parse error — the clean-room rewrite preserves the token-level
shape of the chain.

## Comparison

The comparison branch of `predicateExpression` accepts six binary
operators sharing a single `SdblComparisonExpr` wrapper:

```text
comparisonTail
  :=  ('=' | '<>' | '<' | '<=' | '>' | '>=') additiveExpression
```

Direct children of `SdblComparisonExpr`:

- left `additiveExpression`;
- comparison operator token (one of `Eq`, `Neq`, `Lt`, `Le`, `Gt`,
  `Ge`);
- right `additiveExpression`.

The comparison tail is single-shot, not a loop — `a = b = c`
consumes one tail and leaves `= c` as trailing tokens. This matches
the behaviour of every SDBL comparison example documented in the
ITS pubqlang dump (no chained-comparison form is shown).

The dispatcher function `comparisonExpression` is a 1:1 delegating
shim to `predicateExpression`: both predicates and comparison share
the same precedence slot below NOT, so a single dispatcher reads
one operand and probes for either a predicate keyword or a
comparison operator. The shim preserves the dispatcher abstraction
and clarifies the precedence story.

## Column references and function calls

The `columnOrFunctionCall` dispatcher is reached from
`primaryExpression` whenever the leading token is a non-clause
`Ident` (and the `at_keyword` probes for `CASE` / `ВЫБОР` / `NULL`
have already failed). It performs a single Ident bump, then
dispatches on the next-token lookahead:

```text
columnOrFunctionCall
  := Ident
       ( '.' columnChainTail                    ← SdblColumnRef
       | '(' funcCallTail                       ← SdblFunctionCall
       |  ε                                     ← bare SdblColumnRef
       )
```

The three branches are mutually exclusive — a single Ident either
chains via `Dot`, calls via `LParen`, or stands alone as a bare
column reference. CAST detection (`is_cast_function`) runs **before**
the Ident bump so the resulting `is_cast` flag is available inside
the LParen branch for the `КАК`-type recovery (see §CAST type
specification).

### `SdblColumnRef`

Accepted shape: `Ident ('.' Ident)*`, optionally terminated by an
inline tabular field list `'.' '(' selectedFields ')'`. Direct
children:

- a sequence of `Ident` tokens with `Dot` tokens between them;
- `Error` nodes at incomplete-chain positions (trailing dot, dot
  before clause keyword);
- optional `SdblInlineTableFields` (when the chain terminates in
  `.(Field1, Field2, ...)` — see §Inline tabular field syntax).

The chain is a flat token sequence — no nested wrappers. Consumer:
`crates/sdbl-hir/src/lower/expr/mod.rs:lower_column_ref`.

The dot-chain loop terminates when:

- the next token is not `Dot`;
- the post-`Dot` token is a clause keyword (recovered via
  `super::select::is_clause_keyword` so `SELECT t.FROM ...` does
  not consume `FROM` as a column suffix);
- the post-`Dot` token is `LParen` (which routes to inline tabular
  fields).

### `SdblFunctionCall`

Accepted shape: `Ident '(' [DISTINCT|РАЗЛИЧНЫЕ]? argList ')' (member chain)?`.
Direct children:

- the function-name `Ident` token;
- `LParen` token;
- optional `DISTINCT` / `РАЗЛИЧНЫЕ` `Ident` token (aggregate-
  function prefix);
- a sequence of expression children separated by `Comma` tokens —
  with `Error` nodes for empty / missing / trailing-comma elements;
- `RParen` token;
- optional post-RParen `Dot` / `Ident` chain (member access on the
  function-call result, e.g. `ВЫРАЗИТЬ(... КАК ...).Поле`).

Argument list error recovery emits `NodeKind::Error` for three
positions: first-arg empty (`func(, x)`), middle-element empty
(`func(x, , y)`), trailing comma (`func(x,)`). The argument-start
probe and the remaining-arg-after-comma probe both filter out
clause keywords via `super::select::is_clause_keyword` so that
`func(x, FROM T)` does not hijack `FROM` as an Ident-shaped
argument — the regression gate is the C2 fix landed in Slice 10b
(see §IDE-recovery allowances and the Slice 10b attestation
§Behaviour change).

For aggregate functions a `DISTINCT` / `РАЗЛИЧНЫЕ` Ident-keyword
prefix is consumed before the first argument (e.g.
`КОЛИЧЕСТВО(РАЗЛИЧНЫЕ Поле)`). The DISTINCT prefix is consumed as
a child Ident token of `SdblFunctionCall`; the parser does not
enforce that the function name is one of the documented aggregate
names — it accepts the prefix in any function-call position and
relies on later type-checking to reject misuse.

Member access on the function-call result is a `while p.at(Dot)`
loop that consumes Dot/Ident pairs (with `Error` nodes for
incomplete tails) and terminates on clause keywords. The resulting
chain is a direct token-level child sequence of `SdblFunctionCall`;
HIR ignores this chain in lowering, but
`crates/ide-diagnostics/src/handlers/query_parse_error.rs:52` reads
the trailing chain to detect dot-without-Ident parse errors.

### Inline tabular field syntax

The construction `Table.TabularPart.(Field1, Field2, …)` reads as a
column-chain that terminates in an inline tabular field list. The
post-`Dot` `LParen` lookahead routes the column-chain tail loop
into `inlineTableFields`:

```text
inlineTableFields
  := '(' selectedFields ')'                    ← SdblInlineTableFields
                                                  wraps SdblSelectedField+
```

Direct children of `SdblInlineTableFields`:

- `LParen` token;
- the result of `selected_fields` (Slice 7) — multiple
  `SdblSelectedField` direct children;
- `RParen` token.

This is the only Slice-10b → Slice-7 dispatch boundary: from
`column_or_function` → `inline_table_fields` → `selected_fields`.
The Slice 7 entry is reused as-is; Slice 10b does not extend it.
HIR's consumer reaches `SdblInlineTableFields` indirectly via
walking `SdblColumnRef` descendants in
`crates/sdbl-hir/src/lower/expr/mod.rs`.

## CAST type specification

The `ВЫРАЗИТЬ` (CAST) function takes one argument expression
followed by the keyword `КАК` (`AS`) and a type specification. The
type specification is parsed by `parseCastType`:

```text
castType
  := Ident                                                   ← primitive type or first MDO segment
       ( '.' Ident ('.' Ident)*                              ← MDO chain (СПР.Х, ДОК.Х, etc.)
       | '(' Decimal (',' Decimal)? ')'                      ← primitive parameter list (СТРОКА(200), ЧИСЛО(8, 2))
       | ε
       )
```

Direct children of `SdblType`:

- `Ident` token (the primitive type name OR the first part of an
  MDO chain);
- then either:
  - optional `Dot` / `Ident` pairs (MDO chain), OR
  - `LParen` + `Decimal` token + optional `Comma` + `Decimal` +
    `RParen` (primitive type parameter list).

Recognised primitive type names: `СТРОКА` / `STRING`, `ЧИСЛО` /
`NUMBER`, `ДАТА` / `DATE`, `БУЛЕВО` / `BOOLEAN`. The parser does not
enforce the primitive-vs-MDO distinction at parse time — it reads
the first Ident, then enters a `while p.at(Dot)` loop for MDO
continuation, then checks for `LParen` (primitive parameter list).
For well-formed input only one continuation branch fires; for mixed
input the parser consumes the MDO chain *and* the parameter list
(this is preserved as IDE-recovery behaviour).

The `is_cast_function` predicate
(`p.at_keyword("CAST") || p.at_keyword("ВЫРАЗИТЬ")`) is called from
`columnOrFunctionCall` **before** the Ident bump, so the resulting
`is_cast` boolean is available inside the LParen branch to enable
the `КАК`-type recovery: after consuming the first argument
expression and the `КАК` / `AS` keyword, `parseCastType` is invoked
to parse the type spec and emit `SdblType`.

After the closing `RParen` of a CAST function call, member access
on the result (`ВЫРАЗИТЬ(... КАК Документ.X).Реквизит`) is consumed
by the same post-RParen Dot/Ident chain loop as for any other
function call — see §SdblFunctionCall.

Type-level CAST checking (matching the parsed `SdblType` against
the `bsl-metadata` MDO catalog) is **out of scope** for Slice 10b
and is deferred to Slice 13.

## CASE expressions

The `ВЫБОР` (CASE) expression has two forms — simple (with operand)
and searched (no operand). Both forms terminate with the mandatory
`КОНЕЦ` (END) keyword. The `ИНАЧЕ` (ELSE) clause is optional.

```text
caseExpression
  := (CASE|ВЫБОР)
       expression?                              ← optional operand (simple form)
       whenClause+                              ← 1+ WHEN clauses
       ((ELSE|ИНАЧЕ) expression)?               ← optional ELSE
       (END|КОНЕЦ)

whenClause
  := (WHEN|КОГДА) expression
     (THEN|ТОГДА) expression
```

Direct children of `SdblCaseExpr`:

- `Ident` token (`CASE` or `ВЫБОР`);
- **optional operand expression node** — when present (simple
  form), appears as the first non-token child node *before* any
  `SdblWhenClause`;
- 1+ `SdblWhenClause` children;
- optional `Ident` token (`ELSE` or `ИНАЧЕ`) followed by an optional
  ELSE expression child node;
- `Ident` token (`END` or `КОНЕЦ`).

**Child-order invariant.** HIR consumer
`crates/sdbl-hir/src/lower/expr/case_expr.rs:40-45` distinguishes
the two forms by inspecting the **first child node**:

```rust
let has_operand = !when_clauses_nodes.is_empty()
    && node.children().next()
        .map(|n| n.kind() != SyntaxKind::SDBL_WHEN_CLAUSE)
        .unwrap_or(false);
```

The parser must therefore emit children in source order:

- **Simple CASE:** operand expression as the first child node,
  *then* 1+ `SdblWhenClause` children, *then* optional ELSE
  expression child node, *then* the END token.
- **Searched CASE:** 1+ `SdblWhenClause` as the first child nodes,
  *then* optional ELSE expression, *then* END.

Direct children of `SdblWhenClause`:

- `Ident` token (`WHEN` or `КОГДА`);
- expression child (the condition);
- `Ident` token (`THEN` or `ТОГДА`);
- expression child (the result).

The HIR `case_expr.rs:51-89` reads `node.children()` and assumes
exactly two child expression nodes per `SdblWhenClause`
(condition + result). The clean-room rewrite must preserve this
two-child shape.

## Trivia handling convention

Every operator-level loop calls `p.skip_trivia()` **before** probing
the operator token. The pattern is:

```rust
loop {
    p.skip_trivia();           // CRITICAL — must precede the probe
    if !p.at(operator_token) {
        break;
    }
    p.bump();                  // operator
    p.skip_trivia();
    parse_next_operand(p);
}
```

This invariant is load-bearing: `a\n+\nb` and `a /* comment */ + b`
must parse as `SdblAdditiveExpr( a, +, b )`, with the trailing
trivia preserved as trivia tokens *inside* the wrapper node. A
syntactically cleaner pattern (e.g. `while p.eat_with_trivia(...)`)
is unavailable because the parser's `Parser` API does not expose
such a combinator and adding one is out of scope for Slice 10a.

## Recovery contract

The expression parser exposes a small predicate / helper family that
the surrounding `select` grammar reuses. Slice 10a clean-rooms all
four:

### `is_expression_start`

```text
is_expression_start(p) := true  iff  next-token can lead an expression
                          false otherwise (clause keyword, EOF, etc.)
```

Accept set: `Decimal`, `Float`, `String`, `KwTrue`, `KwFalse`,
`KwUndefined`, non-keyword `Ident`, `Plus`, `Minus`, `KwNot`,
`Star`, `LParen`, `Ampersand`, `at_keyword("CASE")`,
`at_keyword("ВЫБОР")`, `at_keyword("NULL")`. Reject set: every other
`TokenKind`, including all clause keywords gated by
`select::is_clause_keyword`.

The historical `KwNull` `TokenKind` arm in `is_expression_start`
was **dead code** (the converter at `sdbl_token_converter.rs` maps
`LitNull → Ident`, not `KwNull`). Slice 10a C2 dropped that arm.
The live recognition paths for bare `NULL` are split across two
sites:

- `is_expression_start` accepts `NULL` through the generic
  non-clause `Some(TokenKind::Ident) => !is_clause_keyword(p)`
  arm. The `_ => p.at_keyword("NULL") | …` fallback in the same
  function is **unreachable** under the current `Parser::at_keyword`
  API, which only returns true when the current token kind is
  `TokenKind::Ident` — and the `Ident` arm has already matched
  before the `_` fallback runs. The fallback is kept as textual
  symmetry with `primary_expr`'s keyword-probe pattern; it does
  not provide a defence against hypothetical converter changes
  that route `NULL` to a non-Ident `TokenKind`. If such a
  converter change is ever made, both `is_expression_start` and
  `primary_expr` need to grow an explicit
  `Some(TokenKind::SomethingElse)` arm, plus a regression test
  for that shape.
- `primary_expr` performs the **decisive** dispatch: the
  `p.at_keyword("NULL")` probe runs **before** the generic
  `Some(TokenKind::Ident) => column_or_function(p)` match arm,
  so a bare `NULL` at the head of a primary position emits
  `SdblLiteral` rather than being routed into
  `column_or_function`.

The two-site split is by design: `is_expression_start` is a
permissive "could this start an expression?" predicate;
`primary_expr` is the canonical NodeKind-emitting dispatcher.

### `is_recovery_point`

```text
is_recovery_point(p, recovery_set) := true iff
    p.current() ∈ recovery_set  ∨
    select::is_clause_keyword(p) ∨
    p.at_end()
```

Used inside `parse_delimited_list` to detect "stop parsing this
list" positions.

### `recover_to_delimiter`

Consumes tokens from the input cursor up to and not including a
top-level delimiter (`Comma`, `RParen`, `Semicolon`) or a clause
keyword. Tracks `paren_depth` so that nested delimiters
(`ВЫРАЗИТЬ(поле КАК СТРОКА(200))`) are not consumed early — the
recovery hops over nested `(...)` blocks. Wraps the consumed run in
one `NodeKind::Error` marker.

### `parse_delimited_list`

Generic comma-delimited list parser used for SELECT field lists,
FROM source lists, IN value lists, GROUP BY items, INDEX BY items,
and VT method-call arguments. Signature:

```rust
pub(super) fn parse_delimited_list<F>(
    p: &mut Parser,
    delimiter: TokenKind,
    recovery_set: &TokenSet,
    is_item_start: fn(&Parser) -> bool,
    parse_item: F,
)
```

Behaviour contract:

1. Always parse one item first (caller's contract: at least one
   item is expected at the call site).
2. Loop: skip trivia → check `is_recovery_point` → break if true →
   eat delimiter → check `is_recovery_point` again or
   `is_item_start` is false → emit empty `Error` and break /
   continue → otherwise parse the next item.
3. Empty middle elements (`a, , b`) emit an `Error` placeholder so
   the IDE can keep parsing the rest of the input.
4. Trailing delimiters (`a, b,`) emit an `Error` and exit the loop.

## AST-shape contracts

These are the post-Slice-10a AST shape contracts that downstream
consumers depend on. The parser must preserve each one bit-for-bit.

### Operator wrappers are FLAT

For chained operators at one precedence level, the parser opens
*one* marker before the loop and emits *one* `SdblXxxExpr` wrapper
covering all operands and operator tokens.

For `a + b + c` (additive level):

```text
SdblAdditiveExpr
├── (operand a — emitted by multiplicative_expr)
├── '+'
├── (operand b)
├── '+'
└── (operand c)
```

— **not** a nested left-associative tree
`SdblAdditiveExpr(SdblAdditiveExpr(a,+,b),+,c)`.

The same flat shape applies to `SdblMultiplicativeExpr`,
`SdblLogicalOrExpr`, `SdblLogicalAndExpr`. The HIR consumer at
`crates/sdbl-hir/src/lower/expr/ops.rs:42-92` collects ALL direct
children into a `Vec` and detects the operator from
`node.text().contains(...)` — so the rewrite must keep operator
tokens verbatim inside the wrapper's text range.

### Empty wrapper unwrapping

Walking a single non-operator atom through the chain
(`Таблица.Поле`, `42`, `&T`) produces a tower of single-child
wrapper nodes:

```text
SdblLogicalOrExpr
└── SdblLogicalAndExpr
    └── (whatever predicate / additive / mul / unary / primary chain
         produces, terminating in the atom)
```

HIR's `lower_binary_expr` at `ops.rs:45-55` unwraps each single-
child wrapper. The clean-room rewrite must keep the unconditional
wrapper opening — every operator-chain function opens one marker
unconditionally, regardless of whether an operator is present in
the source.

### NOT and unary nesting

`NOT NOT a` and `- -a` produce nested wrappers via right-recursion:

```text
SdblNotExpr( NOT, SdblNotExpr( NOT, a ) )
SdblUnaryExpr( -, SdblUnaryExpr( -, a ) )
```

Each NOT / unary token is the first direct child of its wrapper,
followed by exactly one operand expression as a direct child node.

### Tuple vs paren

```text
'(' expr ')'                      → SdblParenExpr  (1 expression child)
'(' expr ',' expr (',' expr)* ')' → SdblTupleExpr  (≥2 expression children)
'(' SELECT ... ')'                → SdblSubqueryExpr
```

### Parameter direct-child shape

`SdblParameter` emitted at *any* call site (Slice 10a
`parameter_expr` for expression-context, Slice 8 `table_ref` for
parameter-source FROM) has the following shape:

- **Complete `&Ident`:** two direct children — the `Ampersand`
  token and the following `Ident` token, with no trivia node
  between them.
- **Incomplete bare `&` at EOF or before a clause keyword:** one
  direct child — the `Ampersand` token alone. The identifier bump
  is guarded by `if p.at(TokenKind::Ident)` (not required by
  `p.expect`), so the parameter marker still completes when the
  user is mid-typing. Slice 8 attestation §Preserved-behaviour #7
  locks this for FROM-context (`ВЫБРАТЬ * ИЗ &`); Slice 10a
  preserves the same recovery shape for expression-context
  (`ВЫБРАТЬ &` and similar). HIR `lower_parameter` at
  `crates/sdbl-hir/src/lower/expr/mod.rs:453+` reads
  `node.text()` to derive the parameter name and tolerates the
  bare `&` shape.

### VT-arg children direct under SdblTableRef

The HIR consumer at `crates/sdbl-hir/src/lower/from_clause.rs:283-306`
filters direct children of `SdblTableRef` for VT-arg lowering. The
*full* filter set is:

- `SDBL_LOGICAL_OR_EXPR`
- `SDBL_LOGICAL_AND_EXPR`
- `SDBL_COMPARISON_EXPR`
- `SDBL_ADDITIVE_EXPR`
- `SDBL_MULTIPLICATIVE_EXPR`
- `SDBL_UNARY_EXPR`
- `SDBL_COLUMN_REF`
- `SDBL_LITERAL`
- `SDBL_FUNCTION_CALL`
- `SDBL_PARAMETER`
- `SDBL_PAREN_EXPR`
- `SDBL_TUPLE_EXPR`
- `SDBL_IN_EXPR`
- `SDBL_MISSING_ARG`
- `ERROR`

Slice 10a owns the clean-room rewrite of the producers for the
operator wrappers + `SDBL_PARAMETER` + `SDBL_LITERAL` +
`SDBL_PAREN_EXPR` + `SDBL_TUPLE_EXPR` subset (8 of the 15 kinds).
Slice 10b owns `SDBL_COMPARISON_EXPR`, `SDBL_COLUMN_REF`,
`SDBL_FUNCTION_CALL`, `SDBL_IN_EXPR`. `SDBL_MISSING_ARG` and `ERROR`
are emitted by the existing VT-arg recovery path (Slice 8 LEGACY
`virtual_table_args_legacy`, deferred to Slice 5).

For the Slice 10a / 10b clean-room rewrites, the contract is: each
re-authored producer must continue to emit its respective NodeKind
as a *direct* child of `SdblTableRef` (no new intermediate wrappers,
no NodeKind rename). For the deferred kinds (`SDBL_MISSING_ARG`,
`ERROR`), the C1 extraction-preservation rule already locks them
bit-for-bit until Slice 5 reauthors them. For the Slice 10b kinds,
the Slice 10a rewrite *must not* drop them from the VT-arg position
— `column_or_function`, `predicate_expr`, etc. continue to be
reachable via `primary_expr` dispatch and continue to land as
direct `SdblTableRef` children when invoked from the VT-arg call
site.

## IDE-recovery allowances (vs ITS)

Behaviours preserved bit-for-bit for IDE-recovery / consumer-
compatibility reasons that are **not** ITS-mandated:

1. **`expression` and `logical_expression` are two distinct `pub fn`
   entries with equivalent bodies.** Slice 12 may merge.

2. **Multi-string concatenation** for 2+ consecutive `String`
   tokens. Production BSL code uses this form for multi-line
   query string literals.

3. **Bare `*` accepted as expression start** for `COUNT(*)` syntax.

4. **Right-recursive multi-`NOT` and multi-unary** (`NOT NOT a`,
   `- -a`). The parser does not collapse repeated unary operators
   into a single wrapper.

5. **Empty middle elements in delimited lists** emit `Error`
   placeholders rather than abort. Used by SELECT field list,
   FROM source list, IN value list, INDEX BY items.

6. **`recover_to_delimiter` paren-depth tracking** so
   `ВЫРАЗИТЬ(поле КАК СТРОКА(200))` recovery does not consume the
   inner `)` as the recovery target.

7. **Trivia preserved inside operator wrappers.** The wrapper's
   `node.text()` carries trivia tokens between operands and
   operators; HIR depends on this for text-based operator
   detection.

8. **Newline-separated logical operators** parse as the same
   `SdblLogicalAndExpr` / `SdblLogicalOrExpr` wrapper. HIR's
   text-based operator detection at `ops.rs:64-67` looks for
   `" И "` / `" AND "` substrings (with surrounding spaces); when
   newlines replace spaces the substring may not be found and HIR
   falls back to the default operator arm. This is a known
   HIR-side bug to be addressed in Slice 13; the parser-side
   wrapper shape is preserved.

9. **Modulo `%` operator accepted in `multiplicative_expr`.** ITS
   pubqlang/40 explicitly states «Операция получения остатка % в
   языке запросов не поддерживается» — `%` is **not** an
   ITS-supported SDBL operator. The pre-clean-room parser nonetheless
   accepted `TokenKind::Percent` in the multiplicative chain, and
   the Slice 10a C2 rewrite preserves that acceptance as a *local
   IDE-recovery allowance*: a query containing `a % b` produces a
   recoverable parse tree (`SdblMultiplicativeExpr` containing the
   `%` token between two operands) rather than an immediate parse
   error, so the IDE can report the misuse via diagnostics rather
   than aborting the whole query. This is the only ITS-mandated
   negative claim that the parser deliberately violates; all other
   accepted operators / atoms / forms are ITS-supported. The
   §ITS coverage verification table row "Modulo `%` operator"
   records the discrepancy verbatim.

10. **Empty `IN ()` value list accepted as a recoverable parse**
    (Slice 10b). ITS pubqlang/22 documents `IN` with a non-empty
    value list. The parser accepts the empty form as IDE-recovery
    for mid-typing — `IN (...)` with no completed values yet emits
    a recoverable `SdblInExpr` with an empty value-list body so
    diagnostics can flag the misuse without aborting the query.

11. **`IS [NOT] NULL` falls through `m.abandon` if `NULL` is
    missing** (Slice 10b). When the post-`IS [NOT]` lookahead does
    not find `at_keyword("NULL")`, the marker is abandoned and the
    consumed `IS` and optional `NOT` tokens remain as stray tokens
    in the syntax tree. This is a known IDE-recovery boundary; a
    candidate Slice 12 fix would move the consumption inside the
    NULL-confirmed branch.

12. **`BETWEEN low [AND high]` accepts missing AND** (Slice 10b).
    The `AND` is required by ITS pubqlang/22 §МЕЖДУ, but the
    parser emits `SdblBetweenExpr` even when the high-bound `AND`
    is missing — recovery for mid-typing `BETWEEN 1`. The
    high-bound `additiveExpression` is then omitted from the
    direct-child list.

13. **`LIKE pattern [ESCAPE char]` ESCAPE clause is local-spec'd**
    (Slice 10b). ITS pubqlang/23 §Шаблон documents the LIKE
    pattern-matching primitive but does not document an ESCAPE /
    СПЕЦСИМВОЛ clause. The parser accepts the optional ESCAPE
    clause as a local IDE-recovery allowance; the per-function
    provenance comment in `predicate_expr` carries
    `// local: ...` for the ESCAPE branch.

14. **Orphaned-`NOT`-token boundary in `predicate_expr`** (Slice
    10b). When `NOT` is consumed before predicate / comparison
    probing and no branch matches, the marker is abandoned and the
    consumed `NOT` token remains as a stray token in the syntax
    tree. Documented as preserved-behaviour #2 in the Slice 10b
    attestation; candidate Slice 12 fix is to move the NOT
    consumption inside each predicate branch's prefix.

15. **`func(x, FROM ...)` clause-keyword recovery in
    `column_or_function`** (Slice 10b C2 fix). The pre-Slice-10b
    parser used `is_expression_start && !p.at(Comma)` at the
    function-call argument-start probe with no
    `is_clause_keyword` check, so `func(x, FROM T)` hijacked
    `FROM` as an Ident-shaped argument. Slice 10b C2 added
    `&& !super::select::is_clause_keyword(p)` to both the
    first-argument and the after-comma argument-start probes; the
    fix is documented under the Slice 10b attestation §Behaviour
    change.

## ITS coverage verification

Filled at Slice 10a C2 against the local ITS pubqlang dump at
`<ITS pubqlang dump>/` (the publicly reachable
ITS URLs at `its.1c.ru/db/pubqlang/...` are paywalled and serve
JS-rendered navigation only; the local dump is the authoritative
copy of the published ITS pages).

Note on chapter coverage: pubqlang/10 and pubqlang/12 are short
intro pages (15 and 71 lines respectively); the actual normative
prose for expression syntax lives in later "example" chapters
(/22 WHERE, /40 expressions in field list, /60 parameter passing).
The verification below cites the chapters that materially document
each claim.

| Claim | Verified | Citation / quoted excerpt |
|---|---|---|
| Bilingual keyword principle | YES — pubqlang/12 | "все ключевые слова имеют два варианта написания: на русском и английском языках" |
| Logical operator inventory `И` / `ИЛИ` / `НЕ` (Russian) | YES — pubqlang/22 | "простые логические выражения соединяются между собой логическими операторами И, ИЛИ, НЕ" |
| Logical operator bilingual EN equivalents `AND` / `OR` / `NOT` | YES — pubqlang/12 by general bilingual rule + project lexer Slice 2 attestation §Scope which enumerates the EN spellings | bilingual rule applies; specific EN keywords are not literally quoted in the dumped pages but follow the universal RU/EN principle |
| Operator precedence ladder NOT > AND > OR | YES — pubqlang/22 | **"В условиях сначала вычисляются простые логические выражения, затем операции НЕ, затем операции И, в последнюю очередь – операции ИЛИ. Для того чтобы обеспечить другой порядок вычислений, можно использовать круглые скобки."** §Operator precedence section above credits pubqlang/22 with this verbatim quote. |
| Arithmetic operators `+` `-` `*` `/` | YES — pubqlang/40 | "Арифметические операции (+, -, /, *)" |
| Modulo `%` operator | NO — pubqlang/40 | **"Операция получения остатка % в языке запросов не поддерживается."** The current parser at `multiplicative_expr` accepts `TokenKind::Percent`; this is preserved as an IDE-recovery / local allowance NOT ITS-spec'd. The clean-room rewrite continues to accept `Percent` in `multiplicative_expr` to avoid regressing existing tests, but `%` is documented as a local extension under §IDE-recovery allowances. |
| String concatenation via `+` | YES — pubqlang/40 | "Операцию конкатенации строк (+). Операцию конкатенации нельзя использовать для виртуальных полей." |
| Literal types: numeric, string, boolean (Истина / Ложь), NULL, Неопределено (UNDEFINED) | YES — pubqlang/40 | "Литералы типов: число, строка (в кавычках), булево (значения Истина и Ложь), Null, Неопределено" |
| `&Identifier` parameter prefix syntax | YES — pubqlang/60 | concrete `&ЧастьНаименования`, `&ДатаНачала`, `&ДатаОкончания` parameter examples in queries; the `&` prefix is the documented form |
| `ВЫБОР` / CASE expression | YES — pubqlang/40 | "Операцию выбора ВЫБОР – позволяет получить одно из возможных значений в соответствии с указанными условиями" |
| `ВЫРАЗИТЬ` / CAST expression | YES — pubqlang/40 | "Операцию приведения типов ВЫРАЗИТЬ" |
| `ССЫЛКА` / REFS predicate | YES — pubqlang/40 | "при помощи оператора ССЫЛКА проверяется, ссылкой на какой документ является поле" |
| `МЕЖДУ` / BETWEEN predicate | YES — pubqlang/22 + pubqlang/40 | "оператор МЕЖДУ, который проверяет результат вхождения значения в диапазон" |
| `ПОДОБНО` / LIKE predicate | YES — pubqlang/60 | concrete usage `Наименование ПОДОБНО "%" + &ЧастьНаименования + "%"` |
| Comparison operator inventory `=` `<>` `<` `<=` `>` `>=` | NO — verified absent in dump | Chapter 22 §Условие отбора enumerates the logical operators (И/ИЛИ/НЕ) and the МЕЖДУ operator but does NOT explicitly enumerate the 6 binary comparison operators. The 6-operator inventory is treated as a local convention; per-function provenance comments cite mini-spec §Comparison rather than the chapter. |
| `IN` value-list (parens-bracketed comma list) | NO — verified absent in dump | Chapter 22 contains examples that route through the WHERE-condition shape but does not explicitly enumerate the `<expr> В (<v1>, <v2>, ...)` shape. The IN value-list shape is treated as a local IDE-recovery allowance derived by analogy from BETWEEN/МЕЖДУ. Per-function provenance cites mini-spec §SdblInExpr. |
| `IN` with subquery | NO — not explicitly enumerated | Chapter 22 + chapter 40 do not show an `<expr> В (ВЫБРАТЬ ...)` example. The IN-subquery form is local-spec'd; mini-spec §SdblInExpr describes the shape. |
| `IN HIERARCHY` / `В ИЕРАРХИИ` predicate | YES — pubqlang/32 (`chapter 32`, листинг 1.51) | Canonical example: «Товары.Ссылка В ИЕРАРХИИ (&ГруппаТоваров)» — chapter 32 §Как получить записи иерархической таблицы, находящиеся в иерархии выбранной группы. Note: chapter 28 does NOT contain `В ИЕРАРХИИ` (codex Round-1 spot check); chapter 32 is the primary source. |
| `IS NULL` / `ЕСТЬ NULL` predicate | YES — pubqlang/27 (`chapter 27`) | Canonical example: «КОГДА (Товары.Производитель) ЕСТЬ NULL ТОГДА "NULL"». Note: chapter 22 does NOT contain `ЕСТЬ NULL` (codex Round-1 spot check); chapter 27 is the primary source. Secondary refs are present in chapters 90, 99, 100, 101 but were not spot-verified. |
| `LIKE` / `ПОДОБНО` pattern primitive | YES — pubqlang/23 (`chapter 23`) | Chapter 23 §Как получить записи таблицы, содержащие строки, соответствующие заданному шаблону: explicit description that conditional matching uses оператор ПОДОБНО with example «Наименование ПОДОБНО "%Иван%"» (листинг 1.34). The pubqlang/60 concrete-usage row above remains an independent verification of the same predicate in parameter-passing context. |
| `ESCAPE` / `СПЕЦСИМВОЛ` (LIKE escape clause) | NO — verified absent in dump | The LIKE clause is documented in pubqlang/23 + pubqlang/60 but neither chapter mentions an ESCAPE / СПЕЦСИМВОЛ clause. Recorded as local IDE-recovery allowance under §IDE-recovery allowances #13. |
| `BETWEEN low AND high` (full clause shape) | YES — pubqlang/22 (`chapter 22`, листинг 1.33) | Chapter 22 §Условие отбора shows the canonical `Дата МЕЖДУ ДАТАВРЕМЯ(2012, 10, 01) И ДАТАВРЕМЯ(2012, 10, 31)` with the explicit `AND/И` linking the two bounds. Chapter 40 has BETWEEN examples in expression context. |
| `REFS` / `ССЫЛКА` predicate (full canonical example) | YES — pubqlang/40 (`chapter 40`) | Canonical example: «КОГДА (ОстаткиТоваров.Регистратор ССЫЛКА Документ.ПриходнаяНакладная) ТОГДА ВЫРАЗИТЬ (ОстаткиТоваров.Регистратор КАК Документ.ПриходнаяНакладная).Поставщик». Demonstrates REFS with an MDO chain inside a CASE branch. Secondary refs in chapters 95 + 159 were not spot-verified. |
| `ВЫБОР` / CASE expression body (WHEN / THEN / ELSE / END) | YES — pubqlang/40 (`chapter 40`) | Canonical example: «ВЫБОР КОГДА Товары.ЭтоГруппа = ИСТИНА ТОГДА "Это группа" ИНАЧЕ "Это элемент" КОНЕЦ КАК ПризнакГруппы». Chapter 40 also describes the operation as «ВЫБОР (КОГДА … ТОГДА) ИНАЧЕ … КОНЕЦ» — explicit description matches the parser's two-form (simple vs searched) dispatch. |
| `ВЫРАЗИТЬ` / CAST type specification (primitive parameterised + MDO chain) | YES — pubqlang/40 (`chapter 40`) | Chapter 40 describes ВЫРАЗИТЬ as both «операция приведения типов» (composite-to-component MDO chain) and «функция для получения результатов нужной длины и точности» (primitive-with-parameters). Examples present: «ВЫРАЗИТЬ(СУММА(ЗаказТовара.СуммаЗаказа) / КОЛИЧЕСТВО(*) КАК ЧИСЛО(8,2))» (primitive parameterised form) and «ВЫРАЗИТЬ (ОстаткиТоваров.Регистратор КАК Документ.ПриходнаяНакладная).Поставщик» (MDO + member access). |
| `DISTINCT` / `РАЗЛИЧНЫЕ` aggregate prefix | YES — pubqlang/21 (`chapter 21`, листинг 1.29) | Canonical example: «КОЛИЧЕСТВО(РАЗЛИЧНЫЕ ЗаказТовара.Клиент) КАК РазныеКлиенты». Chapter 20 documents the SELECT-level `ВЫБРАТЬ РАЗЛИЧНЫЕ` form independently; chapter 21 documents the aggregate-function-prefix form which is the form Slice 10b's `column_or_function` accepts inside a function call. |
| Inline tabular field syntax `Table.TabularPart.(Field1, Field2, ...)` | NO — verified absent in dump | The dumped chapters under pubqlang do not document the `Table.TabPart.(F1, F2)` inline tabular field shape. Recorded as a local IDE-recovery allowance; per-function provenance in `inline_table_fields` cites mini-spec §Inline tabular field syntax. |
| Member access on CAST function result `ВЫРАЗИТЬ(... КАК Документ.X).Поле` | YES — pubqlang/40 (`chapter 40`) | Canonical example: «ВЫРАЗИТЬ (ОстаткиТоваров.Регистратор КАК Документ.ПриходнаяНакладная).Поставщик» — chapter 40 demonstrates the full chain (CAST + MDO type + post-`)` `.Поле` member access). |

**Verification summary:** every Slice 10a + Slice 10b expression-
grammar claim that the mini-spec attributes to ITS is verified
against the pubqlang dump, with the precedence ladder upgraded
from "mini-spec-declared" to "ITS pubqlang/22-derived". The single
**discrepancy** is `%` modulo — accepted by the parser, not
ITS-supported. The clean-room rewrite preserves the local `%`
allowance and documents it as a local extension.

The Slice 10b verification rows above were filled in during the
Slice 10b C2 commit after directly inspecting the local dump pages
(chapters 21, 22, 23, 27, 32, 40). The
outcome:

- **verified yes** — IN HIERARCHY (chapter 32, листинг 1.51), IS
  NULL (chapter 27), LIKE pattern primitive (chapter 23, листинг
  1.34), BETWEEN (chapter 22, листинг 1.33), REFS (chapter 40,
  CASE-branch example), CASE/ВЫБОР body (chapter 40), CAST type
  spec (chapter 40, listings for both primitive and MDO forms),
  DISTINCT aggregate prefix (**chapter 21**, листинг 1.29 —
  upgrade from the original plan-time guess of chapter 12 / 40),
  CAST member access (chapter 40);
- **verified no** — Comparison operator inventory (chapter 22
  enumerates only logical operators and МЕЖДУ; the 6-operator
  inventory `=` `<>` `<` `<=` `>` `>=` is treated as local
  convention), IN value-list and IN-with-subquery (chapter 22 +
  chapter 40 do not enumerate these shapes; treated as local
  IDE-recovery allowance), ESCAPE / СПЕЦСИМВОЛ (chapters 23 + 60
  do not document the clause; preserved as local IDE-recovery
  allowance per §IDE-recovery allowances #13), inline tabular
  field syntax (no chapter documents the `Table.TabPart.(F1, F2)`
  shape; preserved as local IDE-recovery allowance).

The discrepancies above are documented in §IDE-recovery
allowances #10–#15 and reflected in the per-function provenance
comments inside `crates/parser/src/grammar/sdbl/expressions.rs`:
ITS-citing rows yield ITS citations (e.g. CASE/ВЫБОР cites
`pubqlang/40 §ВЫБОР`); rows that turned out to be unverified
yield `// local: ...` comments cross-referencing the relevant
mini-spec section.

C2 provenance comments in `expressions.rs` cite the verified ITS
chapters (`pubqlang/21`, `pubqlang/22`, `pubqlang/23`,
`pubqlang/27`, `pubqlang/32`, `pubqlang/40`, plus `pubqlang/60`
for parameter passing) wherever applicable, with explicit
`// local: ...` markers for the four rows that turned out to be
unverified.

## Out of scope

The following are **not** covered by this mini-spec:

- specialised function vocabulary at the lexer / type-checker
  layer — Slice 4 owns the lexer-level vocabulary for aggregate /
  date / string / type-helper functions
  (`СУММА` / `SUM`, `КОЛИЧЕСТВО` / `COUNT`, `СРЕДНЕЕ` / `AVG`,
  `МАКСИМУМ` / `MAX`, `МИНИМУМ` / `MIN`, `ДЕНЬ` / `MONTH` /
  `ГОД`, `ПОДСТРОКА`, etc.);
- type-level CAST checking — Slice 13 owns the matching of
  `parseCastType` output (the parsed `SdblType` node) against the
  `bsl-metadata` MDO catalog; the parser merely accepts the
  syntactic shape without enforcing the MDO existence;
- specialised non-CASE / non-CAST keyword expression forms (e.g.
  `АВТОНОМЕРЗАПИСИ`, `ПУСТАЯТАБЛИЦА`) — these are deferred to
  later parser slices and do not affect Slice 10b.

The Slice 10b extension (added in C0a) covers the predicate /
comparison / column-or-function / CAST / CASE bodies that were
deferred during the original Slice 10a authorship.

## Non-consultation statement

During the authorship of this mini-spec the following sources were
not used as working text:

- the sibling `../bsl-parser` project — neither its grammar files
  nor its parser implementation were consulted;
- any other third-party SDBL parser, grammar, or event-tree
  implementation;
- ANTLR-style precedence rules from third-party SQL grammars (the
  precedence ladder above is declared by this mini-spec, not lifted
  from any external grammar).

The sources actually consulted are listed under §Primary sources
and §Secondary sources at the top of this document. The claim made
here is **independent derivation from those sources plus the
project's local compatibility constraints** (the AST-shape
contracts and IDE-recovery allowances enumerated above) — not
textual novelty, and not a uniqueness claim for the resulting
grammar shape. Other clean-room authors working from the same
sources may reach a different but equivalent grammar shape; this
mini-spec records the specific choices this project made, the
rationale for each, and the consumer-side compatibility contracts
that constrained those choices.

The Slice 10b extension (sections §Predicates, §Comparison,
§Column references and function calls, §CAST type specification,
§CASE expressions — added in C0a of the Slice 10b clean-room
rewrite) was authored under the same constraints: the local ITS
pubqlang dump pages (chapters 22, 23, 27, 32, 40) and the
project's own event-parser conventions established in Slices 1, 2,
6, 7, 8, and 10a were consulted; the sibling `../bsl-parser`
project's grammar text and parser implementation were not; the
pre-C1 function bodies of the eight Slice 10b target functions
were not consulted during the C0a authorship. The new sections
inherit the same independent-derivation claim as the Slice 10a
sections.
