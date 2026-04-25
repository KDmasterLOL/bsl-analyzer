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
is the local dump at `/home/itrous/src/tools_migration/its/dump/`
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

Secondary sources:

- `crates/lexer/src/sdbl/mod.rs` (Slice 1 + 2 clean-room) — for the
  canonical `TokenKind` mapping of operator and keyword lexemes;
- `crates/parser/src/parser.rs` — for the project's own event-parser
  conventions established in Slices 6 / 7 / 8;
- the project's own attestations under `docs/legal/sdbl-clean-room-slice{1,2,6,7,8}.md`
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

It does **not** cover (Slice 10b inherits and extends):

- predicate bodies (IN, IN HIERARCHY, IS NULL, BETWEEN, LIKE, REFS);
- column references and function call argument shape;
- CAST type specification (`ВЫРАЗИТЬ(... КАК ...)`);
- CASE expression body (WHEN clauses, ELSE branch).

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
  non-clause-keyword `Ident` arm (with a defensive
  `at_keyword("NULL")` in the `_` fallback for hypothetical future
  converter changes); `primary_expr` performs the **decisive**
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
  arm (the `_ => p.at_keyword("NULL") | …` fallback is defensive
  only — it is unreachable when the token is `Ident`, but the
  fallback would catch a future converter change that produces
  some other `TokenKind` for `NULL`). The predicate's job is
  just to say "yes, this can start an expression"; the
  Ident-route accept is the right answer.
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

## ITS coverage verification

Filled at Slice 10a C2 against the local ITS pubqlang dump at
`/home/itrous/src/tools_migration/its/dump/` (the publicly reachable
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

**Verification summary:** every Slice 10a + Slice 10b expression-
grammar claim that the mini-spec attributes to ITS is verified
against the pubqlang dump, with the precedence ladder upgraded
from "mini-spec-declared" to "ITS pubqlang/22-derived". The single
**discrepancy** is `%` modulo — accepted by the parser, not
ITS-supported. The clean-room rewrite preserves the local `%`
allowance and documents it as a local extension.

C2 provenance comments in `expressions.rs` cite the verified ITS
chapters (`pubqlang/22`, `pubqlang/40`, `pubqlang/60`) wherever
applicable rather than the placeholder `// local: ...` originally
proposed for sections that turned out to be ITS-derived.

## Out of scope

The following are **not** covered by this mini-spec; they will be
added in Slice 10b:

- predicate bodies: IN, IN HIERARCHY, IS NULL, BETWEEN, LIKE, REFS;
- column references and function call argument shape;
- CAST type specification (`ВЫРАЗИТЬ(... КАК ...)`);
- CASE expression body (WHEN / THEN / ELSE / END);
- inline tabular field syntax (`.(Field1, Field2, ...)`);
- specialised function vocabulary (aggregate, date, string, type
  helpers — Slice 4 owns the lexer-level vocabulary;
  Slice 10b owns the parser-level dispatch).

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
