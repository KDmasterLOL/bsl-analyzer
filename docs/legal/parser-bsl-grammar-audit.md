# Parser BSL Grammar Audit

> **Baseline claim retracted, 2026-08-20.** This note declared
> `BSLParser.g4` its comparison baseline and reads as the result of comparing
> against it. That comparison never took place: like the rest of the April 2026
> notes in this directory, it was written without access to the upstream grammar
> files (`sdbl-provenance-2026-07-audit.md`, «Why this document exists»). Every
> statement below about what upstream *is* — the structural line-ups, the
> per-file risk verdicts, the risk ordering — is recollection and structural
> intuition, not a finding. One sentence that quoted an upstream rule in ANTLR
> notation has been removed for the same reason; it was not a citation of
> anything this note had read.
>
> What survives is the part about **our own** files, which the author could read:
> where the local parser collapses layers, where recovery logic is local, where
> precedence is explicit. Those are the starting hypotheses of Slice B3, not its
> conclusions. See `bsl-clean-room-slices.md` for the plan that replaces this
> note's «Practical next step».
>
> The premise that the BSL layer is provenance-sensitive is *separately*
> established, and not by this note: the initial commit `a6204f78` assigns named
> rules of the upstream grammar as work items in its planning document, and a
> header referencing `BSLLexer.g4` was added in `fe2f7ed2` and removed in
> `843b00ab`. `BSLParser.g4` was never named in a BSL parser file. This layer is
> the reason `parser` and `lexer` cannot become Tier A on SDBL progress alone.

## Scope

This note is a focused provenance assessment for the hand-written BSL grammar
layer in `crates/parser`:

- `crates/parser/src/grammar.rs`
- `crates/parser/src/grammar/items.rs`
- `crates/parser/src/grammar/statements.rs`
- `crates/parser/src/grammar/expressions.rs`

The intended comparison baseline was the sibling upstream grammar file
`../bsl-parser/src/main/antlr/BSLParser.g4`. It was never opened — see the
retraction above. No such checkout exists on the machine this note was written
on, and none is to be obtained for the replacement work.

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

This ordering is not about code quality, and — given the retraction above — it
is not evidence either. It records how directly each file *appeared* to track
upstream grammar structure to a reader who had the local files and not the
upstream one. Slice B3 assigns per-rule verdicts from the language specification
and does not inherit this ordering.

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

**Superseded by `bsl-clean-room-slices.md`.** That plan keeps this note's
conclusion — no full BSL rewrite — and makes it precise: the token inventory and
the preprocessor symbols are rewritten from Chapter 4 of the 8.3.27 Developer's
Guide (slices B1 and B2), the grammar rules are *attested* rather than rewritten
(slice B3), and the parser architecture and recovery logic are preserved
throughout.

## Bottom line

The BSL parser grammar is still too close to upstream structure to call it clean
today.

But it is also clearly more than a raw port: especially in `statements.rs` and
`expressions.rs`, the current implementation already contains nontrivial local
parser work worth preserving in any future clean-room rewrite.
