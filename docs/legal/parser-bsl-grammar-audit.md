# Parser BSL Grammar Audit

## Scope

This note is a focused provenance assessment for the hand-written BSL grammar
layer in `crates/parser`:

- `crates/parser/src/grammar.rs`
- `crates/parser/src/grammar/items.rs`
- `crates/parser/src/grammar/statements.rs`
- `crates/parser/src/grammar/expressions.rs`

The comparison baseline is the sibling upstream grammar file:

- `../bsl-parser/src/main/antlr/BSLParser.g4`

## High-level conclusion

The BSL grammar layer looks **mixed**:

- the overall grammar skeleton still strongly resembles `BSLParser.g4`;
- the parser architecture and several important disambiguation/recovery choices
  are clearly local;
- compared to SDBL, this BSL layer looks somewhat **more transformed** and
  therefore somewhat **less risky**, but it is still not a clean permissive
  candidate today.

In short:

- **not a literal port**
- **not a clean-room grammar either**

## Structural similarity to `BSLParser.g4`

Several top-level rule families line up very closely with upstream ANTLR
boundaries:

- procedures / functions / params
- variable declarations
- `if` / `elsif` / `else`
- `while`
- `for`
- `for each`
- `try` / `except`
- `return`
- `raise`
- `execute`
- `add handler` / `remove handler`
- `goto` / labels
- `new`, ternary operator, await, calls, indexing, property access

This does not prove line-by-line copying, but it is the main reason the BSL
grammar layer should still be treated as provenance-sensitive.

## File-by-file assessment

### `items.rs`

Current assessment: **high risk, but with local restructuring**

Why it still tracks upstream:

- `procedure_def` / `function_def` match upstream `procedure` / `function`;
- `param_list` / `param` directly mirror `paramList` / `param`;
- `annotation`, `annotation_params`, compiler directives, and var declarations
  follow the same feature surface as the upstream grammar.

What looks local:

- no ANTLR-style split between `procDeclaration`, `funcDeclaration`,
  `subCodeBlock`, `moduleVar`, `subVar`;
- the hand-written parser collapses several upstream grammar layers into a
  smaller set of pragmatic functions;
- recovery is tooling-oriented, for example:
  - optional acceptance of keywords as names for error recovery,
  - unified `var_declaration_content`,
  - simplified handling of optional async/export/annotations.

Working conclusion:

- **grammar content still likely traces to upstream;**
- **function decomposition and recovery behavior are partly local.**

### `statements.rs`

Current assessment: **high risk, but materially more original than a direct grammar transcription**

Why it still tracks upstream:

- statement taxonomy maps almost one-to-one to `compoundStatement` and related
  rules in `BSLParser.g4`;
- the same control-flow statement families appear in roughly the same semantic
  order.

What looks local:

- `assignment_or_call` is a distinctly local disambiguation routine;
- `raise_stmt` explicitly supports both old-style and call-style
  `ВызватьИсключение`, including omitted arguments, in a way shaped for the
  current parser rather than copied grammar text;
- `stmt_list_inner` and semicolon handling are local convenience/recovery
  helpers;
- the whole file is written around practical parser progress and IDE robustness,
  not around ANTLR production mechanics.

Working conclusion:

- **statement families are still grammar-derived;**
- **but many implementation details here already look like local parser work.**

This file is a better future candidate for rewrite-preserving refactor than
`items.rs`, because some of the highest-value logic is clearly yours already.

### `expressions.rs`

Current assessment: **medium-high risk, with the strongest local transformation in the BSL grammar layer**

Why upstream influence is still visible:

- it covers the same surface as upstream `expression`, `member`, `modifier`,
  `newExpression`, `ternaryOperator`, `waitExpression`, `lValue`, and access
  rules;
- property access, indexing, calls, `new`, ternary, and await all line up with
  upstream grammar concepts.

What looks clearly local:

- the parser uses explicit precedence levels:
  - `or_expr`
  - `and_expr`
  - `comparison_expr`
  - `additive_expr`
  - `multiplicative_expr`
  - `unary_expr`
- upstream ANTLR grammar is flatter:
  `expression: member (operation member)*`
- `postfix_expr_with_call_info` is a local abstraction tailored to statement
  validation and assignment-vs-call disambiguation;
- multiline string handling and postfix validity checks are clearly adapted to
  current parser architecture rather than copied directly from grammar text.

Working conclusion:

- this file still depends on upstream grammar ideas;
- but among the BSL grammar files, it looks the **most transformed** and the
  **least like a direct grammar port**.

### `grammar.rs`

Current assessment: **high risk, but mainly as a glue layer over the other grammar files**

Why it is still provenance-sensitive:

- top-level source-file structure, annotated items, and preprocessor block
  families clearly match upstream language organization.

What looks local:

- it is strongly shaped by the event-parser architecture;
- it integrates module-level statements, annotations, and preprocessor regions
  pragmatically rather than mirroring ANTLR production structure exactly.

Working conclusion:

- this file is not the worst offender by itself;
- its risk mostly comes from being the top-level orchestrator of the still
  provenance-sensitive grammar layer.

## Relative risk inside BSL grammar

From most concerning to least concerning:

1. `items.rs`
2. `grammar.rs`
3. `statements.rs`
4. `expressions.rs`

This ordering is not about code quality. It is about how directly each file
still appears to track upstream grammar structure.

## Comparison with SDBL

The BSL grammar layer still has meaningful provenance risk, but it currently
looks **less severe than SDBL** for two reasons:

1. there are fewer explicit surviving upstream references in comments;
2. `expressions.rs` and `statements.rs` already contain more obvious local
   parser-specific transformation than the SDBL grammar files.

So the practical prioritization remains:

- first: SDBL grammar / lexer cleanup
- later: BSL grammar cleanup

## Licensing conclusion today

The BSL grammar layer should still remain in the **copyleft-risk bucket** for
now.

However, it should be treated as a **mixed** bucket, not as uniformly derived
code. There is already real local implementation value here, especially in:

- parser-oriented recovery behavior;
- assignment/call disambiguation;
- precedence-based expression parsing;
- tooling-friendly handling of multiline strings and semicolon tolerance.

## Practical next step

For relicensing work, the most useful next move is **not** a full BSL rewrite
yet. The better next move is:

1. finish SDBL parser/lexer cleanup first;
2. then return to BSL grammar with a narrower goal:
   - preserve parser architecture and recovery logic,
   - rewrite grammar-expression routines from primary language behavior rather
     than upstream grammar text.

## Bottom line

The BSL parser grammar is still too close to upstream structure to call it clean
today.

But it is also clearly more than a raw port: especially in `statements.rs` and
`expressions.rs`, the current implementation already contains nontrivial local
parser work worth preserving in any future clean-room rewrite.
