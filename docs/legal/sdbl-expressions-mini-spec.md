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

Authoritative sources for this mini-spec:

- 1C query language documentation:
  - `Синтаксис текста запросов`
  - `https://its.1c.ru/db/pubqlang/content/12/hdoc`
  - `Структура запроса`
  - `https://its.1c.ru/db/pubqlang/content/10/hdoc`

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

The expression parser must assume:

- the lexer is the Slice 1 + 2 clean-room reach defined in
  `crates/lexer/src/sdbl/mod.rs`;
- keyword acceptance is bilingual (`И` ↔ `AND`, `ИЛИ` ↔ `OR`,
  `НЕ` ↔ `NOT`, `ИСТИНА` ↔ `TRUE`, `ЛОЖЬ` ↔ `FALSE`,
  `NULL` ↔ `NULL`, `УНДЕФИНЕД` ↔ `UNDEFINED`, etc.);
- keyword matching is case-insensitive at the lexer-token level —
  the parser sees the canonicalised `TokenKind::KwAnd` /
  `TokenKind::KwOr` / `TokenKind::KwNot` / `TokenKind::KwTrue` /
  `TokenKind::KwFalse` / `TokenKind::KwNull` /
  `TokenKind::KwUndefined` regardless of source-text spelling;
- bilingual keywords for which no dedicated `TokenKind` exists
  (e.g. `CASE` / `ВЫБОР`, `WHEN` / `КОГДА`, `THEN` / `ТОГДА`,
  `ELSE` / `ИНАЧЕ`, `END` / `КОНЕЦ`, `BETWEEN` / `МЕЖДУ`,
  `LIKE` / `ПОДОБНО`, `IS` / `ЕСТЬ`, `IN` / `В`,
  `HIERARCHY` / `ИЕРАРХИИ`, `REFS` / `ССЫЛКА`, `ESCAPE` /
  `СПЕЦСИМВОЛ`) arrive as `TokenKind::Ident` and are recognised
  via `p.at_keyword("…")`;
- `+`, `−`, `×`, `÷`, `%`, `(`, `)`, `,`, `=`, `<>`, `<`, `<=`,
  `>`, `>=`, `&`, `*`, identifier, decimal, float, string, date are
  separate `TokenKind` variants;
- trivia (`whitespace`, `newlines`, `comments`) is preserved in the
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

This precedence ladder is **declared by this mini-spec**, not derived
from ITS pubqlang/12, and not derived from any third-party SQL
grammar. The operator inventory at each level is taken from ITS
pubqlang/12 §Operators. The binding strengths between levels are
chosen so that:

- `a OR b AND c` parses as `a OR (b AND c)`;
- `a AND b = c` parses as `a AND (b = c)`;
- `a + b * c` parses as `a + (b * c)`;
- `NOT a AND b` parses as `(NOT a) AND b` — `NOT` binds tighter than
  `AND` because `not_expr` is the `logicalAndExpression` operand.

The Slice 10b body inherits the `comparison/predicate` slot — that is
where `IN`, `BETWEEN`, `LIKE`, `IS NULL`, `REFS`, and the comparison
operator tail land.

## Atoms

`primaryExpression` dispatches by leading token:

```text
primaryExpression
  := caseExpression                         (CASE  / ВЫБОР keyword)
   | parenOrSubqueryExpression              (LPAREN)
   | columnOrFunctionCall                   (Ident, NOT a clause keyword)
   | parameterExpression                    (& Ident)
   | literalExpression                      (Decimal | Float | String | KwTrue | KwFalse | KwNull | KwUndefined)
   | starLiteral                            (Star — emits SdblLiteral)
   | error-fallback (SdblError)             (anything else)
```

### Literals

```text
literalExpression
  := numericLiteral                         (Decimal | Float)
   | stringLiteralOrMulti
   | booleanLiteral                         (KwTrue | KwFalse — bilingual)
   | nullLiteral                            (KwNull | KwUndefined)
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
`KwNull`, `KwUndefined`, non-keyword `Ident`, `Plus`, `Minus`,
`KwNot`, `Star`, `LParen`, `Ampersand`, `at_keyword("CASE")`,
`at_keyword("ВЫБОР")`. Reject set: every other `TokenKind`,
including all clause keywords gated by `select::is_clause_keyword`.

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
parameter-source FROM) has exactly two direct children: the
`Ampersand` token and the following `Ident` token, with no trivia
node between them.

### VT-arg children direct under SdblTableRef

The HIR consumer at `crates/sdbl-hir/src/lower/from_clause.rs:283-306`
filters direct children of `SdblTableRef` for VT-arg lowering. The
filter set includes `SDBL_LOGICAL_OR_EXPR`, `SDBL_LOGICAL_AND_EXPR`,
`SDBL_ADDITIVE_EXPR`, `SDBL_MULTIPLICATIVE_EXPR`, `SDBL_UNARY_EXPR`,
`SDBL_PARAMETER`, `SDBL_LITERAL`, `SDBL_TUPLE_EXPR`,
`SDBL_PAREN_EXPR`. The Slice 10a clean-room rewrite must keep
emitting these exact NodeKinds (no rename, no removal, no new
intermediate wrapping) at the VT-arg expression position.

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

## ITS coverage verification

This section is empty at the time of the mini-spec C0a commit. It
will be filled by the Slice 10a C2 author after fetching ITS
pubqlang/10 and /12 directly during the rewrite. The C2 author
records "verified yes / verified no / page changed" plus a quoted
excerpt for each of the rows below.

| Claim | Verified | Notes / quoted excerpt |
|---|---|---|
| ITS pubqlang/10 has no general "expression" production (only "logical expression" in clause contexts) | TODO at C2 |  |
| ITS pubqlang/12 lists the operator inventory (logical / arithmetic / comparison) | TODO at C2 |  |
| ITS pubqlang/12 documents operator precedence | TODO at C2 |  |
| ITS pubqlang/12 documents the bilingual logical operator names (`И` / `AND`, `ИЛИ` / `OR`, `НЕ` / `NOT`) | TODO at C2 |  |
| ITS pubqlang/12 documents the `&` parameter prefix lexeme | TODO at C2 |  |
| ITS pubqlang/12 documents the literal forms (numeric, string, boolean, NULL/UNDEFINED) | TODO at C2 |  |

If verification reveals that any "no" claim is actually documented
in ITS, the mini-spec is updated in the C2 commit and the
provenance comments in `expressions.rs` are upgraded from
`// local: …` to ITS citations. If verification confirms the "no"
claims, the `// local: …` comments stand and the mini-spec is the
authorial basis for the Slice 10a clean-room claim.

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
and §Secondary sources at the top of this document. The resulting
expression grammar shape is the natural expression of the ITS
operator inventory plus the project's own event-parser conventions
established in Slices 1, 2, 6, 7, and 8, and would converge
regardless of author. The claim made here is **independent
derivation from the sources above**, not textual novelty.
